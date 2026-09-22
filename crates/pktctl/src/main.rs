use std::process::ExitCode;

use pktctl::{config::Config, packet_tracer::LivePacketTracer, server::PktctlServer};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("pktctl {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }

    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(EnvFilter::try_from_env("PKTCTL_LOG").unwrap_or_else(|_| "warn".into()))
        .init();

    let config = match Config::from_env() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("pktctl: {error}");
            return ExitCode::FAILURE;
        }
    };

    let server = PktctlServer::new(LivePacketTracer::new(config.session));
    match server.serve_stdio().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("pktctl: {error}");
            ExitCode::FAILURE
        }
    }
}
