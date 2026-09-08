// Test-only contract for the record assessor; it reports facts and missing
// evidence, and does not certify full conformity with a national standard.
use serde_json::{json, Value};
use tmh_core::process_evidence::assess;
use tmh_core::program::{standard_program, Program};
use tmh_core::storage::SampleRow;

fn runtime() -> Value {
    json!({
        "outcome":"completed", "data_complete":true, "ending_action":"normal",
        "device_mode":"mock", "stage_actions":[],
        "gas_safety_limits":{"flow_steps":[0.1,1.0,0.1,0.1]}
    })
}

fn point(
    program: &Program,
    index: usize,
    temperature: f64,
    elapsed: f64,
    stable: f64,
) -> SampleRow {
    let stage = &program.stages[index];
    let gas: Vec<_> = stage
        .gas
        .iter()
        .map(|value| json!({"pv":value,"sv":value,"on":*value > 0.0}))
        .collect();
    let mut temperatures = vec![Value::Null; 9];
    temperatures[7] = json!(temperature);
    SampleRow {
        experiment_id: "run-evidence".into(),
        timestamp: "2026-09-08T12:00:00Z".into(),
        experiment_duration: Some("02:46:40".into()),
        temperature,
        weight: 490.0,
        weight_loss: 10.0,
        h2_flow: stage.gas[0],
        n2_flow: stage.gas[1],
        co2_flow: stage.gas[2],
        co_flow: stage.gas[3],
        experiment_status: Some(
            serde_json::to_value(stage.kind)
                .unwrap()
                .as_str()
                .unwrap()
                .into(),
        ),
        system_message: None,
        record_json: Some(
            json!({
                "schema_version":1,"stage_index":index,
                "elapsed_s":10000.0 + elapsed,"stage_elapsed_s":elapsed,"stable_elapsed_s":stable,
                "telemetry":{
                    "seq":1,"temps":temperatures,"gas":gas,"weight":490.0,
                    "health":{"temp_valid":true,"mfc_valid":true,"balance_valid":true,
                        "temp_connected":true,"mfc_connected":true,"balance_connected":true,
                        "safety_latched":false}
                }
            })
            .to_string(),
        ),
    }
}

fn patch(row: &mut SampleRow, edit: impl FnOnce(&mut Value)) {
    let mut raw: Value = serde_json::from_str(row.record_json.as_deref().unwrap()).unwrap();
    edit(&mut raw);
    row.record_json = Some(raw.to_string());
}

fn has_note(notes: &[String], marker: &str) -> bool {
    notes.iter().any(|note| note.contains(marker))
}

#[test]
fn samples_require_explicit_stage_indices_and_baselines_never_become_process_points() {
    // FREE_SWELLING contains two different MANUAL steps at indices 3 and 5.
    let program = standard_program("FREE_SWELLING").unwrap();
    let first_manual = point(&program, 3, 80.0, 12.0, 0.0);
    let last_manual = point(&program, 5, 25.0, 8.0, 0.0);
    let mut rows = vec![first_manual.clone(), last_manual];
    for index in [
        json!(-1),
        json!(1.5),
        json!("3"),
        json!(program.stages.len()),
    ] {
        let mut invalid = first_manual.clone();
        patch(&mut invalid, |raw| raw["stage_index"] = index);
        rows.push(invalid);
    }
    let mut no_index = first_manual.clone();
    patch(&mut no_index, |raw| {
        raw.as_object_mut().unwrap().remove("stage_index");
    });
    rows.push(no_index);
    let mut malformed = first_manual;
    malformed.record_json = Some("not-json".into());
    rows.push(malformed);

    let mut indexed_baseline = point(&program, 2, 900.0, 0.0, 0.0);
    patch(&mut indexed_baseline, |raw| {
        raw["sample_role"] = json!("reduction_baseline");
        raw["telemetry"]["gas"][1]["sv"] = json!(15.0);
    });
    let mut actual_native_baseline = indexed_baseline.clone();
    // Native m1 captures precede the reducing-gas ACK and have no stage_index.
    patch(&mut actual_native_baseline, |raw| {
        raw.as_object_mut().unwrap().remove("stage_index");
    });
    rows.extend([indexed_baseline, actual_native_baseline]);

    let result = assess(&program, &rows, &runtime());
    assert!(!result.version.trim().is_empty());
    assert_eq!(
        result.unassigned_points, 6,
        "baseline rows are excluded before attribution"
    );
    assert_eq!(result.stages.len(), program.stages.len());
    assert_eq!(
        result
            .stages
            .iter()
            .map(|stage| stage.index)
            .collect::<Vec<_>>(),
        (0..6).collect::<Vec<_>>()
    );
    assert_eq!(result.stages[3].points, 1);
    assert_eq!(result.stages[3].temperature_range, Some([80.0, 80.0]));
    assert_eq!(result.stages[5].points, 1);
    assert_eq!(result.stages[5].temperature_range, Some([25.0, 25.0]));
    assert_eq!(
        result
            .stages
            .iter()
            .map(|stage| stage.points)
            .sum::<usize>(),
        2
    );
    for index in [0, 1, 2, 4] {
        let stage = &result.stages[index];
        assert_eq!(stage.points, 0);
        assert_eq!(stage.temperature_range, None);
        assert_eq!(stage.elapsed_s, None);
        assert_eq!(stage.stable_elapsed_s, None);
        assert_eq!(stage.gas_sv_mismatches, 0);
        assert!(has_note(&stage.notes, "缺少阶段采样"));
    }
}

#[test]
fn invalid_placeholders_never_enter_measurements_and_sv_comparison_uses_recorded_resolution() {
    let program = standard_program("FREE_SWELLING").unwrap();
    let valid = point(&program, 1, 899.0, 1.0, 1.0);
    let mut placeholder = point(&program, 1, 0.0, 2.0, 0.0);
    placeholder.n2_flow = 0.0;
    patch(&mut placeholder, |raw| {
        raw["telemetry"]["health"]["temp_valid"] = json!(false);
        raw["telemetry"]["health"]["mfc_valid"] = json!(false);
        raw["telemetry"]["gas"][1]["pv"] = json!(0.0);
        raw["telemetry"]["gas"][1]["sv"] = json!(0.0);
    });
    let mut only_gas = point(&program, 1, 0.0, 3.0, 0.0);
    patch(&mut only_gas, |raw| {
        raw["telemetry"]["health"]["temp_valid"] = json!(false)
    });
    let mut only_temperature = point(&program, 1, 901.0, 4.0, 1.0);
    patch(&mut only_temperature, |raw| {
        raw["telemetry"]["health"]["mfc_valid"] = json!(false)
    });
    let mut nonfinite = point(&program, 1, f64::NAN, 5.0, 0.0);
    nonfinite.n2_flow = f64::INFINITY;
    patch(&mut nonfinite, |raw| {
        raw["telemetry"]["gas"][1]["pv"] = Value::Null;
        raw["telemetry"]["gas"][1]["sv"] = Value::Null;
    });
    let mut missing_health = point(&program, 1, 900.0, 6.0, 0.0);
    patch(&mut missing_health, |raw| {
        raw["telemetry"].as_object_mut().unwrap().remove("health");
    });
    let mut mismatch = point(&program, 1, 900.0, 7.0, 1.0);
    patch(&mut mismatch, |raw| {
        raw["telemetry"]["gas"][1]["sv"] = json!(14.0)
    });
    let mut wire_tail = point(&program, 2, 900.0, 1.0, 1.0);
    patch(&mut wire_tail, |raw| {
        raw["telemetry"]["gas"][3]["sv"] = json!(4.500000000000001)
    });

    let result = assess(
        &program,
        &[
            valid,
            placeholder,
            only_gas,
            only_temperature,
            nonfinite,
            missing_health,
            mismatch,
            wire_tail,
        ],
        &runtime(),
    );
    let stage = &result.stages[1];
    assert_eq!(stage.points, 7);
    assert_eq!(stage.valid_temperature_points, 3);
    assert_eq!(stage.valid_gas_points, 3);
    assert_eq!(stage.temperature_range, Some([899.0, 901.0]));
    assert_eq!(stage.gas_sv_mismatches, 1);
    assert_eq!(result.stages[2].valid_gas_points, 1);
    assert_eq!(
        result.stages[2].gas_sv_mismatches, 0,
        "equivalent wire units are not an SV deviation"
    );
}

#[test]
fn elapsed_and_continuous_stability_are_read_from_recorded_clocks_not_global_time_or_sample_count()
{
    let program = standard_program("FREE_SWELLING").unwrap();
    let mut without_clocks = point(&program, 1, 900.0, 9999.0, 9999.0);
    patch(&mut without_clocks, |raw| {
        raw.as_object_mut().unwrap().remove("stage_elapsed_s");
        raw.as_object_mut().unwrap().remove("stable_elapsed_s");
    });
    let rows = [
        point(&program, 1, 900.0, 30.0, 30.0),
        point(&program, 1, 900.0, 90.0, 10.0),
        point(&program, 1, 900.0, 110.0, 30.0),
        without_clocks,
        point(&program, 2, 900.0, 5.0, 5.0),
    ];
    let result = assess(&program, &rows, &runtime());
    assert_eq!(result.stages[1].elapsed_s, Some(110.0));
    assert_eq!(
        result.stages[1].stable_elapsed_s,
        Some(30.0),
        "do not sum separate stability holds across a reset"
    );
    assert_eq!(result.stages[2].elapsed_s, Some(5.0));
    assert_eq!(result.stages[0].elapsed_s, None);
    assert_eq!(result.stages[0].stable_elapsed_s, None);
}

#[test]
fn cooling_requires_strictly_below_threshold_and_each_manual_step_needs_its_own_complete_confirmation(
) {
    let program = standard_program("FREE_SWELLING").unwrap();
    let mut metadata = runtime();
    metadata["stage_actions"] = json!([
        {"kind":"confirm","stage_index":3,"operator":"Alice","notes":"已关炉并取管","confirmed_at":"2026-09-08T12:00:00Z"},
        {"kind":"skip","stage_index":5,"operator":"Alice","notes":"This is not a confirmation"},
        {"kind":"confirm","stage_index":5,"operator":"  ","notes":"Room temperature"},
        {"kind":"confirm","stage_index":5,"operator":"Alice","notes":"  "}
    ]);
    let mut samples = [
        point(&program, 3, 80.0, 12.0, 0.0),
        point(&program, 4, 50.0, 60.0, 0.0),
        point(&program, 5, 25.0, 8.0, 0.0),
    ];
    let boundary = assess(&program, &samples, &metadata);
    assert!(has_note(&boundary.stages[4].notes, "冷却末点未低于阈值"));
    assert!(!has_note(&boundary.stages[3].notes, "缺少本段人工确认"));
    assert!(has_note(&boundary.stages[5].notes, "缺少本段人工确认"));

    samples[1] = point(&program, 4, 49.9, 61.0, 0.0);
    metadata["stage_actions"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "kind":"confirm","stage_index":5,"operator":"Bob","notes":"已确认试料为室温且低于50℃",
            "confirmed_at":"2026-09-08T12:02:00Z"
        }));
    let confirmed = assess(&program, &samples, &metadata);
    assert!(!has_note(&confirmed.stages[4].notes, "冷却末点未低于阈值"));
    assert!(!has_note(&confirmed.stages[5].notes, "缺少本段人工确认"));
    // A terminal outcome alone cannot replace the entirely missing other stages.
    for index in [0, 1, 2] {
        assert!(has_note(&confirmed.stages[index].notes, "缺少阶段采样"));
    }
}

#[test]
fn rendered_process_evidence_lists_missing_stages_and_escapes_operator_labels() {
    let mut program = standard_program("FREE_SWELLING").unwrap();
    program.stages[0].name = "Heating <script>unsafe</script>".into();
    let assessment = assess(&program, &[], &runtime());
    let html = tmh_core::process_evidence::render(&assessment);
    assert!(html.contains("Heating &lt;script&gt;unsafe&lt;/script&gt;"));
    assert!(!html.contains("<script>"));
    assert!(html.contains("缺少阶段采样"));
    assert!(html.contains("不能证明国标方法完全符合"));
    assert_eq!(assessment.method_conformity, "not_determined");
}
// Append to crates/tmh-core/tests/process_evidence_requirements.rs.
// Reuses point, patch, runtime, has_note, assess and standard_program there.

fn assert_unclocked_cooling_endpoint_remains_the_endpoint(temperature_is_valid: bool) {
    let program = standard_program("FREE_SWELLING").unwrap();
    let earlier = point(&program, 4, 49.0, 60.0, 0.0);
    let mut last = point(
        &program,
        4,
        if temperature_is_valid { 70.0 } else { 0.0 },
        61.0,
        0.0,
    );
    last.timestamp = "2026-09-08T12:01:00Z".into();
    patch(&mut last, |raw| {
        raw.as_object_mut().unwrap().remove("stage_elapsed_s");
        if !temperature_is_valid {
            raw["telemetry"]["health"]["temp_valid"] = json!(false);
            raw["telemetry"]["temps"][7] = Value::Null;
        }
    });
    // Native storage supplies samples in insertion order. An endpoint with an
    // incomplete clock must not be moved before the older 49-degree sample.
    let result = assess(&program, &[earlier, last], &runtime());
    let cooling = &result.stages[4];
    assert_eq!(cooling.points, 2);
    assert_eq!(
        cooling.valid_temperature_points,
        if temperature_is_valid { 2 } else { 1 }
    );
    assert!(
        has_note(&cooling.notes, "冷却末点未低于阈值"),
        "an above-threshold or invalid final measurement cannot inherit the earlier low endpoint"
    );
    assert!(has_note(&cooling.notes, "缺少阶段计时"));
}

#[test]
fn a_later_hot_cooling_sample_without_stage_time_is_not_hidden_by_sorting() {
    assert_unclocked_cooling_endpoint_remains_the_endpoint(true);
}

#[test]
fn a_later_invalid_cooling_sample_without_stage_time_does_not_reuse_an_earlier_temperature() {
    assert_unclocked_cooling_endpoint_remains_the_endpoint(false);
}

#[test]
fn an_unrepresentable_required_duration_keeps_the_frozen_assessment_json_readable() {
    let mut program = standard_program("FREE_SWELLING").unwrap();
    program.stages[2].duration_min = 1e308;
    assert!(program.stages[2].duration_min.is_finite());
    assert!(tmh_core::program::validate_program(&program).is_ok());
    let assessment = assess(
        &program,
        &[point(&program, 2, 900.0, 3600.0, 3600.0)],
        &runtime(),
    );
    // FrozenTrial stores this as Option<ProcessAssessment>. The concrete choice
    // for an unrepresentable required_seconds is deliberately left unspecified.
    let encoded = serde_json::to_value(Some(assessment)).unwrap();
    let restored: tmh_core::process_evidence::ProcessAssessment = serde_json::from_value(
        encoded.clone(),
    )
    .expect(
        "a valid finite input must not produce null in a required f64 and poison the saved group",
    );
    assert_eq!(restored.stages[2].points, 1);
    assert_eq!(restored.stages[2].elapsed_s, Some(3600.0));
    assert!(
        has_note(&restored.stages[2].notes, "时长无法核定"),
        "overflow is missing evidence, not a verified duration"
    );
    assert_eq!(serde_json::to_value(&restored).unwrap(), encoded);
    let optional: Option<tmh_core::process_evidence::ProcessAssessment> =
        serde_json::from_value(encoded).unwrap();
    assert!(optional.is_some());
}

#[test]
fn known_dropped_samples_remain_visible_when_the_surviving_stage_records_have_no_gaps() {
    let mut program = standard_program("FREE_SWELLING").unwrap();
    program.stages = vec![program.stages[2].clone()];
    let rows = [point(&program, 0, 900.0, 3600.0, 3600.0)];
    let mut metadata = runtime();
    metadata["data_complete"] = json!(false);
    metadata["dropped_samples"] = json!(7);
    let assessment = assess(&program, &rows, &metadata);
    assert!(assessment.stages[0].notes.is_empty());
    assert_eq!(assessment.unassigned_points, 0);
    let frozen = serde_json::to_value(&assessment).unwrap();
    assert_eq!(frozen["data_complete"], false);
    assert_eq!(frozen["dropped_samples"], 7);
    let restored: tmh_core::process_evidence::ProcessAssessment =
        serde_json::from_value(frozen).unwrap();
    let html = tmh_core::process_evidence::render(&restored);
    assert!(html.contains("数据不完整"));
    assert!(html.contains("已知丢样：7 条"));
    assert_eq!(restored.method_conformity, "not_determined");
}

#[test]
fn a_complete_marker_does_not_override_a_positive_dropped_sample_count() {
    let program = standard_program("FREE_SWELLING").unwrap();
    let mut metadata = runtime();
    metadata["data_complete"] = json!(true);
    metadata["dropped_samples"] = json!(12);
    let assessment = assess(&program, &[], &metadata);
    let frozen = serde_json::to_value(&assessment).unwrap();
    assert_eq!(
        frozen["data_complete"], true,
        "preserve the recorded marker as evidence"
    );
    assert_eq!(frozen["dropped_samples"], 12);
    let html = tmh_core::process_evidence::render(&assessment);
    assert!(html.contains("相互冲突"));
    assert!(html.contains("已知丢样：12 条"));
}

#[test]
fn missing_or_invalid_integrity_fields_stay_unknown_in_new_and_legacy_assessments() {
    let program = standard_program("FREE_SWELLING").unwrap();
    for metadata in [
        json!({}),
        json!({"data_complete":"true","dropped_samples":-1}),
        json!({"data_complete":1,"dropped_samples":1.5}),
        json!({"data_complete":null,"dropped_samples":"0"}),
    ] {
        let assessment = assess(&program, &[], &metadata);
        let html = tmh_core::process_evidence::render(&assessment);
        assert!(html.contains("数据完整性未知"));
        assert!(html.contains("丢样数量未知"));
    }
    // Crash recovery records data_complete=false without inventing a lost count.
    let recovered = assess(&program, &[], &json!({"data_complete":false}));
    let html = tmh_core::process_evidence::render(&recovered);
    assert!(html.contains("数据不完整"));
    assert!(html.contains("丢样数量未知"));

    let mut legacy = serde_json::to_value(assess(&program, &[], &runtime())).unwrap();
    let object = legacy.as_object_mut().unwrap();
    object.remove("data_complete");
    object.remove("dropped_samples");
    object.insert("version".into(), json!("PROCESS-EVIDENCE-2026-09-08.1"));
    let restored: tmh_core::process_evidence::ProcessAssessment =
        serde_json::from_value(legacy).expect("older frozen group summaries remain readable");
    assert_eq!(restored.version, "PROCESS-EVIDENCE-2026-09-08.1");
    let html = tmh_core::process_evidence::render(&restored);
    assert!(html.contains("数据完整性未知"));
    assert!(html.contains("丢样数量未知"));
}
