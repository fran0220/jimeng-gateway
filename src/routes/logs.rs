use std::sync::Arc;

use axum::{
    Json, Router,
    extract::State,
    routing::get,
};

use crate::AppState;

async fn health(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let stats = state.queue.stats().await.unwrap_or_default();
    let sessions = state.pool.list_sessions().await;
    let healthy_sessions = sessions.iter().filter(|s| s.enabled && s.healthy).count();

    Json(serde_json::json!({
        "ok": healthy_sessions > 0,
        "gateway_version": env!("CARGO_PKG_VERSION"),
        "sessions": {
            "total": sessions.len(),
            "healthy": healthy_sessions,
        },
        "tasks": stats,
    }))
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .with_state(state)
}
