//! Observe the balance watch synchronously when the TARE oneshot wakes its waiter.
use std::future::Future;
use std::pin::Pin;
use std::sync::{mpsc, Arc};
use std::task::{Context, Wake, Waker};
use std::time::{Duration, Instant};
use tmh_core::bus::{BalanceActor, BalanceSnapshot, DeviceHub, HubIntervals, MfcActor, TempActor};
use tmh_core::transport::{BusError, MockTransport, Transport};
use tokio::sync::watch;

const WAIT: Duration = Duration::from_secs(5);

struct GatedBalance {
    inner: MockTransport,
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
}
impl Transport for GatedBalance {
    fn transact(&mut self, bytes: &[u8], deadline: Duration) -> Result<Vec<u8>, BusError> {
        self.entered.send(()).unwrap();
        self.release
            .recv_timeout(WAIT)
            .map_err(|error| BusError::Io(error.to_string()))?;
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

struct ReleaseOnDrop(mpsc::Sender<()>);
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

struct SnapshotAtWake {
    snapshots: watch::Receiver<BalanceSnapshot>,
    observed: mpsc::Sender<BalanceSnapshot>,
}
impl Wake for SnapshotAtWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        // oneshot send invokes this on the balance worker itself: a later poll
        // cannot overwrite the watch until this synchronous observation returns.
        let _ = self.observed.send(self.snapshots.borrow().clone());
    }
}

#[test]
fn tare_ack_wakes_waiter_only_after_publishing_the_invalidated_weight() {
    let (entered, entered_rx) = mpsc::channel();
    let (release, release_rx) = mpsc::channel();
    let hub = DeviceHub::spawn(
        TempActor::new(Box::new(MockTransport::temp()), 0, 0.1, false),
        MfcActor::new(
            Box::new(MockTransport::mfc()),
            [1, 2, 3, 4],
            [0.1, 1.0, 0.1, 0.1],
            5.0,
            5.0,
        ),
        BalanceActor::new(Box::new(GatedBalance {
            inner: MockTransport::balance(100.0),
            entered,
            release: release_rx,
        })),
        HubIntervals {
            temp: Duration::from_millis(5),
            mfc: Duration::from_millis(5),
            balance: Duration::from_millis(5),
        },
    );
    // Drop this before the hub on every unwind so its worker cannot remain blocked.
    let release = ReleaseOnDrop(release);
    let snapshots = hub.balance_rx();
    let deadline = Instant::now() + WAIT;
    while !snapshots.borrow().is_fresh(Instant::now()) {
        assert!(
            Instant::now() < deadline,
            "initial balance read did not arrive"
        );
        std::thread::yield_now();
    }
    assert_eq!(snapshots.borrow().weight, Some(100.0));

    let mut reply = hub.request_tare();
    entered_rx
        .recv_timeout(WAIT)
        .expect("TARE reached the wire barrier");
    let (observed, observed_rx) = mpsc::channel();
    let waker = Waker::from(Arc::new(SnapshotAtWake {
        snapshots,
        observed,
    }));
    assert!(Pin::new(&mut reply)
        .poll(&mut Context::from_waker(&waker))
        .is_pending());
    drop(release);

    let at_wake = observed_rx
        .recv_timeout(WAIT)
        .expect("TARE reply woke its waiter");
    let result = reply.blocking_recv().unwrap();
    assert!(result.success);
    assert_eq!(at_wake.weight, Some(0.0));
    assert!(
        !at_wake.is_fresh(Instant::now()),
        "ACK must not expose a pre-tare fresh measurement"
    );
}
