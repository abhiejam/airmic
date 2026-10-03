//! Daemon ↔ desktop app IPC (docs/ipc.md): newline-delimited JSON-RPC 2.0 over a local socket.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Context;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{UnixListener, UnixStream};
use tokio_util::codec::{Framed, LinesCodec, LinesCodecError};
use tracing::{info, warn};

use crate::config::Config;

const MAX_LINE: usize = 64 * 1024;

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const INTERNAL_ERROR: i64 = -32603;
const INVALID_SETTING: i64 = 2;

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

/// State shared by every app connection.
pub struct Ipc {
    config_path: PathBuf,
    config: Mutex<Config>,
}

impl Ipc {
    pub fn new(config_path: PathBuf, config: Config) -> Arc<Ipc> {
        Arc::new(Ipc {
            config_path,
            config: Mutex::new(config),
        })
    }

    /// Returns the reply line for one request line, or `None` for a JSON-RPC notification.
    fn handle_line(&self, line: &str) -> Option<String> {
        let value: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(e) => return Some(reply(Value::Null, Err(rpc_error(PARSE_ERROR, e)))),
        };
        let id = value.get("id").cloned().unwrap_or(Value::Null);
        let request = match serde_json::from_value::<Request>(value) {
            Ok(request) => request,
            Err(e) => return Some(reply(id, Err(rpc_error(INVALID_REQUEST, e)))),
        };
        let result = self.call(&request.method, request.params);
        request.id.map(|id| reply(id, result))
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        match method {
            "get_settings" => Ok(json!(*self.config.lock().expect("config lock"))),
            "set_settings" => self.set_settings(params),
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

async fn handle_client<S: AsyncRead + AsyncWrite + Unpin>(
    stream: S,
    ipc: &Ipc,
) -> anyhow::Result<()> {
    let mut conn = Framed::new(stream, LinesCodec::new_with_max_length(MAX_LINE));
    while let Some(line) = conn.next().await {
        let reply = match line {
            Ok(line) => ipc.handle_line(&line),
            // `Framed` ends the stream after a decode error, so the client is dropped after this reply.
            Err(LinesCodecError::MaxLineLengthExceeded) => Some(reply(
                Value::Null,
                Err(rpc_error(INVALID_REQUEST, "line over 64 KiB")),
            )),
            Err(e) => return Err(e.into()),
        };
        if let Some(reply) = reply {
            conn.send(reply).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    type Client = Framed<UnixStream, LinesCodec>;

    struct Daemon {
        dir: tempfile::TempDir,
    }

    async fn start_daemon() -> Daemon {
        let dir = tempfile::tempdir().unwrap();
        let listener = UnixSocket::bind(&dir.path().join("airmic.sock")).unwrap();
        let ipc = Ipc::new(dir.path().join("config/config.toml"), Config::default());
        tokio::spawn(serve(listener, ipc));
        Daemon { dir }
    }

    impl Daemon {
        async fn connect(&self) -> Client {
            let stream = UnixStream::connect(self.dir.path().join("airmic.sock"));
            Framed::new(stream.await.unwrap(), LinesCodec::new())
        }
    }

    async fn recv(client: &mut Client) -> Value {
        let line = tokio::time::timeout(std::time::Duration::from_secs(5), client.next()).await;
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
        assert_eq!(
            error_code(&call(&mut app, "set_settings", json!([1])).await),
            INVALID_PARAMS
        );
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
}
