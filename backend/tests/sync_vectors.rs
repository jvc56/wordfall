//! The shared rule vectors (contract-fixtures/cascade/rules.json) pushed
//! through the real `POST /api/sync`, with the database compared against each
//! vector's expected state at every checkpoint, and the application-level
//! invariants under PLAN.md § Schema checked there too. The 1,200-operation
//! vector goes in three batches (§ Contract fixtures → Attempt seeds).

mod common;

use axum::http::StatusCode;
use common::sync::{insert_cascade, Device, Setup};
use common::TestApp;
use serde_json::{json, Value};
use uuid::Uuid;
use wordfall::cascade::order::shuffle;

fn fixture() -> Value {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../contract-fixtures/cascade/rules.json");
    serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
}

fn wire_op(dev: &mut Device<'_>, op: &Value, cascade: Uuid) -> Value {
    let mut fields = op.as_object().unwrap().clone();
    let t = fields.remove("type").unwrap();
    if let Some(q) = fields.remove("quiz") {
        fields.insert("quiz_id".into(), q);
    }
    let t = t.as_str().unwrap();
    if matches!(t, "trash_cascade" | "restore_cascade" | "purge_cascade" | "set_cascade_options") {
        fields.insert("cascade_id".into(), json!(cascade));
    }
    dev.op(t, Value::Object(fields))
}

async fn db_state(app: &TestApp, cascade: Uuid) -> Value {
    let c: Option<(i32, i32, i32, bool, bool, i32, String, bool)> = sqlx::query_as(
        "SELECT depth, peak_depth, attempts_since_completion, completed_at IS NOT NULL, trashed_at IS NOT NULL,
                segment_size, progression::text, require_alphabetical FROM cascades WHERE id = $1",
    )
    .bind(cascade)
    .fetch_optional(app.db())
    .await
    .unwrap();
    #[derive(sqlx::FromRow)]
    struct Q(Uuid, i32, String, String, Option<Uuid>, Option<i32>, Option<i32>, bool, i32, String, bool, i32, i64, i32, i64, i32, i32, i32, i32);
    let quizzes: Vec<Q> =
        sqlx::query_as(
            "SELECT id, level, status::text, origin::text, origin_quiz_id, origin_attempt, origin_segment_end,
                    segment_chain, segment_size, progression::text, require_alphabetical, attempt, shuffle_seed,
                    question_count, questions_hash, correct_count, missed_count, cursor, run_start
             FROM quizzes WHERE cascade_id = $1 ORDER BY id",
        )
        .bind(cascade)
        .fetch_all(app.db())
        .await
        .unwrap();
    let mut out = Vec::new();
    for q in quizzes {
        let rows: Vec<(i32, i32, Option<String>)> = sqlx::query_as(
            "SELECT question_idx, position, grade::text FROM quiz_questions WHERE quiz_id = $1 ORDER BY question_idx",
        )
        .bind(q.0)
        .fetch_all(app.db())
        .await
        .unwrap();
        let questions: Vec<u32> = rows.iter().map(|r| r.0 as u32).collect();
        // Invariants: the counters match the grades, and the positions come
        // from the seed through the shared shuffle.
        let correct = rows.iter().filter(|r| r.2.as_deref() == Some("correct")).count() as i32;
        let missed = rows.iter().filter(|r| r.2.as_deref() == Some("missed")).count() as i32;
        assert_eq!((q.15, q.16), (correct, missed), "counters match grades");
        let mut by_pos = rows.clone();
        by_pos.sort_by_key(|r| r.1);
        let order: Vec<u32> = by_pos.iter().map(|r| r.0 as u32).collect();
        assert_eq!(order, shuffle(&questions, q.12 as u64), "positions come from the seed");
        out.push(json!({
            "id": q.0.to_string(), "level": q.1, "status": q.2, "origin": q.3,
            "origin_quiz_id": q.4.map(|u| u.to_string()), "origin_attempt": q.5, "origin_segment_end": q.6,
            "segment_chain": q.7, "segment_size": q.8, "progression": q.9, "require_alphabetical": q.10,
            "attempt": q.11, "shuffle_seed": (q.12 as u64).to_string(), "question_count": q.13,
            "questions_hash": (q.14 as u64).to_string(), "questions": questions, "correct": q.15, "missed": q.16,
            "cursor": q.17, "run_start": q.18,
        }));
    }
    let Some(c) = c else {
        return json!({ "purged": true, "quizzes": out });
    };
    // Invariant: depth is the number of active quizzes, at levels 1..depth.
    let mut levels: Vec<i64> = out.iter().filter(|q| q["status"] == "active").map(|q| q["level"].as_i64().unwrap()).collect();
    levels.sort_unstable();
    assert_eq!(levels, (1..=i64::from(c.0)).collect::<Vec<_>>());
    json!({
        "depth": c.0, "peak_depth": c.1, "attempts_since_completion": c.2, "completed": c.3, "trashed": c.4,
        "purged": false, "segment_size": c.5, "progression": c.6, "require_alphabetical": c.7, "quizzes": out,
    })
}

fn expected(state: &Value) -> Value {
    let c = &state["cascade"];
    let quizzes: Vec<Value> = state["quizzes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|q| {
            let mut q = q.clone();
            let o = q.as_object_mut().unwrap();
            o.remove("next_boundary");
            o.remove("run_indicator");
            q
        })
        .collect();
    if c["purged"] == true {
        return json!({ "purged": true, "quizzes": quizzes });
    }
    json!({
        "depth": c["depth"], "peak_depth": c["peak_depth"], "attempts_since_completion": c["attempts_since_completion"],
        "completed": c["completions"].as_u64().unwrap() > 0, "trashed": c["trashed"], "purged": false,
        "segment_size": c["segment_size"], "progression": c["progression"], "require_alphabetical": c["require_alphabetical"],
        "quizzes": quizzes,
    })
}

fn same_result(got: &Value, want: &Value, ctx: &str) {
    assert_eq!(got["status"], want["status"], "{ctx}: {got}");
    assert_eq!(got.get("reason"), want.get("reason"), "{ctx}");
    assert_eq!(got.get("outcome"), want.get("outcome"), "{ctx}");
    assert_eq!(got.get("new_quiz_question_count"), want.get("new_quiz_question_count"), "{ctx}");
    assert_eq!(got.get("new_quiz_questions_hash"), want.get("new_quiz_questions_hash"), "{ctx}");
}

async fn run_vectors(pool: sqlx::PgPool, filter: impl Fn(&str) -> bool) {
    let app = TestApp::with(pool, &[("SYNC_RATE_PER_MINUTE", "100000"), ("MAX_CASCADES_PER_USER", "1000")]).await;
    let c = app.signed_in("player").await;
    let user = c.user_id.unwrap();
    let mut dev = Device::new(c);
    dev.sync(vec![]).await;
    let f = fixture();
    for (k, v) in f["vectors"].as_array().unwrap().iter().enumerate() {
        let name = v["name"].as_str().unwrap();
        if !filter(name) {
            continue;
        }
        // Quiz ids are global on the server: give each vector its own prefix,
        // which keeps their order.
        let text = serde_json::to_string(v).unwrap().replace("\"00000000-0000-4000-8000-", &format!("\"{:08x}-0000-4000-8000-", k + 1));
        let v: Value = serde_json::from_str(&text).unwrap();
        let v = &v;
        let s = &v["setup"];
        let mut setup = Setup::new(s["question_count"].as_i64().unwrap() as i32);
        setup.threshold = s["clear_threshold"].as_i64().unwrap() as i16;
        setup.segment_size = s["segment_size"].as_i64().unwrap() as i32;
        setup.progression = if s["progression"] == "drill" { "drill" } else { "ladder" };
        setup.require_alphabetical = s["require_alphabetical"].as_bool().unwrap();
        setup.source_seed = s["source_seed"].as_str().unwrap().parse().unwrap();
        setup.source_id = s["source_quiz_id"].as_str().unwrap().parse().unwrap();
        insert_cascade(&app, user, &setup).await;
        dev.sync(vec![]).await;
        let steps = v["steps"].as_array().unwrap();
        let big = steps.len() == 1200;
        let mut i = 0;
        while i < steps.len() {
            // Up to the next checkpoint, at most 500 operations a request; the
            // 1,200-operation outbox goes in three batches.
            let mut j = i;
            let cap = if big { 400 } else { 500 };
            while j < steps.len() && j - i < cap {
                j += 1;
                if !big && steps[j - 1].get("state").is_some() {
                    break;
                }
            }
            let ops: Vec<Value> = steps[i..j].iter().map(|st| wire_op(&mut dev, &st["op"], setup.cascade_id)).collect();
            let r = dev.sync(ops).await;
            assert_eq!(r.status, StatusCode::OK, "{name}: {:?}", r.json());
            let results = r.json()["results"].as_array().cloned().unwrap();
            for (k, st) in steps[i..j].iter().enumerate() {
                same_result(&results[k], &st["result"], &format!("{name} step {}", i + k));
            }
            if let Some(state) = steps[j - 1].get("state") {
                assert_eq!(db_state(&app, setup.cascade_id).await, expected(state), "{name} after step {}", j - 1);
            }
            i = j;
        }
    }
}

#[sqlx::test]
async fn rule_vectors_through_the_server_part_1(pool: sqlx::PgPool) {
    run_vectors(pool, |n| n < "run").await;
}

#[sqlx::test]
async fn rule_vectors_through_the_server_part_2(pool: sqlx::PgPool) {
    run_vectors(pool, |n| n >= "run").await;
}
