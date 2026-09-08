//! Independent F09 tests for finite-number preservation at the journal boundary.
//! This test-only commit targets the pending production RunCompletion/recovery API.
//! The decimal below comes from serde_json's own test_roundtrip_f64 regression
//! vectors; it is an ordinary finite measurement, not NaN or out-of-range JSON.

use std::path::PathBuf;
use tmh_core::bus::GasReading;
use tmh_core::recovery;
use tmh_core::sampler::{build_sample_row, SampleContext};
use tmh_core::storage::{ExperimentRow, RunCompletion, SampleRow, Storage};
use tmh_core::telemetry::{DeviceHealth, TelemetrySnapshot};

struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir =
            std::env::temp_dir().join(format!("tmh_numeric_recovery_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        Self { dir }
    }

    fn open(&self) -> Storage {
        Storage::open(&self.dir.join("experiment.db")).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn completion() -> RunCompletion {
    RunCompletion {
        token: "numeric-completion".into(),
        experiment_id: "numeric-run".into(),
        end_time: "2026-09-08T02:00:00Z".into(),
        metadata: serde_json::json!({"outcome":"completed","data_complete":true}),
        rows: vec![SampleRow {
            experiment_id: "numeric-run".into(),
            timestamp: "2026-09-08T01:59:59Z".into(),
            experiment_duration: None,
            temperature: 900.0,
            weight: 51.248178375505404,
            weight_loss: 48.751821624494596,
            co_flow: 4.5,
            co2_flow: 0.0,
            n2_flow: 10.5,
            h2_flow: 0.0,
            experiment_status: Some("REDUCING".into()),
            system_message: None,
            record_json: None,
        }],
    }
}

#[test]
fn journal_roundtrip_preserves_the_exact_pending_payload() {
    let fixture = Fixture::new();
    let completion = completion();
    recovery::persist(&fixture.dir, &completion).unwrap();
    let scanned = recovery::scan(&fixture.dir);
    assert!(scanned.errors.is_empty(), "{:?}", scanned.errors);
    assert_eq!(scanned.pending.len(), 1);
    assert_eq!(
        serde_json::to_vec(&scanned.pending[0].1).unwrap(),
        serde_json::to_vec(&completion).unwrap(),
        "an untouched journal must not conflict with its in-memory pending payload"
    );
}

#[test]
fn lost_success_ack_replays_finite_measurements_without_a_false_token_conflict() {
    let fixture = Fixture::new();
    let completion = completion();
    let mut database = fixture.open();
    database
        .insert_experiment(&ExperimentRow {
            experiment_id: completion.experiment_id.clone(),
            experiment_name: "Numeric recovery".into(),
            sample_id: None,
            sample_name: "sample".into(),
            sample_weight: 100.0,
            start_time: "2026-09-08T01:00:00Z".into(),
            experiment_time: None,
            end_time: None,
            project_name: None,
            description: None,
            operator: None,
            experiment_type: Some("REDUCIBILITY".into()),
            analysis_results_json: None,
            program_snapshot_json: None,
            created_at: "2026-09-08T01:00:00Z".into(),
            owner_user_id: None,
        })
        .unwrap();
    let journal = fixture.dir.join("pending");
    recovery::persist(&journal, &completion).unwrap();
    database.commit_completion(&completion).unwrap();
    drop(database); // A crash occurs after SQLite commits but before journal removal.

    let mut reopened = fixture.open();
    let scanned = recovery::scan(&journal);
    assert!(scanned.errors.is_empty());
    assert_eq!(scanned.pending.len(), 1);
    reopened
        .commit_completion(&scanned.pending[0].1)
        .expect("the persisted receipt must accept its original journal on restart");
    assert_eq!(
        reopened.count_samples(&completion.experiment_id).unwrap(),
        1
    );
}

#[test]
fn finite_input_subtraction_cannot_create_an_unrecoverable_sample_row() {
    let telemetry = TelemetrySnapshot {
        seq: 1,
        temps: [Some(900.0); 9],
        gas: [GasReading {
            pv: 0.0,
            sv: 0.0,
            on: false,
        }; 4],
        weight: Some(-f64::MAX),
        health: DeviceHealth {
            safety_latched: false,
            temp_valid: true,
            mfc_valid: true,
            balance_valid: true,
            temp_connected: true,
            mfc_connected: true,
            balance_connected: true,
        },
    };
    let row = build_sample_row(
        &SampleContext {
            experiment_id: "overflow-run".into(),
            initial_weight: f64::MAX,
        },
        &telemetry,
        "2026-09-08T01:00:00Z".into(),
        1,
        "REDUCING",
    );
    assert!(
        row.weight_loss.is_finite(),
        "validating the operands alone does not make their subtraction finite"
    );
    let json = serde_json::to_string(&row).unwrap();
    assert!(
        serde_json::from_str::<SampleRow>(&json).is_ok(),
        "the final sample must remain readable after journal serialization"
    );
}
