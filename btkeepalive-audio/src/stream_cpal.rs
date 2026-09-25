//! Windows output stream via `cpal` (WASAPI shared mode).
//!
//! Only compiled on Windows. Picks an `f32` stereo config, preferring
//! 48 kHz, then drives a shared `RenderModel` from the realtime
//! callback. The callback uses `try_lock` and falls back to silence so
//! a busy UI thread can never wedge audio.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::fmt;
use std::sync::{atomic::AtomicU32, atomic::AtomicU64, atomic::Ordering, Arc, Mutex};

use crate::model::{load_volume, RenderModel};

/// Errors opening or running the output stream.
#[derive(Debug)]
pub enum StreamError {
    /// No default output device, or device query failed.
    NoDevice(String),
    /// No usable `f32` stereo mix format.
    NoFormat(String),
    /// Stream build or play failed.
    Stream(String),
}

impl fmt::Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDevice(e) => write!(f, "no output device: {e}"),
            Self::NoFormat(e) => write!(f, "no f32 stereo format: {e}"),
            Self::Stream(e) => write!(f, "stream failed: {e}"),
        }
    }
}

impl std::error::Error for StreamError {}

/// Shared realtime state. `model` is behind a mutex the callback only
/// `try_lock`s; `volume_bits` and `playing` are lock-free.
pub struct SharedAudio {
    /// Render model. Rebuilt off-thread on preset or mode change.
    pub model: Mutex<RenderModel>,
    /// `f32` volume bits, see `load_volume`.
    pub volume_bits: AtomicU32,
    /// Atomic playing flag, 1 while playing.
    pub playing: AtomicU32,
    /// Frames rendered since stream open, for the pulse scheduler.
    pub frames: AtomicU64,
}

/// Open-handed stream handle. Drop stops playback.
pub struct OutputStream {
    _stream: cpal::Stream,
    /// Configured sample rate, for table rebuilds.
    pub sample_rate: u32,
}

impl OutputStream {
    /// Open the default device and start playback of `shared`.
    pub fn open(
        shared: Arc<SharedAudio>,
        error_log: impl Fn(String) + Send + 'static,
    ) -> Result<Self, StreamError> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| StreamError::NoDevice("no default output device".to_string()))?;

        let supported = device
            .supported_output_configs()
            .map_err(|e| StreamError::NoDevice(e.to_string()))?
            .find(|c| c.channels() == 2 && c.sample_format() == cpal::SampleFormat::F32)
            .ok_or_else(|| StreamError::NoFormat("device has no f32 stereo mix".to_string()))?;

        // Prefer 48 kHz inside the supported range, else the range max.
        let rate =
            if supported.min_sample_rate().0 <= 48_000 && 48_000 <= supported.max_sample_rate().0 {
                cpal::SampleRate(48_000)
            } else {
                supported.max_sample_rate()
            };
        let config = supported.with_sample_rate(rate).config();

        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _| {
                    let volume = load_volume(&shared.volume_bits);
                    let playing = shared.playing.load(Ordering::Relaxed) == 1;
                    if let Ok(mut model) = shared.model.try_lock() {
                        model.render_into(data, volume, playing);
                    } else {
                        data.fill(0.0);
                    }
                    shared
                        .frames
                        .fetch_add((data.len() / 2) as u64, Ordering::Relaxed);
                },
                move |err| error_log(err.to_string()),
                None,
            )
            .map_err(|e| StreamError::Stream(e.to_string()))?;
        stream
            .play()
            .map_err(|e| StreamError::Stream(e.to_string()))?;
        Ok(Self {
            _stream: stream,
            sample_rate: rate.0,
        })
    }

    /// Human-readable output device name, if known.
    #[must_use]
    pub fn device_name() -> Option<String> {
        let device = cpal::default_host().default_output_device()?;
        device.name().ok()
    }
}
