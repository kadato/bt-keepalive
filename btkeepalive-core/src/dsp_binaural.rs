//! 40 Hz binaural beat generator.
//!
//! Output is interleaved stereo `f32`: left, right, left, right, and so on.

use std::f64::consts::PI;

/// Beat frequency between left and right channels.
pub const BEAT_HZ: f64 = 40.0;

/// Stateful binaural generator. Keep one per stream.
#[derive(Debug, Clone)]
pub struct BinauralGenerator {
    sample_rate: u32,
    carrier_hz: f64,
    beat_hz: f64,
    phase_left: f64,
    phase_right: f64,
}

impl BinauralGenerator {
    /// Create a generator. `carrier_hz` is the left ear frequency.
    #[must_use]
    pub fn new(sample_rate: u32, carrier_hz: f64, beat_hz: f64) -> Self {
        Self {
            sample_rate: sample_rate.max(1),
            carrier_hz,
            beat_hz,
            phase_left: 0.0,
            phase_right: 0.0,
        }
    }

    /// Right ear frequency, clamped below Nyquist.
    #[must_use]
    pub fn right_hz(&self) -> f64 {
        let nyquist = f64::from(self.sample_rate) / 2.0;
        let right = self.carrier_hz + self.beat_hz;
        if right >= nyquist {
            nyquist - 1.0
        } else {
            right
        }
    }

    /// Generate `n` stereo frames as interleaved `f32` of length `2 * n`.
    #[must_use]
    pub fn generate(&mut self, n: usize) -> Vec<f32> {
        let sr = f64::from(self.sample_rate);
        let left_hz = self.carrier_hz;
        let right_hz = self.right_hz();
        let mut out = Vec::with_capacity(n * 2);
        for i in 0..n {
            let t_l = (i as f64 + self.phase_left) / sr;
            let t_r = (i as f64 + self.phase_right) / sr;
            out.push((2.0 * PI * left_hz * t_l).sin() as f32);
            out.push((2.0 * PI * right_hz * t_r).sin() as f32);
        }
        let n_f = n as f64;
        self.phase_left = (self.phase_left + n_f) % sr;
        self.phase_right = (self.phase_right + n_f) % sr;
        out
    }

    /// Left phase in samples, for tests.
    #[must_use]
    pub fn phase_left(&self) -> f64 {
        self.phase_left
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_is_interleaved_stereo_in_range() {
        let mut g = BinauralGenerator::new(44_100, 200.0, BEAT_HZ);
        let out = g.generate(512);
        assert_eq!(out.len(), 1024);
        assert!(out.iter().all(|v| (-1.0..=1.0).contains(v)));
    }

    #[test]
    fn split_calls_equal_single_call() {
        let mut a = BinauralGenerator::new(44_100, 200.0, BEAT_HZ);
        let mut b = BinauralGenerator::new(44_100, 200.0, BEAT_HZ);
        let mut joined = a.generate(256);
        joined.extend(a.generate(256));
        let whole = b.generate(512);
        assert_eq!(joined.len(), whole.len());
        for (x, y) in joined.iter().zip(whole.iter()) {
            assert!((x - y).abs() < 1e-6, "{x} vs {y}");
        }
    }

    #[test]
    fn right_channel_clamps_below_nyquist() {
        let g = BinauralGenerator::new(8_000, 7_990.0, BEAT_HZ);
        assert!(g.right_hz() < 4_000.0);
    }

    #[test]
    fn channels_differ_with_beat() {
        let mut g = BinauralGenerator::new(44_100, 200.0, BEAT_HZ);
        let out = g.generate(441);
        let mut same = 0;
        let (frames, _) = out.as_chunks::<2>();
        for pair in frames {
            if (pair[0] - pair[1]).abs() < 1e-6 {
                same += 1;
            }
        }
        assert!(same < out.len() / 2 - 10, "channels should mostly differ");
    }
}
