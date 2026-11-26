//! Audio track mixing (gain, resample, time-stretch) and `cpal` output
//! (see ARCHITECTURE.md § Audio pipeline). The mixer position is the
//! playback clock.

pub mod autolevel;
pub mod beat;
pub mod blocks;
pub mod denoise;
pub mod dynamics;
pub mod effect;
pub mod eq;
mod filter;
pub mod levels;
pub mod loudness;
pub mod mixer;
pub mod recorder;
pub mod spectrum;
pub mod stretch;
pub mod stretched;
pub mod sync;

pub use blocks::{BLOCK_FRAMES, SampleBlocks};
pub use effect::{PluginHandle, PluginProcessor, PluginRequest};
pub use levels::{LevelAnalysis, LevelSegment, Measured};
pub use mixer::{
    AnalysisSlot, AudioSource, BandMeter, ClipAudio, ClipInput, LevelReading, MIX_BLOCK_FRAMES,
    MixSnapshot, Mixer, MixerState, StretchedWindow, StripKeys, mix_range_without_plugins,
    plugin_key, plugin_key_in, plugin_keys_in,
};
pub use stretch::{STRETCH_CHUNK_FRAMES, Stretcher, stretch_samples};
pub use stretched::{StretchInput, StretchRequest};

/// Reverses interleaved samples frame by frame, keeping the channel order.
pub fn reverse_frames(samples: &mut [f32], channels: u16) {
    let channels = channels.max(1) as usize;
    let frames = samples.len() / channels;
    for i in 0..frames / 2 {
        let j = frames - 1 - i;
        for c in 0..channels {
            samples.swap(i * channels + c, j * channels + c);
        }
    }
}
