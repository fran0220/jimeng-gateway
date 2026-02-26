use std::sync::Arc;

use axum::{
    Json,
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

use super::api_key;
use super::rate_limiter::RateLimitResult;
use crate::AppState;

/// Authenticated caller identity, injected as request extension.
#[derive(Debug, Clone)]
pub enum Caller {
    /// API Key caller
    ApiKey {
        key_id: String,
        name: String,
        scopes: Vec<String>,
        rate_limit: i32,
        daily_quota: i32,
    },
    /// Admin (env-var token or API key with admin scope)
    Admin {
        source: AdminSource,
    },
    /// Unauthenticated (AUTH_ENABLED=false fallback)
    Anonymous,
}

#[derive(Debug, Clone)]
pub enum AdminSource {
    EnvToken,
    ApiKey(String),
}

impl Caller {
    pub fn has_scope(&self, scope: &str) -> bool {
        match self {
            Caller::Admin { .. } => true,
            Caller::ApiKey { scopes, .. } => scopes.iter().any(|s| s == scope),
            Caller::Anonymous => true, // no auth = open access
        }
    }

    pub fn key_id(&self) -> Option<&str> {
        match self {
            Caller::ApiKey { key_id, .. } => Some(key_id),
            Caller::Admin { source: AdminSource::ApiKey(id) } => Some(id),
            _ => None,
        }
    }
}

/// Extract Bearer token from Authorization header.
fn extract_bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
}

/// API Key authentication middleware.
///
/// When AUTH_ENABLED=true:
///   - Extracts Bearer token → checks admin_token → checks api_keys table
///   - Applies rate limiting + usage recording
///   - Injects Caller as request extension
///
/// When AUTH_ENABLED=false:
///   - Injects Caller::Anonymous
pub async fn api_key_auth(
    State(state): State<Arc<AppState>>,
    mut request: Request,
    next: Next,
) -> Response {
    if !state.config.auth_enabled {
        request.extensions_mut().insert(Caller::Anonymous);
        return next.run(request).await;
    }

    let token = match extract_bearer(request.headers()) {
        Some(t) => t.to_string(),
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "Missing Authorization header" })),
            )
                .into_response();
        }
    };

    // Check env-var admin token first
    if let Some(ref admin_token) = state.config.admin_token {
        if token == *admin_token {
            request
                .extensions_mut()
                .insert(Caller::Admin { source: AdminSource::EnvToken });
            return next.run(request).await;
        }
    }

    // Look up API key by hash
    let key_hash = api_key::hash_key(&token);
    let record = match api_key::lookup_by_hash(&state.db.pool, &key_hash).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "Invalid API key" })),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("API key lookup error: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "Internal error" })),
            )
                .into_response();
        }
    };

    // Check enabled
    if !record.enabled {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "API key is disabled" })),
        )
            .into_response();
    }

    // Check expiry
    if let Some(ref expires_at) = record.expires_at {
        if let Ok(exp) = chrono::DateTime::parse_from_rfc3339(expires_at) {
            if chrono::Utc::now() > exp {
                return (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({ "error": "API key has expired" })),
                )
                    .into_response();
            }
        }
    }

    // Admin scope key
    if record.scopes.contains(&"admin".to_string()) {
        api_key::touch(&state.db.pool, &record.id).await;
        super::usage::record_request(&state.db.pool, &record.id).await;
        request.extensions_mut().insert(Caller::Admin {
            source: AdminSource::ApiKey(record.id),
        });
        return next.run(request).await;
    }

    // Rate limit check
    let rl_result = state.rate_limiter.check(&record.id, record.rate_limit as u32);
    if !rl_result.allowed {
        return rate_limit_response(&rl_result);
    }

    // Update last_used + record usage (fire-and-forget)
    api_key::touch(&state.db.pool, &record.id).await;
    super::usage::record_request(&state.db.pool, &record.id).await;

    let caller = Caller::ApiKey {
        key_id: record.id,
        name: record.name,
        scopes: record.scopes,
        rate_limit: record.rate_limit,
        daily_quota: record.daily_quota,
    };
    request.extensions_mut().insert(caller);

    // Add rate limit headers to response
    let mut response = next.run(request).await;
    inject_rate_limit_headers(response.headers_mut(), &rl_result);
    response
}

fn rate_limit_response(rl: &RateLimitResult) -> Response {
    let mut resp = (
        StatusCode::TOO_MANY_REQUESTS,
        Json(serde_json::json!({
            "error": "Rate limit exceeded",
            "retry_after": rl.reset_secs,
        })),
    )
        .into_response();
    inject_rate_limit_headers(resp.headers_mut(), rl);
    resp
}

fn inject_rate_limit_headers(headers: &mut HeaderMap, rl: &RateLimitResult) {
    if rl.limit > 0 {
        headers.insert("X-RateLimit-Limit", rl.limit.into());
        headers.insert("X-RateLimit-Remaining", rl.remaining.into());
        headers.insert("X-RateLimit-Reset", rl.reset_secs.into());
    }
}

/// Scope guard: returns 403 if the caller lacks the required scope.
pub fn require_scope(caller: &Caller, scope: &str) -> Result<(), Response> {
    if caller.has_scope(scope) {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": format!("Missing required scope: {scope}") })),
        )
            .into_response())
    }
}
