//! Exercise the authorization and audit boundary with real IPC and mock bus actors.
use super::*;
use serde_json::{json, Value};
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};

fn app_fixture() -> tauri::App<MockRuntime> {
    let state = test_state();
    seed_users_and_experiments(&state);
    mock_builder()
        .manage(state)
        .invoke_handler(tauri::generate_handler![set_gas_flow])
        .build(mock_context(noop_assets()))
        .unwrap()
}

fn webview(app: &tauri::App<MockRuntime>) -> tauri::WebviewWindow<MockRuntime> {
    tauri::WebviewWindowBuilder::new(app, "main", Default::default())
        .build()
        .unwrap()
}

fn set_flow(
    view: &tauri::WebviewWindow<MockRuntime>,
    gas: usize,
    value: f64,
) -> Result<Value, Value> {
    tauri::test::get_ipc_response(
        view,
        tauri::webview::InvokeRequest {
            cmd: "set_gas_flow".into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: if cfg!(any(windows, target_os = "android")) {
                "http://tauri.localhost"
            } else {
                "tauri://localhost"
            }
            .parse()
            .unwrap(),
            body: tauri::ipc::InvokeBody::Json(json!({"gas": gas, "value": value})),
            headers: Default::default(),
            invoke_key: tauri::test::INVOKE_KEY.to_string(),
        },
    )
    .map(|body| body.deserialize::<Value>().unwrap())
}

fn gas_audit(state: &AppState) -> Vec<Value> {
    let mut paths: Vec<_> = std::fs::read_dir(state.recovery_dir.parent().unwrap().join("logs"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    paths.sort();
    paths
        .into_iter()
        .flat_map(|path| {
            std::fs::read_to_string(path)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .filter(|entry| entry["category"] == "GAS")
                .collect::<Vec<_>>()
        })
        .collect()
}

fn assert_audit(
    state: &AppState,
    before: usize,
    operator: &str,
    gas: usize,
    requested: f64,
    outcome: Result<f64, &str>,
) {
    let entries = gas_audit(state);
    assert_eq!(entries.len(), before + 1, "one complete outcome per invoke");
    let entry = entries.last().unwrap();
    assert_eq!(entry["category"], "GAS");
    assert_eq!(entry["action"], "set_flow");
    assert_eq!(
        entry["result"],
        if outcome.is_ok() {
            "SUCCESS"
        } else {
            "REJECTED"
        }
    );
    assert_eq!(entry["operator"], operator);
    match outcome {
        Err(code) => {
            assert_eq!(entry["detail"]["code"], code);
            assert_eq!(entry["detail"]["request"]["gas"], gas);
            assert_eq!(entry["detail"]["request"]["requested"], requested);
        }
        Ok(applied) => {
            assert_eq!(entry["detail"]["gas"], gas);
            assert_eq!(entry["detail"]["requested"], requested);
            assert_eq!(entry["detail"]["applied"], json!(applied));
        }
    }
    let text = serde_json::to_string(&entries).unwrap();
    for secret in ["admin-password", "temp-password", "$argon2"] {
        assert!(
            !text.contains(secret),
            "gas audit must not expose credentials"
        );
    }
}

/// The N2 ACK drains earlier ordinary requests. Its changed snapshot prevents a stale
/// watch value from concealing a rejected request that accidentally reached the bus.
fn settled_flammable_setpoints(state: &AppState) -> [f64; 2] {
    let barrier = if state.hub.mfc_rx().borrow().gas[1].sv == 1.0 {
        2.0
    } else {
        1.0
    };
    let acknowledged = state
        .hub
        .request_set_flow(1, barrier)
        .blocking_recv()
        .unwrap()
        .unwrap();
    assert_eq!(acknowledged.applied, barrier);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let snapshot = state.hub.mfc_rx().borrow().clone();
        if snapshot.gas[1].sv == barrier {
            return [snapshot.gas[0].sv, snapshot.gas[3].sv];
        }
        assert!(
            Instant::now() < deadline,
            "mock MFC snapshot did not advance"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn assert_error(error: &Value, code: &str) {
    assert_eq!(error["code"], code);
    assert!(!error["message_zh"].as_str().unwrap().trim().is_empty());
    assert_eq!(error.get("detail"), Some(&Value::Null));
}

#[test]
fn running_gas_ipc_enforces_all_six_actor_states_and_audits_each_outcome() {
    let app = app_fixture();
    let state = app.state::<AppState>();
    let view = webview(&app);
    login(&state, "admin", "admin-password");
    for gas in [0, 3] {
        set_flow(&view, gas, 1.0).unwrap();
    }
    assert_eq!(settled_flammable_setpoints(&state), [1.0, 1.0]);
    let alice = lock(&state.storage)
        .get_user_by_username("alice")
        .unwrap()
        .unwrap();
    state.exp.set_status(ExperimentStatus {
        running: true,
        phase: Phase::Running,
        experiment_id: Some("exp-alice".into()),
        owner_user_id: Some(alice.user_id),
        owner_display_name: Some("爱丽丝".into()),
        ..ExperimentStatus::idle()
    });

    let mut expected_setpoints = [1.0, 1.0];
    for (actor, operator, code, requested) in [
        ("anonymous", "anonymous", Some("AUTH_REQUIRED"), 4.0),
        ("owner", "爱丽丝", None, 2.0),
        ("other", "鲍勃", Some("FORBIDDEN"), 4.0),
        ("admin", "管理员", None, 3.0),
        ("locked", "anonymous", Some("AUTH_REQUIRED"), 4.0),
        (
            "password_change",
            "鲍勃",
            Some("PASSWORD_CHANGE_REQUIRED"),
            4.0,
        ),
    ] {
        match actor {
            "anonymous" => state.auth.logout(),
            "owner" => login(&state, "alice", "temp-password"),
            "other" => login(&state, "bob", "temp-password"),
            "admin" => login(&state, "admin", "admin-password"),
            "locked" => {
                login(&state, "alice", "temp-password");
                state.auth.lock();
            }
            "password_change" => {
                let storage = lock(&state.storage);
                let bob = storage.get_user_by_username("bob").unwrap().unwrap();
                storage
                    .set_user_password(
                        &bob.user_id,
                        &bob.password_hash,
                        true,
                        "2026-09-01T00:00:00",
                    )
                    .unwrap();
                drop(storage);
                login(&state, "bob", "temp-password");
            }
            _ => unreachable!(),
        }
        for (index, gas) in [0, 3].into_iter().enumerate() {
            let before = gas_audit(&state).len();
            let result = set_flow(&view, gas, requested);
            if let Some(code) = code {
                assert_error(&result.unwrap_err(), code);
                assert_audit(&state, before, operator, gas, requested, Err(code));
            } else {
                let response = result.unwrap();
                assert_eq!(response["applied"], requested);
                assert_eq!(response["clamped"], false);
                expected_setpoints[index] = requested;
                assert_audit(&state, before, operator, gas, requested, Ok(requested));
            }
            assert_eq!(
                settled_flammable_setpoints(&state),
                expected_setpoints,
                "{actor}: a rejected gas request must not change either flammable setpoint"
            );
        }
    }
}

#[test]
fn invalid_gas_and_negative_flow_are_rejected_without_changing_confirmed_setpoints() {
    let app = app_fixture();
    let state = app.state::<AppState>();
    let view = webview(&app);
    login(&state, "admin", "admin-password");
    for gas in [0, 3] {
        set_flow(&view, gas, 2.0).unwrap();
    }
    assert_eq!(settled_flammable_setpoints(&state), [2.0, 2.0]);
    for (gas, value) in [(0, -1.0), (3, -1.0), (4, 1.0)] {
        let before = gas_audit(&state).len();
        assert_error(&set_flow(&view, gas, value).unwrap_err(), "INVALID_PARAMS");
        assert_audit(&state, before, "管理员", gas, value, Err("INVALID_PARAMS"));
        assert_eq!(settled_flammable_setpoints(&state), [2.0, 2.0]);
    }
}

#[test]
fn successful_flammable_writes_are_capped_at_five_and_audit_the_applied_value() {
    let app = app_fixture();
    let state = app.state::<AppState>();
    let view = webview(&app);
    login(&state, "admin", "admin-password");
    for gas in [0, 3] {
        let before = gas_audit(&state).len();
        let response = set_flow(&view, gas, 8.0).unwrap();
        assert_eq!(response["applied"], 5.0);
        assert_eq!(response["limit"], 5.0);
        assert_eq!(response["clamped"], true);
        assert_audit(&state, before, "管理员", gas, 8.0, Ok(5.0));
    }
    assert_eq!(settled_flammable_setpoints(&state), [5.0, 5.0]);
}
