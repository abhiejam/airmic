//! Jitter buffer: orders packets by sequence and plays them out at the audio sink's pace.

use std::collections::BTreeMap;
use std::time::Instant;

use airmic_proto::{FRAME_BYTES, FRAME_SAMPLES, Header, SAMPLE_RATE};

type Frame = [i16; FRAME_SAMPLES];

const SILENCE: Frame = [0; FRAME_SAMPLES];
const FRAME_MS: f64 = 10.0;
const MIN_TARGET_FRAMES: usize = 2;
const MAX_TARGET_FRAMES: usize = 12;
/// Frames above target before the buffer drops audio to catch up, e.g. after a Wi-Fi burst.
const MAX_EXCESS_FRAMES: usize = 4;
/// Hard cap so a stuck sink cannot grow the buffer without bound (1 s).
const MAX_BUFFERED_FRAMES: usize = 100;
/// 2 ms ramp around gaps, so loss sounds like a short dropout instead of a click.
const FADE_SAMPLES: usize = 96;

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Stats {
    pub received: u64,
    pub lost: u64,
    pub late: u64,
    pub duplicate: u64,
    pub dropped: u64,
    pub underruns: u64,
    pub jitter_ms: f64,
}

#[derive(Default)]
pub struct JitterBuffer {
    frames: BTreeMap<u64, Frame>,
    /// Highest sequence seen, raw and extended past u32 wrap.
    highest_seq: Option<(u32, u64)>,
    next_seq: u64,
    playing: bool,
    muted: bool,
    fade_in: bool,
    last_arrival: Option<(Instant, u32)>,
    current: Option<(Frame, usize)>,
    stats: Stats,
}

impl JitterBuffer {
    pub fn new() -> Self {
        // Start the estimate at 5 ms so the initial target is 40 ms (PRD §8.1).
        let mut jb = JitterBuffer::default();
        jb.stats.jitter_ms = 5.0;
        jb
    }

    /// Adds one validated packet. `payload` is 960 bytes, or empty when the packet is muted.
    pub fn push(&mut self, header: &Header, payload: &[u8], arrival: Instant) {
        let seq = self.extend_seq(header.sequence);
        if header.muted {
            self.muted = true;
            self.frames.clear();
            self.playing = false;
            return;
        }
        self.muted = false;
        self.update_jitter(header.timestamp, arrival);

        if seq < self.next_seq {
            self.stats.late += 1;
            return;
        }
        if self.frames.contains_key(&seq) {
            self.stats.duplicate += 1;
            return;
        }
        assert_eq!(
            payload.len(),
            FRAME_BYTES,
            "receiver validates payload size"
        );
        let mut frame = SILENCE;
        for (s, b) in frame.iter_mut().zip(payload.as_chunks::<2>().0) {
            *s = i16::from_le_bytes(*b);
        }
        self.frames.insert(seq, frame);
        self.stats.received += 1;
        while self.frames.len() > MAX_BUFFERED_FRAMES {
            self.frames.pop_first();
            self.stats.dropped += 1;
        }
    }

    /// Fills `out` with the next samples. Outputs silence while muted, priming or starved.
    pub fn read(&mut self, out: &mut [i16]) {
        let mut filled = 0;
        while filled < out.len() {
            let (frame, pos) = match self.current.take() {
                Some(current) => current,
                None => (self.next_frame(), 0),
            };
            let n = (FRAME_SAMPLES - pos).min(out.len() - filled);
            out[filled..filled + n].copy_from_slice(&frame[pos..pos + n]);
            filled += n;
            if pos + n < FRAME_SAMPLES {
                self.current = Some((frame, pos + n));
            }
        }
    }

    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// Returns the buffered audio in milliseconds, for the latency estimate.
    pub fn delay_ms(&self) -> f64 {
        self.frames.len() as f64 * FRAME_MS
    }

    fn target_frames(&self) -> usize {
        let target_ms = 20.0 + 4.0 * self.stats.jitter_ms;
        ((target_ms / FRAME_MS).ceil() as usize).clamp(MIN_TARGET_FRAMES, MAX_TARGET_FRAMES)
    }

    fn next_frame(&mut self) -> Frame {
        if self.muted {
            return SILENCE;
        }
        if !self.playing {
            if self.frames.len() < self.target_frames() {
                return SILENCE;
            }
            // Resume at the oldest frame; anything missing before it is not worth the latency.
            self.next_seq = *self.frames.keys().next().unwrap();
            self.playing = true;
        }
        let span = |jb: &Self| {
            jb.frames
                .keys()
                .next_back()
                .map_or(0, |&last| last + 1 - jb.next_seq)
        };
        while span(self) as usize > self.target_frames() + MAX_EXCESS_FRAMES {
            if self.frames.remove(&self.next_seq).is_some() {
                self.stats.dropped += 1;
            }
            self.next_seq += 1;
            self.fade_in = true;
        }
        if self.frames.is_empty() {
            self.playing = false;
            self.fade_in = true;
            self.stats.underruns += 1;
            return SILENCE;
        }

        let seq = self.next_seq;
        self.next_seq += 1;
        let Some(mut frame) = self.frames.remove(&seq) else {
            self.stats.lost += 1;
            self.fade_in = true;
            return SILENCE;
        };
        if std::mem::take(&mut self.fade_in) {
            for (i, s) in frame[..FADE_SAMPLES].iter_mut().enumerate() {
                *s = (i32::from(*s) * i as i32 / FADE_SAMPLES as i32) as i16;
            }
        }
        if !self.frames.contains_key(&self.next_seq) {
            for (i, s) in frame[FRAME_SAMPLES - FADE_SAMPLES..]
                .iter_mut()
                .rev()
                .enumerate()
            {
                *s = (i32::from(*s) * i as i32 / FADE_SAMPLES as i32) as i16;
            }
        }
        frame
    }

    /// Maps a u32 sequence onto a u64 that keeps counting past the wrap.
    fn extend_seq(&mut self, seq: u32) -> u64 {
        let ext = match self.highest_seq {
            // Start at 2^32 so packets slightly older than the first one stay positive.
            None => (1 << 32) + u64::from(seq),
            Some((raw, ext)) => ext.wrapping_add_signed(i64::from(seq.wrapping_sub(raw) as i32)),
        };
        if self.highest_seq.is_none_or(|(_, highest)| ext > highest) {
            self.highest_seq = Some((seq, ext));
        }
        ext
    }

    /// Updates the RFC 3550 interarrival jitter estimate.
    fn update_jitter(&mut self, timestamp: u32, arrival: Instant) {
        if let Some((prev_arrival, prev_ts)) = self.last_arrival {
            let arrival_ms = arrival
                .saturating_duration_since(prev_arrival)
                .as_secs_f64()
                * 1e3;
            let media_ms =
                f64::from(timestamp.wrapping_sub(prev_ts) as i32) * 1e3 / f64::from(SAMPLE_RATE);
            let d = (arrival_ms - media_ms).abs();
            self.stats.jitter_ms += (d - self.stats.jitter_ms) / 16.0;
        }
        self.last_arrival = Some((arrival, timestamp));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Pushes a frame whose samples all equal `seq % 1000 + 1`, arriving on a perfect 10 ms clock.
    fn push(jb: &mut JitterBuffer, start: Instant, seq: u32) {
        let header = Header {
            muted: false,
            codec: 0,
            session_id: 1,
            sequence: seq,
            timestamp: seq.wrapping_mul(FRAME_SAMPLES as u32),
        };
        let value = (seq % 1000 + 1) as i16;
        let payload: Vec<u8> = [value; FRAME_SAMPLES]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        // Offset by 10 so sequences just below the wrap still get small, ordered arrival times.
        let arrival = start + Duration::from_millis(u64::from(seq.wrapping_add(10) % 1000) * 10);
        jb.push(&header, &payload, arrival);
    }

    fn push_muted(jb: &mut JitterBuffer, seq: u32) {
        let header = Header {
            muted: true,
            codec: 0,
            session_id: 1,
            sequence: seq,
            timestamp: 0,
        };
        jb.push(&header, &[], Instant::now());
    }

    /// Plays `n` frames and returns each frame's middle sample, which fades never touch.
    fn play(jb: &mut JitterBuffer, n: usize) -> Vec<i16> {
        let mut out = vec![0; n * FRAME_SAMPLES];
        jb.read(&mut out);
        out.chunks(FRAME_SAMPLES)
            .map(|f| f[FRAME_SAMPLES / 2])
            .collect()
    }

    #[test]
    fn primes_to_40ms_then_plays_in_order() {
        let (mut jb, t) = (JitterBuffer::new(), Instant::now());
        for seq in 0..3 {
            push(&mut jb, t, seq);
        }
        assert_eq!(play(&mut jb, 1), [0], "silent until 4 frames are buffered");
        push(&mut jb, t, 3);
        assert_eq!(play(&mut jb, 4), [1, 2, 3, 4]);
    }

    #[test]
    fn reads_of_any_size_stitch_frames_together() {
        let (mut jb, t) = (JitterBuffer::new(), Instant::now());
        for seq in 0..4 {
            push(&mut jb, t, seq);
        }
        let mut out = vec![0; 2 * FRAME_SAMPLES];
        for chunk in out.chunks_mut(128) {
            jb.read(chunk);
        }
        assert!(out[..FRAME_SAMPLES].iter().all(|&s| s == 1));
        assert!(out[FRAME_SAMPLES..].iter().all(|&s| s == 2));
    }

    #[test]
    fn reorders_and_drops_duplicates() {
        let (mut jb, t) = (JitterBuffer::new(), Instant::now());
        for seq in [0, 2, 1, 2, 3] {
            push(&mut jb, t, seq);
        }
        assert_eq!(play(&mut jb, 4), [1, 2, 3, 4]);
        assert_eq!(jb.stats().duplicate, 1);
    }

    #[test]
    fn lost_frame_is_silence_with_fades_around_it() {
        let (mut jb, t) = (JitterBuffer::new(), Instant::now());
        for seq in [0, 1, 3, 4, 5] {
            push(&mut jb, t, seq);
        }
        let mut out = vec![0; 4 * FRAME_SAMPLES];
        jb.read(&mut out);
        let frames: Vec<&[i16]> = out.chunks(FRAME_SAMPLES).collect();
        assert_eq!(frames[1].last(), Some(&0), "frame before the gap fades out");
        assert!(frames[2].iter().all(|&s| s == 0), "lost frame is silence");
        assert_eq!(frames[3][0], 0, "frame after the gap fades in");
        assert_eq!(frames[3][FRAME_SAMPLES / 2], 4);
        assert_eq!(jb.stats().lost, 1);
    }

    #[test]
    fn late_frame_is_dropped() {
        let (mut jb, t) = (JitterBuffer::new(), Instant::now());
        for seq in [0, 2, 3, 4] {
            push(&mut jb, t, seq);
        }
        assert_eq!(play(&mut jb, 2), [1, 0]);
        push(&mut jb, t, 1);
        assert_eq!(play(&mut jb, 1), [3]);
        assert_eq!(jb.stats().late, 1);
    }

    #[test]
    fn sequence_wrap_keeps_order() {
        let (mut jb, t) = (JitterBuffer::new(), Instant::now());
        let seqs = [u32::MAX - 1, 0, u32::MAX, 1];
        for seq in seqs {
            push(&mut jb, t, seq);
        }
        let expected: Vec<i16> = [u32::MAX - 1, u32::MAX, 0, 1]
            .iter()
            .map(|s| (s % 1000 + 1) as i16)
            .collect();
        assert_eq!(play(&mut jb, 4), expected);
    }

    #[test]
    fn mute_outputs_silence_and_reprimes_on_unmute() {
        let (mut jb, t) = (JitterBuffer::new(), Instant::now());
        for seq in 0..4 {
            push(&mut jb, t, seq);
        }
        push_muted(&mut jb, 4);
        assert_eq!(play(&mut jb, 2), [0, 0]);
        for seq in 5..9 {
            push(&mut jb, t, seq);
        }
        assert_eq!(play(&mut jb, 4), [6, 7, 8, 9]);
    }

    #[test]
    fn underrun_rebuffers_and_skips_the_gap() {
        let (mut jb, t) = (JitterBuffer::new(), Instant::now());
        for seq in 0..4 {
            push(&mut jb, t, seq);
        }
        assert_eq!(play(&mut jb, 5), [1, 2, 3, 4, 0]);
        assert_eq!(jb.stats().underruns, 1);
        // The phone comes back 2 s later: play its new audio, not 2 s of missing frames.
        for seq in 200..204 {
            push(&mut jb, t, seq);
        }
        assert_eq!(play(&mut jb, 4), [201, 202, 203, 204]);
        assert_eq!(jb.stats().lost, 0);
    }

    #[test]
    fn burst_beyond_target_is_dropped_to_catch_up() {
        let (mut jb, t) = (JitterBuffer::new(), Instant::now());
        for seq in 0..20 {
            push(&mut jb, t, seq);
        }
        // Everything beyond target + excess goes, so latency drops back right away.
        let keep = jb.target_frames() + MAX_EXCESS_FRAMES;
        assert_eq!(play(&mut jb, 1), [(20 - keep + 1) as i16]);
        assert_eq!(jb.stats().dropped, (20 - keep) as u64);
    }

    #[test]
    fn jitter_raises_the_target() {
        let (mut jb, t) = (JitterBuffer::new(), Instant::now());
        for seq in 0..200u32 {
            let header = Header {
                muted: false,
                codec: 0,
                session_id: 1,
                sequence: seq,
                timestamp: seq * FRAME_SAMPLES as u32,
            };
            // Packets arrive in pairs every 20 ms, as Wi-Fi power saving tends to deliver them.
            let arrival = t + Duration::from_millis(u64::from(seq / 2) * 20 + 30);
            jb.push(&header, &[0; FRAME_BYTES], arrival);
            let mut out = [0; FRAME_SAMPLES];
            jb.read(&mut out);
        }
        assert!((jb.stats().jitter_ms - 10.0).abs() < 0.1);
        // 20 ms + 4 × 10 ms jitter = 60 ms.
        assert_eq!(jb.target_frames(), 6);
    }
}
