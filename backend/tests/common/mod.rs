//! Shared helpers for integration tests: the real router in-process over the
//! real schema (PLAN.md § Integration tests). `#[sqlx::test]` gives each test
//! its own database on the server `make test-integration` starts.
#![allow(dead_code)]

pub mod sync;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;
use wordfall::AppState;
use wordfall::auth::mail::{MailKind, RecordingMailer};
use wordfall::config::Config;

pub const PASSWORD: &str = "correct-tile-rack-bingo";

pub struct TestApp {
    pub state: AppState,
    pub router: Router,
    pub mail: RecordingMailer,
}

#[derive(Debug)]
pub struct TestResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: bytes::Bytes,
}

impl TestResponse {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }

    pub fn set_cookies(&self) -> Vec<String> {
        self.headers
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().unwrap().to_owned())
            .collect()
    }

    pub fn set_cookie(&self, name: &str) -> Option<String> {
        self.set_cookies()
            .into_iter()
            .find(|c| c.starts_with(&format!("{name}=")))
    }

    pub fn retry_after(&self) -> Option<u64> {
        self.headers
            .get(header::RETRY_AFTER)?
            .to_str()
            .ok()?
            .parse()
            .ok()
    }
}

pub fn test_config(overrides: &[(&str, &str)]) -> Config {
    let mut env: HashMap<String, String> = HashMap::from([
        ("DATABASE_URL".into(), "postgres://unused".into()),
        ("SESSION_SIGNING_KEY".into(), "11".repeat(32)),
        ("TRUSTED_PROXY_HOPS".into(), "1".into()),
    ]);
    for (k, v) in overrides {
        env.insert((*k).into(), (*v).into());
    }
    Config::from_map(&env).expect("test config")
}

impl TestApp {
    pub async fn new(pool: sqlx::PgPool) -> Self {
        Self::with_config(pool, test_config(&[])).await
    }

    pub async fn with(pool: sqlx::PgPool, overrides: &[(&str, &str)]) -> Self {
        Self::with_config(pool, test_config(overrides)).await
    }

    /// A running instance: its startup catalog load done and its background
    /// catalog tasks (LISTEN, reconcile, heartbeat) running.
    pub async fn with_config(pool: sqlx::PgPool, config: Config) -> Self {
        let app = Self::new_unready_with(pool, config).await;
        wordfall::catalog::startup(&app.state)
            .await
            .expect("catalog startup");
        assert!(app.state.catalog_ready.load(Ordering::Acquire));
        app
    }

    /// Reconciles this instance's catalog with the database now.
    pub async fn reconcile(&self) {
        wordfall::catalog::reconcile(&self.state)
            .await
            .expect("reconcile");
    }

    /// Registers `username` as a confirmed admin and uploads the committed
    /// fixture catalog through the admin API, as `stack.seed` does.
    pub async fn seed_fixture_catalog(&self, username: &str) -> Client<'_> {
        let mut admin = self.signed_in(username).await;
        self.make_admin(username).await;
        for item in fixture_manifest() {
            let bytes = std::fs::read(fixture_path(&item.file)).unwrap();
            let r = match item.kind.as_str() {
                "distribution" => {
                    admin
                        .upload(
                            "/api/admin/letter-distributions",
                            &[("name", &item.name)],
                            &item.file,
                            &bytes,
                        )
                        .await
                }
                "lexicon" => {
                    admin
                        .upload(
                            "/api/admin/lexicons",
                            &[
                                ("name", &item.name),
                                ("letter_distribution", item.parent.as_deref().unwrap()),
                            ],
                            &item.file,
                            &bytes,
                        )
                        .await
                }
                _ => {
                    admin
                        .upload(
                            "/api/admin/leave-sets",
                            &[("lexicon", &item.name)],
                            &item.file,
                            &bytes,
                        )
                        .await
                }
            };
            assert_eq!(
                r.status,
                StatusCode::CREATED,
                "{} {}: {:?}",
                item.kind,
                item.name,
                r.json()
            );
        }
        self.reconcile().await;
        admin
    }
}

#[derive(serde::Deserialize, Clone)]
pub struct ManifestItem {
    pub kind: String,
    pub name: String,
    pub file: String,
    pub parent: Option<String>,
}

pub fn fixture_path(file: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/catalog")
        .join(file)
}

pub fn fixture_manifest() -> Vec<ManifestItem> {
    serde_json::from_slice(&std::fs::read(fixture_path("manifest.json")).unwrap()).unwrap()
}

impl TestApp {
    pub async fn new_unready(pool: sqlx::PgPool) -> Self {
        Self::new_unready_with(pool, test_config(&[])).await
    }

    async fn new_unready_with(pool: sqlx::PgPool, config: Config) -> Self {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "warn".into()),
            )
            .with_test_writer()
            .try_init();
        let mail = RecordingMailer::default();
        let state = AppState::new(pool, config, Arc::new(mail.clone()));
        let router = wordfall::build_router(state.clone());
        TestApp {
            state,
            router,
            mail,
        }
    }

    pub fn db(&self) -> &sqlx::PgPool {
        &self.state.db
    }

    pub async fn send(&self, mut req: Request<Body>) -> TestResponse {
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 40000))));
        let resp = self.router.clone().oneshot(req).await.expect("router");
        let status = resp.status();
        let headers = resp.headers().clone();
        let body = resp.into_body().collect().await.expect("body").to_bytes();
        TestResponse {
            status,
            headers,
            body,
        }
    }

    pub async fn get(&self, path: &str) -> TestResponse {
        self.send(Request::get(path).body(Body::empty()).unwrap())
            .await
    }

    /// A GET whose body is counted as it streams and never held, so a test can
    /// measure what serving it costs the process (the scale test's exports).
    pub fn stream_len(&self, path: &str) -> impl Future<Output = (StatusCode, usize)> + Send + 'static {
        let router = self.router.clone();
        let mut req = Request::get(path).body(Body::empty()).unwrap();
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 40000))));
        async move {
            let resp = router.oneshot(req).await.expect("router");
            let status = resp.status();
            let mut body = resp.into_body();
            let mut n = 0;
            while let Some(frame) = body.frame().await {
                if let Ok(data) = frame.expect("body").into_data() {
                    n += data.len();
                }
            }
            (status, n)
        }
    }

    /// A fresh device with no cookies, from 203.0.113.1.
    pub fn client(&self) -> Client<'_> {
        Client::new(self, "203.0.113.1")
    }

    pub fn client_from(&self, ip: &str) -> Client<'_> {
        Client::new(self, ip)
    }

    /// Waits for queued mail (sent after the response) to arrive.
    pub async fn mail_to(
        &self,
        address: &str,
        at_least: usize,
    ) -> Vec<wordfall::auth::mail::Email> {
        for _ in 0..200 {
            let got = self.mail.to(address);
            if got.len() >= at_least {
                return got;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        self.mail.to(address)
    }

    /// Mail that should not arrive: waits a moment and returns what did.
    pub async fn settled_mail_to(&self, address: &str) -> Vec<wordfall::auth::mail::Email> {
        tokio::time::sleep(Duration::from_millis(150)).await;
        self.mail.to(address)
    }

    pub async fn latest_code(&self, address: &str) -> String {
        let mails = self.mail_to(address, 1).await;
        mails
            .iter()
            .rev()
            .find_map(|m| match &m.kind {
                MailKind::Confirmation { code } => Some(code.clone()),
                _ => None,
            })
            .expect("a confirmation code")
    }

    /// Registers, confirms and logs in a user through the real endpoints.
    pub async fn signed_in(&self, username: &str) -> Client<'_> {
        let mut c = self.client();
        let email = format!("{username}@example.com");
        let r = c
            .post(
                "/api/auth/register",
                json!({"username": username, "email": email, "password": PASSWORD}),
            )
            .await;
        assert_eq!(r.status, StatusCode::ACCEPTED, "{:?}", r.json());
        let code = self.latest_code(&email).await;
        let r = c
            .post("/api/auth/confirm-email", json!({ "code": code }))
            .await;
        assert_eq!(r.status, StatusCode::NO_CONTENT);
        let r = c
            .post(
                "/api/auth/login",
                json!({"username": username, "password": PASSWORD}),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.json());
        c
    }

    pub async fn make_admin(&self, username: &str) {
        sqlx::query("UPDATE users SET is_admin = true WHERE username = $1")
            .bind(username)
            .execute(self.db())
            .await
            .unwrap();
    }
}

/// One simulated device: its cookie jar, the account its tab runs as, and
/// its address (sent as `X-Forwarded-For`, read with `TRUSTED_PROXY_HOPS=1`).
pub struct Client<'a> {
    pub app: &'a TestApp,
    pub cookies: HashMap<String, String>,
    pub user_id: Option<Uuid>,
    pub ip: String,
    pub send_user_header: bool,
    pub send_csrf: bool,
}

impl<'a> Client<'a> {
    pub fn new(app: &'a TestApp, ip: &str) -> Self {
        Client {
            app,
            cookies: HashMap::new(),
            user_id: None,
            ip: ip.into(),
            send_user_header: true,
            send_csrf: true,
        }
    }

    pub fn csrf(&self) -> Option<&String> {
        self.cookies.get("wordfall_csrf")
    }

    pub async fn request(
        &mut self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> TestResponse {
        let mut b = Request::builder()
            .method(method)
            .uri(path)
            .header("x-forwarded-for", &self.ip);
        if !self.cookies.is_empty() {
            let cookie = self
                .cookies
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("; ");
            b = b.header(header::COOKIE, cookie);
        }
        if self.send_csrf {
            if let Some(t) = self.csrf() {
                b = b.header("x-csrf-token", t.clone());
            }
        }
        if self.send_user_header {
            if let Some(u) = self.user_id {
                b = b.header("x-wordfall-user", u.to_string());
            }
        }
        let req = match body {
            Some(v) => b
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&v).unwrap()))
                .unwrap(),
            None => b.body(Body::empty()).unwrap(),
        };
        let resp = self.app.send(req).await;
        self.absorb(&resp);
        resp
    }

    /// Keeps cookies as a browser would.
    pub fn absorb(&mut self, resp: &TestResponse) {
        for sc in resp.set_cookies() {
            let (pair, attrs) = sc.split_once(';').unwrap_or((&sc, ""));
            let (name, value) = pair.split_once('=').unwrap();
            if value.is_empty() || attrs.to_ascii_lowercase().contains("max-age=0") {
                self.cookies.remove(name);
            } else {
                self.cookies.insert(name.into(), value.into());
            }
        }
        if resp.status == StatusCode::OK {
            if let Some(id) = resp.json().get("user_id").and_then(|v| v.as_str()) {
                if self.user_id.is_none() {
                    self.user_id = id.parse().ok();
                }
            }
        }
    }

    pub async fn get(&mut self, path: &str) -> TestResponse {
        self.request(Method::GET, path, None).await
    }

    pub async fn post(&mut self, path: &str, body: Value) -> TestResponse {
        self.request(Method::POST, path, Some(body)).await
    }

    pub async fn post_empty(&mut self, path: &str) -> TestResponse {
        self.request(Method::POST, path, None).await
    }

    pub async fn delete(&mut self, path: &str, body: Value) -> TestResponse {
        self.request(Method::DELETE, path, Some(body)).await
    }

    pub async fn delete_empty(&mut self, path: &str) -> TestResponse {
        self.request(Method::DELETE, path, None).await
    }

    /// A multipart admin upload: form fields plus one `file`.
    pub async fn upload(
        &mut self,
        path: &str,
        fields: &[(&str, &str)],
        filename: &str,
        file: &[u8],
    ) -> TestResponse {
        let boundary = "wordfall-test-boundary-7d1a";
        let mut body = Vec::new();
        for (k, v) in fields {
            body.extend_from_slice(
                format!(
                    "--{boundary}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n"
                )
                .as_bytes(),
            );
        }
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\n\
                 Content-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(file);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let mut b = Request::builder()
            .method(Method::POST)
            .uri(path)
            .header("x-forwarded-for", &self.ip)
            .header(
                header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            );
        if !self.cookies.is_empty() {
            let cookie = self
                .cookies
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("; ");
            b = b.header(header::COOKIE, cookie);
        }
        if let Some(t) = self.csrf() {
            b = b.header("x-csrf-token", t.clone());
        }
        if let Some(u) = self.user_id {
            b = b.header("x-wordfall-user", u.to_string());
        }
        let resp = self.app.send(b.body(Body::from(body)).unwrap()).await;
        self.absorb(&resp);
        resp
    }
}
