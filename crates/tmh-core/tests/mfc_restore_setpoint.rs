//! Regression coverage for #28: ON restores the last acknowledged positive flow.
//! Uses the real MFC actor, CPL codec, and mock device; faults affect only the wire.

use std::sync::{Arc, Mutex};
use std::time::Duration;
use tmh_core::bus::{MfcActor, MfcCommand};
use tmh_core::transport::{BusError, MockTransport, Transport};
use tmh_protocols::cpl;

#[derive(Clone, Copy, Debug)]
enum WriteFailure {
    Timeout,
    InvalidAck,
}

#[derive(Default)]
struct WireControl {
    frames: Vec<Vec<u8>>,
    fail_next: Option<WriteFailure>,
}

struct RecordingTransport {
    inner: MockTransport,
    control: Arc<Mutex<WireControl>>,
}

impl Transport for RecordingTransport {
    fn transact(&mut self, tx: &[u8], deadline: Duration) -> Result<Vec<u8>, BusError> {
        let failure = {
            let mut control = self.control.lock().unwrap();
            control.frames.push(tx.to_vec());
            control.fail_next.take()
        };
        if matches!(failure, Some(WriteFailure::Timeout)) {
            return Err(BusError::Timeout);
        }
        let mut reply = self.inner.transact(tx, deadline)?;
        if matches!(failure, Some(WriteFailure::InvalidAck)) {
            let checksum = reply.iter().position(|b| *b == 0x03).unwrap() + 1;
            reply[checksum] = if reply[checksum] == b'0' { b'1' } else { b'0' };
        }
        Ok(reply)
    }

    fn read_line(&mut self, deadline: Duration) -> Result<Vec<u8>, BusError> {
        self.inner.read_line(deadline)
    }

    fn reset_buffers(&mut self) -> Result<(), BusError> {
        self.inner.reset_buffers()
    }

    fn reopen(&mut self) -> Result<(), BusError> {
        self.inner.reopen()
    }
}

fn actor() -> (MfcActor, Arc<Mutex<WireControl>>) {
    let control = Arc::new(Mutex::new(WireControl::default()));
    let mfc = MfcActor::new(
        Box::new(RecordingTransport {
            inner: MockTransport::mfc(),
            control: control.clone(),
        }),
        [1, 2, 3, 4],
        [0.1, 1.0, 0.1, 0.1],
        5.0,
        5.0,
    );
    (mfc, control)
}

fn frames(control: &Arc<Mutex<WireControl>>) -> Vec<Vec<u8>> {
    control.lock().unwrap().frames.clone()
}

fn assert_commanded_flow(mfc: &MfcActor, gas: usize, value: f64) {
    let reading = mfc.snapshot().gas[gas];
    assert_eq!(reading.sv, value, "gas {gas}: {reading:?}");
    assert_eq!(reading.on, value > 0.0, "gas {gas}: {reading:?}");
}

#[test]
fn on_after_off_restores_acknowledged_flow_on_each_channel() {
    for gas in 0..4 {
        let (mut mfc, control) = actor();
        mfc.handle_command(&MfcCommand::SetFlow { gas, value: 3.0 })
            .unwrap();
        mfc.handle_command(&MfcCommand::SetOnOff { gas, on: false })
            .unwrap();
        assert_commanded_flow(&mfc, gas, 0.0);

        mfc.handle_command(&MfcCommand::SetOnOff { gas, on: true })
            .expect("ON restores the previously acknowledged positive setpoint");

        assert_commanded_flow(&mfc, gas, 3.0);
        let addr = gas as u8 + 1;
        let raw = if gas == 1 { 30 } else { 300 };
        assert_eq!(
            frames(&control),
            vec![
                cpl::build_write(addr, 1401, &[raw]),
                cpl::build_write(addr, 1401, &[0]),
                cpl::build_write(addr, 1401, &[raw]),
            ],
            "gas {gas}: restored flow must reach the device with channel scaling"
        );
    }
}

#[test]
fn on_and_off_return_the_acknowledged_applied_value_for_audit() {
    let (mut mfc, _) = actor();
    mfc.set_flow(0, 8.0).unwrap();
    let off = mfc
        .handle_command(&MfcCommand::SetOnOff { gas: 0, on: false })
        .unwrap()
        .expect("OFF acknowledgement must carry its applied value");
    assert_eq!(off.applied, 0.0);
    let on = mfc
        .handle_command(&MfcCommand::SetOnOff { gas: 0, on: true })
        .unwrap()
        .expect("ON acknowledgement must carry its restored value");
    assert_eq!(on.applied, 5.0);
}

#[test]
fn cold_start_on_is_rejected_without_sending_a_write() {
    let (mut mfc, control) = actor();
    for gas in 0..4 {
        let result = mfc.handle_command(&MfcCommand::SetOnOff { gas, on: true });
        assert!(
            result.is_err(),
            "gas {gas}: no flow is available to restore"
        );
        assert_commanded_flow(&mfc, gas, 0.0);
    }
    assert!(
        frames(&control).is_empty(),
        "rejected ON must not reach wire"
    );
}

#[test]
fn zero_only_setpoint_does_not_make_on_restorable() {
    for gas in 0..4 {
        let (mut mfc, control) = actor();
        mfc.handle_command(&MfcCommand::SetFlow { gas, value: 0.0 })
            .unwrap();
        let before = frames(&control);

        let result = mfc.handle_command(&MfcCommand::SetOnOff { gas, on: true });

        assert!(result.is_err(), "gas {gas}: zero is not a flow to restore");
        assert_commanded_flow(&mfc, gas, 0.0);
        assert_eq!(frames(&control), before, "rejected ON must not reach wire");
    }
}

#[test]
fn positive_request_rounded_to_wire_zero_does_not_authorize_on() {
    let (mut mfc, control) = actor();
    mfc.handle_command(&MfcCommand::SetFlow {
        gas: 0,
        value: 0.001,
    })
    .unwrap();
    // H2 scaling is 0.1: round(0.001 / 0.1 * 10) = 0 on the wire.
    let before = frames(&control);
    assert_eq!(before, vec![cpl::build_write(1, 1401, &[0])]);
    let reading_before = mfc.snapshot().gas[0];

    let result = mfc.handle_command(&MfcCommand::SetOnOff { gas: 0, on: true });

    assert!(
        result.is_err(),
        "only a zero wire setpoint was acknowledged"
    );
    assert_eq!(frames(&control), before, "rejected ON must not reach wire");
    assert_eq!(mfc.snapshot().gas[0], reading_before);
}

#[test]
fn on_restores_the_applied_safety_clamped_flow() {
    for gas in [0, 3] {
        let (mut mfc, control) = actor();
        let clamp = mfc
            .handle_command(&MfcCommand::SetFlow { gas, value: 8.0 })
            .unwrap()
            .unwrap();
        assert_eq!(clamp.applied, 5.0);
        mfc.handle_command(&MfcCommand::SetOnOff { gas, on: false })
            .unwrap();

        mfc.handle_command(&MfcCommand::SetOnOff { gas, on: true })
            .unwrap();

        assert_commanded_flow(&mfc, gas, 5.0);
        assert_eq!(
            frames(&control).last(),
            Some(&cpl::build_write(gas as u8 + 1, 1401, &[500]))
        );
    }
}

#[test]
fn failed_setpoint_does_not_replace_a_previous_acknowledged_restore_value() {
    for failure in [WriteFailure::Timeout, WriteFailure::InvalidAck] {
        let (mut mfc, control) = actor();
        mfc.handle_command(&MfcCommand::SetFlow { gas: 0, value: 3.0 })
            .unwrap();
        mfc.handle_command(&MfcCommand::SetOnOff { gas: 0, on: false })
            .unwrap();
        control.lock().unwrap().fail_next = Some(failure);
        assert!(mfc
            .handle_command(&MfcCommand::SetFlow { gas: 0, value: 4.0 })
            .is_err());
        assert_commanded_flow(&mfc, 0, 0.0);

        mfc.handle_command(&MfcCommand::SetOnOff { gas: 0, on: true })
            .expect("a failed setpoint must preserve the earlier acknowledged flow");

        assert_commanded_flow(&mfc, 0, 3.0);
        assert_eq!(
            frames(&control).last(),
            Some(&cpl::build_write(1, 1401, &[300])),
            "{failure:?}: ON must restore the successful 3 L/min write"
        );
    }
}

#[test]
fn first_failed_setpoint_does_not_authorize_on() {
    for failure in [WriteFailure::Timeout, WriteFailure::InvalidAck] {
        let (mut mfc, control) = actor();
        control.lock().unwrap().fail_next = Some(failure);
        assert!(mfc
            .handle_command(&MfcCommand::SetFlow { gas: 0, value: 3.0 })
            .is_err());
        let before = frames(&control);

        let result = mfc.handle_command(&MfcCommand::SetOnOff { gas: 0, on: true });

        assert!(result.is_err(), "{failure:?}: no write was acknowledged");
        assert_commanded_flow(&mfc, 0, 0.0);
        assert_eq!(frames(&control), before, "rejected ON must not reach wire");
    }
}

#[test]
fn safety_latch_blocks_on_even_with_an_acknowledged_restore_value() {
    let (mut mfc, control) = actor();
    for gas in 0..4 {
        mfc.handle_command(&MfcCommand::SetFlow { gas, value: 3.0 })
            .unwrap();
    }
    mfc.handle_command(&MfcCommand::ShutoffFlammable).unwrap();
    assert!(mfc.snapshot().safety_latched);
    let before = frames(&control);

    for gas in 0..4 {
        let result = mfc.handle_command(&MfcCommand::SetOnOff { gas, on: true });
        assert!(result.is_err(), "gas {gas}: protective latch must block ON");
        assert_commanded_flow(&mfc, gas, if gas == 1 { 5.0 } else { 0.0 });
    }

    assert_eq!(frames(&control), before, "latched ON must not reach wire");
    assert!(mfc.snapshot().safety_latched);
}
