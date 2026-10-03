//! Audio output behind a trait, so macOS and Windows can add backends later (PRD §10).

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use airmic_proto::SAMPLE_RATE;

use crate::jitter::JitterBuffer;

pub type SharedBuffer = Arc<Mutex<JitterBuffer>>;

const LEVEL_WINDOW: usize = SAMPLE_RATE as usize / 20;

/// RMS and peak (0–1) of the last 50 ms of output. Atomics, so the real-time thread never locks.
#[derive(Default)]
pub struct Level {
    rms: AtomicU32,
    peak: AtomicU32,
}

impl Level {
    /// Returns `(rms, peak)`.
    pub fn get(&self) -> (f32, f32) {
        let load = |a: &AtomicU32| f32::from_bits(a.load(Ordering::Relaxed));
        (load(&self.rms), load(&self.peak))
    }

    pub fn set(&self, rms: f32, peak: f32) {
        self.rms.store(rms.to_bits(), Ordering::Relaxed);
        self.peak.store(peak.to_bits(), Ordering::Relaxed);
    }
}

/// Accumulates output samples into 50 ms windows for `Level`, without allocating.
#[derive(Default)]
pub struct LevelMeter {
    sum_squares: f64,
    peak: u16,
    samples: usize,
}

impl LevelMeter {
    /// Adds output samples and publishes each completed 50 ms window to `level`.
    pub fn add(&mut self, samples: &[i16], level: &Level) {
        for s in samples {
            let v = s.unsigned_abs();
            self.sum_squares += f64::from(v) * f64::from(v);
            self.peak = self.peak.max(v);
            self.samples += 1;
            if self.samples == LEVEL_WINDOW {
                let rms = (self.sum_squares / LEVEL_WINDOW as f64).sqrt() / 32768.0;
                level.set(rms as f32, f32::from(self.peak) / 32768.0);
                *self = LevelMeter::default();
            }
        }
    }
}

pub trait AudioSink: Send {
    /// Plays audio pulled from `buffer` at the device's pace. Blocks until output fails.
    fn run(self: Box<Self>, buffer: SharedBuffer) -> anyhow::Result<()>;
}

/// Reads and sets the system's default input. A trait so tests never touch the real desktop.
pub trait DefaultSource: Send + Sync {
    fn is_default(&self) -> bool;
    fn make_default(&self) -> anyhow::Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meter_publishes_each_50_ms_window() {
        let (level, mut meter) = (Level::default(), LevelMeter::default());
        let square: Vec<i16> = (0..LEVEL_WINDOW)
            .map(|i| if i % 2 == 0 { 16384 } else { -16384 })
            .collect();
        meter.add(&square[..LEVEL_WINDOW - 1], &level);
        assert_eq!(level.get(), (0.0, 0.0), "no full window yet");
        meter.add(&square[..1], &level);
        assert_eq!(level.get(), (0.5, 0.5));
        meter.add(&[0; LEVEL_WINDOW], &level);
        assert_eq!(level.get(), (0.0, 0.0));
    }
}
