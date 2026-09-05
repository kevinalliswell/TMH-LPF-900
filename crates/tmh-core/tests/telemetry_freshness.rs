//! Freshness reproductions against 60db688: actors -> telemetry -> sample rows.
//! Connectivity keeps its documented five-error threshold. Data quality must
//! independently identify unavailable, failed, and stale acquisition results.
//! Injected Instant values avoid sleeping and do not modify production state.

use std::time::{Duration, Instant};
use tmh_core::bus::{BalanceActor, MfcActor, TempActor};
use tmh_core::sampler::{build_sample_row, SampleContext};
use tmh_core::storage::SampleRow;
use tmh_core::telemetry::aggregate;
use tmh_core::transport::{MockFaults, MockTransport};

const READ_TIMEOUT: Duration = Duration::from_millis(10);
const MAX_START_AGE: Duration = Duration::from_secs(10);

fn temperature(faults: MockFaults) -> TempActor {
    TempActor::new(
        Box::new(MockTransport::temp().with_faults(faults)),
        0,
        0.1,
        false,
    )
}

fn mfc(faults: MockFaults) -> MfcActor {
    MfcActor::new(
        Box::new(MockTransport::mfc().with_faults(faults)),
        [1, 2, 3, 4],
        [0.1, 1.0, 0.1, 0.1],
        5.0,
        5.0,
    )
}

fn fresh_temperature(now: Instant) -> TempActor {
    let mut temp = temperature(MockFaults::default());
    temp.poll_once(now, READ_TIMEOUT);
    temp
}

fn fresh_mfc(now: Instant) -> MfcActor {
    let mut gas = mfc(MockFaults::default());
    for _ in 0..4 {
        gas.poll_next(now);
    }
    gas
}

fn sample(temp: &TempActor, gas: &MfcActor) -> SampleRow {
    // Balance has a real successful mock read, so it cannot supply an unrelated
    // quality annotation and accidentally satisfy a temperature/MFC assertion.
    let mut balance = BalanceActor::new(Box::new(MockTransport::balance(100.0)));
    balance.poll_once(Instant::now(), READ_TIMEOUT);
    let telemetry = aggregate(1, &temp.snapshot(), &gas.snapshot(), &balance.snapshot());
    build_sample_row(
        &SampleContext {
            experiment_id: "freshness-repro".into(),
            initial_weight: 100.0,
        },
        &telemetry,
        "2026-09-05T12:00:00Z".into(),
        1,
        "RUNNING",
    )
}

fn assert_quality_problem_is_recorded(row: SampleRow, scenario: &str) {
    assert!(
        row.system_message.as_deref().is_some_and(|s| !s.trim().is_empty()),
        "{scenario} persisted as an unannotated ordinary row: temperature={}, flows={:?}, system_message={:?}",
        row.temperature,
        [row.h2_flow, row.n2_flow, row.co2_flow, row.co_flow],
        row.system_message,
    );
}

#[test]
fn successful_recent_reads_produce_an_unflagged_sample() {
    let now = Instant::now();
    let row = sample(&fresh_temperature(now), &fresh_mfc(now));
    assert_eq!(row.temperature, 25.0);
    assert_eq!(row.system_message, None);
}

#[test]
fn never_read_temperature_is_not_recorded_as_an_unflagged_zero() {
    let now = Instant::now();
    let temp = temperature(MockFaults::default());
    assert!(temp.health.is_stale(now, MAX_START_AGE));
    assert!(temp.snapshot().temps[7].is_none());
    assert_quality_problem_is_recorded(sample(&temp, &fresh_mfc(now)), "never-read temperature");
}

#[test]
fn never_read_mfc_is_not_recorded_as_four_unflagged_zero_flows() {
    let now = Instant::now();
    let gas = mfc(MockFaults::default());
    assert!(gas.health.is_stale(now, MAX_START_AGE));
    assert_quality_problem_is_recorded(sample(&fresh_temperature(now), &gas), "never-read MFC");
}

#[test]
fn failed_temperature_poll_is_flagged_before_five_error_disconnect() {
    let now = Instant::now();
    let mut temp = temperature(MockFaults {
        drop_every: 2,
        ..Default::default()
    });
    temp.poll_once(now, READ_TIMEOUT);
    temp.poll_once(now, READ_TIMEOUT);
    assert!(
        temp.snapshot().connected,
        "connectivity should retain its documented threshold"
    );
    assert_eq!(temp.snapshot().consecutive_errors, 1);
    assert_quality_problem_is_recorded(
        sample(&temp, &fresh_mfc(now)),
        "latest temperature poll failed",
    );
}

#[test]
fn failed_mfc_poll_is_flagged_before_five_error_disconnect() {
    let now = Instant::now();
    let mut gas = mfc(MockFaults {
        drop_every: 5,
        ..Default::default()
    });
    for _ in 0..4 {
        gas.poll_next(now); // All channels have a valid baseline reading.
    }
    gas.poll_next(now); // H2 fails while the bus is still connected.
    assert!(
        gas.snapshot().connected,
        "connectivity should retain its documented threshold"
    );
    assert_eq!(gas.snapshot().consecutive_errors, 1);
    assert_quality_problem_is_recorded(
        sample(&fresh_temperature(now), &gas),
        "latest H2 poll failed",
    );
}

#[test]
fn temperature_actor_silent_for_eleven_seconds_is_flagged_without_new_polls() {
    let now = Instant::now();
    let temp = fresh_temperature(now - Duration::from_secs(11));
    assert!(temp.health.is_stale(now, MAX_START_AGE));
    assert_eq!(
        temp.snapshot().temps[7],
        Some(25.0),
        "retain last good display value"
    );
    assert_quality_problem_is_recorded(
        sample(&temp, &fresh_mfc(now)),
        "temperature actor silent for 11 seconds",
    );
}

#[test]
fn mfc_actor_silent_for_eleven_seconds_is_flagged_without_new_polls() {
    let now = Instant::now();
    let gas = fresh_mfc(now - Duration::from_secs(11));
    assert!(gas.health.is_stale(now, MAX_START_AGE));
    assert_quality_problem_is_recorded(
        sample(&fresh_temperature(now), &gas),
        "MFC actor silent for 11 seconds",
    );
}

#[test]
fn successful_mfc_write_cannot_refresh_eleven_second_old_flow_measurements() {
    let now = Instant::now();
    let mut gas = fresh_mfc(now - Duration::from_secs(11));
    assert!(gas.health.is_stale(now, MAX_START_AGE));
    gas.set_flow(0, 3.0)
        .expect("valid mock write acknowledgement");
    // Bus health is now fresh, but no PV measurement was read by the write.
    assert!(!gas.health.is_stale(Instant::now(), MAX_START_AGE));
    assert_eq!(gas.snapshot().gas[0].pv, 0.0);
    assert_quality_problem_is_recorded(
        sample(&fresh_temperature(now), &gas),
        "old MFC PV after fresh write acknowledgement",
    );
}
