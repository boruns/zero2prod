use actix_web::{
    HttpResponse, ResponseError,
    http::{self, header::LOCATION},
    web,
};
use secrecy::SecretBox;
use sqlx::PgPool;

use crate::{
    authentication::{AuthError, Credentials, validate_credentials},
    routes::error_chain_fmt,
};

#[derive(thiserror::Error)]
pub enum LoginError {
    #[error("Authentication failed")]
    AuthError(#[source] anyhow::Error),
    #[error("Something went wrong")]
    UnexceptedError(#[from] anyhow::Error),
}

#[derive(serde::Deserialize, Debug)]
pub struct FormData {
    username: String,
    password: SecretBox<String>,
}

impl ResponseError for LoginError {
    fn status_code(&self) -> http::StatusCode {
        match self {
            LoginError::AuthError(_) => http::StatusCode::UNAUTHORIZED,
            LoginError::UnexceptedError(_) => http::StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl std::fmt::Debug for LoginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        error_chain_fmt(self, f)
    }
}

#[tracing::instrument(name = "Login user", skip(pool))]
pub async fn login(
    form: web::Form<FormData>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, LoginError> {
    let credentials = Credentials {
        username: form.0.username,
        password: form.0.password,
    };
    tracing::Span::current().record("username", &tracing::field::display(&credentials.username));

    let user_id = validate_credentials(credentials, &pool)
        .await
        .map_err(|e| match e {
            AuthError::InvalidCredentials(_) => LoginError::AuthError(e.into()),
            AuthError::UnexceptedError(_) => LoginError::UnexceptedError(e.into()),
        })?;
    tracing::Span::current().record("user_id", &tracing::field::display(&user_id));

    Ok(HttpResponse::SeeOther()
        .insert_header((LOCATION, "/"))
        .finish())
}
