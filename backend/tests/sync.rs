//! PLAN.md § Integration tests → Sync integration tests (the push side):
//! repeats, 64-bit values on the wire, ordering, request-level 400s, the
//! device mark, 426, invalid values, conflicts, finishes and segments.

mod common;

use axum::http::StatusCode;
use chrono::{Duration, Utc};
use common::sync::{finish_op, grades, insert_cascade, positions, quiz_now, result, rows, Device, Setup};
use common::TestApp;
use serde_json::{json, Value};
use uuid::Uuid;

async fn app(pool: sqlx::PgPool) -> TestApp {
    TestApp::with(pool, &[("SYNC_RATE_PER_MINUTE", "100000")]).await
}

async fn device<'a>(app: &'a TestApp, name: &str) -> Device<'a> {
    let c = app.signed_in(name).await;
    let mut d = Device::new(c);
    d.sync(vec![]).await;
    d
}

/// A cascade of `count` questions for the device's user, pulled once.
async fn cascade(app: &TestApp, d: &mut Device<'_>, s: Setup) -> Setup {
    insert_cascade(app, d.client.user_id.unwrap(), &s).await;
    d.qrf.push(s.cascade_id);
    d.sync(vec![]).await;
    s
}

#[sqlx::test]
async fn a_repeated_operation_is_applied_once_with_the_same_result(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let mut d = device(&app, "alice").await;
    let s = cascade(&app, &mut d, Setup::new(4)).await;
    let mut ops = grades(&app, &mut d, s.source_id, 0, "CCMM").await;
    let (fin, _) = finish_op(&app, &mut d, s.source_id, "123456789").await;
    ops.push(fin.clone());
    let first = d.sync(ops).await;
    let r1 = result(&first, 4);
    assert_eq!(r1["outcome"], "descended");
    // The response was lost: the same finish again.
    let again = d.sync(vec![fin]).await;
    assert_eq!(result(&again, 0), r1, "the same result in every field");
    let quizzes: i64 = sqlx::query_scalar("SELECT count(*) FROM quizzes").fetch_one(app.db()).await.unwrap();
    assert_eq!(quizzes, 2, "applied once");
}

/// A finish whose seed and resulting hash are both above 2^63: stored as
/// negative BIGINTs, answered, pulled and repeated as unsigned decimal text;
/// the seed recognised as a later grade's attempt_seed.
#[sqlx::test]
async fn sixty_four_bit_values_travel_as_unsigned_text(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let mut d = device(&app, "alice").await;
    let mut setup = Setup::new(3);
    setup.threshold = 100;
    let s = cascade(&app, &mut d, setup).await;
    // Find a miss set whose hash is above 2^63 (idx {1} or {2} or {1,2}).
    let big_seed = "18446744073709551557";
    let pos = positions(&app, s.source_id).await;
    let mut chosen = None;
    for mask in 1..7u32 {
        let misses: Vec<u32> = (0..3).filter(|i| mask & (1 << i) != 0).collect();
        if misses.len() < 3 && wordfall::cascade::order::questions_hash(&misses) > (1u64 << 63) {
            chosen = Some(misses);
            break;
        }
    }
    let misses = chosen.expect("a hash above 2^63 among the subsets");
    let marks: String = pos.iter().map(|i| if misses.contains(&(*i as u32)) { 'M' } else { 'C' }).collect();
    let mut ops = grades(&app, &mut d, s.source_id, 0, &marks).await;
    let (fin, new) = finish_op(&app, &mut d, s.source_id, big_seed).await;
    ops.push(fin.clone());
    let r = d.sync(ops).await;
    let res = result(&r, 3);
    let hash = wordfall::cascade::order::questions_hash(&misses).to_string();
    assert_eq!(res["new_quiz_questions_hash"], hash);
    let (seed, h): (i64, i64) = sqlx::query_as("SELECT shuffle_seed, questions_hash FROM quizzes WHERE id = $1")
        .bind(new)
        .fetch_one(app.db())
        .await
        .unwrap();
    assert!(seed < 0 && h < 0, "stored as two's-complement i64");
    // Pulled as unsigned text.
    let pulled = rows(&[r.json()], "quizzes");
    let row = pulled.iter().find(|q| q["id"] == json!(new)).unwrap();
    assert_eq!(row["shuffle_seed"], big_seed);
    assert_eq!(row["questions_hash"], hash);
    // Repeated: the same text.
    assert_eq!(result(&d.sync(vec![fin]).await, 0)["new_quiz_questions_hash"], hash);
    // The same seed as a later grade's attempt_seed is recognised.
    let idx = misses[0];
    let g = d.op("grade", json!({ "quiz_id": new, "attempt": 1, "attempt_seed": big_seed, "question_idx": idx, "grade": "correct" }));
    assert_eq!(result(&d.sync(vec![g]).await, 0)["status"], "applied");
}

#[sqlx::test]
async fn operations_apply_in_device_seq_order_and_a_rejection_does_not_stop_the_batch(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let mut d = device(&app, "alice").await;
    let s = cascade(&app, &mut d, Setup::new(3)).await;
    let q = quiz_now(&app, s.source_id).await;
    let pos = positions(&app, s.source_id).await;
    let g1 = d.op("grade", json!({ "quiz_id": s.source_id, "attempt": 1, "attempt_seed": q.seed, "question_idx": pos[0], "grade": "missed" }));
    let bad = d.op("grade", json!({ "quiz_id": s.source_id, "attempt": 1, "attempt_seed": "1", "question_idx": pos[1], "grade": "correct" }));
    let g2 = d.op("grade", json!({ "quiz_id": s.source_id, "attempt": 1, "attempt_seed": q.seed, "question_idx": pos[0], "grade": "correct" }));
    // Sent out of order: applied in device_seq order, so the later grade wins.
    let r = d.sync(vec![g2, bad, g1]).await;
    let res: Vec<String> = r.json()["results"].as_array().unwrap().iter().map(|x| x["status"].as_str().unwrap().to_owned()).collect();
    assert_eq!(res, ["applied", "rejected", "applied"]);
    assert_eq!(r.json()["results"][1]["reason"], "stale_attempt");
    let g: String = sqlx::query_scalar("SELECT grade::text FROM quiz_questions WHERE quiz_id = $1 AND question_idx = $2")
        .bind(s.source_id)
        .bind(pos[0])
        .fetch_one(app.db())
        .await
        .unwrap();
    assert_eq!(g, "correct");
}

/// "The request-level 400s, each leaving nothing written and no operation recorded".
#[sqlx::test]
async fn request_level_400s(pool: sqlx::PgPool) {
    let app = TestApp::with(pool, &[("SYNC_MAX_OPS", "3"), ("MAX_CASCADES_PER_USER", "2")]).await;
    let mut d = device(&app, "alice").await;
    let ops: Vec<Value> = (0..4).map(|_| d.op("trash_cascade", json!({ "cascade_id": Uuid::new_v4() }))).collect();
    let base = json!({ "device_id": d.id, "app_version": 1, "cursor": d.cursor, "question_rows_for": [] });
    let mut cases = vec![];
    let mut b = base.clone();
    b["ops"] = json!(ops);
    cases.push(("SYNC_MAX_OPS + 1", b));
    let mut b = base.clone();
    b["ops"] = json!([ops[0]]);
    b["page_token"] = json!("x");
    cases.push(("page_token with an operation", b));
    let mut b = base.clone();
    b["question_rows_for"] = json!([Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()]);
    cases.push(("question_rows_for over the cascade limit", b));
    let mut b = base.clone();
    let mut o = ops[0].clone();
    o["device_id"] = json!(d.id);
    b["ops"] = json!([o]);
    cases.push(("operation-level device_id", b));
    let mut b = base.clone();
    b["app_version"] = json!("1.2.3");
    cases.push(("non-numeric app_version", b));
    let mut b = base.clone();
    let mut o = d.op("set_quiz_options", json!({ "quiz_id": Uuid::new_v4(), "segment_size": 5, "colour": "red" }));
    o["x"] = json!(1);
    b["ops"] = json!([o]);
    cases.push(("an options op with another field", b));
    for (what, body) in cases {
        let r = d.raw_sync(body).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{what}");
    }
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM sync_operations").fetch_one(app.db()).await.unwrap();
    assert_eq!(n, 0, "nothing recorded");
    // device_seq 0, and a device_seq reused under another id: invalid, not error, not recorded.
    let mut zero = d.op("trash_cascade", json!({ "cascade_id": Uuid::new_v4() }));
    zero["device_seq"] = json!(0);
    let r = d.sync(vec![zero]).await;
    assert_eq!(result(&r, 0)["reason"], "invalid");
    let first = d.op("trash_cascade", json!({ "cascade_id": Uuid::new_v4() }));
    let mut reuse = d.op("trash_cascade", json!({ "cascade_id": Uuid::new_v4() }));
    reuse["device_seq"] = first["device_seq"].clone();
    d.sync(vec![first]).await;
    let r = d.sync(vec![reuse]).await;
    assert_eq!(result(&r, 0)["reason"], "invalid");
    let errors: i64 = sqlx::query_scalar("SELECT count(*) FROM sync_operations WHERE reason = 'error'").fetch_one(app.db()).await.unwrap();
    assert_eq!(errors, 0);
}

/// "An operation id already recorded for another user is rejected as not found."
#[sqlx::test]
async fn an_id_recorded_for_another_user_is_not_found(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let mut a = device(&app, "alice").await;
    let mut b = device(&app, "bob").await;
    let s = cascade(&app, &mut a, Setup::new(3)).await;
    let op = a.op("trash_cascade", json!({ "cascade_id": s.cascade_id }));
    assert_eq!(result(&a.sync(vec![op.clone()]).await, 0)["status"], "applied");
    let mut stolen = b.op("trash_cascade", json!({ "cascade_id": s.cascade_id }));
    stolen["id"] = op["id"].clone();
    assert_eq!(result(&b.sync(vec![stolen]).await, 0)["reason"], "not_found");
}

/// The device mark: records of result-free operations below it go; a stale
/// resend below it with no record is answered applied and not applied again.
#[sqlx::test]
async fn the_acknowledged_mark(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let mut d = device(&app, "alice").await;
    let s = cascade(&app, &mut d, Setup::new(3)).await;
    let q = quiz_now(&app, s.source_id).await;
    let pos = positions(&app, s.source_id).await;
    let old = d.op("grade", json!({ "quiz_id": s.source_id, "attempt": 1, "attempt_seed": q.seed, "question_idx": pos[0], "grade": "missed" }));
    d.sync(vec![old.clone()]).await;
    let newer = d.op("grade", json!({ "quiz_id": s.source_id, "attempt": 1, "attempt_seed": q.seed, "question_idx": pos[0], "grade": "correct" }));
    d.sync(vec![newer]).await;
    let kept: i64 = sqlx::query_scalar("SELECT count(*) FROM sync_operations").fetch_one(app.db()).await.unwrap();
    assert_eq!(kept, 1, "the first grade's record went once the second batch proved it acknowledged");
    // A stale resend of the older grade: applied, and not applied again.
    let r = d.sync(vec![old]).await;
    assert_eq!(result(&r, 0)["status"], "applied");
    let g: String = sqlx::query_scalar("SELECT grade::text FROM quiz_questions WHERE quiz_id = $1 AND question_idx = $2")
        .bind(s.source_id)
        .bind(pos[0])
        .fetch_one(app.db())
        .await
        .unwrap();
    assert_eq!(g, "correct", "the newer grade stands");
    // Result-bearing records stay.
    let mut ops = grades(&app, &mut d, s.source_id, 1, "CC").await;
    let (fin, _) = finish_op(&app, &mut d, s.source_id, "5").await;
    ops.push(fin);
    d.sync(ops).await;
    let x = d.op("trash_cascade", json!({ "cascade_id": s.cascade_id }));
    d.sync(vec![x]).await;
    let types: Vec<String> = sqlx::query_scalar("SELECT op_type::text FROM sync_operations ORDER BY device_seq")
        .fetch_all(app.db())
        .await
        .unwrap();
    assert_eq!(types, ["finish", "trash_cascade"]);
}

/// "A sync whose app_version is below MIN_APP_VERSION gets 426 with its
/// operations applied and their results, while the build number equal to it
/// syncs normally; an operation whose seen_seq is above sync_seq is invalid;
/// trash on a trashed cascade, restore or purge of an active cascade and a
/// purge_quiz on a purged quiz are trashed, not_trashed, not_trashed and not_found."
#[sqlx::test]
async fn app_versions_and_simple_rejections(pool: sqlx::PgPool) {
    let app = TestApp::with(pool, &[("MIN_APP_VERSION", "7"), ("SYNC_RATE_PER_MINUTE", "100000")]).await;
    let mut d = device(&app, "alice").await;
    d.app_version = 6;
    let s = cascade(&app, &mut d, Setup::new(3)).await;
    let op = d.op("trash_cascade", json!({ "cascade_id": s.cascade_id }));
    let r = d.sync(vec![op]).await;
    assert_eq!(r.status, StatusCode::UPGRADE_REQUIRED);
    assert_eq!(result(&r, 0)["status"], "applied");
    let trashed: bool = sqlx::query_scalar("SELECT trashed_at IS NOT NULL FROM cascades").fetch_one(app.db()).await.unwrap();
    assert!(trashed);
    d.app_version = 7;
    assert_eq!(d.sync(vec![]).await.status, StatusCode::OK);
    let mut ahead = d.op("restore_cascade", json!({ "cascade_id": s.cascade_id }));
    ahead["seen_seq"] = json!(d.cursor.unwrap() + 10);
    assert_eq!(result(&d.sync(vec![ahead]).await, 0)["reason"], "invalid");
    let t = d.op("trash_cascade", json!({ "cascade_id": s.cascade_id }));
    assert_eq!(result(&d.sync(vec![t]).await, 0)["reason"], "trashed");
    let r = d.op("restore_cascade", json!({ "cascade_id": s.cascade_id }));
    d.sync(vec![r]).await;
    let r = d.op("restore_cascade", json!({ "cascade_id": s.cascade_id }));
    assert_eq!(result(&d.sync(vec![r]).await, 0)["reason"], "not_trashed");
    let p = d.op("purge_cascade", json!({ "cascade_id": s.cascade_id }));
    assert_eq!(result(&d.sync(vec![p]).await, 0)["reason"], "not_trashed");
    let p = d.op("purge_quiz", json!({ "quiz_id": Uuid::new_v4() }));
    assert_eq!(result(&d.sync(vec![p]).await, 0)["reason"], "not_found");
}

/// Invalid option, preference and binding values are `invalid` before any
/// write: the row unchanged and nothing recorded as `error`.
#[sqlx::test]
async fn out_of_range_values_are_invalid_never_error(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let mut d = device(&app, "alice").await;
    let s = cascade(&app, &mut d, Setup::new(10)).await;
    let bindings = |code: &str, kind: &str| json!([
        { "action": "show_next", "kind": kind, "code": code },
        { "action": "toggle_grade", "kind": "key", "code": "KeyX" },
        { "action": "previous", "kind": "key", "code": "Backspace" }]);
    let ops = vec![
        d.op("set_cascade_options", json!({ "cascade_id": s.cascade_id, "segment_size": 3 })),
        d.op("set_quiz_options", json!({ "quiz_id": s.source_id, "segment_size": 300001 })),
        d.op("set_quiz_options", json!({ "quiz_id": s.source_id, "segment_size": 1 })),
        d.op("set_cascade_options", json!({ "cascade_id": s.cascade_id, "progression": "spiral" })),
        d.op("set_preferences", json!({ "default_clear_threshold": 0 })),
        d.op("set_preferences", json!({ "leave_value_decimals": 4 })),
        d.op("set_bindings", json!({ "bindings": bindings("Escape", "key") })),
        d.op("set_bindings", json!({ "bindings": bindings("thumb", "mouse_button") })),
        d.op("set_bindings", json!({ "bindings": [
            { "action": "show_next", "kind": "key", "code": "Space" }, { "action": "show_next", "kind": "key", "code": "KeyA" },
            { "action": "show_next", "kind": "key", "code": "KeyB" }, { "action": "show_next", "kind": "key", "code": "KeyC" },
            { "action": "toggle_grade", "kind": "key", "code": "KeyX" }, { "action": "previous", "kind": "key", "code": "Backspace" }] })),
    ];
    let r = d.sync(ops).await;
    for x in r.json()["results"].as_array().unwrap() {
        assert_eq!(x["reason"], "invalid", "{x}");
    }
    let (seg, thr): (i32, i16) = sqlx::query_as(
        "SELECT c.segment_size, p.default_clear_threshold FROM cascades c JOIN user_preferences p ON p.user_id = c.user_id",
    )
    .fetch_one(app.db())
    .await
    .unwrap();
    assert_eq!((seg, thr), (0, 80));
    let errors: i64 = sqlx::query_scalar("SELECT count(*) FROM sync_operations WHERE reason = 'error'").fetch_one(app.db()).await.unwrap();
    assert_eq!(errors, 0);
    // The schema refuses such a row directly.
    let r = sqlx::query("UPDATE quizzes SET segment_size = 4 WHERE id = $1").bind(s.source_id).execute(app.db()).await;
    assert!(r.is_err());
}

/// "`set_quiz_options` naming progression or segment_size for a segment-chain
/// quiz, or progression for the Source quiz, is rejected as invalid".
#[sqlx::test]
async fn chain_and_source_options(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let mut d = device(&app, "alice").await;
    let mut setup = Setup::new(10);
    setup.segment_size = 5;
    let s = cascade(&app, &mut d, setup).await;
    let mut ops = grades(&app, &mut d, s.source_id, 0, "CCCCM").await;
    let q = quiz_now(&app, s.source_id).await;
    let chain = Uuid::new_v4();
    ops.push(d.op("finish_segment", json!({ "quiz_id": s.source_id, "attempt": 1, "attempt_seed": q.seed, "segment_end": 5,
                                              "shuffle_seed": "77", "new_quiz_id": chain })));
    ops.push(d.op("set_quiz_options", json!({ "quiz_id": chain, "progression": "ladder" })));
    ops.push(d.op("set_quiz_options", json!({ "quiz_id": chain, "segment_size": 5 })));
    ops.push(d.op("set_quiz_options", json!({ "quiz_id": s.source_id, "progression": "drill" })));
    ops.push(d.op("set_quiz_options", json!({ "quiz_id": chain, "require_alphabetical": true })));
    let r = d.sync(ops).await;
    let reasons: Vec<Value> = r.json()["results"].as_array().unwrap()[6..].iter().map(|x| x["reason"].clone()).collect();
    assert_eq!(reasons, [json!("invalid"), json!("invalid"), json!("invalid"), Value::Null]);
}

/// finish_segment refusals, and the race for one boundary.
#[sqlx::test]
async fn finish_segment_rules_and_races(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let mut a = device(&app, "alice").await;
    let mut setup = Setup::new(20);
    setup.segment_size = 5;
    let s = cascade(&app, &mut a, setup).await;
    let mut b = a.sibling(&app);
    b.qrf = a.qrf.clone();
    b.sync(vec![]).await;
    let q = quiz_now(&app, s.source_id).await;
    let fs = |d: &mut Device<'_>, end: i64, new: Uuid| {
        d.op("finish_segment", json!({ "quiz_id": s.source_id, "attempt": 1, "attempt_seed": q.seed, "segment_end": end,
                                        "shuffle_seed": "99", "new_quiz_id": new }))
    };
    // Ungraded before the end, not a multiple, at the count.
    let ops = vec![fs(&mut a, 5, Uuid::new_v4()), fs(&mut a, 7, Uuid::new_v4()), fs(&mut a, 20, Uuid::new_v4())];
    let r = a.sync(ops).await;
    let reasons: Vec<Value> = r.json()["results"].as_array().unwrap().iter().map(|x| x["reason"].clone()).collect();
    assert_eq!(reasons, [json!("ungraded"), json!("bad_segment"), json!("bad_segment")]);
    // Both devices grade the run with a miss and race on the boundary.
    let ga = grades(&app, &mut a, s.source_id, 0, "CCCCM").await;
    a.sync(ga).await;
    let o = fs(&mut a, 5, Uuid::new_v4());
    let ra = a.sync(vec![o.clone()]).await;
    assert_eq!(result(&ra, 0)["outcome"], "drilled");
    // A repeated finish_segment is applied once, with the same result.
    assert_eq!(result(&a.sync(vec![o]).await, 0), result(&ra, 0));
    let o = fs(&mut b, 5, Uuid::new_v4());
    let rb = b.sync(vec![o]).await;
    assert_eq!(result(&rb, 0)["reason"], "duplicate_segment");
    let drills: i64 = sqlx::query_scalar("SELECT count(*) FROM quizzes WHERE origin = 'segment'").fetch_one(app.db()).await.unwrap();
    assert_eq!(drills, 1);
    // A run that missed nothing: the loser meets the cursor test, not a duplicate.
    let chain: Uuid = sqlx::query_scalar("SELECT id FROM quizzes WHERE origin = 'segment'").fetch_one(app.db()).await.unwrap();
    let ops = grades(&app, &mut a, chain, 0, "C").await;
    a.sync(ops).await;
    let (f, _) = finish_op(&app, &mut a, chain, "3").await;
    a.sync(vec![f]).await;
    let g = grades(&app, &mut a, s.source_id, 5, "CCCCC").await;
    a.sync(g).await;
    let o = fs(&mut a, 10, Uuid::new_v4());
    let win = a.sync(vec![o]).await;
    assert_eq!(result(&win, 0)["outcome"], "continued");
    let o = fs(&mut b, 10, Uuid::new_v4());
    let lose = b.sync(vec![o]).await;
    assert_eq!(result(&lose, 0)["reason"], "bad_segment");
}

/// A grade on a question of a run another device has passed is applied; that
/// device's cursor move inside the passed run is bad_cursor and its
/// finish_segment for the boundary duplicate_segment.
#[sqlx::test]
async fn grading_a_passed_run(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let mut a = device(&app, "alice").await;
    let mut setup = Setup::new(10);
    setup.segment_size = 5;
    let s = cascade(&app, &mut a, setup).await;
    let mut b = a.sibling(&app);
    b.sync(vec![]).await;
    let q = quiz_now(&app, s.source_id).await;
    let mut ops = grades(&app, &mut a, s.source_id, 0, "CCCCM").await;
    ops.push(a.op("finish_segment", json!({ "quiz_id": s.source_id, "attempt": 1, "attempt_seed": q.seed, "segment_end": 5,
                                             "shuffle_seed": "8", "new_quiz_id": Uuid::new_v4() })));
    a.sync(ops).await;
    let before: (i32, i32) = sqlx::query_as("SELECT correct_count, missed_count FROM quizzes WHERE id = $1")
        .bind(s.source_id)
        .fetch_one(app.db())
        .await
        .unwrap();
    let pos = positions(&app, s.source_id).await;
    let g = b.op("grade", json!({ "quiz_id": s.source_id, "attempt": 1, "attempt_seed": q.seed, "question_idx": pos[4], "grade": "correct" }));
    let m = b.op("move_cursor", json!({ "quiz_id": s.source_id, "attempt": 1, "attempt_seed": q.seed, "position": 3 }));
    let f = b.op("finish_segment", json!({ "quiz_id": s.source_id, "attempt": 1, "attempt_seed": q.seed, "segment_end": 5,
                                           "shuffle_seed": "9", "new_quiz_id": Uuid::new_v4() }));
    let r = b.sync(vec![g, m, f]).await;
    let got: Vec<(Value, Value)> = r.json()["results"].as_array().unwrap().iter().map(|x| (x["status"].clone(), x["reason"].clone())).collect();
    assert_eq!(got, [(json!("applied"), Value::Null), (json!("rejected"), json!("bad_cursor")), (json!("rejected"), json!("duplicate_segment"))]);
    let after: (i32, i32) = sqlx::query_as("SELECT correct_count, missed_count FROM quizzes WHERE id = $1")
        .bind(s.source_id)
        .fetch_one(app.db())
        .await
        .unwrap();
    assert_eq!(after, (before.0 + 1, before.1 - 1), "the attempt's counters move");
    let drills: i64 = sqlx::query_scalar("SELECT count(*) FROM quizzes WHERE origin = 'segment'").fetch_one(app.db()).await.unwrap();
    assert_eq!(drills, 1);
}

/// move_cursor bounds, and a device_seq gap left by a coalesced cursor move.
#[sqlx::test]
async fn cursor_bounds_and_gaps(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let mut d = device(&app, "alice").await;
    let mut setup = Setup::new(10);
    setup.segment_size = 5;
    let s = cascade(&app, &mut d, setup).await;
    let q = quiz_now(&app, s.source_id).await;
    let mv = |d: &mut Device<'_>, p: i64| d.op("move_cursor", json!({ "quiz_id": s.source_id, "attempt": 1, "attempt_seed": q.seed, "position": p }));
    let first = mv(&mut d, 1);
    d.next_seq += 3; // a gap
    let at_boundary = mv(&mut d, 5);
    let r = d.sync(vec![first, at_boundary]).await;
    assert_eq!(result(&r, 0)["status"], "applied");
    assert_eq!(result(&r, 1)["reason"], "bad_cursor");
    // With no segments, at the question count.
    let o = d.op("set_quiz_options", json!({ "quiz_id": s.source_id, "segment_size": 0 }));
    d.sync(vec![o]).await;
    let ops = vec![mv(&mut d, 10), mv(&mut d, 9)];
    let r = d.sync(ops).await;
    assert_eq!(result(&r, 0)["reason"], "bad_cursor");
    assert_eq!(result(&r, 1)["status"], "applied");
    // A grade for a question not in the quiz is not found.
    let g = d.op("grade", json!({ "quiz_id": s.source_id, "attempt": 1, "attempt_seed": q.seed, "question_idx": 99, "grade": "correct" }));
    assert_eq!(result(&d.sync(vec![g]).await, 0)["reason"], "not_found");
}

/// "`finish` is rejected while a question is ungraded, and applied with an
/// outcome that differs from the sender's when another device's grade moved
/// the score across the threshold"; a second finish after one that cleared
/// the quiz is not_active and after one that reset it stale_attempt.
#[sqlx::test]
async fn finishes_across_devices(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let mut a = device(&app, "alice").await;
    let s = cascade(&app, &mut a, Setup::new(5)).await;
    let mut b = a.sibling(&app);
    b.sync(vec![]).await;
    let (f, _) = finish_op(&app, &mut a, s.source_id, "11").await;
    assert_eq!(result(&a.sync(vec![f]).await, 0)["reason"], "ungraded");
    // Level 1 descends with two misses.
    let mut ops = grades(&app, &mut a, s.source_id, 0, "CCCMM").await;
    let (f, l2) = finish_op(&app, &mut a, s.source_id, "12").await;
    ops.push(f);
    a.sync(ops).await;
    b.sync(vec![]).await;
    // Device A grades Level 2 all correct (it would clear); device B, which
    // had seen A's grades, regrades one missed.
    let ga = grades(&app, &mut a, l2, 0, "CC").await;
    a.sync(ga).await;
    b.sync(vec![]).await;
    let q2 = quiz_now(&app, l2).await;
    let pos = positions(&app, l2).await;
    let gb = b.op("grade", json!({ "quiz_id": l2, "attempt": q2.attempt, "attempt_seed": q2.seed, "question_idx": pos[0], "grade": "missed" }));
    b.sync(vec![gb]).await;
    // A's finish expected "cleared"; the server's grades make it a descent.
    let (fa, _) = finish_op(&app, &mut a, l2, "13").await;
    let r = a.sync(vec![fa.clone()]).await;
    assert_eq!(result(&r, 0)["outcome"], "descended");
    // A second finish on the reset quiz, naming the old attempt: stale_attempt.
    let mut o = fa.clone();
    o["id"] = json!(Uuid::new_v4());
    o["device_seq"] = json!(b.next_seq);
    b.next_seq += 1;
    o["new_quiz_id"] = json!(Uuid::new_v4());
    let r = b.sync(vec![o]).await;
    assert_eq!(result(&r, 0)["reason"], "stale_attempt");
}

/// "Options and preference operations resolve whole", timestamps and seen_seq,
/// clamping, and a device's own changes never competing on timestamps.
#[sqlx::test]
async fn latest_wins_whole(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let mut a = device(&app, "alice").await;
    let s = cascade(&app, &mut a, Setup::new(10)).await;
    let mut b = a.sibling(&app);
    b.sync(vec![]).await;
    let t0 = Utc::now();
    // Interleaved: A changes the segment size at t0+1s, B the progression at t0+2s,
    // but A's arrives second with the earlier time.
    let ob = b.op_at("set_cascade_options", json!({ "cascade_id": s.cascade_id, "progression": "drill" }), t0 + Duration::seconds(2));
    let oa = a.op_at("set_cascade_options", json!({ "cascade_id": s.cascade_id, "segment_size": 5 }), t0 + Duration::seconds(1));
    assert_eq!(result(&b.sync(vec![ob]).await, 0)["status"], "applied");
    // A had not seen B's change (its cursor is older): the earlier time loses whole.
    assert_eq!(result(&a.sync(vec![oa]).await, 0)["reason"], "stale");
    let (seg, prog): (i32, String) = sqlx::query_as("SELECT segment_size, progression::text FROM cascades").fetch_one(app.db()).await.unwrap();
    assert_eq!((seg, prog.as_str()), (0, "drill"));
    // Having seen it (seen_seq), an older timestamp still applies.
    let oa = a.op_at("set_cascade_options", json!({ "cascade_id": s.cascade_id, "segment_size": 5 }), t0);
    assert_eq!(result(&a.sync(vec![oa]).await, 0)["status"], "applied");
    // Preferences likewise.
    let pb = b.op_at("set_preferences", json!({ "default_clear_threshold": 60 }), t0 + Duration::seconds(5));
    b.sync(vec![pb]).await;
    let mut stale = a.op_at("set_preferences", json!({ "leave_value_decimals": 2 }), t0 + Duration::seconds(4));
    stale["seen_seq"] = json!(0);
    assert_eq!(result(&a.sync(vec![stale]).await, 0)["reason"], "stale");
    // Two changes from one device with a clock a day ahead, in one batch: both applied.
    let mut fast = b.sibling(&app);
    fast.skew = Duration::days(1);
    let o1 = fast.op("set_quiz_options", json!({ "quiz_id": s.source_id, "segment_size": 5 }));
    let o2 = fast.op("set_quiz_options", json!({ "quiz_id": s.source_id, "segment_size": 6 }));
    let p1 = fast.op("set_preferences", json!({ "default_clear_threshold": 70 }));
    let p2 = fast.op("set_preferences", json!({ "default_clear_threshold": 71 }));
    let r = fast.sync(vec![o1, o2, p1, p2]).await;
    for i in 0..4 {
        assert_eq!(result(&r, i)["status"], "applied", "{i}");
    }
    let clamped: bool = sqlx::query_scalar("SELECT options_changed_at <= now() + interval '5 minutes' FROM quizzes WHERE id = $1")
        .bind(s.source_id)
        .fetch_one(app.db())
        .await
        .unwrap();
    assert!(clamped, "stored at least(at, now() + 5 minutes)");
}

/// Grades between devices: the first grade applies; a device's own regrade
/// with an earlier graded_at applies; another device's earlier grade is
/// stale unless its seen_seq covers the current grade; a grade a year ahead
/// is stored at now() + 5 minutes and loses to a real grade ten minutes
/// later; a grade's `at` is stored as graded_at.
#[sqlx::test]
async fn grade_conflicts(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let mut a = device(&app, "alice").await;
    let s = cascade(&app, &mut a, Setup::new(4)).await;
    let mut b = a.sibling(&app);
    b.sync(vec![]).await;
    let q = quiz_now(&app, s.source_id).await;
    let pos = positions(&app, s.source_id).await;
    let now = Utc::now();
    let g = |d: &mut Device<'_>, i: usize, grade: &str, at| {
        d.op_at("grade", json!({ "quiz_id": s.source_id, "attempt": 1, "attempt_seed": q.seed, "question_idx": pos[i], "grade": grade }), at)
    };
    let o = g(&mut a, 0, "correct", now);
    assert_eq!(result(&a.sync(vec![o]).await, 0)["status"], "applied");
    let stored: chrono::DateTime<Utc> = sqlx::query_scalar("SELECT graded_at FROM quiz_questions WHERE quiz_id = $1 AND question_idx = $2")
        .bind(s.source_id)
        .bind(pos[0])
        .fetch_one(app.db())
        .await
        .unwrap();
    assert!((stored - now).num_milliseconds().abs() < 2, "at is stored as graded_at");
    // Own regrade, earlier time: applies.
    let o = g(&mut a, 0, "missed", now - Duration::minutes(1));
    assert_eq!(result(&a.sync(vec![o]).await, 0)["status"], "applied");
    // B had not seen it, and is earlier: stale.
    let mut early = g(&mut b, 0, "correct", now - Duration::minutes(2));
    early["seen_seq"] = json!(0);
    assert_eq!(result(&b.sync(vec![early]).await, 0)["reason"], "stale");
    // B, having pulled (seen_seq covers it), earlier time: applies.
    b.sync(vec![]).await;
    let o = g(&mut b, 0, "correct", now - Duration::minutes(3));
    assert_eq!(result(&b.sync(vec![o]).await, 0)["status"], "applied");
    // A year ahead from A (not having seen): stored at now + 5 min, loses to a real grade 10 min later.
    let mut ahead = g(&mut a, 1, "missed", now + Duration::days(365));
    ahead["seen_seq"] = json!(0);
    a.sync(vec![ahead]).await;
    let mut later = g(&mut b, 1, "correct", now + Duration::minutes(10));
    later["seen_seq"] = json!(0);
    assert_eq!(result(&b.sync(vec![later]).await, 0)["status"], "applied");
}

/// Account binding on sync: Bob's cookie naming Alice, or no header: 401 with
/// nothing applied, recorded or pulled.
#[sqlx::test]
async fn sync_needs_the_account_binding(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let alice = app.signed_in("alice").await;
    let mut d = device(&app, "bob").await;
    let s = cascade(&app, &mut d, Setup::new(3)).await;
    let op = d.op("trash_cascade", json!({ "cascade_id": s.cascade_id }));
    let bob = d.client.user_id;
    d.client.user_id = alice.user_id;
    assert_eq!(d.sync(vec![op.clone()]).await.status, StatusCode::UNAUTHORIZED);
    d.client.send_user_header = false;
    assert_eq!(d.sync(vec![op.clone()]).await.status, StatusCode::UNAUTHORIZED);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM sync_operations").fetch_one(app.db()).await.unwrap();
    assert_eq!(n, 0);
    d.client.send_user_header = true;
    d.client.user_id = bob;
    assert_eq!(result(&d.sync(vec![op]).await, 0)["status"], "applied");
}

/// One user cannot reach another's cascades through sync or REST.
#[sqlx::test]
async fn another_users_rows_are_out_of_reach(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let mut a = device(&app, "alice").await;
    let s = cascade(&app, &mut a, Setup::new(3)).await;
    let mut b = device(&app, "bob").await;
    b.qrf = vec![s.cascade_id];
    let q = quiz_now(&app, s.source_id).await;
    let ops = vec![
        b.op("grade", json!({ "quiz_id": s.source_id, "attempt": 1, "attempt_seed": q.seed, "question_idx": 0, "grade": "correct" })),
        b.op("trash_cascade", json!({ "cascade_id": s.cascade_id })),
        b.op("set_cascade_options", json!({ "cascade_id": s.cascade_id, "segment_size": 5 })),
    ];
    let (res, pages) = b.sync_all(ops).await;
    assert!(res.iter().all(|r| r["reason"] == "not_found"));
    assert!(rows(&pages, "cascades").is_empty() && rows(&pages, "quizzes").is_empty());
    for path in [
        format!("/api/cascades/{}/cards?from=0&limit=5", s.cascade_id),
        format!("/api/cascades/{}/quizzes/{}/grades?from=0&limit=5", s.cascade_id, s.source_id),
    ] {
        assert_eq!(b.client.get(&path).await.status, StatusCode::NOT_FOUND, "{path}");
    }
}

/// "The first grade on an ungraded question and the first move_cursor on a fresh quiz are applied."
#[sqlx::test]
async fn first_grades_and_moves(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let mut d = device(&app, "alice").await;
    let s = cascade(&app, &mut d, Setup::new(3)).await;
    let q = quiz_now(&app, s.source_id).await;
    let pos = positions(&app, s.source_id).await;
    let mut g = d.op("grade", json!({ "quiz_id": s.source_id, "attempt": 1, "attempt_seed": q.seed, "question_idx": pos[0], "grade": "correct" }));
    g["seen_seq"] = json!(0);
    let mut m = d.op("move_cursor", json!({ "quiz_id": s.source_id, "attempt": 1, "attempt_seed": q.seed, "position": 1 }));
    m["seen_seq"] = json!(0);
    let r = d.sync(vec![g, m]).await;
    assert_eq!(result(&r, 0)["status"], "applied");
    assert_eq!(result(&r, 1)["status"], "applied");
    // A device-created quiz id that already exists is rejected.
    let mut ops = grades(&app, &mut d, s.source_id, 1, "CM").await;
    let (mut f, _) = finish_op(&app, &mut d, s.source_id, "4").await;
    f["new_quiz_id"] = json!(s.source_id);
    ops.push(f);
    let r = d.sync(ops).await;
    assert_eq!(result(&r, 2)["reason"], "invalid");
}

/// "an options change seconds after creation from a device whose clock is
/// behind the server is applied, not stale."
#[sqlx::test]
async fn an_options_change_right_after_creation_from_a_slow_clock(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let c = app.seed_fixture_catalog("root").await;
    let mut d = Device::new(c);
    d.skew = -Duration::minutes(10);
    let id = Uuid::new_v4();
    let at = d.now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let r = d
        .client
        .post("/api/cascades", json!({
            "id": id, "source_quiz_id": Uuid::new_v4(), "device_id": d.id, "at": at, "name": "A cascade",
            "lexicon": "EN-FIX", "quiz_type": "anagram", "clear_threshold": 80, "segment_size": 0,
            "progression": "ladder", "require_alphabetical": false,
            "filters": { "op": "and", "children": [{ "type": "length", "negated": false, "min": 7, "max": 7 }] },
        }))
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.json());
    d.skew = -Duration::minutes(10) + Duration::seconds(3);
    let o = d.op("set_cascade_options", json!({ "cascade_id": id, "segment_size": 5 }));
    // The device has not pulled since creating it.
    let mut o = o;
    o["seen_seq"] = json!(0);
    assert_eq!(common::sync::one(&mut d, o).await, "applied");
}
