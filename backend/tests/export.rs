//! PLAN.md § Integration tests: "every export endpoint selection, for all three
//! quiz types, including an export of a quiz in the Trash and a 404 for another
//! user's cascade, each through a token from POST /api/cascades/:id/export-token;
//! the token request answering 401 for a mismatched X-Wordfall-User, 404 for a
//! purged quiz and 429 with Retry-After past EXPORT_RATE_PER_MINUTE; and the
//! download answering 204 for a token used twice, a token 61 seconds old, and a
//! token presented with choices other than those it was issued for; and a token
//! issued by one of two in-process instances redeemed on the other, then refused
//! on a second redemption on either …; and an export token presented as the
//! wordfall_session cookie, and a session token presented as an export token,
//! each refused"; and § Unit tests: the export endpoint returns the same bytes
//! the device would build from its own cards and grades.

mod common;

use axum::http::{header, StatusCode};
use chrono::Duration;
use common::sync::{play_and_finish, Device};
use common::{Client, TestApp, TestResponse};
use serde_json::{json, Value};
use uuid::Uuid;
use wordfall::leave::leave_value_text;

fn sevens() -> Value {
    json!({ "op": "and", "children": [{ "type": "length", "negated": false, "min": 7, "max": 7 }] })
}

fn filters(quiz_type: &str) -> Value {
    match quiz_type {
        "leave_value" => json!({ "op": "and", "children": [{ "type": "leave_value", "negated": false, "min": 30.0, "max": null }] }),
        _ => sevens(),
    }
}

async fn create(c: &mut Client<'_>, quiz_type: &str, name: &str) -> (String, String) {
    let r = c
        .post(
            "/api/cascades",
            json!({
                "id": Uuid::new_v4(), "source_quiz_id": Uuid::new_v4(), "device_id": Uuid::new_v4(),
                "at": "2026-01-01T00:00:00Z", "name": name, "lexicon": "EN-FIX", "quiz_type": quiz_type,
                "clear_threshold": 80, "segment_size": 0, "progression": "ladder", "require_alphabetical": false,
                "filters": filters(quiz_type),
            }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.json());
    (r.json()["cascade"]["id"].as_str().unwrap().to_owned(), r.json()["source_quiz"]["id"].as_str().unwrap().to_owned())
}

async fn token(c: &mut Client<'_>, cascade: &str, choices: Value) -> TestResponse {
    c.post(&format!("/api/cascades/{cascade}/export-token"), choices).await
}

/// A token, then the download, as the dialog's hidden frame makes it (no headers).
async fn export(app: &TestApp, c: &mut Client<'_>, cascade: &str, choices: Value) -> TestResponse {
    let r = token(c, cascade, choices).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.json());
    app.get(r.json()["url"].as_str().unwrap()).await
}

fn text(r: &TestResponse) -> String {
    String::from_utf8(r.body.to_vec()).unwrap()
}

/// Every card in idx order, as the device's `cards` store holds them.
async fn cards(c: &mut Client<'_>, cascade: &str, extras: &str) -> Vec<Value> {
    let r = c.get(&format!("/api/cascades/{cascade}/cards?from=0&limit=10000{extras}")).await;
    assert_eq!(r.status, StatusCode::OK);
    r.json().as_array().unwrap().clone()
}

async fn app(pool: sqlx::PgPool, extra: &[(&str, &str)]) -> TestApp {
    let mut o = vec![("EXPORT_RATE_PER_MINUTE", "1000"), ("SYNC_RATE_PER_MINUTE", "100000"), ("DOWNLOAD_RATE_PER_MINUTE", "1000")];
    o.extend_from_slice(extra);
    TestApp::with(pool, &o).await
}

#[sqlx::test]
async fn every_selection_for_all_three_types_matches_what_the_device_builds(pool: sqlx::PgPool) {
    let app = app(pool, &[]).await;
    let mut c = app.seed_fixture_catalog("root").await;
    for quiz_type in ["anagram", "definition", "leave_value"] {
        let (cascade, _) = create(&mut c, quiz_type, &format!("EN-FIX {quiz_type}")).await;
        let cards = cards(&mut c, &cascade, "&definitions=1").await;
        let keys: String = cards.iter().map(|k| format!("{}\n", k["key"].as_str().unwrap())).collect();
        // Questions, every selection: nothing is graded, so all and ungraded are everything.
        for which in ["all", "correct", "missed", "ungraded"] {
            let r = export(&app, &mut c, &cascade, json!({ "scope": "cascade", "which": which, "format": "txt", "lines": "questions", "order": "study", "decimals": 1 })).await;
            assert_eq!(r.status, StatusCode::OK);
            let want = if which == "all" || which == "ungraded" { keys.clone() } else { String::new() };
            assert_eq!(text(&r), want, "{quiz_type} {which}");
            let name = format!("EN-FIX {quiz_type}{}.txt", if which == "all" { String::new() } else { format!(" {which}") });
            let cd = r.headers.get(header::CONTENT_DISPOSITION).unwrap().to_str().unwrap().to_owned();
            assert_eq!(cd, format!("attachment; filename=\"{name}\""));
        }
        // Answers, as the device writes them from its cards.
        let want: String = cards
            .iter()
            .map(|k| match quiz_type {
                "anagram" => k["answer"].as_array().unwrap().iter().map(|w| format!("{}\n", w["word"].as_str().unwrap())).collect(),
                "definition" => format!("{}\n", k["answer"].as_str().unwrap()),
                _ => format!("{}\n", leave_value_text(k["answer"].as_f64().unwrap(), 2, false)),
            })
            .collect();
        let r = export(&app, &mut c, &cascade, json!({ "scope": "cascade", "which": "all", "format": "txt", "lines": "answers", "order": "study", "decimals": 2 })).await;
        assert_eq!(text(&r), want, "{quiz_type} answers");
        // A spreadsheet: header row, CRLF.
        let r = export(&app, &mut c, &cascade, json!({ "scope": "cascade", "which": "all", "format": "csv", "columns": ["question", "grade"], "order": "study", "decimals": 1 })).await;
        assert_eq!(r.headers.get(header::CONTENT_TYPE).unwrap(), "text/csv; charset=utf-8");
        let want = format!("question,grade\r\n{}", cards.iter().map(|k| format!("{},\r\n", k["key"].as_str().unwrap())).collect::<String>());
        assert_eq!(text(&r), want);
    }
}

#[sqlx::test]
async fn a_quiz_in_the_trash_exports_the_attempt_it_finished_on(pool: sqlx::PgPool) {
    let app = app(pool, &[]).await;
    let c = app.seed_fixture_catalog("root").await;
    let mut d = Device::new(c);
    let (cascade, source) = create(&mut d.client, "anagram", "Sevens").await;
    let (cascade, source): (Uuid, Uuid) = (cascade.parse().unwrap(), source.parse().unwrap());
    d.qrf = vec![cascade];
    d.sync(vec![]).await;
    let marks = "CCCCCCCCCCCCCCCCMM";
    let (r, l2) = play_and_finish(&app, &mut d, source, marks).await;
    assert_eq!(r["outcome"], "descended");
    let (r, _) = play_and_finish(&app, &mut d, l2, "CC").await;
    assert_eq!(r["outcome"], "cleared", "{r}");
    // Level 2 is in the Trash (cleared): its export is the attempt it finished on.
    let r = export(
        &app,
        &mut d.client,
        &cascade.to_string(),
        json!({ "scope": "quiz", "quiz_id": l2, "which": "all", "format": "csv", "columns": ["grade"], "order": "study", "decimals": 1 }),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(text(&r), "grade\r\ncorrect\r\ncorrect\r\n");
    let cd = r.headers.get(header::CONTENT_DISPOSITION).unwrap().to_str().unwrap().to_owned();
    assert_eq!(cd, "attachment; filename=\"Sevens - L2.csv\"");
    // The cascade-wide selection ignores the cleared quiz and counts the active ones.
    let r = export(&app, &mut d.client, &cascade.to_string(), json!({ "scope": "cascade", "which": "missed", "format": "txt", "lines": "questions", "order": "study", "decimals": 1 })).await;
    assert_eq!(text(&r).lines().count(), 0, "the Source quiz was reset: nothing is graded in its current attempt");
}

#[sqlx::test]
async fn the_token_request_answers_401_404_and_429(pool: sqlx::PgPool) {
    let app = app(pool, &[("EXPORT_RATE_PER_MINUTE", "1")]).await;
    let mut c = app.seed_fixture_catalog("root").await;
    let (cascade, _) = create(&mut c, "anagram", "Sevens").await;
    let choices = json!({ "scope": "cascade", "which": "all", "format": "txt", "lines": "questions", "order": "study", "decimals": 1 });
    // Another user's cascade: 404.
    let mut bob = app.signed_in("bob").await;
    assert_eq!(token(&mut bob, &cascade, choices.clone()).await.status, StatusCode::NOT_FOUND);
    // A mismatched X-Wordfall-User: 401.
    let me = c.user_id;
    c.user_id = bob.user_id;
    assert_eq!(token(&mut c, &cascade, choices.clone()).await.status, StatusCode::UNAUTHORIZED);
    c.user_id = me;
    // Past the rate: 429 with Retry-After.
    assert_eq!(token(&mut c, &cascade, choices.clone()).await.status, StatusCode::OK);
    let r = token(&mut c, &cascade, choices.clone()).await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(r.retry_after().is_some());
}

#[sqlx::test]
async fn a_purged_quiz_is_404(pool: sqlx::PgPool) {
    let app = app(pool, &[]).await;
    let c = app.seed_fixture_catalog("root").await;
    let mut d = Device::new(c);
    let (cascade, source) = create(&mut d.client, "anagram", "Sevens").await;
    let cid: Uuid = cascade.parse().unwrap();
    d.qrf = vec![cid];
    d.sync(vec![]).await;
    let t = d.op("trash_cascade", json!({ "cascade_id": cid }));
    let p = d.op("purge_cascade", json!({ "cascade_id": cid }));
    d.sync(vec![t, p]).await;
    let r = token(&mut d.client, &cascade, json!({ "scope": "quiz", "quiz_id": source, "which": "all", "format": "txt", "lines": "questions", "order": "study", "decimals": 1 })).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn the_download_answers_204_for_a_spent_old_or_mismatched_token(pool: sqlx::PgPool) {
    let app = app(pool, &[]).await;
    let mut c = app.seed_fixture_catalog("root").await;
    let (cascade, _) = create(&mut c, "anagram", "Sevens").await;
    let choices = json!({ "scope": "cascade", "which": "all", "format": "txt", "lines": "questions", "order": "study", "decimals": 1 });
    let url = token(&mut c, &cascade, choices.clone()).await.json()["url"].as_str().unwrap().to_owned();
    assert_eq!(app.get(&url).await.status, StatusCode::OK);
    assert_eq!(app.get(&url).await.status, StatusCode::NO_CONTENT, "used twice");
    let url = token(&mut c, &cascade, choices.clone()).await.json()["url"].as_str().unwrap().to_owned();
    let other = url.replace("which=all", "which=missed");
    assert_eq!(app.get(&other).await.status, StatusCode::NO_CONTENT, "other choices");
    let url = token(&mut c, &cascade, choices.clone()).await.json()["url"].as_str().unwrap().to_owned();
    app.state.clock.advance(Duration::seconds(61));
    assert_eq!(app.get(&url).await.status, StatusCode::NO_CONTENT, "61 seconds old");
}

#[sqlx::test]
async fn a_token_from_one_instance_is_redeemed_once_on_either(pool: sqlx::PgPool) {
    let a = app(pool.clone(), &[]).await;
    let b = app(pool, &[]).await;
    let mut c = a.seed_fixture_catalog("root").await;
    let (cascade, _) = create(&mut c, "anagram", "Sevens").await;
    let choices = json!({ "scope": "cascade", "which": "all", "format": "txt", "lines": "questions", "order": "study", "decimals": 1 });
    let url = token(&mut c, &cascade, choices).await.json()["url"].as_str().unwrap().to_owned();
    assert_eq!(b.get(&url).await.status, StatusCode::OK);
    assert_eq!(a.get(&url).await.status, StatusCode::NO_CONTENT);
    assert_eq!(b.get(&url).await.status, StatusCode::NO_CONTENT);
}

#[sqlx::test]
async fn export_and_session_tokens_never_stand_in_for_each_other(pool: sqlx::PgPool) {
    let app = app(pool, &[]).await;
    let mut c = app.seed_fixture_catalog("root").await;
    let (cascade, _) = create(&mut c, "anagram", "Sevens").await;
    let choices = json!({ "scope": "cascade", "which": "all", "format": "txt", "lines": "questions", "order": "study", "decimals": 1 });
    let url = token(&mut c, &cascade, choices).await.json()["url"].as_str().unwrap().to_owned();
    let export_token = url.split("token=").nth(1).unwrap().to_owned();
    let export_token = export_token.replace("%2E", ".").replace("%3A", ":");
    // An export token as the session cookie.
    let mut forged = Client::new(&app, "10.0.0.9");
    forged.cookies.insert("wordfall_session".into(), export_token);
    forged.user_id = c.user_id;
    assert_eq!(forged.get("/api/auth/me").await.status, StatusCode::UNAUTHORIZED);
    // A session token as the export token.
    let session = c.cookies.get("wordfall_session").unwrap().clone();
    let swapped = format!("{}token={session}", url.split("token=").next().unwrap());
    assert_eq!(app.get(&swapped).await.status, StatusCode::NO_CONTENT);
}
