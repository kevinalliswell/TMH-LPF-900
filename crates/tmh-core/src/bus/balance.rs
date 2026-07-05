//! Balance bus actor — continuous line reads, plus the tare command sequence
//! (docs/04 §4.3). The actor is single-threaded, so `tare()` runs to completion with
//! no poll frames interleaved (the "去皮期间无轮询" invariant is structural).

use crate::health::Health;
use crate::transport::Transport;
use std::time::{Duration, Instant};
use tmh_protocols::balance::{self, TareResult, TARE_CMD};

/// Snapshot of the balance (`weight` grams; `None` = never read).
#[derive(Debug, Clone, serde::Serialize)]
pub struct BalanceSnapshot {
    pub weight: Option<f64>,
    pub connected: bool,
    pub consecutive_errors: u32,
}

/// Owns the balance transport + health.
pub struct BalanceActor {
    transport: Box<dyn Transport>,
    weight: Option<f64>,
    pub health: Health,
}

impl BalanceActor {
    pub fn new(transport: Box<dyn Transport>) -> Self {
        Self {
            transport,
            weight: None,
            health: Health::new(),
        }
    }

    /// Read one line. A stable ("G S") reading updates weight + health; a non-stable
    /// line is ignored (not an error). A transport error advances the health machine.
    pub fn poll_once(&mut self, now: Instant, deadline: Duration) {
        match self.transport.read_line(deadline) {
            Ok(bytes) => {
                if let Some(w) = std::str::from_utf8(&bytes)
                    .ok()
                    .and_then(balance::parse_line)
                {
                    self.weight = Some(w);
                    self.health.on_success(now);
                }
            }
            Err(_) => self.health.on_error(),
        }
    }

    /// Run the tare sequence: pause implied (single-threaded), reset buffers, send
    /// `TARE_CMD`, then accumulate line reads and judge the outcome (docs/04 §4.3).
    pub fn tare(&mut self) -> TareResult {
        let _ = self.transport.reset_buffers();
        let _ = self
            .transport
            .transact(&TARE_CMD, Duration::from_millis(1000));
        let mut acc = String::new();
        for _ in 0..15 {
            if let Ok(bytes) = self.transport.read_line(Duration::from_millis(150)) {
                if let Ok(s) = std::str::from_utf8(&bytes) {
                    acc.push_str(s);
                }
            }
            if acc.contains("A00") && acc.contains("G S") {
                break;
            }
        }
        let res = balance::parse_tare_response(&acc);
        if res.success {
            self.weight = res.weight;
        }
        res
    }

    pub fn reconnect(&mut self) -> bool {
        self.transport.reopen().is_ok()
    }

    pub fn snapshot(&self) -> BalanceSnapshot {
        BalanceSnapshot {
            weight: self.weight,
            connected: self.health.connected,
            consecutive_errors: self.health.consecutive_errors,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{MockTransport, TareOutcome};

    #[test]
    fn poll_reads_stable_weight() {
        let mut a = BalanceActor::new(Box::new(MockTransport::balance(88.1)));
        a.poll_once(Instant::now(), Duration::from_millis(10));
        assert_eq!(a.snapshot().weight, Some(88.1));
        assert!(a.snapshot().connected);
    }

    #[test]
    fn tare_three_branches() {
        for (outcome, expect_ok) in [
            (TareOutcome::A00, true),
            (TareOutcome::NearZero, true),
            (TareOutcome::Fail, false),
        ] {
            let mut m = MockTransport::balance(3.0);
            m.set_tare_outcome(outcome);
            let mut a = BalanceActor::new(Box::new(m));
            let res = a.tare();
            assert_eq!(res.success, expect_ok, "{outcome:?}");
        }
    }

    #[test]
    fn a00_tare_zeroes_weight() {
        let mut m = MockTransport::balance(5.0);
        m.set_tare_outcome(TareOutcome::A00);
        let mut a = BalanceActor::new(Box::new(m));
        let res = a.tare();
        assert!(res.success && res.a00_found);
        assert_eq!(a.snapshot().weight, Some(0.0));
    }
}
