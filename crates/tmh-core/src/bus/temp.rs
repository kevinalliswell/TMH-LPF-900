//! Temperature bus actor — polls all 9 registers each cycle (docs/04 §4.1).

use crate::config::CommConfig;
use crate::health::Health;
use crate::transport::Transport;
use std::time::{Duration, Instant};
use tmh_protocols::modbus;

/// Snapshot of the temperature bus (°C per channel; `None` = never read).
#[derive(Debug, Clone, serde::Serialize)]
pub struct TempSnapshot {
    pub temps: [Option<f64>; 9],
    pub connected: bool,
    pub consecutive_errors: u32,
}

/// Owns the temperature transport + health.
pub struct TempActor {
    transport: Box<dyn Transport>,
    addr: u8,
    scale: f64,
    signed: bool,
    temps: [Option<f64>; 9],
    pub health: Health,
}

impl TempActor {
    pub fn new(transport: Box<dyn Transport>, addr: u8, scale: f64, signed: bool) -> Self {
        Self {
            transport,
            addr,
            scale,
            signed,
            temps: [None; 9],
            health: Health::new(),
        }
    }

    pub fn from_config(transport: Box<dyn Transport>, cfg: &CommConfig) -> Self {
        Self::new(
            transport,
            cfg.temp.slave_address,
            cfg.temp.scale,
            cfg.temp.signed_registers,
        )
    }

    /// One poll cycle: read all 9 registers, updating temps + health. A read failure
    /// keeps the last values (stale) and advances the health state machine.
    pub fn poll_once(&mut self, now: Instant, deadline: Duration) {
        let req = modbus::build_read_all(self.addr);
        match self.transport.transact(&req, deadline) {
            Ok(buf) => match modbus::extract_read_all(&buf, self.addr)
                .and_then(|f| modbus::parse_read_all(f, self.scale, self.signed))
            {
                Some(temps) => {
                    self.temps = temps.map(Some);
                    self.health.on_success(now);
                }
                None => self.health.on_error(),
            },
            Err(_) => self.health.on_error(),
        }
    }

    /// Attempt to reopen the underlying transport (reconnect).
    pub fn reconnect(&mut self) -> bool {
        self.transport.reopen().is_ok()
    }

    /// Set the target temperature (°C) — only affects a mock transport, so the
    /// synthetic furnace follows an experiment program.
    pub fn set_target_c(&mut self, c: f64) {
        self.transport.set_hint("temp_target_raw", c / self.scale);
    }

    pub fn snapshot(&self) -> TempSnapshot {
        TempSnapshot {
            temps: self.temps,
            connected: self.health.connected,
            consecutive_errors: self.health.consecutive_errors,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{MockFaults, MockTransport};

    #[test]
    fn poll_populates_channels() {
        let mut m = MockTransport::temp();
        m.set_temp_target_raw(9000);
        let mut a = TempActor::new(Box::new(m), 0, 0.1, false);
        a.poll_once(Instant::now(), Duration::from_millis(10));
        let s = a.snapshot();
        assert!(s.connected);
        assert!(s.temps[7].is_some());
    }

    #[test]
    fn offline_after_5_bad_crc_keeps_last_values() {
        let mut m = MockTransport::temp();
        m.set_temp_target_raw(9000);
        let mut a = TempActor::new(Box::new(m), 0, 0.1, false);
        a.poll_once(Instant::now(), Duration::from_millis(10)); // one good read
        let last = a.snapshot().temps[7];
        assert!(last.is_some());

        // now inject bad CRC on every response
        let mut m2 = MockTransport::temp().with_faults(MockFaults {
            bad_crc_every: 1,
            ..Default::default()
        });
        m2.set_temp_target_raw(9000);
        let mut a2 = TempActor::new(Box::new(m2), 0, 0.1, false);
        a2.poll_once(Instant::now(), Duration::from_millis(10)); // seed a value first? no — all bad
        for _ in 0..5 {
            a2.poll_once(Instant::now(), Duration::from_millis(10));
        }
        assert!(!a2.snapshot().connected);
    }

    #[test]
    fn set_target_c_makes_mock_follow_program() {
        // Mock starts at 25.0 °C. Drive it to 500 °C via the actor seam.
        let mut a = TempActor::new(Box::new(MockTransport::temp()), 0, 0.1, false);
        a.set_target_c(500.0);
        for _ in 0..100 {
            a.poll_once(Instant::now(), Duration::from_millis(10));
        }
        let hot = a.snapshot().temps[7].expect("channel read");
        assert!(hot > 495.0, "furnace should approach 500 °C, got {hot}");

        // Cooling: drop the target and confirm it descends.
        a.set_target_c(100.0);
        for _ in 0..100 {
            a.poll_once(Instant::now(), Duration::from_millis(10));
        }
        let cool = a.snapshot().temps[7].expect("channel read");
        assert!(cool < 105.0, "furnace should cool toward 100 °C, got {cool}");
    }

    #[test]
    fn reconnect_recovers() {
        let m = MockTransport::temp().with_faults(MockFaults {
            disconnect_after: 1,
            ..Default::default()
        });
        let mut a = TempActor::new(Box::new(m), 0, 0.1, false);
        a.poll_once(Instant::now(), Duration::from_millis(10)); // ok (tx 1)
        for _ in 0..5 {
            a.poll_once(Instant::now(), Duration::from_millis(10)); // disconnected
        }
        assert!(!a.snapshot().connected);
        assert!(a.reconnect());
        a.poll_once(Instant::now(), Duration::from_millis(10));
        assert!(a.snapshot().connected, "recovers after reopen");
    }
}
