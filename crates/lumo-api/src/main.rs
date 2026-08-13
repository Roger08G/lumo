use lumo_api::{build_app, config::ApiConfig};

#[tokio::main]
async fn main() {
    if std::env::args().any(|argument| argument == "--healthcheck") {
        let status = healthcheck().await;
        std::process::exit(if status { 0 } else { 1 });
    }
    if let Err(error) = run().await {
        eprintln!("lumo-api error: {error}");
        std::process::exit(1);
    }
}

async fn healthcheck() -> bool {
    let bind = std::env::var("LUMO_API_BIND").unwrap_or_else(|_| "0.0.0.0:8443".to_owned());
    let port = bind
        .rsplit_once(':')
        .and_then(|(_, port)| port.parse::<u16>().ok())
        .unwrap_or(8443);
    tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .is_ok()
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config = ApiConfig::from_env()?;
    let app = build_app(&config)?;
    let tls = axum_server::tls_rustls::RustlsConfig::from_pem_file(
        &config.tls_cert_path,
        &config.tls_key_path,
    )
    .await?;
    axum_server::bind_rustls(config.bind, tls)
        .serve(app.into_make_service())
        .await?;
    Ok(())
}
