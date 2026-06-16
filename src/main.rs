//! Hollywood entry point.
//!
//! The foundation binary initializes logging and the media backend and reports
//! readiness. The `egui` desktop shell and the processing subcommands (probe,
//! detect, sync, export) land in later crates.

use clap::Parser;
use tracing_subscriber::EnvFilter;

/// Hollywood — video pre-editing automation.
#[derive(Debug, Parser)]
#[command(name = "hollywood", version, about)]
struct Cli {
    /// Log filter directive (overridden by the `RUST_LOG` environment variable).
    #[arg(long, env = "RUST_LOG", default_value = "info")]
    log_level: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(cli.log_level))
        .try_init()?;

    hollywood::media::init()?;
    tracing::info!("hollywood foundation ready");

    Ok(())
}
