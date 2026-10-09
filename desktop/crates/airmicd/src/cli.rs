//! The `airmicd` subcommands other than the daemon itself: thin clients over the IPC socket (docs/ipc.md).

use std::fmt::Write as _;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, anyhow, bail};
use clap::Subcommand;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::UnixStream;
use tokio_util::codec::{Framed, LinesCodec};

use crate::ipc;

#[derive(Subcommand)]
pub enum Command {
    /// Show the connected phone, stream quality and whether AirMic is the default mic.
    Status,
    /// List the paired phones.
    Devices,
    /// Make AirMic the default microphone.
    MakeDefault,
}

/// Runs `command` against the daemon on `$AIRMIC_SOCKET`, or the default socket.
pub async fn run(command: Command) -> anyhow::Result<()> {
    let path = match std::env::var_os("AIRMIC_SOCKET") {
        Some(path) => PathBuf::from(path),
        None => ipc::socket_path()?,
    };
    let mut client = Client::connect(&path).await?;
    execute(command, &mut client, &mut std::io::stdout()).await
}

async fn execute(
    command: Command,
    client: &mut Client,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    match command {
        Command::Status => {
            let status = client.call("status", Value::Null).await?;
            write!(out, "{}", render_status(&status))?;
        }
        Command::Devices => {
            let devices = client.call("paired_devices", Value::Null).await?;
            write!(out, "{}", render_devices(&devices, unix_ms()))?;
        }
        Command::MakeDefault => {
            client.call("make_default", Value::Null).await?;
            writeln!(out, "AirMic is now the default mic.")?;
        }
    }
    Ok(())
}

struct Client {
    conn: Framed<UnixStream, LinesCodec>,
    next_id: u64,
}

impl Client {
    async fn connect(path: &Path) -> anyhow::Result<Client> {
        let stream = UnixStream::connect(path).await.map_err(|e| {
            anyhow!(
                "airmicd is not running ({}: {e}). Start it with `airmicd`.",
                path.display()
            )
        })?;
        Ok(Client {
            conn: Framed::new(stream, LinesCodec::new()),
            next_id: 0,
        })
    }

    /// Sends a request and returns its result. Notifications that arrive before the reply are dropped.
    async fn call(&mut self, method: &str, params: Value) -> anyhow::Result<Value> {
        self.next_id += 1;
        let id = self.next_id;
        let request = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        self.conn.send(request.to_string()).await?;
        loop {
            let message = self.read().await?;
            if message["id"] != json!(id) {
                continue;
            }
            if let Some(error) = message.get("error") {
                bail!(
                    "airmicd: {}",
                    error["message"].as_str().unwrap_or("unknown error")
                );
            }
            return Ok(message["result"].clone());
        }
    }

    async fn read(&mut self) -> anyhow::Result<Value> {
        let line = self
            .conn
            .next()
            .await
            .context("airmicd closed the connection")??;
        Ok(serde_json::from_str(&line)?)
    }
}

const KNOWN_STATUS_FIELDS: [&str; 6] = [
    "state",
    "phone",
    "stats",
    "audio_flowing",
    "is_default_source",
    "version",
];

/// Renders a `Status` as aligned lines. Unknown string fields, such as hints from a newer daemon, are shown as is.
fn render_status(status: &Value) -> String {
    let mut text = String::new();
    let mut line = |label: &str, value: &str| {
        let _ = writeln!(text, "{:<13}{value}", format!("{label}:"));
    };
    let state = status["state"].as_str().unwrap_or("unknown");
    match &status["phone"] {
        Value::Object(phone) => {
            let name = phone.get("name").and_then(Value::as_str).unwrap_or("?");
            let addr = phone.get("addr").and_then(Value::as_str).unwrap_or("?");
            line("Phone", &format!("{name} ({addr})"));
        }
        _ => line("Phone", "none"),
    }
    line(
        "State",
        match state {
            "idle" => "idle, no phone connected",
            "streaming" => "connected",
            "muted" => "connected, muted",
            other => other,
        },
    );
    if state != "idle" {
        let flowing = status["audio_flowing"].as_bool().unwrap_or(false);
        line("Audio", if flowing { "arriving" } else { "not arriving" });
    }
    if status["audio_blocked"].as_bool().unwrap_or(false) {
        let port = status["audio_port"]
            .as_u64()
            .map_or("<audio port>".into(), |p| p.to_string());
        line(
            "Hint",
            &format!(
                "the phone is connected but no audio arrives. If a firewall is on, run \
                 `sudo ufw allow {port}/udp`. Wi-Fi AP isolation or a guest network can also block it."
            ),
        );
    }
    if let Some(stats) = status["stats"].as_object() {
        let number = |key: &str| stats.get(key).and_then(Value::as_f64).unwrap_or(0.0);
        line("Latency", &format!("{:.0} ms", number("latency_ms")));
        line("Loss", &format!("{:.1}%", number("loss_pct")));
        line("Jitter", &format!("{:.1} ms", number("jitter_ms")));
    }
    if status["is_default_source"].as_bool().unwrap_or(false) {
        line("Default mic", "yes");
    } else {
        line(
            "Default mic",
            "no. Run `airmicd make-default` to make AirMic the default mic.",
        );
    }
    for (key, value) in status.as_object().into_iter().flatten() {
        if let (false, Some(value)) = (KNOWN_STATUS_FIELDS.contains(&key.as_str()), value.as_str())
        {
            line(key, value);
        }
    }
    line("Daemon", status["version"].as_str().unwrap_or("?"));
    text
}

fn render_devices(devices: &Value, now_ms: u64) -> String {
    let devices = devices.as_array().map(Vec::as_slice).unwrap_or_default();
    if devices.is_empty() {
        return "No paired phones. Run `airmicd pair` to pair one.\n".into();
    }
    let field = |d: &Value, key: &str| d[key].as_str().unwrap_or_default().to_string();
    let name_width = devices
        .iter()
        .map(|d| field(d, "name").chars().count())
        .max();
    let name_width = name_width.unwrap_or_default().max("NAME".len());
    let id_width = devices.iter().map(|d| field(d, "phone_id").len()).max();
    let id_width = id_width.unwrap_or_default().max("ID".len());
    let mut text = format!(
        "{:<id_width$}  {:<name_width$}  {:<12}  LAST SEEN\n",
        "ID", "NAME", "PAIRED"
    );
    for d in devices {
        let paired = format_age(d["paired_at"].as_u64().unwrap_or_default(), now_ms);
        let last_seen = match d["last_seen"].as_u64() {
            Some(ms) => format_age(ms, now_ms),
            None => "never".into(),
        };
        let _ = writeln!(
            text,
            "{:<id_width$}  {:<name_width$}  {paired:<12}  {last_seen}",
            field(d, "phone_id"),
            field(d, "name"),
        );
    }
    text
}

fn format_age(ms: u64, now_ms: u64) -> String {
    let minutes = now_ms.saturating_sub(ms) / 60_000;
    match minutes {
        0 => "just now".into(),
        1..60 => format!("{minutes} min ago"),
        60..1440 => format!("{} h ago", minutes / 60),
        _ => format!("{} days ago", minutes / 1440),
    }
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use tokio::net::UnixListener;

    use super::*;

    type Conn = Framed<UnixStream, LinesCodec>;

    /// Connects a `Client` to a fake daemon whose side of the connection is returned.
    async fn connect_fake() -> (Client, Conn, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("airmic.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let client = Client::connect(&path).await.unwrap();
        let (stream, _) = listener.accept().await.unwrap();
        (client, Framed::new(stream, LinesCodec::new()), dir)
    }

    /// Reads the next request, checks its method and answers it with `result`.
    async fn answer(conn: &mut Conn, method: &str, result: Value) -> Value {
        let request: Value = serde_json::from_str(&conn.next().await.unwrap().unwrap()).unwrap();
        assert_eq!(request["method"], method);
        let reply = json!({"jsonrpc": "2.0", "id": request["id"], "result": result});
        conn.send(reply.to_string()).await.unwrap();
        request
    }

    async fn notify_status(conn: &mut Conn) {
        let line = json!({"jsonrpc": "2.0", "method": "status", "params": {"state": "streaming"}});
        conn.send(line.to_string()).await.unwrap();
    }

    fn output(out: Vec<u8>) -> String {
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn status_shows_the_phone_stats_and_the_make_default_hint() {
        let status = json!({
            "state": "muted",
            "phone": {"id": "p1", "name": "Abhishek's iPhone", "addr": "192.168.20.31"},
            "stats": {"loss_pct": 1.25, "jitter_ms": 2.1, "latency_ms": 48.0},
            "audio_flowing": true,
            "is_default_source": false,
            "audio_blocked": true,
            "audio_port": 47801,
            "future_note": "from a newer daemon",
            "version": "0.1.0",
        });
        let text = render_status(&status);
        assert!(
            text.contains("Phone:       Abhishek's iPhone (192.168.20.31)"),
            "{text}"
        );
        assert!(text.contains("State:       connected, muted"), "{text}");
        assert!(text.contains("Latency:     48 ms"), "{text}");
        assert!(text.contains("Loss:        1.2%"), "{text}");
        assert!(text.contains("airmicd make-default"), "{text}");
        assert!(text.contains("`sudo ufw allow 47801/udp`"), "{text}");
        assert!(text.contains("from a newer daemon"), "{text}");
    }

    #[test]
    fn idle_status_has_no_stream_lines() {
        let status = json!({
            "state": "idle", "phone": null, "stats": null, "audio_flowing": false,
            "is_default_source": true, "version": "0.1.0",
        });
        let text = render_status(&status);
        assert!(text.contains("Phone:       none"), "{text}");
        assert!(text.contains("Default mic: yes"), "{text}");
        assert!(
            !text.contains("Latency") && !text.contains("Audio"),
            "{text}"
        );
    }

    #[test]
    fn devices_show_ages_and_never_seen() {
        let now = 1_790_000_000_000;
        let devices = json!([
            {"phone_id": "p1", "name": "iPhone", "paired_at": now - 3 * 86_400_000, "last_seen": now},
            {"phone_id": "p2", "name": "Old iPad", "paired_at": now - 2 * 3_600_000, "last_seen": null},
        ]);
        let text = render_devices(&devices, now);
        assert!(
            text.contains("p1  iPhone    3 days ago    just now"),
            "{text}"
        );
        assert!(text.contains("p2  Old iPad  2 h ago       never"), "{text}");
        assert!(render_devices(&json!([]), now).contains("airmicd pair"));
    }

    #[tokio::test]
    async fn a_missing_daemon_says_it_is_not_running() {
        let dir = tempfile::tempdir().unwrap();
        let error = Client::connect(&dir.path().join("airmic.sock"))
            .await
            .err()
            .unwrap();
        assert!(
            error.to_string().contains("airmicd is not running"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn calls_skip_notifications_and_report_daemon_errors() {
        let (mut client, mut conn, _dir) = connect_fake().await;
        let daemon = tokio::spawn(async move {
            notify_status(&mut conn).await;
            answer(&mut conn, "make_default", Value::Null).await;
            let request: Value =
                serde_json::from_str(&conn.next().await.unwrap().unwrap()).unwrap();
            let error = json!({"code": -32603, "message": "no PipeWire"});
            let reply = json!({"jsonrpc": "2.0", "id": request["id"], "error": error});
            conn.send(reply.to_string()).await.unwrap();
        });
        let mut out = Vec::new();
        execute(Command::MakeDefault, &mut client, &mut out)
            .await
            .unwrap();
        assert_eq!(output(out), "AirMic is now the default mic.\n");
        let error = client.call("make_default", Value::Null).await.unwrap_err();
        assert_eq!(error.to_string(), "airmicd: no PipeWire");
        daemon.await.unwrap();
    }
}
