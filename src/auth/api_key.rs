use anyhow::Result;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

/// API Key format: gw_<32 hex chars> (35 chars total)
const KEY_PREFIX: &str = "gw_";

/// Generate a new random API key.
pub fn generate_key() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    let bytes: [u8; 16] = rng.random();
    format!("{}{}", KEY_PREFIX, hex::encode(bytes))
}

/// SHA256 hash of a raw API key (for storage / lookup).
pub fn hash_key(raw: &str) -> String {
    let digest = Sha256::digest(raw.as_bytes());
    hex::encode(digest)
}

/// Extract the display prefix from a raw key (first 8 chars, e.g. "gw_a1b2").
pub fn key_prefix(raw: &str) -> String {
    raw.chars().take(8).collect()
}

/// Validate key format: gw_ + 32 hex chars = 35 chars total.
#[allow(dead_code)]
pub fn is_valid_format(raw: &str) -> bool {
    raw.starts_with(KEY_PREFIX) && raw.len() == 35
}

/// Stored API key record.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ApiKeyRecord {
    pub id: String,
    pub name: String,
    pub key_prefix: String,
    pub enabled: bool,
    pub expires_at: Option<String>,
    pub rate_limit: i32,
    pub daily_quota: i32,
    pub scopes: Vec<String>,
    pub metadata: serde_json::Value,
    pub created_at: String,
    pub last_used_at: Option<String>,
}

/// Row from SQLite.
#[derive(sqlx::FromRow)]
struct ApiKeyRow {
    id: String,
    name: String,
    key_prefix: String,
    enabled: bool,
    expires_at: Option<String>,
    rate_limit: i32,
    daily_quota: i32,
    scopes: String,
    metadata: String,
    created_at: String,
    last_used_at: Option<String>,
}

impl From<ApiKeyRow> for ApiKeyRecord {
    fn from(row: ApiKeyRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            key_prefix: row.key_prefix,
            enabled: row.enabled,
            expires_at: row.expires_at,
            rate_limit: row.rate_limit,
            daily_quota: row.daily_quota,
            scopes: serde_json::from_str(&row.scopes).unwrap_or_default(),
            metadata: serde_json::from_str(&row.metadata).unwrap_or(serde_json::Value::Object(Default::default())),
            created_at: row.created_at,
            last_used_at: row.last_used_at,
        }
    }
}

/// Look up an API key by its SHA256 hash. Returns None if not found.
pub async fn lookup_by_hash(db: &SqlitePool, key_hash: &str) -> Result<Option<ApiKeyRecord>> {
    let row = sqlx::query_as::<_, ApiKeyRow>(
        "SELECT id, name, key_prefix, enabled, expires_at, rate_limit, daily_quota, scopes, metadata, created_at, last_used_at \
         FROM api_keys WHERE key_hash = ?",
    )
    .bind(key_hash)
    .fetch_optional(db)
    .await?;

    Ok(row.map(ApiKeyRecord::from))
}

/// Update last_used_at for a key.
pub async fn touch(db: &SqlitePool, key_id: &str) {
    let _ = sqlx::query("UPDATE api_keys SET last_used_at = datetime('now') WHERE id = ?")
        .bind(key_id)
        .execute(db)
        .await;
}

/// Create a new API key. Returns (raw_key, record).
pub async fn create(
    db: &SqlitePool,
    name: &str,
    rate_limit: i32,
    daily_quota: i32,
    scopes: &[String],
    expires_at: Option<&str>,
    metadata: &serde_json::Value,
) -> Result<(String, ApiKeyRecord)> {
    let raw_key = generate_key();
    let id = uuid::Uuid::new_v4().to_string();
    let hash = hash_key(&raw_key);
    let prefix = key_prefix(&raw_key);
    let scopes_json = serde_json::to_string(scopes)?;
    let metadata_json = serde_json::to_string(metadata)?;

    sqlx::query(
        "INSERT INTO api_keys (id, name, key_hash, key_prefix, enabled, expires_at, rate_limit, daily_quota, scopes, metadata) \
         VALUES (?, ?, ?, ?, 1, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(name)
    .bind(&hash)
    .bind(&prefix)
    .bind(expires_at)
    .bind(rate_limit)
    .bind(daily_quota)
    .bind(&scopes_json)
    .bind(&metadata_json)
    .execute(db)
    .await?;

    let record = ApiKeyRecord {
        id,
        name: name.to_string(),
        key_prefix: prefix,
        enabled: true,
        expires_at: expires_at.map(String::from),
        rate_limit,
        daily_quota,
        scopes: scopes.to_vec(),
        metadata: metadata.clone(),
        created_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        last_used_at: None,
    };

    Ok((raw_key, record))
}

/// List all API keys.
pub async fn list_all(db: &SqlitePool) -> Result<Vec<ApiKeyRecord>> {
    let rows = sqlx::query_as::<_, ApiKeyRow>(
        "SELECT id, name, key_prefix, enabled, expires_at, rate_limit, daily_quota, scopes, metadata, created_at, last_used_at \
         FROM api_keys ORDER BY created_at DESC",
    )
    .fetch_all(db)
    .await?;

    Ok(rows.into_iter().map(ApiKeyRecord::from).collect())
}

/// Get a single API key by ID.
pub async fn get_by_id(db: &SqlitePool, id: &str) -> Result<Option<ApiKeyRecord>> {
    let row = sqlx::query_as::<_, ApiKeyRow>(
        "SELECT id, name, key_prefix, enabled, expires_at, rate_limit, daily_quota, scopes, metadata, created_at, last_used_at \
         FROM api_keys WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(db)
    .await?;

    Ok(row.map(ApiKeyRecord::from))
}

/// Update a key's mutable fields.
pub struct UpdateApiKey {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub rate_limit: Option<i32>,
    pub daily_quota: Option<i32>,
    pub scopes: Option<Vec<String>>,
    pub expires_at: Option<Option<String>>,
    pub metadata: Option<serde_json::Value>,
}

pub async fn update(db: &SqlitePool, id: &str, patch: &UpdateApiKey) -> Result<bool> {
    let mut sets = Vec::new();
    let mut binds: Vec<String> = Vec::new();

    if let Some(ref name) = patch.name {
        sets.push("name = ?");
        binds.push(name.clone());
    }
    if let Some(enabled) = patch.enabled {
        sets.push("enabled = ?");
        binds.push(if enabled { "1".into() } else { "0".into() });
    }
    if let Some(rl) = patch.rate_limit {
        sets.push("rate_limit = ?");
        binds.push(rl.to_string());
    }
    if let Some(dq) = patch.daily_quota {
        sets.push("daily_quota = ?");
        binds.push(dq.to_string());
    }
    if let Some(ref scopes) = patch.scopes {
        sets.push("scopes = ?");
        binds.push(serde_json::to_string(scopes).unwrap_or_default());
    }
    if let Some(ref ea) = patch.expires_at {
        sets.push("expires_at = ?");
        binds.push(ea.as_deref().unwrap_or("").to_string());
    }
    if let Some(ref meta) = patch.metadata {
        sets.push("metadata = ?");
        binds.push(serde_json::to_string(meta).unwrap_or_default());
    }

    if sets.is_empty() {
        return Ok(false);
    }

    let sql = format!("UPDATE api_keys SET {} WHERE id = ?", sets.join(", "));
    let mut query = sqlx::query(&sql);
    for b in &binds {
        query = query.bind(b);
    }
    query = query.bind(id);

    let result = query.execute(db).await?;
    Ok(result.rows_affected() > 0)
}

/// Delete a key.
pub async fn delete(db: &SqlitePool, id: &str) -> Result<bool> {
    let result = sqlx::query("DELETE FROM api_keys WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Regenerate a key (new raw key + hash, same ID and settings).
pub async fn regenerate(db: &SqlitePool, id: &str) -> Result<Option<String>> {
    let raw_key = generate_key();
    let hash = hash_key(&raw_key);
    let prefix = key_prefix(&raw_key);

    let result = sqlx::query(
        "UPDATE api_keys SET key_hash = ?, key_prefix = ?, last_used_at = NULL WHERE id = ?",
    )
    .bind(&hash)
    .bind(&prefix)
    .bind(id)
    .execute(db)
    .await?;

    if result.rows_affected() > 0 {
        Ok(Some(raw_key))
    } else {
        Ok(None)
    }
}
