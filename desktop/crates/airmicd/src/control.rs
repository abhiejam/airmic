//! TCP control channel (docs/protocol.md §2): hello, pairing, ready, keepalive, mute, bye.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use airmic_proto::{ErrorCode, Message, SAMPLE_RATE};
use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::time::{Instant, interval};
use tokio_util::codec::{Framed, LinesCodec};
use tracing::{info, warn};

use crate::jitter::Stats;
use crate::pairing::{PairOutcome, SharedPairing};
use crate::sink::SharedBuffer;

const MAX_LINE: usize = 64 * 1024;
const PING_EVERY: Duration = Duration::from_secs(2);
const PEER_TIMEOUT: Duration = Duration::from_secs(6);

/// The phone currently allowed to stream. The audio receiver drops packets for any other session.
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub id: u32,
    pub phone_addr: IpAddr,
    pub phone_name: String,
    pub muted: bool,
}

pub type SessionTx = watch::Sender<Option<Session>>;

#[derive(Debug, Clone)]
pub struct ControlOptions {
    pub audio_port: u16,
    /// Accept any phone without pairing (development).
    pub no_auth: bool,
}

type Conn = Framed<TcpStream, LinesCodec>;

pub async fn serve(
    listener: TcpListener,
    opts: ControlOptions,
    session: SessionTx,
    pairing: SharedPairing,
    buffer: SharedBuffer,
) {
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(conn) => conn,
            Err(e) => {
                warn!("accept failed: {e}");
                continue;
            }
        };
        let (opts, session) = (opts.clone(), session.clone());
        let (pairing, buffer) = (pairing.clone(), buffer.clone());
        tokio::spawn(async move {
            let mut session_id = None;
            let phone = handle_phone(
                stream,
                peer,
                &opts,
                &session,
                &pairing,
                &buffer,
                &mut session_id,
            );
            if let Err(e) = phone.await {
                info!(%peer, "control connection ended: {e}");
            }
            if let Some(id) = session_id {
                session.send_if_modified(|s| clear_if_owner(s, id));
                info!(%peer, "session {id:#010x} ended");
            }
        });
    }
}

fn clear_if_owner(s: &mut Option<Session>, id: u32) -> bool {
    let owned = s.as_ref().is_some_and(|s| s.id == id);
    if owned {
        *s = None;
    }
    owned
}

/// Runs one phone connection until it ends. Sets `session_id` once the phone owns the session,
/// so the caller can release it however this returns.
async fn handle_phone(
    stream: TcpStream,
    peer: SocketAddr,
    opts: &ControlOptions,
    session: &SessionTx,
    pairing: &SharedPairing,
    buffer: &SharedBuffer,
    session_id: &mut Option<u32>,
) -> anyhow::Result<()> {
    let mut conn = Framed::new(stream, LinesCodec::new_with_max_length(MAX_LINE));
    let mut ping = interval(PING_EVERY);
    let mut last_rx = Instant::now();
    // (phone_id, phone_name) from the first hello.
    let mut phone: Option<(String, String)> = None;
    let mut pair_required_sent = false;
    let mut ping_sent = None;
    let mut round_trip = None;
    let mut window_start: Option<Stats> = None;

    loop {
        let line = tokio::select! {
            line = conn.next() => match line {
                Some(line) => line?,
                None => return Ok(()),
            },
            _ = ping.tick() => {
                if last_rx.elapsed() >= PEER_TIMEOUT {
                    anyhow::bail!("peer timed out");
                }
                conn.send(Message::Ping.to_line().trim_end()).await?;
                ping_sent = Some(Instant::now());
                if session_id.is_some() {
                    let (stats, delay_ms) = {
                        let jb = buffer.lock().expect("jitter buffer lock");
                        (jb.stats(), jb.delay_ms())
                    };
                    // The first tick after `ready` only opens the window.
                    if let Some(start) = window_start.replace(stats) {
                        let msg = build_stats_message(start, stats, delay_ms, round_trip);
                        conn.send(msg.to_line().trim_end()).await?;
                    }
                }
                continue;
            }
        };
        last_rx = Instant::now();

        let msg = match Message::from_line(&line) {
            Ok(msg) => msg,
            Err(e) => return reject(&mut conn, ErrorCode::BadMessage, &e.to_string()).await,
        };
        let reply = match (msg, &phone) {
            (Message::Hello { .. }, Some(_)) => None,
            (
                Message::Hello {
                    v,
                    phone_id,
                    phone_name,
                },
                None,
            ) => {
                if v != u32::from(airmic_proto::VERSION) {
                    let why = format!("protocol v{v} not supported");
                    return reject(&mut conn, ErrorCode::UnsupportedVersion, &why).await;
                }
                let known = pairing.lock().expect("pairing lock").is_known(&phone_id);
                phone = Some((phone_id, phone_name.clone()));
                if opts.no_auth {
                    start_session(&mut conn, peer, &phone_name, opts, session, session_id).await?;
                    None
                } else if known {
                    // A known phone sends `auth` right after `hello`.
                    None
                } else {
                    pair_required_sent = true;
                    Some(request_pairing(pairing))
                }
            }
            (_, None) => {
                return reject(&mut conn, ErrorCode::BadMessage, "expected hello").await;
            }
            (Message::Ping, _) => Some(Message::Pong),
            (Message::Pong, _) => {
                round_trip = ping_sent.take().map(|sent: Instant| sent.elapsed());
                None
            }
            (Message::Auth { .. } | Message::Pair { .. }, _)
                if opts.no_auth || session_id.is_some() =>
            {
                None
            }
            (Message::Auth { token }, Some((phone_id, phone_name))) => {
                let valid = pairing
                    .lock()
                    .expect("pairing lock")
                    .is_token_valid(phone_id, &token);
                if valid {
                    let name = phone_name.clone();
                    start_session(&mut conn, peer, &name, opts, session, session_id).await?;
                    None
                } else if pair_required_sent {
                    None
                } else {
                    pair_required_sent = true;
                    Some(request_pairing(pairing))
                }
            }
            (Message::Pair { code }, Some((phone_id, phone_name))) => {
                let now = Instant::now().into_std();
                let outcome = pairing
                    .lock()
                    .expect("pairing lock")
                    .pair(&code, phone_id, phone_name, now)?;
                match outcome {
                    PairOutcome::Paired { token } => {
                        let name = phone_name.clone();
                        let paired = Message::Paired { token };
                        conn.send(paired.to_line().trim_end()).await?;
                        start_session(&mut conn, peer, &name, opts, session, session_id).await?;
                        None
                    }
                    PairOutcome::BadCode => Some(Message::Error {
                        code: ErrorCode::BadCode,
                        message: "Wrong or expired code".into(),
                    }),
                    PairOutcome::Locked => {
                        let why = "Too many wrong codes; show a new code on the computer";
                        return reject(&mut conn, ErrorCode::PairLocked, why).await;
                    }
                }
            }
            (Message::Mute { on }, _) => match *session_id {
                Some(id) => {
                    session.send_if_modified(|s| match s {
                        Some(s) if s.id == id && s.muted != on => {
                            s.muted = on;
                            true
                        }
                        _ => false,
                    });
                    None
                }
                None => return reject(&mut conn, ErrorCode::BadMessage, "mute before ready").await,
            },
            (Message::Bye, _) => return Ok(()),
            _ => None,
        };
        if let Some(reply) = reply {
            conn.send(reply.to_line().trim_end()).await?;
        }
    }
}

/// Returns `stats` for the window from `start` to `end` (docs/protocol.md §2.5).
/// The receiver replaces the jitter buffer when a new session's audio starts, so counters that
/// went backwards mean the window started at zero.
fn build_stats_message(
    start: Stats,
    end: Stats,
    delay_ms: f64,
    round_trip: Option<Duration>,
) -> Message {
    let start = if end.received < start.received {
        Stats::default()
    } else {
        start
    };
    let lost = end.lost.saturating_sub(start.lost);
    let expected = end.received.saturating_sub(start.received) + lost;
    let loss_pct = if expected == 0 {
        0.0
    } else {
        100.0 * lost as f64 / expected as f64
    };
    let half_rtt_ms = round_trip.map_or(0.0, |rtt| rtt.as_secs_f64() * 1e3 / 2.0);
    Message::Stats {
        loss_pct,
        jitter_ms: end.jitter_ms,
        latency_ms: half_rtt_ms + delay_ms,
    }
}

/// Returns `pair_required`, first issuing a code if none is valid.
/// Until the desktop app shows codes (D3.7), the phone's request is what brings one up. A lockout
/// still needs a deliberate new code, so a guesser cannot reconnect for fresh attempts.
fn request_pairing(pairing: &SharedPairing) -> Message {
    let mut pairing = pairing.lock().expect("pairing lock");
    let now = Instant::now().into_std();
    if pairing.current_code(now).is_none() && !pairing.is_locked() {
        pairing.regenerate_code(now);
    }
    Message::PairRequired
}

/// Claims the session for this phone and sends `ready`, or rejects it with `busy`.
async fn start_session(
    conn: &mut Conn,
    peer: SocketAddr,
    phone_name: &str,
    opts: &ControlOptions,
    session: &SessionTx,
    session_id: &mut Option<u32>,
) -> anyhow::Result<()> {
    let new = Session {
        id: rand::random::<u32>().max(1),
        phone_addr: peer.ip().to_canonical(),
        phone_name: phone_name.to_string(),
        muted: false,
    };
    let id = new.id;
    let claimed = session.send_if_modified(|s| {
        if s.is_some() {
            return false;
        }
        *s = Some(new.clone());
        true
    });
    if !claimed {
        return reject(conn, ErrorCode::Busy, "Another phone is connected").await;
    }
    *session_id = Some(id);
    info!(%peer, "session {id:#010x} ready");
    let ready = Message::Ready {
        session_id: id,
        udp_port: opts.audio_port,
        sample_rate: SAMPLE_RATE,
    };
    conn.send(ready.to_line().trim_end()).await?;
    Ok(())
}

async fn reject(conn: &mut Conn, code: ErrorCode, message: &str) -> anyhow::Result<()> {
    let err = Message::Error {
        code,
        message: message.to_string(),
    };
    conn.send(err.to_line().trim_end()).await?;
    anyhow::bail!("{code:?}: {message}")
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::jitter::JitterBuffer;
    use crate::pairing::Pairing;
    use crate::pairing::tests::temp_store;

    type Phone = Framed<TcpStream, LinesCodec>;

    async fn start_daemon(
        no_auth: bool,
    ) -> (SocketAddr, watch::Receiver<Option<Session>>, SharedPairing) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = watch::channel(None);
        let pairing = Arc::new(Mutex::new(Pairing::load(temp_store()).unwrap()));
        let opts = ControlOptions {
            audio_port: 47801,
            no_auth,
        };
        let buffer = Arc::new(Mutex::new(JitterBuffer::new()));
        tokio::spawn(serve(listener, opts, tx, pairing.clone(), buffer));
        (addr, rx, pairing)
    }

    async fn connect(addr: SocketAddr) -> Phone {
        Framed::new(TcpStream::connect(addr).await.unwrap(), LinesCodec::new())
    }

    async fn send(phone: &mut Phone, json: &str) {
        phone.send(json).await.unwrap();
    }

    /// Returns the next message from the daemon, skipping its periodic pings and stats.
    async fn recv(phone: &mut Phone) -> Option<Message> {
        loop {
            let line = phone.next().await?.unwrap();
            match Message::from_line(&line).unwrap() {
                Message::Ping | Message::Stats { .. } => continue,
                msg => return Some(msg),
            }
        }
    }

    const HELLO: &str = r#"{"type":"hello","v":1,"phone_id":"p1","phone_name":"iPhone"}"#;

    async fn open_session(addr: SocketAddr) -> (Phone, u32) {
        let mut phone = connect(addr).await;
        send(&mut phone, HELLO).await;
        let session_id = expect_ready(recv(&mut phone).await);
        (phone, session_id)
    }

    fn expect_ready(msg: Option<Message>) -> u32 {
        match msg {
            Some(Message::Ready {
                session_id,
                udp_port: 47801,
                sample_rate: 48000,
            }) => session_id,
            other => panic!("expected ready, got {other:?}"),
        }
    }

    fn error_code(msg: Option<Message>) -> ErrorCode {
        match msg {
            Some(Message::Error { code, .. }) => code,
            other => panic!("expected error, got {other:?}"),
        }
    }

    fn pair_line(code: &str) -> String {
        format!(r#"{{"type":"pair","code":"{code}"}}"#)
    }

    fn auth_line(token: &str) -> String {
        format!(r#"{{"type":"auth","token":"{token}"}}"#)
    }

    fn now() -> std::time::Instant {
        Instant::now().into_std()
    }

    /// Connects as a new phone and asserts `pair_required`.
    async fn connect_unpaired(addr: SocketAddr) -> Phone {
        let mut phone = connect(addr).await;
        send(&mut phone, HELLO).await;
        assert_eq!(recv(&mut phone).await, Some(Message::PairRequired));
        phone
    }

    /// Pairs a new phone with the code the daemon issued, up to `ready`. Returns its token.
    async fn pair_phone(addr: SocketAddr, pairing: &SharedPairing) -> (Phone, String) {
        let mut phone = connect_unpaired(addr).await;
        let code = pairing
            .lock()
            .unwrap()
            .current_code(now())
            .unwrap()
            .to_string();
        send(&mut phone, &pair_line(&code)).await;
        let Some(Message::Paired { token }) = recv(&mut phone).await else {
            panic!("expected paired");
        };
        expect_ready(recv(&mut phone).await);
        (phone, token)
    }

    #[tokio::test]
    async fn hello_opens_a_session() {
        let (addr, mut rx, _) = start_daemon(true).await;
        let (_phone, id) = open_session(addr).await;
        assert_ne!(id, 0);
        let session = rx.wait_for(Option::is_some).await.unwrap().clone().unwrap();
        assert_eq!((session.id, session.phone_name.as_str()), (id, "iPhone"));
    }

    #[tokio::test]
    async fn ping_gets_pong_and_auth_is_ignored() {
        let (addr, _rx, _) = start_daemon(true).await;
        let (mut phone, _) = open_session(addr).await;
        send(&mut phone, &auth_line("abc")).await;
        send(&mut phone, r#"{"type":"ping"}"#).await;
        assert_eq!(recv(&mut phone).await, Some(Message::Pong));
    }

    #[tokio::test]
    async fn second_phone_is_busy() {
        let (addr, _rx, _) = start_daemon(true).await;
        let (_first, _) = open_session(addr).await;
        let mut second = connect(addr).await;
        send(&mut second, HELLO).await;
        assert_eq!(error_code(recv(&mut second).await), ErrorCode::Busy);
        assert_eq!(recv(&mut second).await, None);
    }

    #[tokio::test]
    async fn wrong_version_is_rejected() {
        let (addr, _rx, _) = start_daemon(true).await;
        let mut phone = connect(addr).await;
        send(&mut phone, &HELLO.replace(r#""v":1"#, r#""v":2"#)).await;
        let code = error_code(recv(&mut phone).await);
        assert_eq!(code, ErrorCode::UnsupportedVersion);
    }

    #[tokio::test]
    async fn invalid_or_early_messages_are_rejected() {
        let (addr, _rx, _) = start_daemon(true).await;
        for line in ["not json", r#"{"type":"mute","on":true}"#] {
            let mut phone = connect(addr).await;
            send(&mut phone, line).await;
            assert_eq!(error_code(recv(&mut phone).await), ErrorCode::BadMessage);
            assert_eq!(recv(&mut phone).await, None);
        }
    }

    #[tokio::test]
    async fn mute_and_bye_update_the_session() {
        let (addr, mut rx, _) = start_daemon(true).await;
        let (mut phone, _) = open_session(addr).await;
        send(&mut phone, r#"{"type":"mute","on":true}"#).await;
        rx.wait_for(|s| s.as_ref().is_some_and(|s| s.muted))
            .await
            .unwrap();
        send(&mut phone, r#"{"type":"bye"}"#).await;
        rx.wait_for(Option::is_none).await.unwrap();
        // The session is free again for the next phone.
        open_session(addr).await;
    }

    #[tokio::test(start_paused = true)]
    async fn silent_phone_times_out() {
        let (addr, mut rx, _) = start_daemon(true).await;
        let (mut phone, _) = open_session(addr).await;
        assert_eq!(recv(&mut phone).await, None);
        rx.wait_for(Option::is_none).await.unwrap();
    }

    #[tokio::test]
    async fn new_phone_pairs_then_reconnects_with_its_token() {
        let (addr, mut rx, pairing) = start_daemon(false).await;
        let (mut phone, token) = pair_phone(addr, &pairing).await;
        send(&mut phone, r#"{"type":"bye"}"#).await;
        rx.wait_for(Option::is_none).await.unwrap();

        let mut phone = connect(addr).await;
        send(&mut phone, HELLO).await;
        send(&mut phone, &auth_line(&token)).await;
        expect_ready(recv(&mut phone).await);
        send(&mut phone, r#"{"type":"bye"}"#).await;
        rx.wait_for(Option::is_none).await.unwrap();

        let mut phone = connect(addr).await;
        send(&mut phone, HELLO).await;
        send(&mut phone, &auth_line(&"0".repeat(32))).await;
        assert_eq!(recv(&mut phone).await, Some(Message::PairRequired));
    }

    #[tokio::test]
    async fn bad_code_keeps_the_connection_open() {
        let (addr, _rx, pairing) = start_daemon(false).await;
        let mut phone = connect_unpaired(addr).await;
        let code = pairing.lock().unwrap().regenerate_code(now());
        let wrong = if code == "0000" { "0001" } else { "0000" };
        send(&mut phone, &pair_line(wrong)).await;
        assert_eq!(error_code(recv(&mut phone).await), ErrorCode::BadCode);
        send(&mut phone, &pair_line(&code)).await;
        assert!(matches!(
            recv(&mut phone).await,
            Some(Message::Paired { .. })
        ));
        expect_ready(recv(&mut phone).await);
    }

    #[tokio::test]
    async fn fifth_wrong_code_locks_pairing() {
        let (addr, _rx, pairing) = start_daemon(false).await;
        let mut phone = connect_unpaired(addr).await;
        let code = pairing.lock().unwrap().regenerate_code(now());
        let wrong = if code == "0000" { "0001" } else { "0000" };
        for _ in 0..4 {
            send(&mut phone, &pair_line(wrong)).await;
            assert_eq!(error_code(recv(&mut phone).await), ErrorCode::BadCode);
        }
        send(&mut phone, &pair_line(wrong)).await;
        assert_eq!(error_code(recv(&mut phone).await), ErrorCode::PairLocked);
        assert_eq!(recv(&mut phone).await, None);
        // Reconnecting does not bring a fresh code.
        let mut phone = connect_unpaired(addr).await;
        send(&mut phone, &pair_line(&code)).await;
        assert_eq!(error_code(recv(&mut phone).await), ErrorCode::PairLocked);
    }

    #[tokio::test]
    async fn expired_code_is_a_bad_code() {
        let (addr, _rx, pairing) = start_daemon(false).await;
        let mut phone = connect_unpaired(addr).await;
        let issued = now().checked_sub(Duration::from_secs(121)).unwrap();
        let code = pairing.lock().unwrap().regenerate_code(issued);
        send(&mut phone, &pair_line(&code)).await;
        assert_eq!(error_code(recv(&mut phone).await), ErrorCode::BadCode);
    }

    #[tokio::test]
    async fn forgotten_phone_must_pair_again() {
        let (addr, mut rx, pairing) = start_daemon(false).await;
        let (mut phone, token) = pair_phone(addr, &pairing).await;
        send(&mut phone, r#"{"type":"bye"}"#).await;
        rx.wait_for(Option::is_none).await.unwrap();
        assert!(pairing.lock().unwrap().forget("p1").unwrap());

        let mut phone = connect_unpaired(addr).await;
        send(&mut phone, &auth_line(&token)).await;
        send(&mut phone, r#"{"type":"ping"}"#).await;
        // The stale token gets no second `pair_required`.
        assert_eq!(recv(&mut phone).await, Some(Message::Pong));
    }

    #[tokio::test]
    async fn ready_session_gets_stats_with_every_ping() {
        let (addr, _rx, _) = start_daemon(true).await;
        let (mut phone, _) = open_session(addr).await;
        // Real time (about 4 s): the paused clock jumps while bytes are in flight and times out the phone.
        let mut pings_since_stats = Vec::new();
        let mut pings = 0;
        while pings_since_stats.len() < 2 {
            let line = phone.next().await.unwrap().unwrap();
            match Message::from_line(&line).unwrap() {
                Message::Ping => {
                    pings += 1;
                    send(&mut phone, r#"{"type":"pong"}"#).await;
                }
                msg => {
                    let idle = matches!(
                        msg,
                        Message::Stats {
                            loss_pct: 0.0,
                            jitter_ms: 5.0,
                            latency_ms,
                        } if latency_ms < 5.0
                    );
                    assert!(idle, "expected idle stats, got {msg:?}");
                    pings_since_stats.push(std::mem::take(&mut pings));
                }
            }
        }
        assert_eq!(pings_since_stats[1], 1, "one stats per ping");
    }

    #[test]
    fn stats_cover_only_the_last_window() {
        let stats = |received, lost| Stats {
            received,
            lost,
            jitter_ms: 3.0,
            ..Stats::default()
        };
        let rtt = Some(Duration::from_millis(30));
        let msg = build_stats_message(stats(100, 50), stats(290, 60), 40.0, rtt);
        let expected = Message::Stats {
            loss_pct: 5.0,
            jitter_ms: 3.0,
            latency_ms: 55.0,
        };
        assert_eq!(msg, expected);
        // A new session replaced the buffer, so its counters restarted at zero.
        let msg = build_stats_message(stats(500, 20), stats(95, 5), 40.0, rtt);
        assert_eq!(msg, expected);
        let msg = build_stats_message(stats(7, 1), stats(7, 1), 0.0, None);
        assert!(matches!(
            msg,
            Message::Stats {
                loss_pct: 0.0,
                latency_ms: 0.0,
                ..
            }
        ));
    }
}
