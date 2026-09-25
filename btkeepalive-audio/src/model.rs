//! Lock-free realtime render model.
//!
//! `RenderModel` owns the precomputed tables plus playback positions.
//! The audio thread calls `render_into`, which writes interleaved stereo
//! `f32` and applies the current volume from an external atomic. Volume
//! travels as `f32` bits in an `AtomicU32` so the UI thread can update it
//! without locking.

use btkeepalive_core::config::KeepaliveMode;
use btkeepalive_core::dsp_noise::NoisePreset;
use btkeepalive_core::pulse::{fill_pulse_stereo, PulseParams, PulseState};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::tables::{build_binaural_table, build_preset_table, TABLE_SECONDS};

/// Snapshot of playback settings used to (re)build tables.
#[derive(Debug, Clone, Copy)]
pub struct RenderParams {
    /// Active preset, or binaural carrier when `preset` is binaural.
    pub preset: NoisePresetKind,
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Binaural carrier in Hz. Ignored for noise presets.
    pub carrier_hz: f64,
    /// Continuous or pulse keepalive.
    pub mode: KeepaliveMode,
    /// Pulse timing. Ignored in continuous mode.
    pub pulse: PulseParams,
}

/// Noise preset plus the binaural variants.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NoisePresetKind {
    /// One of the five colored noises.
    Noise(NoisePreset),
    /// Binaural beats at the given beat frequency in Hz.
    Binaural { beat_hz: f64 },
}

/// Realtime renderer. Not `Sync`; share via atomics held outside.
pub struct RenderModel {
    params: RenderParams,
    /// Mono table for noise presets.
    noise_table: Vec<f32>,
    /// Interleaved stereo table for binaural.
    binaural_table: Vec<f32>,
    noise_pos: usize,
    binaural_pos: usize,
    pulse_state: PulseState,
}

impl RenderModel {
    /// Build tables for `params`. Costs a few ms; call off the hot path.
    #[must_use]
    pub fn build(params: RenderParams) -> Self {
        let sr = params.sample_rate.max(1);
        let (noise_table, binaural_table) = match params.preset {
            NoisePresetKind::Noise(p) => (build_preset_table(p, sr, TABLE_SECONDS), Vec::new()),
            NoisePresetKind::Binaural { beat_hz } => (
                Vec::new(),
                build_binaural_table(sr, params.carrier_hz, beat_hz, TABLE_SECONDS),
            ),
        };
        Self {
            params,
            noise_table,
            binaural_table,
            noise_pos: 0,
            binaural_pos: 0,
            pulse_state: PulseState::default(),
        }
    }

    /// Current params.
    #[must_use]
    pub fn params(&self) -> RenderParams {
        self.params
    }

    /// Reset loop positions, for preset or mode switches.
    pub fn reset_positions(&mut self) {
        self.noise_pos = 0;
        self.binaural_pos = 0;
        self.pulse_state = PulseState::default();
    }

    /// Render `frames` stereo frames into `out` (length `2 * frames`).
    ///
    /// Reads volume once per call. When `playing` is false, writes silence
    /// and holds positions so resume continues cleanly.
    pub fn render_into(&mut self, out: &mut [f32], volume: f32, playing: bool) {
        debug_assert_eq!(out.len() % 2, 0);
        if !playing {
            out.fill(0.0);
            return;
        }
        if self.params.mode == KeepaliveMode::Pulse {
            fill_pulse_stereo(out, &self.params.pulse, &mut self.pulse_state);
            apply_gain(out, volume);
            return;
        }
        match self.params.preset {
            NoisePresetKind::Noise(_) => {
                blit_mono_loop(&self.noise_table, &mut self.noise_pos, out);
            }
            NoisePresetKind::Binaural { .. } => {
                blit_stereo_loop(&self.binaural_table, &mut self.binaural_pos, out);
            }
        }
        apply_gain(out, volume);
    }
}

/// Build render params from app config at `sample_rate`.
///
/// Shared by the Windows audio manager and headless smoke tests.
#[must_use]
pub fn params_from_config(
    config: &btkeepalive_core::config::Config,
    sample_rate: u32,
) -> RenderParams {
    use btkeepalive_core::config::Preset;
    let preset = match config.preset {
        Preset::White => NoisePresetKind::Noise(NoisePreset::White),
        Preset::Pink => NoisePresetKind::Noise(NoisePreset::Pink),
        Preset::Brown => NoisePresetKind::Noise(NoisePreset::Brown),
        Preset::Blue => NoisePresetKind::Noise(NoisePreset::Blue),
        Preset::Violet => NoisePresetKind::Noise(NoisePreset::Violet),
        p => NoisePresetKind::Binaural {
            beat_hz: p.beat_hz().unwrap_or(40.0),
        },
    };
    RenderParams {
        preset,
        sample_rate,
        carrier_hz: f64::from(config.carrier_hz),
        mode: config.keepalive_mode,
        pulse: btkeepalive_core::pulse::PulseParams {
            sample_rate,
            duration_sec: config.pulse_duration_sec,
            interval_sec: config.pulse_interval_sec,
            amplitude: config.pulse_amplitude,
        },
    }
}

/// Load a shared atomic volume as `f32`.
#[must_use]
pub fn load_volume(shared: &AtomicU32) -> f32 {
    f32::from_bits(shared.load(Ordering::Relaxed))
}

/// Store a shared atomic volume from `f32`.
pub fn store_volume(shared: &AtomicU32, volume: f32) {
    shared.store(volume.to_bits(), Ordering::Relaxed);
}

fn apply_gain(out: &mut [f32], volume: f32) {
    if (volume - 1.0).abs() < f32::EPSILON {
        for v in out.iter_mut() {
            *v = v.clamp(-1.0, 1.0);
        }
        return;
    }
    for v in out.iter_mut() {
        *v = (*v * volume).clamp(-1.0, 1.0);
    }
}

fn blit_mono_loop(table: &[f32], pos: &mut usize, out: &mut [f32]) {
    if table.is_empty() {
        out.fill(0.0);
        return;
    }
    let (frames, _) = out.as_chunks_mut::<2>();
    for frame in frames {
        let s = table[*pos];
        frame[0] = s;
        frame[1] = s;
        *pos += 1;
        if *pos >= table.len() {
            *pos = 0;
        }
    }
}

fn blit_stereo_loop(table: &[f32], pos: &mut usize, out: &mut [f32]) {
    if table.is_empty() {
        out.fill(0.0);
        return;
    }
    debug_assert_eq!(table.len() % 2, 0);
    let (frames, _) = out.as_chunks_mut::<2>();
    let table_frames = table.len() / 2;
    for frame in frames {
        let i = (*pos % table_frames) * 2;
        frame[0] = table[i];
        frame[1] = table[i + 1];
        *pos += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use btkeepalive_core::dsp_noise::NoisePreset;

    fn continuous_brown() -> RenderModel {
        RenderModel::build(RenderParams {
            preset: NoisePresetKind::Noise(NoisePreset::Brown),
            sample_rate: 48_000,
            carrier_hz: 200.0,
            mode: KeepaliveMode::Continuous,
            pulse: PulseParams {
                sample_rate: 48_000,
                duration_sec: 1.0,
                interval_sec: 55.0,
                amplitude: 0.0001,
            },
        })
    }

    #[test]
    fn renders_audible_signal_at_volume() {
        let mut m = continuous_brown();
        let mut out = vec![0.0f32; 2 * 1024];
        m.render_into(&mut out, 0.02, true);
        assert!(out.iter().any(|v| v.abs() > 1e-9));
        assert!(out.iter().all(|v| v.abs() <= 0.02 + 1e-6));
    }

    #[test]
    fn paused_renders_silence_and_holds_position() {
        let mut m = continuous_brown();
        let mut out = vec![0.0f32; 2 * 256];
        m.render_into(&mut out, 0.02, false);
        assert!(out.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn volume_scales_linearly() {
        let mut a = continuous_brown();
        let mut b = continuous_brown();
        let mut la = vec![0.0f32; 2 * 512];
        let mut lb = vec![0.0f32; 2 * 512];
        a.render_into(&mut la, 0.02, true);
        b.render_into(&mut lb, 0.04, true);
        for (x, y) in la.iter().zip(lb.iter()) {
            assert!((y - 2.0 * x).abs() < 1e-6, "{y} vs 2*{x}");
        }
    }

    #[test]
    fn stereo_channels_match_for_noise() {
        let mut m = continuous_brown();
        let mut out = vec![0.0f32; 2 * 256];
        m.render_into(&mut out, 1.0, true);
        let (frames, _) = out.as_chunks::<2>();
        for f in frames {
            assert_eq!(f[0], f[1]);
        }
    }

    #[test]
    fn binaural_loops_cleanly() {
        for beat_hz in [40.0, 10.0, 6.0] {
            let mut m = RenderModel::build(RenderParams {
                preset: NoisePresetKind::Binaural { beat_hz },
                sample_rate: 48_000,
                carrier_hz: 200.0,
                mode: KeepaliveMode::Continuous,
                pulse: PulseParams {
                    sample_rate: 48_000,
                    duration_sec: 1.0,
                    interval_sec: 55.0,
                    amplitude: 0.0001,
                },
            });
            let mut out = vec![0.0f32; 2 * 480_000 + 4];
            m.render_into(&mut out, 1.0, true);
            assert!(out.iter().all(|v| (-1.0..=1.0).contains(v)), "{beat_hz} Hz");
        }
    }

    #[test]
    fn atomic_volume_roundtrips() {
        let shared = AtomicU32::new(0.02f32.to_bits());
        assert!((load_volume(&shared) - 0.02).abs() < 1e-9);
        store_volume(&shared, 0.05);
        assert!((load_volume(&shared) - 0.05).abs() < 1e-9);
    }
}
