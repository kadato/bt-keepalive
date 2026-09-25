//! Shared application state: config plus realtime audio knobs.
//!
//! The UI thread mutates through here. The audio callback only reads
//! the atomics, never these locks. Config saves are debounced: each
//! mutation marks dirty, and a sweeper thread persists at most twice
//! per second.

use btkeepalive_core::config::{Config, KeepaliveMode, Preset};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};

use btkeepalive_audio::model::store_volume;

/// How long a mutation waits before it reaches disk.
const SAVE_DEBOUNCE: Duration = Duration::from_millis(500);

/// Update banner payload for the settings window.
#[derive(Debug, Clone, Serialize)]
pub struct UiUpdate {
    /// Release tag, for example `v2.1.0`.
    pub version: String,
}

/// Full state snapshot matching the settings-ui contract.
#[derive(Debug, Clone, Serialize)]
pub struct UiState {
    pub preset: String,
    pub volume: f64,
    pub carrier_hz: u32,
    pub keepalive_mode: String,
    pub pulse_interval_sec: f64,
    pub playing: bool,
    pub autoplay: bool,
    pub launch_at_startup: bool,
    pub check_for_updates: bool,
    pub device_name: Option<String>,
    pub status: String,
    pub version: String,
    pub update: Option<UiUpdate>,
}

/// Shared state for CLI, tray, and Tauri commands.
pub struct AppState {
    config: Mutex<Config>,
    config_path: PathBuf,
    dirty_at: Mutex<Option<Instant>>,
    last_save: Mutex<Instant>,
    /// `f32` volume bits for the audio thread.
    pub volume_bits: AtomicU32,
    /// 1 while playing, 0 while paused.
    pub playing_flag: AtomicU32,
    device_name: RwLock<Option<String>>,
    device_error: RwLock<Option<String>>,
    update: Mutex<Option<UiUpdate>>,
}

impl AppState {
    /// Wrap a loaded config. Reads the initial atomics from it.
    #[must_use]
    pub fn new(config: Config, config_path: PathBuf) -> Self {
        let playing = u32::from(config.playing);
        Self {
            volume_bits: AtomicU32::new((config.volume as f32).to_bits()),
            playing_flag: AtomicU32::new(playing),
            config: Mutex::new(config),
            config_path,
            dirty_at: Mutex::new(None),
            last_save: Mutex::new(Instant::now()),
            device_name: RwLock::new(None),
            device_error: RwLock::new(None),
            update: Mutex::new(None),
        }
        .into_ready()
    }

    fn into_ready(self) -> Self {
        self
    }

    /// Current config copy, for audio rebuilds.
    #[must_use]
    pub fn config_snapshot(&self) -> Config {
        self.config.lock().expect("config lock").clone()
    }

    /// True while playing.
    #[must_use]
    pub fn is_playing(&self) -> bool {
        self.playing_flag.load(Ordering::Relaxed) == 1
    }

    /// Snapshot for the settings window and tray tooltip.
    #[must_use]
    pub fn snapshot(&self) -> UiState {
        let cfg = self.config_snapshot();
        let error = self.device_error.read().expect("error lock").clone();
        let status = if error.is_some() {
            "error".to_string()
        } else if cfg.playing {
            "playing".to_string()
        } else {
            "paused".to_string()
        };
        UiState {
            preset: cfg.preset.as_str().to_string(),
            volume: cfg.volume,
            carrier_hz: cfg.carrier_hz,
            keepalive_mode: cfg.keepalive_mode.as_str().to_string(),
            pulse_interval_sec: cfg.pulse_interval_sec,
            playing: cfg.playing,
            autoplay: cfg.autoplay,
            launch_at_startup: cfg.launch_at_startup,
            check_for_updates: cfg.check_for_updates,
            device_name: self.device_name.read().expect("device lock").clone(),
            status,
            version: env!("CARGO_PKG_VERSION").to_string(),
            update: self.update.lock().expect("update lock").clone(),
        }
    }

    /// Set playback volume. Validates, updates the live atomic at once,
    /// and debounces the disk write.
    pub fn set_volume(&self, volume: f64) -> Result<(), String> {
        if !volume.is_finite() || volume <= 0.0 || volume > 1.0 {
            return Err("Enter a percent between 0.01 and 100.".to_string());
        }
        self.config.lock().expect("config lock").volume = volume;
        store_volume(&self.volume_bits, volume as f32);
        self.mark_dirty();
        Ok(())
    }

    /// Switch preset. Returns true when audio tables need a rebuild.
    pub fn set_preset(&self, preset: &str) -> Result<bool, String> {
        let parsed = Preset::parse(preset).ok_or_else(|| format!("Unknown preset: {preset}"))?;
        let mut cfg = self.config.lock().expect("config lock");
        let rebuild = cfg.preset != parsed || cfg.keepalive_mode != KeepaliveMode::Continuous;
        cfg.preset = parsed;
        cfg.keepalive_mode = KeepaliveMode::Continuous;
        drop(cfg);
        self.mark_dirty();
        Ok(rebuild)
    }

    /// Toggle pulse mode. Returns true when audio needs attention.
    pub fn set_pulse(&self, enabled: bool) -> bool {
        let mut cfg = self.config.lock().expect("config lock");
        let mode = if enabled {
            KeepaliveMode::Pulse
        } else {
            KeepaliveMode::Continuous
        };
        let changed = cfg.keepalive_mode != mode;
        cfg.keepalive_mode = mode;
        drop(cfg);
        self.mark_dirty();
        changed
    }

    /// Set the binaural carrier. Only one of the tray options is valid.
    pub fn set_carrier(&self, carrier_hz: u32) -> Result<bool, String> {
        if !btkeepalive_core::config::CARRIER_OPTIONS.contains(&carrier_hz) {
            return Err("Carrier must be one of 100, 150, 200, 250, 300 Hz".to_string());
        }
        let mut cfg = self.config.lock().expect("config lock");
        let changed = cfg.carrier_hz != carrier_hz;
        cfg.carrier_hz = carrier_hz;
        drop(cfg);
        self.mark_dirty();
        Ok(changed)
    }

    /// Pause or resume. Updates the live flag at once.
    pub fn set_playing(&self, playing: bool) {
        self.config.lock().expect("config lock").playing = playing;
        self.playing_flag
            .store(u32::from(playing), Ordering::Relaxed);
        self.mark_dirty();
    }

    /// Record the current output device name for the status card.
    pub fn set_device(&self, name: Option<String>, error: Option<String>) {
        *self.device_name.write().expect("device lock") = name;
        *self.device_error.write().expect("error lock") = error;
    }

    /// Record available update info for the banner.
    pub fn set_update(&self, version: Option<String>) {
        *self.update.lock().expect("update lock") = version.map(|version| UiUpdate { version });
    }

    /// Replace all settings with defaults, keeping the update banner.
    pub fn reset_settings(&self) {
        let update = self.update.lock().expect("update lock").clone();
        let mut cfg = self.config.lock().expect("config lock");
        *cfg = Config::default();
        store_volume(&self.volume_bits, cfg.volume as f32);
        self.playing_flag.store(1, Ordering::Relaxed);
        drop(cfg);
        *self.update.lock().expect("update lock") = update;
        self.mark_dirty();
    }

    /// Apply a startup toggle result already written to the registry.
    pub fn set_startup_flag(&self, enabled: bool) {
        self.config.lock().expect("config lock").launch_at_startup = enabled;
        self.mark_dirty();
    }

    /// Apply autoplay and update-check toggles from settings.
    pub fn set_flags(&self, autoplay: Option<bool>, check_for_updates: Option<bool>) {
        let mut cfg = self.config.lock().expect("config lock");
        if let Some(a) = autoplay {
            cfg.autoplay = a;
        }
        if let Some(c) = check_for_updates {
            cfg.check_for_updates = c;
        }
        drop(cfg);
        self.mark_dirty();
    }

    fn mark_dirty(&self) {
        *self.dirty_at.lock().expect("dirty lock") = Some(Instant::now());
    }

    /// Persist when the debounce window has passed. Called by the
    /// sweeper thread and at shutdown.
    pub fn sweep_save(&self) {
        let now = Instant::now();
        let should = {
            let dirty = self.dirty_at.lock().expect("dirty lock");
            // Save once the pending write has aged past the debounce window.
            matches!(*dirty, Some(t) if now.duration_since(t) >= SAVE_DEBOUNCE)
        };
        if should {
            let snapshot = self.config_snapshot();
            if snapshot.save_to_path(&self.config_path).is_ok() {
                *self.last_save.lock().expect("save lock") = Instant::now();
                *self.dirty_at.lock().expect("dirty lock") = None;
            }
        }
    }

    /// Persist right now, for shutdown.
    pub fn flush_save(&self) {
        let snapshot = self.config_snapshot();
        if snapshot.save_to_path(&self.config_path).is_ok() {
            *self.last_save.lock().expect("save lock") = Instant::now();
            *self.dirty_at.lock().expect("dirty lock") = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use btkeepalive_audio::model::load_volume;

    fn state() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let s = AppState::new(Config::default(), path);
        (dir, s)
    }

    #[test]
    fn snapshot_matches_frontend_contract() {
        let (_d, s) = state();
        let snap = s.snapshot();
        let json = serde_json::to_value(&snap).unwrap();
        for key in [
            "preset",
            "volume",
            "carrier_hz",
            "keepalive_mode",
            "pulse_interval_sec",
            "playing",
            "autoplay",
            "launch_at_startup",
            "check_for_updates",
            "device_name",
            "status",
            "version",
            "update",
        ] {
            assert!(json.get(key).is_some(), "missing {key}");
        }
        assert_eq!(json["preset"], "brown");
        assert_eq!(json["status"], "playing");
    }

    #[test]
    fn volume_updates_live_atomic_and_rejects_garbage() {
        let (_d, s) = state();
        s.set_volume(0.05).unwrap();
        assert!((load_volume(&s.volume_bits) - 0.05).abs() < 1e-6);
        assert!(s.set_volume(0.0).is_err());
        assert!(s.set_volume(2.0).is_err());
    }

    #[test]
    fn preset_switch_requests_rebuild_once() {
        let (_d, s) = state();
        assert!(s.set_preset("pink").unwrap());
        assert!(!s.set_preset("pink").unwrap());
        assert!(s.set_preset("nope").is_err());
    }

    #[test]
    fn reset_restores_defaults() {
        let (_d, s) = state();
        s.set_volume(0.09).unwrap();
        s.set_preset("white").unwrap();
        s.reset_settings();
        let snap = s.snapshot();
        assert_eq!(snap.preset, "brown");
        assert!((snap.volume - 0.02).abs() < 1e-12);
    }

    #[test]
    fn debounce_writes_after_window() {
        let (_d, s) = state();
        s.set_volume(0.07).unwrap();
        s.sweep_save();
        // Too early: nothing on disk yet.
        let fresh = Config::load_from_path(&s.config_path);
        assert!((fresh.volume - 0.02).abs() < 1e-12);
        std::thread::sleep(SAVE_DEBOUNCE + Duration::from_millis(50));
        s.sweep_save();
        let saved = Config::load_from_path(&s.config_path);
        assert!((saved.volume - 0.07).abs() < 1e-12);
    }
}
