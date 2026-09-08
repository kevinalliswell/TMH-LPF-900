//! Experiment sampling (docs/06 §6.3). At 1 Hz during a run, turn the current
//! telemetry snapshot into an `experiment_data` row. When the balance is offline the
//! row is still written (weight = 0.0, annotated) — rows must never be dropped
//! (a real bug in the old system).

use crate::storage::SampleRow;
use crate::telemetry::TelemetrySnapshot;

/// Per-experiment context needed to build sample rows.
#[derive(Debug, Clone)]
pub struct SampleContext {
    pub experiment_id: String,
    /// Initial sample weight W0 (grams); weight loss (grams) is relative to this.
    pub initial_weight: f64,
}

/// Build one `experiment_data` row from a telemetry snapshot.
pub fn build_sample_row(
    ctx: &SampleContext,
    snap: &TelemetrySnapshot,
    timestamp: String,
    elapsed_secs: u64,
    status: &str,
) -> SampleRow {
    let balance_offline = !snap.health.balance_connected || !snap.health.balance_valid;
    let weight = if balance_offline {
        0.0
    } else {
        snap.weight.unwrap_or(0.0)
    };
    // Weight loss in grams (matches the report/export "失重(g)" column and the GB Rt
    // formula's ΔW = W0 − Wt). Floored at 0 so a tare/noise blip can't go negative.
    let weight_loss = (ctx.initial_weight - weight).max(0.0);
    let mut issues = Vec::new();
    if balance_offline {
        issues.push("天平离线");
    }
    if !snap.health.temp_valid {
        issues.push("温度数据无效或过期");
    }
    if !snap.health.mfc_valid {
        issues.push("MFC 数据无效或过期");
    }
    let system_message = (!issues.is_empty()).then(|| issues.join("; "));
    SampleRow {
        record_json: Some(serde_json::json!({"schema_version":1,"telemetry":snap}).to_string()),
        experiment_id: ctx.experiment_id.clone(),
        timestamp,
        experiment_duration: Some(fmt_hms(elapsed_secs)),
        // Sample temperature is T8; unreadable -> 0.0 (never falsely "at target").
        temperature: snap.temps[7].unwrap_or(0.0),
        weight,
        weight_loss,
        h2_flow: snap.gas[0].pv,
        n2_flow: snap.gas[1].pv,
        co2_flow: snap.gas[2].pv,
        co_flow: snap.gas[3].pv,
        experiment_status: Some(status.to_string()),
        system_message,
    }
}

fn fmt_hms(secs: u64) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::GasReading;
    use crate::telemetry::{DeviceHealth, TelemetrySnapshot};

    fn snap(balance_connected: bool, weight: Option<f64>) -> TelemetrySnapshot {
        let mut temps = [None; 9];
        temps[7] = Some(900.0);
        TelemetrySnapshot {
            seq: 1,
            temps,
            gas: [
                GasReading {
                    pv: 0.0,
                    sv: 0.0,
                    on: false,
                },
                GasReading {
                    pv: 10.5,
                    sv: 10.5,
                    on: true,
                },
                GasReading {
                    pv: 0.0,
                    sv: 0.0,
                    on: false,
                },
                GasReading {
                    pv: 4.5,
                    sv: 4.5,
                    on: true,
                },
            ],
            weight,
            health: DeviceHealth {
                safety_latched: false,
                temp_valid: true,
                mfc_valid: true,
                balance_valid: true,
                temp_connected: true,
                mfc_connected: true,
                balance_connected,
            },
        }
    }

    #[test]
    fn normal_row_computes_weight_loss_and_maps_gas() {
        let ctx = SampleContext {
            experiment_id: "e1".into(),
            initial_weight: 100.0,
        };
        let row = build_sample_row(
            &ctx,
            &snap(true, Some(85.0)),
            "2026-07-05T10:00:00".into(),
            3661,
            "RUNNING",
        );
        assert_eq!(row.temperature, 900.0); // T8
        assert_eq!(row.weight, 85.0);
        assert!((row.weight_loss - 15.0).abs() < 1e-9); // 100 - 85 = 15 g
        assert_eq!(row.n2_flow, 10.5);
        assert_eq!(row.co_flow, 4.5);
        assert_eq!(row.experiment_duration.as_deref(), Some("01:01:01"));
        assert_eq!(row.system_message, None);
    }

    #[test]
    fn balance_offline_writes_zero_weight_annotated_row() {
        let ctx = SampleContext {
            experiment_id: "e1".into(),
            initial_weight: 100.0,
        };
        let row = build_sample_row(
            &ctx,
            &snap(false, None),
            "2026-07-05T10:00:00".into(),
            0,
            "RUNNING",
        );
        assert_eq!(row.weight, 0.0);
        assert_eq!(row.system_message.as_deref(), Some("天平离线"));
        // still a full row — never dropped
        assert_eq!(row.experiment_id, "e1");
    }

    #[test]
    fn zero_initial_weight_avoids_division() {
        let ctx = SampleContext {
            experiment_id: "e1".into(),
            initial_weight: 0.0,
        };
        let row = build_sample_row(&ctx, &snap(true, Some(5.0)), "t".into(), 0, "RUNNING");
        assert_eq!(row.weight_loss, 0.0);
    }
}
