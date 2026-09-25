//! Pulse keepalive timing.
//!
//! Most of the cycle is digital silence. A short 1 Hz sine burst
//! keeps the Bluetooth link awake.

use std::f64::consts::PI;

/// Pulse timing and level. Field names match `config.json` keys.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PulseParams {
    /// Output sample rate in Hz.
    pub sample_rate: u32,
    /// Burst length in seconds.
    pub duration_sec: f64,
    /// Repeat interval in seconds.
    pub interval_sec: f64,
    /// Burst peak amplitude, linear gain.
    pub amplitude: f64,
}

impl PulseParams {
    /// Samples per full cycle. At least 1.
    #[must_use]
    pub fn cycle_len(&self) -> u64 {
        let cycle = f64::from(self.sample_rate) * self.interval_sec;
        (cycle.round() as u64).max(1)
    }

    /// Samples per burst. At most `cycle - 1` so silence always remains.
    #[must_use]
    pub fn pulse_len(&self) -> u64 {
        let cycle = self.cycle_len();
        let len = (f64::from(self.sample_rate) * self.duration_sec).round() as u64;
        len.clamp(1, cycle.saturating_sub(1).max(1))
    }
}

/// Playback position in samples since stream start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PulseState {
    /// Next sample index to render.
    pub pos: u64,
}

/// Fill `out` (interleaved stereo `f32`) with the pulse signal.
///
/// `out.len()` must be even. Frames outside the burst are silence.
/// Advances `state.pos`, wrapping to avoid
/// unbounded `u64` growth on very long runs.
pub fn fill_pulse_stereo(out: &mut [f32], params: &PulseParams, state: &mut PulseState) {
    debug_assert_eq!(out.len() % 2, 0);
    let frames = out.len() / 2;
    let cycle = params.cycle_len();
    let pulse_len = params.pulse_len();
    let sr = f64::from(params.sample_rate.max(1));

    let (frames_out, _remainder) = out.as_chunks_mut::<2>();
    for (i, frame) in frames_out.iter_mut().enumerate() {
        let idx = state.pos + i as u64;
        if idx % cycle < pulse_len {
            let t = idx as f64 / sr;
            #[allow(clippy::cast_possible_truncation)]
            let v = (params.amplitude * (2.0 * PI * 1.0 * t).sin()) as f32;
            frame[0] = v;
            frame[1] = v;
        } else {
            frame[0] = 0.0;
            frame[1] = 0.0;
        }
    }
    state.pos += frames as u64;
    if state.pos >= cycle.saturating_mul(1000) {
        state.pos %= cycle;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> PulseParams {
        PulseParams {
            sample_rate: 44_100,
            duration_sec: 1.0,
            interval_sec: 55.0,
            amplitude: 0.0001,
        }
    }

    #[test]
    fn cycle_and_pulse_lengths() {
        let p = params();
        assert_eq!(p.cycle_len(), 2_425_500);
        assert_eq!(p.pulse_len(), 44_100);
    }

    #[test]
    fn burst_at_start_then_silence() {
        let p = params();
        let mut state = PulseState::default();
        let mut out = vec![0.0f32; 2 * 512];
        fill_pulse_stereo(&mut out, &p, &mut state);
        // First frame is sin(0) == 0, later frames in the burst are nonzero.
        assert!(out[2..100].iter().any(|v| v.abs() > 0.0));
        assert!(out.iter().all(|v| v.abs() <= 0.0001 + 1e-9));
        assert_eq!(state.pos, 512);
    }

    #[test]
    fn silence_mid_cycle() {
        let p = params();
        let mut state = PulseState { pos: 100_000 };
        let mut out = vec![0.0f32; 2 * 256];
        fill_pulse_stereo(&mut out, &p, &mut state);
        assert!(out.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn stereo_channels_match() {
        let p = params();
        let mut state = PulseState::default();
        let mut out = vec![0.0f32; 2 * 128];
        fill_pulse_stereo(&mut out, &p, &mut state);
        let (frames, _) = out.as_chunks::<2>();
        for frame in frames {
            assert_eq!(frame[0], frame[1]);
        }
    }
}
