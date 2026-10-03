//! Daemon ↔ desktop app IPC (docs/ipc.md): newline-delimited JSON-RPC 2.0 over a local socket.

use std::collections::HashSet;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Notify, broadcast, watch};
use tokio::time::{Instant, interval};
use tokio_util::codec::{Framed, LinesCodec, LinesCodecError};
use tracing::{info, warn};

use crate::config::Config;
use crate::control::{Session, StreamStats};
use crate::receiver::{LastPacket, Level};
use crate::sink::DefaultSource;

const MAX_LINE: usize = 64 * 1024;
const POLL_EVERY: Duration = Duration::from_secs(1);
const STATUS_EVERY: Duration = Duration::from_secs(2);
const LEVEL_EVERY: Duration = Duration::from_millis(50);
const LEVEL_STALE_AFTER: Duration = Duration::from_millis(200);
const FLOWING_WITHIN: Duration = Duration::from_secs(2);

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const INTERNAL_ERROR: i64 = -32603;
const INVALID_SETTING: i64 = 2;
const UNAVAILABLE: i64 = 3;

/// A local listener for app connections: a Unix socket now, a Windows named pipe later (PRD §10).
pub trait IpcListener: Send + 'static {
    type Stream: AsyncRead + AsyncWrite + Unpin + Send + 'static;
    fn accept(&mut self) -> impl Future<Output = std::io::Result<Self::Stream>> + Send;
}

pub struct UnixSocket(UnixListener);

/// Returns `$XDG_RUNTIME_DIR/airmic.sock`.
pub fn socket_path() -> anyhow::Result<PathBuf> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR is not set")?;
    Ok(Path::new(&dir).join("airmic.sock"))
}

impl UnixSocket {
    /// Binds `path` with mode 0600. A socket file nobody listens on is left from a crash, so it is replaced.
    pub fn bind(path: &Path) -> anyhow::Result<UnixSocket> {
        anyhow::ensure!(
            std::os::unix::net::UnixStream::connect(path).is_err(),
            "{} is in use: is airmicd already running?",
            path.display()
        );
        match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err(e).with_context(|| format!("removing {}", path.display()));
            }
            _ => {}
        }
        let listener =
            UnixListener::bind(path).with_context(|| format!("binding {}", path.display()))?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        Ok(UnixSocket(listener))
    }
}

impl IpcListener for UnixSocket {
    type Stream = UnixStream;

    async fn accept(&mut self) -> std::io::Result<UnixStream> {
        Ok(self.0.accept().await?.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Topic {
    Status,
    Level,
    Transcript,
}

#[derive(Clone)]
struct Notification {
    topic: Topic,
    line: Arc<str>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Status {
    state: State,
    phone: Option<Phone>,
    stats: Option<StreamStats>,
    audio_flowing: bool,
    is_default_source: bool,
    version: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum State {
    Idle,
    Streaming,
    Muted,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct Phone {
    id: String,
    name: String,
    addr: String,
}

struct RpcError {
    code: i64,
    message: String,
}

fn rpc_error(code: i64, message: impl ToString) -> RpcError {
    RpcError {
        code,
        message: message.to_string(),
    }
}

#[derive(Deserialize)]
enum Version {
    #[serde(rename = "2.0")]
    V2,
}

#[derive(Deserialize)]
struct Request {
    #[serde(rename = "jsonrpc")]
    _version: Version,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Deserialize)]
struct SubscribeParams {
    topics: Vec<Topic>,
}

/// State shared by every app connection.
pub struct Ipc {
    session: watch::Receiver<Option<Session>>,
    level: Arc<Level>,
    last_packet: LastPacket,
    default_source: Box<dyn DefaultSource>,
    config_path: PathBuf,
    config: Mutex<Config>,
    status: watch::Sender<Status>,
    notifications: broadcast::Sender<Notification>,
    refresh_status: Notify,
}

impl Ipc {
    pub fn new(
        session: watch::Receiver<Option<Session>>,
        level: Arc<Level>,
        last_packet: LastPacket,
        default_source: Box<dyn DefaultSource>,
        config_path: PathBuf,
        config: Config,
    ) -> Arc<Ipc> {
        let ipc = Arc::new(Ipc {
            session,
            level,
            last_packet,
            default_source,
            config_path,
            config: Mutex::new(config),
            status: watch::Sender::new(idle_status()),
            notifications: broadcast::channel(64).0,
            refresh_status: Notify::new(),
        });
        ipc.status.send_replace(ipc.build_status(false));
        ipc
    }

    fn build_status(&self, is_default_source: bool) -> Status {
        let last_packet = *self.last_packet.lock().expect("last packet lock");
        let audio_flowing = last_packet.is_some_and(|t| t.elapsed() < FLOWING_WITHIN);
        let Some(session) = self.session.borrow().clone() else {
            return Status {
                audio_flowing,
                is_default_source,
                ..idle_status()
            };
        };
        Status {
            state: if session.muted {
                State::Muted
            } else {
                State::Streaming
            },
            phone: Some(Phone {
                id: session.phone_id,
                name: session.phone_name,
                addr: session.phone_addr.to_string(),
            }),
            // Null until the first 2 s stats window closes.
            stats: session.stats,
            audio_flowing,
            is_default_source,
            version: env!("CARGO_PKG_VERSION"),
        }
    }

    fn notify(&self, topic: Topic, params: impl Serialize) {
        let line = json!({"jsonrpc": "2.0", "method": topic, "params": params}).to_string();
        let _ = self.notifications.send(Notification {
            topic,
            line: line.into(),
        });
    }

    /// Returns the reply line for one request line, or `None` for a JSON-RPC notification.
    fn handle_line(&self, line: &str, topics: &mut HashSet<Topic>) -> Option<String> {
        let value: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(e) => return Some(reply(Value::Null, Err(rpc_error(PARSE_ERROR, e)))),
        };
        let id = value.get("id").cloned().unwrap_or(Value::Null);
        let request = match serde_json::from_value::<Request>(value) {
            Ok(request) => request,
            Err(e) => return Some(reply(id, Err(rpc_error(INVALID_REQUEST, e)))),
        };
        let result = self.call(&request.method, request.params, topics);
        request.id.map(|id| reply(id, result))
    }

    fn call(
        &self,
        method: &str,
        params: Value,
        topics: &mut HashSet<Topic>,
    ) -> Result<Value, RpcError> {
        match method {
            "status" => Ok(json!(*self.status.borrow())),
            "get_settings" => Ok(json!(*self.config.lock().expect("config lock"))),
            "set_settings" => self.set_settings(params),
            "make_default" => {
                let result = tokio::task::block_in_place(|| self.default_source.make_default());
                self.refresh_status.notify_one();
                result.map_err(|e| rpc_error(INTERNAL_ERROR, format!("{e:#}")))?;
                Ok(Value::Null)
            }
            "subscribe" => {
                let p: SubscribeParams =
                    serde_json::from_value(params).map_err(|e| rpc_error(INVALID_PARAMS, e))?;
                if p.topics.contains(&Topic::Transcript) {
                    return Err(rpc_error(UNAVAILABLE, "transcription is not built yet"));
                }
                *topics = p.topics.into_iter().collect();
                Ok(Value::Null)
            }
            _ => Err(rpc_error(METHOD_NOT_FOUND, format!("no method {method}"))),
        }
    }

    /// Merges `params` into the config, validates it and saves `config.toml`.
    fn set_settings(&self, params: Value) -> Result<Value, RpcError> {
        if !params.is_object() {
            return Err(rpc_error(INVALID_PARAMS, "expected an object"));
        }
        let mut config = self.config.lock().expect("config lock");
        let mut merged = json!(*config);
        merge_json(&mut merged, params);
        let new: Config =
            serde_json::from_value(merged).map_err(|e| rpc_error(INVALID_SETTING, e))?;
        if new.control_port < 1024 || new.audio_port < 1024 {
            return Err(rpc_error(INVALID_SETTING, "ports must be 1024 to 65535"));
        }
        new.save(&self.config_path)
            .map_err(|e| rpc_error(INTERNAL_ERROR, format!("{e:#}")))?;
        *config = new;
        Ok(json!(*config))
    }
}

fn idle_status() -> Status {
    Status {
        state: State::Idle,
        phone: None,
        stats: None,
        audio_flowing: false,
        is_default_source: false,
        version: env!("CARGO_PKG_VERSION"),
    }
}

fn reply(id: Value, result: Result<Value, RpcError>) -> String {
    match result {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err(e) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": e.code, "message": e.message},
        }),
    }
    .to_string()
}

/// Overwrites `base` with `changes`, recursing into objects so nested settings merge too.
fn merge_json(base: &mut Value, changes: Value) {
    match (base, changes) {
        (Value::Object(base), Value::Object(changes)) => {
            for (key, value) in changes {
                merge_json(base.entry(key).or_insert(Value::Null), value);
            }
        }
        (base, changes) => *base = changes,
    }
}

/// Accepts app connections until the listener fails.
pub async fn serve<L: IpcListener>(mut listener: L, ipc: Arc<Ipc>) {
    tokio::spawn(publish_status(ipc.clone()));
    tokio::spawn(publish_level(ipc.clone()));
    loop {
        match listener.accept().await {
            Ok(stream) => {
                let ipc = ipc.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_client(stream, &ipc).await {
                        info!("IPC client ended: {e}");
                    }
                });
            }
            Err(e) => warn!("IPC accept failed: {e}"),
        }
    }
}

/// Rebuilds the status on every session change and once a second. Sends the `status`
/// notification when anything but `stats` changed, and every 2 s while a phone is connected.
async fn publish_status(ipc: Arc<Ipc>) {
    let mut session = ipc.session.clone();
    let mut tick = interval(POLL_EVERY);
    let mut last_sent = Instant::now();
    loop {
        tokio::select! {
            _ = tick.tick() => {}
            _ = ipc.refresh_status.notified() => {}
            changed = session.changed() => if changed.is_err() { return },
        }
        let is_default = tokio::task::block_in_place(|| ipc.default_source.is_default());
        let new = ipc.build_status(is_default);
        let old = ipc.status.send_replace(new.clone());
        let changed = Status { stats: None, ..old }
            != Status {
                stats: None,
                ..new.clone()
            };
        if changed || (new.stats.is_some() && last_sent.elapsed() >= STATUS_EVERY) {
            last_sent = Instant::now();
            ipc.notify(Topic::Status, new);
        }
    }
}

/// Sends the `level` notification every 50 ms while a phone streams unmuted.
async fn publish_level(ipc: Arc<Ipc>) {
    let mut tick = interval(LEVEL_EVERY);
    loop {
        tick.tick().await;
        if ipc.session.borrow().as_ref().is_some_and(|s| !s.muted) {
            // Packets stopped (Wi-Fi drop): show silence instead of freezing the meter.
            let last_packet = *ipc.last_packet.lock().expect("last packet lock");
            let fresh = last_packet.is_some_and(|t| t.elapsed() < LEVEL_STALE_AFTER);
            let (rms, peak) = if fresh { ipc.level.get() } else { (0.0, 0.0) };
            ipc.notify(Topic::Level, json!({"rms": rms, "peak": peak}));
        }
    }
}

async fn handle_client<S: AsyncRead + AsyncWrite + Unpin>(
    stream: S,
    ipc: &Ipc,
) -> anyhow::Result<()> {
    let mut conn = Framed::new(stream, LinesCodec::new_with_max_length(MAX_LINE));
    let mut notifications = ipc.notifications.subscribe();
    let mut topics = HashSet::new();
    loop {
        tokio::select! {
            line = conn.next() => {
                let reply = match line {
                    None => return Ok(()),
                    Some(Ok(line)) => ipc.handle_line(&line, &mut topics),
                    // `Framed` ends the stream after a decode error, so the client is dropped after this reply.
                    Some(Err(LinesCodecError::MaxLineLengthExceeded)) => {
                        Some(reply(Value::Null, Err(rpc_error(INVALID_REQUEST, "line over 64 KiB"))))
                    }
                    Some(Err(e)) => return Err(e.into()),
                };
                if let Some(reply) = reply {
                    conn.send(reply).await?;
                }
            }
            notification = notifications.recv() => match notification {
                Ok(n) if topics.contains(&n.topic) => conn.send(&*n.line).await?,
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => return Ok(()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    type Client = Framed<UnixStream, LinesCodec>;

    struct FakeDefault(Arc<AtomicBool>);

    impl DefaultSource for FakeDefault {
        fn is_default(&self) -> bool {
            self.0.load(Ordering::SeqCst)
        }
        fn make_default(&self) -> anyhow::Result<()> {
            self.0.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    struct Daemon {
        dir: tempfile::TempDir,
        session: watch::Sender<Option<Session>>,
        level: Arc<Level>,
        last_packet: LastPacket,
    }

    async fn start_daemon() -> Daemon {
        let dir = tempfile::tempdir().unwrap();
        let listener = UnixSocket::bind(&dir.path().join("airmic.sock")).unwrap();
        let (session, session_rx) = watch::channel(None);
        let (level, last_packet) = (Arc::new(Level::default()), LastPacket::default());
        let ipc = Ipc::new(
            session_rx,
            level.clone(),
            last_packet.clone(),
            Box::new(FakeDefault(Arc::default())),
            dir.path().join("config/config.toml"),
            Config::default(),
        );
        tokio::spawn(serve(listener, ipc));
        Daemon {
            dir,
            session,
            level,
            last_packet,
        }
    }

    impl Daemon {
        async fn connect(&self) -> Client {
            let stream = UnixStream::connect(self.dir.path().join("airmic.sock"));
            Framed::new(stream.await.unwrap(), LinesCodec::new())
        }

        fn start_session(&self, muted: bool) {
            self.session.send_replace(Some(Session {
                id: 7,
                phone_id: "p1".into(),
                phone_addr: "192.168.1.20".parse().unwrap(),
                phone_name: "iPhone".into(),
                muted,
                stats: None,
            }));
        }
    }

    async fn recv(client: &mut Client) -> Value {
        let line = tokio::time::timeout(Duration::from_secs(5), client.next()).await;
        serde_json::from_str(&line.expect("no reply in 5 s").unwrap().unwrap()).unwrap()
    }

    /// Sends a request with id 1 and returns the next line, which must be its reply.
    async fn call(client: &mut Client, method: &str, params: Value) -> Value {
        let request = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        client.send(request.to_string()).await.unwrap();
        let reply = recv(client).await;
        assert_eq!(reply["id"], 1, "expected the reply, got {reply}");
        reply
    }

    async fn recv_status(client: &mut Client, wanted: impl Fn(&Value) -> bool) -> Value {
        loop {
            let msg = recv(client).await;
            if msg["method"] == "status" && wanted(&msg["params"]) {
                return msg["params"].clone();
            }
        }
    }

    fn error_code(reply: &Value) -> i64 {
        reply["error"]["code"]
            .as_i64()
            .unwrap_or_else(|| panic!("no error in {reply}"))
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn socket_is_private_and_replaces_a_stale_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("airmic.sock");
        std::fs::write(&path, "stale").unwrap();
        let _listener = UnixSocket::bind(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        assert!(
            UnixSocket::bind(&path).is_err(),
            "a live socket is not replaced"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn settings_merge_and_save() {
        let daemon = start_daemon().await;
        let mut app = daemon.connect().await;
        let changes = json!({"audio_port": 47841, "transcription": {"model": "small.en"}});
        let result = call(&mut app, "set_settings", changes).await["result"].clone();
        assert_eq!(
            result,
            json!({
                "set_default_source": true,
                "control_port": airmic_proto::CONTROL_PORT,
                "audio_port": 47841,
                "transcription": {"enabled": false, "model": "small.en"},
            })
        );
        assert_eq!(
            call(&mut app, "get_settings", Value::Null).await["result"],
            result
        );
        let saved = Config::load(&daemon.dir.path().join("config/config.toml")).unwrap();
        assert_eq!(json!(saved), result);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn bad_requests_get_error_codes() {
        let daemon = start_daemon().await;
        let mut app = daemon.connect().await;
        let batch = r#"[{"jsonrpc":"2.0","id":1,"method":"get_settings"}]"#;
        let no_version = r#"{"id":1,"method":"get_settings"}"#;
        for (line, code) in [
            ("{", PARSE_ERROR),
            (batch, INVALID_REQUEST),
            (no_version, INVALID_REQUEST),
        ] {
            app.send(line).await.unwrap();
            assert_eq!(error_code(&recv(&mut app).await), code, "{line}");
        }
        // A JSON-RPC notification gets no reply, so the next line is the reply to `call`.
        app.send(r#"{"jsonrpc":"2.0","method":"get_settings"}"#)
            .await
            .unwrap();
        assert_eq!(
            error_code(&call(&mut app, "nope", Value::Null).await),
            METHOD_NOT_FOUND
        );
        for (method, params, code) in [
            ("set_settings", json!([1]), INVALID_PARAMS),
            ("subscribe", json!({"topics": ["weather"]}), INVALID_PARAMS),
            ("subscribe", json!({"topics": ["transcript"]}), UNAVAILABLE),
        ] {
            let reply = call(&mut app, method, params.clone()).await;
            assert_eq!(error_code(&reply), code, "{method} {params}");
        }
        for params in [
            json!({"control_port": 0}),
            json!({"audio_port": 70000}),
            json!({"colour": "red"}),
            json!({"transcription": {"model": "large"}}),
        ] {
            let reply = call(&mut app, "set_settings", params.clone()).await;
            assert_eq!(error_code(&reply), INVALID_SETTING, "{params}");
        }
        // Rejected settings change nothing.
        let settings = call(&mut app, "get_settings", Value::Null).await;
        assert_eq!(settings["result"], json!(Config::default()));

        app.send("x".repeat(MAX_LINE + 1)).await.unwrap();
        assert_eq!(error_code(&recv(&mut app).await), INVALID_REQUEST);
        assert!(
            app.next().await.is_none(),
            "an over-long line ends the connection"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn status_follows_the_session() {
        let daemon = start_daemon().await;
        let mut app = daemon.connect().await;
        let idle = call(&mut app, "status", Value::Null).await["result"].clone();
        assert_eq!(
            (idle["state"].as_str(), &idle["stats"]),
            (Some("idle"), &Value::Null)
        );
        assert_eq!(idle["version"], env!("CARGO_PKG_VERSION"));

        call(&mut app, "subscribe", json!({"topics": ["status"]})).await;
        daemon.start_session(true);
        let status = recv_status(&mut app, |s| s["state"] == "muted").await;
        assert_eq!(status["phone"]["id"], "p1");
        assert_eq!(status["phone"]["name"], "iPhone");
        assert_eq!(status["phone"]["addr"], "192.168.1.20");
        assert_eq!(status["stats"], Value::Null, "no stats window yet");
        assert_eq!(status["audio_flowing"], false);

        // The control server publishes each window it sends the phone; IPC serves the same numbers.
        let window = StreamStats {
            loss_pct: 5.0,
            jitter_ms: 3.0,
            latency_ms: 55.0,
        };
        daemon
            .session
            .send_modify(|s| s.as_mut().unwrap().stats = Some(window));
        let status = recv_status(&mut app, |s| !s["stats"].is_null()).await;
        assert_eq!(status["stats"], json!(window));

        *daemon.last_packet.lock().unwrap() = Some(std::time::Instant::now());
        recv_status(&mut app, |s| s["audio_flowing"] == true).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn level_is_sent_while_streaming() {
        let daemon = start_daemon().await;
        let mut app = daemon.connect().await;
        call(&mut app, "subscribe", json!({"topics": ["level"]})).await;
        daemon.level.set(0.25, 0.5);
        daemon.start_session(false);
        let level = recv(&mut app).await;
        assert_eq!(level["method"], "level");
        assert_eq!(
            level["params"],
            json!({"rms": 0.0, "peak": 0.0}),
            "no packets yet"
        );
        *daemon.last_packet.lock().unwrap() = Some(std::time::Instant::now());
        loop {
            let level = recv(&mut app).await;
            if level["params"] == json!({"rms": 0.25, "peak": 0.5}) {
                break;
            }
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn make_default_updates_the_status() {
        let daemon = start_daemon().await;
        let mut app = daemon.connect().await;
        call(&mut app, "subscribe", json!({"topics": ["status"]})).await;
        assert_eq!(
            call(&mut app, "make_default", Value::Null).await["result"],
            Value::Null
        );
        recv_status(&mut app, |s| s["is_default_source"] == true).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn clients_only_get_their_topics() {
        let daemon = start_daemon().await;
        let mut window = daemon.connect().await;
        let mut tray = daemon.connect().await;
        call(&mut window, "subscribe", json!({"topics": ["status"]})).await;
        call(&mut tray, "subscribe", json!({"topics": ["level"]})).await;
        daemon.start_session(false);
        recv_status(&mut window, |s| s["state"] == "streaming").await;
        // The tray streams `level` lines meanwhile, so read past them to the reply, and fail on any `status`.
        let request = json!({"jsonrpc": "2.0", "id": 1, "method": "get_settings"});
        tray.send(request.to_string()).await.unwrap();
        loop {
            let msg = recv(&mut tray).await;
            assert_ne!(
                msg["method"], "status",
                "the tray did not subscribe to status"
            );
            if msg["id"] == 1 {
                break;
            }
        }
    }
}
