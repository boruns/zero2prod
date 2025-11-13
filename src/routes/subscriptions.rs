use crate::{
    domain::{NewSubscriber, SubscriberEmail, SubscriberName},
    email_client::EmailClient,
    startup::ApplicationBaseUrl,
};
use actix_web::{HttpResponse, web};
use chrono::Utc;
use rand::{Rng, distr::Alphanumeric};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(serde::Deserialize)]
pub struct FormData {
    name: String,
    email: String,
}

impl TryFrom<FormData> for NewSubscriber {
    type Error = String;
    fn try_from(value: FormData) -> Result<Self, Self::Error> {
        let name = SubscriberName::parse(value.name)?;
        let email = SubscriberEmail::parse(value.email)?;
        Ok(Self { name, email })
    }
}

#[tracing::instrument(
    name = "Adding a new subscriber",
    skip(form, pool, email_client, base_url),
    fields(
        subscriber_email = %form.email,
        subscriber_name = %form.name
    )
)]
pub async fn subscribe(
    form: web::Form<FormData>,
    pool: web::Data<PgPool>,
    email_client: web::Data<EmailClient>,
    base_url: web::Data<ApplicationBaseUrl>,
) -> HttpResponse {
    let new_subscriber = match form.0.try_into() {
        Ok(subscriber) => subscriber,
        Err(error) => return HttpResponse::BadRequest().body(error),
    };

    let mut transaction = match pool.begin().await {
        Ok(transaction) => transaction,
        Err(e) => {
            tracing::error!("Failed to start a database transaction: {:?}", e);
            return HttpResponse::InternalServerError().finish();
        }
    };

    let subscriber_id = match insert_subscriber(&mut transaction, &new_subscriber).await {
        Ok(id) => id,
        Err(_) => return HttpResponse::InternalServerError().finish(),
    };
    let subscription_token = generate_subscription_token();
    if store_token(&mut transaction, &subscription_token, subscriber_id)
        .await
        .is_err()
    {
        return HttpResponse::InternalServerError().finish();
    }

    // 发送确认邮件
    if send_confirmation_email(
        &email_client,
        new_subscriber,
        &base_url.0,
        &subscription_token,
    )
    .await
    .is_err()
    {
        return HttpResponse::InternalServerError().finish();
    }

    HttpResponse::Ok().finish()
}

#[tracing::instrument(
    name = "Sending a confirmation email to a new subscriber",
    skip(email_client, new_subscriber, base_url, subscription_token)
)]
pub async fn send_confirmation_email(
    email_client: &EmailClient,
    new_subscriber: NewSubscriber,
    base_url: &str,
    subscription_token: &str,
) -> Result<(), reqwest::Error> {
    let confirmation_link = format!(
        "{}/subscriptions/confirm?subscription_token={}",
        base_url, subscription_token
    );
    let plain_body = format!(
        "Welcome to our newsletter!\nVisit {} to confirm your subscription.",
        confirmation_link
    );
    let html_body = format!(
        "Welcome to our newsletter!<br /> \
                    Click <a href=\"{}\">here</a> to confirm your subscription.",
        confirmation_link
    );
    email_client
        .send_email(new_subscriber.email, "Welcome!", &html_body, &plain_body)
        .await
}

#[tracing::instrument(
    name = "Saving new subscriber details in the database",
    skip(transaction, subscription_token, subscriber_id)
)]
pub async fn store_token(
    transaction: &mut Transaction<'_, Postgres>,
    subscription_token: &str,
    subscriber_id: Uuid,
) -> Result<(), sqlx::Error> {
    let sql = "INSERT INTO subscription_tokens (subscription_token, subscriber_id) VALUES ($1, $2)";
    sqlx::query(sql)
        .bind(subscription_token)
        .bind(subscriber_id)
        .execute(transaction.as_mut())
        .await
        .map_err(|e| {
            tracing::error!("Failed to execute query: {:?}", e);
            e
        })?;
    Ok(())
}

#[tracing::instrument(
    name = "Saving new subscriber details in the database",
    skip(form, transaction)
)]
pub async fn insert_subscriber(
    transaction: &mut Transaction<'_, Postgres>,
    form: &NewSubscriber,
) -> Result<Uuid, sqlx::Error> {
    let sql = "INSERT INTO subscriptions (id, email, name, subscribed_at, status) VALUES ($1, $2, $3, $4, $5)";
    let subscription_id = Uuid::new_v4();
    sqlx::query(sql)
        .bind(subscription_id)
        .bind(form.email.as_ref())
        .bind(form.name.as_ref())
        .bind(Utc::now())
        .bind("pending_confirmation")
        .execute(transaction.as_mut())
        .await
        .map_err(|e| {
            tracing::error!("Failed to execute query: {:?}", e);
            e
        })?;
    Ok(subscription_id)
}

fn generate_subscription_token() -> String {
    let mut rng = rand::rng();
    std::iter::repeat_with(|| rng.sample(Alphanumeric))
        .map(char::from)
        .take(25)
        .collect()
}
