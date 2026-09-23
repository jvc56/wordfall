//! PLAN.md § Integration tests → Sync integration tests: "Two simulated
//! devices cover every row of the Conflicts table" (PLAN.md § Conflicts).
//! Rows whose cases live in tests/sync.rs say so.

mod common;

use chrono::{Duration, Utc};
use common::sync::{
    finish_op, grades, insert_cascade, one, play_and_finish, quiz_now, result, rows, Device, Setup,
};
use common::TestApp;
use serde_json::json;
use uuid::Uuid;

async fn app(pool: sqlx::PgPool) -> TestApp {
    TestApp::with(pool, &[("SYNC_RATE_PER_MINUTE", "100000")]).await
}

/// Two devices of one account, both synced, and a cascade.
async fn two<'a>(app: &'a TestApp, s: Setup) -> (Device<'a>, Device<'a>, Setup) {
    let c = app.signed_in("alice").await;
    insert_cascade(app, c.user_id.unwrap(), &s).await;
    let mut a = Device::new(c);
    a.qrf = vec![s.cascade_id];
    a.sync(vec![]).await;
    let mut b = a.sibling(app);
    b.qrf = vec![s.cascade_id];
    b.sync(vec![]).await;
    (a, b, s)
}

async fn restore(d: &mut Device<'_>, quiz: Uuid) -> String {
    let o = d.op("restore_quiz", json!({ "quiz_id": quiz, "shuffle_seed": "31337" }));
    one(d, o).await
}

async fn level_and_depth(app: &TestApp, quiz: Uuid) -> (i32, i32, String) {
    sqlx::query_as("SELECT q.level, c.depth, q.status::text FROM quizzes q JOIN cascades c ON c.id = q.cascade_id WHERE q.id = $1")
        .bind(quiz)
        .fetch_one(app.db())
        .await
        .unwrap()
}

/// A cascade whose Source quiz has descended once and whose Level 2 is
/// cleared: returns the cleared Level 2's id.
async fn with_cleared_level_2(app: &TestApp, d: &mut Device<'_>, s: &Setup) -> Uuid {
    let (r, l2) = play_and_finish(app, d, s.source_id, "CCCCM").await;
    assert_eq!(r["outcome"], "descended");
    let (r, _) = play_and_finish(app, d, l2, "C").await;
    assert_eq!(r["outcome"], "cleared");
    l2
}

// Row: the same question graded on two devices — tests/sync.rs grade_conflicts.

/// Row: the same quiz finished on two devices.
#[sqlx::test]
async fn the_same_quiz_finished_on_two_devices(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let (mut a, mut b, s) = two(&app, Setup::new(5)).await;
    // Reset by the first finish: stale_attempt.
    let gb = grades(&app, &mut b, s.source_id, 0, "CCCCM").await;
    let (fb, _) = finish_op(&app, &mut b, s.source_id, "5").await;
    let (r, l2) = play_and_finish(&app, &mut a, s.source_id, "CCCCM").await;
    assert_eq!(r["outcome"], "descended");
    let rb = b.sync([gb, vec![fb]].concat()).await;
    assert_eq!(result(&rb, 5)["reason"], "stale_attempt");
    // Cleared by the first finish: not_active.
    b.sync(vec![]).await;
    let gb = grades(&app, &mut b, l2, 0, "C").await;
    let (fb, _) = finish_op(&app, &mut b, l2, "6").await;
    let (r, _) = play_and_finish(&app, &mut a, l2, "C").await;
    assert_eq!(r["outcome"], "cleared");
    let rb = b.sync([gb, vec![fb]].concat()).await;
    assert_eq!(result(&rb, 0)["reason"], "not_active");
    assert_eq!(result(&rb, 1)["reason"], "not_active");
}

/// Row: a quiz restored on one device and purged on another — the first wins.
#[sqlx::test]
async fn restored_on_one_device_purged_on_another(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let (mut a, mut b, s) = two(&app, Setup::new(5)).await;
    let l2 = with_cleared_level_2(&app, &mut a, &s).await;
    b.sync(vec![]).await;
    assert_eq!(restore(&mut a, l2).await, "applied");
    let p = b.op("purge_quiz", json!({ "quiz_id": l2 }));
    assert_eq!(one(&mut b, p).await, "not_cleared");
    // The other order, on a second cleared quiz.
    let (r, _) = play_and_finish(&app, &mut a, l2, "C").await;
    assert_eq!(r["outcome"], "cleared");
    b.sync(vec![]).await;
    let p = b.op("purge_quiz", json!({ "quiz_id": l2 }));
    assert_eq!(one(&mut b, p).await, "applied");
    assert_eq!(restore(&mut a, l2).await, "not_found");
}

/// Row: the same quiz restored on two devices — the second is not_cleared,
/// and its grades on the attempt it made locally are stale.
#[sqlx::test]
async fn the_same_quiz_restored_on_two_devices(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let (mut a, mut b, s) = two(&app, Setup::new(5)).await;
    let l2 = with_cleared_level_2(&app, &mut a, &s).await;
    b.sync(vec![]).await;
    assert_eq!(restore(&mut a, l2).await, "applied");
    assert_eq!(restore(&mut b, l2).await, "not_cleared");
    let q = quiz_now(&app, l2).await;
    let g = b.op("grade", json!({ "quiz_id": l2, "attempt": q.attempt, "attempt_seed": "424242", "question_idx": 4, "grade": "correct" }));
    assert_eq!(one(&mut b, g).await, "stale_attempt");
}

/// Row: grades arriving for a quiz since cleared (not_active) or reset (stale_attempt).
#[sqlx::test]
async fn grades_for_a_cleared_or_reset_quiz(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let (mut a, mut b, s) = two(&app, Setup::new(5)).await;
    let late_reset = grades(&app, &mut b, s.source_id, 0, "CC").await;
    let (_, l2) = play_and_finish(&app, &mut a, s.source_id, "CCCCM").await;
    let r = b.sync(late_reset).await;
    assert_eq!(result(&r, 0)["reason"], "stale_attempt");
    b.sync(vec![]).await;
    let late_clear = grades(&app, &mut b, l2, 0, "M").await;
    play_and_finish(&app, &mut a, l2, "C").await;
    let r = b.sync(late_clear).await;
    assert_eq!(result(&r, 0)["reason"], "not_active");
}

// Rows: the same run finished on two devices, and a passed run regraded —
// tests/sync.rs finish_segment_rules_and_races and grading_a_passed_run.

/// Row: a cascade trashed on one device while the other keeps studying it.
#[sqlx::test]
async fn trashed_while_the_other_studies(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let (mut a, mut b, s) = two(&app, Setup::new(5)).await;
    let t = a.op("trash_cascade", json!({ "cascade_id": s.cascade_id }));
    assert_eq!(one(&mut a, t).await, "applied");
    let q = quiz_now(&app, s.source_id).await;
    let mut ops = grades(&app, &mut b, s.source_id, 0, "CCCCM").await;
    ops.push(b.op("move_cursor", json!({ "quiz_id": s.source_id, "attempt": q.attempt, "attempt_seed": q.seed, "position": 1 })));
    let (f, _) = finish_op(&app, &mut b, s.source_id, "9").await;
    ops.push(f);
    ops.push(b.op("set_quiz_options", json!({ "quiz_id": s.source_id, "segment_size": 5 })));
    ops.push(b.op("set_cascade_options", json!({ "cascade_id": s.cascade_id, "segment_size": 5 })));
    let r = b.sync(ops).await;
    for x in r.json()["results"].as_array().unwrap() {
        assert_eq!(x["reason"], "trashed", "{x}");
    }
}

/// Row: a cascade trashed on one device while another restores one of its quizzes.
#[sqlx::test]
async fn trash_and_restore_quiz_in_either_order(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let (mut a, mut b, s) = two(&app, Setup::new(5)).await;
    let l2 = with_cleared_level_2(&app, &mut a, &s).await;
    b.sync(vec![]).await;
    // Trash first: the restore brings the cascade back with the quiz deepest.
    let t = a.op("trash_cascade", json!({ "cascade_id": s.cascade_id }));
    one(&mut a, t).await;
    assert_eq!(restore(&mut b, l2).await, "applied");
    let trashed: bool = sqlx::query_scalar("SELECT trashed_at IS NOT NULL FROM cascades").fetch_one(app.db()).await.unwrap();
    assert!(!trashed);
    let (level, depth, status) = level_and_depth(&app, l2).await;
    assert_eq!((level, status.as_str()), (depth, "active"));
    // Restore first (after clearing again): the trash puts the restored quiz in the Trash with the cascade.
    play_and_finish(&app, &mut a, l2, "C").await;
    b.sync(vec![]).await;
    assert_eq!(restore(&mut b, l2).await, "applied");
    let t = a.op("trash_cascade", json!({ "cascade_id": s.cascade_id }));
    assert_eq!(one(&mut a, t).await, "applied");
    let (trashed, status): (bool, String) = sqlx::query_as(
        "SELECT c.trashed_at IS NOT NULL, q.status::text FROM cascades c JOIN quizzes q ON q.cascade_id = c.id WHERE q.id = $1",
    )
    .bind(l2)
    .fetch_one(app.db())
    .await
    .unwrap();
    assert!(trashed);
    assert_eq!(status, "active");
}

/// Row: the same trash, restore or purge sent from two devices.
#[sqlx::test]
async fn the_same_trash_restore_or_purge_twice(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let (mut a, mut b, s) = two(&app, Setup::new(5)).await;
    let l2 = with_cleared_level_2(&app, &mut a, &s).await;
    b.sync(vec![]).await;
    assert_eq!(restore(&mut a, l2).await, "applied");
    assert_eq!(restore(&mut b, l2).await, "not_cleared");
    play_and_finish(&app, &mut a, l2, "C").await;
    let p = a.op("purge_quiz", json!({ "quiz_id": l2 }));
    assert_eq!(one(&mut a, p).await, "applied");
    let p = b.op("purge_quiz", json!({ "quiz_id": l2 }));
    assert_eq!(one(&mut b, p).await, "not_found");
    for (op, second) in [("trash_cascade", "trashed"), ("restore_cascade", "not_trashed")] {
        let o = a.op(op, json!({ "cascade_id": s.cascade_id }));
        assert_eq!(one(&mut a, o).await, "applied");
        let o = b.op(op, json!({ "cascade_id": s.cascade_id }));
        assert_eq!(one(&mut b, o).await, second);
    }
    let t = a.op("trash_cascade", json!({ "cascade_id": s.cascade_id }));
    one(&mut a, t).await;
    let p = a.op("purge_cascade", json!({ "cascade_id": s.cascade_id }));
    assert_eq!(one(&mut a, p).await, "applied");
    let p = b.op("purge_cascade", json!({ "cascade_id": s.cascade_id }));
    assert_eq!(one(&mut b, p).await, "not_found");
}

/// Row: a cascade purged on one device while the other kept studying it —
/// the other's grades are not_found and the same pull carries the tombstone.
#[sqlx::test]
async fn purged_while_the_other_studied(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let (mut a, mut b, s) = two(&app, Setup::new(5)).await;
    let offline = grades(&app, &mut b, s.source_id, 0, "CCCCM").await;
    let t = a.op("trash_cascade", json!({ "cascade_id": s.cascade_id }));
    let p = a.op("purge_cascade", json!({ "cascade_id": s.cascade_id }));
    a.sync(vec![t, p]).await;
    let (res, pages) = b.sync_all(offline).await;
    assert!(res.iter().all(|x| x["reason"] == "not_found"));
    let tombs = rows(&pages, "tombstones");
    assert!(tombs.iter().any(|t| t["entity"] == "cascade" && t["entity_id"] == json!(s.cascade_id)), "{tombs:?}");
    // And the grades endpoint for the purged cascade's quiz is 404.
    let path = format!("/api/cascades/{}/quizzes/{}/grades?from=0&limit=10", s.cascade_id, s.source_id);
    assert_eq!(b.client.get(&path).await.status, axum::http::StatusCode::NOT_FOUND);
}

/// Row: the same quiz finished on one device and a run of it on another.
#[sqlx::test]
async fn a_finish_and_a_finish_segment(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let mut setup = Setup::new(10);
    setup.segment_size = 5;
    let (mut a, mut b, s) = two(&app, setup).await;
    // finish_segment first: the finish is not_deepest.
    let offline = grades(&app, &mut b, s.source_id, 0, "CCCCMCCCCC").await;
    let (fb, _) = finish_op(&app, &mut b, s.source_id, "5").await;
    let q = quiz_now(&app, s.source_id).await;
    let mut ops = grades(&app, &mut a, s.source_id, 0, "CCCCM").await;
    ops.push(a.op("finish_segment", json!({ "quiz_id": s.source_id, "attempt": q.attempt, "attempt_seed": q.seed,
                                             "segment_end": 5, "shuffle_seed": "6", "new_quiz_id": Uuid::new_v4() })));
    a.sync(ops).await;
    let r = b.sync([offline, vec![fb]].concat()).await;
    assert_eq!(result(&r, 10)["reason"], "not_deepest");
    // finish first: the finish_segment's attempt is out of date.
    let mut setup = Setup::new(10);
    setup.segment_size = 5;
    let s2 = setup;
    insert_cascade(&app, a.client.user_id.unwrap(), &s2).await;
    a.sync(vec![]).await;
    b.sync(vec![]).await;
    let q = quiz_now(&app, s2.source_id).await;
    let mut offline = grades(&app, &mut b, s2.source_id, 0, "CCCCM").await;
    offline.push(b.op("finish_segment", json!({ "quiz_id": s2.source_id, "attempt": q.attempt, "attempt_seed": q.seed,
                                                 "segment_end": 5, "shuffle_seed": "7", "new_quiz_id": Uuid::new_v4() })));
    let o = a.op("set_quiz_options", json!({ "quiz_id": s2.source_id, "segment_size": 0 }));
    one(&mut a, o).await;
    let (r, _) = play_and_finish(&app, &mut a, s2.source_id, "CCCCCCCCCM").await;
    assert_eq!(r["outcome"], "descended");
    let r = b.sync(offline).await;
    assert_eq!(result(&r, 5)["reason"], "stale_attempt");
}

/// Row: a quiz restored on one device while the other finishes the deepest level.
#[sqlx::test]
async fn restore_against_finishing_the_deepest(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let (mut a, mut b, s) = two(&app, Setup::new(5)).await;
    let old = with_cleared_level_2(&app, &mut a, &s).await;
    let (_, l2) = play_and_finish(&app, &mut a, s.source_id, "CCCCM").await;
    b.sync(vec![]).await;
    // The restore lands first: the finish is not_deepest.
    let offline = grades(&app, &mut b, l2, 0, "C").await;
    let (fb, _) = finish_op(&app, &mut b, l2, "8").await;
    assert_eq!(restore(&mut a, old).await, "applied");
    let r = b.sync([offline, vec![fb]].concat()).await;
    assert_eq!(result(&r, 0)["status"], "applied", "a grade needs no depth");
    assert_eq!(result(&r, 1)["reason"], "not_deepest");
    // The finish lands first: the restore applies on top of the new deepest level.
    let (r, _) = play_and_finish(&app, &mut a, old, "C").await;
    assert_eq!(r["outcome"], "cleared");
    b.sync(vec![]).await;
    let (r, _) = play_and_finish(&app, &mut a, l2, "C").await;
    assert_eq!(r["outcome"], "cleared");
    assert_eq!(restore(&mut b, old).await, "applied");
    let (level, depth, _) = level_and_depth(&app, old).await;
    assert_eq!((level, depth), (2, 2));
}

/// Row: two different quizzes restored on two devices — both apply, the
/// second deeper; the first's grades are kept and its finish is not_deepest.
#[sqlx::test]
async fn two_different_quizzes_restored(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let (mut a, mut b, s) = two(&app, Setup::new(5)).await;
    let first = with_cleared_level_2(&app, &mut a, &s).await;
    let second = with_cleared_level_2(&app, &mut a, &s).await;
    b.sync(vec![]).await;
    assert_eq!(restore(&mut a, first).await, "applied");
    assert_eq!(restore(&mut b, second).await, "applied");
    assert!(level_and_depth(&app, second).await.0 > level_and_depth(&app, first).await.0);
    let g = grades(&app, &mut a, first, 0, "C").await;
    let r = a.sync(g).await;
    assert_eq!(result(&r, 0)["status"], "applied");
    let (f, _) = finish_op(&app, &mut a, first, "3").await;
    assert_eq!(one(&mut a, f).await, "not_deepest");
}

/// Row: quiz options changed on one device while the quiz was finished on another.
#[sqlx::test]
async fn options_against_a_finish(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let (mut a, mut b, s) = two(&app, Setup::new(5)).await;
    let (_, l2) = play_and_finish(&app, &mut a, s.source_id, "CCCCM").await;
    b.sync(vec![]).await;
    // Reset (the Source quiz descended): the change applies to the reset quiz.
    let o = b.op("set_quiz_options", json!({ "quiz_id": s.source_id, "require_alphabetical": true }));
    assert_eq!(one(&mut b, o).await, "applied");
    // Cleared: rejected.
    let o = b.op("set_quiz_options", json!({ "quiz_id": l2, "require_alphabetical": true }));
    play_and_finish(&app, &mut a, l2, "C").await;
    assert_eq!(one(&mut b, o).await, "not_active");
}

// Row: quiz or cascade options changed on two devices — tests/sync.rs latest_wins_whole.

/// Row: a quiz created while the cascade's options were different keeps its own.
#[sqlx::test]
async fn quizzes_keep_the_options_they_were_created_with(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let (mut a, _b, s) = two(&app, Setup::new(10)).await;
    let (_, l2) = play_and_finish(&app, &mut a, s.source_id, "CCCCMMMMMM").await;
    let o = a.op("set_cascade_options", json!({ "cascade_id": s.cascade_id, "segment_size": 5, "progression": "drill" }));
    one(&mut a, o).await;
    let (seg, prog): (i32, String) = sqlx::query_as("SELECT segment_size, progression::text FROM quizzes WHERE id = $1")
        .bind(l2)
        .fetch_one(app.db())
        .await
        .unwrap();
    assert_eq!((seg, prog.as_str()), (0, "ladder"));
}

// Row: a finish applied with a different outcome — tests/sync.rs finishes_across_devices.

/// Row: a quiz's segment size changed on one device while another passed a boundary.
#[sqlx::test]
async fn segment_size_changed_under_a_boundary(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let mut setup = Setup::new(20);
    setup.segment_size = 5;
    let (mut a, mut b, s) = two(&app, setup).await;
    let q = quiz_now(&app, s.source_id).await;
    let mut offline = grades(&app, &mut b, s.source_id, 0, "CCCCM").await;
    offline.push(b.op("finish_segment", json!({ "quiz_id": s.source_id, "attempt": q.attempt, "attempt_seed": q.seed,
                                                 "segment_end": 5, "shuffle_seed": "1", "new_quiz_id": Uuid::new_v4() })));
    let o = a.op("set_quiz_options", json!({ "quiz_id": s.source_id, "segment_size": 10 }));
    assert_eq!(one(&mut a, o).await, "applied");
    let r = b.sync(offline).await;
    assert_eq!(result(&r, 5)["reason"], "bad_segment");
}

/// Row: the cascade's options changed while another device, offline with the
/// old options, finished a quiz — the server builds the new quiz from its own.
#[sqlx::test]
async fn new_quizzes_take_the_servers_options(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let (mut a, mut b, s) = two(&app, Setup::new(10)).await;
    let o = a.op("set_cascade_options", json!({ "cascade_id": s.cascade_id, "progression": "drill", "segment_size": 5 }));
    one(&mut a, o).await;
    let (r, l2) = play_and_finish(&app, &mut b, s.source_id, "CCCCMMMMMM").await;
    assert_eq!(r["outcome"], "descended");
    let (seg, prog): (i32, String) = sqlx::query_as("SELECT segment_size, progression::text FROM quizzes WHERE id = $1")
        .bind(l2)
        .fetch_one(app.db())
        .await
        .unwrap();
    assert_eq!((seg, prog.as_str()), (5, "drill"));
}

/// Row: the cursor moved on two devices in the same attempt.
#[sqlx::test]
async fn cursor_moves_on_two_devices(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let (mut a, mut b, s) = two(&app, Setup::new(10)).await;
    let q = quiz_now(&app, s.source_id).await;
    let now = Utc::now();
    let mv = |d: &mut Device<'_>, p: i64, at| {
        d.op_at("move_cursor", json!({ "quiz_id": s.source_id, "attempt": q.attempt, "attempt_seed": q.seed, "position": p }), at)
    };
    let o = mv(&mut a, 4, now);
    assert_eq!(one(&mut a, o).await, "applied");
    // Another device's earlier move, unseen: stale.
    let o = mv(&mut b, 2, now - Duration::minutes(1));
    assert_eq!(one(&mut b, o).await, "stale");
    // Having seen it: applies, whatever the time.
    b.sync(vec![]).await;
    let o = mv(&mut b, 3, now - Duration::minutes(2));
    assert_eq!(one(&mut b, o).await, "applied");
    // A device's own moves apply in order.
    let o1 = mv(&mut b, 6, now - Duration::minutes(5));
    let o2 = mv(&mut b, 5, now - Duration::minutes(6));
    let r = b.sync(vec![o1, o2]).await;
    assert_eq!((result(&r, 0)["status"].clone(), result(&r, 1)["status"].clone()), (json!("applied"), json!("applied")));
    assert_eq!(quiz_now(&app, s.source_id).await.cursor, 5);
    // An options change by a device that had seen the options but not a later
    // cursor move on the same quiz applies (options_seq, not the quiz's seq).
    let o = a.op("set_quiz_options", json!({ "quiz_id": s.source_id, "require_alphabetical": true }));
    a.sync(vec![o]).await;
    b.sync(vec![]).await;
    let m = mv(&mut a, 7, Utc::now());
    a.sync(vec![m]).await;
    let o = b.op_at("set_quiz_options", json!({ "quiz_id": s.source_id, "require_alphabetical": false }), now - Duration::hours(1));
    assert_eq!(one(&mut b, o).await, "applied");
}

// Row: preferences changed on two devices — tests/sync.rs latest_wins_whole.
