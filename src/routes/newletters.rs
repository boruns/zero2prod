use crate::{
    authentication::{AuthError, Credentials, validate_credentials},
    domain::SubscriberEmail,
    email_client::EmailClient,
    routes::error_chain_fmt,
};
use actix_web::{
    HttpRequest, HttpResponse, ResponseError,
    http::header::{HeaderMap, HeaderValue},
    web,
};
use anyhow::{Context, anyhow};
use secrecy::SecretBox;
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

    let user_id = validate_credentials(credentials, &pool)
        .await
        .map_err(|error| match error {
            AuthError::InvalidCredentials(_) => PublishError::AuthError(error.into()),
            AuthError::UnexceptedError(_) => PublishError::UnexpectedError(error.into()),
        })?;
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
