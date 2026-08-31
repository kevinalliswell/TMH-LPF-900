//! MFC bus actor — 4 slaves on one line, priority queue (set preempts poll), and
//! the safety clamp choke point (docs/04 §4.2). Works in the display (L/min) domain;
//! the wire-integer scaling (`raw = value / scaling * 10`) is applied at frame build.

use crate::clamp::{clamp_setpoint, ClampResult, Gas};
use crate::config::CommConfig;
use crate::health::Health;
use crate::transport::{BusError, Transport};
use std::time::{Duration, Instant};
use tmh_protocols::cpl;

const REG_PV: u16 = 1206;
const REG_SV: u16 = 1401;
const TXN_TIMEOUT: Duration = Duration::from_millis(500);

/// Gas index order used everywhere in this actor.
const IDX_H2: usize = 0;
const IDX_CO2: usize = 2;
const IDX_CO: usize = 3;

/// One gas channel reading (display L/min).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct GasReading {
    pub pv: f64,
    pub sv: f64,
    pub on: bool,
}

/// Snapshot of the MFC bus (gas order H2, N2, CO2, CO).
#[derive(Debug, Clone, serde::Serialize)]
pub struct MfcSnapshot {
    pub gas: [GasReading; 4],
    pub connected: bool,
    pub consecutive_errors: u32,
}

/// A HIGH-priority command (preempts polling at the next transaction boundary).
#[derive(Debug, Clone)]
pub enum MfcCommand {
    SetFlow { gas: usize, value: f64 },
    SetOnOff { gas: usize, on: bool },
    ShutoffFlammable,
}

/// A write command is complete only after the MFC transport returns a response.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MfcCommandError {
    #[error("invalid gas channel: {0}")]
    InvalidGas(usize),
    #[error(transparent)]
    Transport(#[from] BusError),
}

/// Owns the MFC transport + per-gas state.
pub struct MfcActor {
    transport: Box<dyn Transport>,
    addr: [u8; 4],
    scaling: [f64; 4],
    limit_h2: f64,
    limit_co: f64,
    gas: [GasReading; 4],
    poll_cursor: usize,
    pub health: Health,
}

fn gas_enum(idx: usize) -> Gas {
    match idx {
        0 => Gas::H2,
        1 => Gas::N2,
        2 => Gas::Co2,
        _ => Gas::Co,
    }
}

impl MfcActor {
    pub fn new(
        transport: Box<dyn Transport>,
        addr: [u8; 4],
        scaling: [f64; 4],
        limit_h2: f64,
        limit_co: f64,
    ) -> Self {
        Self {
            transport,
            addr,
            scaling,
            limit_h2,
            limit_co,
            gas: [GasReading {
                pv: 0.0,
                sv: 0.0,
                on: false,
            }; 4],
            poll_cursor: 0,
            health: Health::new(),
        }
    }

    pub fn from_config(transport: Box<dyn Transport>, cfg: &CommConfig) -> Self {
        let a = &cfg.slave_address_mfc;
        let s = &cfg.flow_scaling;
        Self::new(
            transport,
            [a.h2, a.n2, a.co2, a.co],
            [s.h2, s.n2, s.co2, s.co],
            cfg.gas_safety_limits.h2,
            cfg.gas_safety_limits.co,
        )
    }

    /// Set a gas flow. Passes through the safety clamp (the sole choke point) before
    /// building the frame; returns the clamp decision (for event + audit).
    pub fn set_flow(&mut self, gas_idx: usize, value: f64) -> Result<ClampResult, MfcCommandError> {
        // The webview is untrusted (docs/03): an out-of-range gas index must never panic
        // this bus thread — a panic drops the command receiver and silently disables ALL
        // gas control, including the emergency shutoff. Out of range is safely rejected.
        if gas_idx >= self.gas.len() {
            return Err(MfcCommandError::InvalidGas(gas_idx));
        }
        let clamp = clamp_setpoint(gas_enum(gas_idx), value, self.limit_h2, self.limit_co);
        let raw = (clamp.applied / self.scaling[gas_idx] * 10.0).round() as i64;
        let frame = cpl::build_write(self.addr[gas_idx], REG_SV, &[raw]);
        let response = match self.transport.transact(&frame, TXN_TIMEOUT) {
            Ok(response) => response,
            Err(error) => {
                self.health.on_error();
                return Err(error.into());
            }
        };
        let acknowledged = cpl::parse_response(&response);
        let invalid_ack = match acknowledged {
            Some(value) => (value - raw as f64 / 10.0).abs() > 1e-6,
            None => true,
        };
        if invalid_ack {
            self.health.on_error();
            return Err(MfcCommandError::Transport(BusError::Io(
                "invalid or mismatched MFC write acknowledgement".into(),
            )));
        }
        self.health.on_success(Instant::now());
        self.gas[gas_idx].sv = clamp.applied;
        self.gas[gas_idx].on = clamp.applied > 0.0;
        Ok(clamp)
    }

    /// Handle one HIGH-priority command; returns the clamp result for `SetFlow`.
    pub fn handle_command(
        &mut self,
        cmd: &MfcCommand,
    ) -> Result<Option<ClampResult>, MfcCommandError> {
        match cmd {
            MfcCommand::SetFlow { gas, value } => self.set_flow(*gas, *value).map(Some),
            MfcCommand::SetOnOff { gas, on } => {
                // Bounds-guard the untrusted index (see `set_flow`): out of range -> error.
                let target = if *on {
                    self.gas.get(*gas).map(|g| g.sv.max(0.0)).unwrap_or(0.0)
                } else {
                    0.0
                };
                self.set_flow(*gas, target).map(|_| None)
            }
            MfcCommand::ShutoffFlammable => {
                // H2, CO2, CO -> 0. Zero always passes the clamp (never blocked).
                // Attempt all channels even after an earlier failure: a dead H2 slave must
                // not prevent CO2/CO from receiving their zero commands.
                let mut first_error = None;
                for g in [IDX_H2, IDX_CO2, IDX_CO] {
                    if let Err(error) = self.set_flow(g, 0.0) {
                        if first_error.is_none() {
                            first_error = Some(error);
                        }
                    }
                }
                match first_error {
                    Some(error) => Err(error),
                    None => Ok(None),
                }
            }
        }
    }

    /// Apply all four gas setpoints as one acknowledged batch. Every channel is attempted
    /// even when another channel fails, which preserves best-effort safety on a partial bus.
    pub fn set_atmosphere(
        &mut self,
        values: [f64; 4],
    ) -> Result<[ClampResult; 4], MfcCommandError> {
        let mut results = [clamp_setpoint(Gas::N2, 0.0, self.limit_h2, self.limit_co); 4];
        let mut first_error = None;
        for (gas, value) in values.into_iter().enumerate() {
            match self.set_flow(gas, value) {
                Ok(result) => results[gas] = result,
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(results),
        }
    }

    /// Poll the next slave's PV (round-robin over the 4 gases).
    pub fn poll_next(&mut self, now: Instant) {
        let idx = self.poll_cursor;
        self.poll_cursor = (self.poll_cursor + 1) % 4;
        let frame = cpl::build_read(self.addr[idx], REG_PV, 2);
        match self.transport.transact(&frame, TXN_TIMEOUT) {
            Ok(buf) => match cpl::parse_response(&buf) {
                Some(v) => {
                    self.gas[idx].pv = v * self.scaling[idx];
                    self.health.on_success(now);
                }
                None => self.health.on_error(),
            },
            Err(_) => self.health.on_error(),
        }
    }

    pub fn reconnect(&mut self) -> bool {
        self.transport.reopen().is_ok()
    }

    pub fn snapshot(&self) -> MfcSnapshot {
        MfcSnapshot {
            gas: self.gas,
            connected: self.health.connected,
            consecutive_errors: self.health.consecutive_errors,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{MockFaults, MockTransport};

    struct EmptyAckTransport;

    impl Transport for EmptyAckTransport {
        fn transact(&mut self, _tx: &[u8], _deadline: Duration) -> Result<Vec<u8>, BusError> {
            Ok(Vec::new())
        }

        fn read_line(&mut self, _deadline: Duration) -> Result<Vec<u8>, BusError> {
            Err(BusError::Io("unsupported".into()))
        }

        fn reset_buffers(&mut self) -> Result<(), BusError> {
            Ok(())
        }

        fn reopen(&mut self) -> Result<(), BusError> {
            Ok(())
        }
    }

    fn actor() -> MfcActor {
        MfcActor::new(
            Box::new(MockTransport::mfc()),
            [1, 2, 3, 4],
            [0.1, 1.0, 0.1, 0.1],
            5.0,
            5.0,
        )
    }

    #[test]
    fn set_flow_clamps_and_scales_to_wire() {
        let mut a = actor();
        let clamp = a.set_flow(IDX_H2, 8.0).expect("write ack"); // request 8.0, limit 5.0
        assert_eq!(clamp.applied, 5.0);
        assert!(clamp.clamped);
        assert_eq!(a.gas[IDX_H2].sv, 5.0);
        assert!(a.gas[IDX_H2].on);
        // PV converges to the wire SV (raw 500 -> parse 50.0 -> x0.1 -> 5.0 display).
        for _ in 0..240 {
            a.poll_next(Instant::now());
        }
        assert!(
            (a.gas[IDX_H2].pv - 5.0).abs() < 0.2,
            "pv approaches 5.0: {}",
            a.gas[IDX_H2].pv
        );
    }

    #[test]
    fn zero_setpoint_passes_and_turns_off() {
        let mut a = actor();
        a.set_flow(IDX_H2, 3.0).expect("write ack");
        assert!(a.gas[IDX_H2].on);
        let clamp = a.set_flow(IDX_H2, 0.0).expect("write ack");
        assert_eq!(clamp.applied, 0.0);
        assert!(!clamp.clamped);
        assert!(!a.gas[IDX_H2].on);
    }

    #[test]
    fn set_command_preempts_poll_scan() {
        let mut a = actor();
        a.poll_next(Instant::now()); // polled slave 0, cursor -> 1
                                     // a HIGH-priority set arrives mid-scan and is applied before the next poll
        let clamp = a
            .handle_command(&MfcCommand::SetFlow {
                gas: IDX_CO,
                value: 2.0,
            })
            .expect("write ack")
            .expect("set flow clamp");
        assert_eq!(clamp.applied, 2.0);
        assert_eq!(a.gas[IDX_CO].sv, 2.0, "set applied at transaction boundary");
        a.poll_next(Instant::now()); // scan continues at slave 1
        assert_eq!(a.poll_cursor, 2);
    }

    #[test]
    fn shutoff_flammable_zeroes_h2_co2_co() {
        let mut a = actor();
        for g in [IDX_H2, IDX_CO2, IDX_CO] {
            a.set_flow(g, 3.0).expect("write ack");
        }
        a.handle_command(&MfcCommand::ShutoffFlammable)
            .expect("shutoff ack");
        assert_eq!(a.gas[IDX_H2].sv, 0.0);
        assert_eq!(a.gas[IDX_CO2].sv, 0.0);
        assert_eq!(a.gas[IDX_CO].sv, 0.0);
    }

    #[test]
    fn out_of_range_gas_index_is_rejected_not_panicked() {
        // An untrusted webview could send gas >= 4; it must never panic the bus thread
        // (which would disable all gas control, incl. emergency shutoff — see issue #2).
        let mut a = actor();
        a.set_flow(IDX_H2, 3.0).expect("write ack");
        assert_eq!(a.set_flow(9, 3.0), Err(MfcCommandError::InvalidGas(9)));
        assert_eq!(
            a.handle_command(&MfcCommand::SetOnOff { gas: 7, on: true }),
            Err(MfcCommandError::InvalidGas(7))
        );
        // valid channels are untouched by the out-of-range calls
        assert_eq!(a.gas[IDX_H2].sv, 3.0);
        assert!(a.gas[IDX_H2].on);
    }

    #[test]
    fn offline_after_5_failures() {
        let mut a = MfcActor::new(
            Box::new(MockTransport::mfc().with_faults(MockFaults {
                drop_every: 1,
                ..Default::default()
            })),
            [1, 2, 3, 4],
            [0.1, 1.0, 0.1, 0.1],
            5.0,
            5.0,
        );
        for _ in 0..5 {
            a.poll_next(Instant::now());
        }
        assert!(!a.snapshot().connected);
    }

    #[test]
    fn failed_write_is_reported_and_does_not_update_commanded_state() {
        let mut a = MfcActor::new(
            Box::new(MockTransport::mfc().with_faults(MockFaults {
                drop_every: 1,
                ..Default::default()
            })),
            [1, 2, 3, 4],
            [0.1, 1.0, 0.1, 0.1],
            5.0,
            5.0,
        );

        let result = a.set_flow(IDX_H2, 3.0);

        assert!(result.is_err());
        assert_eq!(a.gas[IDX_H2].sv, 0.0);
        assert!(!a.gas[IDX_H2].on);
        assert_eq!(a.health.consecutive_errors, 1);
    }

    #[test]
    fn empty_write_response_is_not_an_acknowledgement() {
        let mut a = MfcActor::new(
            Box::new(EmptyAckTransport),
            [1, 2, 3, 4],
            [0.1, 1.0, 0.1, 0.1],
            5.0,
            5.0,
        );

        assert!(a.set_flow(IDX_H2, 3.0).is_err());
        assert_eq!(a.gas[IDX_H2].sv, 0.0);
        assert_eq!(a.health.consecutive_errors, 1);
    }

    #[test]
    fn atmosphere_batch_attempts_channels_after_a_failure() {
        let mut a = MfcActor::new(
            Box::new(MockTransport::mfc().with_faults(MockFaults {
                drop_every: 2,
                ..Default::default()
            })),
            [1, 2, 3, 4],
            [0.1, 1.0, 0.1, 0.1],
            5.0,
            5.0,
        );

        assert!(a.set_atmosphere([1.0, 2.0, 3.0, 4.0]).is_err());
        assert_eq!(a.gas[0].sv, 1.0);
        assert_eq!(a.gas[1].sv, 0.0);
        assert_eq!(a.gas[2].sv, 3.0, "channel after failure was attempted");
        assert_eq!(a.gas[3].sv, 0.0);
    }
}
