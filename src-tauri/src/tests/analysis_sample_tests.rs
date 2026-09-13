use super::*;

fn experiment(state: &AppState) -> ExperimentRow {
    let mut value = lock(&state.storage)
        .get_experiment("exp-alice")
        .unwrap()
        .unwrap();
    value.sample_weight = 100.0;
    value
}

fn measured(seconds: f64, weight: f64) -> SampleRow {
    let mut value = row("REDUCING", 0, weight, false);
    value.record_json = Some(serde_json::json!({"stage_elapsed_s": seconds}).to_string());
    value
}

fn manual_input() -> ReductionInput {
    serde_json::from_value(serde_json::json!({
        "w1": 0.0, "w2": 50.0, "manual_m1": 100.0, "manual_mt": 85.0,
        "source_record": "LAB-42 original mass record"
    }))
    .unwrap()
}

fn mixed_samples() -> Vec<SampleRow> {
    let mut baseline = measured(0.0, 100.0);
    baseline.record_json = Some(
        serde_json::json!({"stage_elapsed_s":0,"sample_role":"reduction_baseline"}).to_string(),
    );
    let mut offline = measured(180.0, 0.0);
    offline.system_message = Some("天平离线".into());
    let mut multiple = measured(240.0, 0.0);
    multiple.system_message = Some("天平离线; 温度数据无效或过期".into());
    let mut other = measured(300.0, 90.0);
    other.system_message = Some("MFC 数据无效或过期".into());
    let mut malformed = measured(400.0, 90.0);
    malformed.record_json = Some("broken JSON".into());
    vec![
        row("HEATING", 0, 100.0, false),
        row("COOLING", 0, 80.0, false),
        baseline,
        measured(60.0, 95.0),
        measured(120.0, 90.0),
        measured(11000.0, 80.0),
        offline,
        multiple,
        other,
        row("REDUCING", 0, 90.0, false),
        malformed,
        measured(360.0, 0.0),
    ]
}

#[test]
fn analysis_sample_counts_partition_real_rows_and_keep_baselines_as_a_subset() {
    let state = test_state();
    seed_users_and_experiments(&state);
    let result = calculate_reduction(
        &experiment(&state),
        &mixed_samples(),
        &serde_json::json!({"sample_interval_s":60}),
        &manual_input(),
    )
    .unwrap();
    let counts = &result["sample_evidence"];
    assert_eq!(counts["version"], "REDUCTION-SAMPLES-2026-09-13.1");
    assert_eq!(counts["scope"], "stored_reduction_rows");
    for (field, expected) in [
        ("total_sample_count", 12),
        ("reduction_sample_count", 10),
        ("valid_sample_count", 4),
        ("excluded_offline_rows", 2),
        ("excluded_other_quality_rows", 1),
        ("excluded_time_rows", 2),
        ("excluded_value_rows", 1),
        ("non_reduction_rows", 2),
        ("valid_baseline_count", 1),
    ] {
        assert_eq!(counts[field], expected, "{field}");
    }
    assert_eq!(result["valid_sample_count"], 4);
    assert_eq!(result["excluded_offline_rows"], 2);
    assert!((result["rt"].as_f64().unwrap() - 15.0 / 21.5 * 100.0).abs() < 1e-10);
}

#[test]
fn analysis_sample_counts_do_not_count_gap_markers_or_an_interpolated_endpoint() {
    let state = test_state();
    seed_users_and_experiments(&state);
    let points = vec![
        measured(0.0, 100.0),
        measured(60.0, 99.0),
        measured(179.0 * 60.0, 89.25),
        measured(181.0 * 60.0, 87.1),
    ];
    let input: ReductionInput =
        serde_json::from_value(serde_json::json!({"w1":0.0,"w2":50.0})).unwrap();
    let curve = reduction_curve(&points, 100.0, 100.0, &input, 180.0);
    assert!(
        curve.points.len() > points.len(),
        "fixture contains an explicit synthetic gap marker"
    );
    let result = calculate_reduction(
        &experiment(&state),
        &points,
        &serde_json::json!({"reduction_m1":100.0,"sample_interval_s":120}),
        &input,
    )
    .unwrap();
    assert!((result["rt"].as_f64().unwrap() - 55.0).abs() < 1e-10);
    assert_eq!(result["valid_sample_count"], 4);
    assert_eq!(result["sample_evidence"]["total_sample_count"], 4);
    assert_eq!(result["manual_used"], false);
}

#[test]
fn analysis_sample_counts_are_frozen_per_analysis_and_manual_points_are_not_samples() {
    let state = test_state();
    seed_users_and_experiments(&state);
    let row = experiment(&state);
    let mut input = manual_input();
    input.manual_rate_points = Some(vec![(0.0, 30.0), (10.0, 60.0)]);
    let first = calculate_reduction(&row, &[], &serde_json::json!({}), &input).unwrap();
    assert_eq!(first["valid_sample_count"], 0);
    assert_eq!(first["sample_evidence"]["total_sample_count"], 0);
    assert_eq!(first["manual_used"], true);
    assert!((first["reduction_rate_index"].as_f64().unwrap() - 3.36).abs() < 1e-12);
    persist_analysis(&state, &row.experiment_id, "GB/T 13241", "Alice", &first).unwrap();
    let second = calculate_reduction(
        &row,
        &mixed_samples(),
        &serde_json::json!({"sample_interval_s":60}),
        &input,
    )
    .unwrap();
    persist_analysis(&state, &row.experiment_id, "GB/T 13241", "Alice", &second).unwrap();
    let revisions = lock(&state.storage)
        .analysis_revisions(&row.experiment_id)
        .unwrap();
    assert_eq!(revisions.len(), 2);
    assert_eq!(revisions[0]["valid_sample_count"], 4);
    assert_eq!(revisions[1]["valid_sample_count"], 0);
    assert_eq!(revisions[1]["computed_by"], "Alice");
    // Failed automatic endpoint analysis cannot replace the previously saved evidence.
    let automatic: ReductionInput =
        serde_json::from_value(serde_json::json!({"w1":0.0,"w2":50.0})).unwrap();
    assert!(calculate_reduction(
        &row,
        &[],
        &serde_json::json!({"reduction_m1":100}),
        &automatic
    )
    .is_err());
    assert_eq!(
        lock(&state.storage)
            .analysis_revisions(&row.experiment_id)
            .unwrap(),
        revisions
    );
}
