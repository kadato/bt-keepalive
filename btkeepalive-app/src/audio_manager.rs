//! Windows audio manager: owns the output stream.
//!
//! Continuous mode holds one stream open. Pulse mode closes the stream
//! between bursts with wall-clock deadlines, which removes ~54 of every
//! 55 seconds of wakeups. The manager reopens the stream after device
//! switches, headset reconnects, and late headset arrivals.

use btkeepalive_audio::model::{params_from_config, store_volume, RenderModel};
use btkeepalive_audio::scheduler::{OpenRetry, PulseRuntime};
use btkeepalive_audio::stream_cpal::{OutputStream, SharedAudio};
use btkeepalive_core::config::{Config, KeepaliveMode};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

/// How often the manager rechecks when healthy and idle.
const IDLE_TICK: Duration = Duration::from_millis(250);

/// Delay before retrying a failed open. A missing headset at boot or a
/// disconnected headset retries here, so reconnects play within seconds
/// without spamming the log every 250 ms.
const RETRY_DELAY: Duration = Duration::from_secs(2);

/// Cap for any single sleep. Pulse waits of ~54 s wake every 5 s so a
/// Play or Pause click never waits long.
const MAX_SLEEP: Duration = Duration::from_secs(5);

/// Commands from the UI thread to the manager thread.
#[derive(Debug)]
pub enum ManagerCmd {
    /// Rebuild tables from settings. Keeps a healthy stream open.
    Rebuild(Config),
    /// Default device or list changed. Drops the stream so the next tick
    /// reopens on the current default, even if the old stream still looks open.
    DeviceChanged(Config),
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

    /// Tell the manager the device list or default changed. Reopens the
    /// stream on the current default.
    pub fn device_changed(&self, config: &Config) {
        let _ = self.tx.send(ManagerCmd::DeviceChanged(config.clone()));
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
    pulse: Option<PulseRuntime>,
    stream_failed: Arc<AtomicBool>,
    stream_error: Arc<Mutex<Option<String>>>,
    retry: OpenRetry,
    log_error: Arc<dyn Fn(String) + Send + Sync>,
    on_device_text: Arc<dyn Fn(Option<String>, Option<String>) + Send + Sync>,
}

impl Worker {
    /// Apply one UI command. Returns false when asked to stop.
    fn apply(&mut self, cmd: ManagerCmd) -> bool {
        match cmd {
            ManagerCmd::Rebuild(config) => {
                let was_playing = self.config.playing;
                self.config = config;
                rebuild_shared(&self.config, &self.shared);
                if self.config.playing {
                    self.pulse = pulse_runtime(&self.config);
                    // Settings changed: retry now instead of waiting out an
                    // old backoff from a missing headset.
                    self.retry.clear_backoff();
                    if self.stream.is_none() {
                        let now = Instant::now();
                        self.stream = self.open_stream_or_wait(now);
                        if self.stream.is_some() {
                            self.note_pulse_opened(now);
                        }
                    } else if !was_playing {
                        // Paused to playing with a stale handle: reopen.
                        let now = Instant::now();
                        self.stream = self.open_stream_or_wait(now);
                        if self.stream.is_some() {
                            self.note_pulse_opened(now);
                        }
                    }
                } else {
                    self.stream = None;
                    self.pulse = None;
                    self.retry.on_success();
                }
                true
            }
            ManagerCmd::DeviceChanged(config) => {
                self.config = config;
                rebuild_shared(&self.config, &self.shared);
                if self.config.playing {
                    // Follow the new default even if the old stream looks fine.
                    // A disconnected headset leaves a stale handle behind.
                    self.stream = None;
                    self.retry.clear_backoff();
                    self.pulse = pulse_runtime(&self.config);
                    let now = Instant::now();
                    self.stream = self.open_stream_or_wait(now);
                    if self.stream.is_some() {
                        self.note_pulse_opened(now);
                    }
                } else {
                    self.stream = None;
                    self.pulse = None;
                }
                true
            }
            ManagerCmd::Stop => false,
        }
    }

    /// One scheduler tick. Returns how long to wait for the next event.
    fn tick(&mut self) -> Duration {
        let now = Instant::now();
        // A cpal error callback fired since the last tick. The handle is
        // stale after a headset disconnect, so drop it and reopen.
        if self.stream_failed.swap(false, Ordering::SeqCst) {
            let msg = self
                .stream_error
                .lock()
                .ok()
                .and_then(|mut g| g.take())
                .unwrap_or_else(|| "device lost".to_string());
            (self.log_error)(format!("audio stream error: {msg}, reopening"));
            self.stream = None;
            self.retry.clear_backoff();
            if let Some(pulse) = &mut self.pulse {
                pulse.on_open_failed(now, RETRY_DELAY);
            }
        }
        if !self.config.playing {
            if self.stream.is_some() {
                self.stream = None;
            }
            return IDLE_TICK;
        }
        if self.config.keepalive_mode == KeepaliveMode::Pulse {
            return self.tick_pulse(now);
        }
        // Continuous: hold one stream open, retry with backoff when the
        // headset is missing at boot or disconnected later.
        if self.stream.is_some() {
            return IDLE_TICK;
        }
        if !self.retry.allowed(now) {
            return retry_wait(&self.retry, now);
        }
        self.stream = self.open_stream();
        IDLE_TICK
    }

    fn tick_pulse(&mut self, now: Instant) -> Duration {
        if self.pulse.is_none() {
            self.pulse = pulse_runtime(&self.config);
            return IDLE_TICK;
        }
        if self.stream.is_some() {
            let active = self
                .pulse
                .as_ref()
                .map(|p| p.burst_active(now))
                .unwrap_or(false);
            if active {
                let wait = self
                    .pulse
                    .as_ref()
                    .map(|p| p.sleep_until_next(now, true))
                    .unwrap_or(IDLE_TICK);
                return wait.min(MAX_SLEEP);
            }
            // Burst finished: close until the next one.
            self.stream = None;
            if let Some(p) = self.pulse.as_mut() {
                p.on_burst_ended();
            }
            let wait = self
                .pulse
                .as_ref()
                .map(|p| p.sleep_until_next(now, false))
                .unwrap_or(IDLE_TICK);
            return wait.min(MAX_SLEEP);
        }
        // Stream closed between bursts.
        let due = self
            .pulse
            .as_ref()
            .map(|p| p.burst_due(now))
            .unwrap_or(false);
        if !due {
            let wait = self
                .pulse
                .as_ref()
                .map(|p| p.sleep_until_next(now, false))
                .unwrap_or(IDLE_TICK);
            return wait.min(MAX_SLEEP);
        }
        let opened = self.open_stream_or_wait(now);
        if opened.is_some() {
            self.stream = opened;
            if let Some(p) = self.pulse.as_mut() {
                p.on_opened(now);
            }
            reset_pulse_positions(&self.shared);
            self.retry.on_success();
            let wait = self
                .pulse
                .as_ref()
                .map(|p| p.sleep_until_next(now, true))
                .unwrap_or(IDLE_TICK);
            return wait.min(MAX_SLEEP).max(IDLE_TICK);
        }
        let wait = self
            .pulse
            .as_ref()
            .map(|p| p.sleep_until_next(now, false))
            .unwrap_or(IDLE_TICK);
        wait.min(MAX_SLEEP)
    }

    /// Attempt an open for pulse bursts. Failures retry in seconds with
    /// duplicate messages suppressed, so a late headset plays quickly.
    fn open_stream_or_wait(&mut self, now: Instant) -> Option<OutputStream> {
        if !self.retry.allowed(now) {
            return None;
        }
        match self.try_open() {
            Ok(stream) => {
                self.retry.on_success();
                (self.on_device_text)(OutputStream::device_name(), None);
                Some(stream)
            }
            Err(e) => {
                let msg = e.to_string();
                if self.retry.on_failure(now, &msg, RETRY_DELAY) {
                    (self.log_error)(format!("audio open failed: {e}"));
                }
                (self.on_device_text)(None, Some(msg));
                if let Some(pulse) = &mut self.pulse {
                    pulse.on_open_failed(now, RETRY_DELAY);
                }
                None
            }
        }
    }

    fn open_stream(&mut self) -> Option<OutputStream> {
        let now = Instant::now();
        // Pulse timing already throttles; continuous checks backoff here.
        if self.config.keepalive_mode != KeepaliveMode::Pulse && !self.retry.allowed(now) {
            return None;
        }
        match self.try_open() {
            Ok(stream) => {
                self.retry.on_success();
                (self.on_device_text)(OutputStream::device_name(), None);
                Some(stream)
            }
            Err(e) => {
                let msg = e.to_string();
                if self.retry.on_failure(now, &msg, RETRY_DELAY) {
                    (self.log_error)(format!("audio open failed: {e}"));
                }
                (self.on_device_text)(None, Some(msg));
                None
            }
        }
    }

    fn try_open(&self) -> Result<OutputStream, btkeepalive_audio::stream_cpal::StreamError> {
        let failed = Arc::clone(&self.stream_failed);
        let error_text = Arc::clone(&self.stream_error);
        OutputStream::open(Arc::clone(&self.shared), move |s: String| {
            *error_text.lock().unwrap_or_else(|e| e.into_inner()) = Some(s);
            failed.store(true, Ordering::SeqCst);
        })
    }

    /// Record a successful pulse open: schedule the burst end and reset
    /// positions so the burst starts with sound. No-op in continuous mode.
    fn note_pulse_opened(&mut self, now: Instant) {
        if self.config.keepalive_mode != KeepaliveMode::Pulse {
            return;
        }
        if let Some(pulse) = self.pulse.as_mut() {
            pulse.on_opened(now);
        }
        reset_pulse_positions(&self.shared);
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
        pulse: pulse_runtime(&config),
        config,
        shared,
        stream: None,
        stream_failed: Arc::new(AtomicBool::new(false)),
        stream_error: Arc::new(Mutex::new(None)),
        retry: OpenRetry::new(),
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

/// Reset pulse playback so each burst starts with sound, not silence.
/// Frame and model positions freeze while the stream is closed.
fn reset_pulse_positions(shared: &Arc<SharedAudio>) {
    if let Ok(mut model) = shared.model.lock() {
        model.reset_positions();
    }
    shared.frames.store(0, Ordering::Relaxed);
}

/// Sample rate hint for table builds. cpal reports the real rate after
/// open; 48 kHz matches the preferred Windows mix.
fn current_rate_hint() -> u32 {
    48_000
}

fn pulse_runtime(config: &Config) -> Option<PulseRuntime> {
    if config.keepalive_mode != KeepaliveMode::Pulse || !config.playing {
        return None;
    }
    Some(PulseRuntime::new(
        config.pulse_interval_sec,
        config.pulse_duration_sec,
    ))
}

fn retry_wait(retry: &OpenRetry, now: Instant) -> Duration {
    retry.wait_remaining(now).min(MAX_SLEEP)
}
