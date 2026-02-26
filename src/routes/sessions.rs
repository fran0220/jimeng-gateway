use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, patch, post},
};
use serde::Deserialize;

use crate::AppState;

#[derive(Deserialize)]
struct AddSessionRequest {
    label: Option<String>,
    session_id: String,
}

#[derive(Deserialize)]
struct ToggleRequest {
    enabled: bool,
}

async fn list_sessions(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let sessions: Vec<_> = state
        .pool
        .list_sessions()
        .await
        .into_iter()
        .map(|s| s.masked())
        .collect();

    Json(serde_json::json!({ "sessions": sessions }))
}

async fn add_session(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AddSessionRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    let label = req.label.unwrap_or_default();

    let session = state
        .pool
        .add_session(&label, &req.session_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        })?;

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "session": session.masked() })),
    ))
}

async fn remove_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let removed = state
        .pool
        .remove_session(&id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if removed {
        Ok(Json(serde_json::json!({ "ok": true })))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

async fn toggle_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<ToggleRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let toggled = state
        .pool
        .toggle_session(&id, req.enabled)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if toggled {
        Ok(Json(serde_json::json!({ "ok": true, "enabled": req.enabled })))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

/// Test if a session is still valid by calling upstream /ping with auth.
async fn test_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sessions = state.pool.list_sessions().await;
    let session = sessions
        .iter()
        .find(|s| s.id == id)
        .ok_or(StatusCode::NOT_FOUND)?;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/ping", state.config.jimeng_upstream))
        .header("Authorization", format!("Bearer {}", session.session_id))
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {
            Ok(Json(serde_json::json!({ "ok": true, "message": "Session is valid" })))
        }
        Ok(r) => {
            let status = r.status().as_u16();
            let text = r.text().await.unwrap_or_default();
            Ok(Json(serde_json::json!({
                "ok": false,
                "message": format!("Upstream returned {status}"),
                "detail": text,
            })))
        }
        Err(e) => Ok(Json(serde_json::json!({
            "ok": false,
            "message": format!("Connection failed: {e}"),
        }))),
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/sessions", get(list_sessions).post(add_session))
        .route("/sessions/{id}", delete(remove_session))
        .route("/sessions/{id}", patch(toggle_session))
        .route("/sessions/{id}/test", post(test_session))
        .with_state(state)
}
