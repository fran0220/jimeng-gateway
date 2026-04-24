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
    /// Full browser cookie jar string (all cookies including HttpOnly).
    cookie_jar: Option<String>,
}

#[derive(Deserialize)]
struct ToggleRequest {
    enabled: bool,
}

#[derive(Deserialize)]
struct UpdateCookieJarRequest {
    cookie_jar: String,
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
        .add_session(&label, &req.session_id, req.cookie_jar.as_deref())
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

/// Test if a session is still valid by calling jimeng API directly.
async fn test_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let sessions = state.pool.list_sessions().await;
    let session = sessions
        .iter()
        .find(|s| s.id == id)
        .ok_or(StatusCode::NOT_FOUND)?;

    let client = reqwest::Client::builder().user_agent("").build().unwrap();
    let headers = crate::jimeng::auth::build_headers_with_cookies(
        &session.session_id,
        "/mweb/v1/get_history_by_ids",
        session.cookie_jar.as_deref(),
    );
    let params = crate::jimeng::auth::standard_query_params_with_jar(session.cookie_jar.as_deref());

    let resp = client
        .post("https://jimeng.jianying.com/mweb/v1/get_history_by_ids")
        .headers(headers)
        .query(&params)
        .json(&serde_json::json!({ "history_ids": [] }))
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
                "message": format!("Jimeng API returned {status}"),
                "detail": text,
            })))
        }
        Err(e) => Ok(Json(serde_json::json!({
            "ok": false,
            "message": format!("Connection failed: {e}"),
        }))),
    }
}

/// Trigger cookie harvesting for a session via headless browser.
async fn harvest_cookies(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let sessions = state.pool.list_sessions().await;
    let session = sessions
        .iter()
        .find(|s| s.id == id)
        .ok_or((StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "session not found"}))))?;

    let cookie_jar = state.browser.harvest_cookies(&session.session_id).await
        .map_err(|e| (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Cookie harvest failed: {e}")})),
        ))?;

    let cookie_count = cookie_jar.split("; ").count();

    state.pool.update_cookie_jar(&id, &cookie_jar).await
        .map_err(|e| (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        ))?;

    Ok(Json(serde_json::json!({
        "ok": true,
        "cookies_harvested": cookie_count,
        "cookie_jar_length": cookie_jar.len(),
    })))
}

async fn update_cookie_jar(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateCookieJarRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let updated = state
        .pool
        .update_cookie_jar(&id, &req.cookie_jar)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if updated {
        Ok(Json(serde_json::json!({ "ok": true })))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/sessions", get(list_sessions).post(add_session))
        .route("/sessions/{id}", delete(remove_session))
        .route("/sessions/{id}", patch(toggle_session))
        .route("/sessions/{id}/test", post(test_session))
        .route("/sessions/{id}/cookies", patch(update_cookie_jar))
        .route("/sessions/{id}/harvest", post(harvest_cookies))
        .with_state(state)
}
