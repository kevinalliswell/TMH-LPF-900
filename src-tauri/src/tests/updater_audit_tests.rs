//! Update commands preserve their public status shape and audit every terminal outcome.
use super::device_control_authorization_tests::{
    assert_audit, command_audit, invoke_with_body, run_owned_by_alice, select_actor, webview,
    ACTORS,
};
use super::*;
use serde_json::{json, Value};
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};

fn fixture(updater_enabled: bool) -> tauri::App<MockRuntime> {
    let mut state = test_state();
    state.updater_enabled = updater_enabled;
    seed_users_and_experiments(&state);
    mock_builder()
        .manage(state)
        .invoke_handler(tauri::generate_handler![check_for_update, install_update])
        .build(mock_context(noop_assets()))
        .unwrap()
}

fn update_request(view: &tauri::WebviewWindow<MockRuntime>, command: &str) -> Result<Value, Value> {
    invoke_with_body(view, command, json!({}))
}

#[test]
fn disabled_check_keeps_its_response_shape_and_records_success() {
    let app = fixture(false);
    let state = app.state::<AppState>();
    let view = webview(&app);
    select_actor(&state, "admin");
    let before = command_audit(&state, "check_update").len();
    let result = update_request(&view, "check_for_update").unwrap();
    assert_eq!(result["enabled"], false);
    assert_eq!(result["available"], false);
    assert!(result["new_version"].is_null());
    assert!(result["notes"].is_null());
    assert!(result["error"].is_null());
    assert_audit(
        &state,
        before,
        "APP",
        "check_update",
        "SUCCESS",
        "管理员",
        None,
    );
}

#[test]
fn update_check_audits_authorization_for_all_six_actor_states() {
    let app = fixture(false);
    let state = app.state::<AppState>();
    let view = webview(&app);
    for (actor, operator, code) in ACTORS {
        select_actor(&state, actor);
        let before = command_audit(&state, "check_update").len();
        let result = update_request(&view, "check_for_update");
        if matches!(code, Some("AUTH_REQUIRED" | "PASSWORD_CHANGE_REQUIRED")) {
            let code = code.unwrap();
            assert_eq!(result.unwrap_err()["code"], code, "{actor}");
            assert_audit(
                &state,
                before,
                "APP",
                "check_update",
                "REJECTED",
                operator,
                Some(code),
            );
        } else {
            assert!(result.unwrap()["enabled"] == false, "{actor}");
            assert_audit(
                &state,
                before,
                "APP",
                "check_update",
                "SUCCESS",
                operator,
                None,
            );
        }
    }
}

#[test]
fn update_install_audits_authorization_and_disabled_rejection_for_all_actors() {
    let app = fixture(false);
    let state = app.state::<AppState>();
    let view = webview(&app);
    for (actor, operator, actor_code) in ACTORS {
        select_actor(&state, actor);
        let before = command_audit(&state, "install_update").len();
        let result = update_request(&view, "install_update");
        let code = match actor {
            "admin" => "UPDATE_DISABLED",
            "owner" => "FORBIDDEN",
            _ => actor_code.unwrap(),
        };
        assert_eq!(result.unwrap_err()["code"], code, "{actor}");
        assert_audit(
            &state,
            before,
            "APP",
            "install_update",
            "REJECTED",
            operator,
            Some(code),
        );
    }
}

#[test]
fn install_success_audit_finishes_before_restart_is_requested() {
    let app = fixture(false);
    let state = app.state::<AppState>();
    let before = command_audit(&state, "install_update").len();
    let mut audit_count_at_restart = 0;
    let mut restarted = false;
    let result = finish_update(app.handle(), "管理员".into(), Ok(()), || {
        audit_count_at_restart = command_audit(&state, "install_update").len();
        restarted = true;
    });
    assert!(result.is_ok());
    assert!(restarted);
    assert_eq!(audit_count_at_restart, before + 1);
    assert_audit(
        &state,
        before,
        "APP",
        "install_update",
        "SUCCESS",
        "管理员",
        None,
    );
}

#[test]
fn offline_update_check_is_reported_in_the_existing_status_shape_and_audited_as_failure() {
    let app = fixture(false);
    let state = app.state::<AppState>();
    let before = command_audit(&state, "check_update").len();
    let check = UpdateCheck {
        enabled: true,
        current_version: "0.2.0".into(),
        available: false,
        new_version: None,
        notes: None,
        error: Some("无法连接更新服务器（离线环境属正常）".into()),
    };
    finish_update_check(&state, "管理员".into(), &check);
    assert!(check.enabled);
    assert!(!check.available);
    assert!(check.error.is_some());
    assert_audit(
        &state,
        before,
        "APP",
        "check_update",
        "FAILURE",
        "管理员",
        Some("UPDATE_CHECK_FAILED"),
    );
}

#[test]
fn failed_install_is_audited_with_a_stable_code_without_restarting() {
    let app = fixture(false);
    let state = app.state::<AppState>();
    let before = command_audit(&state, "install_update").len();
    let mut restarted = false;
    let error = finish_update(
        app.handle(),
        "管理员".into(),
        Err(CommandError::from("UPDATE_FAILED")),
        || restarted = true,
    )
    .unwrap_err();
    assert_eq!(error.code, "UPDATE_FAILED");
    assert!(!restarted);
    assert_audit(
        &state,
        before,
        "APP",
        "install_update",
        "FAILURE",
        "管理员",
        Some("UPDATE_FAILED"),
    );
}

#[test]
fn running_experiment_blocks_install_with_audited_stable_state_code() {
    let app = fixture(true);
    let state = app.state::<AppState>();
    let view = webview(&app);
    run_owned_by_alice(&state);
    select_actor(&state, "admin");
    let before = command_audit(&state, "install_update").len();
    let error = update_request(&view, "install_update").unwrap_err();
    assert_eq!(error["code"], "EXPERIMENT_RUNNING");
    assert!(!state.exp.updating.load(Ordering::SeqCst));
    assert_audit(
        &state,
        before,
        "APP",
        "install_update",
        "REJECTED",
        "管理员",
        Some("EXPERIMENT_RUNNING"),
    );
}
