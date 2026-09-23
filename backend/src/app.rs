//! Application state, router and startup.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use axum::routing::get;
use axum::Router;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    /// Set once every catalog item present at startup has been indexed.
    pub catalog_ready: Arc<AtomicBool>,
}

impl AppState {
    pub fn new(db: PgPool, config: Config) -> Self {
        AppState {
            db,
            config: Arc::new(config),
            catalog_ready: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_catalog_ready(&self) -> bool {
        self.catalog_ready.load(Ordering::Acquire)
    }
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(crate::health::health))
        .with_state(state)
}

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Connect, run the migration (SQLx holds an advisory lock around it, so two
/// tasks starting together are safe), load the catalog, then bind.
pub async fn run(config: Config) -> anyhow::Result<()> {
    let db = PgPoolOptions::new()
        .max_connections(20)
        .connect(&config.database_url)
        .await?;
    MIGRATOR.run(&db).await?;
    let bind_addr = config.bind_addr;
    let state = AppState::new(db, config);
    // Nothing to index until the catalog exists.
    state.catalog_ready.store(true, Ordering::Release);

    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    tracing::info!(%bind_addr, "listening");
    axum::serve(
        listener,
        build_router(state).into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = term => {} }
}
