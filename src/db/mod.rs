use anyhow::Result;
use sqlx::SqlitePool;

#[derive(Debug, Clone)]
pub struct Database {
    pub pool: SqlitePool,
}

impl Database {
    pub async fn connect(url: &str) -> Result<Self> {
        // Ensure data directory exists
        if let Some(path) = url.strip_prefix("sqlite://") {
            let path = path.split('?').next().unwrap_or(path);
            if let Some(parent) = std::path::Path::new(path).parent() {
                std::fs::create_dir_all(parent)?;
            }
        }

        let pool = SqlitePool::connect(url).await?;
        Ok(Self { pool })
    }

    pub async fn migrate(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                label TEXT NOT NULL DEFAULT '',
                session_id TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                healthy INTEGER NOT NULL DEFAULT 1,
                active_tasks INTEGER NOT NULL DEFAULT 0,
                total_tasks Integer NOT NULL DEFAULT 0,
                success_count INTEGER NOT NULL DEFAULT 0,
                fail_count INTEGER NOT NULL DEFAULT 0,
                last_used_at TEXT,
                last_error TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY,
                session_pool_id TEXT,
                status TEXT NOT NULL DEFAULT 'queued',
                model TEXT NOT NULL DEFAULT 'jimeng-video-seedance-2.0',
                prompt TEXT NOT NULL,
                duration INTEGER NOT NULL DEFAULT 4,
                ratio TEXT NOT NULL DEFAULT '9:16',
                history_record_id TEXT,
                queue_position INTEGER,
                queue_total INTEGER,
                queue_eta TEXT,
                video_url TEXT,
                error_message TEXT,
                error_kind TEXT,
                request_body BLOB,
                request_content_type TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                started_at TEXT,
                finished_at TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
            CREATE INDEX IF NOT EXISTS idx_tasks_created ON tasks(created_at DESC);

            CREATE TABLE IF NOT EXISTS admin_users (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                email TEXT,
                auth_hash BLOB NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS tower_sessions (
                id TEXT PRIMARY KEY NOT NULL,
                data BLOB NOT NULL,
                expiry_date INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS api_keys (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                key_hash TEXT NOT NULL UNIQUE,
                key_prefix TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                expires_at TEXT,
                rate_limit INTEGER NOT NULL DEFAULT 60,
                daily_quota INTEGER NOT NULL DEFAULT 0,
                scopes TEXT NOT NULL DEFAULT '["video:create","task:read","task:cancel"]',
                metadata TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                last_used_at TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_api_keys_hash ON api_keys(key_hash);

            CREATE TABLE IF NOT EXISTS usage_daily (
                id TEXT PRIMARY KEY,
                api_key_id TEXT NOT NULL REFERENCES api_keys(id),
                date TEXT NOT NULL,
                request_count INTEGER NOT NULL DEFAULT 0,
                task_count INTEGER NOT NULL DEFAULT 0,
                UNIQUE(api_key_id, date)
            );
            CREATE INDEX IF NOT EXISTS idx_usage_daily_key_date ON usage_daily(api_key_id, date);
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Incremental column migrations (idempotent)
        let alter_columns = [
            "ALTER TABLE tasks ADD COLUMN request_content_type TEXT",
            "ALTER TABLE tasks ADD COLUMN api_key_id TEXT",
        ];
        for sql in &alter_columns {
            if let Err(err) = sqlx::query(sql).execute(&self.pool).await {
                if !err.to_string().contains("duplicate column name") {
                    return Err(err.into());
                }
            }
        }

        tracing::info!("Database migrated successfully");
        Ok(())
    }
}
