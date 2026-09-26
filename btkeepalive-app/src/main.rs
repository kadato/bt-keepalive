#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! BT KeepAlive for Windows.
//!
//! Entry point: light CLI commands run anywhere, the full tray app
//! runs on Windows, and other platforms get a headless DSP smoke run.

#[cfg(target_os = "windows")]
mod audio_manager;
mod cli;
#[cfg(target_os = "windows")]
mod commands;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod state;
#[cfg(target_os = "windows")]
mod tray;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod tray_art;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod updater;

use btkeepalive_audio::model::{params_from_config, RenderModel};
use btkeepalive_audio::wav::write_wav_stereo_16;
use btkeepalive_core::config::{config_path, Config};
use clap::Parser;
#[cfg(target_os = "windows")]
use state::AppState;
#[cfg(target_os = "windows")]
use std::sync::Arc;
#[cfg(target_os = "windows")]
use tauri::Manager;

use cli::Cli;

fn main() {
    let cli = Cli::parse();

    if cli.config_path {
        println!("{}", config_path().display());
        return;
    }
    if cli.list_devices {
        list_devices();
        return;
    }
    if let Some(path) = cli.render_wav.as_deref() {
        render_wav(path);
        return;
    }
    if cli.dry_run {
        let path = std::env::temp_dir().join("btkeepalive-dryrun.wav");
        render_wav(&path);
        return;
    }

    #[cfg(target_os = "windows")]
    run_windows(cli);

    #[cfg(not(target_os = "windows"))]
    headless_smoke();
}

/// Render 5 seconds of the current preset to `path`.
fn render_wav(path: &std::path::Path) {
    let config = Config::load_from_path(&config_path());
    let mut model = RenderModel::build(params_from_config(&config, 48_000));
    let frames = 5 * 48_000;
    let mut out = vec![0.0f32; frames * 2];
    model.render_into(&mut out, config.volume as f32, true);
    match write_wav_stereo_16(path, &out, 48_000) {
        Ok(()) => println!("{}", path.display()),
        Err(e) => {
            eprintln!("render failed: {e}");
            std::process::exit(1);
        }
    }
}

/// List output devices. Enumerates on Windows, honest stub elsewhere.
fn list_devices() {
    #[cfg(target_os = "windows")]
    {
        use cpal::traits::{DeviceTrait, HostTrait};
        let host = cpal_host();
        for device in host
            .output_devices()
            .map(|it| it.collect::<Vec<_>>())
            .unwrap_or_default()
        {
            println!(
                "{}",
                device.name().unwrap_or_else(|_| "<unknown>".to_string())
            );
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        println!("device listing is Windows-only");
    }
}

#[cfg(target_os = "windows")]
fn cpal_host() -> cpal::Host {
    cpal::default_host()
}

/// Non-Windows full run: validate config plus DSP headlessly.
#[cfg(not(target_os = "windows"))]
fn headless_smoke() {
    let config = Config::load_from_path(&config_path());
    let mut model = RenderModel::build(params_from_config(&config, 48_000));
    let mut out = vec![0.0f32; 2 * 48_000];
    model.render_into(&mut out, config.volume as f32, true);
    let energy: f32 = out.iter().map(|v| v * v).sum::<f32>() / out.len() as f32;
    if energy <= 0.0 || !energy.is_finite() {
        eprintln!("smoke failed: silent render");
        std::process::exit(1);
    }
    println!("smoke OK: config and DSP valid (audio output is Windows-only)");
}

/// Full Windows app: tray, audio manager, watcher, updater, settings.
#[cfg(target_os = "windows")]
fn run_windows(cli: Cli) {
    use crate::audio_manager::AudioManager;
    use btkeepalive_platform::{device, instance, startup};

    if !instance::acquire() {
        return;
    }

    let exe = std::env::current_exe().unwrap_or_default();
    updater::cleanup_old_version(&exe);

    let mut config = Config::load_from_path(&config_path());
    if cli.no_autoplay {
        config.autoplay = false;
    }
    // Autoplay off means start paused, even if the saved file says playing.
    // Without this the tray would say playing while audio stays silent.
    if !config.autoplay {
        config.playing = false;
    }

    // Sync the Run key with settings on launch.
    if config.launch_at_startup {
        if startup::set_enabled(true, &exe).is_err() {
            config.launch_at_startup = false;
            log_line("startup registry sync failed on launch");
        }
    } else if startup::is_enabled(&exe) {
        let _ = startup::set_enabled(false, &exe);
    }

    let state = Arc::new(AppState::new(config.clone(), config_path()));
    let (manager, shared) =
        AudioManager::start(&config, |msg| log_line(&format!("audio: {msg}")), {
            let state = Arc::clone(&state);
            move |name, error| state.set_device(name, error)
        });
    let audio = Arc::new(std::sync::Mutex::new(Some(manager)));
    // Config save sweeper.
    {
        let state = Arc::clone(&state);
        std::thread::Builder::new()
            .name("config-sweeper".to_string())
            .spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_millis(250));
                state.sweep_save();
            })
            .expect("sweeper thread spawns");
    }

    // Device watcher: rebuild the stream on endpoint changes.
    {
        let state = Arc::clone(&state);
        let audio = Arc::clone(&audio);
        let (rx, _shutdown) = device::spawn_watcher();
        // `_shutdown` lives in this thread for the process lifetime.
        std::thread::Builder::new()
            .name("device-events".to_string())
            .spawn(move || {
                let _keep = _shutdown;
                while let Ok(event) = rx.recv() {
                    if event == device::DeviceEvent::DefaultOutputChanged {
                        state.set_device(
                            btkeepalive_audio::stream_cpal::OutputStream::device_name(),
                            None,
                        );
                        let snapshot = state.config_snapshot();
                        if let Ok(guard) = audio.lock() {
                            if let Some(manager) = guard.as_ref() {
                                manager.rebuild(&snapshot);
                            }
                        }
                    }
                }
            })
            .expect("device thread spawns");
    }

    // Update check: 5 s after start, then every 24 h.
    {
        let state = Arc::clone(&state);
        std::thread::Builder::new()
            .name("update-check-loop".to_string())
            .spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(5));
                loop {
                    if state.config_snapshot().check_for_updates {
                        let version = env!("CARGO_PKG_VERSION").to_string();
                        match updater::check_for_update(updater::DEFAULT_REPO, &version) {
                            Ok(Some(info)) => state.set_update(Some(crate::state::UiUpdate {
                                version: info.version,
                                notes: info.notes,
                            })),
                            Ok(None) => {}
                            Err(e) => log_line(&format!("update check failed: {e}")),
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_secs(86_400));
                }
            })
            .expect("update thread spawns");
    }

    run_tauri(state, audio, shared);
}

/// Tauri shell: settings window, tray, and commands.
#[cfg(target_os = "windows")]
fn run_tauri(
    state: Arc<AppState>,
    audio: Arc<std::sync::Mutex<Option<crate::audio_manager::AudioManager>>>,
    shared: Arc<btkeepalive_audio::stream_cpal::SharedAudio>,
) {
    use crate::commands::CommandContext;

    let ctx = CommandContext {
        state: Arc::clone(&state),
        audio,
        shared,
        tray: std::sync::Mutex::new(None),
    };

    let builder = tauri::Builder::default()
        .setup({
            let ctx = ctx.clone();
            move |app| {
                let handle = app.handle().clone();
                app.manage(ctx.clone());
                // Tray lives for the process; clicks arrive on the channel.
                let (clicks_tx, clicks_rx) = std::sync::mpsc::channel();
                let tray = Arc::new(
                    crate::tray::build_tray(app.handle(), &ctx.state, clicks_tx)
                        .map_err(std::io::Error::other)?,
                );
                app.manage(TrayHandle(Arc::clone(&tray)));
                // Commands refresh the tray through this handle.
                *ctx.tray.lock().expect("tray lock") = Some(Arc::clone(&tray));

                let menu_tray = Arc::clone(&tray);
                let menu_ctx = ctx.clone();
                std::thread::Builder::new()
                    .name("tray-events".to_string())
                    .spawn(move || {
                        while let Ok(id) = clicks_rx.recv() {
                            handle_menu_event(&handle, &menu_ctx, &menu_tray, id.as_str());
                        }
                    })
                    .expect("tray thread spawns");
                Ok(())
            }
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::set_preset,
            commands::set_volume,
            commands::set_pulse,
            commands::set_carrier,
            commands::set_playing,
            commands::set_startup,
            commands::set_autoplay,
            commands::set_check_updates,
            commands::reset_settings,
            commands::check_updates,
            commands::install_update,
            commands::open_logs,
        ]);

    let state_for_exit = Arc::clone(&state);
    builder
        .build(tauri::generate_context!())
        .expect("tauri app builds")
        .run(move |_handle, event| {
            if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
                state_for_exit.flush_save();
            }
        });
}

/// Tray handle kept alive by Tauri's managed state. The field is never
/// read; ownership alone keeps the tray icon alive for the process.
#[cfg(target_os = "windows")]
struct TrayHandle(#[allow(dead_code)] Arc<crate::tray::TrayState>);

/// One menu click: act, rebuild audio when needed, refresh the tray.
#[cfg(target_os = "windows")]
fn handle_menu_event(
    handle: &tauri::AppHandle<tauri::Wry>,
    ctx: &crate::commands::CommandContext,
    tray: &Arc<crate::tray::TrayState>,
    id: &str,
) {
    use crate::tray::{ID_LOGS, ID_PLAY, ID_QUIT, ID_SETTINGS, ID_UPDATE};

    let state = &ctx.state;
    if id == ID_PLAY {
        let playing = !state.is_playing();
        state.set_playing(playing);
        crate::commands::rebuild_audio_ctx(ctx);
    } else if id == ID_SETTINGS {
        if let Some(window) = handle.get_webview_window("main") {
            let _ = window.show();
            let _ = window.set_focus();
        }
    } else if id == ID_UPDATE {
        let state_clone = Arc::clone(state);
        let tray_clone = Arc::clone(tray);
        std::thread::Builder::new()
            .name("update-installer".to_string())
            .spawn(move || {
                run_update_flow(&state_clone);
                tray_clone.refresh(&state_clone);
            })
            .expect("update thread spawns");
    } else if id == ID_LOGS {
        let _ = commands::open_logs();
    } else if id == ID_QUIT {
        state.flush_save();
        std::process::exit(0);
    }
    // Refresh after every click so label, tooltip, and icon track state.
    tray.refresh(state);
}

/// Check, download, verify, and hot swap, then relaunch.
#[cfg(target_os = "windows")]
fn run_update_flow(state: &Arc<AppState>) {
    let version = env!("CARGO_PKG_VERSION").to_string();
    let info = match updater::check_for_update(updater::DEFAULT_REPO, &version) {
        Ok(Some(info)) => info,
        Ok(None) => {
            state.set_update(None);
            return;
        }
        Err(e) => {
            state.set_update_error(Some(e.clone()));
            log_line(&format!("update check failed: {e}"));
            return;
        }
    };
    state.set_update(Some(crate::state::UiUpdate {
        version: info.version.clone(),
        notes: info.notes.clone(),
    }));
    state.set_update_error(None);
    let exe = std::env::current_exe().unwrap_or_default();
    let cancelled = std::sync::atomic::AtomicBool::new(false);
    match updater::install_update(&info, &exe, |_, _| {}, &cancelled) {
        Ok(_) => {}
        Err(e) => {
            state.set_update_error(Some(e.clone()));
            log_line(&format!("update install failed: {e}"));
        }
    }
}

/// Append one line to `%APPDATA%\BTKeepAlive\app.log`.
#[cfg(target_os = "windows")]
fn log_line(message: &str) {
    use std::io::Write;
    let path = btkeepalive_core::config::config_dir().join("app.log");
    // Cap the log near 512 KB.
    if path.metadata().map(|m| m.len()).unwrap_or(0) > 512_000 {
        let _ = std::fs::remove_file(&path);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "{message}");
    }
}
