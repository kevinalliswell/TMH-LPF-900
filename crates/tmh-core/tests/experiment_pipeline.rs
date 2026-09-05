//! Headless integration: drive the experiment engine against a mock `DeviceHub` and an
//! in-memory `Storage`, proving the full control pipeline (M4-3) without the Tauri glue:
//! the furnace follows the program stages, the reduction stage loses mass and flows CO,
//! samples persist, and the safety atmosphere is applied on completion.

use std::thread::sleep;
use std::time::Duration;
use tmh_core::bus::{BalanceActor, DeviceHub, HubIntervals, MfcActor, TempActor};
use tmh_core::engine::{
    standard_program, ExperimentEngine, Phase, Stage, StageKind, SAFETY_ATMOSPHERE,
};
use tmh_core::storage::{ExperimentRow, SampleRow, Storage};
use tmh_core::transport::MockTransport;

const GAS_H2: usize = 0;
const GAS_N2: usize = 1;
const GAS_CO: usize = 3;

fn mock_hub() -> DeviceHub {
    let temp = TempActor::new(Box::new(MockTransport::temp()), 0, 0.1, false);
    let mfc = MfcActor::new(
        Box::new(MockTransport::mfc()),
        [1, 2, 3, 4],
        [0.1, 1.0, 0.1, 0.1],
        5.0,
        5.0,
    );
    let balance = BalanceActor::new(Box::new(MockTransport::balance(100.0)));
    let iv = HubIntervals {
        temp: Duration::from_millis(2),
        mfc: Duration::from_millis(2),
        balance: Duration::from_millis(2),
    };
    DeviceHub::spawn(temp, mfc, balance, iv)
}

fn apply_stage(hub: &DeviceHub, stage: &Stage, w0: f64) {
    for (gas, &v) in stage.gas.iter().enumerate() {
        drop(hub.request_set_flow(gas, v));
    }
    hub.set_temp_target_c(stage.target_temp);
    if stage.kind == StageKind::Reducing {
        hub.set_balance_target_g(w0 * 0.88);
    }
}

#[test]
fn reducibility_run_drives_devices_and_persists() {
    let hub = mock_hub();
    let mut storage = Storage::open_in_memory().unwrap();
    let w0 = 100.0;
    storage
        .insert_experiment(&ExperimentRow {
            experiment_id: "e1".into(),
            experiment_name: "t".into(),
            sample_id: None,
            sample_name: "s".into(),
            sample_weight: w0,
            start_time: "t0".into(),
            experiment_time: None,
            end_time: None,
            project_name: None,
            description: None,
            operator: None,
            experiment_type: Some("REDUCIBILITY".into()),
            analysis_results_json: None,
            program_snapshot_json: None,
            created_at: "t0".into(),
            owner_user_id: None,
        })
        .unwrap();

    let mut engine = ExperimentEngine::start(standard_program("REDUCIBILITY").unwrap());
    apply_stage(&hub, engine.current_stage().unwrap(), w0);

    let mut samples: Vec<SampleRow> = Vec::new();
    let mut peak_temp = 0.0f64;
    let mut min_weight = w0;
    let mut reached_reducing = false;
    let mut co_during_reduction = 0.0f64;
    let mut guard = 0;

    loop {
        guard += 1;
        assert!(
            guard < 4000,
            "run should finish; stuck at {:?} stage {}",
            engine.phase(),
            engine.stage_index()
        );
        sleep(Duration::from_millis(5));

        let t8 = hub.temp_rx().borrow().temps[7].unwrap_or(25.0);
        peak_temp = peak_temp.max(t8);
        let w = hub.balance_rx().borrow().weight.unwrap_or(w0);
        min_weight = min_weight.min(w);

        // Advance a full simulated hour per tick to clear the holds quickly.
        let r = engine.tick(t8, 3600.0);
        if r.apply_stage.is_some() {
            if let Some(s) = engine.current_stage() {
                apply_stage(&hub, s, w0);
                if s.kind == StageKind::Reducing {
                    reached_reducing = true;
                }
            }
        }
        if engine.current_stage().map(|s| s.kind) == Some(StageKind::Reducing) {
            sleep(Duration::from_millis(5)); // let the CO setpoint + mass drift propagate
            co_during_reduction = co_during_reduction.max(hub.mfc_rx().borrow().gas[GAS_CO].sv);
        }

        let gas = hub.mfc_rx().borrow().gas;
        samples.push(SampleRow {
            record_json: None,
            experiment_id: "e1".into(),
            timestamp: "t".into(),
            experiment_duration: Some("00:00:00".into()),
            temperature: t8,
            weight: w,
            weight_loss: (w0 - w).max(0.0),
            co_flow: gas[GAS_CO].sv,
            co2_flow: gas[2].sv,
            n2_flow: gas[GAS_N2].sv,
            h2_flow: gas[GAS_H2].sv,
            experiment_status: Some("RUNNING".into()),
            system_message: None,
        });

        if r.completed {
            break;
        }
    }

    // Completion exit path → safety atmosphere.
    for (gas, &v) in SAFETY_ATMOSPHERE.iter().enumerate() {
        drop(hub.request_set_flow(gas, v));
    }
    sleep(Duration::from_millis(20));
    let final_gas = hub.mfc_rx().borrow().gas;

    storage.insert_samples(&samples).unwrap();

    assert_eq!(engine.phase(), Phase::Completing);
    assert!(
        peak_temp > 895.0,
        "furnace reached ~900 °C (peak {peak_temp})"
    );
    assert!(reached_reducing, "entered the reduction stage");
    assert!(
        co_during_reduction > 4.0,
        "CO flowed during reduction (max sv {co_during_reduction})"
    );
    assert!(min_weight < 99.0, "sample lost mass (min {min_weight} g)");
    assert!(
        (final_gas[GAS_N2].sv - 5.0).abs() < 1e-6,
        "safety N2=5 (got {})",
        final_gas[GAS_N2].sv
    );
    assert!(
        final_gas[GAS_CO].sv.abs() < 1e-6,
        "safety CO=0 (got {})",
        final_gas[GAS_CO].sv
    );
    assert!(
        final_gas[GAS_H2].sv.abs() < 1e-6,
        "safety H2=0 (got {})",
        final_gas[GAS_H2].sv
    );
    assert!(
        storage.count_samples("e1").unwrap() > 5,
        "samples persisted"
    );

    hub.stop();
}
