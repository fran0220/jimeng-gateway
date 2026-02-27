mod session;

pub use session::SessionInfo;

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::RwLock;

use crate::db::Database;

#[derive(Debug, Clone)]
pub struct SessionPool {
    db: Database,
    sessions: Arc<RwLock<Vec<SessionInfo>>>,
}

impl SessionPool {
    pub fn new(db: Database) -> Self {
        Self {
            db,
            sessions: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Load all sessions from database into memory.
    pub async fn load_sessions(&self) -> Result<()> {
        let rows = sqlx::query_as::<_, SessionInfo>(
            "SELECT id, label, session_id, enabled, healthy, active_tasks, total_tasks, \
             success_count, fail_count, last_used_at, last_error, created_at, updated_at \
             FROM sessions ORDER BY created_at"
        )
        .fetch_all(&self.db.pool)
        .await?;

        let count = rows.len();
        *self.sessions.write().await = rows;
        tracing::info!(count, "Loaded sessions into pool");
        Ok(())
    }

    /// Pick the best available session using atomic DB-level CAS.
    pub async fn pick_session(&self) -> Option<SessionInfo> {
        // Atomic pick + reserve: single SQL statement prevents race conditions
        let row = sqlx::query_as::<_, SessionInfo>(
            "UPDATE sessions SET active_tasks = active_tasks + 1, \
             last_used_at = datetime('now'), updated_at = datetime('now') \
             WHERE id = (SELECT id FROM sessions WHERE enabled=1 AND healthy=1 AND active_tasks < 2 \
                         ORDER BY last_used_at LIMIT 1) \
             RETURNING id, label, session_id, enabled, healthy, active_tasks, total_tasks, \
                       success_count, fail_count, last_used_at, last_error, created_at, updated_at",
        )
        .fetch_optional(&self.db.pool)
        .await
        .ok()?;

        if let Some(ref session) = row {
            // Sync in-memory cache
            let mut sessions = self.sessions.write().await;
            if let Some(s) = sessions.iter_mut().find(|s| s.id == session.id) {
                s.active_tasks = session.active_tasks;
                s.last_used_at = session.last_used_at.clone();
            }
        }

        row
    }

    /// No-op: pick_session() atomically increments active_tasks.
    pub async fn mark_active(&self, _session_id: &str) -> Result<()> {
        Ok(())
    }

    /// Release a session (decrement active_tasks) and record result.
    pub async fn release_session(&self, session_id: &str, success: bool, error: Option<&str>) -> Result<()> {
        let success_col = if success { "success_count" } else { "fail_count" };
        let query = format!(
            "UPDATE sessions SET active_tasks = MAX(0, active_tasks - 1), \
             total_tasks = total_tasks + 1, \
             {success_col} = {success_col} + 1, \
             last_error = CASE WHEN ? IS NOT NULL THEN ? ELSE last_error END, \
             updated_at = datetime('now') \
             WHERE id = ?",
        );
        sqlx::query(&query)
            .bind(error)
            .bind(error)
            .bind(session_id)
            .execute(&self.db.pool)
            .await?;

        let mut sessions = self.sessions.write().await;
        if let Some(s) = sessions.iter_mut().find(|s| s.id == session_id) {
            s.active_tasks = s.active_tasks.saturating_sub(1);
            s.total_tasks += 1;
            if success {
                s.success_count += 1;
            } else {
                s.fail_count += 1;
            }
        }
        Ok(())
    }

    /// Mark a session as unhealthy (auto-disable).
    pub async fn mark_unhealthy(&self, session_id: &str) -> Result<()> {
        sqlx::query(
            "UPDATE sessions SET healthy = 0, updated_at = datetime('now') WHERE id = ?",
        )
        .bind(session_id)
        .execute(&self.db.pool)
        .await?;

        let mut sessions = self.sessions.write().await;
        if let Some(s) = sessions.iter_mut().find(|s| s.id == session_id) {
            s.healthy = false;
        }
        Ok(())
    }

    /// Add a new session.
    pub async fn add_session(&self, label: &str, jimeng_session_id: &str) -> Result<SessionInfo> {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO sessions (id, label, session_id) VALUES (?, ?, ?)",
        )
        .bind(&id)
        .bind(label)
        .bind(jimeng_session_id)
        .execute(&self.db.pool)
        .await?;

        let session = SessionInfo {
            id: id.clone(),
            label: label.to_string(),
            session_id: jimeng_session_id.to_string(),
            enabled: true,
            healthy: true,
            active_tasks: 0,
            total_tasks: 0,
            success_count: 0,
            fail_count: 0,
            last_used_at: None,
            last_error: None,
            created_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            updated_at: chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        };

        self.sessions.write().await.push(session.clone());
        tracing::info!(id, label, "Session added to pool");
        Ok(session)
    }

    /// Remove a session.
    pub async fn remove_session(&self, id: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(id)
            .execute(&self.db.pool)
            .await?;

        self.sessions.write().await.retain(|s| s.id != id);
        Ok(result.rows_affected() > 0)
    }

    /// Toggle enabled/disabled.
    pub async fn toggle_session(&self, id: &str, enabled: bool) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE sessions SET enabled = ?, healthy = CASE WHEN ? THEN 1 ELSE healthy END, \
             updated_at = datetime('now') WHERE id = ?",
        )
        .bind(enabled)
        .bind(enabled)
        .bind(id)
        .execute(&self.db.pool)
        .await?;

        let mut sessions = self.sessions.write().await;
        if let Some(s) = sessions.iter_mut().find(|s| s.id == id) {
            s.enabled = enabled;
            if enabled {
                s.healthy = true;
            }
        }
        Ok(result.rows_affected() > 0)
    }

    /// List all sessions (for API response).
    pub async fn list_sessions(&self) -> Vec<SessionInfo> {
        self.sessions.read().await.clone()
    }
}
