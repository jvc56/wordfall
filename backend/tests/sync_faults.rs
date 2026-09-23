//! PLAN.md § Integration tests → Sync integration tests: database errors
//! inside a savepoint, a forced serialization failure, purge against a
//! concurrent sync, the sync and catalog rate limits, the sliding session
//! (PQ-003), and the 300,000-question finish and reset.

mod common;

use std::time::Instant;

use axum::http::StatusCode;
use chrono::Duration;
use common::sync::{finish_op, grades, insert_cascade, play_and_finish, result, rows, Device, Setup};
use common::TestApp;
use serde_json::json;

/// The budget for one 300,000-question finish, and for its reset, in a debug
/// build on the CI runner ("The budgets are constants in the test").
const FINISH_300K_BUDGET_SECS: u64 = 20;

async fn setup<'a>(app: &'a TestApp, s: &Setup) -> Device<'a> {
    let c = app.signed_in("alice").await;
    insert_cascade(app, c.user_id.unwrap(), s).await;
    let mut d = Device::new(c);
    d.qrf = vec![s.cascade_id];
    d.sync(vec![]).await;
    d
}

/// "A restore_quiz whose application raises a database error inside its
/// savepoint is recorded rejected with reason error, leaving the rest of the
/// batch applied" — a CHECK violation injected by a trigger.
#[sqlx::test]
async fn a_database_error_in_one_savepoint(pool: sqlx::PgPool) {
    let app = TestApp::with(pool, &[("SYNC_RATE_PER_MINUTE", "100000")]).await;
    let s = Setup::new(5);
    let mut d = setup(&app, &s).await;
    let (_, l2) = play_and_finish(&app, &mut d, s.source_id, "CCCCM").await;
    play_and_finish(&app, &mut d, l2, "C").await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE FUNCTION wf_fault() RETURNS trigger AS $$
         BEGIN
           IF NEW.id = '{l2}' AND OLD.status = 'cleared' AND NEW.status = 'active' THEN
             RAISE EXCEPTION 'injected' USING ERRCODE = 'check_violation';
           END IF;
           RETURN NEW;
         END $$ LANGUAGE plpgsql;
         CREATE TRIGGER wf_fault BEFORE UPDATE ON quizzes FOR EACH ROW EXECUTE FUNCTION wf_fault();"
    )))
    .execute(app.db())
    .await
    .unwrap();
    let mut ops = grades(&app, &mut d, s.source_id, 0, "C").await;
    ops.push(d.op("restore_quiz", json!({ "quiz_id": l2, "shuffle_seed": "9" })));
    ops.extend(grades(&app, &mut d, s.source_id, 1, "C").await);
    let r = d.sync(ops).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(result(&r, 0)["status"], "applied");
    assert_eq!((result(&r, 1)["status"].clone(), result(&r, 1)["reason"].clone()), (json!("rejected"), json!("error")));
    assert_eq!(result(&r, 2)["status"], "applied");
    let recorded: String = sqlx::query_scalar("SELECT reason::text FROM sync_operations WHERE op_type = 'restore_quiz'")
        .fetch_one(app.db())
        .await
        .unwrap();
    assert_eq!(recorded, "error");
}

/// "a forced serialization failure answers 503 with Retry-After and records
/// nothing, and the same batch sent again is applied in full".
#[sqlx::test]
async fn a_serialization_failure_is_503_and_the_resend_applies(pool: sqlx::PgPool) {
    let app = TestApp::with(pool, &[("SYNC_RATE_PER_MINUTE", "100000")]).await;
    let s = Setup::new(5);
    let mut d = setup(&app, &s).await;
    // Fails the first grade only: a sequence is not rolled back with the transaction.
    sqlx::raw_sql(
        "CREATE SEQUENCE wf_fault_seq;
         CREATE FUNCTION wf_fault() RETURNS trigger AS $$
         BEGIN
           IF nextval('wf_fault_seq') = 1 THEN
             RAISE EXCEPTION 'injected' USING ERRCODE = 'serialization_failure';
           END IF;
           RETURN NEW;
         END $$ LANGUAGE plpgsql;
         CREATE TRIGGER wf_fault BEFORE UPDATE ON quiz_questions FOR EACH ROW EXECUTE FUNCTION wf_fault();",
    )
    .execute(app.db())
    .await
    .unwrap();
    let ops = grades(&app, &mut d, s.source_id, 0, "CCM").await;
    let r = d.sync(ops.clone()).await;
    assert_eq!(r.status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(r.retry_after().is_some());
    let recorded: i64 = sqlx::query_scalar("SELECT count(*) FROM sync_operations").fetch_one(app.db()).await.unwrap();
    let graded: i64 = sqlx::query_scalar("SELECT count(*) FROM quiz_questions WHERE grade IS NOT NULL").fetch_one(app.db()).await.unwrap();
    assert_eq!((recorded, graded), (0, 0), "nothing recorded or written");
    let r = d.sync(ops).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.json()["results"].as_array().unwrap().iter().all(|x| x["status"] == "applied"));
}

/// "the purge task running against a concurrent sync for the same user
/// completes without a deadlock".
#[sqlx::test]
async fn purge_against_a_concurrent_sync(pool: sqlx::PgPool) {
    let app = TestApp::with(pool, &[("SYNC_RATE_PER_MINUTE", "100000"), ("PURGE_MAX_QUIZZES_PER_USER_PER_RUN", "50")]).await;
    let s = Setup::new(40);
    let mut d = setup(&app, &s).await;
    let user = d.client.user_id.unwrap();
    common::sync::insert_cleared_chains(&app, user, &s, 400, 40).await;
    let old = Setup::new(5);
    insert_cascade(&app, user, &old).await;
    sqlx::query("UPDATE cascades SET trashed_at = now() - interval '40 days' WHERE id = $1").bind(old.cascade_id).execute(app.db()).await.unwrap();
    for round in 0..8 {
        let marks: String = (0..5).map(|i| if (i + round) % 2 == 0 { 'C' } else { 'M' }).collect();
        let mut ops = grades(&app, &mut d, s.source_id, round * 5, &marks).await;
        ops.push(d.op("set_cascade_options", json!({ "cascade_id": s.cascade_id, "segment_size": 5 + round })));
        let state = app.state.clone();
        let (purged, r) = tokio::time::timeout(std::time::Duration::from_secs(60), async {
            tokio::join!(wordfall::purge::run_once(&state), d.sync(ops))
        })
        .await
        .expect("no hang");
        purged.expect("the purge completes");
        assert_eq!(r.status, StatusCode::OK, "round {round}: {:?}", r.json());
    }
}

/// The sync limit: with the rate at 1, a second sync in the minute is 429
/// with Retry-After and the card-page bucket is untouched; and the catalog's
/// per-IP bucket does not touch a signed-in user's sync.
#[sqlx::test]
async fn the_sync_and_catalog_limits_are_separate(pool: sqlx::PgPool) {
    let app = TestApp::with(pool, &[("SYNC_RATE_PER_MINUTE", "1"), ("CATALOG_RATE_PER_MINUTE", "1")]).await;
    let s = Setup::new(5);
    let c = app.signed_in("alice").await;
    insert_cascade(&app, c.user_id.unwrap(), &s).await;
    let mut d = Device::new(c);
    // The catalog bucket spent from this address first.
    assert_eq!(d.client.get("/api/lexicons").await.status, StatusCode::OK);
    assert_eq!(d.client.get("/api/lexicons").await.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(d.sync(vec![]).await.status, StatusCode::OK);
    let r = d.sync(vec![]).await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(r.retry_after().is_some());
    let cards = d.client.get(&format!("/api/cascades/{}/cards?from=0&limit=5&keys=1", s.cascade_id)).await;
    assert_eq!(cards.status, StatusCode::OK, "the card-page bucket is its own");
}

/// PQ-003: a sync in the last seven days of a cookie's life gets a fresh
/// cookie of the full TTL; earlier, and on /api/auth/me, it does not.
#[sqlx::test]
async fn a_sync_slides_the_session(pool: sqlx::PgPool) {
    let app = TestApp::with(pool, &[("SYNC_RATE_PER_MINUTE", "100000")]).await;
    let mut d = Device::new(app.signed_in("alice").await);
    let r = d.sync(vec![]).await;
    assert!(r.set_cookie("wordfall_session").is_none());
    app.state.clock.advance(Duration::days(24));
    assert!(d.client.get("/api/auth/me").await.set_cookie("wordfall_session").is_none());
    let r = d.sync(vec![]).await;
    let cookie = r.set_cookie("wordfall_session").expect("renewed");
    assert!(cookie.contains("Max-Age=2592000"), "{cookie}");
    d.client.absorb(&r);
    // Still signed in past the original expiry.
    app.state.clock.advance(Duration::days(10));
    assert_eq!(d.sync(vec![]).await.status, StatusCode::OK);
}

/// "A 300,000-question finish and reset complete within budget, and the reset
/// is pulled by another device as one quiz row."
#[sqlx::test]
async fn a_300000_question_finish_and_reset(pool: sqlx::PgPool) {
    let app = TestApp::with(pool, &[("SYNC_RATE_PER_MINUTE", "100000")]).await;
    let s = Setup::new(300_000);
    let mut d = setup(&app, &s).await;
    let mut b = d.sibling(&app);
    b.qrf = d.qrf.clone();
    b.sync(vec![]).await;
    // Every question graded, half missed, straight in the database (the
    // 600-request push is the scale test's).
    sqlx::query(
        "UPDATE quiz_questions SET grade = CASE WHEN question_idx % 2 = 0 THEN 'correct'::grade ELSE 'missed'::grade END,
                graded_at = now(), graded_by_device_id = $2 WHERE quiz_id = $1",
    )
    .bind(s.source_id)
    .bind(d.id)
    .execute(app.db())
    .await
    .unwrap();
    sqlx::query("UPDATE quizzes SET correct_count = 150000, missed_count = 150000 WHERE id = $1").bind(s.source_id).execute(app.db()).await.unwrap();
    // The finish: the Source quiz descends, which resets it, and creates a 150,000-question Level 2.
    let (f, l2) = finish_op(&app, &mut d, s.source_id, "123").await;
    let t = Instant::now();
    let r = d.sync(vec![f]).await;
    assert!(t.elapsed().as_secs() < FINISH_300K_BUDGET_SECS, "finish took {:?}", t.elapsed());
    assert_eq!(result(&r, 0)["outcome"], "descended");
    assert_eq!(result(&r, 0)["new_quiz_question_count"], 150_000);
    // A reset on its own: a reshuffle of the Source quiz.
    sqlx::query(
        "UPDATE quiz_questions SET grade = 'missed', graded_at = now(), graded_by_device_id = $2 WHERE quiz_id = $1",
    )
    .bind(s.source_id)
    .bind(d.id)
    .execute(app.db())
    .await
    .unwrap();
    sqlx::query("UPDATE quizzes SET correct_count = 0, missed_count = 300000 WHERE id = $1").bind(s.source_id).execute(app.db()).await.unwrap();
    // Level 2 is the deepest; clear it out of the way first.
    sqlx::query("UPDATE quizzes SET status = 'cleared', cleared_at = now() WHERE id = $1").bind(l2).execute(app.db()).await.unwrap();
    sqlx::query("UPDATE cascades SET depth = 1").execute(app.db()).await.unwrap();
    let (f, _) = finish_op(&app, &mut d, s.source_id, "456").await;
    let t = Instant::now();
    let r = d.sync(vec![f]).await;
    assert!(t.elapsed().as_secs() < FINISH_300K_BUDGET_SECS, "reset took {:?}", t.elapsed());
    assert_eq!(result(&r, 0)["outcome"], "reshuffled");
    // The other device pulls the reset as one quiz row and no question rows.
    let (_, pages) = b.sync_all(vec![]).await;
    let source_rows: Vec<_> = rows(&pages, "quizzes").into_iter().filter(|q| q["id"] == json!(s.source_id)).collect();
    assert_eq!(source_rows.len(), 1);
    assert!(rows(&pages, "quiz_questions").iter().all(|g| g["quiz_id"] != json!(s.source_id)));
}
