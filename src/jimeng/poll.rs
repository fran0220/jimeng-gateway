//! Poll video generation status via jimeng.jianying.com API.

use anyhow::{bail, Result};
use reqwest::Client;

use super::auth;

const JIMENG_BASE: &str = "https://jimeng.jianying.com";

/// Upstream status codes.
pub const STATUS_PENDING: i64 = 20;
pub const STATUS_FAILED: i64 = 30;

/// Result of a single poll request.
#[derive(Debug, Clone)]
pub struct PollResult {
    pub status: i64,
    pub fail_code: Option<String>,
    pub fail_msg: Option<String>,
    pub video_url: Option<String>,
    pub queue_position: Option<i32>,
    pub queue_total: Option<i32>,
    pub queue_eta: Option<String>,
    pub item_id: Option<String>,
}

/// Poll the status of a video generation task by history_record_id.
pub async fn poll_status(
    client: &Client,
    session_token: &str,
    history_record_id: &str,
) -> Result<PollResult> {
    let uri = "/mweb/v1/get_history_by_ids";
    let headers = auth::build_headers(session_token, uri);
    let params = auth::standard_query_params();

    let body = serde_json::json!({
        "history_ids": [history_record_id],
    });

    let resp = client.post(format!("{JIMENG_BASE}{uri}"))
        .headers(headers)
        .query(&params)
        .json(&body)
        .send().await?;

    let status_code = resp.status();
    let text = resp.text().await?;

    if !status_code.is_success() {
        bail!("Poll HTTP {status_code}: {}", &text[..text.len().min(500)]);
    }

    let payload: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| anyhow::anyhow!("Poll parse error: {e}. Body: {}", &text[..text.len().min(500)]))?;

    // Extract data (handle {ret: "0", data: {...}} wrapper)
    let data = if let Some(d) = payload.get("data") { d } else { &payload };

    // Find history data: could be in history_list[0], result[historyId], or data[historyId]
    let history_data = data.pointer("/history_list/0")
        .or_else(|| data.get(history_record_id))
        .or_else(|| payload.get(history_record_id))
        .or_else(|| data.pointer("/history_records/0"));

    let history_data = match history_data {
        Some(d) => d,
        None => bail!("History record not found for {history_record_id}"),
    };

    let status = history_data.get("status")
        .and_then(|v| v.as_i64())
        .unwrap_or(STATUS_PENDING);

    let fail_code = history_data.get("fail_code")
        .or_else(|| history_data.get("error_code"))
        .and_then(|v| {
            v.as_i64().map(|n| n.to_string())
                .or_else(|| v.as_str().map(|s| s.to_string()))
        });

    let fail_msg = history_data.get("fail_msg")
        .or_else(|| history_data.get("error_msg"))
        .or_else(|| history_data.get("message"))
        .and_then(|v| v.as_str().map(|s| s.to_string()));

    // Extract video URL from item_list
    let item_list = history_data.get("item_list")
        .and_then(|v| v.as_array());

    let (video_url, item_id) = if let Some(items) = item_list {
        let first = items.first();
        let url = first.and_then(|item| {
            item.pointer("/video/transcoded_video/origin/video_url")
                .or_else(|| item.pointer("/video/play_url"))
                .or_else(|| item.pointer("/video/download_url"))
                .or_else(|| item.pointer("/video/url"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
        });
        let id = first.and_then(|item| {
            item.get("item_id")
                .or_else(|| item.get("id"))
                .or_else(|| item.get("local_item_id"))
                .or_else(|| item.pointer("/common_attr/id"))
                .and_then(|v| {
                    v.as_str().map(|s| s.to_string())
                        .or_else(|| v.as_i64().map(|n| n.to_string()))
                })
        });
        (url, id)
    } else {
        (None, None)
    };

    // Extract queue info from queue_info object
    let queue_info = history_data.get("queue_info");

    let (queue_position, queue_total, queue_eta) = if let Some(qi) = queue_info {
        let pos = qi.get("queue_idx")
            .and_then(|v| v.as_i64().map(|n| n as i32));
        let total = qi.get("queue_length")
            .and_then(|v| v.as_i64().map(|n| n as i32));
        // ETA from forecast_queue_cost (seconds) at top level
        let forecast_secs = history_data.get("forecast_queue_cost")
            .and_then(|v| v.as_i64());
        let eta = forecast_secs.map(|s| {
            if s >= 3600 {
                format!("{}h{}m", s / 3600, (s % 3600) / 60)
            } else if s >= 60 {
                format!("{}m{}s", s / 60, s % 60)
            } else {
                format!("{}s", s)
            }
        });
        (pos, total, eta)
    } else {
        (None, None, None)
    };

    Ok(PollResult {
        status,
        fail_code,
        fail_msg,
        video_url,
        queue_position,
        queue_total,
        queue_eta,
        item_id,
    })
}

/// Try to get high-quality video URL via get_local_item_list API.
pub async fn fetch_hq_video_url(
    client: &Client,
    session_token: &str,
    item_id: &str,
) -> Result<Option<String>> {
    let uri = "/mweb/v1/get_local_item_list";
    let headers = auth::build_headers(session_token, uri);
    let params = auth::standard_query_params();

    let body = serde_json::json!({
        "item_id_list": [item_id],
        "pack_item_opt": { "scene": 1, "need_data_integrity": true },
        "is_for_video_download": true,
    });

    let resp = client.post(format!("{JIMENG_BASE}{uri}"))
        .headers(headers)
        .query(&params)
        .json(&body)
        .send().await?;

    let text = resp.text().await?;
    let data = if let Some(d) = serde_json::from_str::<serde_json::Value>(&text).ok().and_then(|v| v.get("data").cloned()) {
        d
    } else {
        serde_json::from_str(&text)?
    };

    // Try structured extraction first
    let item_list = data.get("item_list").or_else(|| data.get("local_item_list"));
    if let Some(items) = item_list.and_then(|v| v.as_array()) {
        if let Some(item) = items.first() {
            let url = item.pointer("/video/transcoded_video/origin/video_url")
                .or_else(|| item.pointer("/video/download_url"))
                .or_else(|| item.pointer("/video/play_url"))
                .or_else(|| item.pointer("/video/url"))
                .and_then(|v| v.as_str());
            if let Some(url) = url {
                return Ok(Some(url.to_string()));
            }
        }
    }

    // Fallback: regex match high-quality URL patterns
    let re_patterns = [
        r#"https://v\d+-dreamnia\.jimeng\.com/[^"\s\\]+"#,
        r#"https://v\d+-[^"\\\s]*\.jimeng\.com/[^"\s\\]+"#,
    ];
    for pattern in &re_patterns {
        if let Ok(re) = regex::Regex::new(pattern) {
            if let Some(m) = re.find(&text) {
                return Ok(Some(m.as_str().to_string()));
            }
        }
    }

    Ok(None)
}
