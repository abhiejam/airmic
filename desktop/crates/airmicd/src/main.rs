mod cli;
mod config;
mod control;
mod install;
mod ipc;
mod jitter;
mod mdns;
mod pairing;
mod pipewire_sink;
mod receiver;
mod sink;

use std::io::IsTerminal;
use std::net::Ipv6Addr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Context;
use clap::{Parser, Subcommand};
use tokio::net::{TcpListener, UdpSocket};
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::oneshot;
use tokio::sync::watch;
use tracing::{info, warn};

use crate::config::Config;
use crate::control::ControlOptions;
use crate::jitter::JitterBuffer;
use crate::pairing::Pairing;
use crate::pipewire_sink::{PipeWireDefault, PipeWireSink};
use crate::receiver::{LastPacket, Level};
use crate::sink::AudioSink;

/// AirMic: use your iPhone as a wireless microphone.
#[derive(Parser)]
#[command(name = "airmic", version, arg_required_else_help = true)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(flatten)]
    Client(cli::Command),
    /// Run the daemon in the foreground. The airmicd service runs this.
    Daemon {
        /// Accept any phone without pairing (development only).
        #[arg(long)]
        no_auth: bool,

        /// Config file. Default: ~/.config/airmic/config.toml
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Install and start the airmicd systemd user service for this binary.
    Install {
        /// Print the unit and commands without changing anything.
        #[arg(long)]
        dry_run: bool,
    },
    /// Stop the airmicd service and remove it.
    Uninstall,
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

    let (no_auth, config_arg) = match Args::parse().command {
        Command::Client(command) => return cli::run(command).await,
        Command::Daemon { no_auth, config } => (no_auth, config),
        Command::Install { dry_run } => return install::install(dry_run),
        Command::Uninstall => return install::uninstall(),
    };
    let config_dir = config::config_dir()?;
    let config_path = config_arg.unwrap_or_else(|| config_dir.join("config.toml"));
    let config = Config::load(&config_path)?;
    let pairing = Arc::new(Mutex::new(Pairing::load(config_dir.join("paired.json"))?));
    let device_id = mdns::load_or_create_device_id(&config_dir)?;

    let listener = TcpListener::bind((Ipv6Addr::UNSPECIFIED, config.control_port))
        .await
        .with_context(|| format!("binding TCP {}", config.control_port))?;
    info!("control channel on TCP {}", config.control_port);
    let advert = mdns::advertise(config.control_port)
        .inspect_err(|e| warn!("mDNS advert failed, phones must connect by IP: {e:#}"))
        .ok();

    let udp = UdpSocket::bind((Ipv6Addr::UNSPECIFIED, config.audio_port))
        .await
        .with_context(|| format!("binding UDP {}", config.audio_port))?;
    info!("audio channel on UDP {}", config.audio_port);

    let buffer = Arc::new(Mutex::new(JitterBuffer::new()));
    let level = Arc::new(Level::default());
    let last_packet = LastPacket::default();
    let default_source = Arc::new(PipeWireDefault::default());
    let sink: Box<dyn AudioSink> = Box::new(PipeWireSink {
        default_source: config.set_default_source.then(|| default_source.clone()),
    });
    let (sink_failed_tx, sink_failed) = oneshot::channel();
    let sink_buffer = buffer.clone();
    std::thread::spawn(move || {
        let _ = sink_failed_tx.send(sink.run(sink_buffer));
    });

    let socket = ipc::socket_path()?;
    let ipc_listener = ipc::UnixSocket::bind(&socket)?;
    info!("IPC on {}", socket.display());

    let (session_tx, session_rx) = watch::channel(None);
    let ipc = ipc::Ipc::new(
        session_rx.clone(),
        level.clone(),
        last_packet.clone(),
        default_source.clone(),
        config_path,
        config.clone(),
        ipc::PairingContext {
            pairing: pairing.clone(),
            device_id,
            control_port: config.control_port,
            lan_address: ipc::default_lan_address,
        },
    );
    let opts = ControlOptions {
        audio_port: config.audio_port,
        no_auth,
    };
    // SIGTERM is how `systemctl --user stop` ends the daemon.
    let mut terminate = signal(SignalKind::terminate()).context("SIGTERM handler")?;
    let result = tokio::select! {
        _ = control::serve(listener, opts, session_tx, pairing, buffer.clone()) => Ok(()),
        _ = ipc::serve(ipc_listener, ipc) => Ok(()),
        _ = receiver::receive(udp, session_rx.clone(), buffer.clone(), last_packet, level) => Ok(()),
        _ = receiver::log_stats(session_rx, buffer) => Ok(()),
        result = sink_failed => result.context("audio output thread died").and_then(|r| r),
        _ = tokio::signal::ctrl_c() => {
            info!("shutting down");
            Ok(())
        }
        _ = terminate.recv() => {
            info!("shutting down");
            Ok(())
        }
    };
    if let Err(e) = default_source.restore_previous() {
        warn!("could not restore the previous default microphone: {e:#}");
    }
    if let Some(advert) = advert {
        advert.withdraw();
    }
    let _ = std::fs::remove_file(&socket);
    result
}
