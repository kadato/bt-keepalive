//! Windows audio manager: owns the output stream.
//!
//! Continuous mode holds one stream open. Pulse mode closes the stream
//! between bursts using `PulseScheduler`, which removes ~54 of every
//! 55 seconds of wakeups.

use btkeepalive_audio::model::{params_from_config, store_volume, RenderModel};
use btkeepalive_audio::scheduler::{PulseScheduler, SchedulerAction};
use btkeepalive_audio::stream_cpal::{OutputStream, SharedAudio};
use btkeepalive_core::config::{Config, KeepaliveMode};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

/// How often the manager rechecks the schedule when idle.
const IDLE_TICK: Duration = Duration::from_millis(250);

/// Commands from the UI thread to the manager thread.
#[derive(Debug)]
pub enum ManagerCmd {
    /// Rebuild tables from the latest config.
    Rebuild(Config),
    /// Shut the manager down.
    Stop,
}

/// Handle to the running manager thread.
pub struct AudioManager {
    tx: mpsc::Sender<ManagerCmd>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl AudioManager {
    /// Start the manager. Builds tables once, opens the stream when
    /// `config` wants playback, and returns the shared realtime state.
    pub fn start(
        config: &Config,
        log_error: impl Fn(String) + Send + Sync + 'static,
        on_device_text: impl Fn(Option<String>, Option<String>) + Send + Sync + 'static,
    ) -> (Self, Arc<SharedAudio>) {
        let log_error = Arc::new(log_error);
        let on_device_text = Arc::new(on_device_text);
        let shared = Arc::new(SharedAudio {
            model: Mutex::new(RenderModel::build(params_from_config(config, 48_000))),
            volume_bits: AtomicU32::new((config.volume as f32).to_bits()),
            playing: AtomicU32::new(u32::from(config.playing && config.autoplay)),
            frames: AtomicU64::new(0),
        });
        // Friendly device name for the status card.
        on_device_text(OutputStream::device_name(), None);
        let (tx, rx) = mpsc::channel();
        let worker_shared = Arc::clone(&shared);
        let worker_config = config.clone();
        let thread = std::thread::Builder::new()
            .name("audio-manager".to_string())
            .spawn(move || {
                manager_loop(worker_config, worker_shared, rx, log_error, on_device_text);
            })
            .expect("audio manager thread spawns");
        (
            Self {
                tx,
                thread: Some(thread),
            },
            shared,
        )
    }

    /// Ask the manager to rebuild tables from new settings.
    pub fn rebuild(&self, config: &Config) {
        let _ = self.tx.send(ManagerCmd::Rebuild(config.clone()));
    }
}

impl Drop for AudioManager {
    fn drop(&mut self) {
        let _ = self.tx.send(ManagerCmd::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Manager state owned by the worker thread.
struct Worker {
    config: Config,
    shared: Arc<SharedAudio>,
    stream: Option<OutputStream>,
    scheduler: Option<PulseScheduler>,
    log_error: Arc<dyn Fn(String) + Send + Sync>,
    on_device_text: Arc<dyn Fn(Option<String>, Option<String>) + Send + Sync>,
}

impl Worker {
    /// Apply one UI command. Returns false when asked to stop.
    fn apply(&mut self, cmd: ManagerCmd) -> bool {
        match cmd {
            ManagerCmd::Rebuild(config) => {
                self.config = config;
                rebuild_shared(&self.config, &self.shared);
                if self.config.playing {
                    if self.stream.is_none() {
                        self.stream = self.open_stream();
                    }
                    self.scheduler = pulse_scheduler(&self.config);
                } else {
                    self.stream = None;
                    self.scheduler = None;
                }
                true
            }
            ManagerCmd::Stop => false,
        }
    }

    /// One scheduler tick. Returns how long to wait for the next event.
    fn tick(&mut self) -> Duration {
        if self.config.keepalive_mode == KeepaliveMode::Pulse && self.config.playing {
            let pos = self.shared.frames.load(Ordering::Relaxed);
            if let Some(sched) = &self.scheduler {
                // Decide from the current open state.
                let action = if self.stream.is_some() {
                    sched.on_open(pos)
                } else {
                    sched.on_closed(pos)
                };
                match action {
                    SchedulerAction::KeepOpen => {
                        if self.stream.is_none() {
                            self.stream = self.open_stream();
                        }
                    }
                    SchedulerAction::PlayBurst => {
                        self.stream = self.open_stream();
                    }
                    SchedulerAction::CloseFor(d) => {
                        self.stream = None;
                        return d.min(Duration::from_secs(5));
                    }
                }
                return IDLE_TICK;
            }
        }
        if self.config.playing && self.stream.is_none() {
            self.stream = self.open_stream();
        } else if !self.config.playing {
            self.stream = None;
        }
        IDLE_TICK
    }

    fn open_stream(&self) -> Option<OutputStream> {
        let log_cb = Arc::clone(&self.log_error);
        match OutputStream::open(Arc::clone(&self.shared), move |s: String| log_cb(s)) {
            Ok(stream) => {
                (self.on_device_text)(OutputStream::device_name(), None);
                Some(stream)
            }
            Err(e) => {
                (self.log_error)(format!("audio open failed: {e}"));
                (self.on_device_text)(None, Some(e.to_string()));
                None
            }
        }
    }
}

fn manager_loop(
    config: Config,
    shared: Arc<SharedAudio>,
    rx: mpsc::Receiver<ManagerCmd>,
    log_error: Arc<dyn Fn(String) + Send + Sync>,
    on_device_text: Arc<dyn Fn(Option<String>, Option<String>) + Send + Sync>,
) {
    let mut worker = Worker {
        config,
        shared,
        stream: None,
        scheduler: None,
        log_error,
        on_device_text,
    };
    // Seed through the normal path so tables and flags always agree.
    let seed = worker.config.clone();
    worker.apply(ManagerCmd::Rebuild(seed));

    loop {
        let wait = worker.tick();
        match rx.recv_timeout(wait) {
            Ok(cmd) => {
                if !worker.apply(cmd) {
                    return;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn rebuild_shared(config: &Config, shared: &Arc<SharedAudio>) {
    let params = params_from_config(config, current_rate_hint());
    if let Ok(mut model) = shared.model.lock() {
        *model = RenderModel::build(params);
    }
    store_volume(&shared.volume_bits, config.volume as f32);
    shared
        .playing
        .store(u32::from(config.playing), Ordering::Relaxed);
}

/// Sample rate hint for table builds. cpal reports the real rate after
/// open; 48 kHz matches the preferred Windows mix.
fn current_rate_hint() -> u32 {
    48_000
}

fn pulse_scheduler(config: &Config) -> Option<PulseScheduler> {
    if config.keepalive_mode != KeepaliveMode::Pulse {
        return None;
    }
    let sr = 48_000u64;
    let cycle = (sr as f64 * config.pulse_interval_sec).round() as u64;
    let len = (sr as f64 * config.pulse_duration_sec).round() as u64;
    Some(PulseScheduler::new(48_000, cycle.max(1), len.max(1)))
}
