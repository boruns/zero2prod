use crate::{domain::SubscriberEmail, email_client::EmailClient, routes::error_chain_fmt};
use actix_web::{HttpResponse, ResponseError, web};
use anyhow::{Context, anyhow};
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
            PublishError::UnexpectedError(_) => HttpResponse::InternalServerError().finish(),
        }
    }
}

pub async fn publish_newsletter(
    body: web::Json<BodyData>,
    pool: web::Data<PgPool>,
    email_client: web::Data<EmailClient>,
) -> Result<HttpResponse, PublishError> {
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
