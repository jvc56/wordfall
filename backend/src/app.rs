//! Application state, router and startup.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::get;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tower_http::compression::CompressionLayer;

use crate::auth::mail::{ConsoleMailer, EmailCaps, Mailer};
use crate::auth::tokens::Keys;
use crate::catalog::Catalog;
use crate::clock::Clock;
use crate::config::{Config, MailBackend};
use crate::rate::Limiters;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub keys: Arc<Keys>,
    pub clock: Arc<Clock>,
    pub limits: Arc<Limiters>,
    pub mailer: Arc<dyn Mailer>,
    pub email_caps: Arc<EmailCaps>,
    pub catalog: Arc<Catalog>,
    /// `SEARCH_CONCURRENCY` permits, taken before `spawn_blocking`.
    pub search_permits: Arc<tokio::sync::Semaphore>,
    /// Set once every catalog item present at startup has been indexed.
    pub catalog_ready: Arc<AtomicBool>,
    /// Argon2 verifies run by login, for the log and the rate-limit tests.
    pub argon2_verifies: Arc<AtomicU64>,
}

impl AppState {
    pub fn new(db: PgPool, config: Config, mailer: Arc<dyn Mailer>) -> Self {
        let clock = Arc::new(Clock::system());
        AppState {
            db,
            keys: Arc::new(Keys::new(config.session_signing_key)),
            limits: Arc::new(Limiters::new(&config)),
            email_caps: Arc::new(EmailCaps::new(clock.clone())),
            clock,
            mailer,
            catalog: Arc::new(Catalog::default()),
            search_permits: Arc::new(tokio::sync::Semaphore::new(config.search_concurrency as usize)),
            config: Arc::new(config),
            catalog_ready: Arc::new(AtomicBool::new(false)),
            argon2_verifies: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn is_catalog_ready(&self) -> bool {
        self.catalog_ready.load(Ordering::Acquire)
    }
}

pub fn build_router(state: AppState) -> Router {
    let api_limit = state.config.api_max_body_bytes as usize;
    Router::new()
        .route("/health", get(crate::health::health))
        .merge(crate::auth::routes::router())
        .merge(crate::catalog::routes::router(&state))
        .merge(crate::search::routes::router())
        .merge(crate::search::saved::router())
        .merge(crate::cascade::routes::router())
        .merge(crate::sync::routes::router())
        .merge(crate::export::routes())
        .layer(DefaultBodyLimit::max(api_limit))
        .layer(CompressionLayer::new().gzip(true).br(true))
        .with_state(state)
}

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

pub fn mailer_for(config: &Config) -> anyhow::Result<Arc<dyn Mailer>> {
    match config.mail_backend {
        MailBackend::Console => Ok(Arc::new(ConsoleMailer)),
        MailBackend::Ses => anyhow::bail!("MAIL_BACKEND=ses is not available in this build yet"),
    }
}

/// Connect, run the migration (SQLx holds an advisory lock around it, so two
/// tasks starting together are safe), load the catalog, then bind.
pub async fn run(config: Config) -> anyhow::Result<()> {
    let db = PgPoolOptions::new()
        .max_connections(20)
        .connect(&config.database_url)
        .await?;
    MIGRATOR.run(&db).await?;
    let bind_addr = config.bind_addr;
    let mailer = mailer_for(&config)?;
    let state = AppState::new(db, config, mailer);
    // Bind first so /health can answer "not ready" while the catalog loads.
    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    tracing::info!(%bind_addr, instance_id = %state.catalog.instance_id, "listening");
    let st = state.clone();
    tokio::spawn(async move {
        if let Err(e) = crate::catalog::startup(&st).await {
            tracing::error!(error = %e, "catalog startup load failed");
            std::process::exit(1);
        }
        tracing::info!("catalog loaded; ready");
    });
    crate::purge::spawn(state.clone());

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
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = term => {} }
}
