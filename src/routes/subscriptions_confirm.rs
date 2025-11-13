use actix_web::{HttpResponse, web};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(serde::Deserialize)]
pub struct Parameters {
    pub subscription_token: String,
}

#[tracing::instrument(name = "Confirm a pending subscriber", skip(parameters, pool))]
pub async fn confirm(parameters: web::Query<Parameters>, pool: web::Data<PgPool>) -> HttpResponse {
    let id = match get_subscription_id_from_token(&pool, &parameters.subscription_token).await {
        Ok(id) => id,
        Err(_) => return HttpResponse::InternalServerError().finish(),
    };
    match id {
        Some(subscriber_id) => {
            if confirm_subscriber(&pool, subscriber_id).await.is_ok() {
                tracing::info!("Successfully confirmed subscriber.");
                return HttpResponse::Ok().finish();
            } else {
                tracing::warn!("Failed to confirm subscriber.");
                return HttpResponse::InternalServerError().finish();
            }
        }
        None => {
            tracing::warn!("Failed to find subscriber.");
            return HttpResponse::Unauthorized().finish();
        }
    }
}

#[tracing::instrument(name = "Get subscriber_id from token", skip(pool, subscription_token))]
pub async fn get_subscription_id_from_token(
    pool: &PgPool,
    subscription_token: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    let sql = "SELECT subscriber_id FROM subscription_tokens WHERE subscription_token = $1";
    let subscriber_id: Uuid = sqlx::query(sql)
        .bind(subscription_token)
        .map(|row: sqlx::postgres::PgRow| row.get(0))
        .fetch_one(pool)
        .await?;
    Ok(Some(subscriber_id))
}

#[tracing::instrument(name = "Mark subscriber as confirmed", skip(pool, subscription_id))]
pub async fn confirm_subscriber(pool: &PgPool, subscription_id: Uuid) -> Result<(), sqlx::Error> {
    let sql = "UPDATE subscriptions SET status = 'confirmed' WHERE id = $1";
    sqlx::query(sql).bind(subscription_id).execute(pool).await?;
    Ok(())
}
