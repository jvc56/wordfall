//! PLAN.md § API → Catalog and search (`POST /api/search/preview`), § Search
//! Engine (`SEARCH_CONCURRENCY`, `SEARCH_TIMEOUT_MS`), § Integration tests →
//! "Search concurrency", the search rate limit and account binding.

mod common;

use axum::http::StatusCode;
use common::TestApp;
use serde_json::{json, Value};

fn preview_body(lexicon: &str, quiz_type: &str, filters: Value) -> Value {
    json!({ "lexicon": lexicon, "quiz_type": quiz_type, "filters": filters })
}

fn length(min: i64, max: i64) -> Value {
    json!({ "type": "length", "negated": false, "min": min, "max": max })
}

#[sqlx::test]
async fn preview_counts_and_samples(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    let mut c = app.seed_fixture_catalog("root").await;
    let r = c
        .post(
            "/api/search/preview",
            preview_body("EN-FIX", "anagram", json!({ "op": "and", "children": [length(7, 7)] })),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.json());
    let b = r.json();
    // 26 sevens, of which the nine AEINRST anagrams share one alphagram.
    assert_eq!(b["count"], 18);
    assert_eq!(b["over_cap"], false);
    assert_eq!(b["sample"].as_array().unwrap().len(), 18);
    assert!(b["sample"].as_array().unwrap().contains(&json!("AEINRST")));

    // A Catalan Definition search returns keys in MAGPIE notation.
    let r = c
        .post(
            "/api/search/preview",
            preview_body("CA-FIX", "definition", json!({ "op": "and", "children": [
                { "type": "includes_letters", "negated": false, "tiles": "[NY]" } ] })),
        )
        .await;
    let sample = r.json()["sample"].clone();
    assert!(sample.as_array().unwrap().contains(&json!("A[NY]S")), "{sample}");

    // Leave Value previews run over the leave values.
    let r = c
        .post(
            "/api/search/preview",
            preview_body("EN-FIX", "leave_value", json!({ "op": "and", "children": [
                { "type": "leave_value", "negated": false, "min": 30.0, "max": null } ] })),
        )
        .await;
    assert_eq!(r.json()["sample"], json!(["??", "?AEINS", "?EIRS", "?S"]));

    // At most 20 questions in the sample; over the cap is flagged, not truncated.
    let app2 = TestApp::with(app.db().clone(), &[("MAX_QUIZ_QUESTIONS", "10")]).await;
    let mut c2 = app2.signed_in("second").await;
    let r = c2
        .post(
            "/api/search/preview",
            preview_body("EN-FIX", "definition", json!({ "op": "and", "children": [length(2, 15)] })),
        )
        .await;
    let b = r.json();
    assert_eq!(b["sample"].as_array().unwrap().len(), 20);
    assert_eq!(b["over_cap"], true);
    assert!(b["count"].as_u64().unwrap() > 20);
}

#[sqlx::test]
async fn preview_errors_are_keyed_by_path(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    let mut c = app.seed_fixture_catalog("root").await;
    let r = c
        .post(
            "/api/search/preview",
            preview_body("EN-FIX", "anagram", json!({ "op": "and", "children": [
                length(1, 15),
                { "op": "or", "children": [
                    { "type": "definition", "negated": false, "text": "x", "extra": 1 } ] } ] })),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let e = r.json()["errors"].clone();
    assert_eq!(e[0]["path"], json!([1, 0]));
    assert_eq!(e[0]["field"], "extra");
    let r = c
        .post("/api/search/preview", preview_body("EN-FIX", "anagram", json!({ "op": "and", "children": [length(1, 15)] })))
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.json()["errors"][0]["path"], json!([0]));
    let r = c
        .post("/api/search/preview", preview_body("NOPE", "anagram", json!({ "op": "and", "children": [length(2, 3)] })))
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.json()["errors"][0]["field"], "lexicon");
    let r = c
        .post("/api/search/preview", preview_body("EN-FIX-OLD", "leave_value", json!({ "op": "and", "children": [length(2, 3)] })))
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "a lexicon without leave values");
}

/// "Search concurrency: with `SEARCH_CONCURRENCY=1` and a short
/// `SEARCH_TIMEOUT_MS`, a second concurrent search is answered `503` with
/// `Retry-After` and never `422`, while a search admitted on an idle instance
/// returns its results."
#[sqlx::test]
async fn a_search_waiting_for_a_permit_gets_503(pool: sqlx::PgPool) {
    let app = TestApp::with(pool, &[("SEARCH_CONCURRENCY", "1"), ("SEARCH_TIMEOUT_MS", "200")]).await;
    let mut c = app.seed_fixture_catalog("root").await;
    let body = preview_body("EN-FIX", "definition", json!({ "op": "and", "children": [length(2, 3)] }));
    // Idle: admitted and answered.
    assert_eq!(c.post("/api/search/preview", body.clone()).await.status, StatusCode::OK);
    // A search holding the only permit, as a long-running one would.
    let held = app.state.search_permits.clone().acquire_owned().await.unwrap();
    let r = c.post("/api/search/preview", body.clone()).await;
    assert_eq!(r.status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(r.retry_after().is_some());
    drop(held);
    assert_eq!(c.post("/api/search/preview", body).await.status, StatusCode::OK);
}

#[sqlx::test]
async fn the_search_bucket_limits_preview(pool: sqlx::PgPool) {
    let app = TestApp::with(pool, &[("SEARCH_RATE_PER_MINUTE", "1")]).await;
    let mut c = app.seed_fixture_catalog("root").await;
    let body = preview_body("EN-FIX", "definition", json!({ "op": "and", "children": [length(2, 3)] }));
    assert_eq!(c.post("/api/search/preview", body.clone()).await.status, StatusCode::OK);
    let r = c.post("/api/search/preview", body).await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(r.retry_after().is_some());
    // Other buckets are untouched.
    assert_eq!(c.get("/api/auth/me").await.status, StatusCode::OK);
}

#[sqlx::test]
async fn preview_needs_the_account_binding(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    let mut c = app.seed_fixture_catalog("root").await;
    let body = preview_body("EN-FIX", "definition", json!({ "op": "and", "children": [length(2, 3)] }));
    c.send_user_header = false;
    assert_eq!(c.post("/api/search/preview", body.clone()).await.status, StatusCode::UNAUTHORIZED);
    let other = app.signed_in("other").await;
    c.send_user_header = true;
    let mine = c.user_id;
    c.user_id = other.user_id;
    assert_eq!(c.post("/api/search/preview", body.clone()).await.status, StatusCode::UNAUTHORIZED);
    c.user_id = mine;
    assert_eq!(c.post("/api/search/preview", body).await.status, StatusCode::OK);
}
