# AirMic IPC v1: daemon ↔ desktop app

Contract between `airmicd` (server) and the Tauri desktop app (client).
The phone never uses it; the phone protocol is [protocol.md](protocol.md).
Owner: desktop track. The daemon side is D3.7, the app side D5.2.

## 1. Transport

- Unix socket `$XDG_RUNTIME_DIR/airmic.sock`, mode `0600`. The daemon removes a stale socket file on start.
- Behind a trait in the daemon, so Windows can use a named pipe later (PRD §10).
- Several clients at once (window and tray). Each gets its own subscriptions.
- One JSON object per line, UTF-8, `\n` terminated, at most 64 KiB.
- [JSON-RPC 2.0](https://www.jsonrpc.org/specification): requests have `id`, notifications do not. Batches are not supported.

```
→ {"jsonrpc":"2.0","id":1,"method":"status"}
← {"jsonrpc":"2.0","id":1,"result":{...}}
```

## 2. Types

```ts
type Status = {
  state: "idle" | "streaming" | "muted";   // idle = no phone session
  phone: { id: string; name: string; addr: string } | null;
  stats: { loss_pct: number; jitter_ms: number; latency_ms: number } | null; // null when idle
  audio_flowing: boolean;   // a UDP packet arrived in the last 2 s
  is_default_source: boolean;
  version: string;          // daemon version
};

type PairingCode = {
  code: string;             // "0427"
  expires_at: number;       // Unix ms
  qr: string;               // airmic://pair?host=…&port=…&id=…&code=… (protocol.md §1)
};

type PairedDevice = {
  phone_id: string;
  name: string;
  paired_at: number;        // Unix ms
  last_seen: number | null; // Unix ms
};

type Settings = {
  set_default_source: boolean;
  control_port: number;     // takes effect on daemon restart
  audio_port: number;       // takes effect on daemon restart
  transcription: { enabled: boolean; model: "base.en" | "small.en" };
};
```

`audio_flowing` lets the app detect a blocked firewall (D5.9): `state` is `streaming` but `audio_flowing` stays `false`.

## 3. Methods

| Method | Params | Result | Notes |
|---|---|---|---|
| `status` | none | `Status` | |
| `pairing_code` | `{regenerate?: boolean}` | `PairingCode` | Returns the current code, or creates one if none is valid. `regenerate: true` always creates a new one and resets the attempt count. |
| `paired_devices` | none | `PairedDevice[]` | |
| `forget_device` | `{phone_id}` | `null` | Ends that phone's session if it is connected. |
| `get_settings` | none | `Settings` | |
| `set_settings` | `Partial<Settings>` | `Settings` | Merges, saves to `config.toml`, returns the full result. |
| `make_default` | none | `null` | Sets AirMic as the default input now (the "fix" button, D5.4). |
| `subscribe` | `{topics: Topic[]}` | `null` | Replaces this client's subscriptions. |

`type Topic = "status" | "level" | "transcript"`

The daemon's `settings` call in PRD §8.1 is split into `get_settings` and `set_settings`.
"Start at login" is not a daemon setting: the app enables or disables the systemd unit itself (D5.8).

## 4. Notifications (daemon → app)

Sent only for subscribed topics.

| Method | Params | When |
|---|---|---|
| `status` | `Status` | Whenever any `Status` field except `stats` changes, and every 2 s while streaming. |
| `level` | `{rms: number, peak: number}` | About 20 per second while streaming. Both are 0–1 over the last 50 ms of output. |
| `transcript` | `{text: string, final: boolean}` | Same meaning as the phone `transcript` message. |

## 5. Errors

Standard JSON-RPC codes (`-32700` parse error, `-32600` invalid request, `-32601` method not found, `-32602` invalid params) plus:

| Code | Meaning |
|---|---|
| `1` | `unknown_device`: `forget_device` with an id that is not paired |
| `2` | `invalid_setting`: a value out of range, e.g. port 0 |
| `3` | `unavailable`: the feature is off or not built, e.g. transcription |

## 6. Example session

```
→ {"jsonrpc":"2.0","id":1,"method":"subscribe","params":{"topics":["status","level"]}}
← {"jsonrpc":"2.0","id":1,"result":null}
→ {"jsonrpc":"2.0","id":2,"method":"pairing_code"}
← {"jsonrpc":"2.0","id":2,"result":{"code":"0427","expires_at":1791010000000,"qr":"airmic://pair?host=192.168.20.42&port=47800&id=6f1c…&code=0427"}}
← {"jsonrpc":"2.0","method":"status","params":{"state":"streaming","phone":{"id":"3f25…","name":"Abhishek's iPhone","addr":"192.168.20.31"},"stats":{"loss_pct":0.0,"jitter_ms":2.1,"latency_ms":48.0},"audio_flowing":true,"is_default_source":true,"version":"0.1.0"}}
← {"jsonrpc":"2.0","method":"level","params":{"rms":0.12,"peak":0.4}}
```
