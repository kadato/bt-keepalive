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
use tauri::{AppHandle, Manager, PhysicalPosition, Position, Wry};

use crate::state::AppState;
use crate::tray_art::render_icon;

/// Menu item ids.
pub const ID_PLAY: &str = "play";
pub const ID_SETTINGS: &str = "settings";
pub const ID_UPDATE: &str = "update";
pub const ID_LOGS: &str = "logs";
pub const ID_QUIT: &str = "quit";

/// One tray interaction: menu id plus click point when known.
/// Left-click carries the tray position so the settings window can
/// open near the cursor. Menu clicks carry `None`.
pub struct TrayClick {
    pub id: String,
    pub position: Option<PhysicalPosition<f64>>,
}

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

/// Build the tray. Menu clicks are sent as [`TrayClick`] to `clicks`.
/// Left-click shows the settings window.
pub fn build_tray(
    app: &AppHandle<Wry>,
    state: &Arc<AppState>,
    clicks: Sender<TrayClick>,
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
            let _ = clicks.send(TrayClick {
                id: event.id.as_ref().to_string(),
                position: None,
            });
        })
        .on_tray_icon_event(move |_tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                position,
                ..
            } = event
            {
                let _ = left_clicks.send(TrayClick {
                    id: ID_SETTINGS.to_string(),
                    position: Some(position),
                });
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

/// Show the settings window docked to the right edge of the screen,
/// just above the taskbar. The tray lives bottom-right, so opening
/// here keeps the mouse close to the window.
pub fn show_main_window(app: &AppHandle<Wry>, anchor: Option<PhysicalPosition<f64>>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    // Already open: leave it where the user put it, just focus.
    if window.is_visible().unwrap_or(false) {
        let _ = window.unminimize();
        let _ = window.set_focus();
        return;
    }
    dock_to_right_edge(&window, anchor);
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

/// Move a hidden window to the work area bottom-right of the monitor
/// holding `anchor`, else the cursor, else the current monitor.
fn dock_to_right_edge(window: &tauri::WebviewWindow<Wry>, anchor: Option<PhysicalPosition<f64>>) {
    let cursor = window.cursor_position().ok();
    let point = anchor.or(cursor);
    let monitor = point
        .and_then(|p| window.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| window.current_monitor().ok().flatten())
        .or_else(|| window.primary_monitor().ok().flatten())
        .or_else(|| {
            window
                .available_monitors()
                .ok()
                .and_then(|all| all.into_iter().next())
        });
    let Some(monitor) = monitor else {
        return;
    };
    let work = monitor.work_area();
    let scale = monitor.scale_factor();
    let (win_w, win_h) = match window.outer_size() {
        Ok(size) if size.width > 0 && size.height > 0 => (
            i32::try_from(size.width).unwrap_or(0),
            i32::try_from(size.height).unwrap_or(0),
        ),
        _ => ((560.0 * scale) as i32, (760.0 * scale) as i32),
    };
    if win_w <= 0 || win_h <= 0 {
        return;
    }
    let margin = (12.0 * scale) as i32;
    let max_w = work.size.width as i32;
    let max_h = work.size.height as i32;
    let clamp_w = win_w.min(max_w);
    let clamp_h = win_h.min(max_h);
    let mut x = work.position.x + max_w - win_w - margin;
    let mut y = work.position.y + max_h - win_h - margin;
    x = x
        .max(work.position.x)
        .min(work.position.x + max_w - clamp_w);
    y = y
        .max(work.position.y)
        .min(work.position.y + max_h - clamp_h);
    let _ = window.set_position(Position::Physical(PhysicalPosition::new(x, y)));
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
