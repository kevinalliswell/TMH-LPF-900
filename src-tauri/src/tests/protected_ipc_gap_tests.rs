//! Real IPC coverage for protected command entry points missing from earlier matrices.
use super::device_control_authorization_tests::{
    assert_audit, command_audit, invoke_with_body, run_owned_by_alice, select_actor, wait_until,
    webview, ACTORS,
};
use super::group_tests::{native_group_insert_trial, native_group_request};
use super::*;
use serde_json::{json, Value};
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};

const CUSTOM: &str = "custom-protected-ipc-gap";
const TRIALS: [&str; 2] = ["protected-ipc-trial-a", "protected-ipc-trial-b"];

fn fixture() -> tauri::App<MockRuntime> {
    let state = test_state();
    seed_users_and_experiments(&state);

    login(&state, "admin", "admin-password");
    custom_programs::save(&state, f06_native_draft(CUSTOM), "Seed shared template").unwrap();

    let program = standard_program("REDUCIBILITY").unwrap();
    for (index, id) in TRIALS.iter().enumerate() {
        native_group_insert_trial(&state, id, "exp-alice", index as u32, &program, true, true);
    }
    login(&state, "alice", "temp-password");
    groups::save(
        &state,
        native_group_request(None, None, &TRIALS, "Seed owner group"),
    )
    .unwrap();
    state.auth.logout();

    mock_builder()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            set_gas_on_off,
            open_all_valves,
            close_all_valves,
            skip_stage,
            acknowledge_stage,
            list_custom_program_revisions,
            preview_test_group,
            list_test_groups,
            get_test_group
        ])
        .build(mock_context(noop_assets()))
        .unwrap()
}

fn prepare_mock_flows(state: &AppState) {
    wait_until(|| state.hub.mfc_rx().borrow().is_fresh(Instant::now()));
    for gas in 0..GAS_COUNT {
        let clamp = state
            .hub
            .request_set_flow(gas, 1.0)
            .blocking_recv()
            .unwrap()
            .unwrap();
        assert_eq!(clamp.applied, 1.0);
    }
}

fn gas_body(command: &str) -> Value {
    match command {
        "set_gas_on_off" => json!({"gas": 0, "on": true}),
        "open_all_valves" | "close_all_valves" => json!({}),
        _ => unreachable!(),
    }
}

#[test]
fn gas_valve_ipc_actor_matrix_audits_denials_without_changing_mock_bus_state() {
    for (command, audit_action) in [
        ("set_gas_on_off", "on_off"),
        ("open_all_valves", "open_all"),
        ("close_all_valves", "close_all"),
    ] {
        let app = fixture();
        let state = app.state::<AppState>();
        let view = webview(&app);
        prepare_mock_flows(&state);
        run_owned_by_alice(&state);
        let mut status = state.exp.snapshot();
        status.stage_index = 2;
        status.stage_kind = Some(StageKind::Reducing);
        state.exp.set_status(status);

        for (actor, operator, actor_code) in ACTORS {
            select_actor(&state, actor);
            let before_command_state = state.hub.mfc_rx().borrow().gas.map(|gas| (gas.sv, gas.on));
            let before_audit = command_audit(&state, audit_action).len();
            let result = invoke_with_body(&view, command, gas_body(command));
            let code = if matches!(actor, "owner" | "admin") {
                None
            } else if actor == "other" {
                Some("FORBIDDEN")
            } else {
                actor_code
            };

            if let Some(code) = code {
                let error = match result {
                    Err(error) => error,
                    Ok(value) => panic!("{command}/{actor} unexpectedly succeeded: {value}"),
                };
                assert_eq!(error["code"], code, "{command}/{actor}");
                // Mock PV is live telemetry and can keep moving toward SV while the
                // denied request is handled. Only command-controlled state must stay
                // unchanged across an authorization rejection.
                let after_command_state =
                    state.hub.mfc_rx().borrow().gas.map(|gas| (gas.sv, gas.on));
                assert_eq!(after_command_state, before_command_state);
                assert_audit(
                    &state,
                    before_audit,
                    "GAS",
                    audit_action,
                    "REJECTED",
                    operator,
                    Some(code),
                );
            } else {
                result.unwrap_or_else(|error| panic!("{command}/{actor}: {error}"));
                let after_gas = state.hub.mfc_rx().borrow().gas;
                match command {
                    "set_gas_on_off" => assert!(after_gas[0].on),
                    "open_all_valves" => assert!(after_gas.iter().all(|gas| gas.on)),
                    "close_all_valves" => assert!(after_gas.iter().all(|gas| !gas.on)),
                    _ => unreachable!(),
                }
                assert_audit(
                    &state,
                    before_audit,
                    "GAS",
                    audit_action,
                    "SUCCESS",
                    operator,
                    None,
                );
                for gas in 0..GAS_COUNT {
                    state
                        .hub
                        .request_set_on_off(gas, false)
                        .blocking_recv()
                        .unwrap()
                        .unwrap();
                }
            }
        }
    }
}

fn stage_body(command: &str) -> Value {
    match command {
        "skip_stage" => json!({
            "experiment_id": "exp-alice",
            "stage_index": 2,
            "reason": "approved deviation"
        }),
        "acknowledge_stage" => json!({
            "experiment_id": "exp-alice",
            "stage_index": 2,
            "notes": "operator confirmed"
        }),
        _ => unreachable!(),
    }
}

#[test]
fn stage_action_ipc_actor_matrix_preserves_denials_and_attributes_queued_evidence() {
    for (command, kind) in [
        ("skip_stage", StageKind::Reducing),
        ("acknowledge_stage", StageKind::Manual),
    ] {
        let app = fixture();
        let state = app.state::<AppState>();
        let view = webview(&app);
        run_owned_by_alice(&state);
        let mut status = state.exp.snapshot();
        status.stage_index = 2;
        status.stage_kind = Some(kind);
        state.exp.set_status(status);

        for (actor, operator, actor_code) in ACTORS {
            *lock(&state.exp.stage_action) = None;
            select_actor(&state, actor);
            let before_audit = command_audit(&state, "stage_action_request").len();
            let result = invoke_with_body(&view, command, stage_body(command));
            let code = if matches!(actor, "owner" | "admin") {
                None
            } else if actor == "other" {
                Some("FORBIDDEN")
            } else {
                actor_code
            };

            if let Some(code) = code {
                let error = match result {
                    Err(error) => error,
                    Ok(value) => panic!("{command}/{actor} unexpectedly succeeded: {value}"),
                };
                assert_eq!(error["code"], code, "{command}/{actor}");
                assert!(lock(&state.exp.stage_action).is_none());
                assert_audit(
                    &state,
                    before_audit,
                    "EXPERIMENT",
                    "stage_action_request",
                    "REJECTED",
                    operator,
                    Some(code),
                );
            } else {
                result.unwrap_or_else(|error| panic!("{command}/{actor}: {error}"));
                let action = lock(&state.exp.stage_action);
                let action = action.as_ref().expect("authorized action is queued");
                assert_eq!(action.experiment_id, "exp-alice");
                assert_eq!(action.stage_index, 2);
                assert_eq!(action.operator, operator);
                assert_eq!(
                    matches!(action.kind, procedure::ActionKind::Skip),
                    command == "skip_stage"
                );
                assert_audit(
                    &state,
                    before_audit,
                    "EXPERIMENT",
                    "stage_action_request",
                    "SUCCESS",
                    operator,
                    None,
                );
            }
        }
    }
}

fn read_snapshot(state: &AppState) -> Value {
    let db = lock(&state.storage);
    json!({
        "template_history": db.custom_program_revisions(CUSTOM).unwrap(),
        "groups": db.list_test_groups(None, 0).unwrap(),
        "group_history": db.test_group_history(
            &db.list_test_groups(None, 0).unwrap()[0].group_id,
            None
        ).unwrap(),
    })
}

fn request_body(state: &AppState, command: &str) -> Value {
    match command {
        "list_custom_program_revisions" => json!({"id": CUSTOM}),
        "preview_test_group" => json!({
            "request": native_group_request(None, None, &TRIALS, "Preview owner evidence")
        }),
        "list_test_groups" => json!({"offset": 0}),
        "get_test_group" => {
            let group = lock(&state.storage)
                .list_test_groups(None, 0)
                .unwrap()
                .remove(0);
            json!({"id": group.group_id, "revision": group.revision})
        }
        _ => unreachable!(),
    }
}

#[test]
fn protected_template_and_group_reads_enforce_actor_matrix_and_terminal_audit() {
    for (command, category) in [
        ("list_custom_program_revisions", "CONFIG"),
        ("preview_test_group", "EXPERIMENT"),
        ("list_test_groups", "EXPERIMENT"),
        ("get_test_group", "EXPERIMENT"),
    ] {
        let app = fixture();
        let state = app.state::<AppState>();
        let view = webview(&app);

        for (actor, operator, auth_code) in ACTORS {
            select_actor(&state, actor);
            let before_data = read_snapshot(&state);
            let before_audit = command_audit(&state, command).len();
            let result = invoke_with_body(&view, command, request_body(&state, command));
            let code = if matches!(
                command,
                "list_custom_program_revisions" | "list_test_groups"
            ) && matches!(actor, "owner" | "other" | "admin")
            {
                None
            } else {
                auth_code.or_else(|| (actor == "other").then_some("FORBIDDEN"))
            };

            if let Some(code) = code {
                let error = match result {
                    Err(error) => error,
                    Ok(value) => panic!("{command}/{actor} unexpectedly succeeded: {value}"),
                };
                assert_eq!(error["code"], code, "{command}/{actor}");
                assert_audit(
                    &state,
                    before_audit,
                    category,
                    command,
                    "REJECTED",
                    operator,
                    Some(code),
                );
            } else {
                let value = result.unwrap_or_else(|error| panic!("{command}/{actor}: {error}"));
                match command {
                    "list_custom_program_revisions" => {
                        assert_eq!(value.as_array().unwrap().len(), 1)
                    }
                    "preview_test_group" => {
                        assert_eq!(value["trials"].as_array().unwrap().len(), 2)
                    }
                    "list_test_groups" => assert_eq!(
                        value.as_array().unwrap().len(),
                        if actor == "other" { 0 } else { 1 }
                    ),
                    "get_test_group" => assert_eq!(value["revision"], 1),
                    _ => unreachable!(),
                }
                assert_audit(
                    &state,
                    before_audit,
                    category,
                    command,
                    "SUCCESS",
                    operator,
                    None,
                );
            }
            assert_eq!(
                read_snapshot(&state),
                before_data,
                "read changed stored evidence"
            );
        }
    }
}
