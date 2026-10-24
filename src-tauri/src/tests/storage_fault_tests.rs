//! Exercise production persistence and IPC paths with Tauri's headless runtime.
use super::*;
use serde_json::{json, Value};
use std::sync::mpsc;
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
use tauri::Listener;

fn app_fixture() -> tauri::App<MockRuntime> {
    let state = test_state();
    let path = state.recovery_dir.parent().unwrap().join("fault-tests.db");
    *lock(&state.storage) = Storage::open(&path).unwrap();
    seed_users_and_experiments(&state);
    login(&state, "alice", "temp-password");
    mock_builder()
        .manage(state)
        .invoke_handler(tauri::generate_handler![get_experiment_samples])
        .build(mock_context(noop_assets()))
        .unwrap()
}

fn sample(index: usize) -> SampleRow {
    SampleRow {
        experiment_id: "exp-alice".into(),
        timestamp: format!("2026-09-13T08:00:{index:02}Z"),
        experiment_duration: Some(format!("{index}s")),
        record_json: Some(json!({"stage_index": 0, "elapsed_s": index}).to_string()),
        temperature: 25.0,
        weight: 100.0 - index as f64,
        weight_loss: index as f64,
        h2_flow: 0.0,
        n2_flow: 5.0,
        co2_flow: 0.0,
        co_flow: 0.0,
        experiment_status: Some("STABILIZING".into()),
        system_message: None,
    }
}

fn audit_entries(state: &AppState) -> Vec<Value> {
    std::fs::read_dir(state.recovery_dir.parent().unwrap().join("logs"))
        .unwrap()
        .flat_map(|entry| {
            std::fs::read_to_string(entry.unwrap().path())
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect::<Vec<_>>()
        })
        .collect()
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "native state did not reach the expected boundary"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn poisoned_storage_mutex_still_flushes_and_commits_the_terminal_batch() {
    let app = app_fixture();
    let state = app.state::<AppState>();
    let storage = state.storage.clone();
    assert!(std::thread::spawn(move || {
        let _held = storage.lock().unwrap();
        panic!("controlled panic while owning the real storage mutex");
    })
    .join()
    .is_err());
    assert!(
        state.storage.is_poisoned(),
        "the real storage mutex must be poisoned"
    );

    let mut buffer = SampleBuffer {
        rows: vec![sample(0), sample(1)],
        ..SampleBuffer::default()
    };
    flush(
        &state.storage,
        &state.audit,
        &mut buffer,
        "exp-alice",
        "爱丽丝",
        false,
    );
    assert!(buffer.rows.is_empty());
    assert_eq!(buffer.consecutive_failures, 0);
    assert_eq!(lock(&state.storage).count_samples("exp-alice").unwrap(), 2);
    buffer.push(sample(2));
    finalize(
        app.handle(),
        &state.hub,
        &state.storage,
        &state.audit,
        &state.exp,
        &mut buffer,
        "exp-alice",
        "爱丽丝",
        "stopped",
        2.0,
        None,
    );

    assert!(buffer.rows.is_empty());
    assert_eq!(lock(&state.storage).count_samples("exp-alice").unwrap(), 3);
    assert!(lock(&state.storage)
        .get_experiment("exp-alice")
        .unwrap()
        .unwrap()
        .end_time
        .is_some());
    let runtime: Value =
        serde_json::from_str(&lock(&state.storage).runtime_metadata("exp-alice").unwrap()).unwrap();
    assert_eq!(runtime["outcome"], "stopped");
    assert_eq!(runtime["data_complete"], true);
    assert_eq!(runtime["safety_confirmed"], true);
    assert_eq!(runtime["ending_action"], "protective");
    assert!(!state.exp.persistence_pending());
    assert!(recovery::scan(&state.recovery_dir).pending.is_empty());
    assert!(state.hub.safety_latched());
    assert_eq!(state.exp.snapshot().phase, Phase::Idle);
    let stopped: Vec<_> = audit_entries(&state)
        .into_iter()
        .filter(|entry| entry["action"] == "stopped")
        .collect();
    assert_eq!(stopped.len(), 1);
    assert_eq!(stopped[0]["result"], "SUCCESS");
    assert_eq!(stopped[0]["operator"], "爱丽丝");
    assert_eq!(stopped[0]["detail"]["persistence_confirmed"], true);
}

#[test]
fn first_flush_fault_stops_the_real_loop_protects_and_retains_rows_until_retry() {
    let app = app_fixture();
    let state = app.state::<AppState>();
    let db =
        rusqlite::Connection::open(state.recovery_dir.parent().unwrap().join("fault-tests.db"))
            .unwrap();
    db.execute_batch("CREATE TRIGGER fail_samples BEFORE INSERT ON experiment_data BEGIN SELECT RAISE(ABORT, 'controlled sample insert failure'); END;").unwrap();
    wait_until(|| {
        state.hub.temp_rx().borrow().is_fresh(Instant::now())
            && state.hub.mfc_rx().borrow().is_fresh(Instant::now())
    });
    let owner = current_user(&state).unwrap();
    let mut program = standard_program("REDUCIBILITY").unwrap();
    // Keep one nonterminal stage active: only the flush failure may end this run.
    program.stages.truncate(1);
    program.stages[0].kind = StageKind::Stabilizing;
    program.stages[0].duration_min = 60.0;
    program.stages[0].target_temp = 25.0;
    program.stages[0].gas = [1.0, 2.0, 0.0, 0.0];
    let mut running = ExperimentStatus::idle();
    running.running = true;
    running.experiment_id = Some("exp-alice".into());
    state.exp.set_status(running);
    let (tx, rx) = mpsc::channel();
    let listener = app.listen("experiment-state-changed", move |event| {
        let payload: Value = serde_json::from_str(event.payload()).unwrap();
        if payload["running"] == false {
            let _ = tx.send(payload);
        }
    });
    spawn_experiment_task(
        app.handle().clone(),
        state.hub.clone(),
        state.storage.clone(),
        state.audit.clone(),
        state.exp.clone(),
        program,
        "exp-alice".into(),
        "REDUCIBILITY".into(),
        "样品".into(),
        100.0,
        "2026-09-13T08:00:00Z".into(),
        owner.user_id,
        owner.display_name,
        1.0,
        Duration::from_millis(20),
    );
    let terminal = rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap_or_else(|error| {
            state.exp.stop.store(true, Ordering::SeqCst);
            drop(state.hub.request_shutoff_flammable());
            panic!("control loop did not publish its terminal state: {error}");
        });
    app.unlisten(listener);
    assert_eq!(terminal["phase"], "ERROR");
    assert_eq!(terminal["persistence_pending"], true);
    assert!(state.exp.persistence_pending());
    assert!(state.hub.safety_latched());
    wait_until(|| state.hub.mfc_rx().borrow().gas.map(|gas| gas.sv) == [0.0, 5.0, 0.0, 0.0]);
    assert_eq!(lock(&state.storage).count_samples("exp-alice").unwrap(), 0);
    assert!(lock(&state.storage)
        .get_experiment("exp-alice")
        .unwrap()
        .unwrap()
        .end_time
        .is_none());
    let retained = lock(&state.exp.pending).clone();
    assert_eq!(retained.len(), 1);
    assert_eq!(retained[0].rows.len(), FLUSH_EVERY_SAMPLES as usize);
    assert_eq!(retained[0].metadata["outcome"], "error");
    assert_eq!(retained[0].metadata["ending_action"], "protective");
    assert_eq!(retained[0].metadata["dropped_samples"], 0);
    let journal = recovery::scan(&state.recovery_dir);
    assert!(journal.errors.is_empty());
    assert_eq!(journal.pending.len(), 1);
    assert_eq!(
        serde_json::to_value(&journal.pending[0].1).unwrap(),
        serde_json::to_value(&retained[0]).unwrap()
    );
    let log = audit_entries(&state);
    let failures: Vec<_> = log
        .iter()
        .filter(|entry| entry["action"] == "sample_flush_failed")
        .collect();
    assert_eq!(failures.len(), 1, "the first failed flush must end the run");
    assert_eq!(failures[0]["result"], "FAILURE");
    assert_eq!(failures[0]["operator"], "爱丽丝");
    assert_eq!(failures[0]["detail"]["attempts"], 1);
    assert_eq!(failures[0]["detail"]["buffered"], FLUSH_EVERY_SAMPLES);
    assert!(log
        .iter()
        .any(|entry| entry["action"] == "safety_atmosphere" && entry["result"] == "SUCCESS"));
    assert!(log
        .iter()
        .any(|entry| entry["action"] == "completion_persistence" && entry["result"] == "FAILURE"));
    assert!(log.iter().any(|entry| entry["action"] == "error"
        && entry["result"] == "FAILURE"
        && entry["detail"]["persistence_confirmed"] == false));

    db.execute_batch("DROP TRIGGER fail_samples;").unwrap();
    retry_pending_completions(&mut lock(&state.storage), &state.exp, &state.recovery_dir).unwrap();
    retry_pending_completions(&mut lock(&state.storage), &state.exp, &state.recovery_dir).unwrap();
    assert_eq!(
        lock(&state.storage).count_samples("exp-alice").unwrap(),
        FLUSH_EVERY_SAMPLES as i64
    );
    assert!(!state.exp.persistence_pending());
    assert!(recovery::scan(&state.recovery_dir).pending.is_empty());
    assert!(lock(&state.storage)
        .get_experiment("exp-alice")
        .unwrap()
        .unwrap()
        .end_time
        .is_some());
    assert!(
        state.hub.safety_latched(),
        "saving records must not reset protection"
    );
}

#[test]
fn samples_command_parses_snake_case_limit_and_enforces_access_through_real_ipc() {
    let app = app_fixture();
    let state = app.state::<AppState>();
    lock(&state.storage)
        .insert_samples(&(0..10).map(sample).collect::<Vec<_>>())
        .unwrap();
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let invoke = |body: Value| {
        tauri::test::get_ipc_response(
            &webview,
            tauri::webview::InvokeRequest {
                cmd: "get_experiment_samples".into(),
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
    };
    let points = invoke(json!({"id": "exp-alice", "max_points": 4})).unwrap();
    let weights: Vec<_> = points
        .as_array()
        .unwrap()
        .iter()
        .map(|point| point["weight"].as_f64().unwrap())
        .collect();
    assert_eq!(weights, [100.0, 97.0, 94.0, 91.0]);
    assert_eq!(
        invoke(json!({"id": "exp-alice"}))
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        10
    );
    login(&state, "bob", "temp-password");
    assert_eq!(
        invoke(json!({"id": "exp-alice", "max_points": 4})).unwrap_err()["code"],
        json!("FORBIDDEN")
    );
    state.auth.logout();
    assert_eq!(
        invoke(json!({"id": "exp-alice", "max_points": 4})).unwrap_err()["code"],
        json!("AUTH_REQUIRED")
    );
}
