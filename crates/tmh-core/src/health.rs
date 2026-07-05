//! Per-bus health + reconnection state machine (docs/04 §4.4). Five consecutive
//! failures take a bus offline; reconnection then backs off [1,2,5,10,30]s (held at
//! 30s). Pure logic — the owning bus thread drives it and does the real sleeping and
//! `reopen()`. `now: Instant` is injected so tests are deterministic.

use std::time::{Duration, Instant};

/// Consecutive failures that take a bus offline.
pub const OFFLINE_THRESHOLD: u32 = 5;

const BACKOFF_SECS: [u64; 5] = [1, 2, 5, 10, 30];

/// Health + reconnection state for one bus.
#[derive(Debug, Clone)]
pub struct Health {
    pub connected: bool,
    pub consecutive_errors: u32,
    pub last_ok: Option<Instant>,
    reconnect_attempt: u32,
}

impl Default for Health {
    fn default() -> Self {
        Self {
            connected: true,
            consecutive_errors: 0,
            last_ok: None,
            reconnect_attempt: 0,
        }
    }
}

impl Health {
    pub fn new() -> Self {
        Self::default()
    }

    /// A successful transaction: back online, counters and backoff reset.
    pub fn on_success(&mut self, now: Instant) {
        self.connected = true;
        self.consecutive_errors = 0;
        self.last_ok = Some(now);
        self.reconnect_attempt = 0;
    }

    /// A failed transaction. Goes offline at [`OFFLINE_THRESHOLD`] consecutive fails.
    pub fn on_error(&mut self) {
        self.consecutive_errors = self.consecutive_errors.saturating_add(1);
        if self.consecutive_errors >= OFFLINE_THRESHOLD {
            self.connected = false;
        }
    }

    /// Delay before the next reconnect attempt, advancing the [1,2,5,10,30]s
    /// schedule (held at 30s afterwards).
    pub fn next_backoff(&mut self) -> Duration {
        let idx = (self.reconnect_attempt as usize).min(BACKOFF_SECS.len() - 1);
        self.reconnect_attempt = self.reconnect_attempt.saturating_add(1);
        Duration::from_secs(BACKOFF_SECS[idx])
    }

    /// Stale if the last success is older than `timeout` (or there was never one).
    pub fn is_stale(&self, now: Instant, timeout: Duration) -> bool {
        match self.last_ok {
            Some(t) => now.duration_since(t) > timeout,
            None => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_consecutive_errors_go_offline() {
        let mut h = Health::new();
        for _ in 0..4 {
            h.on_error();
        }
        assert!(h.connected, "4 errors still online");
        h.on_error();
        assert!(!h.connected, "5th error goes offline");
        assert_eq!(h.consecutive_errors, 5);
    }

    #[test]
    fn success_resets_and_recovers() {
        let mut h = Health::new();
        for _ in 0..6 {
            h.on_error();
        }
        assert!(!h.connected);
        h.on_success(Instant::now());
        assert!(h.connected);
        assert_eq!(h.consecutive_errors, 0);
    }

    #[test]
    fn backoff_follows_schedule() {
        let mut h = Health::new();
        let secs: Vec<u64> = (0..7).map(|_| h.next_backoff().as_secs()).collect();
        assert_eq!(secs, vec![1, 2, 5, 10, 30, 30, 30]);
        h.on_success(Instant::now());
        assert_eq!(h.next_backoff().as_secs(), 1, "success resets backoff");
    }

    #[test]
    fn staleness_by_timeout() {
        let mut h = Health::new();
        let t0 = Instant::now();
        h.on_success(t0);
        assert!(!h.is_stale(t0 + Duration::from_secs(3), Duration::from_secs(5)));
        assert!(h.is_stale(t0 + Duration::from_secs(6), Duration::from_secs(5)));
    }
}
