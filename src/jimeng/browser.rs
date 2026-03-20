//! Headless Chromium browser proxy for Seedance a_bogus signing via CDP (chromiumoxide).
//! Only used for Seedance's /mweb/v1/aigc_draft/generate endpoint which requires
//! the bdms SDK to inject a_bogus signatures into fetch requests.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use chromiumoxide::{Browser, BrowserConfig, Page};
use futures::StreamExt;
use tokio::sync::RwLock;

use super::auth;

/// Default session idle timeout (30 minutes).
/// Override via BROWSER_SESSION_IDLE_SECS environment variable.
fn session_idle_timeout() -> Duration {
    let secs: u64 = std::env::var("BROWSER_SESSION_IDLE_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1800); // 30 minutes default
    Duration::from_secs(secs)
}

/// Timeout for waiting for bdms SDK to be ready.
const BDMS_READY_TIMEOUT: Duration = Duration::from_secs(30);

struct BrowserSession {
    page: Page,
    last_used: Instant,
}

/// Browser service for proxying requests through headless Chromium.
/// The bdms SDK hooks window.fetch and injects a_bogus automatically.
pub struct BrowserService {
    browser: Arc<RwLock<Option<Browser>>>,
    sessions: Arc<RwLock<HashMap<String, BrowserSession>>>,
    chromium_path: Option<String>,
}

impl BrowserService {
    pub fn new(chromium_path: Option<String>) -> Self {
        Self {
            browser: Arc::new(RwLock::new(None)),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            chromium_path,
        }
    }

    /// Ensure browser is launched. Lazy-start on first use.
    async fn ensure_browser(&self) -> Result<()> {
        let guard = self.browser.read().await;
        if guard.is_some() {
            return Ok(());
        }
        drop(guard);

        let mut guard = self.browser.write().await;
        if guard.is_some() {
            return Ok(());
        }

        tracing::info!("BrowserService: launching headless Chromium...");

        let mut config = BrowserConfig::builder()
            .no_sandbox()
            .arg("--headless=new")
            .arg("--disable-dev-shm-usage")
            .arg("--disable-gpu")
            .arg("--no-first-run")
            .arg("--disable-background-networking")
            .arg("--disable-extensions")
            .window_size(1920, 1080);

        if let Some(ref path) = self.chromium_path {
            config = config.chrome_executable(path);
        }

        let config = config.build().map_err(|e| anyhow::anyhow!("Browser config error: {e}"))?;
        let (browser, mut handler) = Browser::launch(config).await?;

        // Spawn handler task — keep running even on non-fatal errors
        tokio::spawn(async move {
            loop {
                match handler.next().await {
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        tracing::debug!("BrowserService: handler event error: {e}");
                    }
                    None => {
                        tracing::warn!("BrowserService: handler stream ended");
                        break;
                    }
                }
            }
        });

        tracing::info!("BrowserService: Chromium launched successfully");
        *guard = Some(browser);
        Ok(())
    }

    /// Get or create a browser page for the given session token.
    async fn get_page(&self, session_token: &str) -> Result<Page> {
        // Check existing session
        {
            let mut sessions = self.sessions.write().await;
            if let Some(session) = sessions.get_mut(session_token) {
                session.last_used = Instant::now();
                return Ok(session.page.clone());
            }
        }

        self.ensure_browser().await?;

        let browser_guard = self.browser.read().await;
        let browser = browser_guard.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Browser not available"))?;

        tracing::info!("BrowserService: creating new page for session {}...", &session_token[..session_token.len().min(8)]);

        let page = browser.new_page("about:blank").await?;

        // Set user agent
        page.execute(chromiumoxide::cdp::browser_protocol::emulation::SetUserAgentOverrideParams::new(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/132.0.0.0 Safari/537.36".to_string()
        )).await?;

        // Set cookies for the session
        let cookies = auth::get_cookies_for_browser(session_token);
        for (name, value, domain) in &cookies {
            let cookie = chromiumoxide::cdp::browser_protocol::network::CookieParam::builder()
                .name(name.to_string())
                .value(value.clone())
                .domain(domain.to_string())
                .path("/".to_string())
                .build();
            let cookie = cookie.map_err(|e| anyhow::anyhow!("Cookie build error: {e}"))?;
            page.execute(chromiumoxide::cdp::browser_protocol::network::SetCookiesParams::new(vec![cookie])).await?;
        }

        // Navigate to jimeng to load bdms SDK
        tracing::info!("BrowserService: navigating to jimeng.jianying.com...");
        page.goto("https://jimeng.jianying.com").await?;

        // Wait for bdms SDK to be ready (it hooks window.fetch)
        tracing::info!("BrowserService: waiting for bdms SDK...");
        let deadline = Instant::now() + BDMS_READY_TIMEOUT;
        loop {
            if Instant::now() > deadline {
                tracing::warn!("BrowserService: bdms SDK timeout, continuing anyway...");
                break;
            }

            let ready: bool = page.evaluate(
                "!!(window.bdms?.init || window.byted_acrawler || window.fetch.toString().indexOf('native code') === -1)"
            ).await?.into_value()?;

            if ready {
                tracing::info!("BrowserService: bdms SDK ready");
                break;
            }

            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        let mut sessions = self.sessions.write().await;
        sessions.insert(session_token.to_string(), BrowserSession {
            page: page.clone(),
            last_used: Instant::now(),
        });

        Ok(page)
    }

    /// Proxy a fetch request through the browser so bdms injects a_bogus.
    pub async fn fetch(&self, session_token: &str, url: &str, body: &str) -> Result<String> {
        let page = self.get_page(session_token).await?;

        tracing::info!("BrowserService: proxying POST {}", &url[..url.len().min(100)]);

        let js = format!(
            r#"
            (async () => {{
                try {{
                    const res = await fetch({url}, {{
                        method: "POST",
                        headers: {{ "Content-Type": "application/json" }},
                        body: {body},
                        credentials: "include",
                    }});
                    const text = await res.text();
                    return {{ ok: res.ok, status: res.status, text: text }};
                }} catch (err) {{
                    return {{ ok: false, status: 0, text: "", error: err.message }};
                }}
            }})()
            "#,
            url = serde_json::to_string(url)?,
            body = serde_json::to_string(body)?,
        );

        let result: serde_json::Value = page.evaluate(js).await?.into_value()?;

        if let Some(err) = result.get("error").and_then(|v| v.as_str()) {
            // Clean up session on error
            self.close_session(session_token).await;
            bail!("Browser fetch failed: {err}");
        }

        let status = result.get("status").and_then(|v| v.as_i64()).unwrap_or(0);
        tracing::info!("BrowserService: response status {status}");

        Ok(result.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string())
    }

    /// Close a specific session.
    async fn close_session(&self, token: &str) {
        let mut sessions = self.sessions.write().await;
        if sessions.remove(token).is_some() {
            tracing::info!("BrowserService: closed session for {}...", &token[..token.len().min(8)]);
        }
    }

    /// Clean up idle sessions (call periodically).
    /// Closes Pages that haven't been used within the configured idle timeout.
    pub async fn cleanup_idle_sessions(&self) {
        let idle_timeout = session_idle_timeout();
        let mut sessions = self.sessions.write().await;
        let now = Instant::now();
        let mut closed = 0u32;
        sessions.retain(|token, session| {
            if now.duration_since(session.last_used) > idle_timeout {
                tracing::info!(
                    "BrowserService: closing idle session {}... (idle for {:?})",
                    &token[..token.len().min(8)],
                    now.duration_since(session.last_used)
                );
                // Page::drop will close the CDP target automatically.
                closed += 1;
                false
            } else {
                true
            }
        });
        if closed > 0 {
            tracing::info!("BrowserService: cleaned up {closed} idle session(s), {remaining} remaining",
                remaining = sessions.len());
        }
    }

    /// Return the number of active sessions and idle timeout for diagnostics.
    pub async fn session_stats(&self) -> (usize, Duration) {
        let sessions = self.sessions.read().await;
        (sessions.len(), session_idle_timeout())
    }

    /// Shut down browser and all sessions.
    pub async fn close(&self) {
        self.sessions.write().await.clear();
        *self.browser.write().await = None;
        tracing::info!("BrowserService: closed");
    }
}
