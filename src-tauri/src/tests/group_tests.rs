// The native groups API is the authorization/freeze boundary under test.
use super::*;
use crate::groups::{GroupRequest, TrialSelection};
use tmh_core::repeat_group::MaterialKind;
use tmh_core::storage::TestGroupRevision;

fn native_group_request(
    group_id: Option<&str>,
    expected_revision: Option<u64>,
    ids: &[&str],
    reason: &str,
) -> GroupRequest {
    GroupRequest {
        group_id: group_id.map(str::to_owned),
        expected_revision,
        name: "Same-batch pellet group".into(),
        batch_id: "BATCH-NATIVE-001".into(),
        same_batch_confirmed: true,
        report_details: Default::default(),
        material: MaterialKind::Pellet,
        manual_limits: None,
        trials: ids
            .iter()
            .map(|id| TrialSelection {
                experiment_id: (*id).into(),
                exclusion_reason: None,
            })
            .collect(),
        reason: reason.into(),
    }
}

fn native_group_analysis(ri: f64) -> serde_json::Value {
    serde_json::json!({
        "standard": "GB/T 13241",
        "standard_version": "2017",
        "calculation_version": CALCULATION_VERSION,
        "ri": ri,
        "rt": ri,
        "endpoint_min": 180.0,
        "reduction_rate_index": 1.25,
        "result_scope": "single_trial",
        "standard_conformity": "pending_group_and_process_verification"
    })
}

fn native_group_insert_trial(
    state: &AppState,
    id: &str,
    owner_seed: &str,
    ordinal: u32,
    program: &Program,
    finished: bool,
    complete: bool,
) {
    let mut db = lock(&state.storage);
    let mut experiment = db.get_experiment(owner_seed).unwrap().unwrap();
    experiment.experiment_id = id.into();
    experiment.experiment_name = "Independent same-batch reduction".into();
    experiment.sample_id = Some(format!("GROUP-TRIAL-{ordinal:03}"));
    experiment.sample_name = "同批铁球团矿".into();
    experiment.sample_weight = 500.0;
    experiment.start_time = format!("2026-09-{:02}T08:00:00Z", ordinal + 1);
    experiment.experiment_time = Some(experiment.start_time.clone());
    experiment.end_time = finished.then(|| format!("2026-09-{:02}T12:00:00Z", ordinal + 1));
    experiment.created_at = experiment.start_time.clone();
    experiment.experiment_type = Some(program.id.clone());
    experiment.program_snapshot_json = Some(serde_json::to_string(program).unwrap());
    experiment.analysis_results_json = None;
    db.insert_experiment(&experiment).unwrap();
    db.patch_runtime_metadata(
        id,
        &serde_json::json!({
            "schema_version": 1,
            "outcome": if finished { "completed" } else { "running" },
            "device_mode": "mock",
            "data_complete": complete,
            "ending_action": "normal",
            "stage_actions": [],
            "skipped": false,
            "report_details": {"lab_name": "Original laboratory"}
        })
        .to_string(),
    )
    .unwrap();
    // Insert through the real analysis API so a database revision ID exists.
    db.set_analysis_results(
        id,
        &native_group_analysis(42.0 + f64::from(ordinal % 2) * 2.0).to_string(),
    )
    .unwrap();
}

fn native_group_frozen<'a>(record: &'a TestGroupRevision, id: &str) -> &'a serde_json::Value {
    record.snapshot["trials"]
        .as_array()
        .expect("group snapshot contains frozen trials")
        .iter()
        .find(|trial| trial["experiment"]["experiment_id"].as_str() == Some(id))
        .expect("requested trial remains in the frozen group")
}

#[test]
fn native_groups_enforce_session_ownership_and_record_the_actual_actor() {
    let state = test_state();
    let (_admin_id, alice_id, _) = seed_users_and_experiments(&state);
    let program = standard_program("REDUCIBILITY").unwrap();
    let ids = ["exp-group-access-alice-a", "exp-group-access-alice-b"];
    for (ordinal, id) in ids.iter().enumerate() {
        native_group_insert_trial(
            &state,
            id,
            "exp-alice",
            ordinal as u32,
            &program,
            true,
            true,
        );
    }
    login(&state, "alice", "temp-password");
    assert!(groups::preview(
        &state,
        native_group_request(None, None, &ids, "Preview owner data")
    )
    .is_ok());
    let first = groups::save(
        &state,
        native_group_request(None, None, &ids, "Create owner group"),
    )
    .unwrap();
    assert_eq!(first.revision, 1);
    assert_eq!(
        first.snapshot["trials"][0]["process_assessment"]["version"],
        tmh_core::process_evidence::VERSION
    );
    assert!(
        first.snapshot["trials"][0]["process_assessment"]["stages"]
            .as_array()
            .unwrap()
            .len()
            > 2
    );
    assert_eq!(first.owner_user_id, alice_id);
    assert_eq!(first.recorded_by_user_id, alice_id);
    assert_eq!(
        groups::get(&state, &first.group_id, Some(1)).unwrap(),
        first
    );
    assert_eq!(groups::list(&state, 0).unwrap(), vec![first.clone()]);

    let exported = groups::export_report(&state, &first.group_id, 1).unwrap();
    let exported_html = std::fs::read_to_string(&exported).unwrap();
    assert_eq!(
        exported_html,
        tmh_core::group_report::render(&first).unwrap()
    );
    let evidence_path = std::path::Path::new(&exported).with_extension("json");
    let evidence: TestGroupRevision =
        serde_json::from_str(&std::fs::read_to_string(evidence_path).unwrap()).unwrap();
    assert_eq!(evidence, first);

    let denied_without_session = || {
        assert!(groups::preview(
            &state,
            native_group_request(None, None, &ids, "Anonymous preview")
        )
        .is_err());
        assert!(groups::save(
            &state,
            native_group_request(None, None, &ids, "Anonymous save")
        )
        .is_err());
        assert!(groups::get(&state, &first.group_id, None).is_err());
        assert!(groups::get(&state, &first.group_id, Some(1)).is_err());
        assert!(groups::list(&state, 0).is_err());
        assert!(groups::export_report(&state, &first.group_id, 1).is_err());
    };
    state.auth.logout();
    denied_without_session();
    login(&state, "alice", "temp-password");
    state.auth.lock();
    denied_without_session();

    login(&state, "bob", "temp-password");
    assert!(groups::preview(
        &state,
        native_group_request(None, None, &ids, "Other owner's records")
    )
    .is_err());
    assert!(groups::save(
        &state,
        native_group_request(None, None, &ids, "Other owner's records")
    )
    .is_err());
    assert!(groups::get(&state, &first.group_id, None).is_err());
    assert!(groups::get(&state, &first.group_id, Some(1)).is_err());
    assert!(groups::list(&state, 0).unwrap().is_empty());
    assert!(groups::export_report(&state, &first.group_id, 1).is_err());

    login(&state, "admin", "admin-password");
    assert_eq!(groups::get(&state, &first.group_id, None).unwrap(), first);
    assert!(groups::export_report(&state, &first.group_id, 1).is_ok());
    assert_eq!(groups::list(&state, 0).unwrap(), vec![first.clone()]);
    assert_eq!(
        lock(&state.storage)
            .test_group_history(&first.group_id, None)
            .unwrap(),
        vec![first]
    );
    assert_eq!(
        lock(&state.storage)
            .list_test_groups(None, 0)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn native_group_updates_freeze_member_evidence_and_reject_stale_or_invalid_sets() {
    let state = test_state();
    let (_, alice_id, _) = seed_users_and_experiments(&state);
    let program = standard_program("REDUCIBILITY").unwrap();
    let ids = ["exp-group-freeze-alice-a", "exp-group-freeze-alice-b"];
    for (ordinal, id) in ids.iter().enumerate() {
        native_group_insert_trial(
            &state,
            id,
            "exp-alice",
            ordinal as u32,
            &program,
            true,
            true,
        );
    }
    login(&state, "alice", "temp-password");
    let first = groups::save(
        &state,
        native_group_request(None, None, &ids, "Freeze original analyses"),
    )
    .unwrap();
    let original_analysis_revision = lock(&state.storage)
        .current_analysis_revision_id(ids[0])
        .unwrap();
    assert!(original_analysis_revision.is_some());
    let frozen_a = native_group_frozen(&first, ids[0]);
    assert_eq!(
        frozen_a["analysis_revision_id"],
        serde_json::json!(original_analysis_revision)
    );
    let analysis: serde_json::Value = serde_json::from_str(
        frozen_a["experiment"]["analysis_results_json"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(analysis["ri"], 42.0);

    {
        let mut db = lock(&state.storage);
        db.set_analysis_results(ids[0], &native_group_analysis(99.0).to_string())
            .unwrap();
        db.patch_runtime_metadata(
            ids[0],
            r#"{"report_details":{"lab_name":"Changed after grouping"}}"#,
        )
        .unwrap();
        assert_ne!(
            db.current_analysis_revision_id(ids[0]).unwrap(),
            original_analysis_revision
        );
    }
    assert_eq!(
        groups::get(&state, &first.group_id, Some(1)).unwrap(),
        first
    );
    let second = groups::save(
        &state,
        native_group_request(
            Some(&first.group_id),
            Some(1),
            &ids,
            "Save another explained version",
        ),
    )
    .unwrap();
    assert_eq!(second.revision, 2);
    assert_eq!(second.owner_user_id, alice_id);
    assert_eq!(second.recorded_by_user_id, alice_id);
    for id in ids {
        assert_eq!(
            native_group_frozen(&second, id),
            native_group_frozen(&first, id),
            "a new group revision must retain its original member analysis, runtime and revision ID"
        );
    }
    let history = lock(&state.storage)
        .test_group_history(&first.group_id, None)
        .unwrap();
    assert_eq!(history, vec![second.clone(), first.clone()]);
    assert!(groups::save(
        &state,
        native_group_request(
            Some(&first.group_id),
            Some(1),
            &ids,
            "Stale editor submission"
        ),
    )
    .is_err());
    assert_eq!(
        lock(&state.storage)
            .test_group_history(&first.group_id, None)
            .unwrap(),
        history
    );

    // Each invalid new group has only two distinct candidate trials unless the
    // case itself tests duplication. A legitimate need for more trials is not an error.
    native_group_insert_trial(&state, "exp-group-bob", "exp-bob", 2, &program, true, true);
    native_group_insert_trial(
        &state,
        "exp-group-unfinished",
        "exp-alice",
        3,
        &program,
        false,
        true,
    );
    native_group_insert_trial(
        &state,
        "exp-group-incomplete",
        "exp-alice",
        4,
        &program,
        true,
        false,
    );
    let mut other_temperature = program.clone();
    other_temperature.stages[2].target_temp = 850.0;
    native_group_insert_trial(
        &state,
        "exp-group-other-temperature",
        "exp-alice",
        5,
        &other_temperature,
        true,
        true,
    );
    let mut other_atmosphere = program.clone();
    other_atmosphere.stages[2].gas[1] = 10.0;
    native_group_insert_trial(
        &state,
        "exp-group-other-atmosphere",
        "exp-alice",
        6,
        &other_atmosphere,
        true,
        true,
    );
    for other in [
        "exp-group-bob",
        "exp-group-unfinished",
        "exp-group-incomplete",
        "exp-group-other-temperature",
        "exp-group-other-atmosphere",
        ids[1],
    ] {
        let candidates = [ids[1], other];
        assert!(
            groups::preview(
                &state,
                native_group_request(None, None, &candidates, "Invalid member set")
            )
            .is_err(),
            "preview accepted invalid member {other}"
        );
        assert!(
            groups::save(
                &state,
                native_group_request(None, None, &candidates, "Invalid member set")
            )
            .is_err(),
            "save accepted invalid member {other}"
        );
    }
    assert_eq!(
        lock(&state.storage)
            .test_group_history(&first.group_id, None)
            .unwrap(),
        history
    );
    assert_eq!(
        lock(&state.storage).list_test_groups(None, 0).unwrap(),
        vec![second]
    );
}

#[test]
fn group_report_maps_excluded_trials_and_uses_only_frozen_evidence() {
    let state = test_state();
    seed_users_and_experiments(&state);
    let program = standard_program("REDUCIBILITY").unwrap();
    let ids = ["exp-report-first", "exp-report-invalid", "exp-report-last"];
    for (i, id) in ids.iter().enumerate() {
        native_group_insert_trial(&state, id, "exp-alice", i as u32, &program, true, i != 1);
    }
    login(&state, "alice", "temp-password");
    let mut request = native_group_request(None, None, &ids, "Original group <record>");
    request.name = "<img src=x onerror=alert(1)>".into();
    request.report_details.lab_name = "Lab <script>unsafe</script>".into();
    request.trials[1].exclusion_reason = Some("Invalid <data>".into());
    request.trials.reverse();
    let first = groups::save(&state, request.clone()).unwrap();
    assert_eq!(
        first.snapshot["valid_trial_indices"],
        serde_json::json!([0, 2])
    );
    assert_eq!(
        first.snapshot["evaluation"]["decision"]["selected_indices"],
        serde_json::json!([0, 1])
    );
    let html = tmh_core::group_report::render(&first).unwrap();
    assert!(html.contains("模拟"));
    assert!(html.contains("Invalid &lt;data&gt;"));
    assert!(html.contains("Lab &lt;script&gt;unsafe&lt;/script&gt;"));
    assert!(!html.contains("<img src=x"));
    assert!(html.contains("42.0"));
    assert!(html.contains("尚未判定"));
    lock(&state.storage).delete_experiment(ids[0]).unwrap();
    assert_eq!(
        tmh_core::group_report::render(&groups::get(&state, &first.group_id, Some(1)).unwrap())
            .unwrap(),
        html
    );
    request.group_id = Some(first.group_id.clone());
    request.expected_revision = Some(1);
    request.report_details.reviewer = "New reviewer".into();
    request.reason = "Add reviewer".into();
    let second = groups::save(&state, request).unwrap();
    assert_eq!(
        second.snapshot["report_details"]["reviewer"],
        "New reviewer"
    );
    assert_eq!(first.snapshot["report_details"]["reviewer"], "");
    assert_eq!(
        native_group_frozen(&first, ids[0]),
        native_group_frozen(&second, ids[0])
    );
}
// Append to src-tauri/src/tests/group_tests.rs; uses that module's existing
// native_group_request/native_group_insert_trial helpers and parent fixtures.
// Test-only regression: do not grant a group's owner new access merely because
// an administrator can read and append another user's evidence.

fn assert_admin_cannot_append_another_owners_trial(excluded: bool) {
    let state = test_state();
    let (_, alice_id, _) = seed_users_and_experiments(&state);
    let program = standard_program("REDUCIBILITY").unwrap();
    let alice_trial = "exp-group-share-alice";
    let bob_trial = "exp-group-share-bob";
    native_group_insert_trial(&state, alice_trial, "exp-alice", 0, &program, true, true);
    native_group_insert_trial(&state, bob_trial, "exp-bob", 1, &program, true, true);

    login(&state, "alice", "temp-password");
    // One effective trial is a legitimate saved group awaiting its repeat.
    let original = groups::save(
        &state,
        native_group_request(None, None, &[alice_trial], "Await an independent repeat"),
    )
    .unwrap();
    assert_eq!(original.owner_user_id, alice_id);

    login(&state, "admin", "admin-password");
    let mut update = native_group_request(
        Some(&original.group_id),
        Some(1),
        &[alice_trial, bob_trial],
        "Administrator appends another owner's evidence",
    );
    if excluded {
        update.trials[1].exclusion_reason = Some("Excluded evidence is still private".into());
    }
    assert!(
        groups::preview(&state, update.clone()).is_err(),
        "preview must enforce the destination owner's access, including excluded records"
    );
    assert!(
        groups::save(&state, update).is_err(),
        "saving must not disclose Bob's frozen evidence to Alice"
    );
    assert_eq!(
        lock(&state.storage)
            .test_group_history(&original.group_id, None)
            .unwrap(),
        vec![original.clone()],
        "rejection must not append a partial or disclosure-bearing version"
    );

    login(&state, "alice", "temp-password");
    assert_eq!(
        groups::get(&state, &original.group_id, None).unwrap(),
        original
    );
    assert_eq!(groups::list(&state, 0).unwrap(), vec![original]);
}

#[test]
fn native_admin_cannot_append_bob_to_alices_group_as_a_valid_trial() {
    assert_admin_cannot_append_another_owners_trial(false);
}

#[test]
fn native_admin_cannot_append_bob_to_alices_group_as_excluded_evidence() {
    assert_admin_cannot_append_another_owners_trial(true);
}

#[test]
fn native_admin_can_create_an_admin_owned_group_across_trial_owners() {
    let state = test_state();
    let (admin_id, _, _) = seed_users_and_experiments(&state);
    let program = standard_program("REDUCIBILITY").unwrap();
    let ids = ["exp-group-admin-alice", "exp-group-admin-bob"];
    native_group_insert_trial(&state, ids[0], "exp-alice", 0, &program, true, true);
    native_group_insert_trial(&state, ids[1], "exp-bob", 1, &program, true, true);

    login(&state, "admin", "admin-password");
    let request = native_group_request(None, None, &ids, "Administrator's cross-user review");
    let preview = groups::preview(&state, request.clone()).unwrap();
    assert_eq!(preview.trials.len(), 2);
    let saved = groups::save(&state, request).unwrap();
    assert_eq!(saved.owner_user_id, admin_id);
    assert_eq!(saved.recorded_by_user_id, admin_id);
    for id in ids {
        assert_eq!(
            native_group_frozen(&saved, id)["experiment"]["experiment_id"],
            id
        );
    }
    assert_eq!(groups::get(&state, &saved.group_id, None).unwrap(), saved);

    // The administrator's authorized review must not become implicit sharing.
    for username in ["alice", "bob"] {
        login(&state, username, "temp-password");
        assert!(groups::get(&state, &saved.group_id, None).is_err());
        assert!(groups::list(&state, 0).unwrap().is_empty());
    }
}
// Append to src-tauri/src/tests/group_tests.rs. Uses its existing native group
// helpers and parent AppState fixtures; no clock or system-timezone mutation.

fn native_group_insert_time_fixture(
    state: &AppState,
    id: &str,
    display_start: &str,
    started_at_utc: Option<&str>,
    ri: f64,
) {
    let mut db = lock(&state.storage);
    let mut row = db.get_experiment("exp-group-time-seed").unwrap().unwrap();
    let mut runtime: serde_json::Value =
        serde_json::from_str(&db.runtime_metadata("exp-group-time-seed").unwrap()).unwrap();
    row.experiment_id = id.into();
    row.start_time = display_start.into();
    row.experiment_time = Some(display_start.into());
    row.created_at = display_start.into();
    row.end_time = Some("2026-09-09T12:00:00Z".into());
    row.analysis_results_json = None;
    match started_at_utc {
        Some(value) => runtime["started_at_utc"] = serde_json::json!(value),
        None => {
            runtime.as_object_mut().unwrap().remove("started_at_utc");
        }
    }
    db.insert_experiment(&row).unwrap();
    db.patch_runtime_metadata(id, &runtime.to_string()).unwrap();
    db.set_analysis_results(id, &native_group_analysis(ri).to_string())
        .unwrap();
}

fn assert_native_group_uses_actual_start_instants(runtime_is_authoritative: bool) {
    let state = test_state();
    seed_users_and_experiments(&state);
    let program = standard_program("REDUCIBILITY").unwrap();
    native_group_insert_trial(
        &state,
        "exp-group-time-seed",
        "exp-alice",
        0,
        &program,
        true,
        true,
    );
    let ids = ["exp-group-time-a", "exp-group-time-b", "exp-group-time-c"];
    // A starts at 08:00 Shanghai (00:00 UTC). After A completes, the host changes
    // to UTC; B and C start at 03:10 and 06:20 UTC. Sorting the display strings
    // wrongly places B/C before A, so the first pair appears to meet limit A.
    let display = if runtime_is_authoritative {
        [
            "2026-09-08T08:00:00",
            "2026-09-08T03:10:00",
            "2026-09-08T06:20:00",
        ]
    } else {
        [
            "2026-09-08T08:00:00+08:00",
            "2026-09-08T03:10:00Z",
            "2026-09-08T06:20:00+00:00",
        ]
    };
    let utc = [
        "2026-09-08T00:00:00Z",
        "2026-09-08T03:10:00Z",
        "2026-09-08T06:20:00Z",
    ];
    for i in 0..3 {
        native_group_insert_time_fixture(
            &state,
            ids[i],
            display[i],
            runtime_is_authoritative.then_some(utc[i]),
            [80.0, 90.0, 90.5][i],
        );
    }
    login(&state, "alice", "temp-password");
    let request = native_group_request(
        None,
        None,
        &[ids[2], ids[0], ids[1]],
        "Preserve actual chronology across timezone changes",
    );
    let preview = groups::preview(&state, request.clone()).unwrap();
    let actual_order: Vec<_> = preview
        .trials
        .iter()
        .map(|trial| trial.experiment.experiment_id.as_str())
        .collect();
    assert_eq!(actual_order, ids.to_vec());
    assert_eq!(preview.evaluation.decision_values, vec![80.0, 90.0, 90.5]);
    assert_eq!(preview.evaluation.decision.required_count, 4);
    assert!(preview.evaluation.decision.selected_indices.is_empty());
    assert!(preview.evaluation.reported.is_empty());

    let saved = groups::save(&state, request).unwrap();
    assert_eq!(
        saved.snapshot["evaluation"]["decision"]["required_count"],
        4
    );
    for i in 0..3 {
        let frozen = native_group_frozen(&saved, ids[i]);
        assert_eq!(frozen["experiment"]["start_time"], display[i]);
        if runtime_is_authoritative {
            assert_eq!(frozen["runtime"]["started_at_utc"], utc[i]);
        }
    }
}

#[test]
fn native_group_uses_runtime_utc_instead_of_local_display_time() {
    assert_native_group_uses_actual_start_instants(true);
}

#[test]
fn native_group_parses_rfc3339_start_time_as_a_legacy_fallback() {
    assert_native_group_uses_actual_start_instants(false);
}

#[test]
fn native_group_rejects_ambiguous_old_times_but_preserves_explicitly_excluded_evidence() {
    let state = test_state();
    seed_users_and_experiments(&state);
    let program = standard_program("REDUCIBILITY").unwrap();
    native_group_insert_trial(
        &state,
        "exp-group-time-seed",
        "exp-alice",
        0,
        &program,
        true,
        true,
    );
    let old = "exp-group-time-ambiguous";
    let known = "exp-group-time-known";
    native_group_insert_time_fixture(&state, old, "2026-09-08T08:00:00", None, 42.0);
    native_group_insert_time_fixture(
        &state,
        known,
        "2026-09-08T12:00:00",
        Some("2026-09-08T04:00:00Z"),
        44.0,
    );
    login(&state, "alice", "temp-password");
    for ids in [vec![old], vec![old, known]] {
        let request = native_group_request(None, None, &ids, "Do not guess an old timezone");
        assert!(groups::preview(&state, request.clone()).is_err());
        assert!(groups::save(&state, request).is_err());
    }
    assert!(groups::list(&state, 0).unwrap().is_empty());

    let mut request = native_group_request(
        None,
        None,
        &[old, known],
        "Retain the old record while awaiting a repeat with verifiable time",
    );
    request.trials[0].exclusion_reason = Some("Historical start timezone was not recorded".into());
    let preview = groups::preview(&state, request.clone()).unwrap();
    assert_eq!(preview.evaluation.decision_values, vec![44.0]);
    assert_eq!(preview.evaluation.decision.required_count, 2);
    let saved = groups::save(&state, request).unwrap();
    let frozen = native_group_frozen(&saved, old);
    assert_eq!(frozen["experiment"]["start_time"], "2026-09-08T08:00:00");
    assert!(frozen["runtime"]["started_at_utc"].is_null());
    assert_eq!(
        frozen["exclusion_reason"],
        "Historical start timezone was not recorded"
    );
    assert!(tmh_core::group_report::render(&saved).is_ok());
}
