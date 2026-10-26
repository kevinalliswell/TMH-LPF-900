//! A real balance actor pauses at its TARE write while commands exercise lifecycle gates.
use super::*;
use std::future::Future;
use std::sync::mpsc;
use std::task::{Context, Poll, Waker};
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
use tmh_core::transport::{BusError, Transport};

const WAIT: Duration = Duration::from_secs(5);

struct PausedTare {
    inner: MockTransport,
    entered: mpsc::Sender<()>,
    release: Option<mpsc::Receiver<()>>,
    writes: Arc<AtomicU64>,
}

impl Transport for PausedTare {
    fn transact(&mut self, bytes: &[u8], deadline: Duration) -> Result<Vec<u8>, BusError> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        if let Some(release) = self.release.take() {
            let _ = self.entered.send(());
            release
                .recv_timeout(WAIT)
                .map_err(|error| BusError::Io(error.to_string()))?;
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
    fn set_hint(&mut self, key: &str, value: f64) {
        self.inner.set_hint(key, value);
    }
}

/// Always release the bus even if an assertion unwinds.
struct ReleaseTare(Option<mpsc::Sender<()>>);
impl Drop for ReleaseTare {
    fn drop(&mut self) {
        if let Some(release) = self.0.take() {
            let _ = release.send(());
        }
    }
}

fn fixture() -> (
    tauri::App<MockRuntime>,
    mpsc::Receiver<()>,
    ReleaseTare,
    Arc<AtomicU64>,
) {
    let mut state = test_state();
    seed_users_and_experiments(&state);
    login(&state, "admin", "admin-password");
    let (entered, entered_rx) = mpsc::channel();
    let (release, release_rx) = mpsc::channel();
    let writes = Arc::new(AtomicU64::new(0));
    state.hub = Arc::new(DeviceHub::spawn(
        TempActor::new(Box::new(MockTransport::temp()), 0, 0.1, false),
        MfcActor::new(
            Box::new(MockTransport::mfc()),
            [1, 2, 3, 4],
            [0.1, 1.0, 0.1, 0.1],
            5.0,
            5.0,
        ),
        BalanceActor::new(Box::new(PausedTare {
            inner: MockTransport::balance(100.0),
            entered,
            release: Some(release_rx),
            writes: writes.clone(),
        })),
        HubIntervals {
            temp: Duration::from_millis(5),
            mfc: Duration::from_millis(5),
            balance: Duration::from_millis(5),
        },
    ));
    let app = mock_builder()
        .manage(state)
        .build(mock_context(noop_assets()))
        .unwrap();
    (app, entered_rx, ReleaseTare(Some(release)), writes)
}

fn params() -> ExperimentParams {
    ExperimentParams {
        sample_id: "tare-boundary-sample".into(),
        sample_name: "tare-boundary".into(),
        sample_weight: 100.0,
        experiment_type: "REDUCIBILITY".into(),
        project_name: None,
        notes: None,
    }
}

fn wait_for(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "device did not reach the observed boundary"
        );
        std::thread::yield_now();
    }
}

fn ready_before_tare(state: &AppState) {
    wait_for(|| build_preflight(state, &params()).ok);
    assert!(state.hub.balance_rx().borrow().is_fresh(Instant::now()));
}

fn poll_once<F: Future>(future: std::pin::Pin<&mut F>) -> Poll<F::Output> {
    future.poll(&mut Context::from_waker(Waker::noop()))
}

fn tare_audits(state: &AppState) -> Vec<serde_json::Value> {
    std::fs::read_dir(state.recovery_dir.parent().unwrap().join("logs"))
        .unwrap()
        .flat_map(|entry| {
            std::fs::read_to_string(entry.unwrap().path())
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
                .filter(|entry| entry["action"] == "tare")
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn unfinished_tare_blocks_preflight_start_and_update_until_the_balance_ack() {
    let (app, entered, release, writes) = fixture();
    let state = app.state::<AppState>();
    ready_before_tare(&state);
    let mut call = Box::pin(tare_balance(app.handle().clone()));
    assert!(poll_once(call.as_mut()).is_pending());
    entered
        .recv_timeout(WAIT)
        .expect("actual TARE transport write reached its barrier");

    let preflight = build_preflight(&state, &params());
    let update_was_rejected = UpdateLease::begin(state.exp.clone()).is_err();
    // Preserve the blocked snapshot, then release the real actor before assertions.
    drop(release);
    let outcome = tauri::async_runtime::block_on(call).unwrap();
    assert!(outcome.success);
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    assert!(
        preflight
            .blocking
            .iter()
            .any(|issue| issue.code == "BALANCE_BUSY"),
        "a fresh pre-tare weight must not allow start while TARE is unconfirmed"
    );
    assert!(
        update_was_rejected,
        "an installer/restart must not cross a pending TARE"
    );

    ready_before_tare(&state);
    // Repeat the barrier with a fresh fixture to verify the public start command itself.
    let (second_app, second_entered, second_release, _) = fixture();
    let second_state = second_app.state::<AppState>();
    ready_before_tare(&second_state);
    let mut second_call = Box::pin(tare_balance(second_app.handle().clone()));
    assert!(poll_once(second_call.as_mut()).is_pending());
    second_entered.recv_timeout(WAIT).unwrap();
    let start_result = start_experiment(second_app.handle().clone(), params());
    drop(second_release);
    tauri::async_runtime::block_on(second_call).unwrap();
    let error = start_result.expect_err("pending tare must reject actual start");
    assert_eq!(error.code, "BALANCE_BUSY");
    assert!(!second_state.exp.is_running());
    assert_eq!(
        lock(&second_state.storage)
            .list_experiments()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn second_tare_is_rejected_without_queuing_another_baseline_change() {
    let (app, entered, release, writes) = fixture();
    let state = app.state::<AppState>();
    ready_before_tare(&state);
    let mut first = Box::pin(tare_balance(app.handle().clone()));
    assert!(poll_once(first.as_mut()).is_pending());
    entered.recv_timeout(WAIT).unwrap();
    let mut duplicate = Box::pin(tare_balance(app.handle().clone()));
    let duplicate_result = poll_once(duplicate.as_mut());
    drop(duplicate);
    drop(release);
    assert!(tauri::async_runtime::block_on(first).unwrap().success);

    let error = match duplicate_result {
        Poll::Ready(Err(error)) => error,
        _ => panic!("the second command must reject BALANCE_BUSY before its first await"),
    };
    assert_eq!(error.code, "BALANCE_BUSY");
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    let audits = tare_audits(&state);
    assert_eq!(audits.len(), 2);
    assert!(audits
        .iter()
        .any(|entry| entry["result"] == "REJECTED" && entry["detail"]["code"] == "BALANCE_BUSY"));
    assert_eq!(
        audits
            .iter()
            .filter(|entry| entry["result"] == "SUCCESS")
            .count(),
        1
    );
}

#[test]
fn cancelling_the_caller_keeps_tare_busy_until_device_completion_and_records_the_outcome() {
    let (app, entered, release, writes) = fixture();
    let state = app.state::<AppState>();
    ready_before_tare(&state);
    let mut call = Box::pin(tare_balance(app.handle().clone()));
    assert!(poll_once(call.as_mut()).is_pending());
    entered
        .recv_timeout(WAIT)
        .expect("TARE is physically in flight");
    drop(call);
    let after_cancellation = build_preflight(&state, &params());
    let update_was_rejected = UpdateLease::begin(state.exp.clone()).is_err();
    drop(release);

    assert!(
        after_cancellation
            .blocking
            .iter()
            .any(|issue| issue.code == "BALANCE_BUSY"),
        "dropping an IPC caller cannot make an unfinished physical tare disappear"
    );
    assert!(update_was_rejected);
    wait_for(|| {
        tare_audits(&state)
            .iter()
            .any(|entry| entry["result"] == "SUCCESS")
    });
    ready_before_tare(&state);
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    assert_eq!(
        tare_audits(&state).len(),
        1,
        "detached completion writes one final outcome"
    );
    assert!(UpdateLease::begin(state.exp.clone()).is_ok());
}

#[test]
fn close_waits_for_the_physical_tare_and_can_be_retried_after_its_ack() {
    let (app, entered, release, writes) = fixture();
    let state = app.state::<AppState>();
    ready_before_tare(&state);
    let mut call = Box::pin(tare_balance(app.handle().clone()));
    assert!(poll_once(call.as_mut()).is_pending());
    entered
        .recv_timeout(WAIT)
        .expect("physical TARE is unfinished");
    // This real MFC ACK rules out a disconnected protection service as the blocker.
    apply_safety_atmosphere_blocking(&state.hub).unwrap();
    let blocked = prepare_exit(app.handle());
    assert!(!state.exp.is_running());
    assert!(!state.exp.persistence_pending());
    assert!(!state.exp.updating.load(Ordering::SeqCst));
    drop(release);
    assert!(tauri::async_runtime::block_on(call).unwrap().success);

    assert!(
        blocked,
        "window close must keep the TARE completion task alive"
    );
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    assert!(
        !prepare_exit(app.handle()),
        "the user can close again after the ACK"
    );
}

#[test]
fn an_accepted_idle_exit_excludes_later_tare_start_and_update() {
    let (app, _entered, release, writes) = fixture();
    let state = app.state::<AppState>();
    ready_before_tare(&state);
    assert!(
        !prepare_exit(app.handle()),
        "idle close must receive confirmed protection and be accepted"
    );
    assert!(state.hub.safety_latched());

    let preflight = build_preflight(&state, &params());
    let start_error = start_experiment(app.handle().clone(), params()).unwrap_err();
    let update_was_rejected = UpdateLease::begin(state.exp.clone()).is_err();
    let mut call = Box::pin(tare_balance(app.handle().clone()));
    let outcome = poll_once(call.as_mut());
    // If broken code queued the TARE, finish it before reporting the regression.
    drop(release);
    let tare_error = match outcome {
        Poll::Ready(Err(error)) => Some(error),
        Poll::Pending => {
            let _ = tauri::async_runtime::block_on(call);
            None
        }
        Poll::Ready(Ok(_)) => None,
    };

    assert!(
        preflight
            .blocking
            .iter()
            .any(|issue| issue.code == "APP_CLOSING"),
        "accepted close must retain a lifecycle blocker"
    );
    assert_eq!(start_error.code, "APP_CLOSING");
    assert!(
        update_was_rejected,
        "an update cannot acquire a new lease after close acceptance"
    );
    assert_eq!(
        tare_error
            .expect("accepted close must reject tare before enqueue")
            .code,
        "APP_CLOSING"
    );
    assert_eq!(writes.load(Ordering::SeqCst), 0);
    assert!(!state.exp.is_running());
    assert_eq!(lock(&state.storage).list_experiments().unwrap().len(), 3);
}

#[test]
fn repeated_exit_notification_does_not_reopen_an_already_accepted_close() {
    let (app, _entered, _release, _writes) = fixture();
    let state = app.state::<AppState>();
    ready_before_tare(&state);
    assert!(
        !prepare_exit(app.handle()),
        "the first close receives its protection ACK"
    );
    state.hub.stop();
    wait_for(|| state.hub.mfc_rx().has_changed().is_err());
    // Tauri can dispatch ExitRequested after CloseRequested. The first accepted
    // close owns the decision even if the device service has stopped meanwhile.
    let blocked_again = prepare_exit(app.handle());
    assert!(
        !blocked_again,
        "an already accepted close must remain accepted"
    );
    assert!(build_preflight(&state, &params())
        .blocking
        .iter()
        .any(|issue| issue.code == "APP_CLOSING"));
}
