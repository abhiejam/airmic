#!/usr/bin/env python3
"""Fake airmicd for testing the iPhone app without the Linux daemon (mobile task M2.5).

Speaks wire protocol v1 (docs/protocol.md): control on TCP 47800, audio on UDP 47801.
Python 3 standard library only.

  python3 tools/mock_control.py                    # like `airmicd --no-auth`
  python3 tools/mock_control.py --pair 0427        # unknown phones must enter this code
  python3 tools/mock_control.py --wav session.wav  # save the audio it receives
  python3 tools/mock_control.py --drop-after 20    # close the control connection after 20 s (reconnect test)
"""

import argparse
import asyncio
import json
import math
import secrets
import struct
import time
import wave

CONTROL_PORT = 47800
AUDIO_PORT = 47801
SAMPLE_RATE = 48000
FRAME_BYTES = 960
HEADER = struct.Struct(">2sBBIII")


def log(message):
    print(time.strftime("%H:%M:%S"), message, flush=True)


class Server:
    def __init__(self, args):
        self.args = args
        self.session_id = None  # active session
        self.tokens = set()  # paired tokens, for this run only
        self.audio = AudioStats()
        self.wav = None
        if args.wav:
            self.wav = wave.open(args.wav, "wb")
            self.wav.setnchannels(1)
            self.wav.setsampwidth(2)
            self.wav.setframerate(SAMPLE_RATE)


class AudioStats:
    """Counts packets per 2 s window, like the stats the daemon reports."""

    def __init__(self):
        self.reset_window()
        self.expected_seq = None
        self.last_arrival = None

    def reset_window(self):
        self.received = 0
        self.lost = 0
        self.muted = 0
        self.jitter_sum = 0.0
        self.peak = 0.0

    def loss_pct(self):
        total = self.received + self.lost
        return 100.0 * self.lost / total if total else 0.0

    def jitter_ms(self):
        return self.jitter_sum / self.received if self.received else 0.0


class AudioProtocol(asyncio.DatagramProtocol):
    def __init__(self, server):
        self.server = server

    def datagram_received(self, data, addr):
        s = self.server
        if len(data) < 16:
            return log(f"audio: dropped {len(data)} byte packet (too short)")
        magic, version, flags, session_id, seq, ts = HEADER.unpack_from(data)
        if magic != b"AM" or version != 1:
            return log("audio: dropped packet with bad magic or version")
        if session_id != s.session_id:
            return  # stale or foreign session, silently dropped
        muted = bool(flags & 1)
        codec = (flags >> 1) & 7
        if codec != 0:
            return log(f"audio: dropped codec {codec}")
        payload = data[16:]
        if not muted and len(payload) != FRAME_BYTES:
            return log(f"audio: dropped packet with {len(payload)} byte payload")

        a = s.audio
        if a.expected_seq is not None and seq != a.expected_seq:
            gap = (seq - a.expected_seq) & 0xFFFFFFFF
            if gap < 1000:
                a.lost += gap
        a.expected_seq = (seq + 1) & 0xFFFFFFFF
        now = time.monotonic()
        if a.last_arrival is not None and not muted:
            a.jitter_sum += abs((now - a.last_arrival) * 1000 - 10)
        a.last_arrival = now
        a.received += 1
        if muted:
            a.muted += 1
            if s.wav:
                s.wav.writeframes(b"\0" * FRAME_BYTES * 10)
            return
        n = len(payload) // 2
        samples = struct.unpack(f"<{n}h", payload)
        rms = math.sqrt(sum(x * x for x in samples) / n) / 32767
        a.peak = max(a.peak, rms)
        if s.wav:
            s.wav.writeframes(payload)


async def handle_control(reader, writer, server):
    peer = writer.get_extra_info("peername")[0]
    args = server.args

    async def send(message):
        writer.write((json.dumps(message) + "\n").encode())
        await writer.drain()
        if message["type"] not in ("ping", "pong", "stats"):
            log(f"→ {message}")

    async def read(timeout=6.0):
        line = await asyncio.wait_for(reader.readline(), timeout)
        if not line:
            raise ConnectionError("closed by phone")
        message = json.loads(line)
        if message.get("type") not in ("ping", "pong"):
            log(f"← {message}")
        return message

    if server.session_id is not None:
        await send({"type": "error", "code": "busy", "message": "Another phone is connected"})
        writer.close()
        return

    log(f"control: connection from {peer}")
    tasks = []
    try:
        hello = await read()
        if hello.get("type") != "hello":
            await send({"type": "error", "code": "bad_message", "message": "Expected hello"})
            return
        if hello.get("v") != 1:
            await send({"type": "error", "code": "unsupported_version", "message": "Only v1"})
            return

        async def start_session():
            server.session_id = secrets.randbits(32) or 1
            server.audio = AudioStats()
            await send({"type": "ready", "session_id": server.session_id, "udp_port": AUDIO_PORT, "sample_rate": SAMPLE_RATE})

        pending = None
        if not args.pair:
            await start_session()
        else:
            # The phone sends auth right after hello when it has a token.
            try:
                pending = await read(timeout=0.5)
            except asyncio.TimeoutError:
                pending = None
            if pending and pending.get("type") == "auth" and pending.get("token") in server.tokens:
                await start_session()
            else:
                await send({"type": "pair_required"})
            pending = None

        async def pinger():
            while True:
                await asyncio.sleep(2)
                await send({"type": "ping"})

        async def stats():
            while True:
                await asyncio.sleep(2)
                if server.session_id is None:
                    continue
                a = server.audio
                level = 20 * math.log10(a.peak) if a.peak > 0 else -99
                log(f"audio: {a.received / 2:.0f} pkt/s, {a.muted} muted, loss {a.loss_pct():.1f}%, "
                    f"jitter {a.jitter_ms():.1f} ms, loudest {level:.0f} dBFS")
                await send({"type": "stats", "loss_pct": round(a.loss_pct(), 2),
                            "jitter_ms": round(a.jitter_ms(), 2), "latency_ms": round(20 + a.jitter_ms(), 1)})
                a.reset_window()

        async def dropper():
            await asyncio.sleep(args.drop_after)
            log(f"control: dropping the connection (--drop-after {args.drop_after})")
            writer.transport.abort()

        tasks = [asyncio.create_task(pinger()), asyncio.create_task(stats())]
        if args.drop_after:
            tasks.append(asyncio.create_task(dropper()))

        while True:
            message = await read()
            kind = message.get("type")
            if kind == "ping":
                await send({"type": "pong"})
            elif kind == "bye":
                break
            elif kind == "pair" and args.pair:
                if message.get("code") == args.pair:
                    token = secrets.token_hex(16)
                    server.tokens.add(token)
                    await send({"type": "paired", "token": token})
                    await start_session()
                else:
                    await send({"type": "error", "code": "bad_code", "message": "Wrong code"})
            elif kind == "mute":
                log(f"control: phone {'muted' if message.get('on') else 'unmuted'}")
    except asyncio.TimeoutError:
        log("control: no message for 6 s, closing")
    except (ConnectionError, json.JSONDecodeError) as error:
        log(f"control: {error}")
    finally:
        for task in tasks:
            task.cancel()
        server.session_id = None
        writer.close()
        log("control: session ended")


async def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--pair", metavar="CODE", help="require pairing with this 4 digit code")
    parser.add_argument("--wav", metavar="FILE", help="save received audio to a WAV file")
    parser.add_argument("--drop-after", type=float, metavar="SECONDS", help="abort each control connection after this long")
    args = parser.parse_args()

    server = Server(args)
    loop = asyncio.get_running_loop()
    await loop.create_datagram_endpoint(lambda: AudioProtocol(server), local_addr=("0.0.0.0", AUDIO_PORT))
    tcp = await asyncio.start_server(lambda r, w: handle_control(r, w, server), "0.0.0.0", CONTROL_PORT)
    log(f"mock airmicd: control tcp {CONTROL_PORT}, audio udp {AUDIO_PORT}"
        + (f", pairing code {args.pair}" if args.pair else ", no auth"))
    try:
        async with tcp:
            await tcp.serve_forever()
    finally:
        if server.wav:
            server.wav.close()


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        pass
