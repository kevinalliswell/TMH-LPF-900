//! Recovery must exclude new experiment starts and audit refused recovery requests.
use super::*;
use serde_json::{json, Value};
use std::sync::{mpsc, TryLockError};
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
use tmh_core::transport::{BusError, Transport};

const WAIT: Duration = Duration::from_secs(5);

struct PausedProtection {
    inner: MockTransport,
    entered: mpsc::Sender<()>,
    release: Option<mpsc::Receiver<()>>,
    fail_first_write: bool,
}

impl Transport for PausedProtection {
    fn transact(&mut self, bytes: &[u8], deadline: Duration) -> Result<Vec<u8>, BusError> {
        // Read polling remains live; stop only at the actual protective setpoint write.
        if bytes.windows(3).any(|part| part == b"XWS") {
            if let Some(release) = self.release.take() {
                let _ = self.entered.send(());
                release
                    .recv_timeout(WAIT)
                    .map_err(|e| BusError::Io(e.to_string()))?;
                if std::mem::take(&mut self.fail_first_write) {
                    return Err(BusError::Timeout);
                }
            }
        }
        self.inner.transact(bytes, deadline)
    }
    fn read_line(&mut self, deadline: Duration) -> Result<Vec<u8>, BusError> {
        self.inner.read_line(deadline)
    }
    fn reset_buffers(&mut self) -> Result<(), BusError> {
        self.inner.reset_buffers()
    }
    fn reopen(&mut self) -> Result<(), BusError> {
        self.inner.reopen()
    }
}

struct ReleaseProtection(Option<mpsc::Sender<()>>);
impl Drop for ReleaseProtection {
    fn drop(&mut self) {
        if let Some(release) = self.0.take() {
            let _ = release.send(());
        }
    }
}

fn app(state: AppState) -> tauri::App<MockRuntime> {
    mock_builder()
        .manage(state)
        .build(mock_context(noop_assets()))
        .unwrap()
}

fn recovery_audits(state: &AppState) -> Vec<Value> {
    std::fs::read_dir(state.recovery_dir.parent().unwrap().join("logs"))
        .unwrap()
        .flat_map(|entry| {
            std::fs::read_to_string(entry.unwrap().path())
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .filter(|entry| entry["action"] == "retry_pending_save")
                .collect::<Vec<_>>()
        })
        .collect()
}

fn records(state: &AppState) -> Value {
    let db = lock(&state.storage);
    let records: Vec<_> = db
        .list_experiments()
        .unwrap()
        .into_iter()
        .map(|row| {
            json!({
                "id": row.experiment_id,
                "end_time": row.end_time,
                "runtime": db.runtime_metadata(&row.experiment_id).unwrap(),
                "samples": db.count_samples(&row.experiment_id).unwrap(),
            })
        })
        .collect();
    json!(records)
}

#[test]
fn recovery_excludes_the_start_lifecycle_until_protection_and_recovered_data_are_committed() {
    let mut state = test_state();
    seed_users_and_experiments(&state);
    login(&state, "admin", "admin-password");
    let (entered, entered_rx) = mpsc::channel();
    let (release, release_rx) = mpsc::channel();
    let release = ReleaseProtection(Some(release));
    state.hub = Arc::new(DeviceHub::spawn(
        TempActor::new(Box::new(MockTransport::temp()), 0, 0.1, false),
        MfcActor::new(
            Box::new(PausedProtection {
                inner: MockTransport::mfc(),
                entered,
                release: Some(release_rx),
                fail_first_write: false,
            }),
            [1, 2, 3, 4],
            [0.1, 1.0, 0.1, 0.1],
            5.0,
            5.0,
        ),
        BalanceActor::new(Box::new(MockTransport::balance(100.0))),
        HubIntervals {
            temp: Duration::from_millis(5),
            mfc: Duration::from_millis(5),
            balance: Duration::from_millis(5),
        },
    ));
    let app = app(state);
    let state = app.state::<AppState>();
    let handle = app.handle().clone();
    let worker = std::thread::spawn(move || retry_pending_save(handle));
    entered_rx
        .recv_timeout(WAIT)
        .expect("recovery reached its physical protection write");
    // Starting a run requires this same mutex. The bus barrier establishes that the
    // recovery command is active; no absence of a scheduling event is used as proof.
    let start_excluded = matches!(
        state.exp.lifecycle.try_lock(),
        Err(TryLockError::WouldBlock)
    );
    drop(release);
    let outcome = worker.join().unwrap();
    outcome.unwrap();
    assert!(
        start_excluded,
        "a start must not cross the recovery scan/commit interval"
    );
    assert!(lock(&state.storage)
        .list_experiments()
        .unwrap()
        .iter()
        .all(|r| r.end_time.is_some()));
    let audit = recovery_audits(&state);
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0]["result"], "SUCCESS");
    assert_eq!(audit[0]["operator"], "管理员");
}

#[test]
fn refused_recovery_requests_preserve_records_and_audit_the_exact_actor_once() {
    for (actor, operator, code) in [
        ("anonymous", "anonymous", "AUTH_REQUIRED"),
        ("owner_running", "爱丽丝", "EXPERIMENT_RUNNING"),
        ("other_running", "鲍勃", "EXPERIMENT_RUNNING"),
        ("admin_running", "管理员", "EXPERIMENT_RUNNING"),
        ("locked", "anonymous", "AUTH_REQUIRED"),
        ("password_change", "鲍勃", "PASSWORD_CHANGE_REQUIRED"),
    ] {
        let state = test_state();
        let (_, alice, _) = seed_users_and_experiments(&state);
        match actor {
            "anonymous" => {}
            "owner_running" | "locked" => login(&state, "alice", "temp-password"),
            "other_running" | "password_change" => login(&state, "bob", "temp-password"),
            "admin_running" => login(&state, "admin", "admin-password"),
            _ => unreachable!(),
        }
        if actor.ends_with("_running") {
            state.exp.set_status(ExperimentStatus {
                running: true,
                phase: Phase::Running,
                experiment_id: Some("exp-alice".into()),
                owner_user_id: Some(alice),
                owner_display_name: Some("爱丽丝".into()),
                ..ExperimentStatus::idle()
            });
        }
        if actor == "locked" {
            state.auth.lock();
        }
        if actor == "password_change" {
            let db = lock(&state.storage);
            let user = db.get_user_by_username("bob").unwrap().unwrap();
            db.set_user_password(
                &user.user_id,
                &user.password_hash,
                true,
                "2026-10-26T00:00:00",
            )
            .unwrap();
        }
        let app = app(state);
        let state = app.state::<AppState>();
        let before = records(&state);
        let error =
            retry_pending_save(app.handle().clone()).expect_err("unsafe recovery must be refused");
        assert_eq!(error.code, code, "{actor}");
        assert_eq!(records(&state), before, "{actor}");
        assert!(
            !state.hub.safety_latched(),
            "a denied retry must not send protection: {actor}"
        );
        let audit = recovery_audits(&state);
        assert_eq!(audit.len(), 1, "one recovery outcome: {actor}");
        assert_eq!(audit[0]["category"], "EXPERIMENT");
        assert_eq!(audit[0]["result"], "REJECTED");
        assert_eq!(audit[0]["operator"], operator);
        assert_eq!(audit[0]["detail"]["code"], code);
        let text = serde_json::to_string(&audit).unwrap();
        for secret in ["admin-password", "temp-password", "$argon2"] {
            assert!(!text.contains(secret));
        }
    }
}

#[test]
fn a_late_stop_ack_cannot_erase_a_newer_error_or_pending_recovery_context() {
    use super::device_control_authorization_tests::{
        assert_audit, command_audit, run_owned_by_alice,
    };
    use std::future::Future;
    use std::task::{Context, Waker};

    let mut preserved = Vec::new();
    for initially_running in [false, true] {
        for pending_recovery in [false, true] {
            let mut state = test_state();
            seed_users_and_experiments(&state);
            login(&state, "admin", "admin-password");
            if initially_running {
                run_owned_by_alice(&state);
            }
            let (entered, entered_rx) = mpsc::channel();
            let (release, release_rx) = mpsc::channel();
            state.hub = Arc::new(DeviceHub::spawn(
                TempActor::new(Box::new(MockTransport::temp()), 0, 0.1, false),
                MfcActor::new(
                    Box::new(PausedProtection {
                        inner: MockTransport::mfc(),
                        entered,
                        release: Some(release_rx),
                        fail_first_write: false,
                    }),
                    [1, 2, 3, 4],
                    [0.1, 1.0, 0.1, 0.1],
                    5.0,
                    5.0,
                ),
                BalanceActor::new(Box::new(MockTransport::balance(100.0))),
                HubIntervals {
                    temp: Duration::from_millis(5),
                    mfc: Duration::from_millis(5),
                    balance: Duration::from_millis(5),
                },
            ));
            let app = app(state);
            let release = ReleaseProtection(Some(release));
            let state = app.state::<AppState>();
            let before = command_audit(&state, "stop").len();
            let mut stop = Box::pin(stop_experiment(app.handle().clone(), "delayed stop".into()));
            assert!(stop
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending());
            entered_rx
                .recv_timeout(WAIT)
                .expect("stop reached the physical protection write");
            drop(release);
            // This ordinary command is behind the in-flight protection request.
            // Its ACK proves the stop reply is ready without resuming the caller.
            state
                .hub
                .request_set_flow(0, 0.0)
                .blocking_recv()
                .unwrap()
                .unwrap();
            state
                .exp
                .set_status(ExperimentStatus::safety_error("newer-finalization", 17.0));
            if pending_recovery {
                lock(&state.exp.recovery_errors).push("new unsaved finalization".into());
            }
            let newer = serde_json::to_value(state.exp.snapshot()).unwrap();
            tauri::async_runtime::block_on(stop).unwrap();
            preserved.push((
                initially_running,
                pending_recovery,
                serde_json::to_value(state.exp.snapshot()).unwrap() == newer,
            ));
            assert_audit(
                &state,
                before,
                "EXPERIMENT",
                "stop",
                "SUCCESS",
                "管理员",
                None,
            );
            assert_eq!(
                command_audit(&state, "stop").last().unwrap()["detail"]
                    ["safety_atmosphere_confirmed"],
                true
            );
        }
    }
    assert!(
        preserved.iter().all(|(_, _, kept)| *kept),
        "a delayed ACK erased newer state: {preserved:?}"
    );
}

#[test]
fn a_late_failed_stop_cannot_replace_idle_after_a_newer_run_has_finished() {
    use super::device_control_authorization_tests::{
        assert_audit, command_audit, run_owned_by_alice,
    };
    use std::future::Future;
    use std::task::{Context, Waker};

    let mut state = test_state();
    seed_users_and_experiments(&state);
    login(&state, "admin", "admin-password");
    let (entered, entered_rx) = mpsc::channel();
    let (release, release_rx) = mpsc::channel();
    state.hub = Arc::new(DeviceHub::spawn(
        TempActor::new(Box::new(MockTransport::temp()), 0, 0.1, false),
        MfcActor::new(
            Box::new(PausedProtection {
                inner: MockTransport::mfc(),
                entered,
                release: Some(release_rx),
                fail_first_write: true,
            }),
            [1, 2, 3, 4],
            [0.1, 1.0, 0.1, 0.1],
            5.0,
            5.0,
        ),
        BalanceActor::new(Box::new(MockTransport::balance(100.0))),
        HubIntervals {
            temp: Duration::from_millis(5),
            mfc: Duration::from_millis(5),
            balance: Duration::from_millis(5),
        },
    ));
    let app = app(state);
    let release = ReleaseProtection(Some(release));
    let state = app.state::<AppState>();
    let before = command_audit(&state, "stop").len();
    let mut stop = Box::pin(stop_experiment(
        app.handle().clone(),
        "old failed stop".into(),
    ));
    assert!(stop
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
    entered_rx
        .recv_timeout(WAIT)
        .expect("the first protective write reached its gate");
    drop(release);
    // This later ACK proves the old failed protection reply is already ready.
    state
        .hub
        .request_set_flow(0, 0.0)
        .blocking_recv()
        .unwrap()
        .unwrap();
    run_owned_by_alice(&state);
    state.exp.set_status(ExperimentStatus::idle());
    let newer_idle = serde_json::to_value(state.exp.snapshot()).unwrap();
    let error = tauri::async_runtime::block_on(stop).unwrap_err();

    assert_eq!(error.code, "DEVICE_OFFLINE");
    assert_eq!(
        serde_json::to_value(state.exp.snapshot()).unwrap(),
        newer_idle
    );
    assert_audit(
        &state,
        before,
        "EXPERIMENT",
        "stop",
        "FAILURE",
        "管理员",
        Some("DEVICE_OFFLINE"),
    );
    assert_eq!(
        command_audit(&state, "stop").last().unwrap()["detail"]["safety_atmosphere_confirmed"],
        false
    );
}
