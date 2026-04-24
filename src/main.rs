mod auth;
mod config;
mod db;
mod jimeng;
mod pool;
mod queue;
mod routes;
mod webhook;

use std::sync::Arc;

use axum::{Router, middleware};
use axum_login::AuthManagerLayerBuilder;
use tokio::net::TcpListener;
use tower_http::{
    cors::CorsLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use tower_sessions::{ExpiredDeletion, SessionManagerLayer};
use tower_sessions_sqlx_store::SqliteStore;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::auth::backend::PasswordBackend;
use crate::auth::middleware::api_key_auth;
use crate::auth::rate_limiter::RateLimiter;
use crate::config::Config;
use crate::db::Database;
use crate::jimeng::browser::BrowserService;
use crate::pool::SessionPool;
use crate::queue::TaskQueue;

/// Shared application state accessible from all route handlers.
pub struct AppState {
    pub config: Config,
    pub db: Database,
    pub pool: SessionPool,
    pub queue: TaskQueue,
    pub browser: BrowserService,
    pub rate_limiter: RateLimiter,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Logging
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "jimeng_gateway=debug,tower_http=info".into()))
        .with(fmt::layer().with_target(true))
        .init();

    dotenvy::dotenv().ok();
    let config = Config::from_env()?;
    tracing::info!(
        port = config.port,
        auth_enabled = config.auth_enabled,
        "Starting jimeng-gateway"
    );

    // Initialize subsystems
    let db = Database::connect(&config.database_url).await?;
    db.migrate().await?;
    db.recover_on_startup().await?;

    let pool = SessionPool::new(db.clone());
    pool.load_sessions().await?;

    let browser = BrowserService::new(config.chromium_path.clone());
    let queue = TaskQueue::new(
        db.clone(),
        pool.clone(),
        config.concurrency,
    );

    let rate_limiter = RateLimiter::new();

    let state = Arc::new(AppState {
        config: config.clone(),
        db: db.clone(),
        pool,
        queue,
        browser,
        rate_limiter,
    });

    // Start background workers
    state.queue.start_workers(state.clone());

    // Session store (SQLite-backed via tower-sessions)
    let session_store = SqliteStore::new(db.pool.clone());
    session_store.migrate().await?;

    let session_layer = SessionManagerLayer::new(session_store.clone())
        .with_secure(false); // HTTP in dev; set true in production with HTTPS

    // Auth backend + layer
    let auth_backend = PasswordBackend::new(db.pool.clone());
    let auth_layer = AuthManagerLayerBuilder::new(auth_backend, session_layer).build();

    // Spawn session cleanup task
    let deletion_task = tokio::task::spawn(
        session_store
            .continuously_delete_expired(tokio::time::Duration::from_secs(60)),
    );

    // Spawn webhook dispatcher
    let webhook_pool = db.pool.clone();
    let webhook_task = tokio::task::spawn(async move {
        webhook::dispatcher_loop(webhook_pool).await;
    });

    // Spawn periodic cookie refresh (every 2 hours)
    let cookie_state = state.clone();
    let cookie_task = tokio::task::spawn(async move {
        cookie_refresh_loop(cookie_state).await;
    });

    // Build router
    // Auth routes (login/callback/me/logout) — always accessible
    let auth_router = routes::auth_routes::router(state.clone());

    // Admin API routes — protected when AUTH_ENABLED=true
    let admin_router = if config.auth_enabled {
        routes::admin_api_router(state.clone())
            .route_layer(axum_login::login_required!(PasswordBackend))
    } else {
        routes::admin_api_router(state.clone())
    };

    // Public API routes — always accessible
    let public_router = routes::public_api_router(state.clone());

    // Compat routes — protected by API key middleware
    let compat_router = routes::compat::compat_router(state.clone())
        .route_layer(middleware::from_fn_with_state(state.clone(), api_key_auth));

    let ping_router = routes::compat::ping_router();

    let app = Router::new()
        .merge(ping_router)
        .merge(auth_router)
        .nest("/api/v1", admin_router.merge(public_router))
        .merge(compat_router)
        // Serve Vite build output as SPA static assets.
        .fallback_service(
            ServeDir::new("web/dist")
                .append_index_html_on_directories(true)
                .not_found_service(ServeFile::new("web/dist/index.html")),
        )
        .layer(auth_layer)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let listener = TcpListener::bind(format!("0.0.0.0:{}", config.port)).await?;
    tracing::info!("Listening on 0.0.0.0:{}", config.port);
    axum::serve(listener, app).await?;

    deletion_task.abort();
    webhook_task.abort();
    cookie_task.abort();

    Ok(())
}

/// Periodically refresh cookie jars for all enabled sessions.
async fn cookie_refresh_loop(state: Arc<AppState>) {
    // Wait 30s on startup before first harvest
    tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

    let interval = tokio::time::Duration::from_secs(2 * 3600); // 2 hours
    loop {
        let sessions = state.pool.list_sessions().await;
        let active: Vec<_> = sessions.iter().filter(|s| s.enabled).collect();

        if active.is_empty() {
            tokio::time::sleep(interval).await;
            continue;
        }

        tracing::info!("CookieRefresh: refreshing cookies for {} sessions", active.len());

        for session in &active {
            match state.browser.harvest_cookies(&session.session_id).await {
                Ok(cookie_jar) => {
                    let count = cookie_jar.split("; ").count();
                    if let Err(e) = state.pool.update_cookie_jar(&session.id, &cookie_jar).await {
                        tracing::warn!(session_id = session.id, error = %e, "Failed to save harvested cookies");
                    } else {
                        tracing::info!(session_id = session.id, cookies = count, "Cookies refreshed");
                    }
                }
                Err(e) => {
                    tracing::warn!(session_id = session.id, error = %e, "Cookie harvest failed");
                }
            }
            // Small delay between sessions to avoid overwhelming the browser
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        }

        tokio::time::sleep(interval).await;
    }
}
