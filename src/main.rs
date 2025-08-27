use sqlx::postgres::PgPoolOptions;

use zero2prod::configuration::get_configuration;
use zero2prod::startup::run;
use zero2prod::telemetry::{get_subscriber, init_subscriber};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let subscriber = get_subscriber("zero2prod".into(), "info".into(), std::io::stdout);
    init_subscriber(subscriber);

    let settings = get_configuration().expect("Failed to read configuration.");
    let listener = std::net::TcpListener::bind(format!(
        "{}:{}",
        settings.application.host, settings.application.port
    ))?;
    let connection_pool = PgPoolOptions::new()
        .acquire_timeout(std::time::Duration::from_secs(2)) // 链接超时 2秒
        .connect_lazy_with(settings.database.with_db());
    run(listener, connection_pool)?.await
}
