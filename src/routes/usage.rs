use std::sync::Arc;

use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;

use crate::AppState;
use crate::auth::middleware::{Caller, require_scope};
use crate::auth::usage as usage_tracker;

#[derive(Deserialize)]
struct UsageParams {
    key_id: Option<String>,
    from: Option<String>,
    to: Option<String>,
}

async fn query_usage(
    State(state): State<Arc<AppState>>,
    Extension(caller): Extension<Caller>,
    Query(params): Query<UsageParams>,
) -> Result<Json<serde_json::Value>, Response> {
    require_scope(&caller, "admin")?;

    let rows = usage_tracker::query_usage(
        &state.db.pool,
        params.key_id.as_deref(),
        params.from.as_deref(),
        params.to.as_deref(),
    )
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response()
    })?;

    let total_requests: i32 = rows.iter().map(|r| r.request_count).sum();
    let total_tasks: i32 = rows.iter().map(|r| r.task_count).sum();

    Ok(Json(serde_json::json!({
        "usage": rows,
        "total": {
            "request_count": total_requests,
            "task_count": total_tasks,
        }
    })))
}

async fn usage_summary(
    State(state): State<Arc<AppState>>,
    Extension(caller): Extension<Caller>,
    Query(params): Query<UsageParams>,
) -> Result<Json<serde_json::Value>, Response> {
    require_scope(&caller, "admin")?;

    let summary = usage_tracker::usage_summary(
        &state.db.pool,
        params.from.as_deref(),
        params.to.as_deref(),
    )
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response()
    })?;

    Ok(Json(serde_json::json!({ "summary": summary })))
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/usage", get(query_usage))
        .route("/usage/summary", get(usage_summary))
        .with_state(state)
}
