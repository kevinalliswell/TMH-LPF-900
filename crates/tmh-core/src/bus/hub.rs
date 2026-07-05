//! `DeviceHub`: spawns the three bus actors on dedicated OS threads (docs/03) and
//! bridges them to the async orchestration layer. Snapshots flow out through
//! `tokio::watch`; commands (set flow / on-off / shutoff / tare) flow in through
//! `tokio::mpsc` channels carrying a `oneshot` reply. Blocking serial I/O stays on
//! the threads, never on the async runtime. All handles are `Send + Sync` so the
//! hub can live in Tauri's managed state.

use crate::bus::{
    BalanceActor, BalanceSnapshot, MfcActor, MfcCommand, MfcSnapshot, TempActor, TempSnapshot,
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
    SetFlow {
        gas: usize,
        value: f64,
        reply: oneshot::Sender<ClampResult>,
    },
    SetOnOff {
        gas: usize,
        on: bool,
    },
    Shutoff,
}

enum BalReq {
    Tare { reply: oneshot::Sender<TareResult> },
}

/// Owns the running bus threads. Dropping it stops and joins them.
pub struct DeviceHub {
    stop: Arc<AtomicBool>,
    handles: Vec<JoinHandle<()>>,
    temp_rx: watch::Receiver<TempSnapshot>,
    mfc_rx: watch::Receiver<MfcSnapshot>,
    balance_rx: watch::Receiver<BalanceSnapshot>,
    mfc_tx: mpsc::UnboundedSender<MfcReq>,
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
        let (mfc_tx, mfc_cmd_rx) = mpsc::unbounded_channel::<MfcReq>();
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
                    while let Ok(c) = tt_rx.try_recv() {
                        temp.set_target_c(c);
                    }
                    temp.poll_once(Instant::now(), Duration::from_millis(100));
                    let _ = temp_tx.send(temp.snapshot());
                    if !temp.snapshot().connected {
                        temp.reconnect();
                    }
                    thread::sleep(iv.temp);
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
                    while let Ok(req) = cmd_rx.try_recv() {
                        match req {
                            MfcReq::SetFlow { gas, value, reply } => {
                                let _ = reply.send(mfc.set_flow(gas, value));
                            }
                            MfcReq::SetOnOff { gas, on } => {
                                mfc.handle_command(&MfcCommand::SetOnOff { gas, on });
                            }
                            MfcReq::Shutoff => {
                                mfc.handle_command(&MfcCommand::ShutoffFlammable);
                            }
                        }
                    }
                    mfc.poll_next(Instant::now());
                    let _ = mfc_snap_tx.send(mfc.snapshot());
                    if !mfc.snapshot().connected {
                        mfc.reconnect();
                    }
                    thread::sleep(iv.mfc);
                }
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
                    while let Ok(g) = bt_rx.try_recv() {
                        balance.set_target_g(g);
                    }
                    if let Ok(BalReq::Tare { reply }) = cmd_rx.try_recv() {
                        let _ = reply.send(balance.tare());
                    } else {
                        balance.poll_once(Instant::now(), Duration::from_millis(100));
                    }
                    let _ = bal_snap_tx.send(balance.snapshot());
                    if !balance.snapshot().connected {
                        balance.reconnect();
                    }
                    thread::sleep(iv.balance);
                }
            }));
        }

        Self {
            stop,
            handles,
            temp_rx,
            mfc_rx,
            balance_rx,
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

    /// Request a gas setpoint; await the returned receiver for the clamp result.
    pub fn request_set_flow(&self, gas: usize, value: f64) -> oneshot::Receiver<ClampResult> {
        let (reply, rx) = oneshot::channel();
        let _ = self.mfc_tx.send(MfcReq::SetFlow { gas, value, reply });
        rx
    }
    pub fn set_on_off(&self, gas: usize, on: bool) {
        let _ = self.mfc_tx.send(MfcReq::SetOnOff { gas, on });
    }
    /// Shut off flammable gases (H2/CO2/CO → 0). Never blocked by the clamp.
    pub fn shutoff_flammable(&self) {
        let _ = self.mfc_tx.send(MfcReq::Shutoff);
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
    use crate::transport::MockTransport;

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

        let clamp = hub.request_set_flow(0, 8.0).blocking_recv().expect("reply");
        assert_eq!(clamp.applied, 5.0);
        assert!(clamp.clamped);

        hub.stop();
    }

    #[test]
    fn hub_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<DeviceHub>();
    }
}
