//! Idle auto-lock (S8-T02). Pure bookkeeping: the app feeds it activity and ticks and acts on the
//! answer. Wall-clock time is used on purpose: the monotonic clock does not advance while the
//! machine is suspended, so a long gap between ticks is how a sleep/resume is detected.

use std::time::{Duration, SystemTime};

pub const IDLE_MINUTES: &str = "lock.idle_minutes";
pub const KEEP_SESSIONS: &str = "lock.keep_sessions";
pub const KEYS: [&str; 2] = [IDLE_MINUTES, KEEP_SESSIONS];

pub const DEFAULT_IDLE_MINUTES: u32 = 15;
/// How often the app checks for idleness.
pub const TICK: Duration = Duration::from_secs(15);
/// A gap this long between two ticks means the machine slept (or the clock jumped).
const SUSPEND_GAP: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
pub struct AutoLock {
    /// 0 disables auto-lock.
    pub idle_minutes: u32,
    /// Keep terminal sessions and tunnels running while the vault is locked.
    pub keep_sessions: bool,
    last_activity: SystemTime,
    last_tick: SystemTime,
}

impl AutoLock {
    pub fn new(now: SystemTime) -> Self {
        Self {
            idle_minutes: DEFAULT_IDLE_MINUTES,
            keep_sessions: false,
            last_activity: now,
            last_tick: now,
        }
    }

    pub fn enabled(&self) -> bool {
        self.idle_minutes > 0
    }

    /// Apply one stored setting (JSON text); unknown or malformed values keep the current one.
    pub fn apply(&mut self, key: &str, value: &str) {
        match key {
            IDLE_MINUTES => {
                if let Ok(v) = serde_json::from_str::<u32>(value.trim()) {
                    self.idle_minutes = v.min(24 * 60);
                }
            }
            KEEP_SESSIONS => {
                if let Ok(v) = serde_json::from_str::<bool>(value.trim()) {
                    self.keep_sessions = v;
                }
            }
            _ => {}
        }
    }

    /// Keyboard/mouse activity, or a fresh unlock.
    pub fn touch(&mut self, now: SystemTime) {
        self.last_activity = now;
        self.last_tick = now;
    }

    /// Periodic check. True when the vault should lock now: idle for `idle_minutes`, or the
    /// machine slept since the previous tick.
    pub fn should_lock(&mut self, now: SystemTime) -> bool {
        let since_tick = now.duration_since(self.last_tick).unwrap_or_default();
        self.last_tick = now;
        if !self.enabled() {
            return false;
        }
        if since_tick > SUSPEND_GAP {
            return true;
        }
        let idle = now.duration_since(self.last_activity).unwrap_or_default();
        idle >= Duration::from_secs(u64::from(self.idle_minutes) * 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000 + secs)
    }

    fn lock(minutes: u32) -> AutoLock {
        let mut a = AutoLock::new(at(0));
        a.idle_minutes = minutes;
        a
    }

    #[test]
    fn locks_only_after_the_idle_time() {
        let mut a = lock(1);
        assert!(!a.should_lock(at(15)));
        assert!(!a.should_lock(at(45)));
        assert!(a.should_lock(at(60)));
    }

    #[test]
    fn activity_resets_the_timer() {
        let mut a = lock(1);
        assert!(!a.should_lock(at(30)));
        a.touch(at(50));
        assert!(!a.should_lock(at(65)));
        assert!(a.should_lock(at(110)));
    }

    #[test]
    fn zero_disables() {
        let mut a = lock(0);
        assert!(!a.should_lock(at(100_000)));
    }

    #[test]
    fn a_long_gap_between_ticks_means_the_machine_slept() {
        let mut a = lock(30);
        assert!(!a.should_lock(at(15)));
        a.touch(at(20));
        assert!(a.should_lock(at(20 + 61)));
    }

    #[test]
    fn clock_going_backwards_is_harmless() {
        let mut a = lock(1);
        a.touch(at(100));
        assert!(!a.should_lock(at(50)));
    }

    #[test]
    fn settings_are_parsed_defensively() {
        let mut a = lock(15);
        a.apply(IDLE_MINUTES, "5");
        a.apply(KEEP_SESSIONS, "true");
        assert_eq!((a.idle_minutes, a.keep_sessions), (5, true));
        a.apply(IDLE_MINUTES, "\"x\"");
        a.apply(KEEP_SESSIONS, "maybe");
        assert_eq!((a.idle_minutes, a.keep_sessions), (5, true));
        a.apply(IDLE_MINUTES, "99999999");
        assert_eq!(a.idle_minutes, 24 * 60);
    }
}
