//! The in-memory catalog (PLAN.md § Catalog Indexes, § Admin → Loading changes
//! into running servers).
//!
//! Each instance builds a read-only index for every lexicon and leave value
//! set, reconciles with the database on `NOTIFY catalog_changed` and every
//! `CATALOG_RECONCILE_SECONDS`, and records what it has loaded in
//! `catalog_instance_status` — but writes no rows until its startup load is
//! complete, the moment `/health` turns ready.

#[cfg(test)]
pub mod fixtures;
pub mod index;
pub mod pos;
pub mod probability;
pub mod routes;
pub mod store;
pub mod tiles;
pub mod upload;

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use sqlx::postgres::PgListener;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::app::AppState;
use index::{LeaveSetIndex, LexiconIndex};
use tiles::Distribution;

pub const CHANNEL: &str = "catalog_changed";
pub const HEARTBEAT_SECONDS: u64 = 60;
/// Rows older than this belong to a dead instance: ignored, then pruned.
pub const LIVE_WINDOW: &str = "3 minutes";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, sqlx::Type, serde::Serialize)]
#[sqlx(type_name = "catalog_item_kind", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    LetterDistribution,
    Lexicon,
    LeaveSet,
}

/// What one instance has built. Replaced whole on every change, so a reader
/// holding an `Arc` sees one consistent catalog.
#[derive(Default, Clone)]
pub struct Snapshot {
    pub distributions: HashMap<i16, Arc<Distribution>>,
    pub lexicons: HashMap<i16, Arc<LexiconIndex>>,
    pub leave_sets: HashMap<i32, Arc<LeaveSetIndex>>,
}

impl Snapshot {
    pub fn lexicon_by_name(&self, name: &str) -> Option<&Arc<LexiconIndex>> {
        self.lexicons.values().find(|l| l.name == name)
    }

    pub fn leave_set_for(&self, lexicon_id: i16) -> Option<&Arc<LeaveSetIndex>> {
        self.leave_sets
            .values()
            .find(|s| s.lexicon_id == lexicon_id)
    }
}

pub struct Catalog {
    pub instance_id: Uuid,
    snapshot: RwLock<Arc<Snapshot>>,
    reconcile_lock: Mutex<()>,
}

impl Default for Catalog {
    fn default() -> Self {
        Catalog {
            instance_id: Uuid::new_v4(),
            snapshot: RwLock::new(Arc::new(Snapshot::default())),
            reconcile_lock: Mutex::new(()),
        }
    }
}

impl Catalog {
    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.snapshot.read().expect("catalog lock").clone()
    }

    fn update(&self, f: impl FnOnce(&mut Snapshot)) {
        let mut guard = self.snapshot.write().expect("catalog lock");
        let mut next = (**guard).clone();
        f(&mut next);
        *guard = Arc::new(next);
    }
}

/// Builds every item in the database, writes this instance's status rows,
/// and turns `/health` ready. Then keeps the catalog current in the background.
pub async fn startup(state: &AppState) -> anyhow::Result<()> {
    reconcile(state).await?;
    write_all_status(state).await?;
    state.catalog_ready.store(true, Ordering::Release);
    spawn_background(state.clone());
    Ok(())
}

pub fn spawn_background(state: AppState) {
    // LISTEN catalog_changed.
    let st = state.clone();
    tokio::spawn(async move {
        // Its own one-connection pool, so the held connection never takes a
        // slot from the pool requests use.
        let listen_pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .idle_timeout(None)
            .max_lifetime(None)
            .connect_lazy_with((*st.db.connect_options()).clone());
        loop {
            match PgListener::connect_with(&listen_pool).await {
                Ok(mut listener) => {
                    if listener.listen(CHANNEL).await.is_ok() {
                        // Catch anything that changed while not listening.
                        let _ = reconcile(&st).await;
                        while listener.recv().await.is_ok() {
                            if let Err(e) = reconcile(&st).await {
                                tracing::error!(error = %e, "catalog reconcile failed");
                            }
                        }
                    }
                }
                Err(e) => tracing::warn!(error = %e, "catalog listener connect failed"),
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
    // The fallback reconcile.
    let st = state.clone();
    tokio::spawn(async move {
        let period = Duration::from_secs(st.config.catalog_reconcile_seconds.max(1));
        let mut tick = tokio::time::interval(period);
        tick.tick().await;
        loop {
            tick.tick().await;
            if let Err(e) = reconcile(&st).await {
                tracing::error!(error = %e, "catalog reconcile failed");
            }
        }
    });
    // The heartbeat.
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(HEARTBEAT_SECONDS));
        loop {
            tick.tick().await;
            if let Err(e) = heartbeat(&state).await {
                tracing::warn!(error = %e, "catalog heartbeat failed");
            }
        }
    });
}

pub async fn heartbeat(state: &AppState) -> anyhow::Result<()> {
    sqlx::query!(
        "UPDATE catalog_instance_status SET heartbeat_at = now() WHERE instance_id = $1",
        state.catalog.instance_id
    )
    .execute(&state.db)
    .await?;
    Ok(())
}

async fn write_status(state: &AppState, kind: ItemKind, id: i32) -> anyhow::Result<()> {
    if !state.catalog_ready.load(Ordering::Acquire) {
        return Ok(());
    }
    sqlx::query!(
        "INSERT INTO catalog_instance_status (instance_id, item_kind, item_id)
         VALUES ($1, $2, $3)
         ON CONFLICT (instance_id, item_kind, item_id)
         DO UPDATE SET loaded_at = now(), heartbeat_at = now()",
        state.catalog.instance_id,
        kind as ItemKind,
        id,
    )
    .execute(&state.db)
    .await?;
    Ok(())
}

async fn write_all_status(state: &AppState) -> anyhow::Result<()> {
    let snap = state.catalog.snapshot();
    let mut tx = state.db.begin().await?;
    let items: Vec<(ItemKind, i32)> = snap
        .distributions
        .keys()
        .map(|&id| (ItemKind::LetterDistribution, i32::from(id)))
        .chain(
            snap.lexicons
                .keys()
                .map(|&id| (ItemKind::Lexicon, i32::from(id))),
        )
        .chain(snap.leave_sets.keys().map(|&id| (ItemKind::LeaveSet, id)))
        .collect();
    for (kind, id) in items {
        sqlx::query!(
            "INSERT INTO catalog_instance_status (instance_id, item_kind, item_id)
             VALUES ($1, $2, $3)
             ON CONFLICT (instance_id, item_kind, item_id)
             DO UPDATE SET loaded_at = now(), heartbeat_at = now()",
            state.catalog.instance_id,
            kind as ItemKind,
            id,
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

async fn drop_status(state: &AppState, kind: ItemKind, id: i32) -> anyhow::Result<()> {
    sqlx::query!(
        "DELETE FROM catalog_instance_status
         WHERE instance_id = $1 AND item_kind = $2 AND item_id = $3",
        state.catalog.instance_id,
        kind as ItemKind,
        id,
    )
    .execute(&state.db)
    .await?;
    Ok(())
}

/// Builds indexes for new items and drops deleted ones.
pub async fn reconcile(state: &AppState) -> anyhow::Result<()> {
    let _guard = state.catalog.reconcile_lock.lock().await;
    let items = store::list_items(&state.db).await?;
    let snap = state.catalog.snapshot();

    // Drop what the database no longer has, leaving its status rows at once.
    for &id in snap.leave_sets.keys() {
        if !items.leave_sets.iter().any(|&(s, _)| s == id) {
            state.catalog.update(|s| {
                s.leave_sets.remove(&id);
            });
            drop_status(state, ItemKind::LeaveSet, id).await?;
        }
    }
    for &id in snap.lexicons.keys() {
        if !items.lexicons.iter().any(|&(l, _)| l == id) {
            state.catalog.update(|s| {
                s.lexicons.remove(&id);
            });
            drop_status(state, ItemKind::Lexicon, i32::from(id)).await?;
        }
    }
    for &id in snap.distributions.keys() {
        if !items.distributions.contains(&id) {
            state.catalog.update(|s| {
                s.distributions.remove(&id);
            });
            drop_status(state, ItemKind::LetterDistribution, i32::from(id)).await?;
        }
    }

    // Build what is new: distributions, then lexicons, then leave sets.
    for &id in &items.distributions {
        if state.catalog.snapshot().distributions.contains_key(&id) {
            continue;
        }
        let Some(d) = store::load_distribution(&state.db, id).await? else {
            continue;
        };
        state.catalog.update(|s| {
            s.distributions.insert(id, Arc::new(d));
        });
        write_status(state, ItemKind::LetterDistribution, i32::from(id)).await?;
    }
    for &(id, dist_id) in &items.lexicons {
        if state.catalog.snapshot().lexicons.contains_key(&id) {
            continue;
        }
        let Some(dist) = state
            .catalog
            .snapshot()
            .distributions
            .get(&dist_id)
            .cloned()
        else {
            continue;
        };
        let Some((name, raw)) = store::load_lexicon(&state.db, id, &dist).await? else {
            continue;
        };
        let index =
            tokio::task::spawn_blocking(move || LexiconIndex::build(id, &name, dist, raw)).await?;
        state.catalog.update(|s| {
            s.lexicons.insert(id, Arc::new(index));
        });
        write_status(state, ItemKind::Lexicon, i32::from(id)).await?;
    }
    for &(id, lexicon_id) in &items.leave_sets {
        if state.catalog.snapshot().leave_sets.contains_key(&id) {
            continue;
        }
        let Some(lexicon) = state.catalog.snapshot().lexicons.get(&lexicon_id).cloned() else {
            continue;
        };
        let Some(raw) = store::load_leaves(&state.db, id, &lexicon.distribution).await? else {
            continue;
        };
        let index =
            tokio::task::spawn_blocking(move || LeaveSetIndex::build(id, &lexicon, raw)).await?;
        state.catalog.update(|s| {
            s.leave_sets.insert(id, Arc::new(index));
        });
        write_status(state, ItemKind::LeaveSet, id).await?;
    }
    Ok(())
}
