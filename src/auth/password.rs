use std::sync::Arc;

use axum::{Json, extract::State, http::StatusCode};
use serde::Deserialize;

use super::backend::AuthSession;
use crate::AppState;

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

/// POST /auth/login — Password-based login
pub async fn login(
    State(state): State<Arc<AppState>>,
    mut auth_session: AuthSession,
    Json(req): Json<LoginRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !state.config.auth_enabled {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Auth not enabled"})),
        ));
    }

    let creds = super::backend::PasswordCredentials {
        username: req.username,
        password: req.password,
    };

    let user = auth_session
        .authenticate(creds)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("{e}")})),
            )
        })?
        .ok_or((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Invalid credentials"})),
        ))?;

    auth_session.login(&user).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Login failed: {e}")})),
        )
    })?;

    Ok(Json(serde_json::json!({
        "ok": true,
        "user": { "id": user.id, "name": user.name, "email": user.email }
    })))
}

/// GET /auth/me — Current user info
pub async fn me(auth_session: AuthSession) -> Result<Json<serde_json::Value>, StatusCode> {
    let user = auth_session.user.ok_or(StatusCode::UNAUTHORIZED)?;
    Ok(Json(serde_json::json!({
        "user": {
            "id": user.id,
            "name": user.name,
            "email": user.email,
        }
    })))
}

/// POST /auth/logout
pub async fn logout(
    mut auth_session: AuthSession,
) -> Result<Json<serde_json::Value>, StatusCode> {
    auth_session
        .logout()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({"ok": true})))
}
