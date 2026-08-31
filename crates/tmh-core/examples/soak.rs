//! Headless soak (docs/08, docs/10 M2). Fast-forwards the full pipeline — bus actors
//! → telemetry aggregate → sampler → rusqlite storage — with no real sleeps, or
//! replays a recorded dirty session. Verifies: no panic, exact sampling row count,
//! and that a dirty replay session runs to completion with parse errors counted.
//!
//! Usage:
//!   cargo run -p tmh-core --example soak                       # mock, 60 sim-minutes
//!   cargo run -p tmh-core --example soak -- --minutes 120      # mock, 2 sim-hours
//!   cargo run -p tmh-core --example soak -- --replay fixtures/dirty-session.jsonl

use std::time::{Duration, Instant};
use tmh_core::bus::{BalanceActor, MfcActor, TempActor};
use tmh_core::sampler::{build_sample_row, SampleContext};
use tmh_core::storage::{ExperimentRow, SampleRow, Storage};
use tmh_core::telemetry::aggregate;
use tmh_core::transport::{BusError, MockTransport, ReplayTransport, Transport};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--replay") {
        let path = args.get(i + 1).expect("--replay needs a path");
        replay_soak(path);
    } else {
        let minutes = arg_value(&args, "--minutes")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(60);
        mock_soak(minutes);
    }
}

fn arg_value<'a>(args: &'a [String], key: &str) -> Option<&'a String> {
    args.iter()
        .position(|a| a == key)
        .and_then(|i| args.get(i + 1))
}

fn mock_soak(minutes: u64) {
    let sim_seconds = minutes * 60;

    let mut m = MockTransport::temp();
    m.set_temp_target_raw(9000); // 900.0 °C
    let mut temp = TempActor::new(Box::new(m), 0, 0.1, false);
    let mut mfc = MfcActor::new(
        Box::new(MockTransport::mfc()),
        [1, 2, 3, 4],
        [0.1, 1.0, 0.1, 0.1],
        5.0,
        5.0,
    );
    let mut balance = BalanceActor::new(Box::new(MockTransport::balance(100.0)));

    // Reducing atmosphere (also exercises the clamp): CO=4.5, N2=10.5 L/min.
    mfc.set_flow(1, 10.5).expect("mock MFC write ack");
    mfc.set_flow(3, 4.5).expect("mock MFC write ack");

    let dir = std::env::temp_dir().join("tmh_soak");
    let _ = std::fs::create_dir_all(&dir);
    let db = dir.join("soak.db");
    let _ = std::fs::remove_file(&db);
    let mut storage = Storage::open(&db).expect("open db");
    let exp_id = "soak-exp".to_string();
    storage
        .insert_experiment(&ExperimentRow {
            experiment_id: exp_id.clone(),
            experiment_name: "soak".into(),
            sample_name: "s".into(),
            sample_weight: 100.0,
            start_time: "2026-07-05T00:00:00".into(),
            end_time: None,
            description: None,
            operator: None,
            experiment_type: None,
            analysis_results_json: None,
            created_at: "2026-07-05T00:00:00".into(),
        })
        .unwrap();

    let ctx = SampleContext {
        experiment_id: exp_id.clone(),
        initial_weight: 100.0,
    };
    let mut seq = 0u64;
    let mut batch: Vec<SampleRow> = Vec::new();
    let start = Instant::now();

    for sec in 0..sim_seconds {
        // 5 telemetry ticks per simulated second (0.2s cadence).
        for _ in 0..5 {
            temp.poll_once(Instant::now(), Duration::from_millis(50));
            mfc.poll_next(Instant::now());
            balance.poll_once(Instant::now(), Duration::from_millis(50));
            seq += 1;
        }
        // Backoff-driven reconnect stand-in: reopen any offline bus.
        if !temp.snapshot().connected {
            temp.reconnect();
        }
        if !mfc.snapshot().connected {
            mfc.reconnect();
        }
        if !balance.snapshot().connected {
            balance.reconnect();
        }

        // 1 Hz experiment sample.
        let snap = aggregate(seq, &temp.snapshot(), &mfc.snapshot(), &balance.snapshot());
        batch.push(build_sample_row(
            &ctx,
            &snap,
            format!("2026-07-05T{sec}"),
            sec,
            "RUNNING",
        ));
        if batch.len() >= 60 {
            storage.insert_samples(&batch).unwrap();
            batch.clear();
        }
    }
    if !batch.is_empty() {
        storage.insert_samples(&batch).unwrap();
    }

    let rows = storage.count_samples(&exp_id).unwrap();
    println!(
        "mock soak: {sim_seconds} sim-seconds -> {rows} rows in {:?} wall, T8={:?}",
        start.elapsed(),
        temp.snapshot().temps[7]
    );
    assert_eq!(
        rows, sim_seconds as i64,
        "exactly one sample row per simulated second"
    );
    println!("OK: no panic, exact row count.");
    let _ = std::fs::remove_dir_all(&dir);
}

fn replay_soak(path: &str) {
    let text = std::fs::read_to_string(path).expect("read replay session");
    let mut rt = ReplayTransport::from_jsonl(&text).expect("parse jsonl");
    let req = tmh_protocols::modbus::build_read_all(0);
    let (mut ok, mut parse_err, mut timeouts) = (0u64, 0u64, 0u64);
    loop {
        match rt.transact(&req, Duration::from_millis(10)) {
            Ok(bytes) => {
                if tmh_protocols::modbus::extract_read_all(&bytes, 0).is_some() {
                    ok += 1;
                } else {
                    parse_err += 1;
                }
            }
            Err(BusError::Timeout) => timeouts += 1,
            Err(BusError::Disconnected) => break,
            Err(e) => panic!("unexpected transport error: {e:?}"),
        }
    }
    println!("replay soak: ok={ok} parse_errors={parse_err} timeouts={timeouts}");
    assert!(parse_err > 0, "dirty session must contain parse errors");
    println!("OK: dirty session ran to completion, no panic.");
}
