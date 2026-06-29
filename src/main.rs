//! Hollywood entry point — desktop shell by default; headless foundation mode
//! for CI until the CLI pipeline lands.

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use hollywood::cli::ProcessArgs;

/// Hollywood — video pre-editing automation.
#[derive(Debug, Parser)]
#[command(name = "hollywood", version, about)]
struct Cli {
    /// Log filter directive (overridden by the `RUST_LOG` environment variable).
    #[arg(long, env = "RUST_LOG", default_value = "info")]
    log_level: String,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Launch the desktop shell (default).
    Gui,
    /// Initialize subsystems and exit — for CI and headless smoke checks.
    Init,
    /// Pre-edit a source file and write the exported NLE timelines.
    Process(ProcessArgs),
}

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(cli.log_level))
        .try_init()?;

    hollywood::media::init()?;

    match cli.command {
        None | Some(Command::Gui) => hollywood_gui::run()?,
        Some(Command::Init) => tracing::info!("hollywood foundation ready"),
        Some(Command::Process(args)) => hollywood::cli::run_process(&args)?,
    }

    Ok(())
}
