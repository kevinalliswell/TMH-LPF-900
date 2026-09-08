//! On-disk final-commit regressions; original three-call adapter failed seven cases.
//! Faults and state assertions are preserved; adapter now calls the production transaction.

use rusqlite::Connection;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tmh_core::storage::{
    ExperimentRow, Result as StorageResult, RunCompletion, SampleRow, Storage,
};

const RUN_ID: &str = "run-1";
const END: &str = "2026-09-05T10:10:00Z";
const LATER_END: &str = "2026-09-05T10:11:00Z";
const COMPLETE: &str = r#"{"completion_token":"final-1","outcome":"completed","data_complete":true,"safety_confirmed":true}"#;

const FAIL_FINISH: &str = r#"
CREATE TRIGGER fail_finish BEFORE UPDATE OF end_time ON experiments
WHEN NEW.experiment_id = 'run-1' AND NEW.end_time IS NOT NULL
BEGIN SELECT RAISE(ABORT, 'injected finish failure'); END;
"#;

const FAIL_METADATA: &str = r#"
CREATE TRIGGER fail_metadata BEFORE UPDATE OF runtime_metadata ON experiments
WHEN json_extract(NEW.runtime_metadata, '$.outcome') = 'completed'
BEGIN SELECT RAISE(ABORT, 'injected metadata failure'); END;
"#;

const FAIL_SECOND_TAIL_ROW: &str = r#"
CREATE TRIGGER fail_tail BEFORE INSERT ON experiment_data
WHEN NEW.timestamp = '2026-09-05T10:00:02Z'
BEGIN SELECT RAISE(ABORT, 'injected tail-row failure'); END;
"#;

struct TestDb {
    dir: PathBuf,
    path: PathBuf,
}

impl TestDb {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("tmh_completion_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("experiments.db");
        let mut db = Storage::open(&path).unwrap();
        db.insert_experiment(&ExperimentRow {
            sample_id: Some("sample-1".into()),
            project_name: None,
            experiment_time: None,
            program_snapshot_json: None,
            experiment_id: RUN_ID.into(),
            experiment_name: "Completion fault injection".into(),
            sample_name: "Pellet sample".into(),
            sample_weight: 500.0,
            start_time: "2026-09-05T10:00:00Z".into(),
            end_time: None,
            description: None,
            operator: Some("test-operator".into()),
            experiment_type: Some("REDUCIBILITY".into()),
            analysis_results_json: None,
            created_at: "2026-09-05T10:00:00Z".into(),
            owner_user_id: None,
        })
        .unwrap();
        db.patch_runtime_metadata(
            RUN_ID,
            r#"{"outcome":"running","data_complete":false,"source":"mock"}"#,
        )
        .unwrap();
        db.insert_samples(&[sample(0)]).unwrap(); // Previously committed samples must survive.
        Self { dir, path }
    }

    fn inject(&self, sql: &str) {
        // A second real SQLite connection permits fault injection without adding
        // a production API exposing Storage's private connection.
        Connection::open(&self.path)
            .unwrap()
            .execute_batch(sql)
            .unwrap();
    }

    fn open(&self) -> Storage {
        Storage::open(&self.path).unwrap()
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn sample(seq: u32) -> SampleRow {
    SampleRow {
        record_json: Some(serde_json::json!({"seq":seq,"measurement_source":"mock"}).to_string()),
        experiment_id: RUN_ID.into(),
        timestamp: format!("2026-09-05T10:00:{seq:02}Z"),
        experiment_duration: Some(format!("00:00:{seq:02}")),
        temperature: 900.0,
        weight: 500.0 - seq as f64,
        weight_loss: seq as f64,
        co_flow: 4.5,
        co2_flow: 0.0,
        n2_flow: 10.5,
        h2_flow: 0.0,
        experiment_status: Some("REDUCING".into()),
        system_message: None,
    }
}

fn tail() -> Vec<SampleRow> {
    vec![sample(1), sample(2)]
}

fn current_app_completion(db: &mut Storage, rows: &[SampleRow], end: &str) -> StorageResult<()> {
    db.commit_completion(&RunCompletion {
        token: "final-1".into(),
        experiment_id: RUN_ID.into(),
        end_time: end.into(),
        metadata: serde_json::from_str(COMPLETE).unwrap(),
        rows: rows.to_vec(),
    })
}

fn metadata(db: &Storage) -> Value {
    serde_json::from_str(&db.runtime_metadata(RUN_ID).unwrap()).unwrap()
}

fn reread(path: &Path) -> (i64, Option<String>, Value) {
    let db = Storage::open(path).unwrap();
    (
        db.count_samples(RUN_ID).unwrap(),
        db.get_experiment(RUN_ID).unwrap().unwrap().end_time,
        metadata(&db),
    )
}

#[test]
fn successful_final_commit_survives_reopen_with_all_three_components() {
    let fixture = TestDb::new();
    {
        let mut db = fixture.open();
        current_app_completion(&mut db, &tail(), END).unwrap();
    }
    let (count, end, meta) = reread(&fixture.path);
    assert_eq!(count, 3);
    assert_eq!(end.as_deref(), Some(END));
    assert_eq!(meta["outcome"], "completed");
    assert_eq!(meta["data_complete"], true);
    assert_eq!(meta["source"], "mock"); // Completion must preserve prior runtime evidence.
    assert!(fixture.open().unfinished_experiments().unwrap().is_empty());
}

#[test]
fn failed_tail_insert_is_atomic_and_retained_payload_is_not_marked_complete() {
    let fixture = TestDb::new();
    fixture.inject(FAIL_SECOND_TAIL_ROW);
    let retryable_tail = tail();
    {
        let mut db = fixture.open();
        let error = current_app_completion(&mut db, &retryable_tail, END).unwrap_err();
        assert!(error.to_string().contains("injected tail-row failure"));
    }
    let (count, end, meta) = reread(&fixture.path);
    assert_eq!(count, 1, "even the first tail row must roll back");
    assert_eq!(end, None);
    assert_eq!(meta["data_complete"], false);
    assert_eq!(retryable_tail.len(), 2);
    // This control retains the payload in the test caller. The app's lifetime
    // problem (its local buffer is dropped on return) is separate source evidence.
    fixture.inject("DROP TRIGGER fail_tail;");
    current_app_completion(&mut fixture.open(), &retryable_tail, END).unwrap();
    assert_eq!(reread(&fixture.path).0, 3);
}

#[test]
fn finish_update_failure_rolls_back_final_samples_as_well_as_end_time() {
    let fixture = TestDb::new();
    fixture.inject(FAIL_FINISH);
    {
        let error = current_app_completion(&mut fixture.open(), &tail(), END).unwrap_err();
        assert!(error.to_string().contains("injected finish failure"));
    }
    let (count, end, meta) = reread(&fixture.path);
    assert_eq!(end, None);
    assert_eq!(
        count, 1,
        "failed final commit left tail rows on disk; metadata={meta}"
    );
}

#[test]
fn finish_update_failure_cannot_persist_a_completed_outcome_without_end_time() {
    let fixture = TestDb::new();
    fixture.inject(FAIL_FINISH);
    let retryable_tail = tail();
    assert!(current_app_completion(&mut fixture.open(), &retryable_tail, END).is_err());
    let (count, end, meta) = reread(&fixture.path);
    assert_eq!(end, None);
    assert_eq!(
        meta["outcome"], "running",
        "pending finalization is falsely labelled completed: samples={count}, end={end:?}, metadata={meta}"
    );
    assert_eq!(meta["data_complete"], false);
}

#[test]
fn metadata_update_failure_rolls_back_the_final_sample_batch() {
    let fixture = TestDb::new();
    fixture.inject(FAIL_METADATA);
    let error = current_app_completion(&mut fixture.open(), &tail(), END).unwrap_err();
    assert!(error.to_string().contains("injected metadata failure"));
    let (count, end, meta) = reread(&fixture.path);
    assert_eq!(end, None);
    assert_eq!(meta["outcome"], "running");
    assert_eq!(
        count, 1,
        "sample tail committed even though terminal metadata did not"
    );
}

#[test]
fn replaying_the_same_successful_completion_is_idempotent() {
    let fixture = TestDb::new();
    let payload = tail();
    current_app_completion(&mut fixture.open(), &payload, END).unwrap();
    // Simulate a lost completion acknowledgement: reopen and replay the same token/payload.
    current_app_completion(&mut fixture.open(), &payload, END).unwrap();
    let (count, end, meta) = reread(&fixture.path);
    assert_eq!(end.as_deref(), Some(END));
    assert_eq!(meta["completion_token"], "final-1");
    assert_eq!(count, 3, "same completion inserted duplicate tail samples");
}

#[test]
fn retry_after_finish_failure_does_not_duplicate_a_partially_committed_tail() {
    let fixture = TestDb::new();
    let payload = tail();
    fixture.inject(FAIL_FINISH);
    assert!(current_app_completion(&mut fixture.open(), &payload, END).is_err());
    fixture.inject("DROP TRIGGER fail_finish;");
    current_app_completion(&mut fixture.open(), &payload, END).unwrap();
    let (count, end, meta) = reread(&fixture.path);
    assert_eq!(end.as_deref(), Some(END));
    assert_eq!(meta["outcome"], "completed");
    assert_eq!(
        count, 3,
        "retry of one failed completion duplicated its tail rows"
    );
}

#[test]
fn replay_with_the_same_token_cannot_rewrite_original_end_time() {
    let fixture = TestDb::new();
    let payload = tail();
    current_app_completion(&mut fixture.open(), &payload, END).unwrap();
    // Current finalize regenerates iso_now() on each call. A token replay may
    // reject a changed payload or return the original receipt, but cannot mutate it.
    let replay = current_app_completion(&mut fixture.open(), &payload, LATER_END);
    let (_, end, _) = reread(&fixture.path);
    assert_eq!(
        end.as_deref(),
        Some(END),
        "completion replay changed historical end time: {replay:?}"
    );
}

#[test]
fn completion_of_an_unknown_experiment_cannot_report_success() {
    let fixture = TestDb::new();
    fixture.open().delete_experiment(RUN_ID).unwrap();
    let result = current_app_completion(&mut fixture.open(), &[], END);
    assert!(
        result.is_err(),
        "all terminal updates matched zero experiments, but returned {result:?}"
    );
}

#[test]
fn failed_experiment_delete_preserves_samples_revisions_and_completion_receipt() {
    let fixture = TestDb::new();
    current_app_completion(&mut fixture.open(), &tail(), END).unwrap();
    fixture
        .open()
        .set_analysis_results(RUN_ID, "{\"test\":true}")
        .unwrap();
    fixture.inject("CREATE TRIGGER fail_delete BEFORE DELETE ON experiments BEGIN SELECT RAISE(ABORT, 'delete failure'); END;");
    assert!(fixture.open().delete_experiment(RUN_ID).is_err());
    assert_eq!(reread(&fixture.path).0, 3);
    assert_eq!(fixture.open().analysis_revisions(RUN_ID).unwrap().len(), 1);
    // If the receipt was deleted, retry would fail as an already-ended run.
    current_app_completion(&mut fixture.open(), &tail(), END).unwrap();
}
