use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;

use super::TaskQueue;
use crate::AppState;

/// Background worker: dequeue tasks, submit to jimeng, poll for results.
pub async fn worker_loop(queue: TaskQueue, state: Arc<AppState>) {
    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .expect("Failed to build HTTP client");

    loop {
        // Wait for notification or check periodically
        tokio::select! {
            () = queue.notify.notified() => {},
            () = tokio::time::sleep(Duration::from_secs(5)) => {},
        }

        // Try to claim a queued task
        let task_row = sqlx::query_as::<_, TaskIdRow>(
            "UPDATE tasks SET status = 'submitting', started_at = datetime('now'), \
             updated_at = datetime('now') \
             WHERE id = (SELECT id FROM tasks WHERE status = 'queued' ORDER BY created_at LIMIT 1) \
             RETURNING id",
        )
        .fetch_optional(&queue.db.pool)
        .await;

        let task_id = match task_row {
            Ok(Some(row)) => row.id,
            Ok(None) => continue,
            Err(e) => {
                tracing::error!("Failed to claim task: {e}");
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };

        // Pick a session
        let session = match queue.pool.pick_session().await {
            Some(s) => s,
            None => {
                tracing::warn!(task_id, "No available session, re-queuing task");
                let _ = sqlx::query(
                    "UPDATE tasks SET status = 'queued', updated_at = datetime('now') WHERE id = ?",
                )
                .bind(&task_id)
                .execute(&queue.db.pool)
                .await;
                tokio::time::sleep(Duration::from_secs(10)).await;
                continue;
            }
        };

        // Update task with session assignment
        let _ = sqlx::query(
            "UPDATE tasks SET session_pool_id = ?, updated_at = datetime('now') WHERE id = ?",
        )
        .bind(&session.id)
        .bind(&task_id)
        .execute(&queue.db.pool)
        .await;

        let _ = queue.pool.mark_active(&session.id).await;
        *queue.running.write().await += 1;

        tracing::info!(
            task_id,
            session_id = session.id,
            "Processing task"
        );

        // Execute the full pipeline: submit → poll → download
        let result = execute_task(&queue, &state, &client, &task_id, &session.session_id).await;

        *queue.running.write().await -= 1;

        match result {
            Ok(video_url) => {
                let _ = sqlx::query(
                    "UPDATE tasks SET status = 'succeeded', video_url = ?, \
                     finished_at = datetime('now'), updated_at = datetime('now') WHERE id = ?",
                )
                .bind(&video_url)
                .bind(&task_id)
                .execute(&queue.db.pool)
                .await;

                let _ = queue.pool.release_session(&session.id, true, None).await;
                tracing::info!(task_id, "Task succeeded");
            }
            Err(e) => {
                let err_msg = e.to_string();
                let err_kind = classify_error(&err_msg);

                let _ = sqlx::query(
                    "UPDATE tasks SET status = 'failed', error_message = ?, error_kind = ?, \
                     finished_at = datetime('now'), updated_at = datetime('now') WHERE id = ?",
                )
                .bind(&err_msg)
                .bind(&err_kind)
                .bind(&task_id)
                .execute(&queue.db.pool)
                .await;

                let _ = queue
                    .pool
                    .release_session(&session.id, false, Some(&err_msg))
                    .await;

                if err_kind == "auth" {
                    let _ = queue.pool.mark_unhealthy(&session.id).await;
                    tracing::warn!(task_id, session = session.id, "Session marked unhealthy");
                }

                tracing::error!(task_id, error = %e, "Task failed");
            }
        }
    }
}

/// Execute the full video generation pipeline.
async fn execute_task(
    queue: &TaskQueue,
    state: &AppState,
    client: &Client,
    task_id: &str,
    jimeng_session_id: &str,
) -> Result<String> {
    // Step 1: Forward the original request body to jimeng upstream for submission
    // (this triggers Playwright/shark bypass and returns video URL after polling)
    let request_body = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT request_body FROM tasks WHERE id = ?",
    )
    .bind(task_id)
    .fetch_optional(&queue.db.pool)
    .await?
    .ok_or_else(|| anyhow::anyhow!("No request body stored for task {task_id}"))?;

    // Extract content type from stored request
    let _task_meta = sqlx::query_as::<_, TaskMetaRow>(
        "SELECT prompt, duration, ratio, model FROM tasks WHERE id = ?",
    )
    .bind(task_id)
    .fetch_one(&queue.db.pool)
    .await?;

    update_status(queue, task_id, "submitting").await;

    // Phase 1: Submit to jimeng (Playwright handles shark bypass)
    // The upstream jimeng API blocks until video is done or times out.
    // We use a long timeout since queue can take hours.
    let submit_client = Client::builder()
        .timeout(Duration::from_secs(state.config.max_poll_duration_secs))
        .build()?;

    let response = submit_client
        .post(format!("{}/v1/videos/generations", queue.upstream))
        .header("Authorization", format!("Bearer {jimeng_session_id}"))
        .header("Content-Type", "application/octet-stream")
        .body(request_body)
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        anyhow::bail!("Upstream returned HTTP {}: {}", status, text);
    }

    let payload: serde_json::Value = response.json().await?;

    // Check for error in response
    if let Some(code) = payload.get("code").and_then(|v| v.as_i64()) {
        if code != 0 {
            let msg = payload
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown upstream error");
            anyhow::bail!("[{code}] {msg}");
        }
    }

    // Extract video URL
    let video_url = payload
        .pointer("/data/0/url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("No video URL in response: {}", payload))?;

    Ok(video_url.to_string())
}

async fn update_status(queue: &TaskQueue, task_id: &str, status: &str) {
    let _ = sqlx::query(
        "UPDATE tasks SET status = ?, updated_at = datetime('now') WHERE id = ?",
    )
    .bind(status)
    .bind(task_id)
    .execute(&queue.db.pool)
    .await;
}

fn classify_error(msg: &str) -> &'static str {
    if msg.contains("authorization") || msg.contains("login") || msg.contains("token") {
        "auth"
    } else if msg.contains("timeout") || msg.contains("timed out") {
        "timeout"
    } else if msg.contains("平台规则") || msg.contains("内容违规") {
        "platform_rule"
    } else if msg.contains("network") || msg.contains("ECONNREFUSED") {
        "network"
    } else {
        "unknown"
    }
}

#[derive(sqlx::FromRow)]
struct TaskIdRow {
    id: String,
}

#[derive(sqlx::FromRow)]
struct TaskMetaRow {
    prompt: String,
    duration: i32,
    ratio: String,
    model: String,
}

/// Jimeng `get_history_by_ids` response structures for future direct polling.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct JimengHistoryResponse {
    pub ret: String,
    pub errmsg: String,
    pub data: std::collections::HashMap<String, JimengHistoryRecord>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct JimengHistoryRecord {
    pub history_record_id: String,
    pub task: JimengTask,
    pub item_list: Vec<JimengItem>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct JimengTask {
    pub status: i32,
    pub finish_time: i64,
    /// Queue position info (if available in the response).
    pub queue_info: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct JimengItem {
    pub url: Option<String>,
    pub video_url: Option<String>,
}
