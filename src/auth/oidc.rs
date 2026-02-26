use std::sync::Arc;

use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::Redirect,
};
use openidconnect::{IssuerUrl, core::CoreProviderMetadata};
use serde::Deserialize;
use sha2::Digest;

use super::backend::{AdminUser, AuthSession};
use crate::AppState;

/// Discovered OIDC endpoints (cached after first discovery)
#[derive(Debug, Clone)]
struct OidcEndpoints {
    authorization_endpoint: String,
    token_endpoint: String,
    userinfo_endpoint: String,
}

/// Cached OIDC endpoints (lazy-initialized via discovery)
static OIDC_ENDPOINTS: tokio::sync::OnceCell<OidcEndpoints> =
    tokio::sync::OnceCell::const_new();

/// Discover OIDC endpoints from issuer URL using openidconnect provider metadata
async fn get_endpoints(config: &crate::config::Config) -> Result<&OidcEndpoints, String> {
    OIDC_ENDPOINTS
        .get_or_try_init(|| async {
            let issuer_url = IssuerUrl::new(
                config
                    .oidc_issuer_url
                    .clone()
                    .ok_or("OIDC_ISSUER_URL not set")?,
            )
            .map_err(|e| format!("Invalid issuer URL: {e}"))?;

            let http_client = reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .build()
                .map_err(|e| format!("HTTP client error: {e}"))?;

            let provider_metadata =
                CoreProviderMetadata::discover_async(issuer_url, &http_client)
                    .await
                    .map_err(|e| format!("OIDC discovery failed: {e}"))?;

            let auth_ep = provider_metadata
                .authorization_endpoint()
                .to_string();
            let token_ep = provider_metadata
                .token_endpoint()
                .ok_or("Provider has no token_endpoint")?
                .to_string();
            let userinfo_ep = provider_metadata
                .userinfo_endpoint()
                .ok_or("Provider has no userinfo_endpoint")?
                .url()
                .to_string();

            Ok(OidcEndpoints {
                authorization_endpoint: auth_ep,
                token_endpoint: token_ep,
                userinfo_endpoint: userinfo_ep,
            })
        })
        .await
        .map_err(|e: String| e)
}

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap()
}

/// GET /auth/login — Redirect to OIDC provider
pub async fn login(
    State(state): State<Arc<AppState>>,
) -> Result<Redirect, (StatusCode, Json<serde_json::Value>)> {
    if !state.config.auth_enabled {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Auth not enabled"})),
        ));
    }

    let endpoints = get_endpoints(&state.config).await.map_err(|e| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": e})),
        )
    })?;

    let client_id = state
        .config
        .oidc_client_id
        .as_deref()
        .unwrap_or("jimeng-gateway");
    let redirect_url = state
        .config
        .oidc_redirect_url
        .as_deref()
        .unwrap_or("http://localhost:5100/auth/callback");

    let state_param = uuid::Uuid::new_v4().to_string();
    let auth_url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope=openid+email+profile&state={}",
        endpoints.authorization_endpoint,
        urlencoding::encode(client_id),
        urlencoding::encode(redirect_url),
        urlencoding::encode(&state_param),
    );

    Ok(Redirect::temporary(&auth_url))
}

#[derive(Deserialize)]
pub struct CallbackParams {
    pub code: String,
    #[allow(dead_code)]
    pub state: Option<String>,
}

/// GET /auth/callback — Handle OIDC callback, exchange code, login user
pub async fn callback(
    State(state): State<Arc<AppState>>,
    mut auth_session: AuthSession,
    Query(params): Query<CallbackParams>,
) -> Result<Redirect, (StatusCode, Json<serde_json::Value>)> {
    let endpoints = get_endpoints(&state.config).await.map_err(|e| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": e})),
        )
    })?;

    let client_id = state
        .config
        .oidc_client_id
        .as_deref()
        .unwrap_or("jimeng-gateway");
    let client_secret = state.config.oidc_client_secret.as_deref().unwrap_or("");
    let redirect_url = state
        .config
        .oidc_redirect_url
        .as_deref()
        .unwrap_or("http://localhost:5100/auth/callback");

    let http = http_client();

    // Exchange authorization code for tokens
    let token_resp = http
        .post(&endpoints.token_endpoint)
        .basic_auth(client_id, Some(client_secret))
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", &params.code),
            ("redirect_uri", redirect_url),
        ])
        .send()
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": format!("Token exchange failed: {e}")})),
            )
        })?;

    if !token_resp.status().is_success() {
        let text = token_resp.text().await.unwrap_or_default();
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": format!("Token exchange rejected: {text}")})),
        ));
    }

    let tokens: TokenResponse = token_resp.json().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": format!("Invalid token response: {e}")})),
        )
    })?;

    // Fetch user info from userinfo endpoint
    let userinfo_resp = http
        .get(&endpoints.userinfo_endpoint)
        .bearer_auth(&tokens.access_token)
        .send()
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": format!("Userinfo failed: {e}")})),
            )
        })?;

    let userinfo: serde_json::Value = userinfo_resp.json().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": format!("Userinfo parse failed: {e}")})),
        )
    })?;

    let sub = userinfo
        .get("sub")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let name = userinfo
        .get("name")
        .and_then(|v| v.as_str())
        .or_else(|| {
            userinfo
                .get("preferred_username")
                .and_then(|v| v.as_str())
        })
        .unwrap_or(&sub)
        .to_string();
    let email = userinfo
        .get("email")
        .and_then(|v| v.as_str())
        .map(String::from);

    // Create auth hash from sub (stable identifier for session invalidation)
    let auth_hash = sha2::Sha256::digest(sub.as_bytes()).to_vec();

    // Upsert user in admin_users table
    sqlx::query(
        "INSERT INTO admin_users (id, name, email, auth_hash, updated_at) \
         VALUES (?, ?, ?, ?, datetime('now')) \
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, email = excluded.email, updated_at = datetime('now')",
    )
    .bind(&sub)
    .bind(&name)
    .bind(&email)
    .bind(&auth_hash)
    .execute(&state.db.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("DB error: {e}")})),
        )
    })?;

    let user = AdminUser {
        id: sub,
        name,
        email,
        auth_hash,
    };

    // Login via axum-login (stores user in session cookie)
    auth_session.login(&user).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Login failed: {e}")})),
        )
    })?;

    Ok(Redirect::to("/"))
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

/// Token response from OIDC provider
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[allow(dead_code)]
    token_type: String,
    #[allow(dead_code)]
    expires_in: Option<u64>,
    #[allow(dead_code)]
    id_token: Option<String>,
}
