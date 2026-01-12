use crate::{
    domain::SubscriberEmail, email_client::EmailClient, routes::error_chain_fmt,
    telemetry::spawn_blocking_with_tracing,
};
use actix_web::{
    HttpRequest, HttpResponse, ResponseError,
    http::header::{HeaderMap, HeaderValue},
    web,
};
use anyhow::{Context, anyhow};
use argon2::{Argon2, PasswordHash, PasswordVerifier};
use secrecy::{ExposeSecret, SecretBox};
use sqlx::{PgPool, Row};

#[derive(serde::Deserialize)]
pub struct BodyData {
    title: String,
    content: Content,
}

#[derive(serde::Deserialize)]
pub struct Content {
    html: String,
    text: String,
}

#[derive(thiserror::Error)]
pub enum PublishError {
    #[error("Authentication failed")]
    AuthError(#[source] anyhow::Error),
    #[error(transparent)]
    UnexpectedError(#[from] anyhow::Error),
}

impl std::fmt::Debug for PublishError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

impl ResponseError for PublishError {
    fn error_response(&self) -> HttpResponse<actix_web::body::BoxBody> {
        match self {
            PublishError::UnexpectedError(_) => {
                HttpResponse::new(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR)
            }
            PublishError::AuthError(_) => {
                let mut response = HttpResponse::new(actix_web::http::StatusCode::UNAUTHORIZED);
                let header_value = HeaderValue::from_str(r#"Basic realm="publish""#).unwrap();
                response
                    .headers_mut()
                    .insert(actix_web::http::header::WWW_AUTHENTICATE, header_value);
                response
            }
        }
    }
}

#[tracing::instrument(
    name = "Publish a newsletter issue",
    skip(body, pool, email_client, request),
    fields(
        username=tracing::field::Empty,
        user_id=tracing::field::Empty,
    )
)]
pub async fn publish_newsletter(
    body: web::Json<BodyData>,
    pool: web::Data<PgPool>,
    email_client: web::Data<EmailClient>,
    request: HttpRequest,
) -> Result<HttpResponse, PublishError> {
    let credentials = basic_authentication(request.headers()).map_err(PublishError::AuthError)?;
    tracing::Span::current().record("username", &tracing::field::display(&credentials.username));

    let user_id = validate_credentials(credentials, &pool).await?;
    tracing::Span::current().record("user_id", &tracing::field::display(&user_id));

    let subscribers = get_confirmed_subscribers(&pool).await?;
    for subscriber in subscribers {
        match subscriber {
            Ok(subscriber) => {
                email_client
                    .send_email(
                        &subscriber.email,
                        &body.title,
                        &body.content.html,
                        &body.content.text,
                    )
                    .await
                    .with_context(|| {
                        format!("Failed to send newsletter issue to {}", &subscriber.email)
                    })?;
            }
            Err(error) => {
                tracing::error!("Failed to parse subscriber email: {}", error.to_string());
            }
        }
    }
    Ok(HttpResponse::Ok().finish())
}

struct Credentials {
    username: String,
    password: SecretBox<String>,
}

fn basic_authentication(headers: &HeaderMap) -> Result<Credentials, anyhow::Error> {
    let authorization = headers
        .get("Authorization")
        .context("The 'Authorization' header was missing")?
        .to_str()
        .context("The 'Authorization' header was not a valid UTF8 string")?;

    let base64_encoded_header = authorization
        .strip_prefix("Basic ")
        .context("Authorization header was not basic")?;
    let decoded_bytes = base64::decode_config(base64_encoded_header, base64::STANDARD)
        .context("Failed to base64 decode 'Basic' header")?;
    let decoded_header = String::from_utf8(decoded_bytes)
        .context("Failed to decode 'Basic' header as UTF8 string")?;
    let (username, password) = decoded_header
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("Auth header not in the 'username:password' format"))?;
    Ok(Credentials {
        username: username.to_string(),
        password: SecretBox::new(Box::new(password.to_string())),
    })
}

struct ConfirmedSubscriber {
    pub email: SubscriberEmail,
}

#[tracing::instrument(skip(pool), name = "Get confirmed subscribers")]
async fn get_confirmed_subscribers(
    pool: &PgPool,
) -> Result<Vec<Result<ConfirmedSubscriber, anyhow::Error>>, anyhow::Error> {
    let confirmed_subscribers =
        sqlx::query("SELECT email FROM subscriptions WHERE status = 'confirmed'")
            .fetch_all(pool)
            .await?
            .into_iter()
            .map(|row| match SubscriberEmail::parse(row.get(0)) {
                Ok(email) => Ok(ConfirmedSubscriber { email }),
                Err(error) => Err(anyhow!(error)),
            })
            .collect();
    Ok(confirmed_subscribers)
}

#[tracing::instrument(skip(credentials, pool), name = "Validate credentials")]
async fn validate_credentials(
    credentials: Credentials,
    pool: &PgPool,
) -> Result<uuid::Uuid, PublishError> {
    let (user_id, store_hash) = get_stored_credentials(&credentials.username, pool)
        .await
        .map_err(PublishError::AuthError)?
        .ok_or_else(|| PublishError::AuthError(anyhow::anyhow!("Invalid credentials.")))?;

    // 阻塞任务
    // 假设 verify_password_hash 返回了 PublishError::AuthError
    let _ = spawn_blocking_with_tracing(move || {
        // verify_password_hash 内部验证失败，返回 PublishError::AuthError
        verify_password_hash(store_hash, credentials.password) // 返回 Err(PublishError::AuthError(...))
    })
    .await // 现在得到 Err(PublishError::AuthError(...))
    .context("Failed to span blocking task") // 这里对 Err 添加上下文
    .map_err(PublishError::UnexpectedError)??;

    Ok(user_id)
}

#[tracing::instrument(skip(username, pool), name = "Get stored credentials")]
async fn get_stored_credentials(
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
) -> Result<(), PublishError> {
    let expected_password_hash = PasswordHash::new(expected_password_hash.expose_secret())
        .context("Failed to parse hash in PHC string format.")
        .map_err(PublishError::UnexpectedError)?;
    Argon2::default()
        .verify_password(password.expose_secret().as_bytes(), &expected_password_hash)
        .context("Invalid password")
        .map_err(PublishError::AuthError)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn verify_password_hash_failed_with_invalid_password() {
        let excepted_password_hash = SecretBox::new(Box::new(
            "$argon2id$v=19$m=19456,t=2,p=1$zATKil531Eh/DaM1sc9nwQ$1of3JB5MUB9X9B2uzLzxo43UInIydpirdS4r/TFB9xQ".to_string(),
        ));
        let password = SecretBox::new(Box::new("15385a99-8270-4e16-a223-04f4ca78469a".to_string()));
        let result = verify_password_hash(excepted_password_hash, password);
        assert!(result.is_err());
    }
}
