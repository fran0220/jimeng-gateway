use std::sync::Arc;

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;

use crate::AppState;
use crate::auth::api_key;
use crate::auth::middleware::{Caller, require_scope};

fn require_admin_if_api_key(caller: Option<Extension<Caller>>) -> Result<(), Response> {
    if let Some(Extension(caller)) = caller {
        require_scope(&caller, "admin")?;
    }
    Ok(())
}

#[derive(Deserialize)]
struct CreateKeyRequest {
    name: String,
    rate_limit: Option<i32>,
    daily_quota: Option<i32>,
    scopes: Option<Vec<String>>,
    expires_at: Option<String>,
    metadata: Option<serde_json::Value>,
}

async fn create_key(
    State(state): State<Arc<AppState>>,
    caller: Option<Extension<Caller>>,
    Json(req): Json<CreateKeyRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), Response> {
    require_admin_if_api_key(caller)?;

    let scopes = req.scopes.unwrap_or_else(|| {
        vec![
            "video:create".into(),
            "task:read".into(),
            "task:cancel".into(),
        ]
    });
    let metadata = req.metadata.unwrap_or(serde_json::json!({}));

    let (raw_key, record) = api_key::create(
        &state.db.pool,
        &req.name,
        req.rate_limit.unwrap_or(60),
        req.daily_quota.unwrap_or(0),
        &scopes,
        req.expires_at.as_deref(),
        &metadata,
    )
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response()
    })?;

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "key": {
                "id": record.id,
                "name": record.name,
                "key": raw_key,
                "key_prefix": record.key_prefix,
                "rate_limit": record.rate_limit,
                "daily_quota": record.daily_quota,
                "scopes": record.scopes,
                "expires_at": record.expires_at,
                "created_at": record.created_at,
            }
        })),
    ))
}

async fn list_keys(
    State(state): State<Arc<AppState>>,
    caller: Option<Extension<Caller>>,
) -> Result<Json<serde_json::Value>, Response> {
    require_admin_if_api_key(caller)?;

    let keys = api_key::list_all(&state.db.pool).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response()
    })?;

    Ok(Json(serde_json::json!({ "keys": keys })))
}

async fn get_key(
    State(state): State<Arc<AppState>>,
    caller: Option<Extension<Caller>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, Response> {
    require_admin_if_api_key(caller)?;

    let key = api_key::get_by_id(&state.db.pool, &id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response()
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "Key not found" })),
            )
                .into_response()
        })?;

    Ok(Json(serde_json::json!({ "key": key })))
}

#[derive(Deserialize)]
struct UpdateKeyRequest {
    name: Option<String>,
    enabled: Option<bool>,
    rate_limit: Option<i32>,
    daily_quota: Option<i32>,
    scopes: Option<Vec<String>>,
    expires_at: Option<Option<String>>,
    metadata: Option<serde_json::Value>,
}

async fn update_key(
    State(state): State<Arc<AppState>>,
    caller: Option<Extension<Caller>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateKeyRequest>,
) -> Result<Json<serde_json::Value>, Response> {
    require_admin_if_api_key(caller)?;

    let patch = api_key::UpdateApiKey {
        name: req.name,
        enabled: req.enabled,
        rate_limit: req.rate_limit,
        daily_quota: req.daily_quota,
        scopes: req.scopes,
        expires_at: req.expires_at,
        metadata: req.metadata,
    };

    let updated = api_key::update(&state.db.pool, &id, &patch)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response()
        })?;

    if !updated {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Key not found" })),
        )
            .into_response());
    }

    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn delete_key(
    State(state): State<Arc<AppState>>,
    caller: Option<Extension<Caller>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, Response> {
    require_admin_if_api_key(caller)?;

    let deleted = api_key::delete(&state.db.pool, &id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response()
        })?;

    if deleted {
        state.rate_limiter.remove(&id);
        Ok(Json(serde_json::json!({ "ok": true })))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Key not found" })),
        )
            .into_response())
    }
}

async fn regenerate_key(
    State(state): State<Arc<AppState>>,
    caller: Option<Extension<Caller>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, Response> {
    require_admin_if_api_key(caller)?;

    let raw_key = api_key::regenerate(&state.db.pool, &id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response()
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "Key not found" })),
            )
                .into_response()
        })?;

    state.rate_limiter.remove(&id);

    Ok(Json(serde_json::json!({
        "key": raw_key,
        "key_prefix": api_key::key_prefix(&raw_key),
    })))
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/keys", get(list_keys).post(create_key))
        .route("/keys/{id}", get(get_key).patch(update_key).delete(delete_key))
        .route("/keys/{id}/regenerate", post(regenerate_key))
        .with_state(state)
}
