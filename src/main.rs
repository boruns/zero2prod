use zero2prod::configuration::get_configuration;
use zero2prod::startup::run;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let _settings = get_configuration().expect("Failed to read configuration.");
    let listener = std::net::TcpListener::bind("127.0.0.1:8081")?;
    run(listener)?.await
}
