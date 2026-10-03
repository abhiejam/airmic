//! UDP audio receiver (docs/protocol.md §3): validates packets and feeds the jitter buffer.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use airmic_proto::{FRAME_BYTES, FRAME_SAMPLES, HEADER_LEN, Header, SAMPLE_RATE};
use tokio::net::UdpSocket;
use tokio::sync::watch;
use tracing::{info, warn};

use crate::control::Session;
use crate::jitter::JitterBuffer;
use crate::sink::SharedBuffer;

/// When the receiver last accepted a packet from the active phone.
pub type LastPacket = Arc<Mutex<Option<Instant>>>;

const LEVEL_WINDOW: usize = SAMPLE_RATE as usize / 20;

/// RMS and peak (0–1) of the last 50 ms of phone audio, for the desktop app's meter.
#[derive(Default)]
pub struct Level {
    rms: AtomicU32,
    peak: AtomicU32,
}

impl Level {
    /// Returns `(rms, peak)`.
    pub fn get(&self) -> (f32, f32) {
        let load = |a: &AtomicU32| f32::from_bits(a.load(Ordering::Relaxed));
        (load(&self.rms), load(&self.peak))
    }

    pub fn set(&self, rms: f32, peak: f32) {
        self.rms.store(rms.to_bits(), Ordering::Relaxed);
        self.peak.store(peak.to_bits(), Ordering::Relaxed);
    }
}

/// Accumulates received samples into 50 ms windows for `Level`.
#[derive(Default)]
pub struct LevelMeter {
    sum_squares: f64,
    peak: u16,
    samples: usize,
}

impl LevelMeter {
    /// Adds received samples and publishes each completed 50 ms window to `level`.
    pub fn add(&mut self, samples: &[i16], level: &Level) {
        for s in samples {
            let v = s.unsigned_abs();
            self.sum_squares += f64::from(v) * f64::from(v);
            self.peak = self.peak.max(v);
            self.samples += 1;
            if self.samples == LEVEL_WINDOW {
                let rms = (self.sum_squares / LEVEL_WINDOW as f64).sqrt() / 32768.0;
                level.set(rms as f32, f32::from(self.peak) / 32768.0);
                *self = LevelMeter::default();
            }
        }
    }
}

/// Writes one CSV row per accepted packet to the file named by `AIRMIC_PACKET_TRACE`, to measure
/// stalls and clock drift on a real phone stream (tools/analyze-packet-trace.py).
struct PacketTrace {
    out: BufWriter<File>,
    start: Instant,
    rows: u32,
}

impl PacketTrace {
    fn from_env(start: Instant) -> Option<PacketTrace> {
        let path = std::env::var_os("AIRMIC_PACKET_TRACE")?;
        let file = File::create(&path)
            .inspect_err(|e| warn!("packet trace {}: {e}", path.to_string_lossy()))
            .ok()?;
        let mut out = BufWriter::new(file);
        let _ = writeln!(
            out,
            "arrival_us,session,sequence,timestamp,muted,buffered_frames,received,lost,dropped,underruns"
        );
        info!("tracing packets to {}", path.to_string_lossy());
        Some(PacketTrace {
            out,
            start,
            rows: 0,
        })
    }

    /// Logs a packet and the buffer's counters right after it was pushed.
    fn record(&mut self, header: &Header, arrival: Instant, jb: &JitterBuffer) {
        let s = jb.stats();
        let _ = writeln!(
            self.out,
            "{},{},{},{},{},{},{},{},{},{}",
            arrival.saturating_duration_since(self.start).as_micros(),
            header.session_id,
            header.sequence,
            header.timestamp,
            u8::from(header.muted),
            jb.delay_ms() / 10.0,
            s.received,
            s.lost,
            s.dropped,
            s.underruns,
        );
        self.rows += 1;
        if self.rows.is_multiple_of(100) {
            let _ = self.out.flush();
        }
    }
}

pub async fn receive(
    socket: UdpSocket,
    session: watch::Receiver<Option<Session>>,
    buffer: SharedBuffer,
    last_packet: LastPacket,
    level: Arc<Level>,
) {
    let mut meter = LevelMeter::default();
    let mut packet = [0u8; 2048];
    let mut buffer_session = None;
    let mut trace = PacketTrace::from_env(Instant::now());
    loop {
        let (len, from) = match socket.recv_from(&mut packet).await {
            Ok(received) => received,
            Err(e) => {
                warn!("UDP receive failed: {e}");
                continue;
            }
        };
        let Some((header, payload)) =
            accept_packet(&packet[..len], from, session.borrow().as_ref())
        else {
            continue;
        };
        let arrival = Instant::now();
        *last_packet.lock().expect("last packet lock") = Some(arrival);
        if header.muted {
            meter = LevelMeter::default();
            level.set(0.0, 0.0);
        } else {
            let mut samples = [0i16; FRAME_SAMPLES];
            for (s, b) in samples.iter_mut().zip(payload.as_chunks::<2>().0) {
                *s = i16::from_le_bytes(*b);
            }
            meter.add(&samples, &level);
        }
        let mut jb = buffer.lock().expect("jitter buffer lock");
        if buffer_session != Some(header.session_id) {
            *jb = JitterBuffer::new();
            buffer_session = Some(header.session_id);
        }
        jb.push(&header, payload, arrival);
        if let Some(trace) = &mut trace {
            trace.record(&header, arrival, &jb);
        }
    }
}

/// Logs audio health every 10 s while a phone is connected.
pub async fn log_stats(session: watch::Receiver<Option<Session>>, buffer: SharedBuffer) {
    let mut tick = tokio::time::interval(Duration::from_secs(10));
    loop {
        tick.tick().await;
        if session.borrow().is_none() {
            continue;
        }
        let (s, delay_ms) = {
            let jb = buffer.lock().expect("jitter buffer lock");
            (jb.stats(), jb.delay_ms())
        };
        info!(
            s.received,
            s.lost,
            s.late,
            s.duplicate,
            s.dropped,
            s.underruns,
            delay_ms,
            jitter_ms = format!("{:.1}", s.jitter_ms),
            "audio stats"
        );
    }
}

/// Returns the header and PCM payload of a packet from the active phone, or `None` to drop it.
fn accept_packet<'a>(
    packet: &'a [u8],
    from: SocketAddr,
    session: Option<&Session>,
) -> Option<(Header, &'a [u8])> {
    let header = Header::decode(packet).ok()?;
    let session = session?;
    if header.session_id != session.id || from.ip().to_canonical() != session.phone_addr {
        return None;
    }
    let payload = &packet[HEADER_LEN..];
    match (header.codec, header.muted, payload.len()) {
        (0, true, _) => Some((header, &[])),
        (0, false, FRAME_BYTES) => Some((header, payload)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use airmic_proto::FRAME_SAMPLES;

    const PHONE: &str = "192.168.1.20:5000";

    fn session() -> Session {
        Session {
            id: 7,
            phone_id: "p1".into(),
            phone_addr: "192.168.1.20".parse().unwrap(),
            phone_name: "iPhone".into(),
            muted: false,
            stats: None,
        }
    }

    fn packet(session_id: u32, muted: bool, codec: u8, payload_len: usize) -> Vec<u8> {
        let header = Header {
            muted,
            codec,
            session_id,
            sequence: 1,
            timestamp: 480,
        };
        let mut p = header.encode().to_vec();
        p.resize(HEADER_LEN + payload_len, 1);
        p
    }

    fn accepts(packet: &[u8], from: &str, session: Option<&Session>) -> bool {
        accept_packet(packet, from.parse().unwrap(), session).is_some()
    }

    #[test]
    fn accepts_audio_from_the_active_phone() {
        let p = packet(7, false, 0, FRAME_BYTES);
        let (_, payload) = accept_packet(&p, PHONE.parse().unwrap(), Some(&session())).unwrap();
        assert_eq!(payload.len(), FRAME_BYTES);
        // An IPv4 phone seen through the dual-stack socket.
        let mapped = "[::ffff:192.168.1.20]:5000";
        assert!(accepts(
            &packet(7, false, 0, FRAME_BYTES),
            mapped,
            Some(&session())
        ));
    }

    #[test]
    fn muted_packet_carries_no_payload() {
        let p = packet(7, true, 0, 0);
        let accepted = accept_packet(&p, PHONE.parse().unwrap(), Some(&session()));
        assert!(accepted.is_some_and(|(h, p)| h.muted && p.is_empty()));
    }

    #[test]
    fn muted_packets_play_silence_whatever_their_payload() {
        let (mut jb, t, s) = (JitterBuffer::new(), Instant::now(), session());
        let mut feed = |seq: u32, muted: bool, payload_len: usize| {
            let mut p = packet(7, muted, 0, payload_len);
            p[8..12].copy_from_slice(&seq.to_be_bytes());
            let (header, payload) = accept_packet(&p, PHONE.parse().unwrap(), Some(&s)).unwrap();
            let arrival = t + Duration::from_millis(u64::from(seq) * 10);
            jb.push(&header, payload, arrival);
            let mut out = [0; FRAME_SAMPLES];
            jb.read(&mut out);
            out[FRAME_SAMPLES / 2]
        };
        // Payload bytes are all 1, so each unmuted sample is 0x0101 once the buffer has primed.
        let unmuted: Vec<i16> = (0..6).map(|seq| feed(seq, false, FRAME_BYTES)).collect();
        assert_eq!(unmuted[4..], [0x0101, 0x0101]);
        assert_eq!(feed(6, true, 0), 0, "header-only muted packet");
        assert_eq!(feed(7, true, FRAME_BYTES), 0, "muted packet with audio");
    }

    #[test]
    fn drops_packets_outside_the_session() {
        let good = packet(7, false, 0, FRAME_BYTES);
        assert!(!accepts(&good, PHONE, None), "no session");
        assert!(
            !accepts(&packet(8, false, 0, FRAME_BYTES), PHONE, Some(&session())),
            "other session id"
        );
        assert!(
            !accepts(&good, "192.168.1.99:5000", Some(&session())),
            "other host"
        );
    }

    #[test]
    fn drops_malformed_packets() {
        let s = Some(session());
        assert!(
            !accepts(&packet(7, false, 0, 500), PHONE, s.as_ref()),
            "short payload"
        );
        assert!(
            !accepts(&packet(7, false, 0, 0), PHONE, s.as_ref()),
            "empty unmuted"
        );
        assert!(
            !accepts(&packet(7, false, 1, FRAME_BYTES), PHONE, s.as_ref()),
            "Opus"
        );
        assert!(
            !accepts(&packet(7, false, 0, FRAME_BYTES)[..10], PHONE, s.as_ref()),
            "truncated header"
        );
    }

    #[tokio::test]
    async fn accepted_packet_records_its_time() {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = socket.local_addr().unwrap();
        let phone_addr = "127.0.0.1".parse().unwrap();
        let (_session, rx) = watch::channel(Some(Session {
            phone_addr,
            ..session()
        }));
        let last_packet = LastPacket::default();
        let buffer = Arc::new(Mutex::new(JitterBuffer::new()));
        tokio::spawn(receive(
            socket,
            rx,
            buffer,
            last_packet.clone(),
            Arc::default(),
        ));

        let phone = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        phone
            .send_to(&packet(7, false, 0, FRAME_BYTES), addr)
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while last_packet.lock().unwrap().is_none() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("packet never recorded");
    }

    #[test]
    fn meter_publishes_each_50_ms_window() {
        let (level, mut meter) = (Level::default(), LevelMeter::default());
        let square: Vec<i16> = (0..LEVEL_WINDOW)
            .map(|i| if i % 2 == 0 { 16384 } else { -16384 })
            .collect();
        meter.add(&square[..LEVEL_WINDOW - 1], &level);
        assert_eq!(level.get(), (0.0, 0.0), "no full window yet");
        meter.add(&square[..1], &level);
        assert_eq!(level.get(), (0.5, 0.5));
        meter.add(&[0; LEVEL_WINDOW], &level);
        assert_eq!(level.get(), (0.0, 0.0));
    }

    #[tokio::test]
    async fn level_follows_received_audio_and_mute() {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = socket.local_addr().unwrap();
        let session = Session {
            phone_addr: "127.0.0.1".parse().unwrap(),
            ..session()
        };
        let (_session, rx) = watch::channel(Some(session));
        let level = Arc::new(Level::default());
        let buffer = Arc::new(Mutex::new(JitterBuffer::new()));
        tokio::spawn(receive(
            socket,
            rx,
            buffer,
            LastPacket::default(),
            level.clone(),
        ));

        let phone = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        // Five 10 ms frames at half scale fill one 50 ms window, with no app recording.
        for sequence in 0..5 {
            let header = Header {
                muted: false,
                codec: 0,
                session_id: 7,
                sequence,
                timestamp: sequence * FRAME_SAMPLES as u32,
            };
            let mut p = header.encode().to_vec();
            p.extend(
                [16384i16; FRAME_SAMPLES]
                    .iter()
                    .flat_map(|s| s.to_le_bytes()),
            );
            phone.send_to(&p, addr).await.unwrap();
        }
        wait_for(|| level.get() == (0.5, 0.5)).await;
        phone.send_to(&packet(7, true, 0, 0), addr).await.unwrap();
        wait_for(|| level.get() == (0.0, 0.0)).await;
    }

    async fn wait_for(done: impl Fn() -> bool) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !done() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("condition not met in 5 s");
    }
}
