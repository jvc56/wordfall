//! PLAN.md § Integration tests → Sync integration tests (the pull side), and
//! PLAN.md § The sync cycle → Pull, § API (the questions and grades endpoints).

mod common;

use std::collections::HashSet;

use axum::http::StatusCode;
use common::sync::{grades, insert_cascade, insert_cleared_chains, play_and_finish, result, rows, Device, Setup};
use common::TestApp;
use serde_json::{json, Value};
use uuid::Uuid;

const PAGE: usize = 50_000;

async fn app(pool: sqlx::PgPool) -> TestApp {
    TestApp::with(pool, &[("SYNC_RATE_PER_MINUTE", "100000"), ("DOWNLOAD_RATE_PER_MINUTE", "100000")]).await
}

async fn device<'a>(app: &'a TestApp, name: &str) -> Device<'a> {
    let mut d = Device::new(app.signed_in(name).await);
    d.sync(vec![]).await;
    d
}

/// Rows of every kind in one page: a question group counts its rows.
fn page_rows(p: &Value) -> usize {
    let c = &p["changes"];
    let n = |t: &str| c[t].as_array().map_or(0, Vec::len);
    let q: usize = c["quiz_questions"].as_array().map_or(0, |g| g.iter().map(|g| g["question_idx"].as_array().unwrap().len()).sum());
    n("cascades") + n("quizzes") + n("quiz_attempts") + q + n("tombstones") + usize::from(c.get("preferences").is_some_and(|p| !p.is_null()))
}

fn page_request(d: &Device<'_>, token: &Value, qrf: Value) -> Value {
    json!({ "device_id": d.id, "app_version": d.app_version, "cursor": d.cursor, "page_token": token, "question_rows_for": qrf })
}

/// "A page holds at most 50,000 rows of any kind": 60,000 cleared chain
/// quizzes are paged although they carry no question rows, and so are their
/// 60,000 tombstones after the retention period.
#[sqlx::test]
async fn pages_hold_at_most_50000_rows(pool: sqlx::PgPool) {
    let app = TestApp::with(pool, &[("SYNC_RATE_PER_MINUTE", "100000"), ("PURGE_MAX_QUIZZES_PER_USER_PER_RUN", "100000")]).await;
    let mut d = device(&app, "alice").await;
    let user = d.client.user_id.unwrap();
    let s = Setup::new(5);
    insert_cascade(&app, user, &s).await;
    insert_cleared_chains(&app, user, &s, 60_000, 0).await;
    d.qrf = vec![s.cascade_id];
    let g = grades(&app, &mut d, s.source_id, 0, "CM").await;
    d.cursor = None;
    let (_, pages) = d.sync_all(g).await;
    assert!(pages.len() >= 2, "paged");
    let mut seen = HashSet::new();
    let mut quiz_page = std::collections::HashMap::new();
    for (i, p) in pages.iter().enumerate() {
        assert!(page_rows(p) <= PAGE, "page {i} has {}", page_rows(p));
        for q in p["changes"]["quizzes"].as_array().unwrap() {
            assert!(seen.insert(q["id"].as_str().unwrap().to_owned()), "each row once");
            quiz_page.insert(q["id"].as_str().unwrap().to_owned(), i);
        }
        for g in p["changes"]["quiz_questions"].as_array().unwrap() {
            let at = quiz_page[g["quiz_id"].as_str().unwrap()];
            assert!(at <= i, "a quiz row precedes its question rows");
            assert_eq!(g["quiz_id"], json!(s.source_id), "only an active quiz's graded rows");
        }
    }
    assert_eq!(seen.len(), 60_001);
    assert_eq!(rows(&pages, "quiz_questions").iter().map(|g| g["question_idx"].as_array().unwrap().len()).sum::<usize>(), 2);
    // Past the retention period, the purge task tombstones them all.
    sqlx::query("UPDATE quizzes SET cleared_at = now() - interval '31 days' WHERE status = 'cleared'").execute(app.db()).await.unwrap();
    assert!(wordfall::purge::run_once(&app.state).await.unwrap());
    let (_, pages) = d.sync_all(vec![]).await;
    assert!(pages.len() >= 2);
    let mut ids = HashSet::new();
    for p in &pages {
        assert!(page_rows(p) <= PAGE);
        for t in p["changes"]["tombstones"].as_array().unwrap() {
            assert!(ids.insert(t["entity_id"].as_str().unwrap().to_owned()));
        }
    }
    assert_eq!(ids.len(), 60_000);
}

/// "Pulls return exactly the rows changed since the cursor, across page
/// boundaries, with the first page's question_rows_for and sequence ceiling
/// fixed for every page even when a later page request sends a different
/// list and another device commits a row between two pages."
#[sqlx::test]
async fn the_first_page_fixes_the_ceiling_and_the_list(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let mut a = device(&app, "alice").await;
    let user = a.client.user_id.unwrap();
    let big = Setup::new(5);
    let other = Setup::new(5);
    insert_cascade(&app, user, &big).await;
    insert_cascade(&app, user, &other).await;
    insert_cleared_chains(&app, user, &big, 55_000, 0).await;
    a.qrf = vec![big.cascade_id, other.cascade_id];
    let g1 = grades(&app, &mut a, big.source_id, 0, "C").await;
    let g2 = grades(&app, &mut a, other.source_id, 0, "M").await;
    a.sync([g1, g2].concat()).await;
    // A fresh device pulls everything with only `big` in its list.
    let mut b = a.sibling(&app);
    b.qrf = vec![big.cascade_id];
    let first = b.sync(vec![]).await;
    let ceil = first.json()["sync_seq"].clone();
    let token = first.json()["next_page_token"].clone();
    assert!(token.is_string());
    let mut pages = vec![first.json()];
    // Between pages another device commits a grade.
    let g = grades(&app, &mut a, other.source_id, 1, "C").await;
    a.sync(g).await;
    let mut token = Some(token);
    while let Some(t) = token {
        let r = b.raw_sync(page_request(&b, &t, json!([other.cascade_id]))).await;
        assert_eq!(r.status, StatusCode::OK);
        assert_eq!(r.json()["sync_seq"], ceil, "the ceiling holds");
        token = r.json().get("next_page_token").cloned();
        pages.push(r.json());
    }
    let groups = rows(&pages, "quiz_questions");
    assert!(groups.iter().all(|g| g["quiz_id"] == json!(big.source_id)), "the first page's list holds");
    let quizzes = rows(&pages, "quizzes");
    let ids: HashSet<_> = quizzes.iter().map(|q| q["id"].clone()).collect();
    assert_eq!(ids.len(), quizzes.len(), "each row once");
    // The row committed between pages arrives on the next pull, once.
    b.cursor = ceil.as_str().unwrap().parse().ok();
    b.qrf = vec![big.cascade_id, other.cascade_id];
    let (_, next) = b.sync_all(vec![]).await;
    let g = rows(&next, "quiz_questions");
    assert_eq!(g.len(), 1);
    assert_eq!(g[0]["quiz_id"], json!(other.source_id));
    assert_eq!(rows(&next, "quizzes").len(), 1);
}

/// `min_updated_seq`, and every row's own sequence as decimal text.
#[sqlx::test]
async fn sequences_on_every_row(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let mut a = device(&app, "alice").await;
    let user = a.client.user_id.unwrap();
    let c1 = Setup::new(5);
    let c2 = Setup::new(5);
    insert_cascade(&app, user, &c1).await;
    insert_cascade(&app, user, &c2).await;
    a.qrf = vec![c1.cascade_id, c2.cascade_id];
    a.sync(vec![]).await;
    let mut b = a.sibling(&app);
    b.qrf = a.qrf.clone();
    b.sync(vec![]).await;
    // An earlier push (A): a grade on c1 and an options change on c2.
    let mut ops = grades(&app, &mut a, c1.source_id, 0, "C").await;
    ops.push(a.op("set_cascade_options", json!({ "cascade_id": c2.cascade_id, "segment_size": 5 })));
    let ra = a.sync(ops).await;
    let s1: i64 = ra.json()["sync_seq"].as_str().unwrap().parse().unwrap();
    // This push (B): a grade on c1, which stamps c1's row (PLAN.md § Schema notes).
    let ops = grades(&app, &mut b, c1.source_id, 1, "M").await;
    let rb = b.sync(ops).await;
    let s2 = rb.json()["sync_seq"].as_str().unwrap().to_owned();
    let groups = rb.json()["changes"]["quiz_questions"].as_array().cloned().unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0]["min_updated_seq"], s1.to_string());
    let seqs: HashSet<String> =
        rb.json()["changes"]["cascades"].as_array().unwrap().iter().map(|c| c["updated_seq"].as_str().unwrap().to_owned()).collect();
    assert_eq!(seqs, HashSet::from([s1.to_string(), s2.clone()]));
    for q in rb.json()["changes"]["quizzes"].as_array().unwrap() {
        assert!(q["updated_seq"].is_string());
    }
    // A tombstone's seq is text too.
    let t = a.op("trash_cascade", json!({ "cascade_id": c2.cascade_id }));
    let p = a.op("purge_cascade", json!({ "cascade_id": c2.cascade_id }));
    let r = a.sync(vec![t, p]).await;
    assert!(r.json()["changes"]["tombstones"][0]["seq"].is_string());
}

/// "a quiz whose rows span a page break reports a minimum per page, with the
/// earlier device's grade on the first of them".
#[sqlx::test]
async fn a_minimum_per_page(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let mut a = device(&app, "alice").await;
    let user = a.client.user_id.unwrap();
    let s = Setup::new(60_000);
    insert_cascade(&app, user, &s).await;
    a.qrf = vec![s.cascade_id];
    a.sync(vec![]).await;
    let mut b = a.sibling(&app);
    b.qrf = a.qrf.clone();
    b.sync(vec![]).await;
    // The earlier device grades idx 0; then everything else is graded at a later sequence.
    let q = common::sync::quiz_now(&app, s.source_id).await;
    let g = a.op("grade", json!({ "quiz_id": s.source_id, "attempt": 1, "attempt_seed": q.seed, "question_idx": 0, "grade": "correct" }));
    let r = a.sync(vec![g]).await;
    let s1 = r.json()["sync_seq"].as_str().unwrap().to_owned();
    let s2: i64 = sqlx::query_scalar("UPDATE users SET sync_seq = sync_seq + 1 WHERE id = $1 RETURNING sync_seq")
        .bind(user)
        .fetch_one(app.db())
        .await
        .unwrap();
    sqlx::query(
        "UPDATE quiz_questions SET grade = 'correct', graded_at = now(), graded_by_device_id = $3, updated_seq = $2
         WHERE quiz_id = $1 AND question_idx > 0",
    )
    .bind(s.source_id)
    .bind(s2)
    .bind(a.id)
    .execute(app.db())
    .await
    .unwrap();
    sqlx::query("UPDATE quizzes SET correct_count = 60000, updated_seq = $2 WHERE id = $1").bind(s.source_id).bind(s2).execute(app.db()).await.unwrap();
    let (_, pages) = b.sync_all(vec![]).await;
    let mins: Vec<String> = pages
        .iter()
        .flat_map(|p| p["changes"]["quiz_questions"].as_array().unwrap().iter().map(|g| g["min_updated_seq"].as_str().unwrap().to_owned()))
        .collect();
    assert_eq!(mins, [s1, s2.to_string()]);
}

/// Tombstones, the floor and `resync_required`.
#[sqlx::test]
async fn resync_required_cases(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let mut d = device(&app, "alice").await;
    let user = d.client.user_id.unwrap();
    let old = Setup::new(3);
    let kept = Setup::new(3);
    let live = Setup::new(3);
    for s in [&old, &kept, &live] {
        insert_cascade(&app, user, s).await;
    }
    d.qrf = vec![live.cascade_id];
    let start = d.sync(vec![]).await.json()["sync_seq"].as_str().unwrap().parse::<i64>().unwrap();
    for s in [&old, &kept] {
        let t = d.op("trash_cascade", json!({ "cascade_id": s.cascade_id }));
        let p = d.op("purge_cascade", json!({ "cascade_id": s.cascade_id }));
        let r = d.sync(vec![t, p]).await;
        // The cascade's and its quiz's.
        assert_eq!(r.json()["changes"]["tombstones"].as_array().unwrap().len(), 2, "purges produce tombstones");
    }
    let between = d.cursor.unwrap() - 1;
    // The first tombstone passes the retention period and is pruned.
    sqlx::query("UPDATE sync_tombstones SET deleted_at = now() - interval '91 days' WHERE entity_id IN ($1, $2)")
        .bind(old.cascade_id)
        .bind(old.source_id)
        .execute(app.db())
        .await
        .unwrap();
    wordfall::purge::prune_sync_records(&app.state).await.unwrap();
    let floor: i64 = sqlx::query_scalar("SELECT sync_floor_seq FROM users WHERE id = $1").bind(user).fetch_one(app.db()).await.unwrap();
    assert!(floor > start);
    // Below the floor: resync_required, carrying the push's results and no changes.
    let mut behind = d.sibling(&app);
    behind.cursor = Some(start);
    let op = behind.op("set_cascade_options", json!({ "cascade_id": live.cascade_id, "segment_size": 5 }));
    let r = behind.sync(vec![op]).await;
    assert_eq!(r.json()["resync_required"], true);
    assert_eq!(result(&r, 0)["status"], "applied");
    assert!(r.json().get("changes").is_none());
    // Between the floor and the oldest kept tombstone: an ordinary pull.
    let mut mid = d.sibling(&app);
    mid.cursor = Some(between);
    assert!(floor <= between);
    let r = mid.sync(vec![]).await;
    assert!(r.json().get("resync_required").is_none());
    assert_eq!(r.json()["changes"]["tombstones"].as_array().unwrap().len(), 2);
    // Above sync_seq: resync_required with none of the operations applied.
    let seq = d.cursor.unwrap();
    let mut ahead = d.sibling(&app);
    ahead.cursor = Some(seq + 100);
    let op = ahead.op("set_cascade_options", json!({ "cascade_id": live.cascade_id, "segment_size": 6 }));
    let r = ahead.sync(vec![op]).await;
    assert_eq!(r.json()["resync_required"], true);
    let size: i32 = sqlx::query_scalar("SELECT segment_size FROM cascades WHERE id = $1").bind(live.cascade_id).fetch_one(app.db()).await.unwrap();
    assert_eq!(size, 5, "not applied");
    // After the restore procedure's bump, the same device's operations apply and it is told to resync.
    sqlx::query("UPDATE users SET sync_seq = sync_seq + 4294967296, sync_floor_seq = sync_seq + 4294967296").execute(app.db()).await.unwrap();
    let op = ahead.op("set_cascade_options", json!({ "cascade_id": live.cascade_id, "segment_size": 7 }));
    let r = ahead.sync(vec![op]).await;
    assert_eq!(r.json()["resync_required"], true);
    assert_eq!(result(&r, 0)["status"], "applied");
    // cursor: null on an account whose floor is above 0: every row, no tombstones, never resync_required.
    let mut fresh = d.sibling(&app);
    fresh.qrf = vec![live.cascade_id];
    let (_, pages) = fresh.sync_all(vec![]).await;
    assert!(pages[0].get("resync_required").is_none());
    assert!(rows(&pages, "tombstones").is_empty());
    assert_eq!(rows(&pages, "cascades").len(), 1);
}

/// "a cursor: null request carrying 200 grades from a device that created its
/// cascade and never synced applies them and returns every row"; and a new
/// device's first pull carries the preferences of an account that never changed them.
#[sqlx::test]
async fn first_syncs(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let c = app.signed_in("alice").await;
    let s = Setup::new(200);
    insert_cascade(&app, c.user_id.unwrap(), &s).await;
    let mut d = Device::new(c);
    d.qrf = vec![s.cascade_id];
    let marks = "C".repeat(200);
    let q = common::sync::quiz_now(&app, s.source_id).await;
    let pos = common::sync::positions(&app, s.source_id).await;
    let ops: Vec<Value> = marks
        .chars()
        .enumerate()
        .map(|(i, _)| d.op("grade", json!({ "quiz_id": s.source_id, "attempt": 1, "attempt_seed": q.seed, "question_idx": pos[i], "grade": "correct" })))
        .collect();
    let (res, pages) = d.sync_all(ops).await;
    assert!(res.iter().all(|r| r["status"] == "applied"));
    assert_eq!(rows(&pages, "cascades").len(), 1);
    assert_eq!(rows(&pages, "quizzes").len(), 1);
    assert_eq!(rows(&pages, "quiz_questions")[0]["question_idx"].as_array().unwrap().len(), 200);
    let prefs = &pages[0]["changes"]["preferences"];
    assert_eq!(prefs["default_clear_threshold"], 80);
    assert!(prefs["bindings"].as_array().is_some_and(|b| !b.is_empty()));
}

/// "The grades endpoint returns a quiz's graded rows in pages with its attempt
/// and seed, and 404 for another user's or a purged quiz. `from` is an index."
#[sqlx::test]
async fn the_grades_endpoint(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let mut d = device(&app, "alice").await;
    let user = d.client.user_id.unwrap();
    let s = Setup::new(300_000);
    insert_cascade(&app, user, &s).await;
    sqlx::query(
        "UPDATE quiz_questions SET grade = 'missed', graded_at = now(), graded_by_device_id = $2
         WHERE quiz_id = $1 AND question_idx >= 299600",
    )
    .bind(s.source_id)
    .bind(d.id)
    .execute(app.db())
    .await
    .unwrap();
    let q = common::sync::quiz_now(&app, s.source_id).await;
    for page in 0..6 {
        let from = page * 50_000;
        let r = d.client.get(&format!("/api/cascades/{}/quizzes/{}/grades?from={from}&limit=50000", s.cascade_id, s.source_id)).await;
        assert_eq!(r.status, StatusCode::OK);
        assert_eq!(r.headers["cache-control"], "private, no-store");
        let b = r.json();
        assert_eq!(b["attempt"], 1);
        assert_eq!(b["shuffle_seed"], q.seed);
        let idx: Vec<i64> = b["question_idx"].as_array().unwrap().iter().map(|v| v.as_i64().unwrap()).collect();
        assert!(idx.iter().all(|i| (from..from + 50_000).contains(i)), "inside its own window");
        assert_eq!(idx.len(), if page < 5 { 0 } else { 400 });
        assert_eq!(b["grade"].as_array().unwrap().len(), idx.len());
    }
    let mut bob = device(&app, "bob").await;
    let path = format!("/api/cascades/{}/quizzes/{}/grades?from=0&limit=10", s.cascade_id, s.source_id);
    assert_eq!(bob.client.get(&path).await.status, StatusCode::NOT_FOUND);
    let t = d.op("trash_cascade", json!({ "cascade_id": s.cascade_id }));
    let p = d.op("purge_cascade", json!({ "cascade_id": s.cascade_id }));
    d.sync(vec![t, p]).await;
    assert_eq!(d.client.get(&path).await.status, StatusCode::NOT_FOUND);
}

/// "A sync whose question_rows_for omits a cascade carries none of its
/// question rows, one with an empty list carries no question rows at all,
/// and one naming another user's cascade or a purged id has those ids ignored."
#[sqlx::test]
async fn question_rows_for_filters(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let mut d = device(&app, "alice").await;
    let user = d.client.user_id.unwrap();
    let c1 = Setup::new(5);
    let c2 = Setup::new(5);
    insert_cascade(&app, user, &c1).await;
    insert_cascade(&app, user, &c2).await;
    d.qrf = vec![c1.cascade_id, c2.cascade_id];
    let g = [grades(&app, &mut d, c1.source_id, 0, "C").await, grades(&app, &mut d, c2.source_id, 0, "C").await].concat();
    d.sync(g).await;
    let mut bob = device(&app, "bob").await;
    let theirs = Setup::new(5);
    insert_cascade(&app, bob.client.user_id.unwrap(), &theirs).await;
    let g = grades(&app, &mut bob, theirs.source_id, 0, "C").await;
    bob.sync(g).await;
    for (list, want) in [
        (vec![c1.cascade_id], vec![c1.source_id]),
        (vec![], vec![]),
        (vec![c2.cascade_id, theirs.cascade_id, Uuid::new_v4()], vec![c2.source_id]),
    ] {
        let mut fresh = d.sibling(&app);
        fresh.qrf = list;
        let (_, pages) = fresh.sync_all(vec![]).await;
        let got: Vec<Value> = rows(&pages, "quiz_questions").iter().map(|g| g["quiz_id"].clone()).collect();
        assert_eq!(got, want.iter().map(|u| json!(u)).collect::<Vec<_>>());
    }
}

/// "A pull sends only graded question rows of active quizzes and never an
/// index list or a cleared quiz's rows, sends each quiz row before its
/// question rows, and orders tables as specified; the questions endpoint
/// returns a quiz's index list in pages, 404 for a Source quiz, and the same
/// bytes for the quiz's whole life."
#[sqlx::test]
async fn pull_contents_and_the_questions_endpoint(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let mut d = device(&app, "alice").await;
    let s = Setup::new(10);
    insert_cascade(&app, d.client.user_id.unwrap(), &s).await;
    d.qrf = vec![s.cascade_id];
    let (_, l2) = play_and_finish(&app, &mut d, s.source_id, "CCCCCCCMMM").await;
    let q = |from: i64, limit: i64| format!("/api/cascades/{}/quizzes/{}/questions?from={from}&limit={limit}", s.cascade_id, l2);
    let before = d.client.get(&q(0, 2)).await;
    assert_eq!(before.status, StatusCode::OK);
    assert_eq!(before.headers["cache-control"], "private, max-age=31536000, immutable");
    let rest = d.client.get(&q(2, 2)).await;
    let mut all: Vec<Value> = before.json().as_array().unwrap().clone();
    all.extend(rest.json().as_array().unwrap().iter().cloned());
    assert_eq!(all.len(), 3);
    let src = format!("/api/cascades/{}/quizzes/{}/questions?from=0&limit=10", s.cascade_id, s.source_id);
    assert_eq!(d.client.get(&src).await.status, StatusCode::NOT_FOUND);
    // Grade part of Level 2 and clear it: its rows are never pulled again.
    let g = grades(&app, &mut d, l2, 0, "C").await;
    let r = d.sync(g).await;
    let groups = r.json()["changes"]["quiz_questions"].as_array().cloned().unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0]["question_idx"].as_array().unwrap().len(), 1, "graded rows only");
    let (r, _) = play_and_finish(&app, &mut d, l2, "CCC").await;
    assert_eq!(r["outcome"], "cleared");
    let mut fresh = d.sibling(&app);
    fresh.qrf = d.qrf.clone();
    let (_, pages) = fresh.sync_all(vec![]).await;
    let groups = rows(&pages, "quiz_questions");
    assert!(groups.iter().all(|g| g["quiz_id"] != json!(l2)), "no cleared quiz's rows");
    let keys: Vec<&str> = pages[0]["changes"].as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, ["cascades", "quizzes", "quiz_attempts", "quiz_questions", "preferences", "tombstones"]);
    assert_eq!(d.client.get(&q(0, 2)).await.body, before.body, "the same bytes for the quiz's life");
}
