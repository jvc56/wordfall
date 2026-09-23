//! The hourly purge task (PLAN.md § Purge task).
//!
//! Each instance runs it; `pg_try_advisory_lock` lets only one purge at a time.
//! It works one user per transaction, locking the user row first (the
//! `sync_seq` bump) and only then touching that user's cascades and quizzes,
//! the same order a sync takes, so a purge and a sync never deadlock.

use std::time::Duration;

use uuid::Uuid;

use crate::app::AppState;

/// Advisory lock key for the purge task.
pub const PURGE_LOCK_KEY: i64 = 0x5746_5055_5247_45; // "WFPURGE"

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(state.config.purge_interval_seconds.max(1)));
        loop {
            tick.tick().await;
            if let Err(e) = run_once(&state).await {
                tracing::error!(error = %e, "purge run failed");
            }
        }
    });
}

/// One run. Returns false when another instance holds the lock.
pub async fn run_once(state: &AppState) -> anyhow::Result<bool> {
    let mut conn = state.db.acquire().await?;
    let locked: bool = sqlx::query_scalar!("SELECT pg_try_advisory_lock($1) AS \"locked!\"", PURGE_LOCK_KEY)
        .fetch_one(&mut *conn)
        .await?;
    if !locked {
        return Ok(false);
    }
    let result = run_locked(state).await;
    sqlx::query_scalar!("SELECT pg_advisory_unlock($1)", PURGE_LOCK_KEY).fetch_one(&mut *conn).await?;
    result.map(|_| true)
}

async fn run_locked(state: &AppState) -> anyhow::Result<()> {
    purge_trash(state).await?;
    prune_sync_records(state).await?;
    delete_stale_unconfirmed(state).await?;
    prune_export_tokens(state).await?;
    prune_catalog_status(state).await?;
    Ok(())
}

/// Users with anything past the retention period.
async fn users_with_trash(state: &AppState, days: i32) -> anyhow::Result<Vec<Uuid>> {
    Ok(sqlx::query_scalar!(
        r#"SELECT q.user_id AS "user_id!" FROM quizzes q JOIN cascades c ON c.id = q.cascade_id
           WHERE q.status = 'cleared' AND c.trashed_at IS NULL
             AND q.cleared_at < now() - make_interval(days => $1)
           UNION
           SELECT user_id FROM cascades WHERE trashed_at < now() - make_interval(days => $1)"#,
        days
    )
    .fetch_all(&state.db)
    .await?)
}

/// Cleared quizzes past the retention period whose cascade is not trashed,
/// oldest first and at most `PURGE_MAX_QUIZZES_PER_USER_PER_RUN` per user per
/// run, then trashed cascades past it, each whole; a run that has reached its
/// cap starts no further cascade.
pub async fn purge_trash(state: &AppState) -> anyhow::Result<()> {
    let days = state.config.trash_retention_days as i32;
    let cap = i64::from(state.config.purge_max_quizzes_per_user_per_run);
    for user in users_with_trash(state, days).await? {
        let mut tx = state.db.begin().await?;
        sqlx::query!("SELECT id FROM users WHERE id = $1 FOR UPDATE", user).fetch_one(&mut *tx).await?;
        let seq = sqlx::query_scalar!("UPDATE users SET sync_seq = sync_seq + 1 WHERE id = $1 RETURNING sync_seq", user)
            .fetch_one(&mut *tx)
            .await?;
        let quizzes: Vec<Uuid> = sqlx::query_scalar!(
            "SELECT q.id FROM quizzes q JOIN cascades c ON c.id = q.cascade_id
             WHERE q.user_id = $1 AND q.status = 'cleared' AND c.trashed_at IS NULL
               AND q.cleared_at < now() - make_interval(days => $2)
             ORDER BY q.cleared_at, q.id LIMIT $3",
            user,
            days,
            cap
        )
        .fetch_all(&mut *tx)
        .await?;
        if !quizzes.is_empty() {
            sqlx::query!("DELETE FROM quizzes WHERE id = ANY($1)", &quizzes).execute(&mut *tx).await?;
            sqlx::query!(
                "INSERT INTO sync_tombstones (user_id, entity, entity_id, seq)
                 SELECT $1, 'quiz', q, $3 FROM UNNEST($2::uuid[]) AS q
                 ON CONFLICT (user_id, entity, entity_id) DO UPDATE SET seq = EXCLUDED.seq, deleted_at = now()",
                user,
                &quizzes,
                seq
            )
            .execute(&mut *tx)
            .await?;
        }
        if (quizzes.len() as i64) < cap {
            let cascades: Vec<Uuid> = sqlx::query_scalar!(
                "SELECT id FROM cascades WHERE user_id = $1 AND trashed_at < now() - make_interval(days => $2)
                 ORDER BY trashed_at, id",
                user,
                days
            )
            .fetch_all(&mut *tx)
            .await?;
            for c in cascades {
                crate::sync::ops::purge_cascade_rows(&mut tx, user, c, seq).await?;
            }
        }
        tx.commit().await?;
    }
    Ok(())
}

/// Result-bearing sync records and tombstones past `SYNC_RETENTION_DAYS`,
/// raising each affected user's `sync_floor_seq` to the highest tombstone
/// sequence removed.
pub async fn prune_sync_records(state: &AppState) -> anyhow::Result<()> {
    let days = state.config.sync_retention_days as i32;
    sqlx::query!("DELETE FROM sync_operations WHERE received_at < now() - make_interval(days => $1)", days)
        .execute(&state.db)
        .await?;
    let users: Vec<Uuid> = sqlx::query_scalar!(
        "SELECT DISTINCT user_id FROM sync_tombstones WHERE deleted_at < now() - make_interval(days => $1)",
        days
    )
    .fetch_all(&state.db)
    .await?;
    for user in users {
        let mut tx = state.db.begin().await?;
        sqlx::query!("SELECT id FROM users WHERE id = $1 FOR UPDATE", user).fetch_one(&mut *tx).await?;
        let highest = sqlx::query_scalar!(
            "WITH gone AS (DELETE FROM sync_tombstones WHERE user_id = $1 AND deleted_at < now() - make_interval(days => $2)
                           RETURNING seq)
             SELECT max(seq) FROM gone",
            user,
            days
        )
        .fetch_one(&mut *tx)
        .await?;
        if let Some(h) = highest {
            sqlx::query!("UPDATE users SET sync_floor_seq = greatest(sync_floor_seq, $2) WHERE id = $1", user, h)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
    }
    Ok(())
}

/// Accounts never confirmed whose last confirmation code expired more than
/// 7 days ago, so an abandoned registration does not hold an address forever.
pub async fn delete_stale_unconfirmed(state: &AppState) -> anyhow::Result<u64> {
    let r = sqlx::query!(
        "DELETE FROM users u
         WHERE u.email_confirmed_at IS NULL
           AND NOT EXISTS (SELECT 1 FROM email_confirmations c
                           WHERE c.user_id = u.id
                             AND c.expires_at > now() - interval '7 days')"
    )
    .execute(&state.db)
    .await?;
    Ok(r.rows_affected())
}

pub async fn prune_export_tokens(state: &AppState) -> anyhow::Result<u64> {
    let r = sqlx::query!("DELETE FROM export_tokens_spent WHERE expires_at < now()").execute(&state.db).await?;
    Ok(r.rows_affected())
}

/// `catalog_instance_status` rows whose heartbeat is older than 3 minutes,
/// left by instances that have stopped.
pub async fn prune_catalog_status(state: &AppState) -> anyhow::Result<u64> {
    let r = sqlx::query!("DELETE FROM catalog_instance_status WHERE heartbeat_at < now() - interval '3 minutes'")
        .execute(&state.db)
        .await?;
    Ok(r.rows_affected())
}
