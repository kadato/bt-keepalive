//! Config load, validate, and atomic save.
//!
//! Field names match the keys in `%APPDATA%\BTKeepAlive\config.json`.

use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Application folder name under `%APPDATA%`.
pub const APP_NAME: &str = "BTKeepAlive";

/// Valid sound presets, in tray order.
pub const PRESETS: [&str; 8] = [
    "white",
    "pink",
    "brown",
    "blue",
    "violet",
    "binaural40",
    "binaural10",
    "binaural6",
];

/// Valid binaural carrier frequencies.
pub const CARRIER_OPTIONS: [u32; 5] = [100, 150, 200, 250, 300];

/// Short quiet burst before typical ~60s Bluetooth sleep.
pub const PULSE_DURATION_SEC: f64 = 1.0;
/// Pulse repeat interval.
pub const PULSE_INTERVAL_SEC: f64 = 55.0;
/// Pulse peak amplitude, linear gain.
pub const PULSE_AMPLITUDE: f64 = 0.0001;

/// Default continuous volume, linear gain (2%).
pub const DEFAULT_VOLUME: f64 = 0.02;
/// Default preset.
pub const DEFAULT_PRESET: &str = "brown";

/// Sound preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    White,
    Pink,
    Brown,
    Blue,
    Violet,
    #[serde(rename = "binaural40")]
    Binaural40,
    #[serde(rename = "binaural10")]
    Binaural10,
    #[serde(rename = "binaural6")]
    Binaural6,
}

impl Preset {
    /// Parse a preset name, returning `None` for unknown values.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "white" => Some(Self::White),
            "pink" => Some(Self::Pink),
            "brown" => Some(Self::Brown),
            "blue" => Some(Self::Blue),
            "violet" => Some(Self::Violet),
            "binaural40" => Some(Self::Binaural40),
            "binaural10" => Some(Self::Binaural10),
            "binaural6" => Some(Self::Binaural6),
            _ => None,
        }
    }

    /// Canonical preset name used in JSON and the tray.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::White => "white",
            Self::Pink => "pink",
            Self::Brown => "brown",
            Self::Blue => "blue",
            Self::Violet => "violet",
            Self::Binaural40 => "binaural40",
            Self::Binaural10 => "binaural10",
            Self::Binaural6 => "binaural6",
        }
    }

    /// Binaural beat frequency in Hz, or `None` for noise presets.
    #[must_use]
    pub fn beat_hz(self) -> Option<f64> {
        match self {
            Self::Binaural40 => Some(40.0),
            Self::Binaural10 => Some(10.0),
            Self::Binaural6 => Some(6.0),
            _ => None,
        }
    }
}

/// Keepalive playback mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeepaliveMode {
    Continuous,
    Pulse,
}

impl KeepaliveMode {
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "continuous" => Some(Self::Continuous),
            "pulse" => Some(Self::Pulse),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Continuous => "continuous",
            Self::Pulse => "pulse",
        }
    }
}

/// Full application settings. Field names match `config.json` keys.
/// Unknown keys are ignored on load and dropped on save. The engine
/// always renders at the device mix rate with system-managed buffering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub preset: Preset,
    pub volume: f64,
    pub carrier_hz: u32,
    pub keepalive_mode: KeepaliveMode,
    pub pulse_duration_sec: f64,
    pub pulse_interval_sec: f64,
    pub pulse_amplitude: f64,
    pub autoplay: bool,
    pub launch_at_startup: bool,
    pub playing: bool,
    pub check_for_updates: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            preset: Preset::Brown,
            volume: DEFAULT_VOLUME,
            carrier_hz: 200,
            keepalive_mode: KeepaliveMode::Continuous,
            pulse_duration_sec: PULSE_DURATION_SEC,
            pulse_interval_sec: PULSE_INTERVAL_SEC,
            pulse_amplitude: PULSE_AMPLITUDE,
            autoplay: true,
            launch_at_startup: false,
            playing: true,
            check_for_updates: true,
        }
    }
}

fn get_f64(v: &serde_json::Value, key: &str) -> Option<f64> {
    v.get(key)?.as_f64()
}

fn get_bool(v: &serde_json::Value, key: &str) -> Option<bool> {
    v.get(key)?.as_bool()
}

/// Clamp invalid gains to the default. Mirrors `normalize_volume`.
#[must_use]
pub fn normalize_volume(volume: f64) -> f64 {
    if !volume.is_finite() || volume <= 0.0 || volume > 1.0 {
        DEFAULT_VOLUME
    } else {
        volume
    }
}

fn normalize_pulse(
    duration: Option<f64>,
    interval: Option<f64>,
    amplitude: Option<f64>,
) -> (f64, f64, f64) {
    let duration = match duration {
        Some(d) if d.is_finite() => d.max(0.1),
        _ => PULSE_DURATION_SEC,
    };
    let interval = match interval {
        Some(i) if i.is_finite() => i.max(duration + 1.0),
        _ => PULSE_INTERVAL_SEC.max(duration + 1.0),
    };
    let amplitude = match amplitude {
        Some(a) if a.is_finite() => a.clamp(1e-6, 0.01),
        _ => PULSE_AMPLITUDE,
    };
    (duration, interval, amplitude)
}

impl Config {
    /// Parse JSON text, merging over defaults. Corrupt input yields defaults.
    ///
    /// Unknown presets, modes, and carriers fall back to defaults.
    #[must_use]
    pub fn load_from_str(text: &str) -> Self {
        let mut cfg = Self::default();
        let parsed: serde_json::Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(_) => return cfg,
        };

        if let Some(s) = parsed.get("preset").and_then(|v| v.as_str()) {
            if let Some(p) = Preset::parse(s) {
                cfg.preset = p;
            }
        }
        if let Some(s) = parsed.get("keepalive_mode").and_then(|v| v.as_str()) {
            if let Some(m) = KeepaliveMode::parse(s) {
                cfg.keepalive_mode = m;
            }
        }
        if let Some(v) = get_f64(&parsed, "volume") {
            cfg.volume = normalize_volume(v);
        }
        if let Some(n) = parsed.get("carrier_hz").and_then(serde_json::Value::as_u64) {
            #[allow(clippy::cast_possible_truncation)]
            let carrier = n as u32;
            if CARRIER_OPTIONS.contains(&carrier) {
                cfg.carrier_hz = carrier;
            }
        }
        let (duration, interval, amplitude) = normalize_pulse(
            get_f64(&parsed, "pulse_duration_sec"),
            get_f64(&parsed, "pulse_interval_sec"),
            get_f64(&parsed, "pulse_amplitude"),
        );
        // If the file had no pulse keys at all, keep compiled defaults.
        if parsed.get("pulse_duration_sec").is_some()
            || parsed.get("pulse_interval_sec").is_some()
            || parsed.get("pulse_amplitude").is_some()
        {
            cfg.pulse_duration_sec = duration;
            cfg.pulse_interval_sec = interval;
            cfg.pulse_amplitude = amplitude;
        }
        if let Some(b) = get_bool(&parsed, "autoplay") {
            cfg.autoplay = b;
        }
        if let Some(b) = get_bool(&parsed, "launch_at_startup") {
            cfg.launch_at_startup = b;
        }
        if let Some(b) = get_bool(&parsed, "playing") {
            cfg.playing = b;
        }
        if let Some(b) = get_bool(&parsed, "check_for_updates") {
            cfg.check_for_updates = b;
        }
        cfg
    }

    /// Load from a file path. Missing or corrupt files yield defaults.
    #[must_use]
    pub fn load_from_path(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(text) => Self::load_from_str(&text),
            Err(_) => Self::default(),
        }
    }

    /// Save atomically via a `.json.tmp` sibling plus rename.
    pub fn save_to_path(&self, path: &Path) -> io::Result<()> {
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, text)?;
        fs::rename(&tmp, path)?;
        Ok(())
    }
}

/// Resolve the settings folder, creating it: `%APPDATA%\BTKeepAlive`,
/// falling back to the home directory.
pub fn config_dir() -> PathBuf {
    if let Ok(base) = env::var("APPDATA") {
        if !base.is_empty() {
            let path = PathBuf::from(base).join(APP_NAME);
            let _ = fs::create_dir_all(&path);
            return path;
        }
    }
    let home = env::var("USERPROFILE")
        .or_else(|_| env::var("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    let path = home.join(APP_NAME);
    let _ = fs::create_dir_all(&path);
    path
}

/// Full path to `config.json`.
#[must_use]
pub fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_documented() {
        let cfg = Config::default();
        assert_eq!(cfg.preset, Preset::Brown);
        assert_eq!(cfg.volume, DEFAULT_VOLUME);
        assert_eq!(cfg.carrier_hz, 200);
        assert_eq!(cfg.keepalive_mode, KeepaliveMode::Continuous);
        assert_eq!(cfg.pulse_interval_sec, 55.0);
        assert!(cfg.autoplay && cfg.playing);
    }

    #[test]
    fn corrupt_json_yields_defaults() {
        let cfg = Config::load_from_str("{not json");
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn merges_partial_file() {
        let cfg = Config::load_from_str(r#"{"volume": 0.03, "preset": "pink"}"#);
        assert_eq!(cfg.volume, 0.03);
        assert_eq!(cfg.preset, Preset::Pink);
        assert_eq!(cfg.carrier_hz, 200);
    }

    #[test]
    fn unknown_preset_and_carrier_fall_back() {
        let cfg = Config::load_from_str(r#"{"preset": "purple", "carrier_hz": 999}"#);
        assert_eq!(cfg.preset, Preset::Brown);
        assert_eq!(cfg.carrier_hz, 200);
    }

    #[test]
    fn pulse_interval_minimum_holds() {
        let cfg =
            Config::load_from_str(r#"{"pulse_interval_sec": 0.5, "pulse_duration_sec": 1.0}"#);
        assert!(cfg.pulse_interval_sec >= cfg.pulse_duration_sec + 1.0);
    }

    #[test]
    fn invalid_volume_falls_back() {
        assert_eq!(
            Config::load_from_str(r#"{"volume": -1}"#).volume,
            DEFAULT_VOLUME
        );
        assert_eq!(
            Config::load_from_str(r#"{"volume": 2}"#).volume,
            DEFAULT_VOLUME
        );
    }

    #[test]
    fn roundtrip_atomic_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let cfg = Config {
            volume: 0.04,
            ..Config::default()
        };
        cfg.save_to_path(&path).unwrap();
        let loaded = Config::load_from_path(&path);
        assert_eq!(loaded.volume, 0.04);
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn json_keys_match_documented_names() {
        let text = serde_json::to_string(&Config::default()).unwrap();
        for key in [
            "preset",
            "volume",
            "carrier_hz",
            "keepalive_mode",
            "pulse_duration_sec",
            "pulse_interval_sec",
            "pulse_amplitude",
            "autoplay",
            "launch_at_startup",
            "playing",
            "check_for_updates",
        ] {
            assert!(text.contains(key), "missing key {key}");
        }
    }

    #[test]
    fn ignores_unknown_keys() {
        // Files may carry retired keys such as sample_rate and buffer_seconds.
        let cfg = Config::load_from_str(r#"{"sample_rate": 22050, "buffer_seconds": 0.05}"#);
        let text = serde_json::to_string(&cfg).unwrap();
        assert!(!text.contains("sample_rate"));
        assert!(!text.contains("buffer_seconds"));
        assert_eq!(cfg, Config::default());
    }
}
