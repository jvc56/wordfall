//! PLAN.md § Authentication, § API → Auth and account, § Integration tests →
//! "Other integration tests" (auth flows, account binding, sign out
//! everywhere) and "Sync integration tests" (email caps, re-registration,
//! auth limits).

mod common;

use std::sync::atomic::Ordering;
use std::time::Instant;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::{Client, PASSWORD, TestApp};
use serde_json::json;
use wordfall::auth::mail::MailKind;

fn reg(username: &str, email: &str) -> serde_json::Value {
    json!({ "username": username, "email": email, "password": PASSWORD })
}

async fn expire_codes(app: &TestApp, email: &str) {
    sqlx::query(
        "UPDATE email_confirmations SET expires_at = now() - interval '1 second',
                created_at = now() - interval '25 hours'
         WHERE user_id = (SELECT id FROM users WHERE email = $1)",
    )
    .bind(email)
    .execute(app.db())
    .await
    .unwrap();
}

// ---------------------------------------------------------------------------
// Register (PLAN.md § Authentication → Register)
// ---------------------------------------------------------------------------

#[sqlx::test]
async fn register_creates_user_preferences_and_bindings(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    let mut c = app.client();
    let r = c
        .post("/api/auth/register", reg("alice", "alice@example.com"))
        .await;
    assert_eq!(r.status, StatusCode::ACCEPTED);
    assert!(r.set_cookies().is_empty());
    let code = app.latest_code("alice@example.com").await;
    assert_eq!(code.len(), 43, "32 random bytes, base64url");

    // The first sync sequence is taken and stamped on the preferences row.
    let (sync_seq, prefs_seq): (i64, i64) = sqlx::query_as(
        "SELECT u.sync_seq, p.updated_seq FROM users u JOIN user_preferences p ON p.user_id = u.id
         WHERE u.username = 'alice'",
    )
    .fetch_one(app.db())
    .await
    .unwrap();
    assert_eq!((sync_seq, prefs_seq), (1, 1));
    let bindings: Vec<(String, i16, String, String)> = sqlx::query_as(
        "SELECT b.action::text, b.slot, b.kind::text, b.code FROM user_input_bindings b
         JOIN users u ON u.id = b.user_id WHERE u.username = 'alice' ORDER BY 1, 2",
    )
    .fetch_all(app.db())
    .await
    .unwrap();
    let expected = [
        ("previous", 0, "mouse_button", "middle"),
        ("previous", 1, "key", "Backspace"),
        ("show_next", 0, "mouse_button", "left"),
        ("show_next", 1, "key", "Space"),
        ("toggle_grade", 0, "mouse_button", "right"),
        ("toggle_grade", 1, "key", "KeyX"),
    ];
    let got: Vec<_> = bindings
        .iter()
        .map(|(a, s, k, c)| (a.as_str(), *s, k.as_str(), c.as_str()))
        .collect();
    assert_eq!(got, expected);
    // Only the SHA-256 of the code is stored, with a 24-hour expiry.
    let (hash_len, hours): (i32, f64) = sqlx::query_as(
        "SELECT octet_length(code_hash), extract(epoch FROM expires_at - created_at)::float8 / 3600
         FROM email_confirmations",
    )
    .fetch_one(app.db())
    .await
    .unwrap();
    assert_eq!(hash_len, 32);
    assert!((hours - 24.0).abs() < 0.01);
}

#[sqlx::test]
async fn register_reports_every_field_error_at_once(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    let mut c = app.client();
    let r = c
        .post(
            "/api/auth/register",
            json!({"username": "a!", "email": "nope", "password": "password"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let fields: Vec<String> = r.json()["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["field"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(fields, ["username", "email", "password"]);
    assert!(app.settled_mail_to("nope").await.is_empty());
}

#[sqlx::test]
async fn taken_username_is_a_field_error_and_sends_no_email(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    app.signed_in("alice").await;
    let mut c = app.client_from("198.51.100.7");
    let r = c
        .post("/api/auth/register", reg("ALICE", "someone@example.com"))
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.json()["errors"][0]["field"], "username");
    assert!(app.settled_mail_to("someone@example.com").await.is_empty());
}

#[sqlx::test]
async fn an_email_in_use_gets_the_same_response_and_a_notice(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    app.signed_in("alice").await;
    let mut c = app.client_from("198.51.100.7");
    let r = c
        .post("/api/auth/register", reg("mallory", "alice@example.com"))
        .await;
    assert_eq!(r.status, StatusCode::ACCEPTED);
    assert_eq!(r.json(), json!({}));
    let mails = app.mail_to("alice@example.com", 2).await;
    assert_eq!(mails.last().unwrap().kind, MailKind::AccountExists);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(app.db())
        .await
        .unwrap();
    assert_eq!(n, 1);
}

/// PLAN.md § Integration tests: "Re-registering an unconfirmed email re-sends
/// a code while the old one is valid and replaces the account once it has
/// expired, and its username is taken while the code is valid and free after".
#[sqlx::test]
async fn re_registering_an_unconfirmed_email(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    let mut c = app.client();
    c.post("/api/auth/register", reg("bob", "bob@example.com"))
        .await;
    let first = app.latest_code("bob@example.com").await;
    let old_id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE username = 'bob'")
        .fetch_one(app.db())
        .await
        .unwrap();

    // While the code is valid: the username is taken...
    let mut other = app.client_from("198.51.100.9");
    let r = other
        .post("/api/auth/register", reg("bob", "other@example.com"))
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.json()["errors"][0]["field"], "username");
    // ...and the same address gets a fresh code, both valid.
    let r = other
        .post("/api/auth/register", reg("robert", "bob@example.com"))
        .await;
    assert_eq!(r.status, StatusCode::ACCEPTED);
    let mails = app.mail_to("bob@example.com", 2).await;
    assert_eq!(mails.len(), 2);
    let second = app.latest_code("bob@example.com").await;
    assert_ne!(first, second);
    let same_id: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE email = 'bob@example.com'")
            .fetch_one(app.db())
            .await
            .unwrap();
    assert_eq!(same_id, old_id);
    let r = c
        .post("/api/auth/confirm-email", json!({ "code": first }))
        .await;
    assert_eq!(
        r.status,
        StatusCode::NO_CONTENT,
        "the first code still confirms"
    );

    // Once expired: the address is replaced by a fresh account.
    let mut c2 = app.client_from("198.51.100.10");
    c2.post("/api/auth/register", reg("carol", "carol@example.com"))
        .await;
    let carol_id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE username = 'carol'")
        .fetch_one(app.db())
        .await
        .unwrap();
    expire_codes(&app, "carol@example.com").await;
    let r = c2
        .post("/api/auth/register", reg("caroline", "carol@example.com"))
        .await;
    assert_eq!(r.status, StatusCode::ACCEPTED);
    let row: (uuid::Uuid, String) =
        sqlx::query_as("SELECT id, username::text FROM users WHERE email = 'carol@example.com'")
            .fetch_one(app.db())
            .await
            .unwrap();
    assert_ne!(row.0, carol_id);
    assert_eq!(row.1, "caroline");

    // And an expired holder's username is free.
    let mut c3 = app.client_from("198.51.100.11");
    c3.post("/api/auth/register", reg("dave", "dave@example.com"))
        .await;
    expire_codes(&app, "dave@example.com").await;
    let r = c3
        .post("/api/auth/register", reg("dave", "dave2@example.com"))
        .await;
    assert_eq!(r.status, StatusCode::ACCEPTED);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE email = 'dave@example.com'")
        .fetch_one(app.db())
        .await
        .unwrap();
    assert_eq!(n, 0, "the old row is deleted");
}

/// PLAN.md § Integration tests: the email caps — three per address per IP in
/// 24 hours, twenty per address in all.
#[sqlx::test]
async fn email_caps(pool: sqlx::PgPool) {
    let app = TestApp::with(pool, &[("AUTH_RATE_PER_IP_PER_MINUTE", "1000")]).await;
    app.signed_in("erin").await; // one confirmation email from 203.0.113.1
    let mut c = app.client(); // same IP
    for _ in 0..2 {
        c.post(
            "/api/auth/reset-password",
            json!({ "email": "erin@example.com" }),
        )
        .await;
    }
    assert_eq!(app.mail_to("erin@example.com", 3).await.len(), 3);
    // A fourth from the same IP: same response, nothing sent.
    let r = c
        .post(
            "/api/auth/reset-password",
            json!({ "email": "erin@example.com" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::ACCEPTED);
    assert_eq!(app.settled_mail_to("erin@example.com").await.len(), 3);
    // Another IP still sends.
    let mut other = app.client_from("198.51.100.1");
    other
        .post(
            "/api/auth/reset-password",
            json!({ "email": "erin@example.com" }),
        )
        .await;
    assert_eq!(app.mail_to("erin@example.com", 4).await.len(), 4);
    // Up to twenty in all, from fresh IPs; the twenty-first is not sent.
    for i in 0..20 {
        let mut x = app.client_from(&format!("192.0.2.{i}"));
        x.post(
            "/api/auth/reset-password",
            json!({ "email": "erin@example.com" }),
        )
        .await;
    }
    assert_eq!(app.settled_mail_to("erin@example.com").await.len(), 20);
}

/// "a replacement made under the cap still replaces the account and sends its
/// code with the next registration after the window".
#[sqlx::test]
async fn a_replacement_under_the_cap(pool: sqlx::PgPool) {
    let app = TestApp::with(pool, &[("AUTH_RATE_PER_IP_PER_MINUTE", "1000")]).await;
    let mut c = app.client();
    for name in ["fay", "fay2", "fay3"] {
        // Each expires before the next, so each registration replaces the last.
        c.post("/api/auth/register", reg(name, "fay@example.com"))
            .await;
        app.mail_to("fay@example.com", 1).await;
        expire_codes(&app, "fay@example.com").await;
    }
    assert_eq!(app.mail_to("fay@example.com", 3).await.len(), 3);
    let r = c
        .post("/api/auth/register", reg("fay4", "fay@example.com"))
        .await;
    assert_eq!(r.status, StatusCode::ACCEPTED);
    let name: String =
        sqlx::query_scalar("SELECT username::text FROM users WHERE email = 'fay@example.com'")
            .fetch_one(app.db())
            .await
            .unwrap();
    assert_eq!(name, "fay4", "replaced although capped");
    assert_eq!(
        app.settled_mail_to("fay@example.com").await.len(),
        3,
        "code not sent"
    );
    // After the window, the next registration sends a code for that account.
    app.state
        .clock
        .advance(chrono::Duration::hours(24) + chrono::Duration::seconds(1));
    let r = c
        .post("/api/auth/register", reg("fay5", "fay@example.com"))
        .await;
    assert_eq!(r.status, StatusCode::ACCEPTED);
    let mails = app.mail_to("fay@example.com", 4).await;
    assert_eq!(mails.len(), 4);
    let code = app.latest_code("fay@example.com").await;
    assert_eq!(
        c.post("/api/auth/confirm-email", json!({ "code": code }))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    let r = c
        .post(
            "/api/auth/login",
            json!({"username": "fay4", "password": PASSWORD}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
}

/// "The password is hashed on every path ... so the response takes the same
/// time whichever branch runs."
#[sqlx::test]
async fn registration_branches_take_the_same_time(pool: sqlx::PgPool) {
    let app = TestApp::with(pool, &[("AUTH_RATE_PER_IP_PER_MINUTE", "1000")]).await;
    app.signed_in("gil").await;
    let mut new_times = Vec::new();
    let mut used_times = Vec::new();
    for i in 0..6 {
        let mut c = app.client_from(&format!("192.0.2.{i}"));
        let t = Instant::now();
        let r = c
            .post(
                "/api/auth/register",
                reg(&format!("newbie{i}"), &format!("n{i}@example.com")),
            )
            .await;
        new_times.push(t.elapsed().as_secs_f64());
        assert_eq!(r.status, StatusCode::ACCEPTED);
        let t = Instant::now();
        let r = c
            .post(
                "/api/auth/register",
                reg(&format!("copycat{i}"), "gil@example.com"),
            )
            .await;
        used_times.push(t.elapsed().as_secs_f64());
        assert_eq!(r.status, StatusCode::ACCEPTED);
    }
    let median = |v: &mut Vec<f64>| {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v[v.len() / 2]
    };
    let (a, b) = (median(&mut new_times), median(&mut used_times));
    let ratio = a.max(b) / a.min(b);
    assert!(ratio < 1.6, "new {a:.4}s vs in-use {b:.4}s");
}

// ---------------------------------------------------------------------------
// Confirm, login, me, logout
// ---------------------------------------------------------------------------

#[sqlx::test]
async fn login_needs_a_confirmed_email(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    let mut c = app.client();
    c.post("/api/auth/register", reg("hal", "hal@example.com"))
        .await;
    let r = c
        .post(
            "/api/auth/login",
            json!({"username": "hal", "password": PASSWORD}),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert!(r.set_cookies().is_empty());
    let r = c
        .post("/api/auth/confirm-email", json!({ "code": "not-a-code" }))
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let code = app.latest_code("hal@example.com").await;
    assert_eq!(
        c.post("/api/auth/confirm-email", json!({ "code": code }))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    // Single use.
    assert_eq!(
        c.post("/api/auth/confirm-email", json!({ "code": code }))
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    let r = c
        .post(
            "/api/auth/login",
            json!({"username": "hal", "password": PASSWORD}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    // Login is by username, never by email.
    let mut d = app.client_from("198.51.100.2");
    let r = d
        .post(
            "/api/auth/login",
            json!({"username": "hal@example.com", "password": PASSWORD}),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn login_sets_session_and_csrf_cookies_with_one_ttl(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    let mut c = app.client();
    c.post("/api/auth/register", reg("ivy", "ivy@example.com"))
        .await;
    let code = app.latest_code("ivy@example.com").await;
    c.post("/api/auth/confirm-email", json!({ "code": code }))
        .await;
    let r = c
        .post(
            "/api/auth/login",
            json!({"username": "ivy", "password": PASSWORD}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    let body = r.json();
    assert_eq!(body["username"], "ivy");
    assert_eq!(body["is_admin"], false);
    assert_eq!(body["trash_retention_days"], 30);
    assert_eq!(body["max_quiz_questions"], 300000);
    let session = r.set_cookie("wordfall_session").unwrap();
    let csrf = r.set_cookie("wordfall_csrf").unwrap();
    assert!(session.contains("HttpOnly"));
    assert!(session.contains("SameSite=Lax"));
    assert!(
        !csrf.contains("HttpOnly"),
        "the CSRF cookie is readable by scripts"
    );
    assert!(session.contains("Max-Age=2592000"));
    assert!(
        csrf.contains("Max-Age=2592000"),
        "same TTL as the session cookie"
    );
    assert!(session.contains("wordfall_session=v4.local."));
}

#[sqlx::test]
async fn secure_cookies_in_tls_deployments(pool: sqlx::PgPool) {
    let app = TestApp::with(pool, &[("SECURE_COOKIES", "true")]).await;
    let mut c = app.client();
    c.post("/api/auth/register", reg("joe", "joe@example.com"))
        .await;
    let code = app.latest_code("joe@example.com").await;
    c.post("/api/auth/confirm-email", json!({ "code": code }))
        .await;
    let r = c
        .post(
            "/api/auth/login",
            json!({"username": "joe", "password": PASSWORD}),
        )
        .await;
    assert!(r.set_cookie("wordfall_session").unwrap().contains("Secure"));
    assert!(r.set_cookie("wordfall_csrf").unwrap().contains("Secure"));
}

/// "the CSRF cookie carrying the session cookie's TTL and being re-set by
/// `GET /api/auth/me`, so a client holding only the session cookie recovers a
/// usable token".
#[sqlx::test]
async fn me_re_sets_the_csrf_cookie(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    let mut c = app.signed_in("kim").await;
    c.cookies.remove("wordfall_csrf");
    let r = c.get("/api/auth/me").await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json()["username"], "kim");
    let csrf = r.set_cookie("wordfall_csrf").expect("fresh CSRF cookie");
    assert!(csrf.contains("Max-Age="));
    // The recovered token works for a write.
    let r = c
        .post(
            "/api/account/sign-out-everywhere",
            json!({ "password": PASSWORD }),
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
}

#[sqlx::test]
async fn me_is_exempt_from_the_account_binding(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    let alice = app.signed_in("alice").await;
    let mut bob = app.signed_in("bob").await;
    bob.user_id = alice.user_id; // the tab thinks it runs as Alice
    let r = bob.get("/api/auth/me").await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json()["username"], "bob", "the cookie's user");
    assert!(r.set_cookie("wordfall_csrf").is_some());
    let mut anon = app.client();
    assert_eq!(
        anon.get("/api/auth/me").await.status,
        StatusCode::UNAUTHORIZED
    );
}

/// PLAN.md § API → Auth and account, logout; § Integration tests.
#[sqlx::test]
async fn logout_rules(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    // No cookies at all: what a device offline past both TTLs sends.
    let mut none = app.client();
    let r = none.post_empty("/api/auth/logout").await;
    assert!(r.status.is_success(), "{}", r.status);

    // A session cookie and no CSRF token: refused like any other write.
    let mut c = app.signed_in("lee").await;
    c.send_csrf = false;
    let r = c.post_empty("/api/auth/logout").await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert!(c.cookies.contains_key("wordfall_session"));

    // The session cookie and a matching token, no X-Wordfall-User: the queued
    // logout. Both cookies cleared.
    c.send_csrf = true;
    c.send_user_header = false;
    let r = c.post_empty("/api/auth/logout").await;
    assert!(r.status.is_success());
    for name in ["wordfall_session", "wordfall_csrf"] {
        let sc = r.set_cookie(name).unwrap();
        assert!(sc.contains("Max-Age=0"), "{sc}");
    }
    assert!(c.cookies.is_empty());

    // A login after it re-sets both.
    let r = c
        .post(
            "/api/auth/login",
            json!({"username": "lee", "password": PASSWORD}),
        )
        .await;
    assert!(r.set_cookie("wordfall_session").is_some());
    assert!(r.set_cookie("wordfall_csrf").is_some());
}

/// "a `POST /api/auth/login` or `/api/auth/register` sent as `text/plain`
/// with a JSON-shaped body answered `415` with no cookie set".
#[sqlx::test]
async fn json_endpoints_refuse_other_content_types(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    app.signed_in("moe").await;
    for (path, body) in [
        (
            "/api/auth/login",
            json!({"username": "moe", "password": PASSWORD}),
        ),
        ("/api/auth/register", reg("newmo", "newmo@example.com")),
    ] {
        let req = Request::post(path)
            .header(header::CONTENT_TYPE, "text/plain")
            .body(Body::from(body.to_string()))
            .unwrap();
        let r = app.send(req).await;
        assert_eq!(r.status, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{path}");
        assert!(r.set_cookies().is_empty());
    }
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(app.db())
        .await
        .unwrap();
    assert_eq!(n, 1);
}

// ---------------------------------------------------------------------------
// Account binding (PLAN.md § Authentication → Security generally)
// ---------------------------------------------------------------------------

/// Bob's cookie with `X-Wordfall-User` naming Alice, or no header, is `401`
/// with nothing applied. (Sync, card pages, cascade creation and export
/// tokens get the same check in their phases; here the account endpoints.)
#[sqlx::test]
async fn account_binding(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    let alice = app.signed_in("alice").await;
    let mut bob = app.signed_in("bob").await;
    let bob_id = bob.user_id;

    bob.user_id = alice.user_id;
    let r = bob
        .post(
            "/api/account/sign-out-everywhere",
            json!({ "password": PASSWORD }),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    bob.send_user_header = false;
    let r = bob
        .delete("/api/account", json!({ "password": PASSWORD }))
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let generation: i32 =
        sqlx::query_scalar("SELECT session_generation FROM users WHERE username = 'bob'")
            .fetch_one(app.db())
            .await
            .unwrap();
    assert_eq!(generation, 0, "nothing applied");
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(app.db())
        .await
        .unwrap();
    assert_eq!(n, 2);

    // The same requests with Bob's id succeed.
    bob.send_user_header = true;
    bob.user_id = bob_id;
    let r = bob
        .post(
            "/api/account/sign-out-everywhere",
            json!({ "password": PASSWORD }),
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
}

// ---------------------------------------------------------------------------
// Sign out everywhere, change password, delete account
// ---------------------------------------------------------------------------

#[sqlx::test]
async fn sign_out_everywhere(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    let mut here = app.signed_in("nat").await;
    let mut there = app.client_from("198.51.100.3");
    there.user_id = here.user_id;
    let r = there
        .post(
            "/api/auth/login",
            json!({"username": "nat", "password": PASSWORD}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);

    // A wrong password is refused and nothing changes.
    let r = here
        .post(
            "/api/account/sign-out-everywhere",
            json!({ "password": "wrong-password" }),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(there.get("/api/auth/me").await.status, StatusCode::OK);

    let old_csrf = here.csrf().cloned();
    let r = here
        .post(
            "/api/account/sign-out-everywhere",
            json!({ "password": PASSWORD }),
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert!(r.set_cookie("wordfall_session").is_some());
    assert!(r.set_cookie("wordfall_csrf").is_some());
    assert_ne!(here.csrf().cloned(), old_csrf, "a fresh CSRF token");
    assert_eq!(
        there.get("/api/auth/me").await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(here.get("/api/auth/me").await.status, StatusCode::OK);
    let r = here
        .post(
            "/api/account/sign-out-everywhere",
            json!({ "password": PASSWORD }),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::NO_CONTENT,
        "the session that pressed it keeps working"
    );
}

#[sqlx::test]
async fn change_password_reissues_this_session(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    let mut here = app.signed_in("ozzy").await;
    let mut there = app.client_from("198.51.100.4");
    there.user_id = here.user_id;
    there
        .post(
            "/api/auth/login",
            json!({"username": "ozzy", "password": PASSWORD}),
        )
        .await;
    let r = here
        .post(
            "/api/account/password",
            json!({"current_password": "nope-nope", "new_password": "violet-harbor-lantern-quill"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.json()["errors"][0]["field"], "current_password");
    let r = here
        .post(
            "/api/account/password",
            json!({"current_password": PASSWORD, "new_password": "abc"}),
        )
        .await;
    assert_eq!(r.json()["errors"][0]["field"], "new_password");
    let r = here
        .post(
            "/api/account/password",
            json!({"current_password": PASSWORD, "new_password": "violet-harbor-lantern-quill"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(
        there.get("/api/auth/me").await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(here.get("/api/auth/me").await.status, StatusCode::OK);
    let mut fresh = app.client_from("198.51.100.5");
    let r = fresh
        .post(
            "/api/auth/login",
            json!({"username": "ozzy", "password": "violet-harbor-lantern-quill"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
}

#[sqlx::test]
async fn delete_account_cascades_and_clears_cookies(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    let mut c = app.signed_in("pat").await;
    let r = c
        .delete("/api/account", json!({ "password": "wrong-password" }))
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = c
        .delete("/api/account", json!({ "password": PASSWORD }))
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert!(
        r.set_cookie("wordfall_session")
            .unwrap()
            .contains("Max-Age=0")
    );
    for table in [
        "users",
        "user_preferences",
        "user_input_bindings",
        "email_confirmations",
    ] {
        let n: i64 =
            sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
                .fetch_one(app.db())
                .await
                .unwrap();
        assert_eq!(n, 0, "{table}");
    }
}

#[sqlx::test]
async fn an_expired_session_is_refused(pool: sqlx::PgPool) {
    let app = TestApp::with(pool, &[("SESSION_TTL_SECONDS", "60")]).await;
    let mut c = app.signed_in("quin").await;
    assert_eq!(c.get("/api/auth/me").await.status, StatusCode::OK);
    app.state.clock.advance(chrono::Duration::seconds(61));
    assert_eq!(c.get("/api/auth/me").await.status, StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------------------
// Password reset
// ---------------------------------------------------------------------------

#[sqlx::test]
async fn password_reset(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    let mut session = app.signed_in("rae").await;
    let mut c = app.client_from("198.51.100.6");

    // The same answer whether or not the address has an account, in the same time.
    let t = Instant::now();
    let known = c
        .post(
            "/api/auth/reset-password",
            json!({ "email": "rae@example.com" }),
        )
        .await;
    let known_t = t.elapsed();
    let t = Instant::now();
    let unknown = c
        .post(
            "/api/auth/reset-password",
            json!({ "email": "nobody@example.com" }),
        )
        .await;
    let unknown_t = t.elapsed();
    assert_eq!(
        (known.status, known.body.clone()),
        (unknown.status, unknown.body.clone())
    );
    assert_eq!(known.status, StatusCode::ACCEPTED);
    let slower = known_t.max(unknown_t).as_secs_f64();
    assert!(
        slower < 0.05,
        "the lookup and email happen after the response ({slower}s)"
    );

    let mails = app.mail_to("rae@example.com", 2).await;
    let token = mails
        .iter()
        .find_map(|m| match &m.kind {
            MailKind::PasswordReset { token } => Some(token.clone()),
            _ => None,
        })
        .unwrap();
    assert!(app.settled_mail_to("nobody@example.com").await.is_empty());
    // A second outstanding token.
    c.post(
        "/api/auth/reset-password",
        json!({ "email": "rae@example.com" }),
    )
    .await;
    app.mail_to("rae@example.com", 3).await;

    let r = c
        .post(
            "/api/auth/reset-password/confirm",
            json!({"token": token, "password": "weak"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = c
        .post(
            "/api/auth/reset-password/confirm",
            json!({"token": token, "password": "amber-cobalt-meadow-sprocket"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    // Every other outstanding token is spent, and every session signed out.
    let unused: i64 =
        sqlx::query_scalar("SELECT count(*) FROM password_reset_tokens WHERE used_at IS NULL")
            .fetch_one(app.db())
            .await
            .unwrap();
    assert_eq!(unused, 0);
    assert_eq!(
        session.get("/api/auth/me").await.status,
        StatusCode::UNAUTHORIZED
    );
    let r = c
        .post(
            "/api/auth/reset-password/confirm",
            json!({"token": token, "password": "amber-cobalt-meadow-sprocket"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "single use");
    let r = c
        .post(
            "/api/auth/login",
            json!({"username": "rae", "password": "amber-cobalt-meadow-sprocket"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
}

#[sqlx::test]
async fn reset_tokens_last_thirty_minutes(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    app.signed_in("sal").await;
    let mut c = app.client();
    c.post(
        "/api/auth/reset-password",
        json!({ "email": "sal@example.com" }),
    )
    .await;
    let mails = app.mail_to("sal@example.com", 2).await;
    let token = mails.iter().find_map(|m| match &m.kind {
        MailKind::PasswordReset { token } => Some(token.clone()),
        _ => None,
    });
    let minutes: f64 = sqlx::query_scalar(
        "SELECT extract(epoch FROM expires_at - created_at)::float8 / 60 FROM password_reset_tokens",
    )
    .fetch_one(app.db())
    .await
    .unwrap();
    assert!((minutes - 30.0).abs() < 0.01);
    sqlx::query("UPDATE password_reset_tokens SET expires_at = now() - interval '1 second'")
        .execute(app.db())
        .await
        .unwrap();
    let r = c
        .post(
            "/api/auth/reset-password/confirm",
            json!({"token": token.unwrap(), "password": "amber-cobalt-meadow-sprocket"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

// ---------------------------------------------------------------------------
// Rate limits (PLAN.md § Integration tests → "The auth limits")
// ---------------------------------------------------------------------------

#[sqlx::test]
async fn login_failure_limits(pool: sqlx::PgPool) {
    let app = TestApp::with(pool, &[("AUTH_RATE_PER_IP_PER_MINUTE", "1000")]).await;
    // Twenty successful logins for twenty accounts from one address all succeed.
    for i in 0..20 {
        app.signed_in(&format!("user{i:02}")).await; // each logs in from 203.0.113.1
    }
    let mut room = app.client();
    for i in 0..20 {
        let r = room
            .post(
                "/api/auth/login",
                json!({"username": format!("user{i:02}"), "password": PASSWORD}),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK);
    }
    // Ten failures from one address, spread over usernames...
    for i in 0..10 {
        let r = room
            .post(
                "/api/auth/login",
                json!({"username": format!("user{i:02}"), "password": "bad-guess"}),
            )
            .await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    }
    // ...and the eleventh is refused before any Argon2 verify runs.
    let before = app.state.argon2_verifies.load(Ordering::Relaxed);
    let r = room
        .post(
            "/api/auth/login",
            json!({"username": "user15", "password": "bad-guess"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(r.retry_after().is_some());
    assert_eq!(app.state.argon2_verifies.load(Ordering::Relaxed), before);
    // A login for another username from a second address still succeeds.
    let mut elsewhere = app.client_from("198.51.100.20");
    let r = elsewhere
        .post(
            "/api/auth/login",
            json!({"username": "user19", "password": PASSWORD}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);

    // The eleventh failure for one username is refused from any address.
    for i in 0..10 {
        let mut x = app.client_from(&format!("192.0.2.{i}"));
        let r = x
            .post(
                "/api/auth/login",
                json!({"username": "user19", "password": "bad-guess"}),
            )
            .await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    }
    let before = app.state.argon2_verifies.load(Ordering::Relaxed);
    let mut fresh = app.client_from("192.0.2.200");
    let r = fresh
        .post(
            "/api/auth/login",
            json!({"username": "USER19", "password": PASSWORD}),
        )
        .await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(app.state.argon2_verifies.load(Ordering::Relaxed), before);
}

#[sqlx::test]
async fn auth_endpoints_share_one_per_ip_bucket(pool: sqlx::PgPool) {
    let app = TestApp::with(pool, &[("AUTH_RATE_PER_IP_PER_MINUTE", "1")]).await;
    // Register and confirm from two addresses, so the setup spends no bucket
    // the assertions below use.
    let mut signed = app.client_from("192.0.2.50");
    signed
        .post("/api/auth/register", reg("tess", "tess@example.com"))
        .await;
    let code = app.latest_code("tess@example.com").await;
    let mut confirmer = app.client_from("192.0.2.51");
    assert_eq!(
        confirmer
            .post("/api/auth/confirm-email", json!({ "code": code }))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        signed
            .post(
                "/api/auth/login",
                json!({"username": "tess", "password": PASSWORD})
            )
            .await
            .status,
        StatusCode::OK
    );
    let calls: [(&str, serde_json::Value); 4] = [
        ("/api/auth/register", reg("uma", "uma@example.com")),
        ("/api/auth/confirm-email", json!({ "code": "x" })),
        (
            "/api/auth/reset-password",
            json!({ "email": "x@example.com" }),
        ),
        (
            "/api/auth/reset-password/confirm",
            json!({ "token": "x", "password": PASSWORD }),
        ),
    ];
    for (i, (path, body)) in calls.iter().enumerate() {
        let mut c = app.client_from(&format!("198.51.100.{}", 100 + i));
        let first = c.post(path, body.clone()).await;
        assert_ne!(first.status, StatusCode::TOO_MANY_REQUESTS, "{path}");
        let second = c.post(path, body.clone()).await;
        assert_eq!(second.status, StatusCode::TOO_MANY_REQUESTS, "{path}");
        assert!(second.retry_after().is_some());
    }
    // `GET /api/auth/me` and logout from that address are not limited.
    for _ in 0..3 {
        assert_eq!(signed.get("/api/auth/me").await.status, StatusCode::OK);
    }
    for _ in 0..3 {
        let mut c = app.client();
        assert!(c.post_empty("/api/auth/logout").await.status.is_success());
    }
}

// ---------------------------------------------------------------------------
// Purge task: stale unconfirmed accounts
// ---------------------------------------------------------------------------

#[sqlx::test]
async fn purge_deletes_stale_unconfirmed_accounts(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    let mut c = app.client();
    c.post("/api/auth/register", reg("vic", "vic@example.com"))
        .await;
    c.post("/api/auth/register", reg("wes", "wes@example.com"))
        .await;
    app.signed_in("xan").await;
    // vic's code expired 8 days ago, wes's 6 days ago.
    sqlx::query(
        "UPDATE email_confirmations c SET expires_at = now() - interval '8 days'
         FROM users u WHERE u.id = c.user_id AND u.username = 'vic'",
    )
    .execute(app.db())
    .await
    .unwrap();
    sqlx::query(
        "UPDATE email_confirmations c SET expires_at = now() - interval '6 days'
         FROM users u WHERE u.id = c.user_id AND u.username = 'wes'",
    )
    .execute(app.db())
    .await
    .unwrap();
    assert!(wordfall::purge::run_once(&app.state).await.unwrap());
    let names: Vec<String> = sqlx::query_scalar("SELECT username::text FROM users ORDER BY 1")
        .fetch_all(app.db())
        .await
        .unwrap();
    assert_eq!(names, ["wes", "xan"]);
}

#[allow(dead_code)]
fn _unused(_: Client) {}
