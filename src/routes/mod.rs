pub mod auth_routes;
pub mod compat;
mod keys;
mod logs;
mod me;
mod sessions;
mod tasks;
mod usage;

use std::sync::Arc;

use axum::Router;

use crate::AppState;

/// Admin API routes (sessions + logs + keys + usage management) — protected by auth middleware
pub fn admin_api_router(state: Arc<AppState>) -> Router {
    Router::new()
        .merge(sessions::router(state.clone()))
        .merge(logs::router(state.clone()))
        .merge(keys::router(state.clone()))
        .merge(usage::router(state))
}

/// Public API routes (tasks + stats + me) — always accessible
pub fn public_api_router(state: Arc<AppState>) -> Router {
    Router::new()
        .merge(tasks::router(state.clone()))
        .merge(me::router(state))
}
