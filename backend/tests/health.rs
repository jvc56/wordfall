//! PLAN.md § API → Admin: `GET /health` checks the database connection and
//! that every catalog item present at startup is indexed.

mod common;

use axum::http::StatusCode;

#[sqlx::test]
async fn health_answers_once_migrated(pool: sqlx::PgPool) {
    let app = common::TestApp::new(pool).await;
    let resp = app.get("/health").await;
    assert_eq!(resp.status, StatusCode::OK);
}

#[sqlx::test]
async fn health_not_ready_until_catalog_indexed(pool: sqlx::PgPool) {
    let app = common::TestApp::new_unready(pool).await;
    let resp = app.get("/health").await;
    assert_eq!(resp.status, StatusCode::SERVICE_UNAVAILABLE);
}
