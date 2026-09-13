//! Setpoint results and snapshots must describe the acknowledged CPL wire value.
//! Real actor and mock device; the transport wrapper only records requests.

use std::sync::{Arc, Mutex};
use std::time::Duration;
use tmh_core::bus::MfcActor;
use tmh_core::transport::{BusError, MockTransport, Transport};
use tmh_protocols::cpl;

type WireFrames = Arc<Mutex<Vec<Vec<u8>>>>;

struct RecordingTransport {
    inner: MockTransport,
    frames: WireFrames,
}

impl Transport for RecordingTransport {
    fn transact(&mut self, tx: &[u8], deadline: Duration) -> Result<Vec<u8>, BusError> {
        self.frames.lock().unwrap().push(tx.to_vec());
        self.inner.transact(tx, deadline)
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

fn actor(h2_limit: f64) -> (MfcActor, WireFrames) {
    let frames = Arc::new(Mutex::new(Vec::new()));
    let mfc = MfcActor::new(
        Box::new(RecordingTransport {
            inner: MockTransport::mfc(),
            frames: frames.clone(),
        }),
        [1, 2, 3, 4],
        [0.1, 1.0, 0.1, 0.1],
        h2_limit,
        5.0,
    );
    (mfc, frames)
}

fn assert_confirmed_flow(mfc: &MfcActor, returned: f64, expected: f64) {
    let reading = mfc.snapshot().gas[0];
    assert!(
        (returned - expected).abs() < 1e-12,
        "returned {returned}, expected acknowledged flow {expected}"
    );
    assert!(
        (reading.sv - expected).abs() < 1e-12,
        "snapshot {reading:?}, expected acknowledged flow {expected}"
    );
    assert_eq!(reading.on, expected > 0.0);
}

#[test]
fn request_below_wire_resolution_reports_acknowledged_zero_and_off() {
    let (mut mfc, frames) = actor(5.0);

    let result = mfc.set_flow(0, 0.001).unwrap();

    assert_eq!(
        *frames.lock().unwrap(),
        vec![cpl::build_write(1, 1401, &[0])]
    );
    assert_eq!(result.applied, 0.0);
    let reading = mfc.snapshot().gas[0];
    assert_eq!(reading.sv, 0.0);
    assert!(!reading.on, "a zero wire value must be shown as OFF");
}

#[test]
fn rounding_never_sends_a_wire_value_above_the_safety_limit() {
    let limit = 0.015;
    for requested in [limit, 3.0] {
        let (mut mfc, frames) = actor(limit);

        let result = mfc.set_flow(0, requested).unwrap();

        // Raw 2 would command 0.02 L/min, exceeding the configured 0.015 limit.
        assert_eq!(
            *frames.lock().unwrap(),
            vec![cpl::build_write(1, 1401, &[1])],
            "requested {requested}: select the highest safe wire setpoint"
        );
        assert!(result.applied <= limit, "returned value exceeds limit");
        assert!(mfc.snapshot().gas[0].sv <= limit, "snapshot exceeds limit");
        assert_confirmed_flow(&mfc, result.applied, 0.01);
    }
}

#[test]
fn ordinary_rounding_is_reported_and_restored_after_off() {
    let (mut mfc, frames) = actor(5.0);

    let set = mfc.set_flow(0, 1.234).unwrap();
    assert_confirmed_flow(&mfc, set.applied, 1.23);
    let off = mfc.set_on_off(0, false).unwrap();
    assert_confirmed_flow(&mfc, off.applied, 0.0);
    let on = mfc.set_on_off(0, true).unwrap();

    assert_confirmed_flow(&mfc, on.applied, 1.23);
    assert_eq!(
        *frames.lock().unwrap(),
        vec![
            cpl::build_write(1, 1401, &[123]),
            cpl::build_write(1, 1401, &[0]),
            cpl::build_write(1, 1401, &[123]),
        ]
    );
}

#[test]
fn representable_decimal_limits_keep_their_full_wire_resolution() {
    for (limit, raw) in [(0.3, 30), (1.5, 150)] {
        for requested in [limit, limit + 1.0] {
            let (mut mfc, frames) = actor(limit);

            let result = mfc.set_flow(0, requested).unwrap();

            assert_eq!(
                *frames.lock().unwrap(),
                vec![cpl::build_write(1, 1401, &[raw])],
                "limit {limit}, requested {requested}: avoid losing a wire unit to float rounding"
            );
            assert_confirmed_flow(&mfc, result.applied, limit);
        }
    }
}

#[test]
fn unrepresentable_finite_setpoint_is_rejected_without_blocking_zero_shutoff() {
    let (mut mfc, frames) = actor(5.0);
    mfc.set_flow(1, 3.0).unwrap();
    let gas_before = mfc.snapshot().gas;
    let frames_before = frames.lock().unwrap().clone();

    // N2 is not safety-clamped. This finite request needs a raw value above i64::MAX.
    let result = mfc.set_flow(1, 1e308);

    assert!(result.is_err(), "unrepresentable flow must be rejected");
    assert_eq!(
        *frames.lock().unwrap(),
        frames_before,
        "an unrepresentable setpoint must not reach the wire"
    );
    assert_eq!(mfc.snapshot().gas, gas_before);

    let zero = mfc
        .set_flow(1, 0.0)
        .expect("zero shutoff remains available");

    assert_eq!(zero.applied, 0.0);
    assert_eq!(mfc.snapshot().gas[1].sv, 0.0);
    assert!(!mfc.snapshot().gas[1].on);
    assert_eq!(
        frames.lock().unwrap().last(),
        Some(&cpl::build_write(2, 1401, &[0]))
    );
}
