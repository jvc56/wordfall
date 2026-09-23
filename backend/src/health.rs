//! `GET /health` checks the database connection and that every catalog item
//! present at startup is indexed (PLAN.md § API → Admin).

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::app::AppState;

pub async fn health(State(state): State<AppState>) -> (StatusCode, Json<Value>) {
    let db_ok = sqlx::query_scalar!("SELECT 1 AS \"one!\"")
        .fetch_one(&state.db)
        .await
        .is_ok();
    let catalog_ok = state.is_catalog_ready();
    let status = if db_ok && catalog_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        Json(json!({ "database": db_ok, "catalog_indexed": catalog_ok })),
    )
}
