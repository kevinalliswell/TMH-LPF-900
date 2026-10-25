//! Byte-level synthetic transports for headless dev/test (docs/08). Deterministic:
//! physics steps once per `transact` toward a settable target; faults fire on
//! counters, not randomness. Works in the on-wire integer domain — per-gas scaling
//! and temperature scaling are the bus actor's job (M2c), not the transport's.

use super::{BusError, DeviceKind, Transport};
use std::time::Duration;
use tmh_protocols::cpl::cpl_checksum;
use tmh_protocols::{balance, crc16_modbus};

/// Deterministic fault injection for exercising actor resilience.
#[derive(Debug, Clone, Default)]
pub struct MockFaults {
    /// Return `Timeout` on every Nth transaction (0 = never).
    pub drop_every: u32,
    /// Corrupt the CRC of every Nth temperature response (0 = never).
    pub bad_crc_every: u32,
    /// Prepend the echoed request to temperature responses (half-duplex echo).
    pub echo: bool,
    /// After this many successful transactions, go `Disconnected` until `reopen()`
    /// (fires once; reopen recovers permanently). 0 = never.
    pub disconnect_after: u32,
}

/// Balance tare outcome the mock reports on the next read_line after `TARE_CMD`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TareOutcome {
    A00,
    NearZero,
    Fail,
}

enum MockState {
    Temp {
        regs: [u16; 9],
        target: [u16; 9],
    },
    Mfc {
        sv: [i64; 4],
        pv: [i64; 4],
    },
    Balance {
        weight: f64,
        weight_target: f64,
        tare: TareOutcome,
        tare_reply: Option<Vec<u8>>,
    },
}

/// A synthetic [`Transport`] standing in for one device.
pub struct MockTransport {
    kind: DeviceKind,
    state: MockState,
    faults: MockFaults,
    tx_count: u32,
    disconnected: bool,
    disconnect_fired: bool,
}

impl MockTransport {
    /// Temperature mock; all 9 channels start at ambient (25.0 °C @ scale 0.1).
    pub fn temp() -> Self {
        Self::new(
            DeviceKind::Temp,
            MockState::Temp {
                regs: [250; 9],
                target: [250; 9],
            },
        )
    }

    /// MFC mock; all 4 channels start at zero flow.
    pub fn mfc() -> Self {
        Self::new(
            DeviceKind::Mfc,
            MockState::Mfc {
                sv: [0; 4],
                pv: [0; 4],
            },
        )
    }

    /// Balance mock starting at `weight` grams, tare outcome A00.
    pub fn balance(weight: f64) -> Self {
        Self::new(
            DeviceKind::Balance,
            MockState::Balance {
                weight,
                weight_target: weight,
                tare: TareOutcome::A00,
                tare_reply: None,
            },
        )
    }

    fn new(kind: DeviceKind, state: MockState) -> Self {
        Self {
            kind,
            state,
            faults: MockFaults::default(),
            tx_count: 0,
            disconnected: false,
            disconnect_fired: false,
        }
    }

    /// Attach fault injection (builder style).
    pub fn with_faults(mut self, faults: MockFaults) -> Self {
        self.faults = faults;
        self
    }

    /// Which device this mock represents.
    pub fn kind(&self) -> DeviceKind {
        self.kind
    }

    /// Set the raw-register target all temperature channels approach.
    pub fn set_temp_target_raw(&mut self, raw: u16) {
        if let MockState::Temp { target, .. } = &mut self.state {
            *target = [raw; 9];
        }
    }

    /// Set the tare outcome the balance mock will report.
    pub fn set_tare_outcome(&mut self, outcome: TareOutcome) {
        if let MockState::Balance { tare, .. } = &mut self.state {
            *tare = outcome;
        }
    }

    /// Set the balance weight (grams); pins the drift target so it stays put.
    pub fn set_weight(&mut self, w: f64) {
        if let MockState::Balance {
            weight,
            weight_target,
            ..
        } = &mut self.state
        {
            *weight = w;
            *weight_target = w;
        }
    }
}

impl Transport for MockTransport {
    fn transact(&mut self, tx: &[u8], _deadline: Duration) -> Result<Vec<u8>, BusError> {
        self.tx_count += 1;
        if self.faults.disconnect_after != 0
            && !self.disconnect_fired
            && self.tx_count > self.faults.disconnect_after
        {
            self.disconnected = true;
            self.disconnect_fired = true;
        }
        if self.disconnected {
            return Err(BusError::Disconnected);
        }
        if self.faults.drop_every != 0 && self.tx_count.is_multiple_of(self.faults.drop_every) {
            return Err(BusError::Timeout);
        }

        match &mut self.state {
            MockState::Temp { regs, target } => {
                for (r, t) in regs.iter_mut().zip(target.iter()) {
                    *r = step_toward(*r, *t);
                }
                let addr = tx.first().copied().unwrap_or(0);
                let mut resp = build_temp_response(addr, regs);
                if self.faults.bad_crc_every != 0
                    && self.tx_count.is_multiple_of(self.faults.bad_crc_every)
                {
                    let last = resp.len() - 1;
                    resp[last] ^= 0xFF;
                }
                if self.faults.echo {
                    let mut out = tx.to_vec();
                    out.extend_from_slice(&resp);
                    Ok(out)
                } else {
                    Ok(resp)
                }
            }
            MockState::Mfc { sv, pv } => {
                let Some((addr, is_write, reg, val)) = parse_cpl(tx) else {
                    return Ok(Vec::new());
                };
                let idx = addr.saturating_sub(1) as usize;
                if idx >= 4 {
                    return Ok(Vec::new());
                }
                if is_write {
                    sv[idx] = val;
                    Ok(build_cpl_response(addr, sv[idx]))
                } else {
                    pv[idx] = step_toward_i64(pv[idx], sv[idx]);
                    let v = if reg == 1401 { sv[idx] } else { pv[idx] };
                    Ok(build_cpl_response(addr, v))
                }
            }
            MockState::Balance {
                weight,
                weight_target,
                tare,
                tare_reply,
            } => {
                if tx == balance::TARE_CMD.as_slice() {
                    *tare_reply = Some(tare_bytes(*tare));
                    if *tare != TareOutcome::Fail {
                        *weight = 0.0;
                        *weight_target = 0.0;
                    }
                }
                Ok(Vec::new())
            }
        }
    }

    fn read_line(&mut self, _deadline: Duration) -> Result<Vec<u8>, BusError> {
        if self.disconnected {
            return Err(BusError::Disconnected);
        }
        match &mut self.state {
            MockState::Balance {
                weight,
                weight_target,
                tare_reply,
                ..
            } => {
                if let Some(reply) = tare_reply.take() {
                    Ok(reply)
                } else {
                    // Drift toward the target so a reduction run loses mass over time.
                    *weight += (*weight_target - *weight) / 8.0;
                    Ok(format!("{weight:+.3} G S\r\n").into_bytes())
                }
            }
            _ => Err(BusError::Io("read_line only supported for balance".into())),
        }
    }

    fn reset_buffers(&mut self) -> Result<(), BusError> {
        if let MockState::Balance { tare_reply, .. } = &mut self.state {
            *tare_reply = None;
        }
        Ok(())
    }

    fn reopen(&mut self) -> Result<(), BusError> {
        self.disconnected = false;
        Ok(())
    }

    fn set_hint(&mut self, key: &str, value: f64) {
        match key {
            "temp_target_raw" => {
                if let MockState::Temp { target, .. } = &mut self.state {
                    *target = [value.clamp(0.0, u16::MAX as f64) as u16; 9];
                }
            }
            "weight_target" => {
                if let MockState::Balance { weight_target, .. } = &mut self.state {
                    *weight_target = value.max(0.0);
                }
            }
            _ => {}
        }
    }
}

fn step_toward(cur: u16, target: u16) -> u16 {
    let diff = target as i32 - cur as i32;
    let step = if diff / 4 == 0 {
        diff.signum()
    } else {
        diff / 4
    };
    (cur as i32 + step) as u16
}

fn step_toward_i64(cur: i64, target: i64) -> i64 {
    let diff = target - cur;
    let step = if diff / 4 == 0 {
        diff.signum()
    } else {
        diff / 4
    };
    cur + step
}

fn build_temp_response(addr: u8, regs: &[u16; 9]) -> Vec<u8> {
    let mut f = vec![addr, 0x03, 18];
    for &r in regs {
        f.push((r >> 8) as u8);
        f.push((r & 0xFF) as u8);
    }
    let crc = crc16_modbus(&f);
    f.push((crc & 0xFF) as u8);
    f.push((crc >> 8) as u8);
    f
}

fn build_cpl_response(addr: u8, value: i64) -> Vec<u8> {
    // <STX>{addr:02X}00X20,{value}<ETX>{ck}\r\n  — 2nd comma field is the reading.
    let mut body = vec![0x02];
    body.extend_from_slice(format!("{addr:02X}00X20,{value}").as_bytes());
    body.push(0x03);
    body.extend_from_slice(&cpl_checksum(&body));
    body.extend_from_slice(b"\r\n");
    body
}

/// Parse a mock-inbound CPL request into (addr, is_write, reg, value_or_count).
fn parse_cpl(tx: &[u8]) -> Option<(u8, bool, u16, i64)> {
    let s = core::str::from_utf8(tx).ok()?;
    let stx = s.find('\u{2}')?;
    let etx_rel = s[stx + 1..].find('\u{3}')?;
    let mid = &s[stx + 1..stx + 1 + etx_rel]; // e.g. "0100XRS,1206W,2"
    if mid.len() < 9 {
        return None;
    }
    let addr = u8::from_str_radix(&mid[0..2], 16).ok()?;
    let is_write = mid.as_bytes()[5] == b'W';
    let mut it = mid[8..].split("W,");
    let reg = it.next()?.parse::<u16>().ok()?;
    let val = it.next()?.parse::<i64>().ok()?;
    Some((addr, is_write, reg, val))
}

fn tare_bytes(o: TareOutcome) -> Vec<u8> {
    match o {
        TareOutcome::A00 => b"A00\r\n+0.000 G S".to_vec(),
        TareOutcome::NearZero => b"+0.001 G S".to_vec(),
        TareOutcome::Fail => b"+5.000 G S".to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tmh_protocols::{balance, modbus};

    #[test]
    fn temp_response_parses_and_approaches_target() {
        let mut t = MockTransport::temp();
        t.set_temp_target_raw(9000); // 900.0 °C
        let req = modbus::build_read_all(0);
        let mut temps = [0.0; 9];
        for _ in 0..60 {
            let resp = t.transact(&req, Duration::from_millis(10)).unwrap();
            let frame = modbus::extract_read_all(&resp, 0).expect("valid frame");
            temps = modbus::parse_read_all(frame, 0.1, false).unwrap();
        }
        assert!(
            (temps[7] - 900.0).abs() < 1.0,
            "T8 approaches 900: {}",
            temps[7]
        );
    }

    #[test]
    fn temp_bad_crc_makes_extract_fail() {
        let mut t = MockTransport::temp().with_faults(MockFaults {
            bad_crc_every: 1,
            ..Default::default()
        });
        let resp = t
            .transact(&modbus::build_read_all(0), Duration::from_millis(10))
            .unwrap();
        assert_eq!(modbus::extract_read_all(&resp, 0), None);
    }

    #[test]
    fn temp_echo_is_skipped_by_extract() {
        let mut t = MockTransport::temp().with_faults(MockFaults {
            echo: true,
            ..Default::default()
        });
        let req = modbus::build_read_all(0);
        let resp = t.transact(&req, Duration::from_millis(10)).unwrap();
        assert!(resp.len() > 23, "echo prepended");
        let frame = modbus::extract_read_all(&resp, 0).expect("skips echo");
        assert_eq!(frame[2], 18);
    }

    #[test]
    fn drop_fault_times_out() {
        let mut t = MockTransport::temp().with_faults(MockFaults {
            drop_every: 1,
            ..Default::default()
        });
        assert_eq!(
            t.transact(&modbus::build_read_all(0), Duration::from_millis(10)),
            Err(BusError::Timeout)
        );
    }

    #[test]
    fn disconnect_after_then_reopen_recovers() {
        let mut t = MockTransport::temp().with_faults(MockFaults {
            disconnect_after: 1,
            ..Default::default()
        });
        let req = modbus::build_read_all(0);
        assert!(t.transact(&req, Duration::from_millis(10)).is_ok());
        assert_eq!(
            t.transact(&req, Duration::from_millis(10)),
            Err(BusError::Disconnected)
        );
        assert_eq!(
            t.transact(&req, Duration::from_millis(10)),
            Err(BusError::Disconnected)
        );
        t.reopen().unwrap();
        assert!(
            t.transact(&req, Duration::from_millis(10)).is_ok(),
            "recovers after reopen"
        );
    }

    #[test]
    fn mfc_write_then_read_sv_roundtrips() {
        let mut t = MockTransport::mfc();
        let _ = t
            .transact(
                &tmh_protocols::cpl::build_write(1, 1401, &[300]),
                Duration::from_millis(10),
            )
            .unwrap();
        let resp = t
            .transact(
                &tmh_protocols::cpl::build_read(1, 1401, 2),
                Duration::from_millis(10),
            )
            .unwrap();
        assert_eq!(tmh_protocols::cpl::parse_response(&resp), Some(30.0)); // 300 wire / 10
    }

    #[test]
    fn mfc_pv_approaches_sv() {
        let mut t = MockTransport::mfc();
        let _ = t
            .transact(
                &tmh_protocols::cpl::build_write(2, 1401, &[500]),
                Duration::from_millis(10),
            )
            .unwrap();
        let mut pv = 0.0;
        for _ in 0..60 {
            let resp = t
                .transact(
                    &tmh_protocols::cpl::build_read(2, 1206, 2),
                    Duration::from_millis(10),
                )
                .unwrap();
            pv = tmh_protocols::cpl::parse_response(&resp).unwrap();
        }
        assert!((pv - 50.0).abs() < 0.5, "PV approaches SV: {pv}");
    }

    #[test]
    fn balance_line_parses() {
        let mut t = MockTransport::balance(88.1);
        let line = t.read_line(Duration::from_millis(10)).unwrap();
        let s = String::from_utf8(line).unwrap();
        assert_eq!(balance::parse_line(&s), Some(88.1));
    }

    #[test]
    fn balance_tare_branches() {
        for (outcome, ok, a00) in [
            (TareOutcome::A00, true, true),
            (TareOutcome::NearZero, true, false),
            (TareOutcome::Fail, false, false),
        ] {
            let mut t = MockTransport::balance(3.0);
            t.set_tare_outcome(outcome);
            let _ = t
                .transact(&balance::TARE_CMD, Duration::from_millis(10))
                .unwrap();
            let reply = t.read_line(Duration::from_millis(10)).unwrap();
            let res = balance::parse_tare_response(&String::from_utf8(reply).unwrap());
            assert_eq!(res.success, ok, "{outcome:?}");
            assert_eq!(res.a00_found, a00, "{outcome:?}");
        }
    }
}
