//! Production experiment controls must record one truthful outcome per request.
use super::device_control_authorization_tests::*;
use super::*;
use serde_json::json;

fn start_body(sample_id: &str) -> serde_json::Value {
    json!({"params": {
        "sample_id": sample_id, "sample_name": "audit specimen", "sample_weight": 100.0,
        "experiment_type": "REDUCIBILITY", "project_name": null, "notes": null
    }})
}

fn experiment_control_snapshot(state: &AppState) -> serde_json::Value {
    json!({
        "rows": lock(&state.storage).list_experiments().unwrap(),
        "status": state.exp.snapshot(),
        "stop": state.exp.stop.load(Ordering::SeqCst),
        "latch": state.hub.safety_latched(),
    })
}

#[test]
fn start_ipc_running_six_actor_matrix_audits_global_busy_without_changing_the_run() {
    let (app, _, _) = app_fixture();
    let state = app.state::<AppState>();
    let view = webview(&app);
    run_owned_by_alice(&state);
    for (actor, operator, auth_code) in ACTORS {
        select_actor(&state, actor);
        let before_state = experiment_control_snapshot(&state);
        let before = command_audit(&state, "start").len();
        let code = match actor {
            "owner" | "other" | "admin" => "EXPERIMENT_RUNNING",
            _ => auth_code.unwrap(),
        };
        let error =
            invoke_with_body(&view, "start_experiment", start_body("new-sample")).unwrap_err();
        assert_eq!(error["code"], code);
        assert_eq!(experiment_control_snapshot(&state), before_state);
        assert_audit(
            &state,
            before,
            "EXPERIMENT",
            "start",
            "REJECTED",
            operator,
            Some(code),
        );
    }
}

#[test]
fn invalid_idle_start_is_rejected_and_audited_without_inserting_an_experiment() {
    let (app, _, _) = app_fixture();
    let state = app.state::<AppState>();
    let view = webview(&app);
    select_actor(&state, "owner");
    let before_state = experiment_control_snapshot(&state);
    let before = command_audit(&state, "start").len();
    let error = invoke_with_body(&view, "start_experiment", start_body(" ")).unwrap_err();
    assert_eq!(error["code"], "INVALID_PARAMS");
    assert_eq!(experiment_control_snapshot(&state), before_state);
    assert_audit(
        &state,
        before,
        "EXPERIMENT",
        "start",
        "REJECTED",
        "爱丽丝",
        Some("INVALID_PARAMS"),
    );
}

fn finish_seeded_rows(state: &AppState) {
    for id in ["exp-admin", "exp-alice", "exp-bob"] {
        lock(&state.storage)
            .finish_experiment(id, "2026-10-26T10:00:00")
            .unwrap();
    }
}

fn reset_latch(state: &AppState) {
    if state.hub.safety_latched() {
        wait_until(|| state.hub.mfc_rx().borrow().is_fresh(Instant::now()));
        state
            .hub
            .request_reset_safety()
            .blocking_recv()
            .unwrap()
            .unwrap();
    }
}

#[test]
fn stop_ipc_six_actor_matrix_audits_once_and_preserves_state_on_denial() {
    let (app, _, _) = app_fixture();
    let state = app.state::<AppState>();
    let view = webview(&app);
    run_owned_by_alice(&state);
    for (actor, operator, code) in ACTORS {
        reset_latch(&state);
        state.exp.stop.store(false, Ordering::SeqCst);
        select_actor(&state, actor);
        let before = command_audit(&state, "stop").len();
        let result = invoke_with_body(&view, "stop_experiment", json!({"reason":"operator stop"}));
        if let Some(code) = code {
            assert_eq!(result.unwrap_err()["code"], code);
            assert!(!state.exp.stop.load(Ordering::SeqCst));
            assert!(!state.hub.safety_latched());
            assert_audit(
                &state,
                before,
                "EXPERIMENT",
                "stop",
                "REJECTED",
                operator,
                Some(code),
            );
        } else {
            result.unwrap();
            assert!(state.exp.stop.load(Ordering::SeqCst));
            assert!(state.hub.safety_latched());
            assert_audit(
                &state,
                before,
                "EXPERIMENT",
                "stop",
                "SUCCESS",
                operator,
                None,
            );
            assert_eq!(
                command_audit(&state, "stop").last().unwrap()["detail"]
                    ["safety_atmosphere_confirmed"],
                true
            );
        }
    }
}

#[test]
fn reset_ipc_idle_and_running_actor_matrices_audit_every_decision() {
    for running in [false, true] {
        let (app, _, _) = app_fixture();
        let state = app.state::<AppState>();
        let view = webview(&app);
        finish_seeded_rows(&state);
        wait_until(|| state.hub.temp_rx().borrow().is_fresh(Instant::now()));
        for (actor, operator, actor_code) in ACTORS {
            if running {
                run_owned_by_alice(&state);
            } else {
                state.exp.set_status(ExperimentStatus::idle());
            }
            state
                .hub
                .request_shutoff_flammable()
                .blocking_recv()
                .unwrap()
                .unwrap();
            wait_until(|| state.hub.mfc_rx().borrow().is_fresh(Instant::now()));
            state.exp.stop.store(false, Ordering::SeqCst);
            select_actor(&state, actor);
            let before = command_audit(&state, "reset_safety_interlock").len();
            let result = invoke_with_body(&view, "reset_safety_interlock", json!({}));
            let code = if !running && actor == "other" {
                None
            } else {
                actor_code
            };
            if code.is_some() || running {
                let error = result.unwrap_err();
                if let Some(code) = code {
                    assert_eq!(error["code"], code);
                }
                assert!(
                    state.hub.safety_latched(),
                    "denied reset must retain the latch"
                );
                assert_eq!(state.exp.is_running(), running);
                assert_audit(
                    &state,
                    before,
                    "GAS",
                    "reset_safety_interlock",
                    "REJECTED",
                    operator,
                    error["code"].as_str(),
                );
            } else {
                result.unwrap();
                assert!(!state.hub.safety_latched());
                assert_audit(
                    &state,
                    before,
                    "GAS",
                    "reset_safety_interlock",
                    "SUCCESS",
                    operator,
                    None,
                );
            }
            assert!(!state.exp.stop.load(Ordering::SeqCst));
        }
    }
}

#[test]
fn closed_control_service_is_device_offline_and_never_a_success_audit() {
    for (command, category, action) in [
        ("stop_experiment", "EXPERIMENT", "stop"),
        ("reset_safety_interlock", "GAS", "reset_safety_interlock"),
    ] {
        let (app, _, _) = app_fixture();
        let state = app.state::<AppState>();
        let view = webview(&app);
        finish_seeded_rows(&state);
        select_actor(&state, "owner");
        wait_until(|| state.hub.temp_rx().borrow().is_fresh(Instant::now()));
        state.hub.stop();
        wait_until(|| state.hub.mfc_rx().has_changed().is_err());
        assert!(state.hub.temp_rx().borrow().is_fresh(Instant::now()));
        let before = command_audit(&state, action).len();
        let error =
            invoke_with_body(&view, command, json!({"reason":"closed service"})).unwrap_err();
        assert_eq!(error["code"], "DEVICE_OFFLINE");
        assert_audit(
            &state,
            before,
            category,
            action,
            "FAILURE",
            "爱丽丝",
            Some("DEVICE_OFFLINE"),
        );
        if action == "stop" {
            assert_eq!(
                command_audit(&state, action).last().unwrap()["detail"]
                    ["safety_atmosphere_confirmed"],
                false
            );
        }
    }
}

fn running_heating_stage(state: &AppState) {
    run_owned_by_alice(state);
    let mut status = state.exp.snapshot();
    status.stage_index = 3;
    status.stage_kind = Some(StageKind::Heating);
    state.exp.set_status(status);
}

#[test]
fn stage_queue_six_actor_matrix_records_the_request_without_claiming_execution() {
    let state = test_state();
    seed_users_and_experiments(&state);
    running_heating_stage(&state);
    for (actor, operator, code) in ACTORS {
        *lock(&state.exp.stage_action) = None;
        select_actor(&state, actor);
        let before = command_audit(&state, "stage_action_request").len();
        let result = procedure::queue_action(
            &state,
            "exp-alice".into(),
            3,
            procedure::ActionKind::Skip,
            "verified reason".into(),
        );
        if let Some(code) = code {
            assert_eq!(CommandError::from(result.unwrap_err()).code, code);
            assert!(lock(&state.exp.stage_action).is_none());
            assert_audit(
                &state,
                before,
                "EXPERIMENT",
                "stage_action_request",
                "REJECTED",
                operator,
                Some(code),
            );
        } else {
            result.unwrap();
            assert_eq!(
                lock(&state.exp.stage_action).as_ref().unwrap().operator,
                operator
            );
            assert_audit(
                &state,
                before,
                "EXPERIMENT",
                "stage_action_request",
                "SUCCESS",
                operator,
                None,
            );
        }
        assert!(
            command_audit(&state, "stage_action").is_empty(),
            "queueing does not execute the stage transition"
        );
    }
}

#[test]
fn invalid_stale_and_duplicate_stage_requests_are_audited_without_replacing_pending_action() {
    let state = test_state();
    seed_users_and_experiments(&state);
    select_actor(&state, "owner");
    running_heating_stage(&state);
    for (id, index, kind, notes) in [
        ("other-run", 3, procedure::ActionKind::Skip, "reason"),
        ("exp-alice", 4, procedure::ActionKind::Skip, "reason"),
        ("exp-alice", 3, procedure::ActionKind::Skip, " "),
        ("exp-alice", 3, procedure::ActionKind::Confirm, "reason"),
    ] {
        let before = command_audit(&state, "stage_action_request").len();
        let error =
            procedure::queue_action(&state, id.into(), index, kind, notes.into()).unwrap_err();
        assert!(lock(&state.exp.stage_action).is_none());
        assert_audit(
            &state,
            before,
            "EXPERIMENT",
            "stage_action_request",
            "REJECTED",
            "爱丽丝",
            Some(&CommandError::from(error).code),
        );
    }
    procedure::queue_action(
        &state,
        "exp-alice".into(),
        3,
        procedure::ActionKind::Skip,
        "original reason".into(),
    )
    .unwrap();
    let original = serde_json::to_value(lock(&state.exp.stage_action).as_ref().unwrap()).unwrap();
    let before = command_audit(&state, "stage_action_request").len();
    let error = procedure::queue_action(
        &state,
        "exp-alice".into(),
        3,
        procedure::ActionKind::Skip,
        "replacement reason".into(),
    )
    .unwrap_err();
    assert_eq!(
        serde_json::to_value(lock(&state.exp.stage_action).as_ref().unwrap()).unwrap(),
        original
    );
    assert_audit(
        &state,
        before,
        "EXPERIMENT",
        "stage_action_request",
        "REJECTED",
        "爱丽丝",
        Some(&CommandError::from(error).code),
    );
}

#[test]
fn an_ack_superseded_by_a_new_run_pending_data_or_protection_cannot_report_reset_success() {
    for superseded_by in ["run", "pending", "protection"] {
        let (app, _, _) = app_fixture();
        let state = app.state::<AppState>();
        finish_seeded_rows(&state);
        state
            .hub
            .request_shutoff_flammable()
            .blocking_recv()
            .unwrap()
            .unwrap();
        reset_latch(&state); // A real, successful reset ACK has arrived.
        assert!(!state.hub.safety_latched());
        match superseded_by {
            "run" => run_owned_by_alice(&state),
            "pending" => lock(&state.exp.recovery_errors).push("new pending record".into()),
            "protection" => {
                state
                    .hub
                    .request_shutoff_flammable()
                    .blocking_recv()
                    .unwrap()
                    .unwrap();
            }
            _ => unreachable!(),
        }
        let before_status = serde_json::to_value(state.exp.snapshot()).unwrap();
        let before_latch = state.hub.safety_latched();
        let before = command_audit(&state, "reset_safety_interlock").len();
        let emitted = AtomicBool::new(false);
        let result = finish_safety_reset(&state, "爱丽丝".into(), Ok(()), |_| {
            emitted.store(true, Ordering::SeqCst);
        });
        assert!(!emitted.load(Ordering::SeqCst));
        assert_eq!(
            serde_json::to_value(state.exp.snapshot()).unwrap(),
            before_status
        );
        assert_eq!(state.hub.safety_latched(), before_latch);
        assert_eq!(
            result.unwrap_err().code,
            "RESET_SUPERSEDED",
            "{superseded_by}"
        );
        assert_audit(
            &state,
            before,
            "GAS",
            "reset_safety_interlock",
            "REJECTED",
            "爱丽丝",
            Some("RESET_SUPERSEDED"),
        );
    }
}

#[test]
fn failed_reset_ack_is_audited_once_without_publishing_idle() {
    let state = test_state();
    state
        .exp
        .set_status(ExperimentStatus::safety_error("unconfirmed-reset", 0.0));
    let before = command_audit(&state, "reset_safety_interlock").len();
    let error = finish_safety_reset(
        &state,
        "爱丽丝".into(),
        Err(CommandError::from("DEVICE_OFFLINE")),
        |_| {
            panic!("failed ACK cannot publish Idle");
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "DEVICE_OFFLINE");
    assert_eq!(state.exp.snapshot().phase, Phase::Error);
    assert_audit(
        &state,
        before,
        "GAS",
        "reset_safety_interlock",
        "FAILURE",
        "爱丽丝",
        Some("DEVICE_OFFLINE"),
    );
}
