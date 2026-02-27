use std::sync::Arc;

use axum::{routing::{get, post}, Router};

use crate::AppState;
use crate::auth::password;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/auth/login", post(password::login))
        .route("/auth/me", get(password::me))
        .route("/auth/logout", post(password::logout))
        .with_state(state)
}
