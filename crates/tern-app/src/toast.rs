//! Non-blocking status messages (S2-T07), shown in the status bar and auto-dismissed.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

impl ToastKind {
    fn ttl(self) -> Duration {
        match self {
            ToastKind::Error => Duration::from_secs(6),
            _ => Duration::from_secs(3),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    pub kind: ToastKind,
    pub text: String,
    expires: Instant,
}

/// Newest last; the status bar shows the newest. Time is passed in so tests need no sleeping.
#[derive(Debug, Default)]
pub struct Toasts(VecDeque<Toast>);

const MAX_QUEUED: usize = 5;

impl Toasts {
    pub fn push(&mut self, kind: ToastKind, text: impl Into<String>, now: Instant) {
        if self.0.len() == MAX_QUEUED {
            self.0.pop_front();
        }
        self.0.push_back(Toast {
            kind,
            text: text.into(),
            expires: now + kind.ttl(),
        });
    }

    /// Drop everything whose time is up.
    pub fn expire(&mut self, now: Instant) {
        self.0.retain(|t| t.expires > now);
    }

    pub fn current(&self) -> Option<&Toast> {
        self.0.back()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toasts_expire_after_their_ttl() {
        let t0 = Instant::now();
        let mut t = Toasts::default();
        assert!(t.is_empty());
        t.push(ToastKind::Success, "saved", t0);
        t.expire(t0 + Duration::from_secs(2));
        assert_eq!(t.current().map(|x| x.text.as_str()), Some("saved"));
        t.expire(t0 + Duration::from_secs(3));
        assert!(t.is_empty());
    }

    #[test]
    fn errors_linger_longer_and_newest_is_shown() {
        let t0 = Instant::now();
        let mut t = Toasts::default();
        t.push(ToastKind::Error, "boom", t0);
        t.push(ToastKind::Success, "ok", t0);
        assert_eq!(t.current().map(|x| x.kind), Some(ToastKind::Success));
        t.expire(t0 + Duration::from_secs(4));
        assert_eq!(t.current().map(|x| x.kind), Some(ToastKind::Error));
        t.expire(t0 + Duration::from_secs(6));
        assert!(t.is_empty());
    }

    #[test]
    fn queue_is_bounded() {
        let t0 = Instant::now();
        let mut t = Toasts::default();
        for i in 0..20 {
            t.push(ToastKind::Info, format!("{i}"), t0);
        }
        assert_eq!(t.0.len(), MAX_QUEUED);
        assert_eq!(t.current().map(|x| x.text.as_str()), Some("19"));
    }
}
