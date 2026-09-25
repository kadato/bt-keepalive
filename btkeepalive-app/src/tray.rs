//! System tray: menu, tooltip, and icon state.
//!
//! Windows only, built on Tauri's tray support. The menu is flat:
//! Play/Pause, Settings, Check for updates, Open logs, Quit.
//! Clicks travel over a channel to the app loop in `main.rs`.

use std::sync::mpsc::Sender;
use std::sync::Arc;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Wry};

use crate::state::AppState;
use crate::tray_art::render_icon;

/// Menu item ids.
pub const ID_PLAY: &str = "play";
pub const ID_SETTINGS: &str = "settings";
pub const ID_UPDATE: &str = "update";
pub const ID_LOGS: &str = "logs";
pub const ID_QUIT: &str = "quit";

/// Live tray handles for label and icon refresh.
pub struct TrayState {
    tray: TrayIcon,
    play_item: MenuItem<Wry>,
    update_item: MenuItem<Wry>,
}

impl TrayState {
    /// Refresh label, tooltip, and icon after a state change.
    pub fn refresh(&self, state: &Arc<AppState>) {
        let snap = state.snapshot();
        let _ = self
            .play_item
            .set_text(if snap.playing { "Pause" } else { "Play" });
        let _ = self.update_item.set_text(if snap.update.is_some() {
            "Install update..."
        } else {
            "Check for updates"
        });
        let _ = self.tray.set_tooltip(Some(tooltip_text(state)));
        let pixels = render_icon(snap.playing);
        let _ = self.tray.set_icon(Some(Image::new(&pixels, 64, 64)));
    }
}

/// Build the tray. Menu clicks are sent as id strings to `clicks`.
/// Left-click shows the settings window.
pub fn build_tray(
    app: &AppHandle<Wry>,
    state: &Arc<AppState>,
    clicks: Sender<String>,
) -> Result<TrayState, String> {
    let snap = state.snapshot();
    let play_item = MenuItem::with_id(
        app,
        ID_PLAY,
        if snap.playing { "Pause" } else { "Play" },
        true,
        None::<&str>,
    )
    .map_err(|e| format!("tray menu failed: {e}"))?;
    let settings_item = MenuItem::with_id(app, ID_SETTINGS, "Settings", true, None::<&str>)
        .map_err(|e| format!("tray menu failed: {e}"))?;
    let update_item = MenuItem::with_id(app, ID_UPDATE, "Check for updates", true, None::<&str>)
        .map_err(|e| format!("tray menu failed: {e}"))?;
    let logs_item = MenuItem::with_id(app, ID_LOGS, "Open logs folder", true, None::<&str>)
        .map_err(|e| format!("tray menu failed: {e}"))?;
    let quit_item = MenuItem::with_id(app, ID_QUIT, "Quit", true, None::<&str>)
        .map_err(|e| format!("tray menu failed: {e}"))?;
    let sep_top =
        PredefinedMenuItem::separator(app).map_err(|e| format!("tray menu failed: {e}"))?;
    let sep_bottom =
        PredefinedMenuItem::separator(app).map_err(|e| format!("tray menu failed: {e}"))?;
    let menu = Menu::with_items(
        app,
        &[
            &play_item,
            &settings_item,
            &sep_top,
            &update_item,
            &logs_item,
            &sep_bottom,
            &quit_item,
        ],
    )
    .map_err(|e| format!("tray menu failed: {e}"))?;

    let pixels = render_icon(snap.playing);
    let icon = Image::new(&pixels, 64, 64);
    let left_clicks = clicks.clone();
    let tray = TrayIconBuilder::with_id("main")
        .icon(icon)
        .menu(&menu)
        .tooltip(tooltip_text(state))
        .on_menu_event(move |_app, event| {
            let _ = clicks.send(event.id.as_ref().to_string());
        })
        .on_tray_icon_event(move |_tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                ..
            } = event
            {
                let _ = left_clicks.send(ID_SETTINGS.to_string());
            }
        })
        .build(app)
        .map_err(|e| format!("tray build failed: {e}"))?;

    Ok(TrayState {
        tray,
        play_item,
        update_item,
    })
}

/// Tooltip text: what is playing, where, at what level.
fn tooltip_text(state: &Arc<AppState>) -> String {
    let snap = state.snapshot();
    let label = if snap.playing { "Playing" } else { "Paused" };
    let what = format!("{} {}%", snap.preset, trim_percent(snap.volume));
    match snap.device_name {
        Some(d) => format!("BT KeepAlive: {label} {what} on {d}"),
        None => format!("BT KeepAlive: {label} {what}"),
    }
}

fn trim_percent(volume: f64) -> String {
    let pct = volume * 100.0;
    format!("{pct:.4}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}
