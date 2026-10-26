//! `DeviceHub`: spawns the three bus actors on dedicated OS threads (docs/03) and
//! bridges them to the async orchestration layer. Snapshots flow out through
//! `tokio::watch`; commands (set flow / on-off / shutoff / tare) flow in through
//! `tokio::mpsc` channels carrying a `oneshot` reply. Blocking serial I/O stays on
//! the threads, never on the async runtime. All handles are `Send + Sync` so the
//! hub can live in Tauri's managed state.

use crate::bus::mfc::SafetyLatch;
use crate::bus::{
    BalanceActor, BalanceSnapshot, MfcActor, MfcCommand, MfcCommandError, MfcSnapshot, TempActor,
    TempSnapshot,
};
use crate::clamp::ClampResult;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tmh_protocols::balance::TareResult;
use tokio::sync::{mpsc, oneshot, watch};

/// Poll cadences per bus (docs/04 §4.4).
#[derive(Debug, Clone, Copy)]
pub struct HubIntervals {
    pub temp: Duration,
    pub mfc: Duration,
    pub balance: Duration,
}

impl Default for HubIntervals {
    fn default() -> Self {
        Self {
            temp: Duration::from_millis(200),
            mfc: Duration::from_millis(250),
            balance: Duration::from_millis(100),
        }
    }
}

enum MfcReq {
    Reset {
        reply: oneshot::Sender<Result<(), MfcCommandError>>,
    },
    SetFlow {
        gas: usize,
        value: f64,
        reply: oneshot::Sender<Result<ClampResult, MfcCommandError>>,
    },
    SetOnOff {
        gas: usize,
        on: bool,
        reply: oneshot::Sender<Result<ClampResult, MfcCommandError>>,
    },
    SetAtmosphere {
        values: [f64; 4],
        reply: oneshot::Sender<Result<[ClampResult; 4], MfcCommandError>>,
    },
    Shutoff {
        reply: oneshot::Sender<Result<(), MfcCommandError>>,
    },
}

impl MfcReq {
    fn reject(self, error: MfcCommandError) {
        match self {
            Self::SetFlow { reply, .. } | Self::SetOnOff { reply, .. } => {
                let _ = reply.send(Err(error));
            }
            Self::SetAtmosphere { reply, .. } => {
                let _ = reply.send(Err(error));
            }
            Self::Shutoff { reply } | Self::Reset { reply } => {
                let _ = reply.send(Err(error));
            }
        }
    }
}
struct QueuedMfc {
    epoch: u64,
    req: MfcReq,
}
fn dispatch_mfc(mfc: &mut MfcActor, request: QueuedMfc) {
    let safety = mfc.safety_latch();
    if request.epoch != safety.epoch() {
        request.req.reject(MfcCommandError::SafetyLatched);
        return;
    }
    if let MfcReq::Reset { reply } = request.req {
        let _ = reply.send(mfc.reset_safety(request.epoch));
        return;
    }
    handle_mfc_req(mfc, request.req);
}

enum BalReq {
    Tare { reply: oneshot::Sender<TareResult> },
}

/// Apply one MFC request on the bus thread (HIGH priority). Extracted so both the poll
/// loop and the responsive idle wait can dispatch a command the moment it arrives (#5).
fn handle_mfc_req(mfc: &mut MfcActor, req: MfcReq) {
    match req {
        MfcReq::Reset { .. } => unreachable!("reset is dispatched with an epoch"),
        MfcReq::SetFlow { gas, value, reply } => {
            let _ = reply.send(mfc.set_flow(gas, value));
        }
        MfcReq::SetOnOff { gas, on, reply } => {
            let _ = reply.send(mfc.set_on_off(gas, on));
        }
        MfcReq::SetAtmosphere { values, reply } => {
            let _ = reply.send(mfc.set_atmosphere(values));
        }
        MfcReq::Shutoff { reply } => {
            let result = mfc.apply_protection();
            let _ = reply.send(result);
        }
    }
}

/// Owns the running bus threads. Dropping it stops and joins them.
pub struct DeviceHub {
    stop: Arc<AtomicBool>,
    handles: Vec<JoinHandle<()>>,
    temp_rx: watch::Receiver<TempSnapshot>,
    mfc_rx: watch::Receiver<MfcSnapshot>,
    balance_rx: watch::Receiver<BalanceSnapshot>,
    safety: Arc<SafetyLatch>,
    mfc_tx: mpsc::Sender<QueuedMfc>,
    safety_tx: mpsc::Sender<MfcReq>,
    bal_tx: mpsc::UnboundedSender<BalReq>,
    temp_target_tx: mpsc::UnboundedSender<f64>,
    bal_target_tx: mpsc::UnboundedSender<f64>,
}

impl DeviceHub {
    /// Spawn the three bus threads. Actors are pre-constructed (mock/replay/serial).
    pub fn spawn(temp: TempActor, mfc: MfcActor, balance: BalanceActor, iv: HubIntervals) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let (temp_tx, temp_rx) = watch::channel(temp.snapshot());
        let (mfc_snap_tx, mfc_rx) = watch::channel(mfc.snapshot());
        let (bal_snap_tx, balance_rx) = watch::channel(balance.snapshot());
        let safety = mfc.safety_latch();
        let (mfc_tx, mfc_cmd_rx) = mpsc::channel::<QueuedMfc>(64);
        let (safety_tx, mut safety_rx) = mpsc::channel::<MfcReq>(8);
        let (bal_tx, bal_cmd_rx) = mpsc::unbounded_channel::<BalReq>();
        let (temp_target_tx, temp_target_rx) = mpsc::unbounded_channel::<f64>();
        let (bal_target_tx, bal_target_rx) = mpsc::unbounded_channel::<f64>();
        let mut handles = Vec::new();

        // Temperature: poll all 9 registers each tick.
        {
            let stop = stop.clone();
            handles.push(thread::spawn(move || {
                let mut temp = temp;
                let mut tt_rx = temp_target_rx;
                while !stop.load(Ordering::Relaxed) {
                    let deadline = Instant::now() + iv.temp;
                    while let Ok(c) = tt_rx.try_recv() {
                        temp.set_target_c(c);
                    }
                    temp.poll_once(Instant::now(), Duration::from_millis(100));
                    let _ = temp_tx.send(temp.snapshot());
                    if !temp.snapshot().connected {
                        temp.reconnect();
                    }
                    // Deadline-based sleep so the period stays iv.temp, not iv.temp + work (#14).
                    thread::sleep(deadline.saturating_duration_since(Instant::now()));
                }
            }));
        }

        // MFC: drain HIGH-priority requests (preempt poll), then poll one slave.
        {
            let stop = stop.clone();
            handles.push(thread::spawn(move || {
                let mut mfc = mfc;
                let mut cmd_rx = mfc_cmd_rx;
                while !stop.load(Ordering::Relaxed) {
                    loop {
                        while let Ok(req) = safety_rx.try_recv() {
                            handle_mfc_req(&mut mfc, req);
                        }
                        match cmd_rx.try_recv() {
                            Ok(req) => dispatch_mfc(&mut mfc, req),
                            Err(_) => break,
                        }
                    }
                    mfc.poll_next(Instant::now());
                    let _ = mfc_snap_tx.send(mfc.snapshot());
                    if !mfc.snapshot().connected {
                        mfc.reconnect();
                    }
                    // Responsive idle wait: sleep in short slices so a HIGH-priority command
                    // (emergency shutoff / safety atmosphere) is applied within ~25 ms rather
                    // than waiting a full poll interval (issue #5).
                    let deadline = Instant::now() + iv.mfc;
                    while !stop.load(Ordering::Relaxed) {
                        loop {
                            while let Ok(req) = safety_rx.try_recv() {
                                handle_mfc_req(&mut mfc, req);
                            }
                            match cmd_rx.try_recv() {
                                Ok(req) => dispatch_mfc(&mut mfc, req),
                                Err(_) => break,
                            }
                        }
                        let _ = mfc_snap_tx.send(mfc.snapshot());
                        let now = Instant::now();
                        if now >= deadline {
                            break;
                        }
                        thread::sleep((deadline - now).min(Duration::from_millis(25)));
                    }
                }
                // Safety net (issue #1): whenever this bus thread stops — app close or
                // hub drop — zero the flammable gases as its final action, so tearing the
                // app down never leaves H2/CO/CO2 flowing.
                let _ = mfc.handle_command(&MfcCommand::ShutoffFlammable);
            }));
        }

        // Balance: a pending tare runs to completion (no polling interleaved).
        {
            let stop = stop.clone();
            handles.push(thread::spawn(move || {
                let mut balance = balance;
                let mut cmd_rx = bal_cmd_rx;
                let mut bt_rx = bal_target_rx;
                while !stop.load(Ordering::Relaxed) {
                    let deadline = Instant::now() + iv.balance;
                    while let Ok(g) = bt_rx.try_recv() {
                        balance.set_target_g(g);
                    }
                    let tare_reply = if let Ok(BalReq::Tare { reply }) = cmd_rx.try_recv() {
                        Some((reply, balance.tare()))
                    } else {
                        balance.poll_once(Instant::now(), Duration::from_millis(100));
                        None
                    };
                    // Publish invalidated freshness before the ACK can release start.
                    let _ = bal_snap_tx.send(balance.snapshot());
                    if let Some((reply, result)) = tare_reply {
                        let _ = reply.send(result);
                    }
                    if !balance.snapshot().connected {
                        balance.reconnect();
                    }
                    // Deadline-based sleep so the period stays iv.balance, not + work (#14).
                    thread::sleep(deadline.saturating_duration_since(Instant::now()));
                }
            }));
        }

        Self {
            stop,
            handles,
            temp_rx,
            mfc_rx,
            balance_rx,
            safety,
            safety_tx,
            mfc_tx,
            bal_tx,
            temp_target_tx,
            bal_target_tx,
        }
    }

    pub fn temp_rx(&self) -> watch::Receiver<TempSnapshot> {
        self.temp_rx.clone()
    }
    pub fn mfc_rx(&self) -> watch::Receiver<MfcSnapshot> {
        self.mfc_rx.clone()
    }
    pub fn balance_rx(&self) -> watch::Receiver<BalanceSnapshot> {
        self.balance_rx.clone()
    }

    pub fn safety_latched(&self) -> bool {
        self.safety.is_latched()
    }
    pub fn request_reset_safety(&self) -> oneshot::Receiver<Result<(), MfcCommandError>> {
        let (reply, rx) = oneshot::channel();
        self.enqueue(MfcReq::Reset { reply });
        rx
    }
    fn enqueue(&self, req: MfcReq) {
        let request = QueuedMfc {
            epoch: self.safety.epoch(),
            req,
        };
        // A closed service drops the reply, distinguishable from live backpressure.
        if let Err(mpsc::error::TrySendError::Full(request)) = self.mfc_tx.try_send(request) {
            request.req.reject(MfcCommandError::QueueFull);
        }
    }

    /// Request a gas setpoint; await the returned receiver for the clamp result.
    pub fn request_set_flow(
        &self,
        gas: usize,
        value: f64,
    ) -> oneshot::Receiver<Result<ClampResult, MfcCommandError>> {
        let (reply, rx) = oneshot::channel();
        self.enqueue(MfcReq::SetFlow { gas, value, reply });
        rx
    }
    pub fn request_set_on_off(
        &self,
        gas: usize,
        on: bool,
    ) -> oneshot::Receiver<Result<ClampResult, MfcCommandError>> {
        let (reply, rx) = oneshot::channel();
        self.enqueue(MfcReq::SetOnOff { gas, on, reply });
        rx
    }
    /// Apply a four-gas atmosphere and await acknowledgement of every attempted write.
    pub fn request_set_atmosphere(
        &self,
        values: [f64; 4],
    ) -> oneshot::Receiver<Result<[ClampResult; 4], MfcCommandError>> {
        let (reply, rx) = oneshot::channel();
        self.enqueue(MfcReq::SetAtmosphere { values, reply });
        rx
    }
    /// Shut off flammable gases (H2/CO2/CO → 0). Never blocked by the clamp.
    pub fn request_shutoff_flammable(&self) -> oneshot::Receiver<Result<(), MfcCommandError>> {
        let (reply, rx) = oneshot::channel();
        self.safety.trip();
        if let Err(mpsc::error::TrySendError::Full(request)) =
            self.safety_tx.try_send(MfcReq::Shutoff { reply })
        {
            request.reject(MfcCommandError::QueueFull);
        }
        rx
    }
    /// Request a tare; await the returned receiver for the result.
    pub fn request_tare(&self) -> oneshot::Receiver<TareResult> {
        let (reply, rx) = oneshot::channel();
        let _ = self.bal_tx.send(BalReq::Tare { reply });
        rx
    }

    /// Set the mock furnace target temperature (°C) so it follows an experiment program.
    pub fn set_temp_target_c(&self, c: f64) {
        let _ = self.temp_target_tx.send(c);
    }

    /// Set the mock balance drift target (g) so a reduction run loses mass. Mock-only.
    pub fn set_balance_target_g(&self, g: f64) {
        let _ = self.bal_target_tx.send(g);
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for DeviceHub {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for h in self.handles.drain(..) {
            let _ = h.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{MockFaults, MockTransport};

    #[test]
    fn hub_publishes_snapshots_and_handles_set_flow() {
        let mut tm = MockTransport::temp();
        tm.set_temp_target_raw(9000);
        let temp = TempActor::new(Box::new(tm), 0, 0.1, false);
        let mfc = MfcActor::new(
            Box::new(MockTransport::mfc()),
            [1, 2, 3, 4],
            [0.1, 1.0, 0.1, 0.1],
            5.0,
            5.0,
        );
        let balance = BalanceActor::new(Box::new(MockTransport::balance(50.0)));
        let iv = HubIntervals {
            temp: Duration::from_millis(5),
            mfc: Duration::from_millis(5),
            balance: Duration::from_millis(5),
        };
        let hub = DeviceHub::spawn(temp, mfc, balance, iv);

        thread::sleep(Duration::from_millis(80)); // let the threads poll
        assert!(hub.temp_rx().borrow().connected);
        assert!(hub.balance_rx().borrow().weight.is_some());

        let clamp = hub
            .request_set_flow(0, 8.0)
            .blocking_recv()
            .expect("reply")
            .expect("device ack");
        assert_eq!(clamp.applied, 5.0);
        assert!(clamp.clamped);

        hub.stop();
    }

    #[test]
    fn hub_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<DeviceHub>();
    }

    #[test]
    fn hub_returns_transport_failure_instead_of_queue_ack() {
        let temp = TempActor::new(Box::new(MockTransport::temp()), 0, 0.1, false);
        let mfc = MfcActor::new(
            Box::new(MockTransport::mfc().with_faults(MockFaults {
                drop_every: 1,
                ..Default::default()
            })),
            [1, 2, 3, 4],
            [0.1, 1.0, 0.1, 0.1],
            5.0,
            5.0,
        );
        let balance = BalanceActor::new(Box::new(MockTransport::balance(50.0)));
        let hub = DeviceHub::spawn(
            temp,
            mfc,
            balance,
            HubIntervals {
                temp: Duration::from_millis(5),
                mfc: Duration::from_millis(5),
                balance: Duration::from_millis(5),
            },
        );

        let result = hub
            .request_set_flow(0, 1.0)
            .blocking_recv()
            .expect("actor reply");

        assert!(result.is_err());
        hub.stop();
    }
    #[test]
    fn reset_invalidates_commands_submitted_while_protection_was_latched() {
        let mut actor = MfcActor::new(
            Box::new(MockTransport::mfc()),
            [1, 2, 3, 4],
            [0.1, 1.0, 0.1, 0.1],
            5.0,
            5.0,
        );
        actor.set_flow(0, 3.0).unwrap();
        actor.set_on_off(0, false).unwrap();
        for _ in 0..4 {
            actor.poll_next(Instant::now());
        }
        let safety = actor.safety_latch();
        safety.trip();
        let epoch = safety.epoch();
        let (reset_tx, reset_rx) = oneshot::channel();
        let (flow_tx, flow_rx) = oneshot::channel();
        let (on_tx, on_rx) = oneshot::channel();
        let reset = QueuedMfc {
            epoch,
            req: MfcReq::Reset { reply: reset_tx },
        };
        let flow = QueuedMfc {
            epoch,
            req: MfcReq::SetFlow {
                gas: 0,
                value: 3.0,
                reply: flow_tx,
            },
        };
        let on = QueuedMfc {
            epoch,
            req: MfcReq::SetOnOff {
                gas: 0,
                on: true,
                reply: on_tx,
            },
        };
        dispatch_mfc(&mut actor, reset);
        assert!(reset_rx.blocking_recv().unwrap().is_ok());
        dispatch_mfc(&mut actor, flow);
        assert_eq!(
            flow_rx.blocking_recv().unwrap(),
            Err(MfcCommandError::SafetyLatched)
        );
        dispatch_mfc(&mut actor, on);
        assert_eq!(
            on_rx.blocking_recv().unwrap(),
            Err(MfcCommandError::SafetyLatched)
        );
        assert_eq!(actor.snapshot().gas[0].sv, 0.0);
        assert!(
            actor.set_on_off(0, true).is_ok(),
            "a genuinely new command after acknowledged reset is allowed"
        );
        assert_eq!(actor.snapshot().gas[0].sv, 3.0);
    }
}
