//! TCP control channel (docs/protocol.md §2): hello, ready, keepalive, mute, bye.
//! No pairing yet (D3.3): every phone that says hello gets a session, as with `--no-auth`.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use airmic_proto::{ErrorCode, Message, SAMPLE_RATE};
use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::time::{Instant, interval};
use tokio_util::codec::{Framed, LinesCodec};
use tracing::{info, warn};

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
}

pub async fn serve(listener: TcpListener, opts: ControlOptions, session: SessionTx) {
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(conn) => conn,
            Err(e) => {
                warn!("accept failed: {e}");
                continue;
            }
        };
        let (opts, session) = (opts.clone(), session.clone());
        tokio::spawn(async move {
            let mut session_id = None;
            if let Err(e) = handle_phone(stream, peer, &opts, &session, &mut session_id).await {
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
    session_id: &mut Option<u32>,
) -> anyhow::Result<()> {
    let mut conn = Framed::new(stream, LinesCodec::new_with_max_length(MAX_LINE));
    let mut ping = interval(PING_EVERY);
    let mut last_rx = Instant::now();
    let mut greeted = false;

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
                continue;
            }
        };
        last_rx = Instant::now();

        let msg = match Message::from_line(&line) {
            Ok(msg) => msg,
            Err(e) => return reject(&mut conn, ErrorCode::BadMessage, &e.to_string()).await,
        };
        let reply = match msg {
            Message::Hello { .. } if greeted => None,
            Message::Hello { v, phone_name, .. } => {
                greeted = true;
                if v != u32::from(airmic_proto::VERSION) {
                    let why = format!("protocol v{v} not supported");
                    return reject(&mut conn, ErrorCode::UnsupportedVersion, &why).await;
                }
                let new = Session {
                    id: rand::random::<u32>().max(1),
                    phone_addr: peer.ip().to_canonical(),
                    phone_name,
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
                    return reject(&mut conn, ErrorCode::Busy, "Another phone is connected").await;
                }
                *session_id = Some(id);
                info!(%peer, "session {id:#010x} ready");
                Some(Message::Ready {
                    session_id: id,
                    udp_port: opts.audio_port,
                    sample_rate: SAMPLE_RATE,
                })
            }
            _ if !greeted => {
                return reject(&mut conn, ErrorCode::BadMessage, "expected hello").await;
            }
            Message::Ping => Some(Message::Pong),
            Message::Mute { on } => match *session_id {
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
            Message::Bye => return Ok(()),
            _ => None,
        };
        if let Some(reply) = reply {
            conn.send(reply.to_line().trim_end()).await?;
        }
    }
}

async fn reject(
    conn: &mut Framed<TcpStream, LinesCodec>,
    code: ErrorCode,
    message: &str,
) -> anyhow::Result<()> {
    let err = Message::Error {
        code,
        message: message.to_string(),
    };
    conn.send(err.to_line().trim_end()).await?;
    anyhow::bail!("{code:?}: {message}")
}

#[cfg(test)]
mod tests {
    use super::*;

    type Phone = Framed<TcpStream, LinesCodec>;

    async fn start_daemon() -> (SocketAddr, watch::Receiver<Option<Session>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = watch::channel(None);
        tokio::spawn(serve(listener, ControlOptions { audio_port: 47801 }, tx));
        (addr, rx)
    }

    async fn connect(addr: SocketAddr) -> Phone {
        Framed::new(TcpStream::connect(addr).await.unwrap(), LinesCodec::new())
    }

    async fn send(phone: &mut Phone, json: &str) {
        phone.send(json).await.unwrap();
    }

    /// Returns the next message from the daemon, skipping its keepalive pings.
    async fn recv(phone: &mut Phone) -> Option<Message> {
        loop {
            let line = phone.next().await?.unwrap();
            match Message::from_line(&line).unwrap() {
                Message::Ping => continue,
                msg => return Some(msg),
            }
        }
    }

    const HELLO: &str = r#"{"type":"hello","v":1,"phone_id":"p1","phone_name":"iPhone"}"#;

    async fn open_session(addr: SocketAddr) -> (Phone, u32) {
        let mut phone = connect(addr).await;
        send(&mut phone, HELLO).await;
        match recv(&mut phone).await {
            Some(Message::Ready {
                session_id,
                udp_port: 47801,
                sample_rate: 48000,
            }) => (phone, session_id),
            other => panic!("expected ready, got {other:?}"),
        }
    }

    fn error_code(msg: Option<Message>) -> ErrorCode {
        match msg {
            Some(Message::Error { code, .. }) => code,
            other => panic!("expected error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn hello_opens_a_session() {
        let (addr, mut rx) = start_daemon().await;
        let (_phone, id) = open_session(addr).await;
        assert_ne!(id, 0);
        let session = rx.wait_for(Option::is_some).await.unwrap().clone().unwrap();
        assert_eq!((session.id, session.phone_name.as_str()), (id, "iPhone"));
    }

    #[tokio::test]
    async fn ping_gets_pong_and_auth_is_ignored() {
        let (addr, _rx) = start_daemon().await;
        let (mut phone, _) = open_session(addr).await;
        send(&mut phone, r#"{"type":"auth","token":"abc"}"#).await;
        send(&mut phone, r#"{"type":"ping"}"#).await;
        assert_eq!(recv(&mut phone).await, Some(Message::Pong));
    }

    #[tokio::test]
    async fn second_phone_is_busy() {
        let (addr, _rx) = start_daemon().await;
        let (_first, _) = open_session(addr).await;
        let mut second = connect(addr).await;
        send(&mut second, HELLO).await;
        assert_eq!(error_code(recv(&mut second).await), ErrorCode::Busy);
        assert_eq!(recv(&mut second).await, None);
    }

    #[tokio::test]
    async fn wrong_version_is_rejected() {
        let (addr, _rx) = start_daemon().await;
        let mut phone = connect(addr).await;
        send(&mut phone, &HELLO.replace(r#""v":1"#, r#""v":2"#)).await;
        let code = error_code(recv(&mut phone).await);
        assert_eq!(code, ErrorCode::UnsupportedVersion);
    }

    #[tokio::test]
    async fn invalid_or_early_messages_are_rejected() {
        let (addr, _rx) = start_daemon().await;
        for line in ["not json", r#"{"type":"mute","on":true}"#] {
            let mut phone = connect(addr).await;
            send(&mut phone, line).await;
            assert_eq!(error_code(recv(&mut phone).await), ErrorCode::BadMessage);
            assert_eq!(recv(&mut phone).await, None);
        }
    }

    #[tokio::test]
    async fn mute_and_bye_update_the_session() {
        let (addr, mut rx) = start_daemon().await;
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
        let (addr, mut rx) = start_daemon().await;
        let (mut phone, _) = open_session(addr).await;
        assert_eq!(recv(&mut phone).await, None);
        rx.wait_for(Option::is_none).await.unwrap();
    }
}
