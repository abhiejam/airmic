mod config;
mod control;
#[allow(dead_code)] // Removed once the UDP receiver (D2.5) feeds it.
mod jitter;

use std::io::IsTerminal;
use std::net::Ipv6Addr;
use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tracing::info;

use crate::config::Config;
use crate::control::ControlOptions;

/// AirMic daemon: receives audio from the iPhone app and exposes it as a microphone.
#[derive(Parser)]
#[command(version)]
struct Args {
    /// Accept any phone without pairing (development only).
    #[arg(long)]
    no_auth: bool,

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
    anyhow::ensure!(
        args.no_auth,
        "pairing is not implemented yet; run with --no-auth"
    );
    let config_path = match args.config {
        Some(path) => path,
        None => config::config_dir()?.join("config.toml"),
    };
    let config = Config::load(&config_path)?;

    let listener = TcpListener::bind((Ipv6Addr::UNSPECIFIED, config.control_port))
        .await
        .with_context(|| format!("binding TCP {}", config.control_port))?;
    info!("control channel on TCP {}", config.control_port);

    let (session_tx, _session_rx) = watch::channel(None);
    let opts = ControlOptions {
        audio_port: config.audio_port,
    };
    tokio::select! {
        _ = control::serve(listener, opts, session_tx) => {}
        _ = tokio::signal::ctrl_c() => info!("shutting down"),
    }
    Ok(())
}
