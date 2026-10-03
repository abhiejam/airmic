//! UDP audio receiver (docs/protocol.md §3): validates packets and feeds the jitter buffer.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use airmic_proto::{FRAME_BYTES, HEADER_LEN, Header};
use tokio::net::UdpSocket;
use tokio::sync::watch;
use tracing::{info, warn};

use crate::control::Session;
use crate::jitter::JitterBuffer;
use crate::sink::SharedBuffer;

/// When the receiver last accepted a packet from the active phone.
pub type LastPacket = Arc<Mutex<Option<Instant>>>;

pub async fn receive(
    socket: UdpSocket,
    session: watch::Receiver<Option<Session>>,
    buffer: SharedBuffer,
    last_packet: LastPacket,
) {
    let mut packet = [0u8; 2048];
    let mut buffer_session = None;
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
        *last_packet.lock().expect("last packet lock") = Some(Instant::now());
        let mut jb = buffer.lock().expect("jitter buffer lock");
        if buffer_session != Some(header.session_id) {
            *jb = JitterBuffer::new();
            buffer_session = Some(header.session_id);
        }
        jb.push(&header, payload, Instant::now());
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
        tokio::spawn(receive(socket, rx, buffer, last_packet.clone()));

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
}
