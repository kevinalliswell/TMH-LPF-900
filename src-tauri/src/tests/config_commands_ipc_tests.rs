//! Exercise communication-config commands through Tauri's registered IPC boundary.
use super::device_control_authorization_tests::{
    assert_audit, command_audit, invoke_with_body, select_actor, webview, ACTORS,
};
use super::*;
use serde_json::{json, Value};
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};

fn config_fixture() -> tauri::App<MockRuntime> {
    let state = test_state();
    seed_users_and_experiments(&state);
    mock_builder()
        .manage(state)
        .invoke_handler(tauri::generate_handler![get_comm_config, save_comm_config])
        .build(mock_context(noop_assets()))
        .unwrap()
}

fn invoke(
    view: &tauri::WebviewWindow<MockRuntime>,
    command: &str,
    body: Value,
) -> Result<Value, Value> {
    invoke_with_body(view, command, body)
}

fn assert_error(response: Result<Value, Value>, code: &str) {
    let error = response.unwrap_err();
    assert_eq!(error["code"], code);
    assert!(!error["message_zh"].as_str().unwrap_or_default().is_empty());
    assert_eq!(error.get("detail"), Some(&Value::Null));
    for secret in ["admin-password", "temp-password", "$argon2"] {
        assert!(!error.to_string().contains(secret));
    }
}

fn updated_config(saved: &Value) -> Value {
    let mut config = saved.clone();
    config["SAMPLING"]["interval_s"] = json!(2.0);
    config["PERFORMANCE_CONFIG"]["experiment_data_interval"] = json!(2.0);
    config
}

fn save_body(revision: &str, config: Value) -> Value {
    json!({
        "expected_revision": revision,
        "config": config,
        "reason": "IPC authorization and audit regression"
    })
}

#[test]
fn get_comm_config_enforces_all_six_session_states_through_real_ipc() {
    let app = config_fixture();
    let state = app.state::<AppState>();
    let view = webview(&app);

    for (actor, _, _) in ACTORS {
        select_actor(&state, actor);
        let response = invoke(&view, "get_comm_config", json!({}));
        let expected = match actor {
            "anonymous" | "locked" => Some("AUTH_REQUIRED"),
            "password_change" => Some("PASSWORD_CHANGE_REQUIRED"),
            _ => None,
        };
        if let Some(code) = expected {
            assert_error(response, code);
        } else {
            let snapshot = response.expect(actor);
            assert!(snapshot["revision"].as_str().is_some());
            assert!(snapshot["saved"]["SAMPLING"]["interval_s"].is_number());
        }
    }
}

#[test]
fn save_comm_config_enforces_authorization_validation_lifecycle_and_audit_over_ipc() {
    let app = config_fixture();
    let state = app.state::<AppState>();
    let view = webview(&app);
    select_actor(&state, "admin");
    let initial = invoke(&view, "get_comm_config", json!({})).unwrap();
    let revision = initial["revision"].as_str().unwrap().to_string();
    let valid = updated_config(&initial["saved"]);

    for (actor, operator, code) in ACTORS.into_iter().filter(|(actor, _, _)| *actor != "admin") {
        select_actor(&state, actor);
        let before = command_audit(&state, "save_comm_config").len();
        let expected_code = match actor {
            "owner" | "other" => "FORBIDDEN",
            _ => code.expect("denied actor has an authentication error"),
        };
        assert_error(
            invoke(
                &view,
                "save_comm_config",
                save_body(&revision, valid.clone()),
            ),
            expected_code,
        );
        assert_audit(
            &state,
            before,
            "CONFIG",
            "save_comm_config",
            "REJECTED",
            operator,
            Some(expected_code),
        );
    }

    select_actor(&state, "admin");
    let before = command_audit(&state, "save_comm_config").len();
    let mut invalid = valid.clone();
    invalid["GAS_SAFETY_LIMITS"]["H2"] = json!(6.0);
    assert_error(
        invoke(&view, "save_comm_config", save_body(&revision, invalid)),
        "INVALID_PARAMS",
    );
    assert_audit(
        &state,
        before,
        "CONFIG",
        "save_comm_config",
        "REJECTED",
        "管理员",
        Some("INVALID_PARAMS"),
    );

    let before = command_audit(&state, "save_comm_config").len();
    let saved = invoke(
        &view,
        "save_comm_config",
        save_body(&revision, valid.clone()),
    )
    .expect("administrator save succeeds through IPC");
    assert_eq!(saved["saved"]["SAMPLING"]["interval_s"], 2.0);
    assert_eq!(saved["applied"]["SAMPLING"]["interval_s"], 1.0);
    assert_eq!(saved["restart_required"], true);
    assert_audit(
        &state,
        before,
        "CONFIG",
        "save_comm_config",
        "SUCCESS",
        "管理员",
        None,
    );

    state.exp.set_status(ExperimentStatus {
        running: true,
        ..ExperimentStatus::idle()
    });
    let before = command_audit(&state, "save_comm_config").len();
    let mut later = valid;
    later["SAMPLING"]["interval_s"] = json!(3.0);
    later["PERFORMANCE_CONFIG"]["experiment_data_interval"] = json!(3.0);
    assert_error(
        invoke(
            &view,
            "save_comm_config",
            save_body(saved["revision"].as_str().unwrap(), later),
        ),
        "EXPERIMENT_RUNNING",
    );
    assert_audit(
        &state,
        before,
        "CONFIG",
        "save_comm_config",
        "REJECTED",
        "管理员",
        Some("EXPERIMENT_RUNNING"),
    );
}
