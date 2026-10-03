# AirMic wire protocol v1

Contract between the iPhone app (client) and `airmicd` (server).
Source: [PRD §6](PRD.md#6-wire-protocol-v1). Test vectors: [`protocol/vectors.json`](protocol/vectors.json).
Owner: desktop track. Changes need agreement from both tracks and go into PRD §6 and both task files.

Ports: TCP **47800** (control), UDP **47801** (audio).

## 1. Discovery

- mDNS service `_airmic._tcp`, port 47800.
- TXT record: `id=<computer uuid>`, `name=<hostname>`, `v=1`.
- QR code (fallback): a URL
  `airmic://pair?host=192.168.20.42&port=47800&id=<computer uuid>&code=0427`.
  The phone connects to `host:port` and sends `pair` with `code` without asking the user.
- Manual IP: the phone connects to `<ip>:47800`.

## 2. Control channel (TCP 47800)

### 2.1 Framing
- One JSON object per line, UTF-8, terminated by `\n`. No `\n` inside a message.
- Every message has a string field `type`. Other fields sit next to it, flat:
  `{"type":"mute","on":true}`.
- Maximum line length 64 KiB. A longer line closes the connection.
- Receivers ignore unknown fields and unknown `type` values (forward compatibility).
- Key order and whitespace are not significant. Compare parsed values, not strings.

### 2.2 Messages

| `type` | Direction | Fields | Notes |
|---|---|---|---|
| `hello` | phone → PC | `v` int, `phone_id` string, `phone_name` string | First message. `v` = 1. `phone_id` is a UUID the phone generates once and keeps. |
| `pair_required` | PC → phone | none | Phone is unknown or its token is wrong. Phone asks the user for the code. |
| `pair` | phone → PC | `code` string | 4 digits as a string, leading zeros kept: `"0427"`. |
| `paired` | PC → phone | `token` string | 32 lowercase hex chars (128 bit). Phone stores it in the Keychain. `ready` follows. |
| `auth` | phone → PC | `token` string | Sent right after `hello` when the phone has a token for this computer. |
| `ready` | PC → phone | `session_id` uint32, `udp_port` int, `sample_rate` int | Start sending audio. `session_id` is random and never 0. `sample_rate` = 48000. |
| `mute` | phone → PC | `on` bool | Status only. Audio packets carry the mute flag too (§3). |
| `stats` | PC → phone | `loss_pct` number, `jitter_ms` number, `latency_ms` number | Every 2 s while audio flows. `loss_pct` is 0–100 over the last 2 s. |
| `transcript` | PC → phone | `text` string, `final` bool | Optional. `final: false` lines may be replaced by the next one. |
| `ping` | both | none | Every 2 s. |
| `pong` | both | none | Immediate reply to `ping`. |
| `bye` | both | none | End the session. The sender closes the connection after it. |
| `error` | PC → phone | `code` string, `message` string | See §2.4. `message` is human readable, for logs. |

### 2.3 Session flow

New phone:
```
phone → hello {v:1, phone_id, phone_name}
PC    → pair_required
phone → pair {code:"0427"}
PC    → paired {token}
PC    → ready {session_id, udp_port:47801, sample_rate:48000}
```

Known phone:
```
phone → hello {...}
phone → auth {token}
PC    → ready {...}
```

- The phone sends `auth` right after `hello` without waiting. If the token is unknown (the PC forgot the phone), the PC answers `pair_required` and the phone deletes its stored token.
- A wrong code answers `error {code:"bad_code"}`; the connection stays open for another `pair`.
- Daemon run with `--no-auth` (development): `hello` is answered with `ready` directly. `auth` is accepted and ignored.
- Only the first `hello` counts. `ping`, `pong` and `bye` are valid any time after `hello`, so pairing can take longer than the 6 s timeout. `mute` is valid only after `ready`.

### 2.4 Errors

| `code` | When | Connection |
|---|---|---|
| `unsupported_version` | `hello.v` is not 1 | closed |
| `busy` | Another phone has an active session | closed |
| `bad_code` | Wrong or expired pairing code | stays open |
| `pair_locked` | 5 wrong codes for the current code; the user must show a new code on the PC | closed |
| `bad_message` | Invalid JSON, missing field, or message in the wrong state | closed |

### 2.5 Keepalive and timeouts
- Each side sends `ping` every 2 s and answers every `ping` with `pong`.
- Any message received resets the peer's timer. No message for **6 s** means the peer is gone: close the connection.
- The PC estimates `latency_ms` as half its `ping`→`pong` round trip plus the current jitter buffer delay.
- Closing the TCP connection ends the session. The PC stops accepting audio for that `session_id`.

## 3. Audio channel (UDP 47801)

### 3.1 Packet

```
0      2     3      4            8            12           16
+------+-----+------+------------+------------+------------+---------------------+
| "AM" | ver | flags| session_id |  sequence  | timestamp  | payload (0 or 960 B)|
+------+-----+------+------------+------------+------------+---------------------+
```

Header: 16 bytes, all fields big endian.

| Offset | Field | Size | Value |
|---|---|---|---|
| 0 | magic | 2 | `0x41 0x4D` (`"AM"`) |
| 2 | version | 1 | `1` |
| 3 | flags | 1 | bit 0: muted. Bits 1–3: codec (0 = PCM). Bits 4–7: reserved, send 0, ignore on receive. |
| 4 | session_id | 4 | from `ready` |
| 8 | sequence | 4 | +1 per packet, starts at 0, wraps at 2³² |
| 12 | timestamp | 4 | index of the first sample of this packet since session start, wraps at 2³² |

Payload: PCM signed 16 bit **little endian**, 48 kHz, mono, 10 ms = 480 samples = **960 bytes**.
Note the header is big endian and the samples are little endian.

### 3.2 Sending
- Send to the PC address of the TCP connection, port `udp_port` from `ready`, from any local port.
- Unmuted: one packet every 10 ms, timestamp +480 per packet.
- Muted: header-only packets (16 bytes, flag bit 0 set) 10 per second, sequence +1 per packet, timestamp advanced by the samples that elapsed (+4800 per packet at 10/s). On unmute, resume normal packets with the next sequence and the current timestamp.

### 3.3 Receiving (PC)
Drop the packet when any of these is true:
- shorter than 16 bytes, magic not `"AM"`, or version not 1
- `session_id` is not the active session's
- codec not 0
- not muted and payload not exactly 960 bytes

A packet with the muted flag set means silence for its duration, whatever its payload.
The jitter buffer orders packets by sequence (wrap aware) and fills gaps with silence.

## 4. Security (v1)
- LAN only, audio unencrypted.
- Only a paired phone (or any phone in `--no-auth` mode) receives a `session_id`; packets without the active id are dropped.
- Pairing code: 4 digits, valid 2 minutes, 5 attempts. Token: 128 bit random, stored on the phone (Keychain) and the PC (`~/.config/airmic/paired.json`).
