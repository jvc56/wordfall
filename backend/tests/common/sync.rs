//! Sync helpers: a simulated device and a quick cascade setup.
#![allow(dead_code)]

use axum::http::StatusCode;
use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};
use uuid::Uuid;
use wordfall::cascade::order::{questions_hash, shuffle, to_i64};

use super::{Client, TestApp, TestResponse};

/// PLAN.md § The sync cycle: the fixed rejection vocabulary.
pub const REASONS: [&str; 14] = [
    "not_found", "trashed", "not_active", "not_deepest", "stale_attempt", "ungraded", "bad_segment",
    "duplicate_segment", "stale", "not_cleared", "not_trashed", "bad_cursor", "invalid", "error",
];

/// One device of a user: its own id, sync cursor and device_seq.
pub struct Device<'a> {
    pub client: Client<'a>,
    pub id: Uuid,
    pub cursor: Option<i64>,
    pub next_seq: i64,
    pub app_version: i64,
    pub qrf: Vec<Uuid>,
    /// The device's clock, relative to real time.
    pub skew: Duration,
}

impl<'a> Device<'a> {
    pub fn new(client: Client<'a>) -> Self {
        Device { client, id: Uuid::new_v4(), cursor: None, next_seq: 1, app_version: 1, qrf: vec![], skew: Duration::zero() }
    }

    /// A second device of the same account: same cookies, its own device id.
    pub fn sibling(&self, app: &'a TestApp) -> Device<'a> {
        let mut c = Client::new(app, &self.client.ip);
        c.cookies = self.client.cookies.clone();
        c.user_id = self.client.user_id;
        Device::new(c)
    }

    pub fn now(&self) -> DateTime<Utc> {
        Utc::now() + self.skew
    }

    /// An operation with the fields every operation carries.
    pub fn op(&mut self, op_type: &str, fields: Value) -> Value {
        let at = self.now();
        self.op_at(op_type, fields, at)
    }

    pub fn op_at(&mut self, op_type: &str, fields: Value, at: DateTime<Utc>) -> Value {
        let mut o = json!({
            "id": Uuid::new_v4(),
            "device_seq": self.next_seq,
            "seen_seq": self.cursor.unwrap_or(0),
            "at": at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            "type": op_type,
        });
        self.next_seq += 1;
        for (k, v) in fields.as_object().unwrap() {
            o[k] = v.clone();
        }
        o
    }

    pub async fn raw_sync(&mut self, body: Value) -> TestResponse {
        self.client.post("/api/sync", body).await
    }

    /// One request; advances the cursor when the pull is complete.
    pub async fn sync(&mut self, ops: Vec<Value>) -> TestResponse {
        let body = json!({ "device_id": self.id, "app_version": self.app_version, "cursor": self.cursor,
                           "question_rows_for": self.qrf, "ops": ops });
        let r = self.raw_sync(body).await;
        if r.status == StatusCode::OK || r.status == StatusCode::UPGRADE_REQUIRED {
            let b = r.json();
            // "every rejection in the suite carries a reason from the fixed list"
            for x in b["results"].as_array().into_iter().flatten() {
                if x["status"] == "rejected" {
                    assert!(REASONS.contains(&x["reason"].as_str().unwrap_or("")), "{x}");
                }
            }
            if b.get("resync_required") != Some(&json!(true)) && b.get("next_page_token").is_none() {
                self.cursor = b["sync_seq"].as_str().and_then(|s| s.parse().ok());
            }
        }
        r
    }

    /// A sync that follows every page; returns the results and every page.
    pub async fn sync_all(&mut self, ops: Vec<Value>) -> (Vec<Value>, Vec<Value>) {
        let first = self.sync(ops).await;
        assert!(first.status.is_success() || first.status == StatusCode::UPGRADE_REQUIRED, "{:?}", first.json());
        let body = first.json();
        let results = body["results"].as_array().cloned().unwrap_or_default();
        let mut pages = vec![body.clone()];
        let mut token = body.get("next_page_token").cloned();
        while let Some(t) = token {
            let r = self
                .raw_sync(json!({ "device_id": self.id, "app_version": self.app_version, "cursor": self.cursor,
                                  "page_token": t, "question_rows_for": [] }))
                .await;
            assert_eq!(r.status, StatusCode::OK, "{:?}", r.json());
            let b = r.json();
            token = b.get("next_page_token").cloned();
            if token.is_none() {
                self.cursor = b["sync_seq"].as_str().and_then(|s| s.parse().ok());
            }
            pages.push(b);
        }
        (results, pages)
    }
}

/// Every row of one kind across a pull's pages.
pub fn rows(pages: &[Value], table: &str) -> Vec<Value> {
    pages.iter().flat_map(|p| p["changes"][table].as_array().cloned().unwrap_or_default()).collect()
}

/// A minimal catalog item the cascade rows can reference.
pub async fn tiny_lexicon(app: &TestApp) -> i16 {
    if let Some(id) = sqlx::query_scalar::<_, i16>("SELECT id FROM lexicons WHERE name = 'TINY'")
        .fetch_optional(app.db())
        .await
        .unwrap()
    {
        return id;
    }
    let d: i16 = sqlx::query_scalar("INSERT INTO letter_distributions (name) VALUES ('tiny') RETURNING id")
        .fetch_one(app.db())
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO letter_distribution_tiles (letter_distribution_id, position, letter, blank_letter, count, value, is_vowel)
         VALUES ($1, 0, '?', '?', 2, 0, false), ($1, 1, 'A', 'a', 9, 1, true)",
    )
    .bind(d)
    .execute(app.db())
    .await
    .unwrap();
    let l: i16 = sqlx::query_scalar(
        "INSERT INTO lexicons (name, letter_distribution_id, word_count) VALUES ('TINY', $1, 1) RETURNING id",
    )
    .bind(d)
    .fetch_one(app.db())
    .await
    .unwrap();
    sqlx::query("INSERT INTO lexicon_words (lexicon_id, word, playability, definition) VALUES ($1, 'A', 1, 'a')")
        .bind(l)
        .execute(app.db())
        .await
        .unwrap();
    l
}

pub struct Setup {
    pub count: i32,
    pub threshold: i16,
    pub segment_size: i32,
    pub progression: &'static str,
    pub require_alphabetical: bool,
    pub source_seed: u64,
    pub cascade_id: Uuid,
    pub source_id: Uuid,
}

impl Setup {
    pub fn new(count: i32) -> Self {
        Setup {
            count,
            threshold: 80,
            segment_size: 0,
            progression: "ladder",
            require_alphabetical: false,
            source_seed: 0x8000_0000_0000_1234,
            cascade_id: Uuid::new_v4(),
            source_id: Uuid::new_v4(),
        }
    }
}

/// A cascade and its Source quiz written straight to the database, with an
/// exact count and seed, stamped with a fresh sequence of the user's.
pub async fn insert_cascade(app: &TestApp, user: Uuid, s: &Setup) {
    let lexicon = tiny_lexicon(app).await;
    let mut tx = app.db().begin().await.unwrap();
    let seq: i64 = sqlx::query_scalar("UPDATE users SET sync_seq = sync_seq + 1 WHERE id = $1 RETURNING sync_seq")
        .bind(user)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    let spec: Uuid = sqlx::query_scalar("INSERT INTO search_specs (user_id, quiz_type) VALUES ($1, 'anagram') RETURNING id")
        .bind(user)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO search_groups (spec_id, id, parent_id, op, order_in_group) VALUES ($1, 0, NULL, 'and', 0)")
        .bind(spec)
        .execute(&mut *tx)
        .await
        .unwrap();
    let device = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO cascades (id, user_id, name, quiz_type, lexicon_id, spec_id, clear_threshold, segment_size,
                               progression, require_alphabetical, options_changed_at, options_seq, options_device_id,
                               question_count, depth, updated_seq)
         VALUES ($1, $2, 'test', 'anagram', $3, $4, $5, $6, $7::quiz_progression, $8, now() - interval '1 day', $9, $10,
                 $11, 1, $9)",
    )
    .bind(s.cascade_id)
    .bind(user)
    .bind(lexicon)
    .bind(spec)
    .bind(s.threshold)
    .bind(s.segment_size)
    .bind(s.progression)
    .bind(s.require_alphabetical)
    .bind(seq)
    .bind(device)
    .bind(s.count)
    .execute(&mut *tx)
    .await
    .unwrap();
    let idx: Vec<i32> = (0..s.count).collect();
    let keys: Vec<String> = idx.iter().map(|i| format!("K{i:06}")).collect();
    sqlx::query("INSERT INTO cascade_questions (cascade_id, idx, question_key) SELECT $1, * FROM UNNEST($2::int4[], $3::text[])")
        .bind(s.cascade_id)
        .bind(&idx)
        .bind(&keys)
        .execute(&mut *tx)
        .await
        .unwrap();
    let all: Vec<u32> = (0..s.count as u32).collect();
    sqlx::query(
        "INSERT INTO quizzes (id, cascade_id, user_id, level, origin, segment_size, progression, require_alphabetical,
                              options_changed_at, options_seq, options_device_id, shuffle_seed, questions_hash,
                              question_count, created_seq, updated_seq)
         VALUES ($1, $2, $3, 1, 'source', $4, $5::quiz_progression, $6, now() - interval '1 day', $7, $8, $9, $10, $11, $7, $7)",
    )
    .bind(s.source_id)
    .bind(s.cascade_id)
    .bind(user)
    .bind(s.segment_size)
    .bind(s.progression)
    .bind(s.require_alphabetical)
    .bind(seq)
    .bind(device)
    .bind(to_i64(s.source_seed))
    .bind(to_i64(questions_hash(&all)))
    .bind(s.count)
    .execute(&mut *tx)
    .await
    .unwrap();
    let order: Vec<i32> = shuffle(&all, s.source_seed).into_iter().map(|i| i as i32).collect();
    let pos: Vec<i32> = (0..s.count).collect();
    sqlx::query(
        "INSERT INTO quiz_questions (quiz_id, question_idx, position, updated_seq)
         SELECT $1, u.i, u.p, $4 FROM UNNEST($2::int4[], $3::int4[]) AS u(i, p)",
    )
    .bind(s.source_id)
    .bind(&order)
    .bind(&pos)
    .bind(seq)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

/// The positions of a quiz, from the database.
pub async fn positions(app: &TestApp, quiz: Uuid) -> Vec<i32> {
    sqlx::query_scalar("SELECT question_idx FROM quiz_questions WHERE quiz_id = $1 ORDER BY position")
        .bind(quiz)
        .fetch_all(app.db())
        .await
        .unwrap()
}

pub struct QuizNow {
    pub attempt: i32,
    pub seed: String,
    pub cursor: i32,
    pub run_start: i32,
    pub level: i32,
    pub status: String,
}

pub async fn quiz_now(app: &TestApp, quiz: Uuid) -> QuizNow {
    let (attempt, seed, cursor, run_start, level, status): (i32, i64, i32, i32, i32, String) = sqlx::query_as(
        "SELECT attempt, shuffle_seed, cursor, run_start, level, status::text FROM quizzes WHERE id = $1",
    )
    .bind(quiz)
    .fetch_one(app.db())
    .await
    .unwrap();
    QuizNow { attempt, seed: (seed as u64).to_string(), cursor, run_start, level, status }
}

/// Grade ops for the quiz's cards in position order from `from`.
pub async fn grades(app: &TestApp, d: &mut Device<'_>, quiz: Uuid, from: usize, marks: &str) -> Vec<Value> {
    let q = quiz_now(app, quiz).await;
    let pos = positions(app, quiz).await;
    marks
        .chars()
        .enumerate()
        .map(|(i, m)| {
            d.op("grade", json!({ "quiz_id": quiz, "attempt": q.attempt, "attempt_seed": q.seed,
                                  "question_idx": pos[from + i], "grade": if m == 'C' { "correct" } else { "missed" } }))
        })
        .collect()
}

pub async fn finish_op(app: &TestApp, d: &mut Device<'_>, quiz: Uuid, seed: &str) -> (Value, Uuid) {
    let q = quiz_now(app, quiz).await;
    let new = Uuid::new_v4();
    (d.op("finish", json!({ "quiz_id": quiz, "attempt": q.attempt, "attempt_seed": q.seed, "shuffle_seed": seed,
                             "new_quiz_id": new })), new)
}

/// Grades a whole quiz and finishes it in one request; returns the finish's
/// result and the id it offered for a new quiz.
pub async fn play_and_finish(app: &TestApp, d: &mut Device<'_>, quiz: Uuid, marks: &str) -> (Value, Uuid) {
    let mut ops = grades(app, d, quiz, 0, marks).await;
    let (f, new) = finish_op(app, d, quiz, &format!("{}", 1000 + marks.len())).await;
    ops.push(f);
    let n = ops.len();
    let r = d.sync(ops).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.json());
    (result(&r, n - 1), new)
}

pub fn result(r: &TestResponse, i: usize) -> Value {
    r.json()["results"][i].clone()
}

/// The reason of a one-operation sync, or "applied".
pub async fn one(d: &mut Device<'_>, op: Value) -> String {
    let r = d.sync(vec![op]).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.json());
    let x = result(&r, 0);
    x["reason"].as_str().unwrap_or(x["status"].as_str().unwrap()).to_owned()
}

/// `n` cleared one-question chain quizzes under the Source quiz, written
/// straight to the database at a fresh sequence, cleared `days_ago` days ago.
pub async fn insert_cleared_chains(app: &TestApp, user: Uuid, s: &Setup, n: i32, days_ago: i32) -> i64 {
    let mut tx = app.db().begin().await.unwrap();
    let seq: i64 = sqlx::query_scalar("UPDATE users SET sync_seq = sync_seq + 1 WHERE id = $1 RETURNING sync_seq")
        .bind(user)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO quizzes (id, cascade_id, user_id, level, origin, origin_quiz_id, origin_attempt, origin_segment_end,
                              status, segment_chain, segment_size, progression, options_changed_at, options_seq,
                              options_device_id, shuffle_seed, questions_hash, question_count, created_seq, updated_seq,
                              cleared_at, last_activity_at, correct_count)
         SELECT gen_random_uuid(), $1, $2, 2, 'segment', $3, g, 5, 'cleared', true, 0, 'drill', now(), $4,
                gen_random_uuid(), g, 0, 1, $4, $4, now() - make_interval(days => $6), now() - make_interval(days => $6), 1
         FROM generate_series(1, $5) AS g",
    )
    .bind(s.cascade_id)
    .bind(user)
    .bind(s.source_id)
    .bind(seq)
    .bind(n)
    .bind(days_ago)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO quiz_questions (quiz_id, question_idx, position, grade, graded_at, graded_by_device_id, updated_seq)
         SELECT id, 0, 0, 'correct', now(), gen_random_uuid(), $2 FROM quizzes
         WHERE origin_quiz_id = $1 AND status = 'cleared' AND created_seq = $2",
    )
    .bind(s.source_id)
    .bind(seq)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    seq
}
