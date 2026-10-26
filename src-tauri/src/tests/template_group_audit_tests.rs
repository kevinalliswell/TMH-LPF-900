//! Native template/group commands append exactly one truthful terminal audit.
use super::device_control_authorization_tests::{
    assert_audit, command_audit, invoke_with_body, select_actor, webview, ACTORS,
};
use super::group_tests::{native_group_insert_trial, native_group_request};
use super::*;
use serde_json::{json, Value};
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};

const CUSTOM: &str = "custom-command-audit";
const TRIALS: [&str; 2] = ["audit-trial-a", "audit-trial-b"];
const COMMANDS: [(&str, &str); 3] = [
    ("save_custom_program", "CONFIG"),
    ("delete_custom_program", "CONFIG"),
    ("save_test_group", "EXPERIMENT"),
];

fn database_path(state: &AppState) -> PathBuf {
    state
        .recovery_dir
        .parent()
        .unwrap()
        .join("template-group-audit.db")
}

fn fixture() -> tauri::App<MockRuntime> {
    let state = test_state();
    *lock(&state.storage) = Storage::open(&database_path(&state)).unwrap();
    seed_users_and_experiments(&state);
    login(&state, "admin", "admin-password");
    custom_programs::save(
        &state,
        f06_native_draft(CUSTOM),
        "Initial versioned template",
    )
    .unwrap();
    let program = standard_program("REDUCIBILITY").unwrap();
    for (index, id) in TRIALS.iter().enumerate() {
        native_group_insert_trial(&state, id, "exp-alice", index as u32, &program, true, true);
    }
    login(&state, "alice", "temp-password");
    groups::save(
        &state,
        native_group_request(None, None, &TRIALS, "Initial owner group"),
    )
    .unwrap();
    state.auth.logout();
    mock_builder()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            save_custom_program,
            delete_custom_program,
            save_test_group
        ])
        .build(mock_context(noop_assets()))
        .unwrap()
}

fn snapshot(state: &AppState) -> Value {
    let db = lock(&state.storage);
    let groups = db.list_test_groups(None, 0).unwrap();
    let history: Vec<_> = groups
        .iter()
        .map(|g| db.test_group_history(&g.group_id, None).unwrap())
        .collect();
    let trials: Vec<_> = TRIALS.iter().map(|id| json!({
        "row":db.get_experiment(id).unwrap(), "runtime":db.runtime_metadata(id).unwrap(),
        "samples":db.get_samples(id,0).unwrap(), "analyses":db.analysis_revisions(id).unwrap()
    })).collect();
    json!({"templates":db.list_custom_programs().unwrap(), "template_history":db.custom_program_revisions(CUSTOM).unwrap(), "groups":groups,"history":history,"trials":trials})
}

fn body(state: &AppState, command: &str) -> Value {
    let db = lock(&state.storage);
    match command {
        "save_custom_program" => {
            let mut program: Program =
                serde_json::from_str(&db.get_custom_program(CUSTOM).unwrap().unwrap()).unwrap();
            program.stages[2].target_temp += 1.0;
            json!({"program":program,"reason":"Compare revised temperature"})
        }
        "delete_custom_program" => json!({"id":CUSTOM}),
        "save_test_group" => {
            let group = db.list_test_groups(None, 0).unwrap().remove(0);
            json!({"request":native_group_request(Some(&group.group_id),Some(group.revision),&TRIALS,"Revise group report")})
        }
        _ => unreachable!(),
    }
}

fn reject(
    state: &AppState,
    view: &tauri::WebviewWindow<MockRuntime>,
    command: &str,
    category: &str,
    request: Value,
    expected_code: Option<&str>,
) {
    let before_data = snapshot(state);
    let before = command_audit(state, command).len();
    let error = invoke_with_body(view, command, request).unwrap_err();
    if let Some(code) = expected_code {
        assert_eq!(error["code"], code);
    }
    assert_eq!(snapshot(state), before_data);
    assert_audit(
        state,
        before,
        category,
        command,
        "REJECTED",
        "管理员",
        error["code"].as_str(),
    );
}

#[test]
fn template_group_ipc_actor_matrices_audit_denials_and_authorized_revisions_once() {
    for (command, category) in COMMANDS {
        let app = fixture();
        let state = app.state::<AppState>();
        let view = webview(&app);
        for (actor, operator, actor_code) in ACTORS {
            select_actor(&state, actor);
            let request = body(&state, command);
            let before_data = snapshot(&state);
            let before = command_audit(&state, command).len();
            let code = if actor == "owner" && command != "save_test_group" {
                Some("FORBIDDEN")
            } else {
                actor_code
            };
            let result = invoke_with_body(&view, command, request);
            if let Some(code) = code {
                assert_eq!(result.unwrap_err()["code"], code, "{command}/{actor}");
                assert_eq!(snapshot(&state), before_data);
                assert_audit(
                    &state,
                    before,
                    category,
                    command,
                    "REJECTED",
                    operator,
                    Some(code),
                );
            } else {
                result.unwrap();
                assert_audit(&state, before, category, command, "SUCCESS", operator, None);
                let after = snapshot(&state);
                assert_eq!(after["trials"], before_data["trials"]);
                if command == "save_test_group" {
                    assert_eq!(
                        after["groups"][0]["revision"].as_u64().unwrap(),
                        before_data["groups"][0]["revision"].as_u64().unwrap() + 1
                    );
                    assert_eq!(
                        after["groups"][0]["owner_user_id"],
                        before_data["groups"][0]["owner_user_id"]
                    );
                } else if command == "save_custom_program" {
                    assert_eq!(
                        after["template_history"].as_array().unwrap().len(),
                        before_data["template_history"].as_array().unwrap().len() + 1
                    );
                } else {
                    assert_eq!(after["templates"], json!([]));
                    assert_eq!(after["template_history"], before_data["template_history"]);
                }
            }
        }
    }
}

#[test]
fn stale_invalid_and_protected_template_group_requests_never_replace_saved_evidence() {
    for (command, category) in COMMANDS {
        let app = fixture();
        let state = app.state::<AppState>();
        let view = webview(&app);
        select_actor(&state, "admin");
        for case in 0..3 {
            let mut request = body(&state, command);
            match (command, case) {
                ("save_custom_program", 0) => {
                    request["program"]["provenance"]["revision"] = json!(0)
                }
                ("save_custom_program", 1) => request["reason"] = json!(" "),
                ("save_custom_program", _) => request["program"]["id"] = json!("REDUCIBILITY"),
                ("delete_custom_program", 0) => request["id"] = json!("REDUCIBILITY"),
                ("delete_custom_program", _) => request["id"] = json!("custom-missing"),
                ("save_test_group", 0) => request["request"]["expected_revision"] = json!(0),
                ("save_test_group", 1) => request["request"]["same_batch_confirmed"] = json!(false),
                ("save_test_group", _) => request["request"]["reason"] = json!(" "),
                _ => unreachable!(),
            }
            reject(&state, &view, command, category, request, None);
        }
    }
}

#[test]
fn update_and_accepted_close_exclude_template_and_group_writes() {
    for updating in [true, false] {
        for (command, category) in COMMANDS {
            let app = fixture();
            let state = app.state::<AppState>();
            let view = webview(&app);
            select_actor(&state, "admin");
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
                category,
                body(&state, command),
                Some(code),
            );
            drop(lease);
        }
    }
}

#[test]
fn existing_running_template_and_pending_group_guards_preserve_stored_versions() {
    for (command, category) in COMMANDS {
        let app = fixture();
        let state = app.state::<AppState>();
        let view = webview(&app);
        select_actor(&state, "admin");
        if command == "save_test_group" {
            lock(&state.exp.recovery_errors).push("unsaved trial".into());
        } else {
            state.exp.set_status(ExperimentStatus {
                running: true,
                phase: Phase::Running,
                ..ExperimentStatus::idle()
            });
        }
        reject(
            &state,
            &view,
            command,
            category,
            body(&state, command),
            None,
        );
    }
}

#[test]
fn sqlite_template_group_write_faults_are_failures_with_full_rollback() {
    for (command, category) in COMMANDS {
        let app = fixture();
        let state = app.state::<AppState>();
        let view = webview(&app);
        select_actor(&state, "admin");
        let event = match command {
            "save_custom_program" => "INSERT ON custom_program_revisions",
            "delete_custom_program" => "DELETE ON custom_programs",
            _ => "INSERT ON test_group_revisions",
        };
        rusqlite::Connection::open(database_path(&state)).unwrap().execute_batch(&format!(
            "CREATE TRIGGER deny_revision BEFORE {event} BEGIN SELECT RAISE(ABORT,'controlled revision fault'); END;"
        )).unwrap();
        let before_data = snapshot(&state);
        let before = command_audit(&state, command).len();
        let error = invoke_with_body(&view, command, body(&state, command)).unwrap_err();
        assert_eq!(error["code"], "STORAGE_ERROR");
        assert!(!error.to_string().contains("controlled revision fault"));
        assert_eq!(snapshot(&state), before_data);
        assert_audit(
            &state,
            before,
            category,
            command,
            "FAILURE",
            "管理员",
            Some("STORAGE_ERROR"),
        );
    }
}

#[test]
fn authentication_storage_faults_are_audited_for_template_and_group_commands() {
    let app = fixture();
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
    for ((command, category), request) in requests {
        let before_data = snapshot(&state);
        let before = command_audit(&state, command).len();
        let error = invoke_with_body(&view, command, request).unwrap_err();
        assert_eq!(error["code"], "STORAGE_ERROR");
        assert_eq!(snapshot(&state), before_data);
        assert_audit(
            &state,
            before,
            category,
            command,
            "FAILURE",
            "anonymous",
            Some("STORAGE_ERROR"),
        );
    }
}
