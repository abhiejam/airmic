# Audio underruns on the phone stream (D2.6)

Investigated 2026-10-03 on the real iPhone, PC on Wi-Fi (Intel 8260, -55 dBm, channel 149, power save off).
Tools: `AIRMIC_PACKET_TRACE=<file> airmicd` writes one CSV row per packet, `tools/analyze-packet-trace.py <file>` summarises it.

## Result

The underruns come from Wi-Fi link stalls of 50-300 ms, not from clock drift.

- Clock drift between phone and PC: -14 ppm (negligible).
- Capture 1 (60 s): 3 underruns, each after a 95-116 ms gap. Most gaps were about 12 s apart.
- Capture 2 (183 s, with `ping -i 0.2` to the router and the phone): 28 underruns (9.2/min), 34 stalls over 40 ms.
  - 15 of the 28 underruns fell in one 14 s stretch where both pings peaked at 190-243 ms.
  - The other stalls were 46-183 ms, often in pairs 2 s apart and repeating about every 12 s.
  - 19 of 34 stalls had a router ping spike over 30 ms nearby and 15 of 34 a phone ping spike. A random moment hits a spike 6-7% of the time. Stalls with no ping spike may be phone-side or between pings.
  - Stalls often lose 4-5 packets, so loss was about 0.5%.
- The jitter estimate stays at 0.5-1 ms, so the target (20 ms + 4 x jitter) never sees these rare stalls.

## What a larger target would prevent

Estimate from capture 2, assuming the buffer is full when a stall starts. Real numbers will be worse, since stalls cluster.

| Target | Underruns prevented (of 28) |
|---|---|
| 80 ms | 6 |
| 120 ms | 19 |
| 160 ms | 23 |
| 200 ms | 26 |
| 300 ms | 28 |

## Impact

Each underrun is a 50-150 ms silent gap. In capture 2 that is roughly 1-3% of the audio. `/voice` dictation has not shown errors from it.
Not measured: what an app records. The underrun counter does not say whether anything was recording. To check, run `pw-record --target airmic --rate 48000 --channels 1 out.wav` during a capture and count gaps in the WAV.

## Options

1. Ethernet for the PC.
2. A higher target cap (PRD 8.1 sets 120 ms): a latency decision.
3. Conceal short gaps instead of playing silence.
