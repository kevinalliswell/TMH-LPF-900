//! Exercise account-management authorization and DTO mapping through real Tauri IPC.
use super::super::login;
use super::device_control_authorization_tests::{
    assert_audit, command_audit, invoke_with_body, select_actor, webview, ACTORS,
};
use super::*;
use serde_json::{json, Value};
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};

fn management_fixture() -> (tauri::App<MockRuntime>, String) {
    let state = test_state();
    seed_users_and_experiments(&state);
    state
        .auth
        .login(
            &lock(&state.storage),
            "admin",
            "admin-password",
            epoch_now(),
        )
        .unwrap();
    let target = state
        .auth
        .create_user(
            &lock(&state.storage),
            "target",
            "Target",
            "target-password",
            UserRole::User,
            epoch_now(),
        )
        .unwrap();
    state.auth.logout();
    lock(&state.storage)
        .set_user_password(
            &target.user_id,
            &target.password_hash,
            false,
            "2026-10-27T00:00:00",
        )
        .unwrap();
    let target_id = target.user_id;
    let app = mock_builder()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            list_users,
            create_user,
            update_user,
            reset_user_password
        ])
        .build(mock_context(noop_assets()))
        .unwrap();
    (app, target_id)
}

fn auth_fixture() -> tauri::App<MockRuntime> {
    mock_builder()
        .manage(test_state())
        .invoke_handler(tauri::generate_handler![
            get_auth_state,
            bootstrap_admin,
            login,
            logout,
            lock_session,
            change_own_password
        ])
        .build(mock_context(noop_assets()))
        .unwrap()
}

fn assert_error(error: &Value, code: &str) {
    assert_eq!(error["code"], code);
    let message = error["message_zh"].as_str().expect("stable error DTO");
    assert!(!message.trim().is_empty());
    assert!(!message.is_ascii());
    assert_eq!(error.get("detail"), Some(&Value::Null));
    for secret in ["admin-password", "temp-password", "SecretForTest!"] {
        assert!(!error.to_string().contains(secret));
    }
}

fn account_snapshot(state: &AppState, target_id: &str) -> (i64, Value) {
    let storage = lock(&state.storage);
    let user = storage.get_user(target_id).unwrap().unwrap();
    (
        storage.user_count().unwrap(),
        json!({
            "username": user.username,
            "display_name": user.display_name,
            "role": user.role,
            "is_active": user.is_active,
            "must_change_password": user.must_change_password,
            "password_hash": user.password_hash,
        }),
    )
}

#[test]
fn account_management_commands_enforce_six_actor_states_through_real_ipc() {
    let (app, target_id) = management_fixture();
    let state = app.state::<AppState>();
    let view = webview(&app);

    for (command, action) in [
        ("list_users", None),
        ("create_user", Some("create_user")),
        ("update_user", Some("update_user")),
        ("reset_user_password", Some("reset_password")),
    ] {
        for (actor, operator, actor_code) in ACTORS {
            select_actor(&state, actor);
            let before = account_snapshot(&state, &target_id);
            let audit_before = action.map(|name| command_audit(&state, name).len());
            let body = match command {
                "list_users" => json!({}),
                "create_user" => json!({
                    "username":"ipc-created",
                    "display_name":"IPC Created",
                    "temporary_password":"SecretForTest!",
                    "role":"USER"
                }),
                "update_user" => json!({
                    "user_id":target_id,
                    "display_name":"Target Updated",
                    "role":"USER",
                    "is_active":true
                }),
                "reset_user_password" => json!({
                    "user_id":target_id,
                    "temporary_password":"SecretForTest!"
                }),
                _ => unreachable!(),
            };
            let result = invoke_with_body(&view, command, body);

            if actor == "admin" {
                let value = result.expect("administrator command should be allowed");
                if let Some(action) = action {
                    match command {
                        "create_user" => {
                            assert_eq!(value["username"], "ipc-created");
                            assert_eq!(value["must_change_password"], true);
                            assert!(value.get("password_hash").is_none());
                        }
                        "update_user" => {
                            assert_eq!(value["display_name"], "Target Updated");
                            assert!(value.get("password_hash").is_none());
                        }
                        "reset_user_password" => assert!(value.is_null()),
                        _ => unreachable!(),
                    }
                    assert_audit(
                        &state,
                        audit_before.unwrap(),
                        "AUTH",
                        action,
                        "SUCCESS",
                        operator,
                        None,
                    );
                } else {
                    assert!(value.is_array());
                    for user in value.as_array().unwrap() {
                        assert!(user.get("password_hash").is_none());
                    }
                }
            } else {
                let code = actor_code.unwrap_or("FORBIDDEN");
                assert_eq!(
                    result.as_ref().unwrap_err()["code"],
                    code,
                    "command={command}, actor={actor}"
                );
                assert_error(&result.unwrap_err(), code);
                if let Some(action) = action {
                    assert_audit(
                        &state,
                        audit_before.unwrap(),
                        "AUTH",
                        action,
                        "REJECTED",
                        operator,
                        Some(code),
                    );
                }
                assert_eq!(account_snapshot(&state, &target_id), before);
            }
        }

        // `select_actor("password_change")` changes the seeded account's stored
        // must-change flag; restore the fixture before the next command matrix.
        state.auth.logout();
        let storage = lock(&state.storage);
        let bob = storage.get_user_by_username("bob").unwrap().unwrap();
        storage
            .set_user_password(
                &bob.user_id,
                &bob.password_hash,
                false,
                "2026-10-27T00:00:00",
            )
            .unwrap();
    }

    for action in ["create_user", "update_user", "reset_password"] {
        assert!(!serde_json::to_string(&command_audit(&state, action))
            .unwrap()
            .contains("SecretForTest!"));
    }
}

#[test]
fn bootstrap_login_and_password_change_keep_auth_dtos_and_snake_case_arguments() {
    let app = auth_fixture();
    let state = app.state::<AppState>();
    let view = webview(&app);

    assert_eq!(
        invoke_with_body(&view, "get_auth_state", json!({})).unwrap()["status"],
        "SETUP_REQUIRED"
    );
    let before = command_audit(&state, "bootstrap_admin").len();
    let admin = invoke_with_body(
        &view,
        "bootstrap_admin",
        json!({
            "username":"admin",
            "display_name":"Administrator",
            "password":"FirstAdminPassword!"
        }),
    )
    .unwrap();
    assert_eq!(admin["username"], "admin");
    assert!(admin.get("password_hash").is_none());
    assert_audit(
        &state,
        before,
        "AUTH",
        "bootstrap_admin",
        "SUCCESS",
        "Administrator",
        None,
    );
    assert_eq!(
        invoke_with_body(&view, "get_auth_state", json!({})).unwrap()["status"],
        "AUTHENTICATED"
    );

    let duplicate = invoke_with_body(
        &view,
        "bootstrap_admin",
        json!({
            "username":"second",
            "display_name":"Second",
            "password":"AnotherAdminPassword!"
        }),
    )
    .unwrap_err();
    assert_error(&duplicate, "ALREADY_INITIALIZED");
    assert_audit(
        &state,
        before + 1,
        "AUTH",
        "bootstrap_admin",
        "REJECTED",
        "Administrator",
        Some("ALREADY_INITIALIZED"),
    );

    let before = command_audit(&state, "logout").len();
    invoke_with_body(&view, "logout", json!({})).unwrap();
    assert_audit(
        &state,
        before,
        "AUTH",
        "logout",
        "SUCCESS",
        "Administrator",
        None,
    );
    assert_eq!(
        invoke_with_body(&view, "get_auth_state", json!({})).unwrap()["status"],
        "ANONYMOUS"
    );
    let before = command_audit(&state, "login").len();
    let bad_login = invoke_with_body(
        &view,
        "login",
        json!({"username":"admin","password":"wrong-password"}),
    )
    .unwrap_err();
    assert_error(&bad_login, "AUTH_FAILED");
    assert_audit(
        &state,
        before,
        "AUTH",
        "login",
        "REJECTED",
        "admin",
        Some("AUTH_FAILED"),
    );

    let before = command_audit(&state, "login").len();
    let logged_in = invoke_with_body(
        &view,
        "login",
        json!({"username":"admin","password":"FirstAdminPassword!"}),
    )
    .unwrap();
    assert_eq!(logged_in["status"], "AUTHENTICATED");
    assert_audit(
        &state,
        before,
        "AUTH",
        "login",
        "SUCCESS",
        "Administrator",
        None,
    );
    let before = command_audit(&state, "change_password").len();
    invoke_with_body(
        &view,
        "change_own_password",
        json!({
            "current_password":"FirstAdminPassword!",
            "new_password":"ReplacementAdminPassword!"
        }),
    )
    .unwrap();
    assert_audit(
        &state,
        before,
        "AUTH",
        "change_password",
        "SUCCESS",
        "Administrator",
        None,
    );
    invoke_with_body(&view, "logout", json!({})).unwrap();
    assert_error(
        &invoke_with_body(
            &view,
            "login",
            json!({"username":"admin","password":"FirstAdminPassword!"}),
        )
        .unwrap_err(),
        "AUTH_FAILED",
    );
    assert_eq!(
        invoke_with_body(
            &view,
            "login",
            json!({"username":"admin","password":"ReplacementAdminPassword!"}),
        )
        .unwrap()["status"],
        "AUTHENTICATED"
    );
    let before = command_audit(&state, "lock_session").len();
    invoke_with_body(&view, "lock_session", json!({})).unwrap();
    assert_eq!(
        invoke_with_body(&view, "get_auth_state", json!({})).unwrap()["status"],
        "LOCKED"
    );
    assert_audit(
        &state,
        before,
        "AUTH",
        "lock_session",
        "SUCCESS",
        "Administrator",
        None,
    );

    let logs = command_audit(&state, "login");
    assert!(logs.iter().any(|entry| entry["result"] == "REJECTED"));
    let serialized = ["bootstrap_admin", "login", "logout", "change_password"]
        .into_iter()
        .flat_map(|action| command_audit(&state, action))
        .map(|entry| entry.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    for secret in [
        "wrong-password",
        "FirstAdminPassword!",
        "ReplacementAdminPassword!",
    ] {
        assert!(!serialized.contains(secret));
    }
}
