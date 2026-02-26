use std::sync::Arc;

use axum::{
    Extension, Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse as _, Response},
    routing::get,
};

use crate::AppState;
use crate::auth::api_key;
use crate::auth::middleware::Caller;
use crate::auth::usage as usage_tracker;

async fn me(
    State(state): State<Arc<AppState>>,
    Extension(caller): Extension<Caller>,
) -> Result<Json<serde_json::Value>, Response> {
    match caller {
        Caller::ApiKey {
            ref key_id,
            ref name,
            ref scopes,
            rate_limit,
            daily_quota,
        } => {
            let record = api_key::get_by_id(&state.db.pool, key_id)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())?
                .ok_or_else(|| StatusCode::NOT_FOUND.into_response())?;

            let (req_count, task_count) =
                usage_tracker::today_usage(&state.db.pool, key_id)
                    .await
                    .unwrap_or((0, 0));

            let quota_remaining = if daily_quota > 0 {
                (daily_quota - task_count).max(0)
            } else {
                -1 // unlimited
            };

            Ok(Json(serde_json::json!({
                "key": {
                    "id": key_id,
                    "name": name,
                    "key_prefix": record.key_prefix,
                    "scopes": scopes,
                    "rate_limit": rate_limit,
                    "daily_quota": daily_quota,
                },
                "today": {
                    "request_count": req_count,
                    "task_count": task_count,
                    "quota_remaining": quota_remaining,
                }
            })))
        }
        Caller::Admin { .. } => Ok(Json(serde_json::json!({
            "role": "admin",
            "scopes": ["admin"],
        }))),
        Caller::Anonymous => Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "Not authenticated" })),
        )
            .into_response()),
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/me", get(me))
        .with_state(state)
}
