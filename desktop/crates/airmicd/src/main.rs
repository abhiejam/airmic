mod config;

use std::io::IsTerminal;
use std::path::PathBuf;

use clap::Parser;
use tracing::info;

use crate::config::Config;

/// AirMic daemon: receives audio from the iPhone app and exposes it as a microphone.
#[derive(Parser)]
#[command(version)]
struct Args {
    /// Config file. Default: ~/.config/airmic/config.toml
    #[arg(long)]
    config: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Under systemd, stderr goes to journald, which adds its own timestamps.
    let journald = std::env::var_os("JOURNAL_STREAM").is_some();
    let log = tracing_subscriber::fmt().with_ansi(std::io::stderr().is_terminal());
    if journald {
        log.without_time().init();
    } else {
        log.init();
    }

    let args = Args::parse();
    let config_path = match args.config {
        Some(path) => path,
        None => config::config_dir()?.join("config.toml"),
    };
    let config = Config::load(&config_path)?;
    info!(?config, "airmicd started");

    tokio::signal::ctrl_c().await?;
    info!("shutting down");
    Ok(())
}
