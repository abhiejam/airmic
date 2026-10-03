//! Audio output behind a trait, so macOS and Windows can add backends later (PRD §10).

use std::sync::{Arc, Mutex};

use crate::jitter::JitterBuffer;

pub type SharedBuffer = Arc<Mutex<JitterBuffer>>;

pub trait AudioSink: Send {
    /// Plays audio pulled from `buffer` at the device's pace. Blocks until output fails.
    fn run(self: Box<Self>, buffer: SharedBuffer) -> anyhow::Result<()>;
}

/// Reads and sets the system's default input. A trait so tests never touch the real desktop.
pub trait DefaultSource: Send + Sync {
    fn is_default(&self) -> bool;
    fn make_default(&self) -> anyhow::Result<()>;
}
