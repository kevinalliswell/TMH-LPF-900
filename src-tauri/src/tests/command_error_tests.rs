//! Assert the public error contract after Tauri deserializes and dispatches a real invoke.
use super::*;
use serde_json::{json, Value};
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};

fn app_fixture() -> tauri::App<MockRuntime> {
    let state = test_state();
    seed_users_and_experiments(&state);
    mock_builder()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            list_experiments,
            get_experiment_detail,
            get_experiment_samples,
            delete_experiment,
            analyze_gb13241,
            analyze_gb13242,
            analyze_gb13240,
        ])
        .build(mock_context(noop_assets()))
        .unwrap()
}

fn webview(app: &tauri::App<MockRuntime>) -> tauri::WebviewWindow<MockRuntime> {
    tauri::WebviewWindowBuilder::new(app, "main", Default::default())
        .build()
        .unwrap()
}

fn invoke(
    webview: &tauri::WebviewWindow<MockRuntime>,
    command: &str,
    body: Value,
) -> Result<Value, Value> {
    tauri::test::get_ipc_response(
        webview,
        tauri::webview::InvokeRequest {
            cmd: command.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: if cfg!(any(windows, target_os = "android")) {
                "http://tauri.localhost"
            } else {
                "tauri://localhost"
            }
            .parse()
            .unwrap(),
            body: tauri::ipc::InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: tauri::test::INVOKE_KEY.to_string(),
        },
    )
    .map(|body| body.deserialize::<Value>().unwrap())
}

fn reduction_input() -> Value {
    json!({
        "w1": 0.0,
        "w2": 50.0,
        "manual_m1": 100.0,
        "manual_mt": 85.0,
        "source_record": "LAB-PRIVATE-SECRET"
    })
}

fn assert_command_error(error: &Value, code: &str) {
    assert!(
        error.is_object(),
        "command returned a non-DTO error: {error}"
    );
    assert_eq!(error["code"], code);
    let message = error["message_zh"]
        .as_str()
        .expect("command error includes a Chinese message");
    assert!(!message.trim().is_empty());
    assert!(
        !message.is_ascii(),
        "error needs a user-facing Chinese message"
    );
    assert_eq!(error.get("detail"), Some(&Value::Null));
    let serialized = error.to_string();
    for secret in ["admin-password", "temp-password", "LAB-PRIVATE-SECRET"] {
        assert!(!serialized.contains(secret), "command error exposed input");
    }
}

fn experiment_snapshot(state: &AppState, id: &str) -> Value {
    let storage = lock(&state.storage);
    json!({
        "experiment": storage.get_experiment(id).unwrap(),
        "samples": storage.get_samples(id, 0).unwrap(),
        "metadata": storage.runtime_metadata(id).unwrap(),
        "revisions": storage.analysis_revisions(id).unwrap(),
        "current_revision": storage.current_analysis_revision_id(id).unwrap(),
    })
}

#[test]
fn anonymous_protected_commands_return_auth_required_dto_through_real_ipc() {
    let app = app_fixture();
    let webview = webview(&app);
    for (command, body) in [
        ("list_experiments", json!({})),
        ("get_experiment_detail", json!({"id": "exp-alice"})),
        ("get_experiment_samples", json!({"id": "exp-alice"})),
        ("delete_experiment", json!({"id": "exp-alice"})),
        (
            "analyze_gb13241",
            json!({"id": "exp-alice", "input": reduction_input()}),
        ),
    ] {
        let error = invoke(&webview, command, body).unwrap_err();
        assert_command_error(&error, "AUTH_REQUIRED");
    }
}

#[test]
fn ordinary_user_admin_and_other_owner_commands_return_forbidden_dto() {
    let app = app_fixture();
    let state = app.state::<AppState>();
    login(&state, "bob", "temp-password");
    let webview = webview(&app);
    let before = experiment_snapshot(&state, "exp-alice");
    for (command, body) in [
        // Bob cannot delete even his own record: deletion requires an administrator.
        ("delete_experiment", json!({"id": "exp-bob"})),
        ("get_experiment_detail", json!({"id": "exp-alice"})),
        ("get_experiment_samples", json!({"id": "exp-alice"})),
        (
            "analyze_gb13241",
            json!({"id": "exp-alice", "input": reduction_input()}),
        ),
    ] {
        let error = invoke(&webview, command, body).unwrap_err();
        assert_command_error(&error, "FORBIDDEN");
    }
    assert_eq!(experiment_snapshot(&state, "exp-alice"), before);
    assert!(lock(&state.storage)
        .get_experiment("exp-bob")
        .unwrap()
        .is_some());
}

#[test]
fn invalid_analysis_input_returns_dto_and_preserves_saved_result_and_history() {
    let app = app_fixture();
    let state = app.state::<AppState>();
    login(&state, "alice", "temp-password");
    lock(&state.storage)
        .finish_experiment("exp-alice", "2026-09-01T12:00:00")
        .unwrap();
    let webview = webview(&app);
    invoke(
        &webview,
        "analyze_gb13241",
        json!({"id": "exp-alice", "input": reduction_input()}),
    )
    .expect("valid original-record masses produce a saved analysis");
    let before = experiment_snapshot(&state, "exp-alice");
    assert_eq!(before["revisions"].as_array().unwrap().len(), 1);
    let mut input = reduction_input();
    input["w2"] = json!(-50.0);
    let error = invoke(
        &webview,
        "analyze_gb13241",
        json!({"id": "exp-alice", "input": input}),
    )
    .unwrap_err();
    assert_eq!(experiment_snapshot(&state, "exp-alice"), before);
    assert_command_error(&error, "INVALID_PARAMS");
}

#[test]
fn unfinished_experiment_delete_returns_dto_and_preserves_record_and_samples() {
    let app = app_fixture();
    let state = app.state::<AppState>();
    login(&state, "admin", "admin-password");
    let mut sample = row("REDUCING", 1, 99.0, false);
    sample.experiment_id = "exp-alice".into();
    sample.record_json = Some(json!({"stage_elapsed_s": 60.0}).to_string());
    lock(&state.storage).insert_samples(&[sample]).unwrap();
    let before = experiment_snapshot(&state, "exp-alice");
    let webview = webview(&app);
    let error = invoke(&webview, "delete_experiment", json!({"id": "exp-alice"})).unwrap_err();
    assert_eq!(experiment_snapshot(&state, "exp-alice"), before);
    assert_command_error(&error, "INVALID_STATE");
}

#[test]
fn missing_tauri_argument_is_rejected_before_command_without_changing_database() {
    let app = app_fixture();
    let state = app.state::<AppState>();
    login(&state, "admin", "admin-password");
    let before = experiment_snapshot(&state, "exp-alice");
    let webview = webview(&app);
    for (command, body) in [
        ("delete_experiment", json!({})),
        ("analyze_gb13241", json!({"id": "exp-alice"})),
    ] {
        // Tauri owns argument decoding; its pre-dispatch rejection is not our DTO.
        let error = invoke(&webview, command, body).unwrap_err();
        assert!(
            error.is_string(),
            "expected a Tauri argument error: {error}"
        );
        assert_eq!(experiment_snapshot(&state, "exp-alice"), before);
    }
}

#[test]
fn analysis_type_and_insufficient_data_rejections_have_distinct_codes() {
    let app = app_fixture();
    let state = app.state::<AppState>();
    login(&state, "alice", "temp-password");
    lock(&state.storage)
        .finish_experiment("exp-alice", "2026-09-01T12:00:00")
        .unwrap();
    let before = experiment_snapshot(&state, "exp-alice");
    let view = webview(&app);
    let missing = invoke(
        &view,
        "analyze_gb13241",
        json!({"id":"exp-alice", "input":{"w1":0.0,"w2":50.0}}),
    )
    .unwrap_err();
    assert_command_error(&missing, "ANALYSIS_DATA_INSUFFICIENT");
    let wrong_type = invoke(&view, "analyze_gb13240", json!({"id":"exp-alice", "input":{"v0":1.0,"v1":1.2,"pellet_count":18,"volume_method":"volume","same_method":true}})).unwrap_err();
    assert_command_error(&wrong_type, "ANALYSIS_TYPE_MISMATCH");
    assert_eq!(experiment_snapshot(&state, "exp-alice"), before);
}
