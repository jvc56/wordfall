//! Wordfall backend (PLAN.md § Architecture).

pub mod app;
pub mod auth;
pub mod cascade;
pub mod catalog;
pub mod clock;
pub mod config;
pub mod error;
pub mod extract;
pub mod health;
pub mod leave;
pub mod net;
pub mod purge;
pub mod rate;
pub mod search;

pub use app::{AppState, build_router};
