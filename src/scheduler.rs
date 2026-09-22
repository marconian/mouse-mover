//! Pure scheduling policy. No input injection, wall clock, sleeps, or Windows calls.

pub const IDLE_GRACE_MS: u32 = 30_000;
pub const PULSE_INTERVAL_MS: u32 = 240_000;
pub const RETRY_MS: u32 = 30_000;

#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Stop,
    Wait(u32),
    Pulse,
}

pub struct Scheduler {
    pub enabled: bool,
    session_available: bool,
    suspended: bool,
    not_before: u64,
}

impl Scheduler {
    pub fn new(now: u64) -> Self {
        Self {
            enabled: true,
            session_available: true,
            suspended: false,
            not_before: now.saturating_add(IDLE_GRACE_MS.into()),
        }
    }

    pub fn toggle(&mut self, now: u64) {
        self.enabled = !self.enabled;
        self.defer(now);
    }

    pub fn session_changed(&mut self, available: bool, now: u64) {
        self.session_available = available;
        self.defer(now);
    }

    pub fn power_changed(&mut self, suspended: bool, now: u64) {
        self.suspended = suspended;
        self.defer(now);
    }

    fn defer(&mut self, now: u64) {
        // Resuming must not shorten an existing pulse interval.
        self.not_before = self
            .not_before
            .max(now.saturating_add(IDLE_GRACE_MS.into()));
    }

    pub fn running(&self) -> bool {
        self.enabled && self.session_available && !self.suspended
    }

    pub fn decide(&self, now: u64, idle_ms: Option<u32>) -> Decision {
        if !self.running() {
            return Decision::Stop;
        }
        let Some(idle_ms) = idle_ms else {
            return Decision::Wait(RETRY_MS);
        };
        let remaining = self
            .not_before
            .saturating_sub(now)
            .max(IDLE_GRACE_MS.saturating_sub(idle_ms).into());
        if remaining == 0 {
            Decision::Pulse
        } else {
            Decision::Wait(remaining.min(PULSE_INTERVAL_MS.into()) as u32)
        }
    }

    pub fn attempted(&mut self, now: u64) {
        // Rate-limit failures too; a denied SendInput must never become a hot loop.
        self.not_before = now.saturating_add(PULSE_INTERVAL_MS.into());
    }
}

/// LASTINPUTINFO is a wrapping 32-bit uptime, not a wall-clock timestamp.
pub fn idle_elapsed(now: u32, last_input: u32) -> Option<u32> {
    let elapsed = now.wrapping_sub(last_input);
    // A future/anomalous sample is ambiguous, so fail closed instead of injecting.
    (elapsed <= i32::MAX as u32).then_some(elapsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_has_grace_even_if_already_idle() {
        let scheduler = Scheduler::new(1_000);
        assert_eq!(
            scheduler.decide(1_000, Some(99_000)),
            Decision::Wait(30_000)
        );
        assert_eq!(scheduler.decide(30_999, Some(99_000)), Decision::Wait(1));
        assert_eq!(scheduler.decide(31_000, Some(99_000)), Decision::Pulse);
    }

    #[test]
    fn recent_activity_always_defers() {
        let scheduler = Scheduler::new(0);
        for idle in 0..IDLE_GRACE_MS {
            assert_eq!(
                scheduler.decide(600_000, Some(idle)),
                Decision::Wait(IDLE_GRACE_MS - idle)
            );
        }
    }

    #[test]
    fn pulse_interval_is_four_minutes_not_one_poll() {
        let mut scheduler = Scheduler::new(0);
        scheduler.attempted(30_000);
        assert_eq!(scheduler.decide(30_000, Some(0)), Decision::Wait(240_000));
        assert_eq!(scheduler.decide(269_999, Some(239_999)), Decision::Wait(1));
        assert_eq!(scheduler.decide(270_000, Some(240_000)), Decision::Pulse);
    }

    #[test]
    fn typing_near_deadline_wins_over_pulse() {
        let mut scheduler = Scheduler::new(0);
        scheduler.attempted(30_000);
        assert_eq!(
            scheduler.decide(270_000, Some(5_000)),
            Decision::Wait(25_000)
        );
    }

    #[test]
    fn paused_has_no_timer_and_resume_has_grace() {
        let mut scheduler = Scheduler::new(0);
        scheduler.toggle(31_000);
        assert_eq!(scheduler.decide(100_000, Some(100_000)), Decision::Stop);
        scheduler.toggle(100_000);
        assert_eq!(
            scheduler.decide(100_000, Some(100_000)),
            Decision::Wait(30_000)
        );
        assert_eq!(scheduler.decide(130_000, Some(130_000)), Decision::Pulse);
    }

    #[test]
    fn resume_does_not_shorten_previous_pulse_interval() {
        let mut scheduler = Scheduler::new(0);
        scheduler.attempted(30_000);
        scheduler.toggle(40_000);
        scheduler.toggle(50_000);
        assert_eq!(
            scheduler.decide(80_000, Some(80_000)),
            Decision::Wait(190_000)
        );
    }

    #[test]
    fn session_lock_and_disconnect_stop_timers() {
        let mut scheduler = Scheduler::new(0);
        scheduler.session_changed(false, 40_000);
        assert_eq!(scheduler.decide(300_000, Some(300_000)), Decision::Stop);
        scheduler.session_changed(true, 300_000);
        assert_eq!(
            scheduler.decide(300_000, Some(300_000)),
            Decision::Wait(30_000)
        );
    }

    #[test]
    fn unlocking_does_not_cancel_manual_pause() {
        let mut scheduler = Scheduler::new(0);
        scheduler.toggle(10_000);
        scheduler.session_changed(false, 20_000);
        scheduler.session_changed(true, 30_000);
        assert_eq!(scheduler.decide(500_000, Some(500_000)), Decision::Stop);
    }

    #[test]
    fn sleep_and_resume_require_new_grace() {
        let mut scheduler = Scheduler::new(0);
        scheduler.power_changed(true, 40_000);
        assert_eq!(scheduler.decide(1_000_000, Some(1_000_000)), Decision::Stop);
        scheduler.power_changed(false, 1_000_000);
        assert_eq!(
            scheduler.decide(1_000_000, Some(1_000_000)),
            Decision::Wait(30_000)
        );
    }

    #[test]
    fn resuming_power_does_not_unlock_session() {
        let mut scheduler = Scheduler::new(0);
        scheduler.session_changed(false, 50_000);
        scheduler.power_changed(true, 60_000);
        scheduler.power_changed(false, 100_000);
        assert_eq!(scheduler.decide(1_000_000, Some(1_000_000)), Decision::Stop);
    }

    #[test]
    fn missing_idle_sample_fails_closed() {
        assert_eq!(
            Scheduler::new(0).decide(1_000_000, None),
            Decision::Wait(RETRY_MS)
        );
    }

    #[test]
    fn attempted_failure_is_rate_limited_too() {
        let mut scheduler = Scheduler::new(0);
        scheduler.attempted(50_000);
        assert_eq!(
            scheduler.decide(50_001, Some(90_000)),
            Decision::Wait(239_999)
        );
    }

    #[test]
    fn idle_arithmetic_handles_uptime_rollover() {
        assert_eq!(idle_elapsed(100, u32::MAX - 199), Some(300));
        assert_eq!(idle_elapsed(30_000, 0), Some(30_000));
    }

    #[test]
    fn anomalous_future_input_fails_closed() {
        assert_eq!(idle_elapsed(100, 200), None);
        assert_eq!(idle_elapsed(0, 0), Some(0));
    }
}
