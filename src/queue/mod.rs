mod worker;

use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::{Notify, RwLock};

use crate::db::Database;
use crate::pool::SessionPool;

/// Task status in the gateway's lifecycle.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Queued,
    Submitting,
    Polling,
    Downloading,
    Succeeded,
    Failed,
    Cancelled,
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Queued => write!(f, "queued"),
            Self::Submitting => write!(f, "submitting"),
            Self::Polling => write!(f, "polling"),
            Self::Downloading => write!(f, "downloading"),
            Self::Succeeded => write!(f, "succeeded"),
            Self::Failed => write!(f, "failed"),
            Self::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// A video generation task record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRecord {
    pub id: String,
    pub status: TaskStatus,
    pub model: String,
    pub prompt: String,
    pub duration: i32,
    pub ratio: String,
    pub session_pool_id: Option<String>,
    /// The jimeng-internal history_record_id (obtained after Playwright submission).
    pub history_record_id: Option<String>,
    /// Queue position from jimeng (e.g., 26785).
    pub queue_position: Option<i32>,
    /// Total queue size (e.g., 91854).
    pub queue_total: Option<i32>,
    /// Estimated remaining time (e.g., "4小时").
    pub queue_eta: Option<String>,
    /// Final video URL.
    pub video_url: Option<String>,
    pub error_message: Option<String>,
    pub error_kind: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateTaskRequest {
    pub prompt: String,
    pub duration: Option<i32>,
    pub ratio: Option<String>,
    pub model: Option<String>,
    /// Base64-encoded files or file URLs.
    pub files: Option<Vec<FileInput>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileInput {
    /// Base64 data or URL.
    pub data: String,
    pub filename: String,
    pub mime_type: String,
}

/// Manages the async task queue.
#[derive(Clone)]
pub struct TaskQueue {
    db: Database,
    pool: SessionPool,
    concurrency: usize,
    notify: Arc<Notify>,
    running: Arc<RwLock<usize>>,
}

impl TaskQueue {
    pub fn new(db: Database, pool: SessionPool, concurrency: usize) -> Self {
        Self {
            db,
            pool,
            concurrency,
            notify: Arc::new(Notify::new()),
            running: Arc::new(RwLock::new(0)),
        }
    }

    /// Enqueue a new video generation task.
    pub async fn enqueue(
        &self,
        req: CreateTaskRequest,
        request_body: Option<Vec<u8>>,
        request_content_type: Option<String>,
    ) -> Result<TaskRecord> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

        let model = req.model.unwrap_or_else(|| "jimeng-video-seedance-2.0".to_string());
        let duration = req.duration.unwrap_or(4);
        let ratio = req.ratio.unwrap_or_else(|| "9:16".to_string());

        sqlx::query(
            "INSERT INTO tasks (id, status, model, prompt, duration, ratio, request_body, request_content_type, created_at, updated_at) \
             VALUES (?, 'queued', ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&model)
        .bind(&req.prompt)
        .bind(duration)
        .bind(&ratio)
        .bind(&request_body)
        .bind(&request_content_type)
        .bind(&now)
        .bind(&now)
        .execute(&self.db.pool)
        .await?;

        let task = TaskRecord {
            id,
            status: TaskStatus::Queued,
            model,
            prompt: req.prompt,
            duration,
            ratio,
            session_pool_id: None,
            history_record_id: None,
            queue_position: None,
            queue_total: None,
            queue_eta: None,
            video_url: None,
            error_message: None,
            error_kind: None,
            created_at: now.clone(),
            updated_at: now,
            started_at: None,
            finished_at: None,
        };

        // Wake up worker
        self.notify.notify_one();
        tracing::info!(task_id = %task.id, "Task enqueued");

        Ok(task)
    }

    /// List tasks with optional status filter.
    pub async fn list_tasks(&self, status: Option<&str>, limit: i64) -> Result<Vec<TaskRecord>> {
        let tasks = if let Some(status) = status {
            sqlx::query_as::<_, TaskQueryRow>(
                "SELECT id, status, model, prompt, duration, ratio, session_pool_id, \
                 history_record_id, queue_position, queue_total, queue_eta, \
                 video_url, error_message, error_kind, \
                 created_at, updated_at, started_at, finished_at \
                 FROM tasks WHERE status = ? ORDER BY created_at DESC LIMIT ?",
            )
            .bind(status)
            .bind(limit)
            .fetch_all(&self.db.pool)
            .await?
        } else {
            sqlx::query_as::<_, TaskQueryRow>(
                "SELECT id, status, model, prompt, duration, ratio, session_pool_id, \
                 history_record_id, queue_position, queue_total, queue_eta, \
                 video_url, error_message, error_kind, \
                 created_at, updated_at, started_at, finished_at \
                 FROM tasks ORDER BY created_at DESC LIMIT ?",
            )
            .bind(limit)
            .fetch_all(&self.db.pool)
            .await?
        };

        Ok(tasks.into_iter().map(Into::into).collect())
    }

    /// Get a single task by ID.
    pub async fn get_task(&self, id: &str) -> Result<Option<TaskRecord>> {
        let row = sqlx::query_as::<_, TaskQueryRow>(
            "SELECT id, status, model, prompt, duration, ratio, session_pool_id, \
             history_record_id, queue_position, queue_total, queue_eta, \
             video_url, error_message, error_kind, \
             created_at, updated_at, started_at, finished_at \
             FROM tasks WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.db.pool)
        .await?;

        Ok(row.map(Into::into))
    }

    /// Cancel a task.
    pub async fn cancel_task(&self, id: &str) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE tasks SET status = 'cancelled', updated_at = datetime('now'), \
             finished_at = datetime('now') WHERE id = ? AND status IN ('queued', 'submitting', 'polling')",
        )
        .bind(id)
        .execute(&self.db.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Retry a task by cloning its original payload into a new queued record.
    pub async fn retry_task(&self, id: &str) -> Result<Option<TaskRecord>> {
        let src = sqlx::query_as::<_, RetryTaskRow>(
            "SELECT model, prompt, duration, ratio, request_body, request_content_type \
             FROM tasks WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.db.pool)
        .await?;

        let Some(src) = src else {
            return Ok(None);
        };

        let req = CreateTaskRequest {
            prompt: src.prompt,
            duration: Some(src.duration),
            ratio: Some(src.ratio),
            model: Some(src.model),
            files: None,
        };

        let task = self
            .enqueue(req, src.request_body, src.request_content_type)
            .await?;
        Ok(Some(task))
    }

    /// Get stats summary.
    pub async fn stats(&self) -> Result<serde_json::Value> {
        let row = sqlx::query_as::<_, StatsRow>(
            "SELECT \
               COALESCE(COUNT(*), 0) as total, \
               COALESCE(SUM(CASE WHEN status = 'queued' THEN 1 ELSE 0 END), 0) as queued, \
               COALESCE(SUM(CASE WHEN status IN ('submitting', 'polling', 'downloading') THEN 1 ELSE 0 END), 0) as running, \
               COALESCE(SUM(CASE WHEN status = 'succeeded' THEN 1 ELSE 0 END), 0) as succeeded, \
               COALESCE(SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END), 0) as failed, \
               COALESCE(SUM(CASE WHEN status = 'cancelled' THEN 1 ELSE 0 END), 0) as cancelled \
             FROM tasks",
        )
        .fetch_one(&self.db.pool)
        .await?;

        Ok(serde_json::json!({
            "total": row.total,
            "queued": row.queued,
            "running": row.running,
            "succeeded": row.succeeded,
            "failed": row.failed,
            "cancelled": row.cancelled,
        }))
    }

    /// Start background workers.
    pub fn start_workers(&self, state: Arc<crate::AppState>) {
        for i in 0..self.concurrency {
            let queue = self.clone();
            let state = state.clone();
            tokio::spawn(async move {
                tracing::info!(worker = i, "Worker started");
                worker::worker_loop(queue, state).await;
            });
        }
    }
}

// Internal query types for sqlx
#[derive(sqlx::FromRow)]
struct TaskQueryRow {
    id: String,
    status: String,
    model: String,
    prompt: String,
    duration: i32,
    ratio: String,
    session_pool_id: Option<String>,
    history_record_id: Option<String>,
    queue_position: Option<i32>,
    queue_total: Option<i32>,
    queue_eta: Option<String>,
    video_url: Option<String>,
    error_message: Option<String>,
    error_kind: Option<String>,
    created_at: String,
    updated_at: String,
    started_at: Option<String>,
    finished_at: Option<String>,
}

impl From<TaskQueryRow> for TaskRecord {
    fn from(row: TaskQueryRow) -> Self {
        Self {
            id: row.id,
            status: match row.status.as_str() {
                "queued" => TaskStatus::Queued,
                "submitting" => TaskStatus::Submitting,
                "polling" => TaskStatus::Polling,
                "downloading" => TaskStatus::Downloading,
                "succeeded" => TaskStatus::Succeeded,
                "failed" => TaskStatus::Failed,
                "cancelled" => TaskStatus::Cancelled,
                _ => TaskStatus::Failed,
            },
            model: row.model,
            prompt: row.prompt,
            duration: row.duration,
            ratio: row.ratio,
            session_pool_id: row.session_pool_id,
            history_record_id: row.history_record_id,
            queue_position: row.queue_position,
            queue_total: row.queue_total,
            queue_eta: row.queue_eta,
            video_url: row.video_url,
            error_message: row.error_message,
            error_kind: row.error_kind,
            created_at: row.created_at,
            updated_at: row.updated_at,
            started_at: row.started_at,
            finished_at: row.finished_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct StatsRow {
    total: i32,
    queued: i32,
    running: i32,
    succeeded: i32,
    failed: i32,
    cancelled: i32,
}

#[derive(sqlx::FromRow)]
struct RetryTaskRow {
    model: String,
    prompt: String,
    duration: i32,
    ratio: String,
    request_body: Option<Vec<u8>>,
    request_content_type: Option<String>,
}
