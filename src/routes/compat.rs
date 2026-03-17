use std::sync::Arc;
use std::time::Duration;

use axum::{
    Extension, Json, Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};

use crate::AppState;
use crate::auth::middleware::{Caller, require_scope};
use crate::auth::usage as usage_tracker;
use crate::queue::{CreateTaskRequest, TaskStatus};

/// Compatibility layer: accepts the same API format as jimeng-free-api-all
/// but converts to async task model internally.
///
/// `POST /v1/videos/generations` → enqueue task, return task info.
/// `GET /v1/models` → proxy to upstream.
/// `GET /ping` → health check.
async fn compat_video_generations(
    State(state): State<Arc<AppState>>,
    Extension(caller): Extension<Caller>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    // Scope check
    if let Err(resp) = require_scope(&caller, "video:create") {
        let (parts, body) = resp.into_parts();
        let bytes = axum::body::to_bytes(body, 4096).await.unwrap_or_default();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
        return Err((parts.status, Json(json)));
    }

    // Daily quota check for API key callers
    if let Caller::ApiKey { ref key_id, daily_quota, .. } = caller {
        if daily_quota > 0 {
            let today_tasks = usage_tracker::today_task_count(&state.db.pool, key_id)
                .await
                .unwrap_or(0);
            if today_tasks >= daily_quota {
                return Err((
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(serde_json::json!({
                        "error": "Daily quota exceeded",
                        "daily_quota": daily_quota,
                        "used": today_tasks,
                    })),
                ));
            }
        }
    }
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
        resolution: None,
        files: None,
    };

    let task = state
        .queue
        .enqueue(req, Some(body.to_vec()), Some(content_type.to_string()))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        })?;

    // Record task creation for daily quota tracking
    if let Some(key_id) = caller.key_id() {
        usage_tracker::record_task(&state.db.pool, key_id).await;
    }

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

async fn compat_models() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "object": "list",
        "data": [
            { "id": "seedance-2.0", "object": "model" },
            { "id": "seedance-2.0-pro", "object": "model" },
            { "id": "seedance-2.0-fast", "object": "model" },
            { "id": "jimeng-5.0", "object": "model" },
        ]
    }))
}

async fn compat_ping() -> &'static str {
    "pong"
}

/// Parse OpenAI `size` field (e.g. "1024x1024", "2560x1440") into (ratio, resolution).
///
/// First tries exact match against all supported pixel dimensions.
/// Falls back to nearest ratio/tier for non-standard sizes.
fn parse_openai_size(size: &str) -> Result<(String, String), String> {
    let parts: Vec<&str> = size.split('x').collect();
    if parts.len() != 2 {
        return Err(format!("Invalid size format: \"{size}\". Use WIDTHxHEIGHT (e.g. \"1024x1024\")."));
    }
    let w: u32 = parts[0].parse().map_err(|_| format!("Invalid width in size: \"{size}\""))?;
    let h: u32 = parts[1].parse().map_err(|_| format!("Invalid height in size: \"{size}\""))?;

    // Exact match against supported resolution table
    if let Some((ratio, res)) = crate::jimeng::models::lookup_image_size(w, h) {
        return Ok((ratio.to_string(), res.to_string()));
    }

    Err(format!(
        "Unsupported size: \"{size}\". Supported sizes: \
         1k: 1024x1024, 768x1024, 1024x768, 1024x576, 576x1024, 1024x682, 682x1024, 1195x512 | \
         2k: 2048x2048, 2304x1728, 1728x2304, 2560x1440, 1440x2560, 2496x1664, 1664x2496, 3024x1296 | \
         4k: 4096x4096, 4608x3456, 3456x4608, 5120x2880, 2880x5120, 4992x3328, 3328x4992, 6048x2592"
    ))
}

/// OpenAI-compatible `POST /v1/images/generations`.
///
/// Accepts standard OpenAI fields: `prompt`, `model`, `size`, `n`, `response_format`.
/// Also accepts extensions: `ratio`, `resolution`.
///
/// Synchronous: enqueues task, waits for completion, returns OpenAI response format.
async fn compat_image_generations(
    State(state): State<Arc<AppState>>,
    Extension(caller): Extension<Caller>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // Scope check
    if let Err(resp) = require_scope(&caller, "video:create") {
        let (parts, body) = resp.into_parts();
        let bytes = axum::body::to_bytes(body, 4096).await.unwrap_or_default();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
        return Err((parts.status, Json(json)));
    }

    // Daily quota check for API key callers
    if let Caller::ApiKey { ref key_id, daily_quota, .. } = caller {
        if daily_quota > 0 {
            let today_tasks = usage_tracker::today_task_count(&state.db.pool, key_id)
                .await
                .unwrap_or(0);
            if today_tasks >= daily_quota {
                return Err((
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(serde_json::json!({
                        "error": {
                            "message": "Daily quota exceeded",
                            "type": "rate_limit_error",
                            "code": "daily_quota_exceeded"
                        }
                    })),
                ));
            }
        }
    }

    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // Parse request fields
    let (prompt, model, mut ratio, mut resolution) = if content_type.contains("multipart") {
        let (p, m, _dur, r) = extract_multipart_fields(content_type, &body);
        (p, m, r, None::<String>)
    } else {
        match serde_json::from_slice::<serde_json::Value>(&body) {
            Ok(v) => {
                // If OpenAI `size` field is provided, parse it into ratio/resolution
                let (size_ratio, size_resolution) = if let Some(size) = v.get("size").and_then(|v| v.as_str()) {
                    let (r, res) = parse_openai_size(size).map_err(|e| {
                        (StatusCode::BAD_REQUEST, Json(serde_json::json!({
                            "error": { "message": e, "type": "invalid_request_error", "code": "invalid_size" }
                        })))
                    })?;
                    (Some(r), Some(res))
                } else {
                    (None, None)
                };

                (
                    v.get("prompt").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    v.get("model").and_then(|v| v.as_str()).map(String::from),
                    v.get("ratio").and_then(|v| v.as_str()).map(String::from).or(size_ratio),
                    v.get("resolution").and_then(|v| v.as_str()).map(String::from).or(size_resolution),
                )
            }
            Err(_) => ("".to_string(), None, None, None),
        }
    };

    if prompt.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": {
                    "message": "prompt is required",
                    "type": "invalid_request_error",
                    "code": "missing_required_parameter"
                }
            })),
        ));
    }

    // Apply defaults
    if ratio.is_none() { ratio = Some("1:1".to_string()); }
    if resolution.is_none() { resolution = Some("2k".to_string()); }

    let req = CreateTaskRequest {
        prompt,
        duration: None,
        ratio,
        model: model.or_else(|| Some("jimeng-5.0".to_string())),
        resolution,
        files: None,
    };

    let task = state
        .queue
        .enqueue(req, None, None)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": { "message": e.to_string(), "type": "server_error" }
                })),
            )
        })?;

    // Record task creation for daily quota tracking
    if let Some(key_id) = caller.key_id() {
        usage_tracker::record_task(&state.db.pool, key_id).await;
    }

    let task_id = task.id.clone();

    // Synchronous wait: poll DB until task completes (up to max_poll_duration)
    let max_wait = Duration::from_secs(state.config.max_poll_duration_secs.max(60) + 30);
    let poll_interval = Duration::from_secs(2);
    let deadline = tokio::time::Instant::now() + max_wait;

    loop {
        tokio::time::sleep(poll_interval).await;

        let task = state.queue.get_task(&task_id).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": { "message": e.to_string(), "type": "server_error" }
                })),
            )
        })?;

        let task = match task {
            Some(t) => t,
            None => return Err((
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": { "message": "Task not found", "type": "server_error" }
                })),
            )),
        };

        match task.status {
            TaskStatus::Succeeded => {
                let created = chrono::Utc::now().timestamp();
                let urls: Vec<&str> = task.video_url.as_deref()
                    .unwrap_or("")
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .collect();

                let data: Vec<serde_json::Value> = urls.iter().map(|url| {
                    serde_json::json!({ "url": url, "revised_prompt": task.prompt })
                }).collect();

                return Ok(Json(serde_json::json!({
                    "created": created,
                    "data": data,
                })));
            }
            TaskStatus::Failed | TaskStatus::Cancelled => {
                let err_msg = task.error_message.unwrap_or_else(|| "Generation failed".to_string());
                let err_kind = task.error_kind.unwrap_or_else(|| "unknown".to_string());

                let (status, code) = match err_kind.as_str() {
                    "content_risk" => (StatusCode::BAD_REQUEST, "content_policy_violation"),
                    "quota" => (StatusCode::TOO_MANY_REQUESTS, "rate_limit_exceeded"),
                    "auth" | "account_blocked" => (StatusCode::UNAUTHORIZED, "authentication_error"),
                    _ => (StatusCode::INTERNAL_SERVER_ERROR, "server_error"),
                };

                return Err((
                    status,
                    Json(serde_json::json!({
                        "error": {
                            "message": err_msg,
                            "type": code,
                            "code": err_kind,
                        }
                    })),
                ));
            }
            _ => {
                // Still in progress
                if tokio::time::Instant::now() >= deadline {
                    return Err((
                        StatusCode::GATEWAY_TIMEOUT,
                        Json(serde_json::json!({
                            "error": {
                                "message": "Image generation timed out",
                                "type": "timeout_error",
                                "code": "timeout"
                            }
                        })),
                    ));
                }
            }
        }
    }
}

pub fn compat_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/v1/videos/generations", post(compat_video_generations))
        .route("/v1/images/generations", post(compat_image_generations))
        .route("/v1/models", get(compat_models))
        .with_state(state)
}

/// Unauthenticated health check route
pub fn ping_router() -> Router {
    Router::new().route("/ping", get(compat_ping))
}
