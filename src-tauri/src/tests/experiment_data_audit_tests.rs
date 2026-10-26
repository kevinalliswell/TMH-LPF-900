//! Protected data mutations use real IPC, SQLite transactions and audit files.
use super::device_control_authorization_tests::{
    assert_audit, command_audit, invoke_with_body, select_actor, webview, ACTORS,
};
use super::*;
use serde_json::{json, Value};
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};

const ID: &str = "data-audit-target";
const COMMANDS: [(&str, &str, &str, &str); 6] = [
    (
        "set_experiment_owner",
        "set_experiment_owner",
        "",
        "REDUCIBILITY",
    ),
    ("delete_experiment", "delete", "", "REDUCIBILITY"),
    ("analyze_gb13241", "analyze", "GB/T 13241", "REDUCIBILITY"),
    (
        "analyze_gb13242",
        "analyze",
        "GB/T 13242",
        "LOW_TEMP_DEGRADATION",
    ),
    ("analyze_gb13240", "analyze", "GB/T 13240", "FREE_SWELLING"),
    ("save_report_details", "report_details", "", "REDUCIBILITY"),
];

fn fixture(method: &str) -> tauri::App<MockRuntime> {
    let state = test_state();
    *lock(&state.storage) = Storage::open(&database_path(&state)).unwrap();
    seed_users_and_experiments(&state);
    let mut target = lock(&state.storage)
        .get_experiment("exp-alice")
        .unwrap()
        .unwrap();
    target.experiment_id = ID.into();
    target.experiment_type = Some(method.into());
    target.end_time = Some("2026-10-26T10:00:00".into());
    lock(&state.storage).insert_experiment(&target).unwrap();
    let mut sample = row("HEATING", 0, 100.0, false);
    sample.experiment_id = ID.into();
    lock(&state.storage).insert_samples(&[sample]).unwrap();
    lock(&state.storage)
        .set_analysis_results(ID, r#"{"fixture":"prior revision"}"#)
        .unwrap();
    mock_builder()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            set_experiment_owner,
            delete_experiment,
            analyze_gb13241,
            analyze_gb13242,
            analyze_gb13240,
            save_report_details
        ])
        .build(mock_context(noop_assets()))
        .unwrap()
}

fn database_path(state: &AppState) -> PathBuf {
    state.recovery_dir.parent().unwrap().join("data-audit.db")
}

fn report_details() -> Value {
    json!({"lab_name":"test laboratory","lab_address":"","report_number":"audit-1",
        "report_date":"2026-10-26","sample_preparation":"","equipment":"",
        "gas_and_calibration":"","external_steps":"","deviations":"",
        "reviewer":"","repeat_group_reference":""})
}

fn body(state: &AppState, command: &str) -> Value {
    match command {
        "set_experiment_owner" => {
            json!({"experiment_id":ID,"owner_user_id":lock(&state.storage).get_user_by_username("bob").unwrap().unwrap().user_id})
        }
        "delete_experiment" => json!({"id":ID}),
        "analyze_gb13241" => {
            json!({"id":ID,"input":{"w1":0.0,"w2":50.0,"manual_m1":100.0,"manual_mt":85.0,"source_record":"LAB-AUDIT original mass record"}})
        }
        "analyze_gb13242" => {
            json!({"id":ID,"input":{"m0":500.0,"m1":300.0,"m2":100.0,"m3":50.0,"drum_rpm":30.0,"drum_revolutions":300,"sieve_seconds":60.0}})
        }
        "analyze_gb13240" => {
            json!({"id":ID,"input":{"v0":27.0,"v1":36.0,"pellet_count":18,"volume_method":"volume record","same_method":true}})
        }
        "save_report_details" => json!({"id":ID,"details":report_details()}),
        _ => unreachable!(),
    }
}

fn snapshot(state: &AppState) -> Value {
    let db = lock(&state.storage);
    let target = db.get_experiment(ID).unwrap();
    json!({"rows":db.list_experiments().unwrap(), "samples":db.get_samples(ID,0).unwrap(),
        "metadata":target.as_ref().map(|_|db.runtime_metadata(ID).unwrap()),
        "revisions":db.analysis_revisions(ID).unwrap(), "current_revision":db.current_analysis_revision_id(ID).unwrap()})
}

fn terminal(
    state: &AppState,
    before: usize,
    action: &str,
    standard: &str,
    outcome: &str,
    operator: &str,
    code: Option<&str>,
) {
    assert_audit(state, before, "EXPERIMENT", action, outcome, operator, code);
    if !standard.is_empty() {
        assert_eq!(
            command_audit(state, action).last().unwrap()["detail"]["standard"],
            standard
        );
    }
}

#[test]
fn protected_data_ipc_six_actor_matrices_audit_denials_and_valid_writes_once() {
    for (command, action, standard, method) in COMMANDS {
        let app = fixture(method);
        let state = app.state::<AppState>();
        let view = webview(&app);
        for (actor, operator, actor_code) in ACTORS {
            select_actor(&state, actor);
            let before_data = snapshot(&state);
            let before = command_audit(&state, action).len();
            let code = if actor == "owner"
                && matches!(command, "set_experiment_owner" | "delete_experiment")
            {
                Some("FORBIDDEN")
            } else {
                actor_code
            };
            let result = invoke_with_body(&view, command, body(&state, command));
            if let Some(code) = code {
                assert_eq!(result.unwrap_err()["code"], code, "{command}/{actor}");
                assert_eq!(snapshot(&state), before_data, "{command}/{actor}");
                terminal(
                    &state,
                    before,
                    action,
                    standard,
                    "REJECTED",
                    operator,
                    Some(code),
                );
            } else {
                result.unwrap_or_else(|error| panic!("{command}/{actor}: {error}"));
                terminal(&state, before, action, standard, "SUCCESS", operator, None);
                let after = snapshot(&state);
                if command.starts_with("analyze_") || command == "save_report_details" {
                    assert_eq!(
                        after["revisions"].as_array().unwrap().len(),
                        before_data["revisions"].as_array().unwrap().len() + 1
                    );
                    assert_eq!(after["samples"], before_data["samples"]);
                } else if command == "delete_experiment" {
                    assert!(lock(&state.storage).get_experiment(ID).unwrap().is_none());
                    assert_eq!(after["samples"], json!([]));
                } else {
                    let db = lock(&state.storage);
                    assert_eq!(
                        db.get_experiment(ID).unwrap().unwrap().owner_user_id,
                        db.get_user_by_username("bob").unwrap().map(|u| u.user_id)
                    );
                }
            }
        }
    }
}

#[test]
fn data_state_type_and_input_rejections_preserve_samples_and_saved_revisions() {
    for (command, action, standard, method) in COMMANDS {
        let app = fixture(method);
        let state = app.state::<AppState>();
        let view = webview(&app);
        select_actor(&state, "admin");
        let mut request = body(&state, command);
        let known_code = match command {
            "set_experiment_owner" | "delete_experiment" => {
                state.exp.set_status(ExperimentStatus {
                    running: true,
                    experiment_id: Some(ID.into()),
                    ..ExperimentStatus::idle()
                });
                (command == "delete_experiment").then_some("INVALID_STATE")
            }
            "analyze_gb13241" => {
                request["input"]["w2"] = json!(-1);
                Some("INVALID_PARAMS")
            }
            "analyze_gb13242" => {
                request["input"]["m0"] = json!(-1);
                Some("INVALID_PARAMS")
            }
            "analyze_gb13240" => {
                request["input"]["pellet_count"] = json!(17);
                Some("INVALID_PARAMS")
            }
            "save_report_details" => {
                request["details"]["lab_name"] = json!("x".repeat(4097));
                None
            }
            _ => unreachable!(),
        };
        let before_data = snapshot(&state);
        let before = command_audit(&state, action).len();
        let error = invoke_with_body(&view, command, request).unwrap_err();
        if let Some(code) = known_code {
            assert_eq!(error["code"], code);
        }
        assert_eq!(snapshot(&state), before_data);
        terminal(
            &state,
            before,
            action,
            standard,
            "REJECTED",
            "管理员",
            error["code"].as_str(),
        );
        if command.starts_with("analyze_") {
            for (sql, code) in [
                ("UPDATE experiments SET end_time=NULL WHERE experiment_id='data-audit-target'", "INVALID_STATE"),
                ("UPDATE experiments SET end_time='2026-10-26',experiment_type='unsupported' WHERE experiment_id='data-audit-target'", "ANALYSIS_TYPE_MISMATCH"),
            ] {
                rusqlite::Connection::open(database_path(&state)).unwrap().execute_batch(sql).unwrap();
                let before_data = snapshot(&state);
                let before = command_audit(&state, action).len();
                let error = invoke_with_body(&view, command, body(&state, command)).unwrap_err();
                assert_eq!(error["code"], code);
                assert_eq!(snapshot(&state), before_data);
                terminal(&state, before, action, standard, "REJECTED", "管理员", Some(code));
            }
        }
    }
}

#[test]
fn sqlite_mutation_faults_are_storage_failures_and_roll_back_data_and_revisions() {
    for (command, action, standard, method) in COMMANDS {
        let app = fixture(method);
        let state = app.state::<AppState>();
        let view = webview(&app);
        select_actor(&state, "admin");
        let event = match command {
            "set_experiment_owner" => "UPDATE OF owner_user_id ON experiments",
            "delete_experiment" => "DELETE ON experiments",
            _ => "INSERT ON analysis_revisions",
        };
        rusqlite::Connection::open(database_path(&state)).unwrap().execute_batch(&format!(
            "CREATE TRIGGER deny_data_mutation BEFORE {event} BEGIN SELECT RAISE(ABORT,'controlled data audit fault'); END;"
        )).unwrap();
        let before_data = snapshot(&state);
        let before = command_audit(&state, action).len();
        let error = invoke_with_body(&view, command, body(&state, command)).unwrap_err();
        assert_eq!(error["code"], "STORAGE_ERROR", "{command}");
        assert!(!error.to_string().contains("controlled data audit fault"));
        assert_eq!(snapshot(&state), before_data);
        terminal(
            &state,
            before,
            action,
            standard,
            "FAILURE",
            "管理员",
            Some("STORAGE_ERROR"),
        );
    }
}

#[test]
fn authentication_storage_fault_audits_failure_for_each_data_command() {
    let app = fixture("REDUCIBILITY");
    let state = app.state::<AppState>();
    let view = webview(&app);
    select_actor(&state, "admin");
    let requests: Vec<_> = COMMANDS
        .into_iter()
        .map(|spec| (spec, body(&state, spec.0)))
        .collect();
    rusqlite::Connection::open(database_path(&state))
        .unwrap()
        .execute_batch("PRAGMA foreign_keys=OFF; DROP TABLE users")
        .unwrap();
    for ((command, action, standard, _), request) in requests {
        let before_data = snapshot(&state);
        let before = command_audit(&state, action).len();
        let error = invoke_with_body(&view, command, request).unwrap_err();
        assert_eq!(error["code"], "STORAGE_ERROR");
        assert_eq!(snapshot(&state), before_data);
        terminal(
            &state,
            before,
            action,
            standard,
            "FAILURE",
            "anonymous",
            Some("STORAGE_ERROR"),
        );
    }
}

#[test]
fn update_and_accepted_close_exclude_every_data_mutation_and_audit_each_refusal() {
    for updating in [true, false] {
        for (command, action, standard, method) in COMMANDS {
            let app = fixture(method);
            let state = app.state::<AppState>();
            let view = webview(&app);
            select_actor(&state, "admin");
            let _lease = if updating {
                Some(UpdateLease::begin(state.exp.clone()).unwrap())
            } else {
                assert!(!prepare_exit(app.handle()));
                None
            };
            let code = if updating {
                "UPDATE_IN_PROGRESS"
            } else {
                "APP_CLOSING"
            };
            let before_data = snapshot(&state);
            let before = command_audit(&state, action).len();
            let error = invoke_with_body(&view, command, body(&state, command)).unwrap_err();
            assert_eq!(error["code"], code, "{command}");
            assert_eq!(snapshot(&state), before_data);
            terminal(
                &state,
                before,
                action,
                standard,
                "REJECTED",
                "管理员",
                Some(code),
            );
        }
    }
}

#[test]
fn malformed_stored_program_cannot_qualify_any_analysis_method() {
    for (command, action, standard, method) in COMMANDS
        .into_iter()
        .filter(|spec| spec.0.starts_with("analyze_"))
    {
        let app = fixture(method);
        let state = app.state::<AppState>();
        let view = webview(&app);
        select_actor(&state, "owner");
        rusqlite::Connection::open(database_path(&state)).unwrap().execute_batch(
            "UPDATE experiments SET program_snapshot_json='broken JSON' WHERE experiment_id='data-audit-target'"
        ).unwrap();
        let before_data = snapshot(&state);
        let before = command_audit(&state, action).len();
        let error = invoke_with_body(&view, command, body(&state, command)).unwrap_err();
        assert_eq!(error["code"], "ANALYSIS_DATA_INVALID");
        assert_eq!(snapshot(&state), before_data);
        terminal(
            &state,
            before,
            action,
            standard,
            "REJECTED",
            "爱丽丝",
            Some("ANALYSIS_DATA_INVALID"),
        );
    }
}
