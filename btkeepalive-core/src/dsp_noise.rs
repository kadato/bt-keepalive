//! Colored noise generators.
//!
//! Output is mono `f32`, one value per frame. Callers duplicate to stereo.

use rand::Rng;
use rand_distr::{Distribution, StandardNormal};
use std::str::FromStr;

/// Valid noise presets, in tray order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoisePreset {
    White,
    Pink,
    Brown,
    Blue,
    Violet,
}

impl NoisePreset {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::White => "white",
            Self::Pink => "pink",
            Self::Brown => "brown",
            Self::Blue => "blue",
            Self::Violet => "violet",
        }
    }
}

impl FromStr for NoisePreset {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "white" => Ok(Self::White),
            "pink" => Ok(Self::Pink),
            "brown" => Ok(Self::Brown),
            "blue" => Ok(Self::Blue),
            "violet" => Ok(Self::Violet),
            _ => Err(format!("Unknown noise preset: {s}")),
        }
    }
}

// Fixed gains: per-buffer peak normalization caused clicks, so these stay constant.
const GAIN_WHITE: f64 = 0.22;
const GAIN_PINK: f64 = 0.055;
const GAIN_BROWN: f64 = 18.0;
const GAIN_BLUE: f64 = 0.22;
const GAIN_VIOLET: f64 = 0.32;

// Leaky integrator for stationary brown noise (~-6 dB/octave).
const BROWN_LEAK: f64 = 0.998;
const BROWN_DRIVE: f64 = 0.35;

/// Stateful noise generator. Keep one per stream; it carries filter memory.
#[derive(Debug, Clone)]
pub struct NoiseGenerator {
    preset: NoisePreset,
    pink_rows: [f64; 16],
    pink_counter: u64,
    brown_state: f64,
    blue_prev: f64,
    violet_prev: (f64, f64),
}

impl NoiseGenerator {
    /// Create a generator for `preset`.
    #[must_use]
    pub fn new(preset: NoisePreset) -> Self {
        Self {
            preset,
            pink_rows: [0.0; 16],
            pink_counter: 0,
            brown_state: 0.0,
            blue_prev: 0.0,
            violet_prev: (0.0, 0.0),
        }
    }

    /// Current preset.
    #[must_use]
    pub fn preset(&self) -> NoisePreset {
        self.preset
    }

    /// Generate `n` mono frames.
    pub fn generate<R: Rng + ?Sized>(&mut self, n: usize, rng: &mut R) -> Vec<f32> {
        match self.preset {
            NoisePreset::White => Self::white(n, rng),
            NoisePreset::Pink => self.pink(n, rng),
            NoisePreset::Brown => self.brown(n, rng),
            NoisePreset::Blue => self.blue(n, rng),
            NoisePreset::Violet => self.violet(n, rng),
        }
    }

    fn white<R: Rng + ?Sized>(n: usize, rng: &mut R) -> Vec<f32> {
        (0..n)
            .map(|_| {
                let w: f64 = StandardNormal.sample(rng);
                (w * GAIN_WHITE) as f32
            })
            .collect()
    }

    fn pink<R: Rng + ?Sized>(&mut self, n: usize, rng: &mut R) -> Vec<f32> {
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            self.pink_counter += 1;
            let tz = self.pink_counter.trailing_zeros().min(15) as usize;
            let w: f64 = StandardNormal.sample(rng);
            self.pink_rows[tz] = w;
            let sum: f64 = self.pink_rows.iter().sum();
            out.push((sum * GAIN_PINK) as f32);
        }
        out
    }

    fn brown<R: Rng + ?Sized>(&mut self, n: usize, rng: &mut R) -> Vec<f32> {
        let mut out = Vec::with_capacity(n);
        let mut state = self.brown_state;
        for _ in 0..n {
            let w: f64 = StandardNormal.sample(rng);
            state = BROWN_LEAK * state + (1.0 - BROWN_LEAK) * BROWN_DRIVE * w;
            out.push((state * GAIN_BROWN) as f32);
        }
        self.brown_state = state;
        out
    }

    fn blue<R: Rng + ?Sized>(&mut self, n: usize, rng: &mut R) -> Vec<f32> {
        let mut white = Vec::with_capacity(n + 1);
        for _ in 0..=n {
            white.push(StandardNormal.sample(rng));
        }
        white[0] = self.blue_prev;
        self.blue_prev = white[n];
        white
            .windows(2)
            .map(|w| ((w[1] - w[0]) * GAIN_BLUE) as f32)
            .collect()
    }

    fn violet<R: Rng + ?Sized>(&mut self, n: usize, rng: &mut R) -> Vec<f32> {
        let mut white = Vec::with_capacity(n + 2);
        for _ in 0..n + 2 {
            white.push(StandardNormal.sample(rng));
        }
        white[0] = self.violet_prev.0;
        white[1] = self.violet_prev.1;
        self.violet_prev = (white[n], white[n + 1]);
        white
            .windows(3)
            .map(|w| ((w[2] - 2.0 * w[1] + w[0]) * GAIN_VIOLET) as f32)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn seeded_rng() -> StdRng {
        StdRng::seed_from_u64(0xC0FFEE)
    }

    #[test]
    fn all_presets_finite() {
        for preset in [
            NoisePreset::White,
            NoisePreset::Pink,
            NoisePreset::Brown,
            NoisePreset::Blue,
            NoisePreset::Violet,
        ] {
            let mut g = NoiseGenerator::new(preset);
            let out = g.generate(1024, &mut seeded_rng());
            assert_eq!(out.len(), 1024);
            assert!(out.iter().all(|v| v.is_finite()), "{preset:?}");
        }
    }

    #[test]
    fn unknown_preset_errors() {
        assert!("purple".parse::<NoisePreset>().is_err());
    }

    #[test]
    fn brown_state_is_continuous() {
        let mut a = NoiseGenerator::new(NoisePreset::Brown);
        let mut b = NoiseGenerator::new(NoisePreset::Brown);
        let mut rng_a = seeded_rng();
        let mut rng_b = seeded_rng();
        let mut joined = a.generate(512, &mut rng_a);
        joined.extend(a.generate(512, &mut rng_a));
        let whole = b.generate(1024, &mut rng_b);
        assert_eq!(joined, whole);
    }

    #[test]
    fn pink_state_is_continuous() {
        let mut a = NoiseGenerator::new(NoisePreset::Pink);
        let mut b = NoiseGenerator::new(NoisePreset::Pink);
        let mut rng_a = seeded_rng();
        let mut rng_b = seeded_rng();
        let mut joined = a.generate(256, &mut rng_a);
        joined.extend(a.generate(256, &mut rng_a));
        let whole = b.generate(512, &mut rng_b);
        assert_eq!(joined, whole);
    }

    #[test]
    fn blue_keeps_edge_continuity() {
        let mut g = NoiseGenerator::new(NoisePreset::Blue);
        let mut rng = seeded_rng();
        let first = g.generate(64, &mut rng);
        let second = g.generate(64, &mut rng);
        assert_eq!(first.len(), 64);
        assert_eq!(second.len(), 64);
        assert!(first.iter().all(|v| v.is_finite()));
        assert!(second.iter().all(|v| v.is_finite()));
    }
}
