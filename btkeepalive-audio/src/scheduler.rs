//! Pulse stream scheduler.
//!
//! Continuous mode holds the stream open. Pulse mode closes the stream
//! between bursts and wakes shortly before the next one, which removes
//! ~54 of every 55 seconds of wakeups of an always-open silent stream.
//!
//! `PulseScheduler` is sample-count logic kept for tests. The Windows
//! manager uses `PulseRuntime`, which tracks wall-clock deadlines so a
//! closed stream (frozen frame counter) still wakes for the next burst
//! and resumes within seconds of a headset reconnect.

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

/// Wall-clock pulse state for a manager that closes the stream.
///
/// Sample counts freeze while the stream is closed, so the manager
/// cannot derive timing from rendered frames. This tracks burst
/// start and end with `Instant` deadlines instead. A missed burst from
/// a missing device retries in seconds, so a headset that connects
/// later plays within seconds rather than waiting a full cycle.
#[derive(Debug, Clone)]
pub struct PulseRuntime {
    interval: Duration,
    duration: Duration,
    next_burst_at: Instant,
    burst_end_at: Option<Instant>,
}

impl PulseRuntime {
    /// Start a runtime whose first burst is due now.
    #[must_use]
    pub fn new(interval_sec: f64, duration_sec: f64) -> Self {
        Self::new_at(Instant::now(), interval_sec, duration_sec)
    }

    /// Start a runtime at `now`. Takes the clock for deterministic tests.
    #[must_use]
    pub fn new_at(now: Instant, interval_sec: f64, duration_sec: f64) -> Self {
        let duration = Duration::from_secs_f64(duration_sec.max(0.1));
        let mut interval = Duration::from_secs_f64(interval_sec.max(0.5));
        if interval < duration + Duration::from_secs(1) {
            interval = duration + Duration::from_secs(1);
        }
        Self {
            interval,
            duration,
            next_burst_at: now,
            burst_end_at: None,
        }
    }

    /// True when a burst should be playing now and no burst is open.
    #[must_use]
    pub fn burst_due(&self, now: Instant) -> bool {
        self.burst_end_at.is_none() && now >= self.next_burst_at
    }

    /// True while an opened burst is still within its duration.
    #[must_use]
    pub fn burst_active(&self, now: Instant) -> bool {
        self.burst_end_at.map(|end| now < end).unwrap_or(false)
    }

    /// Record a successful open at `now`. The burst ends after
    /// `duration` and the following burst starts one `interval` after
    /// this one started.
    pub fn on_opened(&mut self, now: Instant) {
        self.burst_end_at = Some(now + self.duration);
        self.next_burst_at = now + self.interval;
    }

    /// Record the end of a burst. Keeps the already scheduled next start.
    pub fn on_burst_ended(&mut self) {
        self.burst_end_at = None;
    }

    /// Record a failed open at `now`. Retries after `retry` instead of
    /// waiting a full interval, so a late headset plays quickly.
    pub fn on_open_failed(&mut self, now: Instant, retry: Duration) {
        self.burst_end_at = None;
        self.next_burst_at = now + retry;
    }

    /// How long the manager sleeps before rechecking.
    #[must_use]
    pub fn sleep_until_next(&self, now: Instant, stream_open: bool) -> Duration {
        if stream_open {
            if let Some(end) = self.burst_end_at {
                return end
                    .saturating_duration_since(now)
                    .min(Duration::from_secs(5));
            }
        }
        self.next_burst_at
            .saturating_duration_since(now)
            .min(Duration::from_secs(5))
    }

    /// Burst length, for the render path.
    #[must_use]
    pub fn duration(&self) -> Duration {
        self.duration
    }

    /// Cycle length, for diagnostics.
    #[must_use]
    pub fn interval(&self) -> Duration {
        self.interval
    }
}

/// Throttle for device-open retries.
///
/// The manager ticks every 250 ms for UI responsiveness. Without a
/// throttle a missing headset would attempt an open and log on every
/// tick. This spaces retries seconds apart and reports only when the
/// error text changes, so `%APPDATA%\\BTKeepAlive\\app.log` stays quiet
/// while the PC waits for the headset.
#[derive(Debug, Clone, Default)]
pub struct OpenRetry {
    next_at: Option<Instant>,
    last_error: Option<String>,
}

impl OpenRetry {
    /// Empty throttle that allows the first open at once.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// True when an open attempt is allowed at `now`.
    #[must_use]
    pub fn allowed(&self, now: Instant) -> bool {
        self.next_at.map(|t| now >= t).unwrap_or(true)
    }

    /// Time until the next allowed attempt. Zero when allowed now.
    #[must_use]
    pub fn wait_remaining(&self, now: Instant) -> Duration {
        self.next_at
            .map(|t| t.saturating_duration_since(now))
            .unwrap_or_default()
    }

    /// Clear backoff and the duplicate log filter after a good open.
    pub fn on_success(&mut self) {
        self.next_at = None;
        self.last_error = None;
    }

    /// Record a failure at `now`. Returns true when the caller should
    /// log, which is only the first failure or a changed message.
    /// Next attempt waits `delay`.
    pub fn on_failure(&mut self, now: Instant, error: &str, delay: Duration) -> bool {
        let should_log = self.last_error.as_deref() != Some(error);
        self.last_error = Some(error.to_string());
        self.next_at = Some(now + delay);
        should_log
    }

    /// Drop pending backoff, for device-change events that should retry now.
    pub fn clear_backoff(&mut self) {
        self.next_at = None;
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

    #[test]
    fn pulse_runtime_fires_second_burst_after_close() {
        // The old frame-count scheduler froze while closed and never
        // reached the next burst. Wall-clock deadlines advance instead.
        let start = Instant::now();
        let mut rt = PulseRuntime::new_at(start, 55.0, 1.0);
        assert!(rt.burst_due(start));
        rt.on_opened(start);
        assert!(rt.burst_active(start + Duration::from_millis(500)));
        assert!(!rt.burst_active(start + Duration::from_secs(2)));
        rt.on_burst_ended();
        assert!(!rt.burst_due(start + Duration::from_secs(10)));
        let next = start + Duration::from_secs(55);
        assert!(rt.burst_due(next));
        assert!(rt.burst_due(next + Duration::from_secs(5)));
    }

    #[test]
    fn pulse_runtime_retries_quickly_after_missing_device() {
        let start = Instant::now();
        let mut rt = PulseRuntime::new_at(start, 55.0, 1.0);
        rt.on_open_failed(start, Duration::from_secs(2));
        assert!(!rt.burst_due(start + Duration::from_secs(1)));
        assert!(rt.burst_due(start + Duration::from_secs(3)));
    }

    #[test]
    fn open_retry_throttles_and_logs_once() {
        let start = Instant::now();
        let mut retry = OpenRetry::default();
        assert!(retry.allowed(start));
        assert!(retry.on_failure(start, "no device", Duration::from_secs(2)));
        assert!(!retry.allowed(start + Duration::from_secs(1)));
        // Same error stays quiet.
        assert!(!retry.on_failure(
            start + Duration::from_secs(1),
            "no device",
            Duration::from_secs(2)
        ));
        // Changed error logs again.
        assert!(retry.on_failure(
            start + Duration::from_secs(1),
            "new error",
            Duration::from_secs(2)
        ));
        retry.on_success();
        assert!(retry.allowed(start + Duration::from_secs(2)));
    }
}
