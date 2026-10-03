//! Test sender: plays the phone's role with the real protocol, so `airmicd` can be tested without a phone.

use std::f64::consts::TAU;
use std::path::PathBuf;
use std::time::Duration;

use airmic_proto::{FRAME_SAMPLES, Header, Message, SAMPLE_RATE};
use anyhow::{Context, bail};
use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::time::{MissedTickBehavior, interval, sleep};
use tokio_util::codec::{Framed, LinesCodec};

/// Streams a sine tone or a WAV file to airmicd, optionally with simulated network trouble.
#[derive(Parser)]
#[command(version)]
struct Args {
    /// Daemon address.
    #[arg(default_value = "127.0.0.1")]
    host: String,

    #[arg(long, default_value_t = airmic_proto::CONTROL_PORT)]
    port: u16,

    /// WAV file to stream (48 kHz, mono, 16 bit). Default: a sine tone.
    #[arg(long)]
    wav: Option<PathBuf>,

    /// Tone frequency in Hz.
    #[arg(long, default_value_t = 440.0)]
    freq: f64,

    /// Stop after this many seconds. Default: until the WAV ends, or Ctrl-C for a tone.
    #[arg(long)]
    seconds: Option<f64>,

    /// Percent of packets to drop.
    #[arg(long, default_value_t = 0.0)]
    loss: f64,

    /// Maximum random extra delay per packet, in ms. Delayed packets can overtake each other.
    #[arg(long, default_value_t = 0)]
    jitter: u64,

    /// Percent of packets swapped with the packet after them.
    #[arg(long, default_value_t = 0.0)]
    reorder: f64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let mut samples = load_samples(&args)?;

    let tcp = TcpStream::connect((args.host.as_str(), args.port))
        .await
        .with_context(|| format!("connecting to {}:{}", args.host, args.port))?;
    let mut control = Framed::new(tcp, LinesCodec::new());
    let hello = Message::Hello {
        v: 1,
        phone_id: "airmic-send".into(),
        phone_name: "airmic-send".into(),
    };
    control.send(hello.to_line().trim_end()).await?;
    let (session_id, udp_port) = loop {
        let line = control
            .next()
            .await
            .context("daemon closed the connection")??;
        match Message::from_line(&line)? {
            Message::Ready {
                session_id,
                udp_port,
                ..
            } => break (session_id, udp_port),
            Message::Error { code, message } => bail!("daemon refused: {code:?}: {message}"),
            _ => {}
        }
    };
    eprintln!("session {session_id:#010x}, streaming to UDP {udp_port}");

    let udp = UdpSocket::bind("0.0.0.0:0").await?;
    udp.connect((args.host.as_str(), udp_port)).await?;
    let udp = std::sync::Arc::new(udp);

    let mut tick = interval(Duration::from_millis(10));
    tick.set_missed_tick_behavior(MissedTickBehavior::Burst);
    let mut keepalive = interval(Duration::from_secs(2));
    let mut held: Option<Vec<u8>> = None;
    let mut sent = 0u32;

    loop {
        tokio::select! {
            _ = tick.tick() => {
                let Some(frame) = samples.next() else { break };
                let header = Header {
                    muted: false,
                    codec: 0,
                    session_id,
                    sequence: sent,
                    timestamp: sent.wrapping_mul(FRAME_SAMPLES as u32),
                };
                sent = sent.wrapping_add(1);
                let mut packet = header.encode().to_vec();
                packet.extend(frame.iter().flat_map(|s| s.to_le_bytes()));

                if rand::random_bool(args.loss / 100.0) {
                    continue;
                }
                if held.is_none() && rand::random_bool(args.reorder / 100.0) {
                    held = Some(packet);
                    continue;
                }
                for packet in [Some(packet), held.take()].into_iter().flatten() {
                    let delay = rand::random_range(0..=args.jitter);
                    let udp = udp.clone();
                    tokio::spawn(async move {
                        sleep(Duration::from_millis(delay)).await;
                        let _ = udp.send(&packet).await;
                    });
                }
            }
            _ = keepalive.tick() => control.send(Message::Ping.to_line().trim_end()).await?,
            line = control.next() => {
                let line = line.context("daemon closed the connection")??;
                match Message::from_line(&line)? {
                    Message::Ping => control.send(Message::Pong.to_line().trim_end()).await?,
                    Message::Stats { loss_pct, jitter_ms, latency_ms } => {
                        eprintln!("stats: loss {loss_pct:.1}%, jitter {jitter_ms:.1} ms, latency {latency_ms:.0} ms");
                    }
                    Message::Bye => bail!("daemon ended the session"),
                    _ => {}
                }
            }
            _ = tokio::signal::ctrl_c() => break,
        }
    }

    control.send(Message::Bye.to_line().trim_end()).await?;
    eprintln!("sent {sent} packets");
    Ok(())
}

/// Returns an iterator of 10 ms frames from the WAV file or a sine tone.
fn load_samples(args: &Args) -> anyhow::Result<Box<dyn Iterator<Item = Vec<i16>>>> {
    let max_frames = args.seconds.map_or(usize::MAX, |s| (s * 100.0) as usize);
    let Some(path) = &args.wav else {
        let step = TAU * args.freq / f64::from(SAMPLE_RATE);
        let tone = (0..).map(move |n: u64| ((n as f64 * step).sin() * 8000.0) as i16);
        let frames = chunk(tone);
        return Ok(Box::new(frames.take(max_frames)));
    };
    let wav =
        hound::WavReader::open(path).with_context(|| format!("opening {}", path.display()))?;
    let spec = wav.spec();
    if (spec.sample_rate, spec.channels, spec.bits_per_sample) != (SAMPLE_RATE, 1, 16) {
        bail!("WAV must be 48 kHz mono 16 bit, got {spec:?}");
    }
    let samples: Vec<i16> = wav.into_samples().collect::<Result<_, _>>()?;
    Ok(Box::new(chunk(samples.into_iter()).take(max_frames)))
}

/// Groups samples into whole 480 sample frames, padding the last one with silence.
fn chunk(mut samples: impl Iterator<Item = i16>) -> impl Iterator<Item = Vec<i16>> {
    std::iter::from_fn(move || {
        let mut frame: Vec<i16> = samples.by_ref().take(FRAME_SAMPLES).collect();
        if frame.is_empty() {
            return None;
        }
        frame.resize(FRAME_SAMPLES, 0);
        Some(frame)
    })
}
