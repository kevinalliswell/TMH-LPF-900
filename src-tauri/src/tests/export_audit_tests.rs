//! Native exports authorize their frozen source and record one terminal outcome.
use super::device_control_authorization_tests::{
    assert_audit, command_audit, invoke_with_body, select_actor, webview, ACTORS,
};
use super::group_tests::{native_group_insert_trial, native_group_request};
use super::*;
use serde_json::{json, Value};
use std::path::Path;
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};

const TRIALS: [&str; 2] = ["export-trial-a", "export-trial-b"];
const COMMANDS: [(&str, &str, &str); 3] = [
    (
        "generate_test_group_report",
        "EXPORT",
        "generate_test_group_report",
    ),
    ("generate_report", "EXPORT", "generate_report"),
    ("export_experiment", "EXPORT", "export_data"),
];

fn database_path(state: &AppState) -> PathBuf {
    state.recovery_dir.parent().unwrap().join("export-audit.db")
}

fn fixture() -> tauri::App<MockRuntime> {
    let mut state = test_state();
    *lock(&state.storage) = Storage::open(&database_path(&state)).unwrap();
    state.exports_dir = state.recovery_dir.parent().unwrap().join("exports");
    seed_users_and_experiments(&state);
    let program = standard_program("REDUCIBILITY").unwrap();
    for (index, id) in TRIALS.iter().enumerate() {
        native_group_insert_trial(&state, id, "exp-alice", index as u32, &program, true, true);
    }
    login(&state, "alice", "temp-password");
    groups::save(
        &state,
        native_group_request(None, None, &TRIALS, "Initial export fixture"),
    )
    .unwrap();
    state.auth.logout();
    mock_builder()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            generate_test_group_report,
            generate_report,
            export_experiment
        ])
        .build(mock_context(noop_assets()))
        .unwrap()
}

fn request(state: &AppState, command: &str) -> Value {
    match command {
        "generate_test_group_report" => {
            let group = lock(&state.storage)
                .list_test_groups(None, 0)
                .unwrap()
                .remove(0);
            json!({"id":group.group_id,"revision":group.revision})
        }
        "generate_report" => json!({"id":"exp-alice"}),
        "export_experiment" => json!({"id":"exp-alice","format":"csv"}),
        _ => unreachable!(),
    }
}

fn exported_files(state: &AppState) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if !state.exports_dir.exists() {
        return files;
    }
    for user in std::fs::read_dir(&state.exports_dir).unwrap() {
        for entry in std::fs::read_dir(user.unwrap().path()).unwrap() {
            files.push(entry.unwrap().path());
        }
    }
    files.sort();
    files
}

fn reject(
    state: &AppState,
    view: &tauri::WebviewWindow<MockRuntime>,
    command: &str,
    action: &str,
    request: Value,
    code: &str,
) {
    let before_files = exported_files(state);
    let before_audit = command_audit(state, action).len();
    let error = invoke_with_body(view, command, request).unwrap_err();
    assert_eq!(error["code"], code);
    assert_eq!(exported_files(state), before_files);
    assert_audit(
        state,
        before_audit,
        "EXPORT",
        action,
        "REJECTED",
        "爱丽丝",
        Some(code),
    );
}

#[test]
fn export_ipc_actor_matrices_audit_denials_without_files_and_authorized_outputs_once() {
    for (command, category, action) in COMMANDS {
        let app = fixture();
        let state = app.state::<AppState>();
        let view = webview(&app);
        for (actor, operator, actor_code) in ACTORS {
            select_actor(&state, actor);
            let before_files = exported_files(&state);
            let before_audit = command_audit(&state, action).len();
            let result = invoke_with_body(&view, command, request(&state, command));
            if let Some(code) = actor_code {
                let error = result.unwrap_err();
                assert_eq!(error["code"], code, "{command}/{actor}");
                assert_eq!(exported_files(&state), before_files);
                assert_audit(
                    &state,
                    before_audit,
                    category,
                    action,
                    "REJECTED",
                    operator,
                    Some(code),
                );
            } else {
                let path = result.unwrap().as_str().unwrap().to_string();
                assert!(
                    Path::new(&path).is_file(),
                    "{command} must return a saved file"
                );
                assert_audit(
                    &state,
                    before_audit,
                    category,
                    action,
                    "SUCCESS",
                    operator,
                    None,
                );
                if command == "generate_test_group_report" {
                    assert!(Path::new(&path).with_extension("json").is_file());
                }
            }
        }
    }
}

#[test]
fn unsupported_format_is_rejected_and_audited_without_creating_a_file() {
    let app = fixture();
    let state = app.state::<AppState>();
    let view = webview(&app);
    select_actor(&state, "owner");
    let mut body = request(&state, "export_experiment");
    body["format"] = json!("exe");
    let before = exported_files(&state);
    let before_audit = command_audit(&state, "export_data").len();
    let error = invoke_with_body(&view, "export_experiment", body).unwrap_err();
    assert_eq!(error["code"], "INVALID_PARAMS");
    assert_eq!(exported_files(&state), before);
    assert_audit(
        &state,
        before_audit,
        "EXPORT",
        "export_data",
        "REJECTED",
        "爱丽丝",
        Some("INVALID_PARAMS"),
    );
}

#[test]
fn sample_exports_keep_csv_txt_and_xlsx_formats() {
    for format in ["csv", "txt", "xlsx"] {
        let app = fixture();
        let state = app.state::<AppState>();
        let view = webview(&app);
        select_actor(&state, "owner");
        let mut body = request(&state, "export_experiment");
        body["format"] = json!(format);
        let before = command_audit(&state, "export_data").len();
        let path = invoke_with_body(&view, "export_experiment", body)
            .unwrap()
            .as_str()
            .unwrap()
            .to_string();
        let bytes = std::fs::read(&path).unwrap();
        assert!(!bytes.is_empty());
        assert_eq!(Path::new(&path).extension().unwrap(), format);
        if format == "xlsx" {
            assert!(bytes.starts_with(b"PK\x03\x04"));
        }
        assert_audit(
            &state,
            before,
            "EXPORT",
            "export_data",
            "SUCCESS",
            "爱丽丝",
            None,
        );
    }
}

#[test]
fn malformed_saved_analysis_cannot_be_exported_as_a_complete_report() {
    let app = fixture();
    let state = app.state::<AppState>();
    let view = webview(&app);
    select_actor(&state, "owner");
    lock(&state.storage)
        .set_analysis_results("exp-alice", "not-json")
        .unwrap();
    let before = command_audit(&state, "generate_report").len();
    let error =
        invoke_with_body(&view, "generate_report", request(&state, "generate_report")).unwrap_err();
    assert_eq!(error["code"], "REPORT_DATA_INVALID");
    assert!(exported_files(&state).is_empty());
    assert_audit(
        &state,
        before,
        "EXPORT",
        "generate_report",
        "FAILURE",
        "爱丽丝",
        Some("REPORT_DATA_INVALID"),
    );
}

#[test]
fn update_and_accepted_close_reject_new_exports() {
    for updating in [true, false] {
        for (command, category, action) in COMMANDS {
            let app = fixture();
            let state = app.state::<AppState>();
            let view = webview(&app);
            select_actor(&state, "owner");
            let lease = if updating {
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
            reject(
                &state,
                &view,
                command,
                action,
                request(&state, command),
                code,
            );
            drop(lease);
            let entries = command_audit(&state, action);
            assert_eq!(entries.last().unwrap()["category"], category);
        }
    }
}

#[test]
fn database_read_faults_fail_without_leaking_sql_or_creating_export_artifacts() {
    for (command, action, table) in [
        (
            "generate_test_group_report",
            "generate_test_group_report",
            "test_group_revisions",
        ),
        ("generate_report", "generate_report", "experiment_data"),
        ("export_experiment", "export_data", "experiment_data"),
    ] {
        let app = fixture();
        let state = app.state::<AppState>();
        let view = webview(&app);
        select_actor(&state, "owner");
        let body = request(&state, command);
        rusqlite::Connection::open(database_path(&state))
            .unwrap()
            .execute_batch(&format!("DROP TABLE {table}"))
            .unwrap();
        let before = command_audit(&state, action).len();
        let error = invoke_with_body(&view, command, body).unwrap_err();
        assert_eq!(error["code"], "STORAGE_ERROR");
        assert!(!error.to_string().contains("no such table"));
        assert!(exported_files(&state).is_empty());
        assert_audit(
            &state,
            before,
            "EXPORT",
            action,
            "FAILURE",
            "爱丽丝",
            Some("STORAGE_ERROR"),
        );
    }
}

#[test]
fn export_directory_write_faults_are_failures_with_stable_errors() {
    for (command, action) in [
        ("generate_test_group_report", "generate_test_group_report"),
        ("generate_report", "generate_report"),
        ("export_experiment", "export_data"),
    ] {
        let app = fixture();
        let state = app.state::<AppState>();
        let view = webview(&app);
        select_actor(&state, "owner");
        std::fs::write(&state.exports_dir, "not a directory").unwrap();
        let before = command_audit(&state, action).len();
        let error = invoke_with_body(&view, command, request(&state, command)).unwrap_err();
        assert_eq!(error["code"], "EXPORT_FAILED");
        assert!(!error.to_string().contains("not a directory"));
        assert_audit(
            &state,
            before,
            "EXPORT",
            action,
            "FAILURE",
            "爱丽丝",
            Some("EXPORT_FAILED"),
        );
    }
}

#[test]
fn authentication_storage_faults_are_export_failures_not_rejections() {
    let app = fixture();
    let state = app.state::<AppState>();
    let view = webview(&app);
    select_actor(&state, "owner");
    rusqlite::Connection::open(database_path(&state))
        .unwrap()
        .execute_batch("PRAGMA foreign_keys=OFF; DROP TABLE users")
        .unwrap();
    for (command, action) in [
        ("generate_test_group_report", "generate_test_group_report"),
        ("generate_report", "generate_report"),
        ("export_experiment", "export_data"),
    ] {
        let before = command_audit(&state, action).len();
        let error = invoke_with_body(&view, command, request(&state, command)).unwrap_err();
        assert_eq!(error["code"], "STORAGE_ERROR");
        assert!(exported_files(&state).is_empty());
        assert_audit(
            &state,
            before,
            "EXPORT",
            action,
            "FAILURE",
            "anonymous",
            Some("STORAGE_ERROR"),
        );
    }
}
