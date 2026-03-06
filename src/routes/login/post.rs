use actix_web::{
    HttpResponse, ResponseError,
    http::{self, header::LOCATION},
    web,
};
use hmac::{Hmac, Mac};
use secrecy::{ExposeSecret, SecretBox};
use sqlx::PgPool;

use crate::{
    authentication::{AuthError, Credentials, validate_credentials},
    routes::error_chain_fmt,
    startup::HmacSecret,
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

    // fn error_response(&self) -> HttpResponse<actix_web::body::BoxBody> {
    //     let query_string = format!("error={}", urlencoding::encode(&self.to_string()));
    //     let secret: &[u8] = vec![0; 32];
    //     let hmac_tag = {
    //         let mut mac = Hmac::<sha2::Sha256>::new_from_slice(secret).expect("HMAC key is valid");
    //         mac.update(query_string.as_bytes());
    //         mac.finalize().into_bytes()
    //     };
    //     HttpResponse::build(self.status_code())
    //         .insert_header((LOCATION, format!("/login?{query_string}&tag={hmac_tag:x}")))
    //         .finish()
    // }
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
    secret: web::Data<HmacSecret>,
) -> HttpResponse {
    let credentials = Credentials {
        username: form.0.username,
        password: form.0.password,
    };
    tracing::Span::current().record("username", &tracing::field::display(&credentials.username));

    match validate_credentials(credentials, &pool).await {
        Ok(user_id) => {
            tracing::Span::current().record("user_id", &tracing::field::display(&user_id));
            HttpResponse::SeeOther()
                .insert_header((LOCATION, "/"))
                .finish()
        }
        Err(e) => {
            let e = match e {
                AuthError::InvalidCredentials(_) => LoginError::AuthError(e.into()),
                AuthError::UnexceptedError(_) => LoginError::UnexceptedError(e.into()),
            };
            let query_string = format!("error={}", urlencoding::Encoded::new(e.to_string()));
            let hmac_tag = {
                let mut mac =
                    Hmac::<sha2::Sha256>::new_from_slice(secret.0.expose_secret().as_bytes())
                        .expect("HMAC key is valid");
                mac.update(query_string.as_bytes());
                mac.finalize().into_bytes()
            };
            HttpResponse::SeeOther()
                .insert_header((LOCATION, format!("/login?{query_string}&tag={hmac_tag:x}")))
                .finish()
        }
    }
}
