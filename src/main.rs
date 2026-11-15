//! Explicit server startup; deployment TLS and administration protection belong
//! to the configured reverse proxy, not a built-in authentication subsystem.
use geello::server::{ServerConfig, ServerState, router};
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = match args.next().as_deref() {
        None => "server.toml".to_owned(),
        Some("--config") => args.next().ok_or("--config requires a file")?,
        _ => return Err("usage: geello [--config server.toml]".into()),
    };
    if args.next().is_some() {
        return Err("unexpected command-line arguments".into());
    }
    let config = ServerConfig::load(std::path::Path::new(&path))?;
    let listen = config.listen;
    let state = ServerState::open(config).await?;
    let listener = tokio::net::TcpListener::bind(listen).await?;
    eprintln!(
        "Geello ready on {}; actual backend={}",
        listener.local_addr()?,
        state.backend_name
    );
    let shutdown_state = state.clone();
    let serving = axum::serve(listener, router(state.clone()))
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            shutdown_state.begin_shutdown();
        })
        .await;
    state.shutdown().await?;
    serving?;
    Ok(())
}
