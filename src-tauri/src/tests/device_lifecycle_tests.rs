use super::*;
use std::sync::{mpsc, TryLockError};

const WAIT: Duration = Duration::from_secs(5);

#[test]
fn update_lease_rejects_manual_enqueue_and_safety_reset_before_device_checks() {
    let state = test_state();
    seed_users_and_experiments(&state);
    login(&state, "admin", "admin-password");
    let _lease = UpdateLease::begin(state.exp.clone()).unwrap();
    let called = AtomicBool::new(false);

    let manual = enqueue_manual_gas(
        &state,
        "set_flow",
        serde_json::json!({"gas":0,"requested":3.0}),
        |hub| {
            called.store(true, Ordering::SeqCst);
            Ok(hub.request_set_flow(0, 3.0))
        },
    );

    assert!(manual.is_err(), "an update excludes manual gas commands");
    assert!(
        !called.load(Ordering::SeqCst),
        "rejected work must not enqueue"
    );
    let error = match enqueue_safety_reset(&state) {
        Err(error) => error,
        Ok(_) => panic!("an update must reject the reset before it is enqueued"),
    };
    assert!(
        error.contains("更新"),
        "updating must be checked before device health: {error}"
    );
}

#[test]
fn update_lease_cannot_cross_an_authorized_manual_enqueue() {
    let state = Arc::new(test_state());
    seed_users_and_experiments(&state);
    login(&state, "admin", "admin-password");
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (manual_done_tx, manual_done_rx) = mpsc::channel();
    let worker_state = state.clone();
    let manual_worker = std::thread::spawn(move || {
        let acknowledged = enqueue_manual_gas(
            &worker_state,
            "set_flow",
            serde_json::json!({"gas":0,"requested":3.0}),
            |hub| {
                entered_tx.send(()).map_err(|e| e.to_string())?;
                release_rx.recv_timeout(WAIT).map_err(|e| e.to_string())?;
                Ok(hub.request_set_flow(0, 3.0))
            },
        )
        .and_then(|(_, reply)| {
            reply
                .blocking_recv()
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())
        });
        let _ = manual_done_tx.send(acknowledged);
    });

    entered_rx
        .recv_timeout(WAIT)
        .expect("manual enqueue reached its controlled boundary");
    // This check establishes exclusion without a scheduling delay: a lease
    // needs this same mutex and cannot cross the paused enqueue while held.
    let lifecycle_held = matches!(
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
        .expect("update worker started");
    let early_lease = lease_rx.try_recv();
    let acquired_before_release = early_lease.is_ok();

    // Release before any exclusion assertion. The worker gate also has a
    // timeout, so a failing assertion cannot strand a lifecycle mutex.
    release_tx
        .send(())
        .expect("manual worker is waiting for release");
    let ack = manual_done_rx
        .recv_timeout(WAIT)
        .expect("manual command must finish or report failure")
        .expect("authorized manual command receives its device ACK");
    let lease = match early_lease {
        Ok(result) => result,
        Err(mpsc::TryRecvError::Empty) => lease_rx
            .recv_timeout(WAIT)
            .expect("update lease completes after manual enqueue releases lifecycle"),
        Err(mpsc::TryRecvError::Disconnected) => panic!("update worker disappeared"),
    }
    .expect("update lease is allowed after the manual enqueue");
    manual_worker.join().expect("manual worker did not panic");
    update_worker.join().expect("update worker did not panic");

    assert!(
        lifecycle_held,
        "authorization and actual enqueue must hold lifecycle continuously"
    );
    assert!(
        !acquired_before_release,
        "update lease crossed an unfinished manual enqueue"
    );
    assert_eq!(ack.applied, 3.0);
    assert!(state.exp.updating.load(Ordering::SeqCst));
    drop(lease);
    assert!(!state.exp.updating.load(Ordering::SeqCst));
}

#[test]
fn old_reset_ack_does_not_publish_idle_over_a_new_run_or_update() {
    let state = test_state();
    let mut running = ExperimentStatus::idle();
    running.running = true;
    running.experiment_id = Some("new-run".into());
    state.exp.set_status(running);

    assert!(!publish_safety_reset(&state, |_| panic!(
        "stale reset must not emit"
    )));
    let current = state.exp.snapshot();
    assert!(current.running);
    assert_eq!(current.experiment_id.as_deref(), Some("new-run"));

    state.exp.set_status(ExperimentStatus::idle());
    let lease = UpdateLease::begin(state.exp.clone()).unwrap();
    assert!(!publish_safety_reset(&state, |_| panic!(
        "stale reset must not emit"
    )));
    assert!(state.exp.snapshot().updating);
    drop(lease);

    lock(&state.exp.recovery_errors).push("new persistence failure".into());
    assert!(!publish_safety_reset(&state, |_| panic!(
        "stale reset must not emit"
    )));
    assert!(state.exp.persistence_pending());
    lock(&state.exp.recovery_errors).clear();

    let _protection = state.hub.request_shutoff_flammable();
    assert!(state.hub.safety_latched());
    assert!(!publish_safety_reset(&state, |_| panic!(
        "stale reset must not emit"
    )));
}

#[test]
fn reset_status_event_is_published_before_a_new_lifecycle_can_start() {
    let state = test_state();
    let mut held_during_event = false;
    assert!(publish_safety_reset(&state, |status| {
        assert!(!status.running);
        held_during_event = matches!(
            state.exp.lifecycle.try_lock(),
            Err(TryLockError::WouldBlock)
        );
    }));
    assert!(
        held_during_event,
        "a new run must not emit Running before the preceding reset emits Idle"
    );
}
