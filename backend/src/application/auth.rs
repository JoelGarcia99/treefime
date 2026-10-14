use std::time::Duration;

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand_core::{OsRng, RngCore};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::application::error::AppError as ApiError;

const ACCESS_TTL: Duration = Duration::from_secs(15 * 60);
const REFRESH_TTL_DAYS: i64 = 30;

#[derive(Clone)]
pub struct AuthState {
    pub database: DatabaseConnection,
    pub secure_cookies: bool,
}

#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub id: Uuid,
    pub session_id: Option<Uuid>,
    pub email: String,
    pub display_name: String,
    pub is_root: bool,
    pub must_update_credentials: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UserResponse {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
    pub is_root: bool,
    pub must_update_credentials: bool,
}

impl From<AuthenticatedUser> for UserResponse {
    fn from(user: AuthenticatedUser) -> Self {
        Self {
            id: user.id,
            email: user.email,
            display_name: user.display_name,
            is_root: user.is_root,
            must_update_credentials: user.must_update_credentials,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
    #[schema(example = "My laptop")]
    pub device_name: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ProfileUpdateRequest {
    pub email: String,
    pub display_name: String,
    #[schema(min_length = 12, max_length = 128)]
    pub new_password: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AuthResponse {
    pub user: UserResponse,
    pub csrf_token: String,
    pub access_expires_in_seconds: u64,
    pub refresh_expires_in_days: i64,
}

pub struct IssuedTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub response: AuthResponse,
}

#[derive(Serialize, ToSchema)]
pub struct CsrfResponse {
    pub csrf_token: String,
}

impl AuthState {
    pub async fn seed_root(&self) -> Result<(), ApiError> {
        let transaction = self.database.begin().await.map_err(ApiError::database)?;
        transaction
            .query_one(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT pg_advisory_xact_lock(72819431)".to_owned(),
            ))
            .await
            .map_err(ApiError::database)?;

        let result = transaction
            .query_one(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT EXISTS (SELECT 1 FROM users) AS has_users".to_owned(),
            ))
            .await
            .map_err(ApiError::database)?
            .expect("EXISTS always returns one row");
        let has_users: bool = result
            .try_get("", "has_users")
            .map_err(ApiError::database)?;
        if has_users {
            transaction.commit().await.map_err(ApiError::database)?;
            return Ok(());
        }

        let bootstrap_email = format!("root-{}@treefime.invalid", generate_token()?);
        let bootstrap_password = generate_token()?;
        let password_hash = hash_password(bootstrap_password.clone()).await?;
        transaction
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "INSERT INTO users (email, password_hash, display_name, is_root, must_update_credentials) VALUES ($1, $2, $3, TRUE, TRUE)",
                [bootstrap_email.clone().into(), password_hash.into(), "System root".to_owned().into()],
            ))
            .await
            .map_err(ApiError::database)?;
        transaction
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "INSERT INTO user_system_roles (user_id, role_id) SELECT id, (SELECT id FROM system_roles WHERE key = 'platform_workspace_manager') FROM users WHERE LOWER(email) = LOWER($1) ON CONFLICT DO NOTHING",
                [bootstrap_email.clone().into()],
            ))
            .await
            .map_err(ApiError::database)?;
        transaction.commit().await.map_err(ApiError::database)?;

        println!(
            "\nTreefime first-run root credentials\n  email: {bootstrap_email}\n  password: {bootstrap_password}\nSign in and immediately set a permanent email and password. This secret is printed only on first creation.\n"
        );
        Ok(())
    }

    pub async fn login(
        &self,
        request: LoginRequest,
        user_agent: Option<String>,
    ) -> Result<IssuedTokens, ApiError> {
        let email = normalize_email(&request.email)?;
        if request.password.len() > 128 {
            return Err(ApiError::unauthorized());
        }
        let row = self
            .database
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT id, email, display_name, password_hash, is_root, must_update_credentials FROM users WHERE LOWER(email) = LOWER($1) AND disabled_at IS NULL",
                [email.into()],
            ))
            .await
            .map_err(ApiError::database)?;
        let Some(row) = row else {
            return Err(ApiError::unauthorized());
        };

        let password_hash: String = row
            .try_get("", "password_hash")
            .map_err(ApiError::database)?;
        verify_password(password_hash, request.password).await?;
        let user = user_from_row(&row)?;
        let refresh_token = generate_token()?;
        let access_token = generate_token()?;
        let csrf_token = generate_token()?;
        let refresh_hash = token_hash(&refresh_token);
        let access_hash = token_hash(&access_token);
        let csrf_hash = token_hash(&csrf_token);
        let transaction = self.database.begin().await.map_err(ApiError::database)?;
        let session = transaction
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "INSERT INTO sessions (user_id, token_hash, access_token_hash, access_expires_at, csrf_token_hash, device_name, user_agent, expires_at) VALUES ($1, $2, $3, NOW() + INTERVAL '15 minutes', $4, $5, $6, NOW() + INTERVAL '30 days') RETURNING id",
                [
                    user.id.into(),
                    refresh_hash.clone().into(),
                    access_hash.into(),
                    csrf_hash.into(),
                    request.device_name.map(|name| name.chars().take(100).collect::<String>()).into(),
                    user_agent.into(),
                ],
            ))
            .await
            .map_err(ApiError::database)?
            .expect("INSERT ... RETURNING always returns one row");
        let session_id: Uuid = session.try_get("", "id").map_err(ApiError::database)?;
        transaction
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "INSERT INTO refresh_tokens (session_id, token_hash, expires_at) SELECT $1, $2, expires_at FROM sessions WHERE id = $1",
                [session_id.into(), refresh_hash.into()],
            ))
            .await
            .map_err(ApiError::database)?;
        transaction.commit().await.map_err(ApiError::database)?;

        Ok(IssuedTokens {
            access_token,
            refresh_token,
            response: AuthResponse {
                user: user.into(),
                csrf_token,
                access_expires_in_seconds: ACCESS_TTL.as_secs(),
                refresh_expires_in_days: REFRESH_TTL_DAYS,
            },
        })
    }

    pub async fn refresh(
        &self,
        old_token: &str,
        csrf_token: &str,
    ) -> Result<IssuedTokens, ApiError> {
        let old_hash = token_hash(old_token);
        let transaction = self.database.begin().await.map_err(ApiError::database)?;
        let row = transaction
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT rt.id AS refresh_id, rt.session_id, rt.consumed_at IS NOT NULL AS consumed, rt.expires_at > NOW() AS refresh_valid, s.expires_at > NOW() AS session_valid, s.revoked_at IS NOT NULL AS revoked, s.csrf_token_hash, u.id AS user_id, u.email, u.display_name, u.is_root, u.must_update_credentials, u.disabled_at IS NOT NULL AS disabled FROM refresh_tokens rt JOIN sessions s ON s.id = rt.session_id JOIN users u ON u.id = s.user_id WHERE rt.token_hash = $1 FOR UPDATE OF rt, s",
                [old_hash.clone().into()],
            ))
            .await
            .map_err(ApiError::database)?;
        let Some(row) = row else {
            transaction.rollback().await.map_err(ApiError::database)?;
            return Err(ApiError::unauthorized());
        };
        let session_id: Uuid = row.try_get("", "session_id").map_err(ApiError::database)?;
        let consumed: bool = row.try_get("", "consumed").map_err(ApiError::database)?;
        let revoked: bool = row.try_get("", "revoked").map_err(ApiError::database)?;
        let refresh_valid: bool = row
            .try_get("", "refresh_valid")
            .map_err(ApiError::database)?;
        let session_valid: bool = row
            .try_get("", "session_valid")
            .map_err(ApiError::database)?;
        let disabled: bool = row.try_get("", "disabled").map_err(ApiError::database)?;
        let expected_csrf: Vec<u8> = row
            .try_get("", "csrf_token_hash")
            .map_err(ApiError::database)?;

        if consumed {
            transaction
                .execute(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "UPDATE sessions SET revoked_at = COALESCE(revoked_at, NOW()) WHERE id = $1",
                    [session_id.into()],
                ))
                .await
                .map_err(ApiError::database)?;
            transaction.commit().await.map_err(ApiError::database)?;
            return Err(ApiError::unauthorized());
        }
        if expected_csrf.ct_eq(&token_hash(csrf_token)).unwrap_u8() != 1 {
            transaction.rollback().await.map_err(ApiError::database)?;
            return Err(ApiError::forbidden("Invalid CSRF token."));
        }
        if revoked || !refresh_valid || !session_valid || disabled {
            transaction.rollback().await.map_err(ApiError::database)?;
            return Err(ApiError::unauthorized());
        }

        let old_refresh_id: Uuid = row.try_get("", "refresh_id").map_err(ApiError::database)?;
        let user = user_from_row(&row)?;
        let refresh_token = generate_token()?;
        let access_token = generate_token()?;
        let csrf_token = generate_token()?;
        let refresh_hash = token_hash(&refresh_token);
        let access_hash = token_hash(&access_token);
        let csrf_hash = token_hash(&csrf_token);
        let new_refresh = transaction
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "INSERT INTO refresh_tokens (session_id, token_hash, expires_at) SELECT $1, $2, expires_at FROM sessions WHERE id = $1 RETURNING id",
                [session_id.into(), refresh_hash.clone().into()],
            ))
            .await
            .map_err(ApiError::database)?
            .expect("INSERT ... RETURNING always returns one row");
        let new_refresh_id: Uuid = new_refresh.try_get("", "id").map_err(ApiError::database)?;
        transaction
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "UPDATE refresh_tokens SET consumed_at = NOW(), replaced_by_id = $2 WHERE id = $1",
                [old_refresh_id.into(), new_refresh_id.into()],
            ))
            .await
            .map_err(ApiError::database)?;
        transaction
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "UPDATE sessions SET token_hash = $2, access_token_hash = $3, csrf_token_hash = $4, access_expires_at = NOW() + INTERVAL '15 minutes', last_seen_at = NOW() WHERE id = $1",
                [session_id.into(), refresh_hash.into(), access_hash.into(), csrf_hash.into()],
            ))
            .await
            .map_err(ApiError::database)?;
        transaction.commit().await.map_err(ApiError::database)?;

        Ok(IssuedTokens {
            access_token,
            refresh_token,
            response: AuthResponse {
                user: user.into(),
                csrf_token,
                access_expires_in_seconds: ACCESS_TTL.as_secs(),
                refresh_expires_in_days: REFRESH_TTL_DAYS,
            },
        })
    }

    pub async fn update_profile(
        &self,
        user: &AuthenticatedUser,
        request: ProfileUpdateRequest,
    ) -> Result<UserResponse, ApiError> {
        let email = normalize_email(&request.email)?;
        let display_name = request.display_name.trim();
        if display_name.is_empty() || display_name.len() > 80 {
            return Err(ApiError::bad_request(
                "Display name must be between 1 and 80 characters.",
            ));
        }
        if user.must_update_credentials && request.new_password.is_none() {
            return Err(ApiError::bad_request(
                "The first-run root account must set a new password and email before continuing.",
            ));
        }
        if user.must_update_credentials && email == user.email.to_lowercase() {
            return Err(ApiError::bad_request(
                "The first-run root account must use a different email address.",
            ));
        }
        if user.must_update_credentials {
            let current_hash = self
                .database
                .query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "SELECT password_hash FROM users WHERE id = $1",
                    [user.id.into()],
                ))
                .await
                .map_err(ApiError::database)?
                .ok_or_else(ApiError::unauthorized)?
                .try_get::<String>("", "password_hash")
                .map_err(ApiError::database)?;
            let proposed_password = request
                .new_password
                .as_ref()
                .expect("checked above")
                .clone();
            match verify_password(current_hash, proposed_password).await {
                Ok(()) => {
                    return Err(ApiError::bad_request(
                        "The first-run root account must choose a different password.",
                    ));
                }
                Err(ApiError::Unauthorized) => {}
                Err(error) => return Err(error),
            }
        }
        let password_hash = match request.new_password {
            Some(password) => Some(hash_password(password).await?),
            None => None,
        };
        let password_changed = password_hash.is_some();

        let email_taken = self
            .database
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT EXISTS (SELECT 1 FROM users WHERE LOWER(email) = LOWER($1) AND id <> $2) AS taken",
                [email.clone().into(), user.id.into()],
            ))
            .await
            .map_err(ApiError::database)?
            .expect("EXISTS always returns one row")
            .try_get::<bool>("", "taken")
            .map_err(ApiError::database)?;
        if email_taken {
            return Err(ApiError::conflict("That email address is already in use."));
        }

        let row = self
            .database
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "UPDATE users SET email = $1, display_name = $2, password_hash = COALESCE($3, password_hash), must_update_credentials = FALSE, updated_at = NOW() WHERE id = $4 AND disabled_at IS NULL RETURNING id, email, display_name, is_root, must_update_credentials",
                [email.into(), display_name.to_owned().into(), password_hash.into(), user.id.into()],
            ))
            .await
            .map_err(ApiError::database)?
            .ok_or_else(ApiError::unauthorized)?;
        if password_changed {
            if let Some(session_id) = user.session_id {
                self.database
                    .execute(Statement::from_sql_and_values(
                        DatabaseBackend::Postgres,
                        "UPDATE sessions SET revoked_at = COALESCE(revoked_at, NOW()) WHERE user_id = $1 AND id <> $2 AND revoked_at IS NULL",
                        [user.id.into(), session_id.into()],
                    ))
                    .await
                    .map_err(ApiError::database)?;
            }
        }
        Ok(user_from_row(&row)?.into())
    }

    pub async fn authenticate(&self, access_token: &str) -> Result<AuthenticatedUser, ApiError> {
        let access_hash = token_hash(access_token);
        let row = self
            .database
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT s.id AS session_id, u.id, u.email, u.display_name, u.is_root, u.must_update_credentials FROM sessions s JOIN users u ON u.id = s.user_id WHERE s.access_token_hash = $1 AND s.access_expires_at > NOW() AND s.expires_at > NOW() AND s.revoked_at IS NULL AND u.disabled_at IS NULL",
                [access_hash.into()],
            ))
            .await
            .map_err(ApiError::database)?
            .ok_or_else(ApiError::unauthorized)?;
        user_from_row(&row)
    }

    pub async fn verify_session_csrf(
        &self,
        session_id: Uuid,
        csrf_token: &str,
    ) -> Result<(), ApiError> {
        let expected = self
            .database
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT csrf_token_hash FROM sessions WHERE id = $1 AND revoked_at IS NULL",
                [session_id.into()],
            ))
            .await
            .map_err(ApiError::database)?
            .ok_or_else(ApiError::unauthorized)?
            .try_get::<Vec<u8>>("", "csrf_token_hash")
            .map_err(ApiError::database)?;
        if expected.ct_eq(&token_hash(csrf_token)).unwrap_u8() != 1 {
            return Err(ApiError::forbidden("Invalid CSRF token."));
        }
        Ok(())
    }

    pub async fn revoke_from_cookies(
        &self,
        access: Option<&str>,
        refresh: Option<&str>,
    ) -> Result<(), ApiError> {
        let transaction = self.database.begin().await.map_err(ApiError::database)?;
        if let Some(token) = refresh {
            transaction
                .execute(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "UPDATE sessions SET revoked_at = COALESCE(revoked_at, NOW()) WHERE id IN (SELECT session_id FROM refresh_tokens WHERE token_hash = $1)",
                    [token_hash(token).into()],
                ))
                .await
                .map_err(ApiError::database)?;
        }
        if let Some(token) = access {
            transaction
                .execute(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "UPDATE sessions SET revoked_at = COALESCE(revoked_at, NOW()) WHERE access_token_hash = $1",
                    [token_hash(token).into()],
                ))
                .await
                .map_err(ApiError::database)?;
        }
        transaction.commit().await.map_err(ApiError::database)
    }

    pub async fn verify_cookie_session_csrf(
        &self,
        access: Option<&str>,
        refresh: Option<&str>,
        csrf_token: &str,
    ) -> Result<(), ApiError> {
        let row = self
            .database
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT s.csrf_token_hash FROM sessions s WHERE ($1::BYTEA IS NOT NULL AND s.access_token_hash = $1) OR ($2::BYTEA IS NOT NULL AND s.id IN (SELECT session_id FROM refresh_tokens WHERE token_hash = $2)) LIMIT 1",
                [access.map(token_hash).into(), refresh.map(token_hash).into()],
            ))
            .await
            .map_err(ApiError::database)?;
        if let Some(row) = row {
            let expected: Vec<u8> = row
                .try_get("", "csrf_token_hash")
                .map_err(ApiError::database)?;
            if expected.ct_eq(&token_hash(csrf_token)).unwrap_u8() != 1 {
                return Err(ApiError::forbidden("Invalid CSRF token."));
            }
        }
        Ok(())
    }
}

pub fn generate_csrf_token() -> Result<String, ApiError> {
    generate_token()
}

pub fn token_from_cookie_header<'a>(
    cookie_header: Option<&'a str>,
    cookie_name: &str,
) -> Option<&'a str> {
    cookie_header?.split(';').find_map(|part| {
        let (name, value) = part.trim().split_once('=')?;
        (name == cookie_name).then_some(value)
    })
}

fn user_from_row(row: &sea_orm::QueryResult) -> Result<AuthenticatedUser, ApiError> {
    Ok(AuthenticatedUser {
        id: row
            .try_get("", "id")
            .or_else(|_| row.try_get("", "user_id"))
            .map_err(ApiError::database)?,
        session_id: row.try_get("", "session_id").ok(),
        email: row.try_get("", "email").map_err(ApiError::database)?,
        display_name: row
            .try_get("", "display_name")
            .map_err(ApiError::database)?,
        is_root: row.try_get("", "is_root").map_err(ApiError::database)?,
        must_update_credentials: row
            .try_get("", "must_update_credentials")
            .map_err(ApiError::database)?,
    })
}

pub(crate) fn normalize_email(email: &str) -> Result<String, ApiError> {
    let normalized = email.trim().to_lowercase();
    let mut parts = normalized.split('@');
    let valid = normalized.len() <= 320
        && parts
            .next()
            .is_some_and(|local| !local.is_empty() && local.len() <= 64)
        && parts.next().is_some_and(|domain| {
            domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
        })
        && parts.next().is_none();
    if !valid {
        return Err(ApiError::bad_request("Enter a valid email address."));
    }
    Ok(normalized)
}

pub(crate) async fn hash_password(password: String) -> Result<String, ApiError> {
    if password.len() < 12 || password.len() > 128 {
        return Err(ApiError::bad_request(
            "Password must be between 12 and 128 bytes.",
        ));
    }
    tokio::task::spawn_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|hash| hash.to_string())
            .map_err(|_| ApiError::internal())
    })
    .await
    .map_err(|_| ApiError::internal())?
}

pub(crate) async fn verify_password(
    encoded_hash: String,
    password: String,
) -> Result<(), ApiError> {
    tokio::task::spawn_blocking(move || {
        let parsed_hash = PasswordHash::new(&encoded_hash).map_err(|_| ApiError::internal())?;
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed_hash)
            .map_err(|_| ApiError::unauthorized())
    })
    .await
    .map_err(|_| ApiError::internal())?
}

pub(crate) fn generate_token() -> Result<String, ApiError> {
    let mut bytes = [0_u8; 32];
    OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| ApiError::internal())?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

pub(crate) fn token_hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}
