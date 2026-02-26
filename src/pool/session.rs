use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SessionInfo {
    pub id: String,
    pub label: String,
    /// The actual jimeng session_id cookie value.
    /// Masked in API responses for security.
    pub session_id: String,
    pub enabled: bool,
    pub healthy: bool,
    pub active_tasks: i32,
    pub total_tasks: i32,
    pub success_count: i32,
    pub fail_count: i32,
    pub last_used_at: Option<String>,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl SessionInfo {
    /// Return a masked version for API responses (hide most of the session_id).
    pub fn masked(&self) -> Self {
        let masked_id = if self.session_id.len() > 8 {
            format!("{}...{}", &self.session_id[..8], &self.session_id[self.session_id.len() - 4..])
        } else {
            "****".to_string()
        };

        Self {
            session_id: masked_id,
            ..self.clone()
        }
    }
}
