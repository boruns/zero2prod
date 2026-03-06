use anyhow::Context;
use argon2::{Argon2, PasswordHash, PasswordVerifier};
use secrecy::{ExposeSecret, SecretBox};
use sqlx::PgPool;

use crate::telemetry::spawn_blocking_with_tracing;

#[derive(thiserror::Error, Debug)]
pub enum AuthError {
    #[error("Invalid Credentials")]
    InvalidCredentials(#[source] anyhow::Error),

    #[error(transparent)]
    UnexceptedError(#[from] anyhow::Error),
}

pub struct Credentials {
    pub username: String,
    pub password: SecretBox<String>,
}

#[tracing::instrument(skip(credentials, pool), name = "Validate credentials")]
pub async fn validate_credentials(
    credentials: Credentials,
    pool: &PgPool,
) -> Result<uuid::Uuid, AuthError> {
    let mut user_id = None;
    let mut expected_password_hash = SecretBox::new(
        Box::new(
            "$argon2id$v=19$m=19456,t=2,p=1$zATKil531Eh/DaM1sc9nwQ$1of3JB5MUB9X9B2uzLzxo43UInIydpirdS4r/TFB9xQ".to_string()
        )
    );

    if let Some((store_user_id, store_hash)) =
        get_stored_credentials(&credentials.username, pool).await?
    {
        user_id = Some(store_user_id);
        expected_password_hash = store_hash;
    };

    // 阻塞任务
    let _ = spawn_blocking_with_tracing(move || {
        verify_password_hash(expected_password_hash, credentials.password)
    })
    .await
    .context("Failed to span blocking stash")??;

    // some 判断
    user_id
        .ok_or_else(|| anyhow::anyhow!("Unknown username."))
        .map_err(AuthError::InvalidCredentials)
}

#[tracing::instrument(skip(username, pool), name = "Get stored credentials")]
pub async fn get_stored_credentials(
    username: &str,
    pool: &PgPool,
) -> Result<Option<(uuid::Uuid, SecretBox<String>)>, anyhow::Error> {
    let row: Option<(uuid::Uuid, String)> =
        sqlx::query_as("SELECT user_id, password_hash FROM users WHERE username = $1")
            .bind(username)
            .fetch_optional(pool)
            .await
            .context("Failed to perform a query to retrieve stored credentials.")?;
    let row = match row {
        Some(row) => (row.0, SecretBox::new(Box::new(row.1))),
        None => {
            return Err(anyhow::anyhow!("User credentials not found."));
        }
    };
    Ok(Some(row))
}

#[tracing::instrument(skip(expected_password_hash, password), name = "Verify password hash")]
fn verify_password_hash(
    expected_password_hash: SecretBox<String>,
    password: SecretBox<String>,
) -> Result<(), AuthError> {
    let expected_password_hash = PasswordHash::new(expected_password_hash.expose_secret())
        .context("Failed to parse hash in PHC string format.")?;
    Argon2::default()
        .verify_password(password.expose_secret().as_bytes(), &expected_password_hash)
        .context("Invalid password")
        .map_err(AuthError::InvalidCredentials)?;
    Ok(())
}
