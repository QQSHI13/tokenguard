//! Token Guard CLI — headless proxy server.

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use tokenguard_lib::backend;

#[derive(Parser, Debug)]
#[command(name = "tokenguard-cli")]
#[command(about = "Headless Token Guard proxy server")]
struct Args {
    /// Data directory for the SQLite database and exports.
    #[arg(short, long, env = "TOKENGUARD_DATA_DIR")]
    data_dir: Option<PathBuf>,

    /// Override the port stored in the database.
    #[arg(short, long, env = "TOKENGUARD_PORT")]
    port: Option<u16>,

    /// Expose the proxy to the LAN (0.0.0.0) instead of loopback only.
    #[arg(long, env = "TOKENGUARD_EXPOSE_TO_LAN")]
    expose_to_lan: bool,
}

fn default_data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("tokenguard"))
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = tracing_subscriber::fmt::try_init();
    let args = Args::parse();

    let data_dir = args
        .data_dir
        .or_else(default_data_dir)
        .ok_or_else(|| anyhow::anyhow!("could not determine data directory; pass --data-dir"))?;

    let backend::BackendInit {
        state,
        port: db_port,
        expose_to_lan: db_expose,
    } = backend::init_backend(data_dir, None)?;

    let port = args.port.unwrap_or(db_port);
    let expose_to_lan = args.expose_to_lan || db_expose;

    tracing::info!("Token Guard CLI proxy on http://127.0.0.1:{port}");
    if expose_to_lan {
        tracing::info!("proxy exposed to LAN on http://0.0.0.0:{port}");
    }

    let mut proxy_handle = tokio::spawn(backend::serve_proxy(state.clone(), port, expose_to_lan));

    tokio::select! {
        result = &mut proxy_handle => {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(e)) => tracing::error!("proxy server error: {e}"),
                Err(e) => tracing::error!("proxy task panicked: {e}"),
            }
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Ctrl+C received, shutting down");
            state.shutdown();
            // Wait for the proxy to finish graceful shutdown.
            if let Err(e) = proxy_handle.await {
                tracing::error!("proxy task error during shutdown: {e}");
            }
        }
    }

    Ok(())
}
