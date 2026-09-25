//! Tauri commands for the settings window.
//!
//! Windows only. Names match the frontend contract in
//! `settings-ui/main.js`. Every command returns `{ok, state?/error?}`.

use std::sync::{Arc, Mutex};
use tauri::State;

use crate::audio_manager::AudioManager;
use crate::state::AppState;
use crate::tray::TrayState;
use crate::updater;
use btkeepalive_audio::stream_cpal::SharedAudio;
use btkeepalive_core::config::config_dir;

/// Shared handles available to every command.
pub struct CommandContext {
    /// App state plus config and atomics.
    pub state: Arc<AppState>,
    /// Audio manager for table rebuilds. Locked only to send a command.
    pub audio: Arc<Mutex<Option<AudioManager>>>,
    /// Live realtime audio state. Volume writes go straight here so the
    /// output callback hears them without a table rebuild.
    pub shared: Arc<SharedAudio>,
    /// Live tray for label and icon refresh. Set once at startup.
    pub tray: Mutex<Option<Arc<TrayState>>>,
}

impl Clone for CommandContext {
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
            audio: Arc::clone(&self.audio),
            shared: Arc::clone(&self.shared),
            tray: Mutex::new(self.tray.lock().expect("tray lock").clone()),
        }
    }
}

fn ok(state: &Arc<AppState>) -> serde_json::Value {
    serde_json::json!({ "ok": true, "state": state.snapshot() })
}

fn fail(message: impl Into<String>) -> serde_json::Value {
    serde_json::json!({ "ok": false, "error": message.into() })
}

/// Respond with fresh state and refresh the tray to match.
fn respond(ctx: &CommandContext) -> serde_json::Value {
    if let Ok(tray) = ctx.tray.lock() {
        if let Some(tray) = tray.as_ref() {
            tray.refresh(&ctx.state);
        }
    }
    ok(&ctx.state)
}

#[tauri::command]
pub fn get_state(ctx: State<'_, CommandContext>) -> serde_json::Value {
    ok(&ctx.state)
}

#[tauri::command]
pub fn set_preset(ctx: State<'_, CommandContext>, preset: String) -> serde_json::Value {
    match ctx.state.set_preset(&preset) {
        Ok(rebuild) => {
            if rebuild {
                rebuild_audio(&ctx);
            }
            respond(&ctx)
        }
        Err(e) => fail(e),
    }
}

#[tauri::command]
pub fn set_volume(ctx: State<'_, CommandContext>, volume: f64) -> serde_json::Value {
    match ctx.state.set_volume(volume) {
        Ok(()) => {
            btkeepalive_audio::model::store_volume(&ctx.shared.volume_bits, volume as f32);
            respond(&ctx)
        }
        Err(e) => fail(e),
    }
}

#[tauri::command]
pub fn set_pulse(ctx: State<'_, CommandContext>, enabled: bool) -> serde_json::Value {
    ctx.state.set_pulse(enabled);
    rebuild_audio(&ctx);
    respond(&ctx)
}

#[tauri::command]
pub fn set_carrier(ctx: State<'_, CommandContext>, carrier_hz: u32) -> serde_json::Value {
    match ctx.state.set_carrier(carrier_hz) {
        Ok(changed) => {
            if changed {
                rebuild_audio(&ctx);
            }
            respond(&ctx)
        }
        Err(e) => fail(e),
    }
}

#[tauri::command]
pub fn set_playing(ctx: State<'_, CommandContext>, playing: bool) -> serde_json::Value {
    ctx.state.set_playing(playing);
    rebuild_audio(&ctx);
    respond(&ctx)
}

#[tauri::command]
pub fn set_startup(ctx: State<'_, CommandContext>, enabled: bool) -> serde_json::Value {
    let exe = std::env::current_exe().unwrap_or_default();
    match btkeepalive_platform::startup::set_enabled(enabled, &exe) {
        Ok(()) => {
            ctx.state.set_startup_flag(enabled);
            respond(&ctx)
        }
        Err(e) => fail(format!("Startup change failed: {e}")),
    }
}

#[tauri::command]
pub fn set_autoplay(ctx: State<'_, CommandContext>, enabled: bool) -> serde_json::Value {
    ctx.state.set_flags(Some(enabled), None);
    respond(&ctx)
}

#[tauri::command]
pub fn set_check_updates(ctx: State<'_, CommandContext>, enabled: bool) -> serde_json::Value {
    ctx.state.set_flags(None, Some(enabled));
    respond(&ctx)
}

#[tauri::command]
pub fn reset_settings(ctx: State<'_, CommandContext>) -> serde_json::Value {
    ctx.state.reset_settings();
    rebuild_audio(&ctx);
    respond(&ctx)
}

#[tauri::command]
pub fn check_updates(ctx: State<'_, CommandContext>) -> serde_json::Value {
    let version = env!("CARGO_PKG_VERSION").to_string();
    match updater::check_for_update(updater::DEFAULT_REPO, &version) {
        Ok(Some(info)) => {
            ctx.state.set_update(Some(info.version.clone()));
            respond(&ctx)
        }
        Ok(None) => {
            ctx.state.set_update(None);
            respond(&ctx)
        }
        Err(e) => fail(e),
    }
}

#[tauri::command]
pub fn open_logs() -> serde_json::Value {
    let dir = config_dir();
    match std::process::Command::new("explorer").arg(&dir).spawn() {
        Ok(_) => serde_json::json!({ "ok": true }),
        Err(e) => fail(format!("Cannot open {dir:?}: {e}")),
    }
}

fn rebuild_audio(ctx: &State<'_, CommandContext>) {
    rebuild_audio_ctx(ctx);
}

/// Rebuild audio tables from current settings. Shared with the tray
/// menu thread, which owns a cloned context instead of Tauri state.
pub fn rebuild_audio_ctx(ctx: &CommandContext) {
    let snapshot = ctx.state.config_snapshot();
    if let Ok(guard) = ctx.audio.lock() {
        if let Some(manager) = guard.as_ref() {
            manager.rebuild(&snapshot);
        }
    }
}
