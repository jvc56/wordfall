//! Shared helpers for integration tests: the real router over the real schema.
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::atomic::Ordering;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use tower::ServiceExt;
use wordfall::config::Config;
use wordfall::AppState;

pub struct TestApp {
    pub state: AppState,
    pub router: Router,
}

pub struct TestResponse {
    pub status: StatusCode,
    pub headers: axum::http::HeaderMap,
    pub body: bytes::Bytes,
}

impl TestResponse {
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).unwrap_or(serde_json::Value::Null)
    }
}

pub fn test_config(overrides: &[(&str, &str)]) -> Config {
    let mut env: HashMap<String, String> = HashMap::from([
        ("DATABASE_URL".into(), "postgres://unused".into()),
        ("SESSION_SIGNING_KEY".into(), "11".repeat(32)),
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

    pub async fn with_config(pool: sqlx::PgPool, config: Config) -> Self {
        let app = Self::new_unready_with(pool, config).await;
        app.state.catalog_ready.store(true, Ordering::Release);
        app
    }

    pub async fn new_unready(pool: sqlx::PgPool) -> Self {
        Self::new_unready_with(pool, test_config(&[])).await
    }

    async fn new_unready_with(pool: sqlx::PgPool, config: Config) -> Self {
        let state = AppState::new(pool, config);
        let router = wordfall::build_router(state.clone());
        TestApp { state, router }
    }

    pub async fn send(&self, req: Request<Body>) -> TestResponse {
        let resp = self.router.clone().oneshot(req).await.expect("router");
        let status = resp.status();
        let headers = resp.headers().clone();
        let body = resp.into_body().collect().await.expect("body").to_bytes();
        TestResponse { status, headers, body }
    }

    pub async fn get(&self, path: &str) -> TestResponse {
        self.send(Request::get(path).body(Body::empty()).unwrap()).await
    }
}
