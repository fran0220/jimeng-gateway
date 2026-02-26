use std::sync::Arc;

use axum::{
    Json, Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};

use crate::AppState;
use crate::queue::CreateTaskRequest;

/// Compatibility layer: accepts the same API format as jimeng-free-api-all
/// but converts to async task model internally.
///
/// `POST /v1/videos/generations` → enqueue task, return task info.
/// `GET /v1/models` → proxy to upstream.
/// `GET /ping` → health check.
async fn compat_video_generations(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    // Try to parse the multipart body to extract prompt/model/duration/ratio.
    // For now, store the raw body and forward it to upstream in the worker.
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // Extract fields from multipart or JSON body
    let (prompt, model, duration, ratio) = if content_type.contains("multipart") {
        extract_multipart_fields(content_type, &body)
    } else {
        // Try JSON
        match serde_json::from_slice::<serde_json::Value>(&body) {
            Ok(v) => (
                v.get("prompt").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                v.get("model").and_then(|v| v.as_str()).map(String::from),
                v.get("duration").and_then(|v| v.as_i64()).map(|v| v as i32),
                v.get("ratio").and_then(|v| v.as_str()).map(String::from),
            ),
            Err(_) => ("".to_string(), None, None, None),
        }
    };

    let req = CreateTaskRequest {
        prompt,
        duration,
        ratio,
        model,
        files: None,
    };

    let task = state
        .queue
        .enqueue(req, Some(body.to_vec()))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        })?;

    // Return in a format compatible with the jimeng API response structure,
    // but with additional task tracking info.
    Ok((
        StatusCode::ACCEPTED,
        Json(serde_json::json!({
            "code": 0,
            "message": "Task queued",
            "data": [{
                "task_id": task.id,
                "status": task.status,
            }],
            "task": {
                "id": task.id,
                "status": task.status,
                "poll_url": format!("/api/v1/tasks/{}", task.id),
            }
        })),
    ))
}

/// Parse multipart form data to extract text fields.
fn extract_multipart_fields(content_type: &str, body: &[u8]) -> (String, Option<String>, Option<i32>, Option<String>) {
    let boundary = content_type
        .split("boundary=")
        .nth(1)
        .unwrap_or("")
        .trim();

    if boundary.is_empty() {
        return (String::new(), None, None, None);
    }

    let body_str = String::from_utf8_lossy(body);
    let mut prompt = String::new();
    let mut model = None;
    let mut duration = None;
    let mut ratio = None;

    // Simple multipart parser for text fields
    for part in body_str.split(&format!("--{boundary}")) {
        if let Some(name_start) = part.find("name=\"") {
            let name_start = name_start + 6;
            if let Some(name_end) = part[name_start..].find('"') {
                let name = &part[name_start..name_start + name_end];

                // Skip file fields
                if part.contains("filename=\"") {
                    continue;
                }

                // Extract value (after double CRLF)
                if let Some(value_start) = part.find("\r\n\r\n") {
                    let value = part[value_start + 4..].trim_end_matches("\r\n").trim();
                    match name {
                        "prompt" => prompt = value.to_string(),
                        "model" => model = Some(value.to_string()),
                        "duration" => duration = value.parse().ok(),
                        "ratio" => ratio = Some(value.to_string()),
                        _ => {}
                    }
                }
            }
        }
    }

    (prompt, model, duration, ratio)
}

async fn compat_models(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/v1/models", state.config.jimeng_upstream))
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let body: serde_json::Value = resp.json().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    Ok(Json(body))
}

async fn compat_ping() -> &'static str {
    "pong"
}

pub fn compat_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/v1/videos/generations", post(compat_video_generations))
        .route("/v1/models", get(compat_models))
        .route("/ping", get(compat_ping))
        .with_state(state)
}
