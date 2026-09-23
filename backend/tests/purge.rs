//! PLAN.md § Purge task, § Trash, restore and purge, and the Sync integration
//! tests about retention clocks, counters and purge rejections.

mod common;

use chrono::{DateTime, Duration, Utc};
use common::sync::{finish_op, grades, insert_cascade, insert_cleared_chains, one, play_and_finish, quiz_now, Device, Setup};
use common::TestApp;
use serde_json::json;
use uuid::Uuid;

async fn app_with(pool: sqlx::PgPool, extra: &[(&str, &str)]) -> TestApp {
    let mut o = vec![("SYNC_RATE_PER_MINUTE", "100000")];
    o.extend_from_slice(extra);
    TestApp::with(pool, &o).await
}

async fn setup<'a>(app: &'a TestApp, s: &Setup) -> Device<'a> {
    let c = app.signed_in("alice").await;
    insert_cascade(app, c.user_id.unwrap(), s).await;
    let mut d = Device::new(c);
    d.qrf = vec![s.cascade_id];
    d.sync(vec![]).await;
    d
}

async fn exists(app: &TestApp, quiz: Uuid) -> bool {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM quizzes WHERE id = $1)").bind(quiz).fetch_one(app.db()).await.unwrap()
}

async fn cleared_at(app: &TestApp, quiz: Uuid) -> DateTime<Utc> {
    sqlx::query_scalar("SELECT cleared_at FROM quizzes WHERE id = $1").bind(quiz).fetch_one(app.db()).await.unwrap()
}

async fn age_cleared(app: &TestApp, days: i32) {
    sqlx::query("UPDATE quizzes SET cleared_at = now() - make_interval(days => $1) WHERE status = 'cleared'")
        .bind(days)
        .execute(app.db())
        .await
        .unwrap();
}

/// "Purge a user whose segmented attempt left 60,000 cleared chain quizzes"
/// in miniature: each run takes at most the cap, oldest cleared_at first;
/// a trashed cascade past retention is purged whole, and a run that reached
/// its cap starts no further cascade.
#[sqlx::test]
async fn the_per_user_cap_and_trashed_cascades_whole(pool: sqlx::PgPool) {
    let app = app_with(pool, &[("PURGE_MAX_QUIZZES_PER_USER_PER_RUN", "3")]).await;
    let s = Setup::new(5);
    let mut d = setup(&app, &s).await;
    let user = d.client.user_id.unwrap();
    insert_cleared_chains(&app, user, &s, 5, 0).await;
    // Oldest first: stagger the clear times.
    sqlx::query(
        "UPDATE quizzes SET cleared_at = now() - make_interval(days => 40 + origin_attempt) WHERE status = 'cleared'",
    )
    .execute(app.db())
    .await
    .unwrap();
    let order: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM quizzes WHERE status = 'cleared' ORDER BY cleared_at")
        .fetch_all(app.db())
        .await
        .unwrap();
    // A second cascade, trashed long ago, with more quizzes than the cap.
    let t = Setup::new(5);
    insert_cascade(&app, user, &t).await;
    insert_cleared_chains(&app, user, &t, 10, 0).await;
    sqlx::query("UPDATE cascades SET trashed_at = now() - interval '40 days' WHERE id = $1").bind(t.cascade_id).execute(app.db()).await.unwrap();
    sqlx::query("UPDATE quizzes SET cleared_at = now() - interval '40 days' WHERE cascade_id = $1 AND status = 'cleared'")
        .bind(t.cascade_id)
        .execute(app.db())
        .await
        .unwrap();
    // Run 1: the three oldest; the cap is reached, so no cascade starts.
    assert!(wordfall::purge::run_once(&app.state).await.unwrap());
    for (i, q) in order.iter().enumerate() {
        assert_eq!(exists(&app, *q).await, i >= 3, "{i}");
    }
    let trashed_left: i64 = sqlx::query_scalar("SELECT count(*) FROM cascades WHERE id = $1").bind(t.cascade_id).fetch_one(app.db()).await.unwrap();
    assert_eq!(trashed_left, 1);
    // Run 2: the last two, then the trashed cascade whole (all 11 quizzes).
    assert!(wordfall::purge::run_once(&app.state).await.unwrap());
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM quizzes").fetch_one(app.db()).await.unwrap();
    assert_eq!(left, 1, "only the live Source quiz");
    let tombs: i64 = sqlx::query_scalar("SELECT count(*) FROM sync_tombstones").fetch_one(app.db()).await.unwrap();
    assert_eq!(tombs, 5 + 11 + 1);
    // The device learns of it all.
    let (_, pages) = d.sync_all(vec![]).await;
    assert_eq!(common::sync::rows(&pages, "tombstones").len(), 17);
}

/// "the purge task leaves a trashed cascade's quizzes until the cascade's own purge".
#[sqlx::test]
async fn a_trashed_cascades_quizzes_wait_for_the_cascade(pool: sqlx::PgPool) {
    let app = app_with(pool, &[]).await;
    let s = Setup::new(5);
    let mut d = setup(&app, &s).await;
    let (_, l2) = play_and_finish(&app, &mut d, s.source_id, "CCCCM").await;
    play_and_finish(&app, &mut d, l2, "C").await;
    let t = d.op("trash_cascade", json!({ "cascade_id": s.cascade_id }));
    one(&mut d, t).await;
    age_cleared(&app, 40).await;
    wordfall::purge::run_once(&app.state).await.unwrap();
    assert!(exists(&app, l2).await);
    // purge_quiz on a quiz of a trashed cascade is `trashed`.
    let p = d.op("purge_quiz", json!({ "quiz_id": l2 }));
    assert_eq!(one(&mut d, p).await, "trashed");
    let p = d.op("purge_quiz", json!({ "quiz_id": s.source_id }));
    assert_eq!(one(&mut d, p).await, "trashed");
    let r = d.op("restore_cascade", json!({ "cascade_id": s.cascade_id }));
    one(&mut d, r).await;
    // On an active quiz of a live cascade: not_cleared.
    let p = d.op("purge_quiz", json!({ "quiz_id": s.source_id }));
    assert_eq!(one(&mut d, p).await, "not_cleared");
}

/// Two instances running the purge at once: the advisory lock lets one purge,
/// and everything is purged once with no error.
#[sqlx::test]
async fn two_instances_purge_concurrently(pool: sqlx::PgPool) {
    let app = app_with(pool.clone(), &[]).await;
    let other = app_with(pool, &[]).await;
    let s = Setup::new(5);
    let d = setup(&app, &s).await;
    insert_cleared_chains(&app, d.client.user_id.unwrap(), &s, 200, 40).await;
    let (x, y) = tokio::join!(wordfall::purge::run_once(&app.state), wordfall::purge::run_once(&other.state));
    let (x, y) = (x.unwrap(), y.unwrap());
    assert!(x || y);
    if !(x && y) {
        // One was turned away; a later run finds nothing left either way.
        assert!(wordfall::purge::run_once(&other.state).await.unwrap());
    }
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM quizzes WHERE status = 'cleared'").fetch_one(app.db()).await.unwrap();
    assert_eq!(left, 0);
    let tombs: i64 = sqlx::query_scalar("SELECT count(*) FROM sync_tombstones").fetch_one(app.db()).await.unwrap();
    assert_eq!(tombs, 200);
}

/// "peak_depth and attempts_since_completion are unchanged by a purge".
#[sqlx::test]
async fn counters_survive_the_purge(pool: sqlx::PgPool) {
    let app = app_with(pool, &[]).await;
    let s = Setup::new(5);
    let mut d = setup(&app, &s).await;
    let mut finishes = 0;
    while finishes < 14 {
        let (r, l2) = play_and_finish(&app, &mut d, s.source_id, "CCCCM").await;
        assert_eq!(r["outcome"], "descended");
        let (r, _) = play_and_finish(&app, &mut d, l2, "C").await;
        assert_eq!(r["outcome"], "cleared");
        finishes += 2;
    }
    let (r, _) = play_and_finish(&app, &mut d, s.source_id, "CCCCM").await;
    assert_eq!(r["outcome"], "descended");
    let before: (i32, i32) =
        sqlx::query_as("SELECT peak_depth, attempts_since_completion FROM cascades").fetch_one(app.db()).await.unwrap();
    assert_eq!(before, (2, 15));
    age_cleared(&app, 40).await;
    let attempts_before: i64 = sqlx::query_scalar("SELECT count(*) FROM quiz_attempts").fetch_one(app.db()).await.unwrap();
    wordfall::purge::run_once(&app.state).await.unwrap();
    let cleared: i64 = sqlx::query_scalar("SELECT count(*) FROM quizzes WHERE status = 'cleared'").fetch_one(app.db()).await.unwrap();
    assert_eq!(cleared, 0);
    let attempts_after: i64 = sqlx::query_scalar("SELECT count(*) FROM quiz_attempts").fetch_one(app.db()).await.unwrap();
    assert!(attempts_after < attempts_before, "their attempt rows went with them");
    let after: (i32, i32) =
        sqlx::query_as("SELECT peak_depth, attempts_since_completion FROM cascades").fetch_one(app.db()).await.unwrap();
    assert_eq!(after, before);
}

/// "Restoring a trashed cascade, or one of its quizzes, resets cleared_at on
/// its remaining cleared quizzes, and the purge task leaves them for a full
/// retention period afterwards, while a quiz restored in an active cascade
/// leaves its siblings' cleared_at untouched".
#[sqlx::test]
async fn restores_restart_the_retention_clock(pool: sqlx::PgPool) {
    let app = app_with(pool, &[]).await;
    let s = Setup::new(5);
    let mut d = setup(&app, &s).await;
    let mut cleared = vec![];
    for _ in 0..3 {
        let (_, l2) = play_and_finish(&app, &mut d, s.source_id, "CCCCM").await;
        play_and_finish(&app, &mut d, l2, "C").await;
        cleared.push(l2);
    }
    // A quiz restored in an active cascade: siblings untouched.
    age_cleared(&app, 20).await;
    let sibling_before = cleared_at(&app, cleared[1]).await;
    let r = d.op("restore_quiz", json!({ "quiz_id": cleared[0], "shuffle_seed": "5" }));
    assert_eq!(one(&mut d, r).await, "applied");
    assert_eq!(cleared_at(&app, cleared[1]).await, sibling_before);
    // Restoring the trashed cascade restarts them.
    let t = d.op("trash_cascade", json!({ "cascade_id": s.cascade_id }));
    one(&mut d, t).await;
    age_cleared(&app, 29).await;
    let r = d.op("restore_cascade", json!({ "cascade_id": s.cascade_id }));
    one(&mut d, r).await;
    assert!(Utc::now() - cleared_at(&app, cleared[1]).await < Duration::minutes(1));
    wordfall::purge::run_once(&app.state).await.unwrap();
    assert!(exists(&app, cleared[1]).await && exists(&app, cleared[2]).await);
    // Restoring one of a trashed cascade's quizzes restarts its siblings too.
    let t = d.op("trash_cascade", json!({ "cascade_id": s.cascade_id }));
    one(&mut d, t).await;
    age_cleared(&app, 29).await;
    let r = d.op("restore_quiz", json!({ "quiz_id": cleared[1], "shuffle_seed": "6" }));
    assert_eq!(one(&mut d, r).await, "applied");
    assert!(Utc::now() - cleared_at(&app, cleared[2]).await < Duration::minutes(1));
}

/// "a finish whose at is 40 days old, applied today, clears a quiz the purge
/// task leaves alone for a full retention period, while the cascade's
/// last_activity_at is 40 days old on a cascade with no newer activity, and
/// an older at never lowers either last_activity_at column"; and "a finish
/// dated a year ahead records finished_at at now() + 5 minutes".
#[sqlx::test]
async fn old_and_future_timestamps(pool: sqlx::PgPool) {
    let app = app_with(pool, &[]).await;
    let s = Setup::new(5);
    let mut d = setup(&app, &s).await;
    sqlx::query("UPDATE cascades SET last_activity_at = now() - interval '50 days'").execute(app.db()).await.unwrap();
    sqlx::query("UPDATE quizzes SET last_activity_at = now() - interval '50 days'").execute(app.db()).await.unwrap();
    d.skew = -Duration::days(40);
    let (r, l2) = play_and_finish(&app, &mut d, s.source_id, "CCCCM").await;
    assert_eq!(r["outcome"], "descended");
    let (r, _) = play_and_finish(&app, &mut d, l2, "C").await;
    assert_eq!(r["outcome"], "cleared");
    let activity = |app: &TestApp| {
        let db = app.db().clone();
        async move {
            let c: DateTime<Utc> = sqlx::query_scalar("SELECT last_activity_at FROM cascades").fetch_one(&db).await.unwrap();
            let q: DateTime<Utc> =
                sqlx::query_scalar("SELECT last_activity_at FROM quizzes WHERE origin = 'source'").fetch_one(&db).await.unwrap();
            (c, q)
        }
    };
    let (c, q) = activity(&app).await;
    let forty = Utc::now() - Duration::days(40);
    assert!((c - forty).num_minutes().abs() < 2, "the cascade's last_activity_at is 40 days old");
    assert!((q - forty).num_minutes().abs() < 2);
    assert!(Utc::now() - cleared_at(&app, l2).await < Duration::minutes(1), "cleared_at is the server's now()");
    wordfall::purge::run_once(&app.state).await.unwrap();
    assert!(exists(&app, l2).await);
    // An older at never lowers them.
    d.skew = -Duration::days(45);
    let g = grades(&app, &mut d, s.source_id, 0, "C").await;
    d.sync(g).await;
    assert_eq!(activity(&app).await, (c, q));
    // A finish a year ahead: finished_at at now() + 5 minutes.
    d.skew = Duration::days(365);
    let mut ops = grades(&app, &mut d, s.source_id, 0, "CCCCM").await;
    let (f, _) = finish_op(&app, &mut d, s.source_id, "77").await;
    ops.push(f);
    d.sync(ops).await;
    let finished: DateTime<Utc> =
        sqlx::query_scalar("SELECT max(finished_at) FROM quiz_attempts").fetch_one(app.db()).await.unwrap();
    let limit = Utc::now() + Duration::minutes(5);
    assert!(finished <= limit && limit - finished < Duration::minutes(1), "{finished}");
}

/// "restoring a segment-chain quiz in a Ladder cascade keeps it on Drill with no segments".
#[sqlx::test]
async fn a_restored_chain_quiz_stays_a_chain(pool: sqlx::PgPool) {
    let app = app_with(pool, &[]).await;
    let mut s = Setup::new(10);
    s.segment_size = 5;
    let mut d = setup(&app, &s).await;
    let q = quiz_now(&app, s.source_id).await;
    let mut ops = grades(&app, &mut d, s.source_id, 0, "CCCCM").await;
    let chain = Uuid::new_v4();
    ops.push(d.op("finish_segment", json!({ "quiz_id": s.source_id, "attempt": q.attempt, "attempt_seed": q.seed,
                                             "segment_end": 5, "shuffle_seed": "3", "new_quiz_id": chain })));
    d.sync(ops).await;
    let (r, _) = play_and_finish(&app, &mut d, chain, "C").await;
    assert_eq!(r["outcome"], "cleared");
    let r = d.op("restore_quiz", json!({ "quiz_id": chain, "shuffle_seed": "4" }));
    assert_eq!(one(&mut d, r).await, "applied");
    let (prog, seg, is_chain): (String, i32, bool) =
        sqlx::query_as("SELECT progression::text, segment_size, segment_chain FROM quizzes WHERE id = $1")
            .bind(chain)
            .fetch_one(app.db())
            .await
            .unwrap();
    assert_eq!((prog.as_str(), seg, is_chain), ("drill", 0, true));
}
