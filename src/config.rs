use std::env;

use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    /// Upstream jimeng-free-api-all URL (e.g. http://jimeng-api:8000)
    pub jimeng_upstream: String,
    /// Docker container name for log streaming
    pub jimeng_container_name: String,
    /// SQLite database URL
    pub database_url: String,
    /// Max concurrent video generation tasks
    pub concurrency: usize,
    /// Poll interval in seconds for checking video generation status
    pub poll_interval_secs: u64,
    /// Max poll duration (no timeout by default — queue can take hours)
    pub max_poll_duration_secs: u64,
    /// Enable authentication (default: false for backward compat)
    pub auth_enabled: bool,
    /// Static admin token fallback (for scripts/CI)
    pub admin_token: Option<String>,
    /// OIDC issuer URL (e.g., http://localhost:8080/auth/v1)
    /// Supports any OIDC-compliant provider: Rauthy, Kanidm, Google, etc.
    pub oidc_issuer_url: Option<String>,
    /// OAuth2 client ID
    pub oidc_client_id: Option<String>,
    /// OAuth2 client secret
    pub oidc_client_secret: Option<String>,
    /// OAuth2 redirect URL (callback endpoint on this gateway)
    pub oidc_redirect_url: Option<String>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            port: env::var("PORT")
                .unwrap_or_else(|_| "5100".into())
                .parse()
                .context("PORT must be a valid u16")?,
            jimeng_upstream: env::var("JIMENG_UPSTREAM")
                .unwrap_or_else(|_| "http://127.0.0.1:8000".into()),
            jimeng_container_name: env::var("JIMENG_CONTAINER")
                .unwrap_or_else(|_| "jimeng-free-api-all".into()),
            database_url: env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite://data/gateway.db?mode=rwc".into()),
            concurrency: env::var("CONCURRENCY")
                .unwrap_or_else(|_| "2".into())
                .parse()
                .unwrap_or(2),
            poll_interval_secs: env::var("POLL_INTERVAL_SECS")
                .unwrap_or_else(|_| "10".into())
                .parse()
                .unwrap_or(10),
            max_poll_duration_secs: env::var("MAX_POLL_DURATION_SECS")
                .unwrap_or_else(|_| "14400".into()) // 4 hours default
                .parse()
                .unwrap_or(14400),
            auth_enabled: env::var("AUTH_ENABLED")
                .unwrap_or_else(|_| "false".into())
                .parse()
                .unwrap_or(false),
            admin_token: env::var("ADMIN_TOKEN").ok().filter(|s| !s.is_empty()),
            oidc_issuer_url: env::var("OIDC_ISSUER_URL").ok().filter(|s| !s.is_empty()),
            oidc_client_id: env::var("OIDC_CLIENT_ID").ok().filter(|s| !s.is_empty()),
            oidc_client_secret: env::var("OIDC_CLIENT_SECRET").ok().filter(|s| !s.is_empty()),
            oidc_redirect_url: env::var("OIDC_REDIRECT_URL").ok().filter(|s| !s.is_empty()),
        })
    }
}
