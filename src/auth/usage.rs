use anyhow::Result;
use sqlx::SqlitePool;

/// Increment request_count for today. Called on every authenticated request.
pub async fn record_request(db: &SqlitePool, api_key_id: &str) {
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let id = format!("{}_{}", api_key_id, today);

    let _ = sqlx::query(
        "INSERT INTO usage_daily (id, api_key_id, date, request_count, task_count) \
         VALUES (?, ?, ?, 1, 0) \
         ON CONFLICT(api_key_id, date) DO UPDATE SET request_count = request_count + 1",
    )
    .bind(&id)
    .bind(api_key_id)
    .bind(&today)
    .execute(db)
    .await;
}

/// Increment task_count for today. Called when a task is created.
pub async fn record_task(db: &SqlitePool, api_key_id: &str) {
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let id = format!("{}_{}", api_key_id, today);

    let _ = sqlx::query(
        "INSERT INTO usage_daily (id, api_key_id, date, request_count, task_count) \
         VALUES (?, ?, ?, 0, 1) \
         ON CONFLICT(api_key_id, date) DO UPDATE SET task_count = task_count + 1",
    )
    .bind(&id)
    .bind(api_key_id)
    .bind(&today)
    .execute(db)
    .await;
}

/// Get today's task count for a key (for daily quota check).
pub async fn today_task_count(db: &SqlitePool, api_key_id: &str) -> Result<i32> {
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

    let row: Option<(i32,)> = sqlx::query_as(
        "SELECT task_count FROM usage_daily WHERE api_key_id = ? AND date = ?",
    )
    .bind(api_key_id)
    .bind(&today)
    .fetch_optional(db)
    .await?;

    Ok(row.map(|r| r.0).unwrap_or(0))
}

/// Get today's request + task counts for a key.
pub async fn today_usage(db: &SqlitePool, api_key_id: &str) -> Result<(i32, i32)> {
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

    let row: Option<(i32, i32)> = sqlx::query_as(
        "SELECT request_count, task_count FROM usage_daily WHERE api_key_id = ? AND date = ?",
    )
    .bind(api_key_id)
    .bind(&today)
    .fetch_optional(db)
    .await?;

    Ok(row.unwrap_or((0, 0)))
}

/// Usage row for queries.
#[derive(Debug, serde::Serialize)]
pub struct UsageRow {
    pub date: String,
    pub api_key_id: String,
    pub api_key_name: String,
    pub request_count: i32,
    pub task_count: i32,
}

/// Query usage by date range, optionally filtered by key_id.
pub async fn query_usage(
    db: &SqlitePool,
    key_id: Option<&str>,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<Vec<UsageRow>> {
    let mut sql = String::from(
        "SELECT u.date, u.api_key_id, k.name AS api_key_name, u.request_count, u.task_count \
         FROM usage_daily u JOIN api_keys k ON u.api_key_id = k.id WHERE 1=1",
    );
    let mut binds: Vec<String> = Vec::new();

    if let Some(kid) = key_id {
        sql.push_str(" AND u.api_key_id = ?");
        binds.push(kid.to_string());
    }
    if let Some(f) = from {
        sql.push_str(" AND u.date >= ?");
        binds.push(f.to_string());
    }
    if let Some(t) = to {
        sql.push_str(" AND u.date <= ?");
        binds.push(t.to_string());
    }
    sql.push_str(" ORDER BY u.date DESC, k.name");

    let mut query = sqlx::query_as::<_, UsageQueryRow>(&sql);
    for b in &binds {
        query = query.bind(b);
    }

    let rows = query.fetch_all(db).await?;
    Ok(rows.into_iter().map(|r| UsageRow {
        date: r.date,
        api_key_id: r.api_key_id,
        api_key_name: r.api_key_name,
        request_count: r.request_count,
        task_count: r.task_count,
    }).collect())
}

#[derive(sqlx::FromRow)]
struct UsageQueryRow {
    date: String,
    api_key_id: String,
    api_key_name: String,
    request_count: i32,
    task_count: i32,
}

/// Summary across all keys for a date range.
pub async fn usage_summary(
    db: &SqlitePool,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<Vec<UsageSummaryRow>> {
    let mut sql = String::from(
        "SELECT k.id AS api_key_id, k.name AS api_key_name, \
         COALESCE(SUM(u.request_count), 0) AS total_requests, \
         COALESCE(SUM(u.task_count), 0) AS total_tasks \
         FROM api_keys k LEFT JOIN usage_daily u ON k.id = u.api_key_id \
         WHERE 1=1",
    );
    let mut binds: Vec<String> = Vec::new();

    if let Some(f) = from {
        sql.push_str(" AND (u.date >= ? OR u.date IS NULL)");
        binds.push(f.to_string());
    }
    if let Some(t) = to {
        sql.push_str(" AND (u.date <= ? OR u.date IS NULL)");
        binds.push(t.to_string());
    }
    sql.push_str(" GROUP BY k.id ORDER BY k.name");

    let mut query = sqlx::query_as::<_, SummaryQueryRow>(&sql);
    for b in &binds {
        query = query.bind(b);
    }

    let rows = query.fetch_all(db).await?;
    Ok(rows.into_iter().map(|r| UsageSummaryRow {
        api_key_id: r.api_key_id,
        api_key_name: r.api_key_name,
        total_requests: r.total_requests,
        total_tasks: r.total_tasks,
    }).collect())
}

#[derive(Debug, serde::Serialize)]
pub struct UsageSummaryRow {
    pub api_key_id: String,
    pub api_key_name: String,
    pub total_requests: i32,
    pub total_tasks: i32,
}

#[derive(sqlx::FromRow)]
struct SummaryQueryRow {
    api_key_id: String,
    api_key_name: String,
    total_requests: i32,
    total_tasks: i32,
}
