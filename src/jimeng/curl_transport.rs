//! Curl-based HTTP transport for jimeng API calls.
//!
//! ByteDance uses TLS/JA3 fingerprinting to detect non-browser clients.
//! reqwest's TLS fingerprint differs from Chrome, triggering 4013 risk control.
//! System curl links to OpenSSL with a fingerprint that ByteDance accepts.

use anyhow::{bail, Result};
use reqwest::header::HeaderMap;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// POST JSON to a URL via system curl, returning (status_code, response_body).
///
/// Body is piped via stdin to avoid shell escaping issues.
pub async fn post_json_via_curl(
    url: &str,
    headers: &HeaderMap,
    body: &str,
    timeout_secs: u64,
) -> Result<(u16, String)> {
    let mut args = vec![
        "--silent",
        "--show-error",
        "--max-time",
    ];
    let timeout_str = timeout_secs.to_string();
    args.push(&timeout_str);
    args.extend_from_slice(&[
        "--connect-timeout", "15",
        "--request", "POST",
        "--write-out", "\n__CURL_STATUS__%{http_code}",
        "--url",
    ]);
    args.push(url);
    args.extend_from_slice(&["--data-binary", "@-"]);

    // Build header strings
    let mut header_strings = vec!["Content-Type: application/json".to_string()];
    for (name, value) in headers.iter() {
        let value_str = value.to_str().unwrap_or("");
        header_strings.push(format!("{}: {}", name.as_str(), value_str));
    }

    let mut cmd = Command::new("curl");
    for arg in &args {
        cmd.arg(arg);
    }
    for h in &header_strings {
        cmd.arg("-H").arg(h);
    }

    // Log header names (not values to avoid leaking cookies)
    let header_names: Vec<&str> = header_strings.iter()
        .map(|h| h.split(':').next().unwrap_or("?"))
        .collect();
    tracing::debug!(?header_names, body_len = body.len(), "curl transport request");

    let mut child = cmd
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to spawn curl: {e}"))?;

    // Write body to stdin
    if let Some(ref mut stdin) = child.stdin {
        stdin.write_all(body.as_bytes()).await?;
        stdin.shutdown().await?;
    }

    let output = child.wait_with_output().await?;

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
