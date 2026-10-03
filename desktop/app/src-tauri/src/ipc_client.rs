//! Client for the daemon's IPC socket (docs/ipc.md).
//!
//! One connection serves the whole app. It subscribes to every topic the daemon offers on each
//! (re)connect, so the web UI never calls `subscribe`: `subscribe` replaces a client's topics.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{sleep, timeout};
use tokio_util::codec::{Framed, LinesCodec};

const MAX_LINE: usize = 64 * 1024;
const RECONNECT_EVERY: Duration = Duration::from_secs(1);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const CALL_TIMEOUT: Duration = Duration::from_secs(10);
const SUBSCRIBE_ID: u64 = 0;
/// Add `transcript` with the transcript screen (D6.3): the daemon refuses that topic until then.
const TOPICS: [&str; 2] = ["status", "level"];
/// JSON-RPC's range for implementation-defined errors. The daemon never sends it.
const LOCAL_ERROR: i64 = -32000;

/// A JSON-RPC error: the daemon's (docs/ipc.md §5) or `-32000` when the call never got an answer.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IpcError {
    pub code: i64,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Connected,
    Disconnected,
    Notification { method: String, params: Value },
}

struct Request {
    method: String,
    params: Value,
    reply: oneshot::Sender<Result<Value, IpcError>>,
}

pub struct IpcClient {
    requests: mpsc::Sender<Request>,
}

/// Returns `$AIRMIC_SOCKET` if set (to point at a test daemon), else `$XDG_RUNTIME_DIR/airmic.sock`.
pub fn default_socket_path() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("AIRMIC_SOCKET") {
        return Ok(path.into());
    }
    let dir = std::env::var_os("XDG_RUNTIME_DIR").ok_or("XDG_RUNTIME_DIR is not set")?;
    Ok(Path::new(&dir).join("airmic.sock"))
}

impl IpcClient {
    /// Starts connecting to `path` in the background and keeps reconnecting. Must run inside a Tokio runtime.
    pub fn spawn(path: PathBuf, on_event: impl Fn(Event) + Send + Sync + 'static) -> IpcClient {
        let (requests, queue) = mpsc::channel(32);
        tokio::spawn(run(path, queue, on_event));
        IpcClient { requests }
    }

    /// Sends one request and waits for its reply. Fails at once while the daemon is unreachable.
    pub async fn call(&self, method: &str, params: Value) -> Result<Value, IpcError> {
        let (reply, answer) = oneshot::channel();
        let request = Request {
            method: method.to_owned(),
            params,
            reply,
        };
        self.requests
            .send(request)
            .await
            .map_err(|_| local_error("the IPC client has stopped"))?;
        match timeout(CALL_TIMEOUT, answer).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(local_error("the AirMic service is not running")),
            Err(_) => Err(local_error("the AirMic service did not answer")),
        }
    }
}

fn local_error(message: &str) -> IpcError {
    IpcError {
        code: LOCAL_ERROR,
        message: message.to_owned(),
    }
}

async fn run(
    path: PathBuf,
    mut requests: mpsc::Receiver<Request>,
    on_event: impl Fn(Event) + Send + Sync + 'static,
) {
    while !requests.is_closed() {
        if let Some(conn) = connect(&path).await {
            on_event(Event::Connected);
            serve(conn, &mut requests, &on_event).await;
            on_event(Event::Disconnected);
        }
        // While the daemon is away, fail requests at once instead of letting them queue up stale.
        let retry = sleep(RECONNECT_EVERY);
        tokio::pin!(retry);
        loop {
            tokio::select! {
                () = &mut retry => break,
                request = requests.recv() => match request {
                    Some(request) => {
                        let _ = request.reply.send(Err(local_error("the AirMic service is not running")));
                    }
                    None => return,
                },
            }
        }
    }
}

/// Connects and subscribes. `None` when the daemon is down or refuses the subscription.
async fn connect(path: &Path) -> Option<Framed<UnixStream, LinesCodec>> {
    let stream = UnixStream::connect(path).await.ok()?;
    let mut conn = Framed::new(stream, LinesCodec::new_with_max_length(MAX_LINE));
    let subscribed = async {
        let line = request_line(SUBSCRIBE_ID, "subscribe", json!({ "topics": TOPICS }));
        conn.send(line).await.ok()?;
        // Nothing else can arrive before the subscription is in place.
        loop {
            let reply: Value = serde_json::from_str(&conn.next().await?.ok()?).ok()?;
            if reply["id"] == SUBSCRIBE_ID {
                return reply.get("error").is_none().then_some(());
            }
        }
    };
    timeout(HANDSHAKE_TIMEOUT, subscribed).await.ok()??;
    Some(conn)
}

/// Runs one connection until it drops or the client is gone.
async fn serve(
    mut conn: Framed<UnixStream, LinesCodec>,
    requests: &mut mpsc::Receiver<Request>,
    on_event: &impl Fn(Event),
) {
    let mut pending: HashMap<u64, oneshot::Sender<Result<Value, IpcError>>> = HashMap::new();
    let mut next_id = SUBSCRIBE_ID + 1;
    loop {
        tokio::select! {
            request = requests.recv() => {
                let Some(request) = request else { break };
                let id = next_id;
                next_id += 1;
                let line = request_line(id, &request.method, request.params);
                if conn.send(line).await.is_err() {
                    let _ = request.reply.send(Err(local_error("the AirMic service is not running")));
                    break;
                }
                pending.insert(id, request.reply);
            }
            line = conn.next() => {
                let Some(Ok(line)) = line else { break };
                let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
                if let Some(id) = message["id"].as_u64() {
                    if let Some(reply) = pending.remove(&id) {
                        let _ = reply.send(parse_reply(&message));
                    }
                } else if let Some(method) = message["method"].as_str() {
                    on_event(Event::Notification {
                        method: method.to_owned(),
                        params: message["params"].clone(),
                    });
                }
            }
        }
    }
    for reply in pending.into_values() {
        let _ = reply.send(Err(local_error("the AirMic service is not running")));
    }
}

fn request_line(id: u64, method: &str, params: Value) -> String {
    let mut request = json!({ "jsonrpc": "2.0", "id": id, "method": method });
    if !params.is_null() {
        request["params"] = params;
    }
    request.to_string()
}

fn parse_reply(message: &Value) -> Result<Value, IpcError> {
    match message.get("error") {
        Some(error) => Err(IpcError {
            code: error["code"].as_i64().unwrap_or(LOCAL_ERROR),
            message: error["message"].as_str().unwrap_or_default().to_owned(),
        }),
        None => Ok(message["result"].clone()),
    }
}

#[cfg(test)]
mod tests {
    use tokio::net::UnixListener;
    use tokio::sync::mpsc::UnboundedReceiver;

    use super::*;

    type Conn = Framed<UnixStream, LinesCodec>;

    struct Harness {
        _dir: tempfile::TempDir,
        path: PathBuf,
        client: IpcClient,
        events: UnboundedReceiver<Event>,
    }

    fn start_client() -> Harness {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("airmic.sock");
        let (tx, events) = mpsc::unbounded_channel();
        let client = IpcClient::spawn(path.clone(), move |event| {
            let _ = tx.send(event);
        });
        Harness {
            _dir: dir,
            path,
            client,
            events,
        }
    }

    /// Accepts the client and answers its `subscribe`, like the daemon does.
    async fn accept(listener: &UnixListener) -> Conn {
        let (stream, _) = timeout(Duration::from_secs(5), listener.accept())
            .await
            .expect("client connects")
            .expect("accept");
        let mut conn = Framed::new(stream, LinesCodec::new());
        let subscribe = next_request(&mut conn).await;
        assert_eq!(subscribe["method"], "subscribe");
        assert_eq!(subscribe["params"]["topics"], json!(["status", "level"]));
        reply(&mut conn, &subscribe, json!(null)).await;
        conn
    }

    async fn next_request(conn: &mut Conn) -> Value {
        let line = conn.next().await.expect("a line").expect("valid line");
        serde_json::from_str(&line).expect("json")
    }

    async fn reply(conn: &mut Conn, request: &Value, result: Value) {
        let line = json!({ "jsonrpc": "2.0", "id": request["id"], "result": result });
        conn.send(line.to_string()).await.expect("send");
    }

    async fn next_event(events: &mut UnboundedReceiver<Event>) -> Event {
        timeout(Duration::from_secs(5), events.recv())
            .await
            .expect("an event")
            .expect("channel open")
    }

    #[tokio::test]
    async fn subscribes_then_answers_calls() {
        let mut h = start_client();
        let listener = UnixListener::bind(&h.path).unwrap();
        let mut conn = accept(&listener).await;
        assert_eq!(next_event(&mut h.events).await, Event::Connected);

        let call = h.client.call("get_settings", json!(null));
        let serve = async {
            let request = next_request(&mut conn).await;
            assert_eq!(request["method"], "get_settings");
            assert!(request.get("params").is_none());
            reply(&mut conn, &request, json!({ "audio_port": 47801 })).await;
        };
        let (result, ()) = tokio::join!(call, serve);
        assert_eq!(result.unwrap(), json!({ "audio_port": 47801 }));
    }

    #[tokio::test]
    async fn notifications_become_events() {
        let mut h = start_client();
        let listener = UnixListener::bind(&h.path).unwrap();
        let mut conn = accept(&listener).await;
        assert_eq!(next_event(&mut h.events).await, Event::Connected);

        let line =
            json!({ "jsonrpc": "2.0", "method": "level", "params": { "rms": 0.1, "peak": 0.4 } });
        conn.send(line.to_string()).await.unwrap();
        assert_eq!(
            next_event(&mut h.events).await,
            Event::Notification {
                method: "level".into(),
                params: json!({ "rms": 0.1, "peak": 0.4 }),
            }
        );
    }

    #[tokio::test]
    async fn daemon_errors_keep_their_code() {
        let mut h = start_client();
        let listener = UnixListener::bind(&h.path).unwrap();
        let mut conn = accept(&listener).await;
        assert_eq!(next_event(&mut h.events).await, Event::Connected);

        let call = h
            .client
            .call("forget_device", json!({ "phone_id": "nope" }));
        let serve = async {
            let request = next_request(&mut conn).await;
            let line = json!({
                "jsonrpc": "2.0",
                "id": request["id"],
                "error": { "code": 1, "message": "unknown_device" },
            });
            conn.send(line.to_string()).await.unwrap();
        };
        let (result, ()) = tokio::join!(call, serve);
        assert_eq!(
            result.unwrap_err(),
            IpcError {
                code: 1,
                message: "unknown_device".into()
            }
        );
    }

    #[tokio::test]
    async fn fails_fast_while_the_daemon_is_down_then_reconnects() {
        let mut h = start_client();
        let error = h.client.call("status", json!(null)).await.unwrap_err();
        assert_eq!(error.code, LOCAL_ERROR);

        let listener = UnixListener::bind(&h.path).unwrap();
        let conn = accept(&listener).await;
        assert_eq!(next_event(&mut h.events).await, Event::Connected);

        drop(conn);
        assert_eq!(next_event(&mut h.events).await, Event::Disconnected);

        let mut conn = accept(&listener).await;
        assert_eq!(next_event(&mut h.events).await, Event::Connected);
        let call = h.client.call("status", json!(null));
        let serve = async {
            let request = next_request(&mut conn).await;
            reply(&mut conn, &request, json!("ok")).await;
        };
        let (result, ()) = tokio::join!(call, serve);
        assert_eq!(result.unwrap(), json!("ok"));
    }

    #[tokio::test]
    async fn a_call_in_flight_fails_when_the_connection_drops() {
        let mut h = start_client();
        let listener = UnixListener::bind(&h.path).unwrap();
        let mut conn = accept(&listener).await;
        assert_eq!(next_event(&mut h.events).await, Event::Connected);

        let call = h.client.call("status", json!(null));
        let serve = async {
            next_request(&mut conn).await;
            drop(conn);
        };
        let (result, ()) = tokio::join!(call, serve);
        assert_eq!(result.unwrap_err().code, LOCAL_ERROR);
    }
}
