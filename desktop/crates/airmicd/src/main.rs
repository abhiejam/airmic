mod config;
mod control;
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
use clap::Parser;
use tokio::net::{TcpListener, UdpSocket};
use tokio::sync::oneshot;
use tokio::sync::watch;
use tracing::{info, warn};

use crate::config::Config;
use crate::control::ControlOptions;
use crate::jitter::JitterBuffer;
use crate::pairing::Pairing;
use crate::pipewire_sink::PipeWireSink;
use crate::sink::AudioSink;

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
    let config_dir = config::config_dir()?;
    let config_path = args
        .config
        .unwrap_or_else(|| config_dir.join("config.toml"));
    let config = Config::load(&config_path)?;
    let pairing = Arc::new(Mutex::new(Pairing::load(config_dir.join("paired.json"))?));

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
    let sink: Box<dyn AudioSink> = Box::new(PipeWireSink {
        set_default_source: config.set_default_source,
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
    let ipc = ipc::Ipc::new(config_path, config.clone());
    let opts = ControlOptions {
        audio_port: config.audio_port,
        no_auth: args.no_auth,
    };
    tokio::select! {
        _ = control::serve(listener, opts, session_tx, pairing, buffer.clone()) => {}
        _ = ipc::serve(ipc_listener, ipc) => {}
        _ = receiver::receive(udp, session_rx.clone(), buffer.clone()) => {}
        _ = receiver::log_stats(session_rx, buffer) => {}
        result = sink_failed => return result.context("audio output thread died")?,
        _ = tokio::signal::ctrl_c() => info!("shutting down"),
    }
    if let Some(advert) = advert {
        advert.withdraw();
    }
    let _ = std::fs::remove_file(&socket);
    Ok(())
}
