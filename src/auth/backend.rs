use axum_login::{AuthUser, AuthnBackend, UserId};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use sqlx::SqlitePool;

/// Admin user stored in session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminUser {
    pub id: String,
    pub name: String,
    pub email: Option<String>,
    pub(crate) auth_hash: Vec<u8>,
}

impl AuthUser for AdminUser {
    type Id = String;

    fn id(&self) -> Self::Id {
        self.id.clone()
    }

    fn session_auth_hash(&self) -> &[u8] {
        &self.auth_hash
    }
}

/// Password-based credentials
#[derive(Debug, Clone)]
pub struct PasswordCredentials {
    pub username: String,
    pub password: String,
}

/// Auth backend that validates passwords against admin_users table
#[derive(Debug, Clone)]
pub struct PasswordBackend {
    pub db: SqlitePool,
}

impl PasswordBackend {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }
}

impl AuthnBackend for PasswordBackend {
    type User = AdminUser;
    type Credentials = PasswordCredentials;
    type Error = sqlx::Error;

    async fn authenticate(
        &self,
        creds: Self::Credentials,
    ) -> Result<Option<Self::User>, Self::Error> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, name, email, auth_hash FROM admin_users WHERE id = ?",
        )
        .bind(&creds.username)
        .fetch_optional(&self.db)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        let candidate = sha2::Sha256::digest(creds.password.as_bytes()).to_vec();

        if row.auth_hash == candidate {
            Ok(Some(AdminUser {
                id: row.id,
                name: row.name,
                email: row.email,
                auth_hash: row.auth_hash,
            }))
        } else {
            Ok(None)
        }
    }

    async fn get_user(&self, user_id: &UserId<Self>) -> Result<Option<Self::User>, Self::Error> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, name, email, auth_hash FROM admin_users WHERE id = ?",
        )
        .bind(user_id)
        .fetch_optional(&self.db)
        .await?;

        Ok(row.map(|r| AdminUser {
            id: r.id,
            name: r.name,
            email: r.email,
            auth_hash: r.auth_hash,
        }))
    }
}

#[derive(sqlx::FromRow)]
struct UserRow {
    id: String,
    name: String,
    email: Option<String>,
    auth_hash: Vec<u8>,
}

pub type AuthSession = axum_login::AuthSession<PasswordBackend>;
