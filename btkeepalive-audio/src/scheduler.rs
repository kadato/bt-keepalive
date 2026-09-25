//! Pulse stream scheduler.
//!
//! Continuous mode holds the stream open. Pulse mode closes the stream
//! between bursts and wakes shortly before the next one, which removes
//! ~54 of every 55 seconds of wakeups of an always-open silent stream.

use std::time::{Duration, Instant};

/// What the app layer should do with the output stream.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SchedulerAction {
    /// Keep a running stream open.
    KeepOpen,
    /// Close the stream; wake after the delay to play the next burst.
    CloseFor(Duration),
    /// Open the stream now and play the burst, then close again.
    PlayBurst,
}

/// Decides open and close transitions for pulse mode.
///
/// Pure logic over sample positions; the app layer owns the real stream
/// and the timer. `cycle_len` and `pulse_len` come from `PulseParams`.
#[derive(Debug, Clone)]
pub struct PulseScheduler {
    cycle_len: u64,
    pulse_len: u64,
    sample_rate: u32,
}

impl PulseScheduler {
    /// Create a scheduler. Values of 0 are clamped to 1.
    #[must_use]
    pub fn new(sample_rate: u32, cycle_len: u64, pulse_len: u64) -> Self {
        Self {
            cycle_len: cycle_len.max(1),
            pulse_len: pulse_len.max(1),
            sample_rate: sample_rate.max(1),
        }
    }

    /// Action for a stream that is currently open at `pos` samples.
    ///
    /// Returns `CloseFor` once the burst has fully played, so the app
    /// can close the stream until just before the next cycle.
    #[must_use]
    pub fn on_open(&self, pos: u64) -> SchedulerAction {
        let in_cycle = pos % self.cycle_len;
        if in_cycle < self.pulse_len {
            SchedulerAction::KeepOpen
        } else {
            SchedulerAction::CloseFor(self.until_next_cycle(pos))
        }
    }

    /// Action for a closed stream. Returns `PlayBurst` when a burst is
    /// due within the next callback window.
    #[must_use]
    pub fn on_closed(&self, pos: u64) -> SchedulerAction {
        let in_cycle = pos % self.cycle_len;
        if in_cycle < self.pulse_len {
            SchedulerAction::PlayBurst
        } else {
            SchedulerAction::CloseFor(self.until_next_cycle(pos))
        }
    }

    fn until_next_cycle(&self, pos: u64) -> Duration {
        let remaining = self.cycle_len - pos % self.cycle_len;
        let secs = remaining as f64 / f64::from(self.sample_rate);
        // Wake a touch early so stream open latency never clips the burst.
        Duration::from_secs_f64((secs - 0.5).max(0.1))
    }

    /// Wall-clock deadline helper for tests and the app timer.
    #[must_use]
    pub fn deadline(&self, delay: Duration) -> Instant {
        Instant::now() + delay
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sched() -> PulseScheduler {
        // 55 s cycle, 1 s burst at 48 kHz.
        PulseScheduler::new(48_000, 55 * 48_000, 48_000)
    }

    #[test]
    fn stays_open_during_burst() {
        assert_eq!(sched().on_open(0), SchedulerAction::KeepOpen);
        assert_eq!(sched().on_open(47_999), SchedulerAction::KeepOpen);
    }

    #[test]
    fn closes_after_burst_with_long_sleep() {
        match sched().on_open(48_000) {
            SchedulerAction::CloseFor(d) => {
                assert!(d.as_secs() >= 50, "sleep {d:?} too short");
            }
            other => panic!("expected CloseFor, got {other:?}"),
        }
    }

    #[test]
    fn closed_stream_wakes_for_burst() {
        assert_eq!(sched().on_closed(0), SchedulerAction::PlayBurst);
        match sched().on_closed(100_000) {
            SchedulerAction::CloseFor(_) => {}
            other => panic!("expected CloseFor, got {other:?}"),
        }
    }

    #[test]
    fn wakes_early_before_next_cycle() {
        // One second before the cycle ends: wake in ~0.5 s, not 1 s.
        match sched().on_closed(55 * 48_000 - 48_000) {
            SchedulerAction::CloseFor(d) => {
                assert!(d.as_secs_f64() < 1.0, "wake {d:?} not early");
            }
            other => panic!("expected CloseFor, got {other:?}"),
        }
    }
}
