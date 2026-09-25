//! Audio rendering for BT KeepAlive.
//!
//! Design: expensive DSP runs once at stream start into plain `Vec<f32>`
//! tables. The realtime callback only reads tables, an atomic volume,
//! and integer loop positions. No allocation, no RNG, no locks on the
//! hot path; shared state uses atomics plus a `try_lock` fallback to
//! silence so the callback never blocks.
//!
//! Windows playback goes through `cpal` (WASAPI shared mode). The pure
//! render model plus a hand-rolled WAV writer let Linux and CI verify
//! the full DSP path without sound hardware.

pub mod model;
pub mod scheduler;
pub mod tables;
pub mod wav;

#[cfg(target_os = "windows")]
pub mod stream_cpal;

pub use model::{RenderModel, RenderParams};
pub use scheduler::{PulseScheduler, SchedulerAction};
pub use tables::{build_binaural_table, build_preset_table};
