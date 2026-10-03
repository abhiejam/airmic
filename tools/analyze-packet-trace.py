#!/usr/bin/env python3
"""Summarises a packet trace from `AIRMIC_PACKET_TRACE=<file> airmicd`: stalls, bursts, clock drift, underruns."""
import csv
import sys

SAMPLE_RATE = 48000


def pct(sorted_values, p):
    return sorted_values[min(len(sorted_values) - 1, int(p / 100 * len(sorted_values)))]


def main(path):
    rows = []
    with open(path) as f:
        for r in csv.DictReader(f):
            rows.append({k: float(v) for k, v in r.items()})
    audio = [r for r in rows if not r["muted"]]
    if len(audio) < 100:
        sys.exit("fewer than 100 unmuted packets in the trace")
    seconds = (audio[-1]["arrival_us"] - audio[0]["arrival_us"]) / 1e6
    print(f"{len(audio)} packets over {seconds:.1f} s")

    gaps = [(b["arrival_us"] - a["arrival_us"]) / 1e3 for a, b in zip(audio, audio[1:])]
    ordered = sorted(gaps)
    print("\ninter-arrival gap (ms), ideal 10:")
    print("  " + "  ".join(f"p{p}={pct(ordered, p):.1f}" for p in (1, 50, 90, 99, 99.9)) + f"  max={ordered[-1]:.1f}")
    for limit in (20, 30, 50, 80, 120):
        print(f"  gaps over {limit} ms: {sum(g > limit for g in gaps)}")
    bursts = sum(g < 2 for g in gaps)
    print(f"  packets arriving <2 ms after the previous one (catching up after a stall): {bursts}")

    # Clock drift: arrival minus media time rises by drift ppm. The per-5 s minimum ignores queueing delay.
    t0, ts0 = audio[0]["arrival_us"], audio[0]["timestamp"]
    offsets, counts = {}, {}
    for r in audio:
        media_s = ((r["timestamp"] - ts0) % 2**32) / SAMPLE_RATE
        arrival_s = (r["arrival_us"] - t0) / 1e6
        window = int(arrival_s // 5)
        offsets[window] = min(offsets.get(window, 1e9), arrival_s - media_s)
        counts[window] = counts.get(window, 0) + 1
    xs = sorted(w for w in offsets if counts[w] >= 400)  # skip the partial first and last windows
    if len(xs) >= 3:
        n = len(xs)
        mx = sum(xs) / n
        my = sum(offsets[x] for x in xs) / n
        slope = sum((x - mx) * (offsets[x] - my) for x in xs) / sum((x - mx) ** 2 for x in xs)
        print(f"\nclock drift (phone clock vs PC clock): {slope / 5 * 1e6:+.0f} ppm (positive = phone slower)")
        print(f"  = {slope / 5 * 1e3 * 10:+.2f} ms of buffer change per 10 s")

    seqs = [r["sequence"] for r in audio]
    missing = sum(max(0, int(b - a) - 1) for a, b in zip(seqs, seqs[1:]) if 0 < b - a < 1000)
    reordered = sum(b < a for a, b in zip(seqs, seqs[1:]))
    print(f"\nsequence gaps (packets never seen in order): {missing}, out of order: {reordered}")

    last = rows[-1]
    first = rows[0]
    minutes = seconds / 60
    print(f"\nbuffer counters over the trace: underruns {last['underruns'] - first['underruns']:.0f} "
          f"({(last['underruns'] - first['underruns']) / minutes:.1f}/min), "
          f"dropped {last['dropped'] - first['dropped']:.0f}, lost {last['lost'] - first['lost']:.0f}")

    print("\neach underrun: gap before the packet that revealed it, and the buffer level before that gap")
    shown = 0
    for prev, cur in zip(audio, audio[1:]):
        if cur["underruns"] > prev["underruns"]:
            gap = (cur["arrival_us"] - prev["arrival_us"]) / 1e3
            at = (cur["arrival_us"] - audio[0]["arrival_us"]) / 1e6
            print(f"  t={at:6.1f}s  gap {gap:6.1f} ms  buffer before {prev['buffered_frames'] * 10:.0f} ms  "
                  f"(+{cur['underruns'] - prev['underruns']:.0f})")
            shown += 1
            if shown == 25:
                print("  ...")
                break


if __name__ == "__main__":
    main(sys.argv[1])
