//! Reproductions for transaction acknowledgement validation on main cbed665.
//! Real MfcActor and CPL codec; deterministic transport boundary; no hardware.

use std::collections::VecDeque;
use std::time::Duration;
use tmh_core::bus::MfcActor;
use tmh_core::transport::{BusError, Transport};
use tmh_protocols::cpl;

/// Responses that were already queued before the command was transmitted.
/// The device deliberately provides no new acknowledgement for that command.
struct PendingInputTransport {
    pending: VecDeque<Vec<u8>>,
}

impl Transport for PendingInputTransport {
    fn transact(&mut self, _tx: &[u8], _deadline: Duration) -> Result<Vec<u8>, BusError> {
        self.pending.pop_front().ok_or(BusError::Timeout)
    }

    fn read_line(&mut self, _deadline: Duration) -> Result<Vec<u8>, BusError> {
        Err(BusError::Timeout)
    }

    fn reset_buffers(&mut self) -> Result<(), BusError> {
        self.pending.clear();
        Ok(())
    }

    fn reopen(&mut self) -> Result<(), BusError> {
        Ok(())
    }
}

/// Sends the exact canned bytes as the response to a new request. This makes
/// checksum/address/content faults independent from device simulation behavior.
struct ReplyTransport(Vec<u8>);

impl Transport for ReplyTransport {
    fn transact(&mut self, _tx: &[u8], _deadline: Duration) -> Result<Vec<u8>, BusError> {
        Ok(self.0.clone())
    }

    fn read_line(&mut self, _deadline: Duration) -> Result<Vec<u8>, BusError> {
        Err(BusError::Timeout)
    }

    fn reset_buffers(&mut self) -> Result<(), BusError> {
        Ok(())
    }

    fn reopen(&mut self) -> Result<(), BusError> {
        Ok(())
    }
}

fn response(addr: u8, value: &str) -> Vec<u8> {
    let mut bytes = format!("\x02{addr:02X}00X20,{value}\x03").into_bytes();
    bytes.extend_from_slice(&cpl::cpl_checksum(&bytes));
    bytes.extend_from_slice(b"\r\n");
    bytes
}

fn actor(transport: impl Transport + 'static) -> MfcActor {
    MfcActor::new(
        Box::new(transport),
        [1, 2, 3, 4],
        [0.1, 1.0, 0.1, 0.1],
        5.0,
        5.0,
    )
}

fn assert_rejected_without_committing_state(mut mfc: MfcActor) {
    let result = mfc.set_flow(0, 3.0); // H2 slave 1, raw 300.
    let snapshot = mfc.snapshot();
    assert!(
        result.is_err(),
        "invalid reply acknowledged as {result:?}; state became {snapshot:?}"
    );
    assert_eq!(snapshot.gas[0].sv, 0.0);
    assert!(!snapshot.gas[0].on);
    assert_eq!(snapshot.consecutive_errors, 1);
}

#[test]
fn matching_valid_acknowledgement_commits_commanded_state() {
    let mut mfc = actor(ReplyTransport(response(1, "300")));
    assert_eq!(mfc.set_flow(0, 3.0).unwrap().applied, 3.0);
    assert_eq!(mfc.snapshot().gas[0].sv, 3.0);
    assert!(mfc.snapshot().gas[0].on);
}

#[test]
fn mismatched_value_is_rejected() {
    assert_rejected_without_committing_state(actor(ReplyTransport(response(1, "299"))));
}

#[test]
fn raw_write_request_echo_alone_is_rejected() {
    // An adapter's raw local echo is not a device acknowledgement.
    let echo = cpl::build_write(1, 1401, &[300]);
    assert_rejected_without_committing_state(actor(ReplyTransport(echo)));
}

#[test]
fn corrupted_checksum_is_rejected() {
    let mut reply = response(1, "300");
    let checksum_start = reply.iter().position(|b| *b == 0x03).unwrap() + 1;
    reply[checksum_start] = if reply[checksum_start] == b'0' {
        b'1'
    } else {
        b'0'
    };
    assert_rejected_without_committing_state(actor(ReplyTransport(reply)));
}

#[test]
fn different_slave_with_matching_value_is_rejected() {
    // A delayed CO response (slave 4) must not confirm a new H2 write (slave 1).
    assert_rejected_without_committing_state(actor(ReplyTransport(response(4, "300"))));
}

#[test]
fn incomplete_frame_without_etx_or_checksum_is_rejected() {
    assert_rejected_without_committing_state(actor(ReplyTransport(b"\x020100X20,300".to_vec())));
}

#[test]
fn nonfinite_numeric_value_is_rejected() {
    assert_rejected_without_committing_state(actor(ReplyTransport(response(1, "NaN"))));
}

#[test]
fn buffered_response_from_previous_transaction_cannot_acknowledge_new_command() {
    // docs/04 requires clearing bus input/output buffers before each transaction.
    // A stale but otherwise valid reply cannot prove this new write succeeded.
    let pending = VecDeque::from([response(1, "300")]);
    assert_rejected_without_committing_state(actor(PendingInputTransport { pending }));
}

#[test]
fn request_echo_followed_by_valid_acknowledgement_is_accepted() {
    let mut echo_and_reply = cpl::build_write(1, 1401, &[300]);
    echo_and_reply.extend_from_slice(&response(1, "300"));
    let mut mfc = actor(ReplyTransport(echo_and_reply));
    let result = mfc.set_flow(0, 3.0);
    assert!(
        result.is_ok(),
        "valid device reply after local echo rejected: {result:?}"
    );
    assert_eq!(mfc.snapshot().gas[0].sv, 3.0);
}

#[test]
fn safety_atmosphere_reports_failure_for_nonfinite_acknowledgements() {
    let mut mfc = actor(ReplyTransport(response(1, "NaN")));
    let result = mfc.set_atmosphere([0.0, 5.0, 0.0, 0.0]);
    assert!(
        result.is_err(),
        "safety atmosphere incorrectly confirmed by NaN replies: {result:?}"
    );
}
