//! Wordfall backend (PLAN.md § Architecture).

pub mod app;
pub mod config;
pub mod health;

pub use app::{build_router, AppState};
