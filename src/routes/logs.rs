use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    routing::get,
};
use serde::Deserialize;

use crate::AppState;

#[derive(Deserialize)]
struct LogParams {
    lines: Option<usize>,
    since: Option<i64>,
}

async fn get_logs(
    State(state): State<Arc<AppState>>,
    Query(params): Query<LogParams>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let lines = params.lines.unwrap_or(100);
    let logs = state
        .docker
        .get_logs(lines, params.since)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({ "logs": logs, "count": logs.len() })))
}

async fn health(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let container_running = state.docker.is_running().await;
    let container_info = state.docker.container_info().await.unwrap_or_default();
    let stats = state.queue.stats().await.unwrap_or_default();
    let sessions = state.pool.list_sessions().await;

    let healthy_sessions = sessions.iter().filter(|s| s.enabled && s.healthy).count();

    Json(serde_json::json!({
        "ok": container_running && healthy_sessions > 0,
        "gateway_version": env!("CARGO_PKG_VERSION"),
        "container": container_info,
        "sessions": {
            "total": sessions.len(),
            "healthy": healthy_sessions,
        },
        "tasks": stats,
    }))
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/logs", get(get_logs))
        .route("/health", get(health))
        .with_state(state)
}
