//! Hand-written DSP. Each effect lives in its own file, works on mono blocks of
//! [`BLOCK`](crate::audio::block::BLOCK) samples and never allocates after construction.

pub mod chain;
pub mod params;
pub mod smooth;

use crate::audio::block::BLOCK;
use params::Snapshot;

/// A mono voice block.
pub type Block = [f32; BLOCK];

/// Anything that transforms the voice in real time. The built-in effect chain implements this;
/// a neural voice model could later implement it too and slot into the same place.
pub trait VoiceProcessor: Send {
    /// Processes one block in place. Must be real-time safe: no allocation, locks or I/O.
    fn process(&mut self, block: &mut Block, params: &Snapshot);
    /// Delay this processor currently adds, in samples.
    fn latency_samples(&self) -> usize {
        0
    }
}
