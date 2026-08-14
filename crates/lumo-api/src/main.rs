use lumo_api::{build_app, config::ApiConfig, crypto::MasterKey, storage::ApiStore};

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
    let config = match ApiConfig::from_env() {
        Ok(config) => config,
        Err(_) => return false,
    };
    let database_path = config.database_path.clone();
    let master = match MasterKey::new(&config.master_key) {
        Ok(master) => master,
        Err(_) => return false,
    };
    let database_ok = tokio::task::spawn_blocking(move || {
        ApiStore::open(database_path, &master).and_then(|store| store.healthcheck())
    })
    .await
    .is_ok_and(|result| result.is_ok());
    database_ok
        && tokio::net::TcpStream::connect(("127.0.0.1", config.bind.port()))
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
        .serve(app.into_make_service_with_connect_info::<std::net::SocketAddr>())
        .await?;
    Ok(())
}
