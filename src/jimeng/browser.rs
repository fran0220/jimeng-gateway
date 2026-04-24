//! Headless Chromium browser proxy for Seedance a_bogus signing via CDP (chromiumoxide).
//! Uses a single shared browser page to minimize memory usage.

use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use chromiumoxide::{Browser, BrowserConfig, Page};
use futures::StreamExt;
use tokio::sync::Mutex;

use super::auth;

/// Max lifetime for a browser page before recreation (1 hour).
const MAX_PAGE_LIFETIME: Duration = Duration::from_secs(3600);

/// Timeout for waiting for bdms SDK to be ready.
const BDMS_READY_TIMEOUT: Duration = Duration::from_secs(30);

struct SharedPage {
    page: Page,
    created_at: Instant,
}

/// Browser service using a single shared page for all sessions.
/// The page is protected by a Mutex to prevent concurrent cookie contamination.
pub struct BrowserService {
    browser: tokio::sync::RwLock<Option<Browser>>,
    shared_page: Mutex<Option<SharedPage>>,
    chromium_path: Option<String>,
}

impl BrowserService {
    pub fn new(chromium_path: Option<String>) -> Self {
        Self {
            browser: tokio::sync::RwLock::new(None),
            shared_page: Mutex::new(None),
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
            .arg("--disable-images")
            .arg("--blink-settings=imagesEnabled=false")
            .window_size(1920, 1080);

        if let Some(ref path) = self.chromium_path {
            config = config.chrome_executable(path);
        }

        let config = config.build().map_err(|e| anyhow::anyhow!("Browser config error: {e}"))?;
        let (browser, mut handler) = Browser::launch(config).await?;

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

    /// Create a new browser page and navigate to jimeng to load bdms SDK.
    async fn create_page(&self) -> Result<Page> {
        self.ensure_browser().await?;

        let browser_guard = self.browser.read().await;
        let browser = browser_guard.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Browser not available"))?;

        tracing::info!("BrowserService: creating shared page...");

        let page = browser.new_page("about:blank").await?;

        // Set user agent
        page.execute(chromiumoxide::cdp::browser_protocol::emulation::SetUserAgentOverrideParams::new(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/132.0.0.0 Safari/537.36".to_string()
        )).await?;

        // Navigate to jimeng to load bdms SDK
        tracing::info!("BrowserService: navigating to jimeng.jianying.com...");
        page.goto("https://jimeng.jianying.com").await?;

        // Wait for bdms SDK to be ready
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

        Ok(page)
    }

    /// Set cookies for a specific session on the shared page.
    async fn set_session_cookies(&self, page: &Page, session_token: &str) -> Result<()> {
        // Clear all existing cookies
        page.execute(
            chromiumoxide::cdp::browser_protocol::network::ClearBrowserCookiesParams::default()
        ).await?;

        // Set cookies for the current session
        let cookies = auth::get_cookies_for_browser(session_token);
        for (name, value, domain) in &cookies {
            let cookie = chromiumoxide::cdp::browser_protocol::network::CookieParam::builder()
                .name(name.to_string())
                .value(value.clone())
                .domain(domain.to_string())
                .path("/".to_string())
                .build();
            let cookie = cookie.map_err(|e| anyhow::anyhow!("Cookie build error: {e}"))?;
            page.execute(
                chromiumoxide::cdp::browser_protocol::network::SetCookiesParams::new(vec![cookie])
            ).await?;
        }

        Ok(())
    }

    /// Proxy a fetch request through the browser so bdms injects a_bogus.
    /// Uses a single shared page with mutex serialization.
    pub async fn fetch(&self, session_token: &str, url: &str, body: &str) -> Result<String> {
        let mut page_guard = self.shared_page.lock().await;

        // Check if page needs recreation (expired or missing)
        let needs_new_page = match &*page_guard {
            None => true,
            Some(sp) => sp.created_at.elapsed() > MAX_PAGE_LIFETIME,
        };

        if needs_new_page {
            if page_guard.is_some() {
                tracing::info!("BrowserService: recycling expired page");
            }
            let page = self.create_page().await?;
            *page_guard = Some(SharedPage {
                page,
                created_at: Instant::now(),
            });
        }

        let sp = page_guard.as_ref().unwrap();

        // Set cookies for this session
        self.set_session_cookies(&sp.page, session_token).await?;

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

        let result: serde_json::Value = match sp.page.evaluate(js).await {
            Ok(v) => v.into_value()?,
            Err(e) => {
                // Page crashed or disconnected — drop it so next call creates a new one
                tracing::error!("BrowserService: page evaluate failed: {e}");
                *page_guard = None;
                bail!("Browser page crashed: {e}");
            }
        };

        if let Some(err) = result.get("error").and_then(|v| v.as_str()) {
            // Drop page on fetch errors too, to get a clean slate
            *page_guard = None;
            bail!("Browser fetch failed: {err}");
        }

        let status = result.get("status").and_then(|v| v.as_i64()).unwrap_or(0);
        tracing::info!("BrowserService: response status {status}");

        Ok(result.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string())
    }

    /// Harvest full cookie jar for a session.
    ///
    /// 1. Launch browser, navigate to jimeng.jianying.com
    /// 2. Inject session cookies (sessionid, sid_tt, etc.)
    /// 3. Reload so bdms SDK runs with the session context
    /// 4. Wait for fingerprint cookies to be set (ttwid, odin_tt, fpk1, etc.)
    /// 5. Extract all cookies via CDP and return as a cookie header string
    pub async fn harvest_cookies(&self, session_token: &str) -> Result<String> {
        self.ensure_browser().await?;

        let browser_guard = self.browser.read().await;
        let browser = browser_guard.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Browser not available"))?;

        tracing::info!("CookieHarvest: creating page for cookie harvesting...");
        let page = browser.new_page("about:blank").await?;

        // Set session cookies before navigating
        let session_cookies = auth::get_cookies_for_browser(session_token);
        for (name, value, domain) in &session_cookies {
            let cookie = chromiumoxide::cdp::browser_protocol::network::CookieParam::builder()
                .name(name.to_string())
                .value(value.clone())
                .domain(domain.to_string())
                .path("/".to_string())
                .build()
                .map_err(|e| anyhow::anyhow!("Cookie build error: {e}"))?;
            page.execute(
                chromiumoxide::cdp::browser_protocol::network::SetCookiesParams::new(vec![cookie])
            ).await?;
        }

        // Navigate to jimeng — this triggers bdms SDK which sets fingerprint cookies
        tracing::info!("CookieHarvest: navigating to jimeng.jianying.com...");
        page.goto("https://jimeng.jianying.com").await?;

        // Wait for key fingerprint cookies to appear
        tracing::info!("CookieHarvest: waiting for fingerprint cookies...");
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if Instant::now() > deadline {
                tracing::warn!("CookieHarvest: timeout waiting for fingerprint cookies, using what we have");
                break;
            }

            let cookies = page.get_cookies().await?;
            let has_ttwid = cookies.iter().any(|c| c.name == "ttwid");
            let has_fpk1 = cookies.iter().any(|c| c.name == "fpk1");
            if has_ttwid && has_fpk1 {
                tracing::info!("CookieHarvest: fingerprint cookies ready ({} total)", cookies.len());
                break;
            }

            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        // Extract all cookies as a header string
        let cookies = page.get_cookies().await?;
        let cookie_str = cookies.iter()
            .map(|c| format!("{}={}", c.name, c.value))
            .collect::<Vec<_>>()
            .join("; ");

        tracing::info!("CookieHarvest: harvested {} cookies ({} bytes)", cookies.len(), cookie_str.len());

        // Close the temporary page
        page.execute(chromiumoxide::cdp::browser_protocol::page::CloseParams::default()).await.ok();

        if cookie_str.is_empty() {
            bail!("Cookie harvest produced empty result");
        }

        Ok(cookie_str)
    }

    /// Return diagnostics: (active_pages, max_lifetime).
    pub async fn session_stats(&self) -> (usize, Duration) {
        let guard = self.shared_page.lock().await;
        let count = if guard.is_some() { 1 } else { 0 };
        (count, MAX_PAGE_LIFETIME)
    }

    /// Shut down browser and page.
    pub async fn close(&self) {
        *self.shared_page.lock().await = None;
        *self.browser.write().await = None;
        tracing::info!("BrowserService: closed");
    }
}
