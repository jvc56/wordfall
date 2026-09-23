//! Wordfall backend (PLAN.md § Architecture).

pub mod app;
pub mod auth;
pub mod catalog;
pub mod clock;
pub mod config;
pub mod error;
pub mod extract;
pub mod health;
pub mod net;
pub mod purge;
pub mod rate;

pub use app::{AppState, build_router};
