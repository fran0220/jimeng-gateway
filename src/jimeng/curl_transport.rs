//! Curl-based HTTP transport for jimeng API calls.
//!
//! ByteDance uses TLS/JA3 fingerprinting to detect non-browser clients.
//! reqwest's TLS fingerprint differs from Chrome, triggering 4013 risk control.
//! System curl links to OpenSSL with a fingerprint that ByteDance accepts.

use anyhow::{bail, Result};
use reqwest::header::HeaderMap;
use tokio::process::Command;

/// POST JSON to a URL via system curl, returning (status_code, response_body).
///
/// Body is written to a temp file to avoid stdin piping issues.
pub async fn post_json_via_curl(
    url: &str,
    headers: &HeaderMap,
    body: &str,
    timeout_secs: u64,
) -> Result<(u16, String)> {
    // Write body to a temp file (avoids stdin pipe buffering issues)
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let body_file = format!("/tmp/jimeng_curl_{}_{}.json", std::process::id(), ts);
    tokio::fs::write(&body_file, body.as_bytes()).await?;

    let timeout_str = timeout_secs.to_string();

    // Build header strings
    let mut header_strings = vec!["Content-Type: application/json".to_string()];
    for (name, value) in headers.iter() {
        let value_str = value.to_str().unwrap_or("");
        header_strings.push(format!("{}: {}", name.as_str(), value_str));
    }

    let mut cmd = Command::new("curl");
    cmd.arg("--silent")
        .arg("--show-error")
        .arg("--max-time").arg(&timeout_str)
        .arg("--connect-timeout").arg("15")
        .arg("--request").arg("POST")
        .arg("--write-out").arg("\n__CURL_STATUS__%{http_code}")
        .arg("--url").arg(url)
        .arg("-d").arg(format!("@{body_file}"));

    for h in &header_strings {
        cmd.arg("-H").arg(h);
    }

    // Log header names (not values to avoid leaking cookies)
    let header_names: Vec<&str> = header_strings.iter()
        .map(|h| h.split(':').next().unwrap_or("?"))
        .collect();
    tracing::debug!(?header_names, body_len = body.len(), %body_file, "curl transport request");

    let output = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to spawn curl: {e}"))?
        .wait_with_output()
        .await?;

    let _ = tokio::fs::remove_file(&body_file).await;

    if !output.status.success() && output.stdout.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("curl failed (exit {}): {}", output.status, stderr.trim());
    }

    let raw = String::from_utf8_lossy(&output.stdout);

    // Parse status code from --write-out marker
    let (response_body, status_code) = if let Some(idx) = raw.rfind("\n__CURL_STATUS__") {
        let body_part = &raw[..idx];
        let code_str = &raw[idx + "\n__CURL_STATUS__".len()..];
        let code: u16 = code_str.trim().parse().unwrap_or(0);
        (body_part.to_string(), code)
    } else {
        (raw.to_string(), 0)
    };

    Ok((status_code, response_body))
}
