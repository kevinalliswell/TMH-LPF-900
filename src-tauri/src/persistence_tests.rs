use super::*;

fn fixture() -> (PathBuf, Storage, RunCompletion) {
    let dir = std::env::temp_dir().join(format!("tmh_recovery_{}", tmh_core::auth::new_user_id()));
    std::fs::create_dir_all(&dir).unwrap();
    let db = Storage::open(&dir.join("experiment.db")).unwrap();
    db.insert_experiment(&ExperimentRow {
        experiment_id: "run-1".into(),
        experiment_name: "recovery".into(),
        sample_name: "sample".into(),
        sample_weight: 500.0,
        start_time: "2026-09-08T01:00:00Z".into(),
        end_time: None,
        description: None,
        operator: None,
        experiment_type: Some("REDUCIBILITY".into()),
        analysis_results_json: None,
        created_at: "2026-09-08T01:00:00Z".into(),
        owner_user_id: None,
        sample_id: None,
        project_name: None,
        experiment_time: None,
        program_snapshot_json: None,
    })
    .unwrap();
    let completion = RunCompletion {
        token: "test-completion".into(),
        experiment_id: "run-1".into(),
        end_time: "2026-09-08T01:10:00Z".into(),
        metadata: serde_json::json!({"outcome":"completed","data_complete":true}),
        rows: vec![SampleRow {
            experiment_id: "run-1".into(),
            timestamp: "2026-09-08T01:09:59Z".into(),
            record_json: Some("{\"sample\":true}".into()),
            experiment_duration: None,
            temperature: 900.0,
            weight: 490.0,
            weight_loss: 10.0,
            h2_flow: 0.0,
            n2_flow: 10.5,
            co2_flow: 0.0,
            co_flow: 4.5,
            experiment_status: Some("REDUCING".into()),
            system_message: None,
        }],
    };
    (dir, db, completion)
}

#[test]
fn pending_file_recovers_after_restart_and_a_lost_success_ack_does_not_duplicate() {
    let (dir, mut db, completion) = fixture();
    let journal = dir.join("pending");
    recovery::persist(&journal, &completion).unwrap();
    db.commit_completion(&completion).unwrap(); // Crash before removing the journal.
    drop(db);
    let mut reopened = Storage::open(&dir.join("experiment.db")).unwrap();
    let exp = ExpControl::new();
    retry_pending_completions(&mut reopened, &exp, &journal).unwrap();
    assert!(!exp.persistence_pending());
    assert_eq!(reopened.count_samples("run-1").unwrap(), 1);
    assert_eq!(
        reopened.get_experiment("run-1").unwrap().unwrap().end_time,
        Some(completion.end_time)
    );
    assert!(recovery::scan(&journal).pending.is_empty());
    drop(reopened);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn database_failure_retains_memory_and_journal_until_successful_retry() {
    let (dir, mut db, completion) = fixture();
    let journal = dir.join("pending");
    recovery::persist(&journal, &completion).unwrap();
    let exp = ExpControl::new();
    let mut unavailable = Storage::open_in_memory().unwrap(); // Missing run rejects transaction.
    assert!(retry_pending_completions(&mut unavailable, &exp, &journal).is_err());
    assert!(exp.snapshot().persistence_pending);
    assert_eq!(lock(&exp.pending).len(), 1);
    assert_eq!(recovery::scan(&journal).pending.len(), 1);
    retry_pending_completions(&mut db, &exp, &journal).unwrap();
    assert!(!exp.snapshot().persistence_pending);
    assert_eq!(db.count_samples("run-1").unwrap(), 1);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn corrupt_committed_journal_is_retained_and_never_clears_pending_state() {
    let (dir, mut db, _) = fixture();
    let journal = dir.join("pending");
    std::fs::create_dir_all(&journal).unwrap();
    let path = journal.join("damaged.json");
    std::fs::write(&path, b"{broken").unwrap();
    let exp = ExpControl::new();
    assert!(retry_pending_completions(&mut db, &exp, &journal).is_err());
    assert!(exp.persistence_pending());
    assert_eq!(std::fs::read(path).unwrap(), b"{broken");
    assert!(db
        .get_experiment("run-1")
        .unwrap()
        .unwrap()
        .end_time
        .is_none());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn complete_temporary_journal_is_recovered_but_partial_file_is_kept_for_inspection() {
    let (dir, mut db, completion) = fixture();
    let journal = dir.join("pending");
    std::fs::create_dir_all(&journal).unwrap();
    let path = journal.join("test-completion.tmp");
    std::fs::write(&path, serde_json::to_vec(&completion).unwrap()).unwrap();
    let incomplete = journal.join("interrupted.tmp");
    std::fs::write(&incomplete, b"{").unwrap();
    let exp = ExpControl::new();
    retry_pending_completions(&mut db, &exp, &journal).unwrap();
    assert_eq!(db.count_samples("run-1").unwrap(), 1);
    assert!(!path.exists());
    assert!(incomplete.exists());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn conflicting_disk_and_memory_payloads_are_not_committed_or_discarded() {
    let (dir, mut db, completion) = fixture();
    let journal = dir.join("pending");
    recovery::persist(&journal, &completion).unwrap();
    let mut changed = completion.clone();
    changed.rows[0].weight = 10.0;
    assert!(recovery::persist(&journal, &changed).is_err());
    let exp = ExpControl::new();
    lock(&exp.pending).push(changed);
    assert!(retry_pending_completions(&mut db, &exp, &journal).is_err());
    assert_eq!(db.count_samples("run-1").unwrap(), 0);
    assert_eq!(lock(&exp.pending).len(), 1);
    assert_eq!(recovery::scan(&journal).pending.len(), 1);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn update_lease_blocks_start_and_releases_after_failure() {
    let state = super::tests::test_state();
    let lease = UpdateLease::begin(state.exp.clone()).unwrap();
    assert!(state.exp.snapshot().updating);
    assert!(UpdateLease::begin(state.exp.clone()).is_err());
    let params = ExperimentParams {
        sample_id: "S1".into(),
        sample_name: "sample".into(),
        sample_weight: 500.0,
        experiment_type: "REDUCIBILITY".into(),
        project_name: None,
        notes: None,
    };
    assert!(build_preflight(&state, &params)
        .blocking
        .iter()
        .any(|issue| issue.code == "UPDATE_IN_PROGRESS"));
    drop(lease);
    assert!(!state.exp.snapshot().updating);
    lock(&state.exp.pending).push(fixture().2);
    assert!(UpdateLease::begin(state.exp.clone()).is_err());
}
