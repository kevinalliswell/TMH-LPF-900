//! Invoke production device controls through real headless IPC and observable mock buses.
use super::*;
use serde_json::{json, Value};
use std::sync::{mpsc, TryLockError};
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
use tmh_core::transport::{BusError, Transport};

const WAIT: Duration = Duration::from_secs(5);

#[derive(Default)]
pub(super) struct BalanceTrace {
    tares: AtomicU64,
    reads: AtomicU64,
}

struct ObservedBalance {
    inner: MockTransport,
    trace: Arc<BalanceTrace>,
    closed: mpsc::Sender<()>,
}

impl Transport for ObservedBalance {
    fn transact(&mut self, bytes: &[u8], deadline: Duration) -> Result<Vec<u8>, BusError> {
        // Balance polling is read-only; the actor's only request/response write is tare.
        self.trace.tares.fetch_add(1, Ordering::SeqCst);
        self.inner.transact(bytes, deadline)
    }
    fn read_line(&mut self, deadline: Duration) -> Result<Vec<u8>, BusError> {
        self.trace.reads.fetch_add(1, Ordering::SeqCst);
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

impl Drop for ObservedBalance {
    fn drop(&mut self) {
        let _ = self.closed.send(());
    }
}

pub(super) fn app_fixture() -> (
    tauri::App<MockRuntime>,
    Arc<BalanceTrace>,
    mpsc::Receiver<()>,
) {
    let mut state = test_state();
    seed_users_and_experiments(&state);
    let trace = Arc::new(BalanceTrace::default());
    let (closed, closed_rx) = mpsc::channel();
    state.hub = Arc::new(DeviceHub::spawn(
        TempActor::new(Box::new(MockTransport::temp()), 0, 0.1, false),
        MfcActor::new(
            Box::new(MockTransport::mfc()),
            [1, 2, 3, 4],
            [0.1, 1.0, 0.1, 0.1],
            5.0,
            5.0,
        ),
        BalanceActor::new(Box::new(ObservedBalance {
            inner: MockTransport::balance(100.0),
            trace: trace.clone(),
            closed,
        })),
        HubIntervals {
            temp: Duration::from_millis(5),
            mfc: Duration::from_millis(5),
            balance: Duration::from_millis(5),
        },
    ));
    let app = mock_builder()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            tare_balance,
            shutoff_flammable_gases,
            stop_experiment,
            reset_safety_interlock,
            start_experiment
        ])
        .build(mock_context(noop_assets()))
        .unwrap();
    (app, trace, closed_rx)
}

pub(super) fn webview(app: &tauri::App<MockRuntime>) -> tauri::WebviewWindow<MockRuntime> {
    tauri::WebviewWindowBuilder::new(app, "main", Default::default())
        .build()
        .unwrap()
}

fn invoke(view: &tauri::WebviewWindow<MockRuntime>, command: &str) -> Result<Value, Value> {
    invoke_with_body(view, command, json!({}))
}

pub(super) fn invoke_with_body(
    view: &tauri::WebviewWindow<MockRuntime>,
    command: &str,
    body: Value,
) -> Result<Value, Value> {
    tauri::test::get_ipc_response(
        view,
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

pub(super) fn command_audit(state: &AppState, action: &str) -> Vec<Value> {
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
                .filter(|entry| entry["action"] == action)
                .collect::<Vec<_>>()
        })
        .collect()
}

pub(super) fn assert_audit(
    state: &AppState,
    before: usize,
    category: &str,
    action: &str,
    result: &str,
    operator: &str,
    code: Option<&str>,
) {
    let entries = command_audit(state, action);
    assert_eq!(
        entries.len(),
        before + 1,
        "one complete outcome per {action} invoke"
    );
    let entry = entries.last().unwrap();
    assert_eq!(entry["category"], category);
    assert_eq!(entry["operator"], operator);
    assert_eq!(entry["result"], result);
    if let Some(code) = code {
        assert_eq!(entry["detail"]["code"], code);
    }
    let text = serde_json::to_string(&entries).unwrap();
    for secret in ["admin-password", "temp-password", "$argon2"] {
        assert!(!text.contains(secret));
    }
}

pub(super) fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "mock bus did not reach the observed boundary"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

pub(super) fn run_owned_by_alice(state: &AppState) {
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
}

pub(super) fn select_actor(state: &AppState, actor: &str) {
    state.auth.logout();
    match actor {
        "anonymous" => {}
        "owner" => login(state, "alice", "temp-password"),
        "other" => login(state, "bob", "temp-password"),
        "admin" => login(state, "admin", "admin-password"),
        "locked" => {
            login(state, "alice", "temp-password");
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
            login(state, "bob", "temp-password");
        }
        _ => unreachable!(),
    }
}

pub(super) const ACTORS: [(&str, &str, Option<&str>); 6] = [
    ("anonymous", "anonymous", Some("AUTH_REQUIRED")),
    ("owner", "爱丽丝", None),
    ("other", "鲍勃", Some("FORBIDDEN")),
    ("admin", "管理员", None),
    ("locked", "anonymous", Some("AUTH_REQUIRED")),
    ("password_change", "鲍勃", Some("PASSWORD_CHANGE_REQUIRED")),
];

fn assert_tare_outcome(
    view: &tauri::WebviewWindow<MockRuntime>,
    state: &AppState,
    trace: &BalanceTrace,
    actor: &str,
    operator: &str,
    code: Option<&str>,
) {
    select_actor(state, actor);
    let before_audit = command_audit(state, "tare").len();
    let before_tares = trace.tares.load(Ordering::SeqCst);
    let response = invoke(view, "tare_balance");
    // Observe subsequent real reads so an incorrectly queued tare cannot be
    // concealed by a stale balance snapshot or a command returning too early.
    let drain_reads = trace.reads.load(Ordering::SeqCst) + 3;
    wait_until(|| trace.reads.load(Ordering::SeqCst) >= drain_reads);
    if let Some(code) = code {
        assert_eq!(
            trace.tares.load(Ordering::SeqCst),
            before_tares,
            "{actor}: {code} must not change the weighing baseline"
        );
        let error = response.unwrap_err();
        assert_eq!(error["code"], code);
        assert!(!error["message_zh"].as_str().unwrap().is_empty());
        assert_audit(
            state,
            before_audit,
            "BALANCE",
            "tare",
            "REJECTED",
            operator,
            Some(code),
        );
    } else {
        let outcome = response.unwrap();
        assert_eq!(outcome["success"], true);
        assert_eq!(outcome["weight"], 0.0);
        assert_eq!(trace.tares.load(Ordering::SeqCst), before_tares + 1);
        assert_audit(
            state,
            before_audit,
            "BALANCE",
            "tare",
            "SUCCESS",
            operator,
            None,
        );
    }
    assert!(!state.exp.stop.load(Ordering::SeqCst));
}

#[test]
fn idle_tare_ipc_six_actor_matrix_allows_authenticated_users_and_audits_denials() {
    let (app, trace, _closed) = app_fixture();
    let state = app.state::<AppState>();
    let view = webview(&app);
    assert!(!state.exp.is_running());
    for (actor, operator, code) in ACTORS {
        // "Other" is only an authorization distinction while Alice owns a run.
        let expected = if actor == "other" { None } else { code };
        assert_tare_outcome(&view, &state, &trace, actor, operator, expected);
    }
}

#[test]
fn running_tare_ipc_preserves_mass_baseline_for_owner_and_admin() {
    let (app, trace, _closed) = app_fixture();
    let state = app.state::<AppState>();
    let view = webview(&app);
    run_owned_by_alice(&state);
    for (actor, operator, code) in ACTORS {
        assert_tare_outcome(
            &view,
            &state,
            &trace,
            actor,
            operator,
            code.or(Some("EXPERIMENT_RUNNING")),
        );
    }
    assert!(state.exp.is_running());
}

#[test]
fn pending_persistence_tare_ipc_preserves_the_existing_weighing_baseline() {
    let (app, trace, _closed) = app_fixture();
    let state = app.state::<AppState>();
    let view = webview(&app);
    lock(&state.exp.recovery_errors).push("controlled unresolved recovery record".into());
    assert!(!state.exp.is_running());
    assert!(state.exp.persistence_pending());
    for (actor, operator) in [("owner", "爱丽丝"), ("admin", "管理员")] {
        assert_tare_outcome(
            &view,
            &state,
            &trace,
            actor,
            operator,
            Some("EXPERIMENT_RUNNING"),
        );
    }
    assert!(state.exp.persistence_pending());
}

#[test]
fn shutoff_ipc_six_actor_matrix_audits_and_denials_leave_stop_and_latch_unchanged() {
    let (app, _trace, _closed) = app_fixture();
    let state = app.state::<AppState>();
    let view = webview(&app);
    run_owned_by_alice(&state);
    for (actor, operator, code) in ACTORS {
        if state.hub.safety_latched() {
            wait_until(|| state.hub.mfc_rx().borrow().is_fresh(Instant::now()));
            state
                .hub
                .request_reset_safety()
                .blocking_recv()
                .unwrap()
                .unwrap();
        }
        state.exp.stop.store(false, Ordering::SeqCst);
        select_actor(&state, actor);
        let before = command_audit(&state, "shutoff").len();
        let response = invoke(&view, "shutoff_flammable_gases");
        if let Some(code) = code {
            assert_eq!(response.unwrap_err()["code"], code);
            assert!(
                !state.exp.stop.load(Ordering::SeqCst),
                "{actor}: rejection changed the running experiment"
            );
            assert!(
                !state.hub.safety_latched(),
                "{actor}: rejection enqueued a protection request"
            );
            assert_audit(
                &state,
                before,
                "GAS",
                "shutoff",
                "REJECTED",
                operator,
                Some(code),
            );
        } else {
            response.unwrap();
            assert!(state.exp.stop.load(Ordering::SeqCst));
            assert!(state.hub.safety_latched());
            assert_audit(&state, before, "GAS", "shutoff", "SUCCESS", operator, None);
        }
    }
}

#[test]
fn active_update_lease_rejects_both_controls_without_device_effect_and_audits_each_rejection() {
    let (app, trace, _closed) = app_fixture();
    let state = app.state::<AppState>();
    let view = webview(&app);
    login(&state, "admin", "admin-password");
    let _lease = UpdateLease::begin(state.exp.clone()).unwrap();
    for (command, category, action) in [
        ("tare_balance", "BALANCE", "tare"),
        ("shutoff_flammable_gases", "GAS", "shutoff"),
    ] {
        let before = command_audit(&state, action).len();
        let error = invoke(&view, command).unwrap_err();
        assert!(!error["message_zh"].as_str().unwrap().is_empty());
        assert_eq!(trace.tares.load(Ordering::SeqCst), 0);
        assert!(!state.hub.safety_latched());
        assert!(!state.exp.stop.load(Ordering::SeqCst));
        assert_audit(
            &state,
            before,
            category,
            action,
            "REJECTED",
            "管理员",
            error["code"].as_str(),
        );
    }
}

#[test]
fn closed_balance_service_returns_failure_and_audits_operator_without_a_prior_success() {
    let (app, trace, closed) = app_fixture();
    let state = app.state::<AppState>();
    let view = webview(&app);
    login(&state, "alice", "temp-password");
    let mut balance = state.hub.balance_rx();
    state.hub.stop();
    closed
        .recv_timeout(WAIT)
        .expect("balance transport was dropped after its command receiver closed");
    let deadline = Instant::now() + WAIT;
    loop {
        match balance.has_changed() {
            Err(_) => break,
            Ok(true) => {
                balance.borrow_and_update();
            }
            Ok(false) => {}
        }
        assert!(
            Instant::now() < deadline,
            "balance worker sender should close after its final snapshot"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        balance.has_changed().is_err(),
        "the balance worker has really exited"
    );
    let before = command_audit(&state, "tare").len();
    let error = invoke(&view, "tare_balance").unwrap_err();
    assert!(!error["message_zh"].as_str().unwrap().is_empty());
    assert_eq!(trace.tares.load(Ordering::SeqCst), 0);
    assert_audit(
        &state,
        before,
        "BALANCE",
        "tare",
        "FAILURE",
        "爱丽丝",
        error["code"].as_str(),
    );
}

#[test]
fn balance_authorization_and_tare_enqueue_exclude_the_update_lifecycle() {
    let state = Arc::new(test_state());
    seed_users_and_experiments(&state);
    login(&state, "admin", "admin-password");
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (completed_tx, completed_rx) = mpsc::channel();
    let worker_state = state.clone();
    let control_worker = std::thread::spawn(move || {
        let result = enqueue_device_control(
            &worker_state,
            Category::Balance,
            "tare",
            json!({}),
            |hub, _user| {
                entered_tx.send(()).map_err(|error| error.to_string())?;
                release_rx
                    .recv_timeout(WAIT)
                    .map_err(|error| error.to_string())?;
                Ok(hub.request_tare())
            },
        )
        .and_then(|(operator, reply)| {
            reply
                .blocking_recv()
                .map(|result| (operator, result))
                .map_err(|error| error.to_string())
        });
        let _ = completed_tx.send(result);
    });
    entered_rx
        .recv_timeout(WAIT)
        .expect("authorized callback reached the pre-enqueue barrier");
    let held_during_enqueue = matches!(
        state.exp.lifecycle.try_lock(),
        Err(TryLockError::WouldBlock)
    );

    let (attempted_tx, attempted_rx) = mpsc::channel();
    let (lease_tx, lease_rx) = mpsc::channel();
    let exp = state.exp.clone();
    let update_worker = std::thread::spawn(move || {
        let _ = attempted_tx.send(());
        let _ = lease_tx.send(UpdateLease::begin(exp));
    });
    attempted_rx
        .recv_timeout(WAIT)
        .expect("update worker attempted its lease");
    let updating_while_paused = state.exp.updating.load(Ordering::SeqCst);
    // Release before assertions so a failing lock invariant cannot strand either worker.
    release_tx
        .send(())
        .expect("tare callback is waiting at its barrier");
    let (operator, outcome) = completed_rx
        .recv_timeout(WAIT)
        .expect("tare control completed")
        .unwrap();
    let lease = lease_rx
        .recv_timeout(WAIT)
        .expect("update lease proceeds after enqueue")
        .unwrap();
    control_worker.join().unwrap();
    update_worker.join().unwrap();

    assert!(
        held_during_enqueue,
        "authorization and actual tare enqueue must hold lifecycle continuously"
    );
    assert!(
        !updating_while_paused,
        "update crossed the authorized tare enqueue boundary"
    );
    assert_eq!(operator, "管理员");
    assert!(outcome.success);
    assert!(state.exp.updating.load(Ordering::SeqCst));
    drop(lease);
    assert!(!state.exp.updating.load(Ordering::SeqCst));
}

#[test]
fn closed_mfc_service_returns_failure_without_claiming_confirmed_protection() {
    let (app, _trace, _closed) = app_fixture();
    let state = app.state::<AppState>();
    let view = webview(&app);
    login(&state, "alice", "temp-password");
    let mut snapshots = state.hub.mfc_rx();
    state.hub.stop();
    let (closed_tx, closed_rx) = mpsc::channel();
    let watcher = std::thread::spawn(move || {
        tauri::async_runtime::block_on(async { while snapshots.changed().await.is_ok() {} });
        let _ = closed_tx.send(());
    });
    closed_rx
        .recv_timeout(WAIT)
        .expect("MFC watch sender closed after its real worker exited");
    watcher.join().unwrap();
    assert!(state.hub.mfc_rx().has_changed().is_err());

    let before = command_audit(&state, "shutoff").len();
    let error = invoke(&view, "shutoff_flammable_gases").unwrap_err();
    assert_eq!(error["code"], "DEVICE_OFFLINE");
    assert!(
        state.exp.stop.load(Ordering::SeqCst),
        "the authorized stop intent remains set even when no device confirms it"
    );
    assert_audit(
        &state,
        before,
        "GAS",
        "shutoff",
        "FAILURE",
        "爱丽丝",
        Some("DEVICE_OFFLINE"),
    );
    let entries = command_audit(&state, "shutoff");
    assert_eq!(
        entries.last().unwrap()["detail"]["safety_atmosphere_confirmed"],
        false
    );
}

#[test]
fn device_authorization_storage_fault_is_a_failure_and_never_enqueues_controls() {
    let (app, trace, _closed) = app_fixture();
    let state = app.state::<AppState>();
    let view = webview(&app);
    login(&state, "alice", "temp-password");
    let path = state
        .recovery_dir
        .parent()
        .unwrap()
        .join("control-auth-fault.db");
    *lock(&state.storage) = Storage::open(&path).unwrap();
    rusqlite::Connection::open(path)
        .unwrap()
        .execute_batch("DROP TABLE users")
        .unwrap();
    for (command, category, action) in [
        ("tare_balance", "BALANCE", "tare"),
        ("shutoff_flammable_gases", "GAS", "shutoff"),
    ] {
        let before = command_audit(&state, action).len();
        let error = invoke(&view, command).unwrap_err();
        assert_eq!(error["code"], "STORAGE_ERROR");
        let drain_reads = trace.reads.load(Ordering::SeqCst) + 3;
        wait_until(|| trace.reads.load(Ordering::SeqCst) >= drain_reads);
        assert_eq!(trace.tares.load(Ordering::SeqCst), 0);
        assert!(!state.exp.stop.load(Ordering::SeqCst));
        assert!(!state.hub.safety_latched());
        // The failed user-row read cannot establish a display name; do not invent one.
        assert_audit(
            &state,
            before,
            category,
            action,
            "FAILURE",
            "anonymous",
            Some("STORAGE_ERROR"),
        );
    }
}
