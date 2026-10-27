//! Verify protected read-only commands at the real Tauri IPC boundary.
use super::device_control_authorization_tests::{invoke_with_body, select_actor, webview, ACTORS};
use super::*;
use serde_json::{json, Value};
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};

fn fixture() -> tauri::App<MockRuntime> {
    let state = test_state();
    seed_users_and_experiments(&state);
    mock_builder()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            touch_session,
            get_runtime_mode,
            get_gas_safety_limits,
            get_runtime_settings,
            get_device_health,
            unsubscribe_telemetry,
            get_experiment_state,
            list_programs,
            preflight_experiment,
            generate_sample_id,
            get_experiment_start_defaults,
            get_updater_status,
            subscribe_telemetry
        ])
        .build(mock_context(noop_assets()))
        .unwrap()
}

fn expected_auth_error(actor: &str) -> Option<&'static str> {
    match actor {
        "anonymous" | "locked" => Some("AUTH_REQUIRED"),
        "password_change" => Some("PASSWORD_CHANGE_REQUIRED"),
        "owner" | "other" | "admin" => None,
        _ => unreachable!(),
    }
}

fn assert_result_for_actor(actor: &str, result: Result<Value, Value>, command: &str) {
    if let Some(code) = expected_auth_error(actor) {
        let error = match result {
            Err(error) => error,
            Ok(value) => panic!("command={command}, actor={actor} unexpectedly succeeded: {value}"),
        };
        assert_eq!(error["code"], code, "command={command}, actor={actor}");
    } else {
        result.unwrap_or_else(|error| panic!("command={command}, actor={actor}: {error}"));
    }
}

fn restore_temporary_password_state(state: &AppState) {
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

#[test]
fn protected_read_commands_enforce_session_states_through_real_ipc() {
    let app = fixture();
    let state = app.state::<AppState>();
    let view = webview(&app);

    for command in [
        "get_runtime_mode",
        "get_gas_safety_limits",
        "get_runtime_settings",
        "get_device_health",
        "unsubscribe_telemetry",
        "get_experiment_state",
        "list_programs",
        "get_updater_status",
    ] {
        for (actor, _, _) in ACTORS {
            select_actor(&state, actor);
            let result = invoke_with_body(&view, command, json!({}));
            assert_result_for_actor(actor, result, command);
        }
        restore_temporary_password_state(&state);
    }
}

#[test]
fn session_touch_remains_available_while_a_user_changes_their_temporary_password() {
    let app = fixture();
    let state = app.state::<AppState>();
    let view = webview(&app);

    for (actor, _, _) in ACTORS {
        select_actor(&state, actor);
        let result = invoke_with_body(&view, "touch_session", json!({}));
        let expected = match actor {
            "anonymous" | "locked" => Some("AUTH_REQUIRED"),
            "owner" | "other" | "admin" | "password_change" => None,
            _ => unreachable!(),
        };
        if let Some(code) = expected {
            assert_eq!(result.unwrap_err()["code"], code, "actor={actor}");
        } else {
            result.unwrap_or_else(|error| panic!("actor={actor}: {error}"));
        }
    }
}

#[test]
fn experiment_setup_queries_authorize_before_looking_up_programs() {
    let app = fixture();
    let state = app.state::<AppState>();
    let view = webview(&app);

    let params = json!({
        "sample_id": "S-IPC-001",
        "sample_name": "授权边界测试",
        "sample_weight": 100.0,
        "experiment_type": "REDUCIBILITY",
        "project_name": null,
        "notes": null
    });

    for (actor, _, _) in ACTORS {
        select_actor(&state, actor);
        for (command, body) in [
            ("preflight_experiment", json!({"params": params})),
            (
                "generate_sample_id",
                json!({"program_id": "REDUCIBILITY", "experiment_time": "2026-10-27T00:00:00"}),
            ),
            (
                "get_experiment_start_defaults",
                json!({"program_id": "REDUCIBILITY"}),
            ),
        ] {
            let result = invoke_with_body(&view, command, body);
            assert_result_for_actor(actor, result, command);
        }

        if let Some(code) = expected_auth_error(actor) {
            for (command, body) in [
                (
                    "generate_sample_id",
                    json!({"program_id": "unknown-program", "experiment_time": "2026-10-27T00:00:00"}),
                ),
                (
                    "get_experiment_start_defaults",
                    json!({"program_id": "unknown-program"}),
                ),
            ] {
                let error = invoke_with_body(&view, command, body).unwrap_err();
                assert_eq!(error["code"], code, "command={command}, actor={actor}");
            }
        }
    }
}

#[test]
fn telemetry_subscription_enforces_session_states_through_real_ipc() {
    let app = fixture();
    let state = app.state::<AppState>();
    let view = webview(&app);

    for (actor, _, _) in ACTORS {
        select_actor(&state, actor);
        let result = invoke_with_body(
            &view,
            "subscribe_telemetry",
            json!({"channel": "__CHANNEL__:7001"}),
        );
        assert_result_for_actor(actor, result, "subscribe_telemetry");
        if expected_auth_error(actor).is_none() {
            invoke_with_body(&view, "unsubscribe_telemetry", json!({})).unwrap();
        }
    }
}
