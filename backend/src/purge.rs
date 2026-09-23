//! The hourly purge task (PLAN.md § Purge task).
//!
//! Each instance runs it; `pg_try_advisory_lock` lets only one purge at a time.

use std::time::Duration;

use crate::app::AppState;

/// Advisory lock key for the purge task.
pub const PURGE_LOCK_KEY: i64 = 0x5746_5055_5247_45; // "WFPURGE"

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(
            state.config.purge_interval_seconds.max(1),
        ));
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
    let locked: bool = sqlx::query_scalar!(
        "SELECT pg_try_advisory_lock($1) AS \"locked!\"",
        PURGE_LOCK_KEY
    )
    .fetch_one(&mut *conn)
    .await?;
    if !locked {
        return Ok(false);
    }
    let result = run_locked(state).await;
    sqlx::query_scalar!("SELECT pg_advisory_unlock($1)", PURGE_LOCK_KEY)
        .fetch_one(&mut *conn)
        .await?;
    result.map(|_| true)
}

async fn run_locked(state: &AppState) -> anyhow::Result<()> {
    delete_stale_unconfirmed(state).await?;
    prune_catalog_status(state).await?;
    Ok(())
}

/// `catalog_instance_status` rows whose heartbeat is older than 3 minutes,
/// left by instances that have stopped.
pub async fn prune_catalog_status(state: &AppState) -> anyhow::Result<u64> {
    let r = sqlx::query!(
        "DELETE FROM catalog_instance_status WHERE heartbeat_at < now() - interval '3 minutes'"
    )
    .execute(&state.db)
    .await?;
    Ok(r.rows_affected())
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
