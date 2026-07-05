//! Telemetry aggregation (docs/03 §IPC, docs/06 §6.3). Merges the three bus
//! snapshots into one `TelemetrySnapshot` at the 0.2s tick. This is the in-memory
//! payload the src-tauri layer maps onto the frontend Channel (M3).

use crate::bus::{BalanceSnapshot, GasReading, MfcSnapshot, TempSnapshot};
use serde::Serialize;

/// Per-device connectivity for the combined snapshot.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceHealth {
    pub temp_connected: bool,
    pub mfc_connected: bool,
    pub balance_connected: bool,
}

/// One merged telemetry frame. Gas order: H2, N2, CO2, CO.
#[derive(Debug, Clone, Serialize)]
pub struct TelemetrySnapshot {
    pub seq: u64,
    pub temps: [Option<f64>; 9],
    pub gas: [GasReading; 4],
    pub weight: Option<f64>,
    pub health: DeviceHealth,
}

/// Merge the three device snapshots into one telemetry frame.
pub fn aggregate(
    seq: u64,
    temp: &TempSnapshot,
    mfc: &MfcSnapshot,
    balance: &BalanceSnapshot,
) -> TelemetrySnapshot {
    TelemetrySnapshot {
        seq,
        temps: temp.temps,
        gas: mfc.gas,
        weight: balance.weight,
        health: DeviceHealth {
            temp_connected: temp.connected,
            mfc_connected: mfc.connected,
            balance_connected: balance.connected,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::{BalanceActor, MfcActor, TempActor};
    use crate::transport::MockTransport;
    use std::time::{Duration, Instant};

    #[test]
    fn aggregate_merges_three_buses() {
        let mut t = TempActor::new(Box::new(MockTransport::temp()), 0, 0.1, false);
        t.poll_once(Instant::now(), Duration::from_millis(10));
        let m = MfcActor::new(
            Box::new(MockTransport::mfc()),
            [1, 2, 3, 4],
            [0.1, 1.0, 0.1, 0.1],
            5.0,
            5.0,
        );
        let mut b = BalanceActor::new(Box::new(MockTransport::balance(42.0)));
        b.poll_once(Instant::now(), Duration::from_millis(10));

        let snap = aggregate(7, &t.snapshot(), &m.snapshot(), &b.snapshot());
        assert_eq!(snap.seq, 7);
        assert!(snap.temps[7].is_some());
        assert_eq!(snap.weight, Some(42.0));
        assert!(snap.health.temp_connected);
        assert!(snap.health.balance_connected);
    }
}
