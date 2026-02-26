pub mod compat;
mod logs;
mod sessions;
mod tasks;

use std::sync::Arc;

use axum::Router;

use crate::AppState;

pub fn api_router(state: Arc<AppState>) -> Router {
    Router::new()
        .merge(tasks::router(state.clone()))
        .merge(sessions::router(state.clone()))
        .merge(logs::router(state))
}
