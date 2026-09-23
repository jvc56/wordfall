//! PLAN.md § API → Catalog and search (saved searches) and § Integration tests
//! (saved-search overwrite, round trip, retries, limits, validation).

mod common;

use axum::http::StatusCode;
use common::TestApp;
use serde_json::{json, Value};
use uuid::Uuid;

fn length(min: i64, max: i64) -> Value {
    json!({ "type": "length", "negated": false, "min": min, "max": max })
}

fn save_body(id: Uuid, name: &str, filters: Value) -> Value {
    json!({ "id": id, "name": name, "quiz_type": "anagram", "filters": filters })
}

fn simple() -> Value {
    json!({ "op": "and", "children": [length(7, 7)] })
}

/// A round trip of a tree with nested AND and OR groups, every parameter
/// shape and a 10,000-entry In Word List.
#[sqlx::test]
async fn a_saved_tree_round_trips(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    let mut c = app.signed_in("alice").await;
    let entries: Vec<String> = {
        let mut v: Vec<String> = (0..10_000).map(|i| format!("W{:05}", i).replace('0', "A").replace('1', "B")
            .replace('2', "C").replace('3', "D").replace('4', "E").replace('5', "F").replace('6', "G")
            .replace('7', "H").replace('8', "I").replace('9', "J")).collect();
        v.sort();
        v.dedup();
        v
    };
    assert_eq!(entries.len(), 10_000);
    let tree = json!({ "op": "and", "children": [
        length(2, 7),
        { "op": "or", "children": [
            { "type": "anagram_match", "negated": false, "pattern": "A . [B C] *" },
            { "type": "probability_order", "negated": false, "min": 1, "max": 500, "lax": false },
            { "op": "and", "children": [
                { "type": "consists_of", "negated": false, "tiles": "AEIOU", "min": 50, "max": 100 },
                { "type": "part_of_speech", "negated": true, "part_of_speech": "verb" },
                { "type": "definition", "negated": false, "text": "a fish" } ] } ] },
        { "type": "in_word_list", "negated": false, "entries": entries },
        { "type": "in_lexicon", "negated": true, "lexicon": "CSW21" },
        { "type": "front_inner_hook", "negated": false },
        { "type": "limit_by_playability_order", "negated": false, "min": 1, "max": 100, "lax": true } ] });
    let id = Uuid::new_v4();
    let r = c.post("/api/searches", save_body(id, "Big one", tree.clone())).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.json());
    assert_eq!(r.json()["word_list_entries"], 10_000);
    let r = c.get(&format!("/api/searches/{id}")).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json(), json!({ "id": id, "name": "Big one", "quiz_type": "anagram", "filters": tree }));
    // The list carries the entry count and no tree at all.
    let r = c.get("/api/searches").await;
    let list = r.json();
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["word_list_entries"], 10_000);
    assert!(list[0].get("filters").is_none());
    // Another user's search is 404.
    let mut bob = app.signed_in("bob").await;
    assert_eq!(bob.get(&format!("/api/searches/{id}")).await.status, StatusCode::NOT_FOUND);
    // A save records its quiz_type on the spec.
    let q: String = sqlx::query_scalar(
        "SELECT p.quiz_type::text FROM saved_searches s JOIN search_specs p ON p.id = s.spec_id WHERE s.id = $1",
    )
    .bind(id)
    .fetch_one(app.db())
    .await
    .unwrap();
    assert_eq!(q, "anagram");
}

#[sqlx::test]
async fn overwrite_and_delete(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    let mut c = app.signed_in("alice").await;
    let first = Uuid::new_v4();
    assert_eq!(c.post("/api/searches", save_body(first, "Sevens", simple())).await.status, StatusCode::CREATED);
    let second = Uuid::new_v4();
    let other = json!({ "op": "and", "children": [length(8, 8)] });
    let r = c.post("/api/searches", save_body(second, "Sevens", other.clone())).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(r.json(), json!({ "error": "name_taken" }));
    let mut body = save_body(second, "Sevens", other.clone());
    body["overwrite"] = json!(true);
    assert_eq!(c.post("/api/searches", body).await.status, StatusCode::CREATED);
    // The replaced search and its spec are gone.
    assert_eq!(c.get(&format!("/api/searches/{first}")).await.status, StatusCode::NOT_FOUND);
    let specs: i64 = sqlx::query_scalar("SELECT count(*) FROM search_specs").fetch_one(app.db()).await.unwrap();
    assert_eq!(specs, 1);
    assert_eq!(c.get(&format!("/api/searches/{second}")).await.json()["filters"], other);
    // Delete removes the spec.
    assert_eq!(c.delete_empty(&format!("/api/searches/{second}")).await.status, StatusCode::NO_CONTENT);
    let specs: i64 = sqlx::query_scalar("SELECT count(*) FROM search_specs").fetch_one(app.db()).await.unwrap();
    assert_eq!(specs, 0);
    assert_eq!(c.delete_empty(&format!("/api/searches/{second}")).await.status, StatusCode::NOT_FOUND);
}

/// A save whose response is lost, retried with the same id and body, returns
/// the saved search rather than `name_taken`, and at one below the limit
/// rather than `saved_search_limit`, with one row stored; an id held by
/// another user is refused with 409.
#[sqlx::test]
async fn a_retried_save_is_recognised_by_its_id(pool: sqlx::PgPool) {
    let app = TestApp::with(pool, &[("MAX_SAVED_SEARCHES_PER_USER", "2")]).await;
    let mut c = app.signed_in("alice").await;
    assert_eq!(c.post("/api/searches", save_body(Uuid::new_v4(), "one", simple())).await.status, StatusCode::CREATED);
    let id = Uuid::new_v4();
    let body = save_body(id, "two", simple());
    assert_eq!(c.post("/api/searches", body.clone()).await.status, StatusCode::CREATED);
    let r = c.post("/api/searches", body.clone()).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json()["id"], json!(id));
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM saved_searches").fetch_one(app.db()).await.unwrap();
    assert_eq!(n, 2);
    let mut bob = app.signed_in("bob").await;
    let r = bob.post("/api/searches", save_body(id, "mine", simple())).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
}

#[sqlx::test]
async fn the_saved_search_limit(pool: sqlx::PgPool) {
    let app = TestApp::with(pool, &[("MAX_SAVED_SEARCHES_PER_USER", "2")]).await;
    let mut c = app.signed_in("alice").await;
    for name in ["a", "b"] {
        assert_eq!(c.post("/api/searches", save_body(Uuid::new_v4(), name, simple())).await.status, StatusCode::CREATED);
    }
    let r = c.post("/api/searches", save_body(Uuid::new_v4(), "c", simple())).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(r.json(), json!({ "error": "saved_search_limit", "limit": 2, "count": 2 }));
    // An overwrite of an existing name is accepted at the limit.
    let mut body = save_body(Uuid::new_v4(), "a", simple());
    body["overwrite"] = json!(true);
    assert_eq!(c.post("/api/searches", body).await.status, StatusCode::CREATED);
}

/// Two simultaneous saves compete for the last slot and only one takes it.
#[sqlx::test]
async fn two_saves_race_for_the_last_slot(pool: sqlx::PgPool) {
    let app = TestApp::with(pool, &[("MAX_SAVED_SEARCHES_PER_USER", "1")]).await;
    let a = app.signed_in("alice").await;
    let mut b = common::Client::new(&app, "203.0.113.9");
    b.cookies = a.cookies.clone();
    b.user_id = a.user_id;
    let mut a = a;
    let (ra, rb) = tokio::join!(
        a.post("/api/searches", save_body(Uuid::new_v4(), "x", simple())),
        b.post("/api/searches", save_body(Uuid::new_v4(), "y", simple()))
    );
    let statuses = [ra.status, rb.status];
    assert!(statuses.contains(&StatusCode::CREATED) && statuses.contains(&StatusCode::CONFLICT), "{statuses:?}");
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM saved_searches").fetch_one(app.db()).await.unwrap();
    assert_eq!(n, 1);
}

/// A save validates its tree like a preview, for everything that needs no lexicon.
#[sqlx::test]
async fn a_save_validates_its_tree(pool: sqlx::PgPool) {
    let app = TestApp::new(pool).await;
    let mut c = app.signed_in("alice").await;
    let cases = vec![
        ("empty group", json!({ "op": "and", "children": [length(2, 3), { "op": "or", "children": [] }] })),
        ("fifth nesting level", json!({ "op": "and", "children": [{ "op": "and", "children": [{ "op": "and", "children": [
            { "op": "and", "children": [{ "op": "and", "children": [length(2, 3)] }] }] }] }] })),
        ("does not apply", json!({ "op": "and", "children": [
            { "type": "leave_value", "negated": false, "min": 1.0, "max": null }] })),
        ("narrows nothing", json!({ "op": "and", "children": [length(1, 15)] })),
        ("text too long", json!({ "op": "and", "children": [
            { "type": "definition", "negated": false, "text": "x".repeat(501) }] })),
        ("word list total", json!({ "op": "and", "children": [
            { "type": "in_word_list", "negated": false, "entries": vec!["AB"; 150_001] },
            { "type": "in_word_list", "negated": false, "entries": vec!["AB"; 150_000] }] })),
    ];
    for (what, filters) in cases {
        let r = c.post("/api/searches", save_body(Uuid::new_v4(), what, filters)).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{what}");
        let e = &r.json()["errors"][0];
        assert!(e.get("path").is_some() && e.get("field").is_some() && e.get("message").is_some(), "{what}");
    }
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM search_specs").fetch_one(app.db()).await.unwrap();
    assert_eq!(n, 0, "nothing written");
    // A tile that is not in some other lexicon's distribution is not an error:
    // a saved search carries no lexicon.
    let filters = json!({ "op": "and", "children": [
        { "type": "includes_letters", "negated": false, "tiles": "[NY]Ç" }] });
    assert_eq!(c.post("/api/searches", save_body(Uuid::new_v4(), "catalan", filters)).await.status, StatusCode::CREATED);
    // A missing name, and a 101-scalar-value name, are field errors; 100 is stored.
    let r = c.post("/api/searches", save_body(Uuid::new_v4(), "", simple())).await;
    assert_eq!(r.json()["errors"][0]["field"], "name");
    let r = c.post("/api/searches", save_body(Uuid::new_v4(), &"é".repeat(101), simple())).await;
    assert_eq!(r.json()["errors"][0]["field"], "name");
    let r = c.post("/api/searches", save_body(Uuid::new_v4(), &"é".repeat(100), simple())).await;
    assert_eq!(r.status, StatusCode::CREATED);
}

#[sqlx::test]
async fn loading_a_saved_search_shares_the_search_bucket(pool: sqlx::PgPool) {
    let app = TestApp::with(pool, &[("SEARCH_RATE_PER_MINUTE", "2")]).await;
    let mut c = app.signed_in("alice").await;
    let id = Uuid::new_v4();
    assert_eq!(c.post("/api/searches", save_body(id, "one", simple())).await.status, StatusCode::CREATED);
    assert_eq!(c.get(&format!("/api/searches/{id}")).await.status, StatusCode::OK);
    let r = c.get(&format!("/api/searches/{id}")).await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(r.retry_after().is_some());
}
