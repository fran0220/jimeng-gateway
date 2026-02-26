use axum_login::{AuthUser, AuthnBackend, UserId};
use serde::{Deserialize, Serialize};
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

/// Credentials placeholder (OIDC exchange happens in route handler, not authenticate())
#[derive(Debug, Clone)]
pub struct OidcCredentials;

/// Auth backend that validates OIDC codes and manages admin users
#[derive(Debug, Clone)]
pub struct OidcBackend {
    pub db: SqlitePool,
}

impl OidcBackend {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }
}

impl AuthnBackend for OidcBackend {
    type User = AdminUser;
    type Credentials = OidcCredentials;
    type Error = std::convert::Infallible;

    async fn authenticate(
        &self,
        _creds: Self::Credentials,
    ) -> Result<Option<Self::User>, Self::Error> {
        // OIDC token exchange is handled in the /auth/callback route handler.
        // The callback calls auth_session.login() directly after exchanging the code.
        Ok(None)
    }

    async fn get_user(&self, user_id: &UserId<Self>) -> Result<Option<Self::User>, Self::Error> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, name, email, auth_hash FROM admin_users WHERE id = ?",
        )
        .bind(user_id)
        .fetch_optional(&self.db)
        .await
        .unwrap_or(None);

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

pub type AuthSession = axum_login::AuthSession<OidcBackend>;
