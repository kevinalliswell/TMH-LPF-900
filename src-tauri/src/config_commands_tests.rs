use super::*;
use crate::tests::test_state;
use serde_json::Value;

fn audits(state: &AppState, action: &str) -> Vec<Value> {
    std::fs::read_dir(state.recovery_dir.parent().unwrap().join("logs"))
        .unwrap()
        .flat_map(|file| {
            std::fs::read_to_string(file.unwrap().path())
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .collect::<Vec<_>>()
        })
        .filter(|entry| entry["action"] == action)
        .collect()
}

fn admin(state: &AppState) {
    state
        .auth
        .bootstrap(
            &lock(&state.storage),
            "admin",
            "Config administrator",
            "ConfigPassword!",
            epoch_now(),
        )
        .unwrap();
}

#[test]
fn rejected_restart_is_audited_once_with_the_authenticated_actor() {
    let state = test_state();
    let revision = lock(&state.config).snapshot().revision;
    tauri::async_runtime::block_on(async {
        assert!(prepare_restart(&state, &revision).await.is_err());
        admin(&state);
        lock(&state.exp.status).running = true;
        assert!(prepare_restart(&state, &revision).await.is_err());
    });
    let log_dir = state.recovery_dir.parent().unwrap().join("logs");
    let mut entries = Vec::<serde_json::Value>::new();
    for file in std::fs::read_dir(log_dir).unwrap() {
        for line in std::fs::read_to_string(file.unwrap().path())
            .unwrap()
            .lines()
        {
            let value: serde_json::Value = serde_json::from_str(line).unwrap();
            if value["action"] == "restart_for_config" {
                entries.push(value);
            }
        }
    }
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().all(|entry| entry["result"] == "REJECTED"));
    assert_eq!(entries[0]["operator"], "anonymous");
    assert_eq!(entries[1]["operator"], "Config administrator");
}

fn changed(state: &AppState) -> ConfigSnapshot {
    let mut snapshot = lock(&state.config).snapshot();
    snapshot.saved.sampling.interval_s = 2.0;
    snapshot.saved.performance.experiment_data_interval = 2.0;
    snapshot
}

#[test]
fn explicit_invalid_config_never_falls_back_or_overwrites_the_file() {
    let state = test_state();
    let root = state.recovery_dir.parent().unwrap();
    let missing = root.join("explicit-missing.json");
    assert!(open_store(root, Some(missing.clone())).is_err());
    assert!(!missing.exists());
    let invalid = root.join("invalid.json");
    std::fs::write(&invalid, "invalid original bytes").unwrap();
    assert!(open_store(root, Some(invalid.clone())).is_err());
    assert_eq!(
        std::fs::read_to_string(invalid).unwrap(),
        "invalid original bytes"
    );
}

#[test]
fn save_requires_admin_idle_state_reason_and_current_revision() {
    let state = test_state();
    let change = changed(&state);
    assert_eq!(snapshot(&state).unwrap_err(), "AUTH_REQUIRED");
    assert_eq!(
        save(&state, &change.revision, change.saved.clone(), "reason")
            .unwrap_err()
            .code,
        "AUTH_REQUIRED"
    );
    admin(&state);
    state
        .auth
        .create_user(
            &lock(&state.storage),
            "operator",
            "Operator",
            "ConfigPassword!",
            UserRole::User,
            epoch_now(),
        )
        .unwrap();
    state
        .auth
        .login(
            &lock(&state.storage),
            "operator",
            "ConfigPassword!",
            epoch_now(),
        )
        .unwrap();
    state
        .auth
        .change_own_password(
            &lock(&state.storage),
            "ConfigPassword!",
            "NewConfigPassword!",
            epoch_now(),
        )
        .unwrap();
    assert!(snapshot(&state).is_ok());
    assert_eq!(
        save(&state, &change.revision, change.saved.clone(), "reason")
            .unwrap_err()
            .code,
        "FORBIDDEN"
    );
    state
        .auth
        .login(
            &lock(&state.storage),
            "admin",
            "ConfigPassword!",
            epoch_now(),
        )
        .unwrap();
    lock(&state.exp.status).running = true;
    assert_eq!(
        save(&state, &change.revision, change.saved.clone(), "reason")
            .unwrap_err()
            .code,
        "EXPERIMENT_RUNNING"
    );
    lock(&state.exp.status).running = false;
    lock(&state.exp.recovery_errors).push("pending recovery".into());
    assert_eq!(
        save(&state, &change.revision, change.saved.clone(), "reason")
            .unwrap_err()
            .code,
        "PERSISTENCE_PENDING"
    );
    lock(&state.exp.recovery_errors).clear();
    state.exp.updating.store(true, Ordering::SeqCst);
    assert_eq!(
        save(&state, &change.revision, change.saved.clone(), "reason")
            .unwrap_err()
            .code,
        "UPDATE_IN_PROGRESS"
    );
    state.exp.updating.store(false, Ordering::SeqCst);
    assert_eq!(
        save(&state, &change.revision, change.saved.clone(), " ")
            .unwrap_err()
            .code,
        "INVALID_PARAMS"
    );
    assert_eq!(
        save(&state, "stale", change.saved.clone(), "reason")
            .unwrap_err()
            .code,
        "CONFIG_CONFLICT"
    );
    state.auth.lock();
    assert_eq!(
        save(&state, &change.revision, change.saved, "reason")
            .unwrap_err()
            .code,
        "AUTH_REQUIRED"
    );
    assert!(!lock(&state.config).snapshot().restart_required);
}

#[test]
fn successful_save_is_pending_and_preflight_blocks_until_reopen() {
    let state = test_state();
    admin(&state);
    let change = changed(&state);
    let saved = save(
        &state,
        &change.revision,
        change.saved,
        "Change the sample interval for software QA",
    )
    .unwrap();
    assert!(saved.restart_required);
    assert_eq!(saved.applied.sampling.interval_s, 1.0);
    assert_eq!(saved.saved.sampling.interval_s, 2.0);
    assert_eq!(state.cadence.sampling.as_secs_f64(), 1.0);
    assert!(start_blocker(&state).is_some());
    let bytes = std::fs::read(&saved.path).unwrap();
    assert_eq!(
        serde_json::from_slice::<CommConfig>(&bytes)
            .unwrap()
            .sampling
            .interval_s,
        2.0
    );
    let log_dir = state.recovery_dir.parent().unwrap().join("logs");
    let entries: Vec<serde_json::Value> = std::fs::read_dir(log_dir)
        .unwrap()
        .flat_map(|file| {
            std::fs::read_to_string(file.unwrap().path())
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(entries
        .iter()
        .any(|entry| entry["action"] == "save_comm_config"
            && entry["result"] == "SUCCESS"
            && entry["operator"] == "Config administrator"));
}

#[test]
fn accepted_close_rejects_config_save_and_audits_the_admin_once() {
    let state = test_state();
    admin(&state);
    let change = changed(&state);
    state.exp.closing.store(true, Ordering::SeqCst);

    let error = save(
        &state,
        &change.revision,
        change.saved,
        "Save after accepted close",
    )
    .unwrap_err();
    assert_eq!(error.code, "APP_CLOSING");

    let saved = lock(&state.config).snapshot();
    assert!(!saved.restart_required);
    let entries = audits(&state, "save_comm_config");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["category"], "CONFIG");
    assert_eq!(entries[0]["result"], "REJECTED");
    assert_eq!(entries[0]["operator"], "Config administrator");
}

#[test]
fn config_save_authorization_database_failure_is_audited_as_failure() {
    let state = test_state();
    let db_path = state
        .recovery_dir
        .parent()
        .unwrap()
        .join("config-auth-fault.db");
    *lock(&state.storage) = Storage::open(&db_path).unwrap();
    admin(&state);
    let change = changed(&state);
    rusqlite::Connection::open(&db_path)
        .unwrap()
        .execute_batch("PRAGMA foreign_keys=OFF; DROP TABLE users")
        .unwrap();

    let error = save(
        &state,
        &change.revision,
        change.saved,
        "Authorization database fault",
    )
    .unwrap_err();
    assert_eq!(error.code, "STORAGE_ERROR");

    let entries = audits(&state, "save_comm_config");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["category"], "CONFIG");
    assert_eq!(entries[0]["result"], "FAILURE");
    assert_eq!(entries[0]["operator"], "anonymous");
    assert_eq!(entries[0]["detail"]["code"], "STORAGE_ERROR");
}

#[test]
fn config_file_failure_has_a_stable_code_and_failure_audit() {
    let state = test_state();
    admin(&state);
    let change = changed(&state);
    let initial = lock(&state.config).snapshot();
    std::fs::remove_file(&initial.path).unwrap();

    let error = save(
        &state,
        &change.revision,
        change.saved,
        "Config file failure",
    )
    .unwrap_err();
    assert_eq!(error.code, "CONFIG_SAVE_FAILED");

    let current = lock(&state.config).snapshot();
    assert!(!current.restart_required);
    assert_eq!(current.saved, initial.saved);
    let entries = audits(&state, "save_comm_config");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["result"], "FAILURE");
    assert_eq!(entries[0]["operator"], "Config administrator");
    assert_eq!(entries[0]["detail"]["code"], "CONFIG_SAVE_FAILED");
}

#[test]
fn restart_requires_admin_and_protection_and_rejects_external_changes() {
    tauri::async_runtime::block_on(async {
        let state = test_state();
        admin(&state);
        let change = changed(&state);
        let saved = save(
            &state,
            &change.revision,
            change.saved,
            "Software restart QA",
        )
        .unwrap();
        state.auth.lock();
        assert!(prepare_restart(&state, &saved.revision).await.is_err());
        assert!(!state.exp.updating.load(Ordering::SeqCst));
        state
            .auth
            .login(
                &lock(&state.storage),
                "admin",
                "ConfigPassword!",
                epoch_now(),
            )
            .unwrap();
        assert!(prepare_restart(&state, "old revision").await.is_err());
        let lease = prepare_restart(&state, &saved.revision).await.unwrap();
        assert!(state.exp.updating.load(Ordering::SeqCst));
        assert!(state.hub.safety_latched());
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            let flows: Vec<_> = state
                .hub
                .mfc_rx()
                .borrow()
                .gas
                .iter()
                .map(|gas| gas.sv)
                .collect();
            if flows == [0.0, 5.0, 0.0, 0.0] {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "acknowledged protection must reach telemetry"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        drop(lease);
        assert!(!state.exp.updating.load(Ordering::SeqCst));
        std::fs::write(&saved.path, b"outside changes").unwrap();
        assert!(prepare_restart(&state, &saved.revision).await.is_err());
        assert!(!state.exp.updating.load(Ordering::SeqCst));
    });
}

// Paste into config_commands_tests.rs; uses its existing test_state/admin/changed helpers.
#[test]
fn failed_protection_ack_cancels_restart_releases_lease_and_keeps_safety_latched() {
    use tmh_core::transport::MockFaults;

    tauri::async_runtime::block_on(async {
        let mut state = test_state();
        let cfg = lock(&state.config).snapshot().applied;
        state.hub = Arc::new(DeviceHub::spawn(
            TempActor::new(
                Box::new(MockTransport::temp()),
                cfg.temp.slave_address,
                cfg.temp.scale,
                cfg.temp.signed_registers,
            ),
            MfcActor::from_config(
                Box::new(MockTransport::mfc().with_faults(MockFaults {
                    // Every wire transaction times out, independent of polling phase.
                    drop_every: 1,
                    ..Default::default()
                })),
                &cfg,
            ),
            BalanceActor::new(Box::new(MockTransport::balance(100.0))),
            HubIntervals::default(),
        ));
        admin(&state);
        let change = changed(&state);
        let saved = save(
            &state,
            &change.revision,
            change.saved,
            "Failure ACK regression",
        )
        .unwrap();

        let error = match prepare_restart(&state, &saved.revision).await {
            Err(error) => error,
            Ok(_) => panic!("a restart lease must not be returned without protection ACK"),
        };
        assert!(
            error.message_zh.contains("保护"),
            "unexpected error: {error}"
        );
        assert_eq!(error.code, "DEVICE_COMMAND_FAILED");
        assert!(!state.exp.updating.load(Ordering::SeqCst));
        assert!(
            state.hub.safety_latched(),
            "failed restart must not clear protection"
        );
        assert!(lock(&state.config).snapshot().restart_required);

        let entries: Vec<serde_json::Value> =
            std::fs::read_dir(state.recovery_dir.parent().unwrap().join("logs"))
                .unwrap()
                .flat_map(|file| {
                    std::fs::read_to_string(file.unwrap().path())
                        .unwrap()
                        .lines()
                        .map(|line| serde_json::from_str(line).unwrap())
                        .collect::<Vec<_>>()
                })
                .filter(|entry: &serde_json::Value| entry["action"] == "restart_for_config")
                .collect();
        assert_eq!(
            entries.len(),
            1,
            "one result audit for this restart attempt"
        );
        assert_eq!(entries[0]["category"], "CONFIG");
        assert_eq!(entries[0]["result"], "FAILURE");
        assert_eq!(entries[0]["operator"], "Config administrator");
        assert_eq!(entries[0]["detail"]["code"], "DEVICE_COMMAND_FAILED");
    });
}
