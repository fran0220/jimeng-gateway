use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use reqwest::Client;

use super::TaskQueue;
use crate::AppState;
use crate::jimeng::{models, poll, submit, upload};
use crate::jimeng::models::{MaterialType, UploadedMaterial};

/// Background worker: dequeue tasks, submit to jimeng, poll for results.
pub async fn worker_loop(queue: TaskQueue, state: Arc<AppState>) {
    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .expect("Failed to build HTTP client");

    loop {
        tokio::select! {
            () = queue.notify.notified() => {},
            () = tokio::time::sleep(Duration::from_secs(5)) => {},
        }

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

        let session = match queue.pool.pick_session().await {
            Some(s) => s,
            None => {
                tracing::warn!(task_id, "No available session, re-queuing task");
                if let Err(e) = sqlx::query(
                    "UPDATE tasks SET status = 'queued', updated_at = datetime('now') WHERE id = ?",
                )
                .bind(&task_id)
                .execute(&queue.db.pool)
                .await {
                    tracing::warn!(task_id, error = %e, "Failed to re-queue task");
                }
                tokio::time::sleep(Duration::from_secs(10)).await;
                continue;
            }
        };

        if let Err(e) = sqlx::query(
            "UPDATE tasks SET session_pool_id = ?, updated_at = datetime('now') WHERE id = ?",
        )
        .bind(&session.id)
        .bind(&task_id)
        .execute(&queue.db.pool)
        .await {
            tracing::warn!(task_id, error = %e, "Failed to assign session");
        }

        *queue.running.write().await += 1;

        tracing::info!(task_id, session_id = session.id, "Processing task");

        let result = execute_task(&queue, &state, &client, &task_id, &session.session_id).await;

        *queue.running.write().await -= 1;

        match result {
            Ok(video_url) => {
                if let Err(e) = sqlx::query(
                    "UPDATE tasks SET status = 'succeeded', video_url = ?, \
                     finished_at = datetime('now'), updated_at = datetime('now') \
                     WHERE id = ? AND status != 'cancelled'",
                )
                .bind(&video_url)
                .bind(&task_id)
                .execute(&queue.db.pool)
                .await {
                    tracing::warn!(task_id, error = %e, "Failed to mark task succeeded");
                }

                let _ = queue.pool.release_session(&session.id, true, None).await;
                tracing::info!(task_id, "Task succeeded");
            }
            Err(e) => {
                if is_task_cancelled(&queue, &task_id).await {
                    let _ = queue.pool.release_session(&session.id, false, Some("cancelled by user")).await;
                    tracing::info!(task_id, "Task cancelled by user");
                    continue;
                }

                let err_msg = e.to_string();
                let err_kind = classify_error(&err_msg);

                if let Err(e) = sqlx::query(
                    "UPDATE tasks SET status = 'failed', error_message = ?, error_kind = ?, \
                     finished_at = datetime('now'), updated_at = datetime('now') WHERE id = ?",
                )
                .bind(&err_msg)
                .bind(&err_kind)
                .bind(&task_id)
                .execute(&queue.db.pool)
                .await {
                    tracing::warn!(task_id, error = %e, "Failed to mark task failed");
                }

                let _ = queue.pool.release_session(&session.id, false, Some(&err_msg)).await;

                if err_kind == "auth" {
                    let _ = queue.pool.mark_unhealthy(&session.id).await;
                    tracing::warn!(task_id, session = session.id, "Session marked unhealthy");
                }

                tracing::error!(task_id, error = %e, "Task failed");
            }
        }
    }
}

/// Execute the full video generation pipeline via direct jimeng API.
async fn execute_task(
    queue: &TaskQueue,
    state: &AppState,
    client: &Client,
    task_id: &str,
    session_token: &str,
) -> Result<String> {
    let task_meta = sqlx::query_as::<_, TaskMetaRow>(
        "SELECT prompt, duration, ratio, model, request_body, request_content_type FROM tasks WHERE id = ?",
    )
    .bind(task_id)
    .fetch_one(&queue.db.pool)
    .await?;

    let model_name = &task_meta.model;
    let is_seedance = models::is_seedance_model(model_name);
    let resolution = "720p"; // default resolution
    let res = models::resolve_video_resolution(resolution, &task_meta.ratio)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    update_status(queue, task_id, "submitting").await;

    // Process uploaded materials from multipart body
    let materials = process_materials(
        client,
        session_token,
        task_meta.request_body.as_deref(),
        task_meta.request_content_type.as_deref(),
    ).await;

    // Submit task
    let submit_result = if is_seedance {
        tracing::info!(task_id, materials_count = materials.len(), "Submitting Seedance task via browser proxy");
        submit::submit_seedance_video(
            client,
            &state.browser,
            session_token,
            &task_meta.prompt,
            model_name,
            res.width,
            res.height,
            task_meta.duration as u32,
            &materials,
        ).await?
    } else {
        // For regular video: use first image as first_frame
        let first_frame_uri = materials.iter()
            .find(|m| m.material_type == MaterialType::Image)
            .and_then(|m| m.uri.as_deref());

        tracing::info!(task_id, ?first_frame_uri, "Submitting regular video task");
        submit::submit_regular_video(
            client,
            session_token,
            &task_meta.prompt,
            model_name,
            res.width,
            res.height,
            task_meta.duration as u32,
            resolution,
            first_frame_uri,
            None, // end_frame
        ).await?
    };

    let history_record_id = submit_result.history_record_id;
    tracing::info!(task_id, %history_record_id, "Task submitted, starting poll");

    let _ = sqlx::query(
        "UPDATE tasks SET status = 'polling', history_record_id = ?, updated_at = datetime('now') WHERE id = ?",
    )
    .bind(&history_record_id)
    .bind(task_id)
    .execute(&queue.db.pool)
    .await;

    // Poll for results
    let poll_interval = Duration::from_secs(state.config.poll_interval_secs.max(1));
    let deadline = Instant::now() + Duration::from_secs(state.config.max_poll_duration_secs.max(60));

    loop {
        if is_task_cancelled(queue, task_id).await {
            anyhow::bail!("Task cancelled");
        }

        if Instant::now() >= deadline {
            anyhow::bail!("Polling timed out after {}s", state.config.max_poll_duration_secs);
        }

        let poll_result = poll::poll_status(client, session_token, &history_record_id).await?;

        // Update queue progress
        let _ = sqlx::query(
            "UPDATE tasks SET status = 'polling', queue_position = ?, queue_total = ?, \
             queue_eta = ?, updated_at = datetime('now') WHERE id = ?",
        )
        .bind(poll_result.queue_position)
        .bind(poll_result.queue_total)
        .bind(&poll_result.queue_eta)
        .bind(task_id)
        .execute(&queue.db.pool)
        .await;

        if poll_result.status == poll::STATUS_FAILED {
            let fail_code = poll_result.fail_code.as_deref().unwrap_or("unknown");
            let fail_msg = poll_result.fail_msg.as_deref().unwrap_or("");
            anyhow::bail!("Upstream task failed with code {fail_code}: {fail_msg}");
        }

        if let Some(ref video_url) = poll_result.video_url {
            if !video_url.is_empty() {
                update_status(queue, task_id, "downloading").await;

                // Try to get high-quality URL
                if let Some(ref item_id) = poll_result.item_id {
                    match poll::fetch_hq_video_url(client, session_token, item_id).await {
                        Ok(Some(hq_url)) => {
                            tracing::info!(task_id, "Got HQ video URL");
                            return Ok(hq_url);
                        }
                        Ok(None) => {}
                        Err(e) => {
                            tracing::warn!(task_id, error = %e, "Failed to get HQ video URL, using preview");
                        }
                    }
                }

                return Ok(video_url.clone());
            }
        }

        if poll_result.status != poll::STATUS_PENDING {
            anyhow::bail!("Upstream returned status {} without video_url", poll_result.status);
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
    } else if msg.contains("network") || msg.contains("econnrefused") {
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

/// Process uploaded materials from a stored multipart request body.
/// Returns empty Vec on any failure (graceful degradation).
async fn process_materials(
    client: &Client,
    session_token: &str,
    request_body: Option<&[u8]>,
    request_content_type: Option<&str>,
) -> Vec<UploadedMaterial> {
    let (body, ct) = match (request_body, request_content_type) {
        (Some(b), Some(ct)) if !b.is_empty() => (b, ct),
        _ => return Vec::new(),
    };

    let files = match extract_multipart_files(ct, body) {
        Ok(f) if !f.is_empty() => f,
        Ok(_) => return Vec::new(),
        Err(e) => {
            tracing::warn!(error = %e, "Failed to parse multipart files");
            return Vec::new();
        }
    };

    tracing::info!(file_count = files.len(), "Processing uploaded materials");
    let mut materials = Vec::new();

    for file in files {
        let material_type = models::detect_material_type_from_mime(&file.content_type);
        tracing::info!(
            filename = file.filename,
            mime = file.content_type,
            size = file.data.len(),
            ?material_type,
            "Uploading material"
        );

        match material_type {
            MaterialType::Image => {
                match upload::upload_image(client, session_token, &file.data).await {
                    Ok(uri) => {
                        tracing::info!(filename = file.filename, %uri, "Image uploaded");
                        materials.push(UploadedMaterial {
                            material_type,
                            uri: Some(uri),
                            vid: None,
                            width: 0,
                            height: 0,
                            duration: 0,
                            fps: 0,
                            name: file.filename,
                        });
                    }
                    Err(e) => tracing::warn!(filename = file.filename, error = %e, "Image upload failed"),
                }
            }
            MaterialType::Video | MaterialType::Audio => {
                match upload::upload_media(client, session_token, &file.data, material_type).await {
                    Ok(result) => {
                        tracing::info!(filename = file.filename, vid = %result.vid, "Media uploaded");
                        materials.push(UploadedMaterial {
                            material_type,
                            uri: None,
                            vid: Some(result.vid),
                            width: result.width,
                            height: result.height,
                            duration: result.duration,
                            fps: result.fps,
                            name: file.filename,
                        });
                    }
                    Err(e) => tracing::warn!(filename = file.filename, error = %e, "Media upload failed"),
                }
            }
        }
    }

    materials
}

/// A file part extracted from multipart form data.
struct MultipartFile {
    filename: String,
    content_type: String,
    data: Vec<u8>,
}

/// Extract binary file parts from a raw multipart body.
fn extract_multipart_files(content_type: &str, body: &[u8]) -> Result<Vec<MultipartFile>> {
    let boundary = content_type
        .split("boundary=")
        .nth(1)
        .unwrap_or("")
        .trim();

    if boundary.is_empty() {
        return Ok(Vec::new());
    }

    let delimiter = format!("--{boundary}").into_bytes();
    let mut files = Vec::new();

    // Split body on boundary delimiter
    let mut parts: Vec<&[u8]> = Vec::new();
    let mut start = 0;
    while let Some(pos) = find_subsequence(&body[start..], &delimiter) {
        if start > 0 {
            parts.push(&body[start..start + pos]);
        }
        start += pos + delimiter.len();
    }
    // Last part after final delimiter
    if start < body.len() {
        parts.push(&body[start..]);
    }

    for part in parts {
        // Skip closing boundary marker "--\r\n"
        if part.starts_with(b"--") {
            continue;
        }

        // Find header/body separator \r\n\r\n
        let sep = b"\r\n\r\n";
        let header_end = match find_subsequence(part, sep) {
            Some(pos) => pos,
            None => continue,
        };

        let header_bytes = &part[..header_end];
        let file_data = &part[header_end + 4..];
        // Trim trailing \r\n
        let file_data = file_data.strip_suffix(b"\r\n").unwrap_or(file_data);

        let header_str = String::from_utf8_lossy(header_bytes);

        // Only process parts with a filename
        let filename = match extract_header_value(&header_str, "filename=\"", "\"") {
            Some(f) if !f.is_empty() => f,
            _ => continue,
        };

        // Get Content-Type from part headers, default to application/octet-stream
        let part_content_type = header_str.lines()
            .find(|line| line.to_lowercase().starts_with("content-type:"))
            .map(|line| line.split_once(':').unwrap().1.trim().to_string())
            .unwrap_or_else(|| "application/octet-stream".to_string());

        files.push(MultipartFile {
            filename,
            content_type: part_content_type,
            data: file_data.to_vec(),
        });
    }

    Ok(files)
}

/// Find the position of a subsequence in a byte slice.
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Extract a value between start_marker and end_marker from a header string.
fn extract_header_value(header: &str, start_marker: &str, end_marker: &str) -> Option<String> {
    let start = header.find(start_marker)? + start_marker.len();
    let end = header[start..].find(end_marker)? + start;
    Some(header[start..end].to_string())
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
