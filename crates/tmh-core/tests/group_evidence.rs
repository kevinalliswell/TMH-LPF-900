use serde_json::json;
use tmh_core::{gb::CALCULATION_VERSION, group_report, storage::TestGroupRevision, test_group::*};

fn trial(id: &str, day: u32) -> FrozenTrial {
    let programs: serde_json::Value =
        serde_json::from_str(include_str!("../../../config/standard_programs.json")).unwrap();
    let analysis = json!({"standard":"GB/T 13241","standard_version":"2017",
        "calculation_version":CALCULATION_VERSION,"ri":42.0,"reduction_rate_index":1.25});
    serde_json::from_value(json!({
        "experiment": {"experiment_id":id,"experiment_name":"Trial","sample_name":"Pellets",
        "sample_weight":500.0,"start_time":format!("2026-09-{day:02}T08:00:00Z"),
        "end_time":"2026-09-08T15:00:00Z","created_at":"2026-09-01T08:00:00Z",
        "program_snapshot_json":programs[0].to_string(),"analysis_results_json":analysis.to_string()},
        "runtime":{"outcome":"completed","data_complete":true,"ending_action":"normal","device_mode":"mock"},
        "analysis_revision_id":1,"exclusion_reason":null
    })).unwrap()
}
fn request() -> GroupRequest {
    serde_json::from_value(json!({"name":"Group <script>alert(1)</script>","batch_id":"Batch 1",
        "same_batch_confirmed":true,"material":"pellet","reason":"Initial group",
        "trials":[{"experiment_id":"a"},{"experiment_id":"bad","exclusion_reason":"Damaged <sample>"},{"experiment_id":"b"}]
    })).unwrap()
}
#[test]
fn frozen_report_preserves_exclusions_and_requires_valid_evidence() {
    let request = request();
    let mut excluded = trial("bad", 2);
    excluded.runtime["data_complete"] = json!(false);
    let snapshot = build_snapshot(
        &request,
        None,
        vec![trial("b", 3), excluded.clone(), trial("a", 1)],
        "Operator",
    )
    .unwrap();
    assert_eq!(snapshot.valid_trial_indices, vec![0, 2]);
    assert_eq!(snapshot.evaluation.decision.selected_indices, vec![0, 1]);
    assert_eq!(snapshot.evaluation.reported["RI"].as_deref(), Some("42.0"));
    let record = TestGroupRevision {
        group_id: "group-core".into(),
        revision: 1,
        owner_user_id: "owner".into(),
        recorded_by_user_id: "owner".into(),
        created_at: "2026-09-08T15:00:00Z".into(),
        snapshot: serde_json::to_value(&snapshot).unwrap(),
    };
    let html = group_report::render(&record).unwrap();
    assert!(html.contains("模拟 / 回放数据"));
    assert!(html.contains("Damaged &lt;sample&gt;"));
    assert!(!html.contains("<script>alert(1)</script>"));
    assert!(html.contains("尚未判定"));
    let mut invalid = request.clone();
    invalid.trials[1].exclusion_reason = None;
    assert!(build_snapshot(
        &invalid,
        None,
        vec![trial("a", 1), excluded, trial("b", 3)],
        "Operator"
    )
    .is_err());
    let mut skipped = trial("a", 1);
    skipped.runtime["stage_actions"] = json!([{"kind":"skip"}]);
    assert!(build_snapshot(
        &request,
        None,
        vec![skipped, trial("bad", 2), trial("b", 3)],
        "Operator"
    )
    .is_err());
    let mut different_source = trial("b", 3);
    different_source.runtime["device_mode"] = json!("real");
    assert!(build_snapshot(
        &request,
        None,
        vec![trial("a", 1), trial("bad", 2), different_source],
        "Operator"
    )
    .is_err());
    let mut moved = request.clone();
    moved.batch_id = "Other batch".into();
    assert!(build_snapshot(&moved, Some(&snapshot), snapshot.trials.clone(), "Operator").is_err());
}
