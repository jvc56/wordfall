//! PLAN.md § Scale tests, the server's half: 300,000 questions end to end
//! against Postgres, each within a time budget. The budgets are constants
//! beside the row counts they measure. Every test here is `#[ignore]`d so
//! `make test-integration` stays quick; `make test-scale` runs them with
//! `--ignored` in release mode, one at a time, and prints each measurement.
//!
//! The device's half (IndexedDB rows, the rebase's work per batch, the
//! keys-only pass, the drop pass) is `frontend/src/lib/sync/scale.scale.ts`.

mod common;

use std::time::{Duration, Instant};

use axum::http::StatusCode;
use common::sync::{insert_cascade, insert_cleared_chains, positions, quiz_now, Device, Setup};
use common::{Client, TestApp, TestResponse};
use serde_json::{json, Value};
use uuid::Uuid;

/// The question ceiling (`MAX_QUIZ_QUESTIONS`' default).
const QUESTIONS: usize = 300_000;
/// Operations per sync request (`SYNC_MAX_OPS`): 300,000 grades are 600 requests.
const BATCH: usize = 500;

const CREATE_BUDGET: Duration = Duration::from_secs(20);
/// 3 keys pages and 30 answer pages.
const DOWNLOAD_BUDGET: Duration = Duration::from_secs(60);
/// 600 requests back to back under `SYNC_RATE_PER_MINUTE` = 120 (a burst of
/// 120, then two a second): at least 240 seconds are the limit's own, and a
/// `Retry-After` in whole seconds rounds each wait up. "Drains in minutes, not
/// hours" (§ When the device syncs): ten of them.
const PUSH_BUDGET: Duration = Duration::from_secs(600);
/// A second device's full pull of the cascade with its 300,000 graded rows.
const PULL_BUDGET: Duration = Duration::from_secs(60);
/// The finish that resets 300,000 rows and creates the 150,000-question Level 2.
const FINISH_BUDGET: Duration = Duration::from_secs(20);
/// Ten concurrent streams of a 300,000-question export with definitions.
const EXPORTS_BUDGET: Duration = Duration::from_secs(120);
/// The ECS task's memory (`infra/variables.tf` `task_memory`, 8192 MiB).
const TASK_MEMORY_BYTES: u64 = 8192 * 1024 * 1024;
/// The ALB's idle timeout for admin uploads (`infra/alb.tf`).
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(120);
const LEAVE_ROWS: usize = 1_000_000;
/// One purge transaction taking a 300,000-question cascade with three levels.
const PURGE_CASCADE_BUDGET: Duration = Duration::from_secs(30);
/// One capped purge run of `PURGE_MAX_QUIZZES_PER_USER_PER_RUN` (5,000) chain quizzes.
const PURGE_RUN_BUDGET: Duration = Duration::from_secs(10);
const CHAINS: i32 = 60_000;
const PURGE_CAP: i32 = 5_000;
/// Rows of other users the purge and pull tables are padded with.
const PADDING: i32 = 1_000_000;
/// An incremental pull on a user with 60,000 chain quizzes.
const INCREMENTAL_PULL_BUDGET: Duration = Duration::from_millis(500);
/// Buffers an `EXPLAIN (ANALYZE, BUFFERS)` of an unchanged pull's query may
/// touch: a handful of index pages, whatever the user's quiz count.
const UNCHANGED_PULL_BUFFERS: i64 = 20;

fn report(what: &str, took: Duration, budget: Duration, rows: &str) {
    println!("scale: {what}: {:.3} s of {:.1} s ({rows})", took.as_secs_f64(), budget.as_secs_f64());
}

async fn app(pool: sqlx::PgPool, extra: &[(&str, &str)]) -> TestApp {
    TestApp::with(pool, extra).await
}

// ---------------------------------------------------------------------------
// A catalog big enough: a distribution of A–Z with nine of each, and a
// lexicon of 300,000 eight-letter words with distinct alphagrams, plus one
// seven-letter word, so a length 7–8 search holds 300,001 questions.
// ---------------------------------------------------------------------------

fn distribution_csv() -> String {
    let mut s = String::from("?,?,2,0,0\n");
    for c in 'A'..='Z' {
        let vowel = u8::from("AEIOU".contains(c));
        s.push_str(&format!("{c},{},9,1,{vowel}\n", c.to_ascii_lowercase()));
    }
    s
}

/// The first `n` sets of `k` distinct letters, in lexicographic order, each
/// written in alphabetical order (so each is its own alphagram).
fn letter_sets(k: usize, n: usize) -> Vec<String> {
    let mut idx: Vec<usize> = (0..k).collect();
    let mut out = Vec::with_capacity(n);
    while out.len() < n {
        out.push(idx.iter().map(|&i| (b'A' + i as u8) as char).collect());
        // Next combination of k from 26.
        let mut i = k;
        while i > 0 && idx[i - 1] == 26 - k + i - 1 {
            i -= 1;
        }
        assert!(i > 0, "ran out of letter sets");
        idx[i - 1] += 1;
        for j in i..k {
            idx[j] = idx[j - 1] + 1;
        }
    }
    out
}

fn lexicon_tsv() -> String {
    let mut s = String::with_capacity(QUESTIONS * 48);
    for (i, w) in letter_sets(8, QUESTIONS).into_iter().enumerate() {
        s.push_str(&format!("{w}\t{}\ta scale test word, number {i} [n -S]\n", 1 + i % 5000));
    }
    s.push_str("ABCDEFG\t1\tthe one seven-letter word\n");
    s
}

async fn upload(admin: &mut Client<'_>, path: &str, fields: &[(&str, &str)], file: &str, bytes: &[u8]) -> (TestResponse, Duration) {
    let t = Instant::now();
    let r = admin.upload(path, fields, file, bytes).await;
    let took = t.elapsed();
    assert_eq!(r.status, StatusCode::CREATED, "{path}: {:?}", r.json());
    (r, took)
}

/// The admin, with the scale distribution and lexicon loaded.
async fn scale_catalog(app: &TestApp) -> Client<'_> {
    let mut admin = app.signed_in("root").await;
    app.make_admin("root").await;
    upload(&mut admin, "/api/admin/letter-distributions", &[("name", "scale")], "scale.csv", distribution_csv().as_bytes()).await;
    let (_, took) = upload(
        &mut admin,
        "/api/admin/lexicons",
        &[("name", "SCALE"), ("letter_distribution", "scale")],
        "SCALE.tsv",
        lexicon_tsv().as_bytes(),
    )
    .await;
    report("lexicon upload", took, UPLOAD_TIMEOUT, "300,001 words");
    let t = Instant::now();
    app.reconcile().await;
    println!("scale: index build: {:.1} s", t.elapsed().as_secs_f64());
    admin
}

fn length(min: i64, max: i64) -> Value {
    json!({ "op": "and", "children": [{ "type": "length", "negated": false, "min": min, "max": max }] })
}

fn cascade_body(filters: Value) -> Value {
    json!({
        "id": Uuid::new_v4(), "source_quiz_id": Uuid::new_v4(), "device_id": Uuid::new_v4(),
        "at": "2026-01-01T00:00:00Z", "name": "Scale", "lexicon": "SCALE", "quiz_type": "anagram",
        "clear_threshold": 80, "segment_size": 0, "progression": "ladder", "require_alphabetical": false,
        "filters": filters,
    })
}

/// A request retried after `Retry-After` while it answers `429`, as the device does.
async fn get_paced(c: &mut Client<'_>, path: &str) -> TestResponse {
    loop {
        let r = c.get(path).await;
        if r.status != StatusCode::TOO_MANY_REQUESTS {
            return r;
        }
        tokio::time::sleep(Duration::from_secs(r.retry_after().unwrap_or(1))).await;
    }
}

async fn sync_paced(d: &mut Device<'_>, ops: Vec<Value>) -> TestResponse {
    loop {
        let r = d.sync(ops.clone()).await;
        if r.status != StatusCode::TOO_MANY_REQUESTS {
            return r;
        }
        tokio::time::sleep(Duration::from_secs(r.retry_after().unwrap_or(1))).await;
    }
}

/// Follows a pull's pages; returns them.
async fn pull_all(d: &mut Device<'_>) -> Vec<Value> {
    let (_, pages) = d.sync_all(vec![]).await;
    pages
}

async fn create_300k(c: &mut Client<'_>) -> (String, Uuid) {
    let r = c.post("/api/cascades", cascade_body(length(8, 8))).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.json());
    let j = r.json();
    assert_eq!(j["cascade"]["question_count"], QUESTIONS);
    (j["cascade"]["id"].as_str().unwrap().to_owned(), j["source_quiz"]["id"].as_str().unwrap().parse().unwrap())
}

/// PLAN.md § Scale tests: "Create a 300,000-question cascade, download its
/// cards, grade every question (half missed) through sync, finish, and assert
/// the timings for creation, download, push (300,000 grades in 600 requests
/// back to back, under the per-user sync rate limit), pull and finish stay
/// within budget … A 300,001-question search is refused with its count."
/// Run at the default rate limits.
#[sqlx::test]
#[ignore = "scale: make test-scale"]
async fn a_300000_question_cascade_end_to_end(pool: sqlx::PgPool) {
    let app = app(pool, &[]).await;
    scale_catalog(&app).await;
    let mut c = app.signed_in("alice").await;

    // A 300,001-question search is refused with its count.
    let r = c.post("/api/search/preview", json!({ "lexicon": "SCALE", "quiz_type": "anagram", "filters": length(7, 8) })).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.json());
    assert_eq!(r.json()["count"], QUESTIONS + 1);
    assert_eq!(r.json()["over_cap"], true);
    let r = c.post("/api/cascades", cascade_body(length(7, 8))).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(r.json()["error"], "over_cap");
    assert_eq!(r.json()["count"], QUESTIONS + 1);

    // Creation.
    let t = Instant::now();
    let (cascade, source) = create_300k(&mut c).await;
    let took = t.elapsed();
    report("creation", took, CREATE_BUDGET, "300,000 questions");
    assert!(took <= CREATE_BUDGET);

    // Download: the keys pass (3 pages of 100,000), then the answers (30 of 10,000).
    let t = Instant::now();
    let (mut keys, mut key_bytes, mut key_pages) = (0, 0, 0);
    while keys < QUESTIONS {
        let r = get_paced(&mut c, &format!("/api/cascades/{cascade}/cards?from={keys}&limit=100000&keys=1")).await;
        assert_eq!(r.status, StatusCode::OK);
        keys += r.json()["keys"].as_array().unwrap().len();
        key_bytes += r.body.len();
        key_pages += 1;
    }
    let (mut cards, mut card_bytes, mut card_pages) = (0, 0, 0);
    while cards < QUESTIONS {
        let r = get_paced(&mut c, &format!("/api/cascades/{cascade}/cards?from={cards}&limit=10000")).await;
        assert_eq!(r.status, StatusCode::OK);
        cards += r.json().as_array().unwrap().len();
        card_bytes += r.body.len();
        card_pages += 1;
    }
    let took = t.elapsed();
    report("download", took, DOWNLOAD_BUDGET, &format!("{key_pages} keys pages, {key_bytes} bytes; {card_pages} answer pages, {card_bytes} bytes"));
    assert_eq!((key_pages, card_pages), (3, 30));
    assert!(took <= DOWNLOAD_BUDGET);

    // Push: every question graded, half missed, 600 requests of 500.
    let mut d = Device::new(c);
    d.qrf = vec![cascade.parse().unwrap()];
    assert_eq!(sync_paced(&mut d, vec![]).await.status, StatusCode::OK);
    let order = positions(&app, source).await;
    let q = quiz_now(&app, source).await;
    let t = Instant::now();
    let mut requests = 0;
    for (b, chunk) in order.chunks(BATCH).enumerate() {
        let ops: Vec<Value> = chunk
            .iter()
            .enumerate()
            .map(|(i, idx)| {
                let missed = (b * BATCH + i) % 2 == 1;
                d.op("grade", json!({ "quiz_id": source, "attempt": q.attempt, "attempt_seed": q.seed,
                                      "question_idx": idx, "grade": if missed { "missed" } else { "correct" } }))
            })
            .collect();
        let r = sync_paced(&mut d, ops).await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.json());
        assert!(r.json()["results"].as_array().unwrap().iter().all(|x| x["status"] == "applied"));
        requests += 1;
    }
    let took = t.elapsed();
    report("push", took, PUSH_BUDGET, &format!("{} grades in {requests} requests", order.len()));
    assert_eq!(requests, QUESTIONS / BATCH);
    assert!(took <= PUSH_BUDGET);

    // Pull: a second device's full pull of the cascade and its graded rows.
    let mut other = d.sibling(&app);
    other.qrf = d.qrf.clone();
    let t = Instant::now();
    let pages = loop {
        // Paced like the device: a 429 on the first request is retried whole.
        let r = sync_paced(&mut other, vec![]).await;
        assert_eq!(r.status, StatusCode::OK);
        let mut pages = vec![r.json()];
        let mut token = r.json().get("next_page_token").cloned();
        while let Some(tk) = token {
            let body = json!({ "device_id": other.id, "app_version": 1, "cursor": null, "page_token": tk, "question_rows_for": [] });
            let r = loop {
                let r = other.raw_sync(body.clone()).await;
                if r.status != StatusCode::TOO_MANY_REQUESTS {
                    break r;
                }
                tokio::time::sleep(Duration::from_secs(r.retry_after().unwrap_or(1))).await;
            };
            assert_eq!(r.status, StatusCode::OK);
            token = r.json().get("next_page_token").cloned();
            pages.push(r.json());
        }
        break pages;
    };
    let took = t.elapsed();
    let graded: usize = common::sync::rows(&pages, "quiz_questions")
        .iter()
        .map(|g| g["question_idx"].as_array().unwrap().len())
        .sum();
    report("pull", took, PULL_BUDGET, &format!("{graded} graded rows in {} pages", pages.len()));
    assert_eq!(graded, QUESTIONS);
    assert!(took <= PULL_BUDGET);

    // Finish: the Source quiz descends; its 150,000 misses are Level 2.
    let new = Uuid::new_v4();
    let q = quiz_now(&app, source).await;
    let op = d.op("finish", json!({ "quiz_id": source, "attempt": q.attempt, "attempt_seed": q.seed,
                                    "shuffle_seed": "4242", "new_quiz_id": new }));
    let t = Instant::now();
    let r = sync_paced(&mut d, vec![op]).await;
    let took = t.elapsed();
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json()["results"][0]["status"], "applied", "{:?}", r.json()["results"][0]);
    let level2: i32 = sqlx::query_scalar("SELECT question_count FROM quizzes WHERE id = $1")
        .bind(new)
        .fetch_one(app.db())
        .await
        .unwrap();
    report("finish", took, FINISH_BUDGET, "300,000 rows reset, a 150,000-question Level 2");
    assert_eq!(level2 as usize, QUESTIONS / 2);
    assert!(took <= FINISH_BUDGET);
}

/// The process's peak resident set since the last reset, in bytes.
fn peak_rss() -> u64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap();
    let kb: u64 = s
        .lines()
        .find(|l| l.starts_with("VmHWM:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|n| n.parse().ok())
        .unwrap();
    kb * 1024
}

fn reset_peak_rss() {
    // Linux: writing 5 to clear_refs resets VmHWM to the current RSS.
    std::fs::write("/proc/self/clear_refs", "5").unwrap();
}

/// PLAN.md § Scale tests: "Ten concurrent GET /api/cascades/:id/export
/// streams of a 300,000-question anagram cascade with definitions, the
/// EXPORT_RATE_PER_MINUTE default, complete within budget without the task
/// exceeding its memory reservation."
#[sqlx::test]
#[ignore = "scale: make test-scale"]
async fn ten_concurrent_exports(pool: sqlx::PgPool) {
    let app = app(pool, &[]).await;
    scale_catalog(&app).await;
    let mut c = app.signed_in("alice").await;
    let (cascade, _) = create_300k(&mut c).await;
    let choices = json!({ "scope": "cascade", "which": "all", "format": "csv", "definitions": true,
                          "columns": ["question", "answer", "definition", "grade"], "order": "study", "decimals": 1 });
    // Ten tokens: the default EXPORT_RATE_PER_MINUTE bucket, exactly.
    let mut urls = Vec::new();
    for _ in 0..10 {
        let r = c.post(&format!("/api/cascades/{cascade}/export-token"), choices.clone()).await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.json());
        urls.push(r.json()["url"].as_str().unwrap().to_owned());
    }
    reset_peak_rss();
    let before = peak_rss();
    let t = Instant::now();
    let mut set = tokio::task::JoinSet::new();
    for u in &urls {
        set.spawn(app.stream_len(u));
    }
    let mut sizes = Vec::new();
    while let Some(r) = set.join_next().await {
        let (status, n) = r.unwrap();
        assert_eq!(status, StatusCode::OK);
        sizes.push(n);
    }
    let took = t.elapsed();
    let peak = peak_rss();
    report(
        "ten exports",
        took,
        EXPORTS_BUDGET,
        &format!("{} bytes each; peak RSS {} MiB, {} MiB above the start", sizes[0], peak >> 20, peak.saturating_sub(before) >> 20),
    );
    assert!(sizes.iter().all(|&n| n == sizes[0] && n > QUESTIONS * 40));
    assert!(took <= EXPORTS_BUDGET);
    // The test process holds the whole backend, so its peak bounds the task's.
    assert!(peak < TASK_MEMORY_BYTES, "peak RSS {} MiB", peak >> 20);
}

/// A million distinct leaves of at most six tiles on the scale distribution:
/// up to six letters, or a blank with up to five, or two blanks with up to four.
fn leave_csv() -> String {
    let mut s = String::with_capacity(LEAVE_ROWS * 12);
    let mut n = 0;
    'all: for blanks in 0..=2usize {
        for k in 1..=(6 - blanks) {
            // Multisets of k letters in non-decreasing order.
            let mut idx = vec![0usize; k];
            loop {
                let mut leave = "?".repeat(blanks);
                leave.extend(idx.iter().map(|&i| (b'A' + i as u8) as char));
                s.push_str(&format!("{leave},{}.{:03}\n", n % 40, n % 1000));
                n += 1;
                if n == LEAVE_ROWS {
                    break 'all;
                }
                let mut i = k;
                while i > 0 && idx[i - 1] == 25 {
                    i -= 1;
                }
                if i == 0 {
                    break;
                }
                let v = idx[i - 1] + 1;
                for j in i - 1..k {
                    idx[j] = v;
                }
            }
        }
        if blanks == 0 {
            // The lone blank and the pair of blanks, once each.
            s.push_str(&format!("?,{}.0\n", 25));
            s.push_str(&format!("??,{}.0\n", 50));
            n += 2;
        }
    }
    assert_eq!(n, LEAVE_ROWS);
    s
}

/// PLAN.md § Scale tests: "Upload a million-row leave file within the upload timeout."
#[sqlx::test]
#[ignore = "scale: make test-scale"]
async fn a_million_row_leave_upload(pool: sqlx::PgPool) {
    let app = app(pool, &[]).await;
    let mut admin = app.signed_in("root").await;
    app.make_admin("root").await;
    upload(&mut admin, "/api/admin/letter-distributions", &[("name", "scale")], "scale.csv", distribution_csv().as_bytes()).await;
    upload(&mut admin, "/api/admin/lexicons", &[("name", "SMALL"), ("letter_distribution", "scale")], "SMALL.tsv", b"AB\t1\tab\n").await;
    let file = leave_csv();
    let (_, took) = upload(&mut admin, "/api/admin/leave-sets", &[("lexicon", "SMALL")], "SMALL-leaves.csv", file.as_bytes()).await;
    report("leave upload", took, UPLOAD_TIMEOUT, &format!("{LEAVE_ROWS} rows, {} bytes", file.len()));
    let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM leave_values").fetch_one(app.db()).await.unwrap();
    assert_eq!(stored as usize, LEAVE_ROWS);
    assert!(took <= UPLOAD_TIMEOUT);
}

// ---------------------------------------------------------------------------
// Purges and pulls on padded tables. These write rows straight to the
// database, as the integration tests' `insert_cascade` does, so the
// measurement is the purge or the pull, not the setup.
// ---------------------------------------------------------------------------

/// `PADDING` cleared chain quizzes of ten other users, each with a question
/// row and an attempt row, cleared today so no purge takes them, and
/// `PADDING` cascades of theirs.
async fn pad(app: &TestApp) {
    for u in 0..10 {
        // Written straight in: they never sign in.
        let user: Uuid = sqlx::query_scalar("INSERT INTO users (username, email, password_hash) VALUES ($1, $1 || '@example.com', 'x') RETURNING id")
            .bind(format!("pad{u}"))
            .fetch_one(app.db())
            .await
            .unwrap();
        let s = Setup::new(1);
        insert_cascade(app, user, &s).await;
        insert_cleared_chains(app, user, &s, PADDING / 10, 0).await;
        attempts_for_chains(app, user).await;
        // And the cascades table: beside eleven rows the planner rightly scans
        // the table, which says nothing about a production instance.
        sqlx::query(
            "INSERT INTO cascades (id, user_id, name, quiz_type, lexicon_id, spec_id, clear_threshold, options_changed_at,
                                   options_seq, options_device_id, question_count, depth, updated_seq)
             SELECT gen_random_uuid(), user_id, 'padding', quiz_type, lexicon_id, spec_id, 80, now(), 1, gen_random_uuid(),
                    1, 1, g
             FROM cascades, generate_series(1, $2 - 1) AS g WHERE id = $1",
        )
        .bind(s.cascade_id)
        .bind(PADDING / 10)
        .execute(app.db())
        .await
        .unwrap();
    }
    sqlx::query("ANALYZE").execute(app.db()).await.unwrap();
}

/// An attempt row for every cleared chain quiz of the user.
async fn attempts_for_chains(app: &TestApp, user: Uuid) {
    sqlx::query(
        "INSERT INTO quiz_attempts (quiz_id, user_id, attempt, question_count, correct_count, missed_count, outcome,
                                    shuffle_seed, finished_at, updated_seq)
         SELECT id, user_id, 1, 1, 1, 0, 'cleared', 7, cleared_at, updated_seq FROM quizzes
         WHERE user_id = $1 AND status = 'cleared'",
    )
    .bind(user)
    .execute(app.db())
    .await
    .unwrap();
}

async fn explain(app: &TestApp, sql: &str, binds: impl FnOnce(sqlx::query::Query<'_, sqlx::Postgres, sqlx::postgres::PgArguments>) -> sqlx::query::Query<'_, sqlx::Postgres, sqlx::postgres::PgArguments>) -> Value {
    let q = format!("EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) {sql}");
    let row: (Value,) = {
        let query = sqlx::query(sqlx::AssertSqlSafe(q));
        let query = binds(query);
        let r = query.fetch_one(app.db()).await.unwrap();
        use sqlx::Row;
        (r.get::<Value, _>(0),)
    };
    row.0[0]["Plan"].clone()
}

/// Every index a plan scans, and the buffers it touched.
fn indexes(plan: &Value, out: &mut Vec<String>) {
    if let Some(i) = plan["Index Name"].as_str() {
        out.push(i.to_owned());
    }
    for p in plan["Plans"].as_array().into_iter().flatten() {
        indexes(p, out);
    }
}

fn buffers(plan: &Value) -> i64 {
    plan["Shared Hit Blocks"].as_i64().unwrap_or(0) + plan["Shared Read Blocks"].as_i64().unwrap_or(0)
}

/// Adds levels 2 and 3 under a cascade's Source quiz, of `n2` and `n3` questions.
async fn add_levels(app: &TestApp, s: &Setup, n2: i32, n3: i32) {
    let mut prev = s.source_id;
    for (level, n) in [(2, n2), (3, n3)] {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO quizzes (id, cascade_id, user_id, level, origin, origin_quiz_id, origin_attempt, segment_size,
                                  progression, require_alphabetical, options_changed_at, options_seq, options_device_id,
                                  shuffle_seed, questions_hash, question_count, created_seq, updated_seq)
             SELECT $1, cascade_id, user_id, $2, 'descent', id, 1, segment_size, progression, require_alphabetical,
                    options_changed_at, options_seq, options_device_id, 7, 0, $3, created_seq, updated_seq
             FROM quizzes WHERE id = $4",
        )
        .bind(id)
        .bind(level)
        .bind(n)
        .bind(prev)
        .execute(app.db())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO quiz_questions (quiz_id, question_idx, position, grade, graded_at, graded_by_device_id, updated_seq)
             SELECT $1, g, g, CASE WHEN g % 2 = 0 THEN 'correct'::grade END, CASE WHEN g % 2 = 0 THEN now() END,
                    CASE WHEN g % 2 = 0 THEN gen_random_uuid() END, 1
             FROM generate_series(0, $2 - 1) AS g",
        )
        .bind(id)
        .bind(n)
        .execute(app.db())
        .await
        .unwrap();
        prev = id;
    }
    sqlx::query("UPDATE cascades SET depth = 3, peak_depth = 3 WHERE id = $1").bind(s.cascade_id).execute(app.db()).await.unwrap();
}

/// PLAN.md § Scale tests: "Purge a trashed 300,000-question cascade with three
/// levels within budget while a sync for the same user proceeds, on a
/// quizzes table padded with a million rows of other users, asserting with
/// EXPLAIN that the delete uses quizzes_cascade_id."
#[sqlx::test]
#[ignore = "scale: make test-scale"]
async fn purging_a_300000_question_cascade(pool: sqlx::PgPool) {
    let app = app(pool, &[("SYNC_RATE_PER_MINUTE", "100000")]).await;
    pad(&app).await;
    let c = app.signed_in("alice").await;
    let user = c.user_id.unwrap();
    let s = Setup::new(QUESTIONS as i32);
    insert_cascade(&app, user, &s).await;
    add_levels(&app, &s, 150_000, 75_000).await;
    sqlx::query("UPDATE cascades SET trashed_at = now() - interval '31 days' WHERE id = $1")
        .bind(s.cascade_id)
        .execute(app.db())
        .await
        .unwrap();
    sqlx::query("ANALYZE").execute(app.db()).await.unwrap();
    let padded: i64 = sqlx::query_scalar("SELECT count(*) FROM quizzes WHERE user_id <> $1").bind(user).fetch_one(app.db()).await.unwrap();
    assert!(padded >= i64::from(PADDING));

    // EXPLAIN (without ANALYZE, which would run it) the purge's delete.
    let row: (Value,) = sqlx::query_as("EXPLAIN (FORMAT JSON) DELETE FROM quizzes WHERE cascade_id = $1")
        .bind(s.cascade_id)
        .fetch_one(app.db())
        .await
        .unwrap();
    let mut used = Vec::new();
    indexes(&row.0[0]["Plan"], &mut used);
    assert!(used.contains(&"quizzes_cascade_id".to_owned()), "{used:?}");

    let mut d = Device::new(c);
    assert_eq!(d.sync(vec![]).await.status, StatusCode::OK);
    let t = Instant::now();
    let (purged, synced) = tokio::join!(wordfall::purge::purge_trash(&app.state), async {
        tokio::time::sleep(Duration::from_millis(50)).await;
        d.sync(vec![]).await
    });
    let took = t.elapsed();
    purged.unwrap();
    report("purge of a trashed cascade", took, PURGE_CASCADE_BUDGET, "300,000 + 150,000 + 75,000 question rows");
    assert_eq!(synced.status, StatusCode::OK, "{:?}", synced.json());
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM quizzes WHERE cascade_id = $1").bind(s.cascade_id).fetch_one(app.db()).await.unwrap();
    assert_eq!(left, 0);
    assert!(took <= PURGE_CASCADE_BUDGET);
}

/// PLAN.md § Scale tests: "Purge a user whose segmented attempt left 60,000
/// cleared chain quizzes all past TRASH_RETENTION_DAYS at once: assert each
/// run takes at most PURGE_MAX_QUIZZES_PER_USER_PER_RUN of them, oldest
/// cleared_at first, that each transaction stays within budget, that the
/// expected number of runs empties the Trash, and that a sync for that user
/// during each run is answered normally rather than 503."
#[sqlx::test]
#[ignore = "scale: make test-scale"]
async fn purging_60000_chain_quizzes_in_capped_runs(pool: sqlx::PgPool) {
    let app = app(pool, &[("SYNC_RATE_PER_MINUTE", "100000")]).await;
    pad(&app).await;
    let c = app.signed_in("alice").await;
    let user = c.user_id.unwrap();
    let s = Setup::new(5);
    insert_cascade(&app, user, &s).await;
    insert_cleared_chains(&app, user, &s, CHAINS, 0).await;
    // All past retention; a larger origin_attempt was cleared earlier.
    sqlx::query(
        "UPDATE quizzes SET cleared_at = now() - interval '31 days' - make_interval(secs => origin_attempt)
         WHERE user_id = $1 AND status = 'cleared'",
    )
    .bind(user)
    .execute(app.db())
    .await
    .unwrap();
    sqlx::query("ANALYZE").execute(app.db()).await.unwrap();
    let mut d = Device::new(c);
    d.qrf = vec![s.cascade_id];
    assert_eq!(d.sync(vec![]).await.status, StatusCode::OK);
    let runs = CHAINS / PURGE_CAP;
    for run in 1..=runs {
        let t = Instant::now();
        let (purged, synced) = tokio::join!(wordfall::purge::purge_trash(&app.state), async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            d.sync(vec![]).await
        });
        let took = t.elapsed();
        purged.unwrap();
        assert_eq!(synced.status, StatusCode::OK, "run {run}: {:?}", synced.json());
        let (left, newest_left): (i64, Option<i32>) = sqlx::query_as(
            "SELECT count(*), max(origin_attempt) FROM quizzes WHERE user_id = $1 AND status = 'cleared'",
        )
        .bind(user)
        .fetch_one(app.db())
        .await
        .unwrap();
        // At most the cap, oldest first: the highest origin_attempts went.
        assert_eq!(left, i64::from(CHAINS - run * PURGE_CAP), "run {run}");
        assert_eq!(newest_left, (left > 0).then_some(CHAINS - run * PURGE_CAP), "run {run}");
        report(&format!("purge run {run} of {runs}"), took, PURGE_RUN_BUDGET, &format!("{PURGE_CAP} chain quizzes"));
        assert!(took <= PURGE_RUN_BUDGET, "run {run}");
    }
}

/// PLAN.md § Scale tests: "An incremental pull on a user whose segmented
/// attempt left 60,000 cleared chain quizzes, each with its own quiz_attempts
/// row, on tables padded with a million rows of other users: assert with
/// EXPLAIN that the attempt query uses quiz_attempts_user_seq and the cascade
/// and quiz queries their own user_seq indexes, that a pull with nothing
/// changed reads a number of rows proportional to what changed rather than to
/// the user's quiz count, and that it stays within a budget beside the row counts."
#[sqlx::test]
#[ignore = "scale: make test-scale"]
async fn an_incremental_pull_beside_60000_chain_quizzes(pool: sqlx::PgPool) {
    let app = app(pool, &[("SYNC_RATE_PER_MINUTE", "100000")]).await;
    pad(&app).await;
    let c = app.signed_in("alice").await;
    let user = c.user_id.unwrap();
    let s = Setup::new(5);
    insert_cascade(&app, user, &s).await;
    insert_cleared_chains(&app, user, &s, CHAINS, 0).await;
    attempts_for_chains(&app, user).await;
    sqlx::query("ANALYZE").execute(app.db()).await.unwrap();
    let mut d = Device::new(c);
    d.qrf = vec![s.cascade_id];
    // The first pull carries everything.
    let pages = pull_all(&mut d).await;
    assert_eq!(common::sync::rows(&pages, "quizzes").len() as i32, CHAINS + 1);
    let cursor = d.cursor.unwrap();

    // Nothing changed: each query scans its user_seq index and touches a
    // handful of pages, whatever the user's quiz count.
    for (sql, index, attempts) in [
        (wordfall::sync::pull::cascades_sql(), "cascades_user_seq", false),
        (wordfall::sync::pull::quizzes_sql(), "quizzes_user_seq", false),
        (wordfall::sync::pull::ATTEMPTS_SQL.to_owned(), "quiz_attempts_user_seq", true),
    ] {
        let plan = explain(&app, &sql, |q| {
            let q = q.bind(user).bind(cursor).bind(cursor).bind(Uuid::nil());
            if attempts { q.bind(0).bind(50_001_i64) } else { q.bind(50_001_i64) }
        })
        .await;
        let mut used = Vec::new();
        indexes(&plan, &mut used);
        assert!(used.contains(&index.to_owned()), "{index}: {used:?}");
        assert!(buffers(&plan) <= UNCHANGED_PULL_BUFFERS, "{index}: {} buffers", buffers(&plan));
        println!("scale: unchanged pull, {index}: {} buffers", buffers(&plan));
    }
    let t = Instant::now();
    let r = d.sync(vec![]).await;
    let took = t.elapsed();
    assert_eq!(r.status, StatusCode::OK);
    report("incremental pull, nothing changed", took, INCREMENTAL_PULL_BUDGET, &format!("{CHAINS} chain quizzes, {PADDING} padding rows"));
    assert!(took <= INCREMENTAL_PULL_BUDGET);

    // One change, made on another device: the pull carries that change alone,
    // through the same indexes.
    let before = d.cursor.unwrap();
    let mut other = d.sibling(&app);
    other.cursor = Some(before);
    let q = quiz_now(&app, s.source_id).await;
    let op = other.op("move_cursor", json!({ "quiz_id": s.source_id, "attempt": q.attempt, "attempt_seed": q.seed, "position": 1 }));
    let r = other.sync(vec![op]).await;
    assert_eq!(r.json()["results"][0]["status"], "applied", "{:?}", r.json());
    let plan = explain(&app, &wordfall::sync::pull::quizzes_sql(), |q| {
        q.bind(user).bind(before).bind(i64::MAX).bind(Uuid::nil()).bind(50_001_i64)
    })
    .await;
    assert_eq!(plan["Actual Rows"], 1, "{plan}");
    assert!(buffers(&plan) <= UNCHANGED_PULL_BUFFERS);
    let t = Instant::now();
    let r = d.sync(vec![]).await;
    let took = t.elapsed();
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json()["changes"]["quizzes"].as_array().unwrap().len(), 1);
    report("incremental pull, one change", took, INCREMENTAL_PULL_BUDGET, "1 quiz row");
    assert!(took <= INCREMENTAL_PULL_BUDGET);
}
