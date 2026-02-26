use std::sync::Arc;

use axum::{routing::{get, post}, Router};

use crate::AppState;
use crate::auth::oidc;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/auth/login", get(oidc::login))
        .route("/auth/callback", get(oidc::callback))
        .route("/auth/me", get(oidc::me))
        .route("/auth/logout", post(oidc::logout))
        .with_state(state)
}
