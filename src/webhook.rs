//! Webhook delivery for task completion notifications.
//! Uses a SQLite outbox pattern with exponential backoff retries.

use std::time::Duration;

use reqwest::Client;
use sqlx::SqlitePool;

/// Maximum delivery attempts before giving up.
const MAX_ATTEMPTS: i32 = 8;
/// Initial backoff interval.
const INITIAL_BACKOFF_SECS: u64 = 30;
/// Maximum backoff interval.
const MAX_BACKOFF_SECS: u64 = 3600;
/// HTTP timeout for webhook requests.
const WEBHOOK_TIMEOUT: Duration = Duration::from_secs(10);

/// Enqueue a webhook delivery for a completed task.
/// Does nothing if the task has no webhook_url.
pub async fn enqueue_delivery(pool: &SqlitePool, task_id: &str) {
    let row = sqlx::query_as::<_, WebhookTaskRow>(
        "SELECT t.id, t.status, t.model, t.prompt, t.video_url, t.error_message, t.error_kind, \
         t.webhook_url, t.webhook_secret, t.created_at, t.started_at, t.finished_at \
         FROM tasks t WHERE t.id = ?"
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await;

    let row = match row {
        Ok(Some(r)) if r.webhook_url.is_some() => r,
        _ => return,
    };

    let webhook_url = row.webhook_url.as_ref().unwrap();

    let event = match row.status.as_str() {
        "succeeded" => "task.succeeded",
        "failed" => "task.failed",
        "cancelled" => "task.cancelled",
        _ => return,
    };

    let delivery_id = uuid::Uuid::new_v4().to_string();

    // Build result/error based on status
    let (result_val, error_val) = if row.status == "succeeded" {
        let urls: Vec<&str> = row.video_url.as_deref()
            .unwrap_or("")
            .split(',')
            .filter(|s| !s.is_empty())
            .collect();

        let result = if urls.len() > 1 || urls.first().map_or(false, |u| u.contains("image")) {
            serde_json::json!({ "image_urls": urls, "video_url": null })
        } else {
            serde_json::json!({ "video_url": urls.first(), "image_urls": [] })
        };
        (serde_json::Value::Object(result.as_object().unwrap().clone()), serde_json::Value::Null)
    } else {
        (serde_json::Value::Null, serde_json::json!({
            "kind": row.error_kind.as_deref().unwrap_or("unknown"),
            "message": row.error_message.as_deref().unwrap_or(""),
        }))
    };

    let payload = serde_json::json!({
        "event": event,
        "delivery_id": delivery_id,
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "task": {
            "id": row.id,
            "status": row.status,
            "model": row.model,
            "prompt": row.prompt,
            "created_at": row.created_at,
            "started_at": row.started_at,
            "finished_at": row.finished_at,
        },
        "result": result_val,
        "error": error_val,
    });

    if let Err(e) = sqlx::query(
        "INSERT OR IGNORE INTO webhook_deliveries (id, task_id, webhook_url, webhook_secret, payload) \
         VALUES (?, ?, ?, ?, ?)"
    )
    .bind(&delivery_id)
    .bind(task_id)
    .bind(webhook_url)
    .bind(&row.webhook_secret)
    .bind(payload.to_string())
    .execute(pool)
    .await {
        tracing::warn!(task_id, error = %e, "Failed to enqueue webhook delivery");
    }
}

/// Background worker that dispatches pending webhook deliveries.
pub async fn dispatcher_loop(pool: SqlitePool) {
    let client = Client::builder()
        .timeout(WEBHOOK_TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
        .expect("Failed to build webhook HTTP client");

    let mut interval = tokio::time::interval(Duration::from_secs(5));
    interval.tick().await; // skip first immediate tick

    loop {
        interval.tick().await;

        // Claim one pending delivery that is due
        let delivery = sqlx::query_as::<_, DeliveryRow>(
            "UPDATE webhook_deliveries SET status = 'sending', updated_at = datetime('now') \
             WHERE id = ( \
               SELECT id FROM webhook_deliveries \
               WHERE status IN ('pending', 'retrying') \
                 AND next_attempt_at <= datetime('now') \
               ORDER BY next_attempt_at ASC LIMIT 1 \
             ) RETURNING id, task_id, webhook_url, webhook_secret, payload, attempt_count"
        )
        .fetch_optional(&pool)
        .await;

        let delivery = match delivery {
            Ok(Some(d)) => d,
            Ok(None) => continue,
            Err(e) => {
                tracing::error!(error = %e, "Failed to claim webhook delivery");
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        let attempt = delivery.attempt_count + 1;
        tracing::info!(
            delivery_id = delivery.id,
            task_id = delivery.task_id,
            attempt,
            url = &delivery.webhook_url[..delivery.webhook_url.len().min(80)],
            "Sending webhook"
        );

        // Build request
        let mut req = client
            .post(&delivery.webhook_url)
            .header("Content-Type", "application/json")
            .header("X-Jimeng-Event", extract_event(&delivery.payload))
            .header("X-Jimeng-Task-Id", &delivery.task_id)
            .header("X-Jimeng-Delivery-Id", &delivery.id)
            .header("X-Jimeng-Attempt", attempt.to_string());

        // Sign payload if secret exists
        if let Some(ref secret) = delivery.webhook_secret {
            if !secret.is_empty() {
                use hmac::{Hmac, Mac};
                use sha2::Sha256;
                type HmacSha256 = Hmac<Sha256>;

                if let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes()) {
                    mac.update(delivery.payload.as_bytes());
                    let signature = hex::encode(mac.finalize().into_bytes());
                    req = req.header("X-Jimeng-Signature", format!("sha256={signature}"));
                }
            }
        }

        let result = req.body(delivery.payload.clone()).send().await;

        match result {
            Ok(resp) => {
                let status_code = resp.status().as_u16() as i32;
                if resp.status().is_success() {
                    let _ = sqlx::query(
                        "UPDATE webhook_deliveries SET status = 'delivered', \
                         attempt_count = ?, last_attempt_at = datetime('now'), \
                         last_status_code = ?, updated_at = datetime('now') WHERE id = ?"
                    )
                    .bind(attempt)
                    .bind(status_code)
                    .bind(&delivery.id)
                    .execute(&pool)
                    .await;
                    tracing::info!(delivery_id = delivery.id, status_code, "Webhook delivered");
                } else {
                    handle_retry(&pool, &delivery.id, attempt, status_code, &format!("HTTP {status_code}")).await;
                }
            }
            Err(e) => {
                handle_retry(&pool, &delivery.id, attempt, 0, &e.to_string()).await;
            }
        }
    }
}

async fn handle_retry(pool: &SqlitePool, delivery_id: &str, attempt: i32, status_code: i32, error: &str) {
    if attempt >= MAX_ATTEMPTS {
        tracing::warn!(delivery_id, attempt, error, "Webhook delivery failed permanently");
        let _ = sqlx::query(
            "UPDATE webhook_deliveries SET status = 'failed', \
             attempt_count = ?, last_attempt_at = datetime('now'), \
             last_status_code = ?, last_error = ?, updated_at = datetime('now') WHERE id = ?"
        )
        .bind(attempt)
        .bind(status_code)
        .bind(error)
        .bind(delivery_id)
        .execute(pool)
        .await;
    } else {
        let backoff = (INITIAL_BACKOFF_SECS * 2u64.pow((attempt - 1) as u32)).min(MAX_BACKOFF_SECS);
        tracing::warn!(delivery_id, attempt, backoff, error, "Webhook delivery failed, scheduling retry");
        let _ = sqlx::query(
            "UPDATE webhook_deliveries SET status = 'retrying', \
             attempt_count = ?, last_attempt_at = datetime('now'), \
             last_status_code = ?, last_error = ?, \
             next_attempt_at = datetime('now', ?), updated_at = datetime('now') WHERE id = ?"
        )
        .bind(attempt)
        .bind(status_code)
        .bind(error)
        .bind(format!("+{backoff} seconds"))
        .bind(delivery_id)
        .execute(pool)
        .await;
    }
}

fn extract_event(payload: &str) -> String {
    serde_json::from_str::<serde_json::Value>(payload)
        .ok()
        .and_then(|v| v.get("event").and_then(|e| e.as_str()).map(String::from))
        .unwrap_or_else(|| "task.unknown".to_string())
}

#[derive(sqlx::FromRow)]
struct WebhookTaskRow {
    id: String,
    status: String,
    model: String,
    prompt: String,
    video_url: Option<String>,
    error_message: Option<String>,
    error_kind: Option<String>,
    webhook_url: Option<String>,
    webhook_secret: Option<String>,
    created_at: String,
    started_at: Option<String>,
    finished_at: Option<String>,
}

#[derive(sqlx::FromRow)]
struct DeliveryRow {
    id: String,
    task_id: String,
    webhook_url: String,
    webhook_secret: Option<String>,
    payload: String,
    attempt_count: i32,
}
