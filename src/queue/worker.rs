use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use reqwest::Client;

use super::TaskQueue;
use crate::AppState;

const UPSTREAM_PENDING_STATUS: i64 = 20;
const UPSTREAM_FAILED_STATUS: i64 = 30;

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
                if is_task_cancelled(&queue, &task_id).await {
                    let _ = queue
                        .pool
                        .release_session(&session.id, false, Some("cancelled by user"))
                        .await;
                    tracing::info!(task_id, "Task cancelled by user");
                    continue;
                }

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
    let task_meta = sqlx::query_as::<_, TaskMetaRow>(
        "SELECT prompt, duration, ratio, model, request_body, request_content_type FROM tasks WHERE id = ?",
    )
    .bind(task_id)
    .fetch_one(&queue.db.pool)
    .await?
    ;

    let (request_body, content_type) = build_submit_payload(&task_meta)?;

    update_status(queue, task_id, "submitting").await;

    let response = client
        .post(format!("{}/v1/videos/submit", queue.upstream))
        .header("Authorization", format!("Bearer {jimeng_session_id}"))
        .header("Content-Type", &content_type)
        .body(request_body)
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        anyhow::bail!("Upstream returned HTTP {}: {}", status, text);
    }

    let submit_payload: serde_json::Value = response.json().await?;
    if let Some(code) = submit_payload.get("code").and_then(|v| v.as_i64()) {
        if code != 0 {
            let msg = submit_payload
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown upstream error");
            anyhow::bail!("submit failed [{code}] {msg}");
        }
    }

    let history_record_id = submit_payload
        .pointer("/data/history_record_id")
        .and_then(|v| v.as_str())
        .or_else(|| {
            submit_payload
                .pointer("/data/historyId")
                .and_then(|v| v.as_str())
        })
        .ok_or_else(|| anyhow::anyhow!("No history_record_id in submit response: {submit_payload}"))?
        .to_string();

    let _ = sqlx::query(
        "UPDATE tasks SET status = 'polling', history_record_id = ?, updated_at = datetime('now') WHERE id = ?",
    )
    .bind(&history_record_id)
    .bind(task_id)
    .execute(&queue.db.pool)
    .await;

    let poll_interval = Duration::from_secs(state.config.poll_interval_secs.max(1));
    let deadline = Instant::now() + Duration::from_secs(state.config.max_poll_duration_secs.max(60));

    loop {
        if is_task_cancelled(queue, task_id).await {
            anyhow::bail!("Task cancelled");
        }

        if Instant::now() >= deadline {
            anyhow::bail!(
                "Polling timed out after {}s",
                state.config.max_poll_duration_secs
            );
        }

        let response = client
            .get(format!(
                "{}/v1/videos/{}/status",
                queue.upstream, history_record_id
            ))
            .header("Authorization", format!("Bearer {jimeng_session_id}"))
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("status polling HTTP {}: {}", status, text);
        }

        let payload: serde_json::Value = response.json().await?;
        if let Some(code) = payload.get("code").and_then(|v| v.as_i64()) {
            if code != 0 {
                let msg = payload
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown upstream error");
                anyhow::bail!("status polling failed [{code}] {msg}");
            }
        }

        let data = payload.get("data").unwrap_or(&payload);
        let upstream_status = data.get("status").and_then(|v| v.as_i64()).unwrap_or(UPSTREAM_PENDING_STATUS);
        let queue_position = parse_i32(data.get("queue_position"));
        let queue_total = parse_i32(data.get("queue_total"));
        let queue_eta = data
            .get("queue_eta")
            .and_then(|v| v.as_str())
            .map(|v| v.to_string());

        let _ = sqlx::query(
            "UPDATE tasks SET status = 'polling', queue_position = ?, queue_total = ?, \
             queue_eta = ?, updated_at = datetime('now') WHERE id = ?",
        )
        .bind(queue_position)
        .bind(queue_total)
        .bind(queue_eta)
        .bind(task_id)
        .execute(&queue.db.pool)
        .await;

        if upstream_status == UPSTREAM_FAILED_STATUS {
            let fail_code = data
                .get("fail_code")
                .and_then(|v| v.as_i64())
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            anyhow::bail!("Upstream task failed with code {fail_code}");
        }

        if let Some(video_url) = data.get("video_url").and_then(|v| v.as_str()) {
            if !video_url.is_empty() {
                update_status(queue, task_id, "downloading").await;
                return Ok(video_url.to_string());
            }
        }

        if upstream_status != UPSTREAM_PENDING_STATUS {
            anyhow::bail!(
                "Upstream returned status {} without video_url: {}",
                upstream_status,
                payload
            );
        }

        tokio::time::sleep(poll_interval).await;
    }
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
    let msg = msg.to_lowercase();
    if msg.contains("authorization") || msg.contains("unauthorized") || msg.contains("login") || msg.contains("token") {
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
    request_body: Option<Vec<u8>>,
    request_content_type: Option<String>,
}

fn parse_i32(value: Option<&serde_json::Value>) -> Option<i32> {
    match value {
        Some(v) if v.is_number() => v.as_i64().map(|n| n as i32),
        Some(v) if v.is_string() => v.as_str()?.parse::<i32>().ok(),
        _ => None,
    }
}

fn build_submit_payload(task: &TaskMetaRow) -> Result<(Vec<u8>, String)> {
    if let Some(body) = &task.request_body {
        let content_type = task
            .request_content_type
            .clone()
            .unwrap_or_else(|| "application/json".to_string());
        return Ok((body.clone(), content_type));
    }

    let payload = serde_json::json!({
        "prompt": task.prompt,
        "model": task.model,
        "duration": task.duration,
        "ratio": task.ratio,
    });
    Ok((serde_json::to_vec(&payload)?, "application/json".to_string()))
}

async fn is_task_cancelled(queue: &TaskQueue, task_id: &str) -> bool {
    match sqlx::query_scalar::<_, String>("SELECT status FROM tasks WHERE id = ?")
        .bind(task_id)
        .fetch_optional(&queue.db.pool)
        .await
    {
        Ok(Some(status)) => status == "cancelled",
        _ => false,
    }
}
