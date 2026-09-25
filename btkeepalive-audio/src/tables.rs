//! One-time DSP table builders.
//!
//! Tables hold a few seconds of audio per preset. The realtime callback
//! loops over them with an integer offset, which replaces per-callback
//! signal generation on the hot path.

use btkeepalive_core::dsp_binaural::BinauralGenerator;
use btkeepalive_core::dsp_noise::{NoiseGenerator, NoisePreset};
use rand::rngs::StdRng;
use rand::SeedableRng;

/// Seconds of audio precomputed per preset table.
pub const TABLE_SECONDS: u32 = 10;

/// Build a mono `f32` table for one noise preset.
///
/// Uses a seed derived from the preset and sample rate so tables are
/// identical across runs on the same machine. Deterministic output
/// keeps tests stable and avoids a startup burst of `thread_rng` use.
#[must_use]
pub fn build_preset_table(preset: NoisePreset, sample_rate: u32, seconds: u32) -> Vec<f32> {
    let n = (u64::from(sample_rate.max(1)) * u64::from(seconds.max(1))) as usize;
    let seed = u64::from(sample_rate) ^ (preset_seed(preset) << 32);
    let mut rng = StdRng::seed_from_u64(seed);
    NoiseGenerator::new(preset).generate(n, &mut rng)
}

/// Build an interleaved stereo `f32` table for binaural beats.
#[must_use]
pub fn build_binaural_table(
    sample_rate: u32,
    carrier_hz: f64,
    beat_hz: f64,
    seconds: u32,
) -> Vec<f32> {
    let n = (u64::from(sample_rate.max(1)) * u64::from(seconds.max(1))) as usize;
    BinauralGenerator::new(sample_rate, carrier_hz, beat_hz).generate(n)
}

fn preset_seed(preset: NoisePreset) -> u64 {
    match preset {
        NoisePreset::White => 0x11,
        NoisePreset::Pink => 0x22,
        NoisePreset::Brown => 0x33,
        NoisePreset::Blue => 0x44,
        NoisePreset::Violet => 0x55,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_have_expected_length() {
        let t = build_preset_table(NoisePreset::Brown, 48_000, 10);
        assert_eq!(t.len(), 480_000);
        let b = build_binaural_table(48_000, 200.0, 40.0, 10);
        assert_eq!(b.len(), 960_000);
    }

    #[test]
    fn tables_are_deterministic() {
        let a = build_preset_table(NoisePreset::Pink, 44_100, 2);
        let b = build_preset_table(NoisePreset::Pink, 44_100, 2);
        assert_eq!(a, b);
    }

    #[test]
    fn tables_are_finite() {
        for preset in [
            NoisePreset::White,
            NoisePreset::Pink,
            NoisePreset::Brown,
            NoisePreset::Blue,
            NoisePreset::Violet,
        ] {
            let t = build_preset_table(preset, 48_000, 1);
            assert!(t.iter().all(|v| v.is_finite()), "{preset:?}");
        }
    }
}
