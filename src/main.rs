mod config;
mod db;
mod docker;
mod pool;
mod queue;
mod routes;

use std::sync::Arc;

use axum::Router;
use tokio::net::TcpListener;
use tower_http::{
    cors::CorsLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::Config;
use crate::db::Database;
use crate::docker::DockerService;
use crate::pool::SessionPool;
use crate::queue::TaskQueue;

/// Shared application state accessible from all route handlers.
pub struct AppState {
    pub config: Config,
    pub db: Database,
    pub pool: SessionPool,
    pub queue: TaskQueue,
    pub docker: DockerService,
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
        upstream = %config.jimeng_upstream,
        "Starting jimeng-gateway"
    );

    // Initialize subsystems
    let db = Database::connect(&config.database_url).await?;
    db.migrate().await?;

    let pool = SessionPool::new(db.clone());
    pool.load_sessions().await?;

    let docker = DockerService::new(&config.jimeng_container_name)?;
    let queue = TaskQueue::new(
        db.clone(),
        pool.clone(),
        config.jimeng_upstream.clone(),
        config.concurrency,
    );

    let state = Arc::new(AppState {
        config: config.clone(),
        db,
        pool,
        queue,
        docker,
    });

    // Start background workers
    state.queue.start_workers(state.clone());

    // Build router
    let api_router = routes::api_router(state.clone());

    let app = Router::new()
        .nest("/api/v1", api_router)
        // Compatibility layer: proxy original jimeng API format
        .merge(routes::compat::compat_router(state.clone()))
        // Serve Vite build output as SPA static assets.
        .fallback_service(
            ServeDir::new("web/dist")
                .append_index_html_on_directories(true)
                .not_found_service(ServeFile::new("web/dist/index.html")),
        )
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let listener = TcpListener::bind(format!("0.0.0.0:{}", config.port)).await?;
    tracing::info!("Listening on 0.0.0.0:{}", config.port);
    axum::serve(listener, app).await?;

    Ok(())
}
