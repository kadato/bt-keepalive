//! Portable core for BT KeepAlive.
//!
//! Windows-only audio, tray, and startup code lives outside this crate.
//! Everything here is std plus serde/rand only so it builds and tests
//! on any OS, including WSL Linux.

pub mod config;
pub mod dsp_binaural;
pub mod dsp_noise;
pub mod pulse;
pub mod volume;

pub use config::Config;
pub use dsp_binaural::BinauralGenerator;
pub use dsp_noise::{NoiseGenerator, NoisePreset};
pub use pulse::{PulseParams, PulseState};

/// Audio callback frame count from sample rate and target buffer duration.
///
/// Result is clamped to 64..=8192 frames.
#[must_use]
pub fn blocksize_from_buffer_seconds(sample_rate: u32, buffer_seconds: f64) -> u32 {
    if !buffer_seconds.is_finite() || buffer_seconds <= 0.0 || sample_rate == 0 {
        return 512;
    }
    let frames = f64::from(sample_rate) * buffer_seconds;
    (frames.round() as u32).clamp(64, 8192)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocksize_matches_defaults() {
        // 44100 * 0.012 = 529.2 -> 529
        assert_eq!(blocksize_from_buffer_seconds(44100, 0.012), 529);
    }

    #[test]
    fn blocksize_clamps() {
        assert_eq!(blocksize_from_buffer_seconds(44100, 0.0001), 64);
        assert_eq!(blocksize_from_buffer_seconds(44100, 10.0), 8192);
        assert_eq!(blocksize_from_buffer_seconds(0, 0.012), 512);
        assert_eq!(blocksize_from_buffer_seconds(44100, f64::NAN), 512);
    }
}
