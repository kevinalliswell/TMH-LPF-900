//! Operator acknowledgements are bound to one run and one published stage.
use super::*;

#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ActionKind {
    Skip,
    Confirm,
}

#[derive(serde::Serialize)]
pub(super) struct StageAction {
    pub experiment_id: String,
    pub stage_index: usize,
    pub kind: ActionKind,
    pub notes: String,
    pub user_id: String,
    pub operator: String,
    pub requested_at: String,
}

impl StageAction {
    fn validate(&self, status: &ExperimentStatus) -> Result<(), String> {
        if !status.running
            || status.phase != Phase::Running
            || status.experiment_id.as_deref() != Some(&self.experiment_id)
            || status.stage_index != self.stage_index
        {
            return Err("实验或阶段已变化，请查看当前流程后重新操作".into());
        }
        if self.notes.chars().count() > 4096 {
            return Err("操作备注不能超过 4096 字".into());
        }
        match self.kind {
            ActionKind::Confirm if status.stage_kind == Some(StageKind::Manual) => Ok(()),
            ActionKind::Skip
                if matches!(
                    status.stage_kind,
                    Some(StageKind::Heating | StageKind::Stabilizing | StageKind::Reducing)
                ) && !self.notes.trim().is_empty() =>
            {
                Ok(())
            }
            _ => Err("此阶段不允许该操作；吹扫、取管确认和冷却不能跳过，跳段需填写原因".into()),
        }
    }
}

pub(super) fn queue_action(
    state: &AppState,
    experiment_id: String,
    stage_index: usize,
    kind: ActionKind,
    notes: String,
) -> Result<(), String> {
    let user = require_experiment_controller(state)?;
    let action = StageAction {
        experiment_id,
        stage_index,
        kind,
        notes: notes.trim().into(),
        user_id: user.user_id,
        operator: user.display_name,
        requested_at: iso_now(),
    };
    let mut pending = lock(&state.exp.stage_action);
    action.validate(&state.exp.snapshot())?;
    if pending.is_some() {
        return Err("上一项阶段操作尚未执行，请等待".into());
    }
    *pending = Some(action);
    Ok(())
}

pub(super) struct CycleResult {
    pub transition: tmh_core::engine::TickResult,
    pub rejected: Option<String>,
    pub action_applied: bool,
}

/// Account for the full cycle first. A timer transition wins over a now-stale
/// operator request; neither path may backdate the reduction baseline or final row.
pub(super) fn tick_cycle(
    engine: &mut ExperimentEngine,
    action: Option<&StageAction>,
    storage: &mut Storage,
    id: &str,
    t8: f64,
    dt: f64,
) -> Result<CycleResult, String> {
    let transition = engine.tick(t8, dt);
    let mut cycle = CycleResult {
        transition,
        rejected: None,
        action_applied: false,
    };
    let Some(action) = action else {
        return Ok(cycle);
    };
    let status = ExperimentStatus {
        running: true,
        phase: engine.phase(),
        experiment_id: Some(id.into()),
        stage_index: engine.stage_index(),
        stage_kind: engine.current_stage().map(|s| s.kind),
        ..ExperimentStatus::idle()
    };
    if let Err(error) = action.validate(&status) {
        cycle.rejected = Some(error);
        return Ok(cycle);
    }
    cycle.transition = apply_action(action, engine, storage, id)?;
    cycle.action_applied = true;
    Ok(cycle)
}

/// Persist operator evidence before releasing a manual gate or applying the next gas.
fn apply_action(
    action: &StageAction,
    engine: &mut ExperimentEngine,
    storage: &mut Storage,
    id: &str,
) -> Result<tmh_core::engine::TickResult, String> {
    let mut runtime: serde_json::Value =
        serde_json::from_str(&storage.runtime_metadata(id).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if runtime.get("stage_actions").is_none() {
        runtime["stage_actions"] = serde_json::json!([]);
    }
    let events = runtime["stage_actions"]
        .as_array_mut()
        .ok_or("阶段操作记录格式无效")?;
    let mut event = serde_json::to_value(action).map_err(|e| e.to_string())?;
    event["confirmed_at"] = serde_json::json!(iso_now());
    event["elapsed_s"] = serde_json::json!(engine.total_elapsed_s());
    events.push(event);
    if matches!(action.kind, ActionKind::Skip) {
        runtime["skipped"] = serde_json::json!(true);
    }
    storage
        .patch_runtime_metadata(id, &runtime.to_string())
        .map_err(|e| e.to_string())?;
    match action.kind {
        ActionKind::Skip => Ok(engine.skip_stage()),
        ActionKind::Confirm => engine.acknowledge_manual(action.stage_index),
    }
}

/// A successful standard ending has already acknowledged its final nonreactive gas
/// setpoints. Do not replace RI's nitrogen-off step with the fault atmosphere.
pub(super) fn confirm_normal_ending(hub: &DeviceHub, gas: [f64; 4]) -> Result<(), String> {
    if hub.safety_latched() {
        return Err("保护已锁存".into());
    }
    validate_ending_snapshot(&hub.mfc_rx().borrow().clone(), gas)?;
    hub.set_temp_target_c(25.0); // Simulation only; furnace operation belongs to its instrument.
    Ok(())
}

fn validate_ending_snapshot(
    state: &tmh_core::bus::MfcSnapshot,
    gas: [f64; 4],
) -> Result<(), String> {
    validate_ending_gas(gas)?;
    if state.safety_latched
        || !state.is_fresh(Instant::now())
        || state.gas.map(|g| g.sv) != gas
        || [0, 2, 3]
            .iter()
            .any(|&i| !state.gas[i].pv.is_finite() || state.gas[i].pv.abs() > 0.1)
    {
        return Err("正常结束的气氛状态未确认".into());
    }
    Ok(())
}

fn validate_ending_gas(gas: [f64; 4]) -> Result<(), String> {
    if gas != [0.0; 4] && gas != [0.0, 5.0, 0.0, 0.0] {
        return Err("正常结束必须为停气或 N₂ 5 L/min 保护".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn action(kind: ActionKind) -> StageAction {
        StageAction {
            experiment_id: "run-a".into(),
            stage_index: 3,
            kind,
            notes: "checked".into(),
            user_id: "operator-a".into(),
            operator: "Operator A".into(),
            requested_at: iso_now(),
        }
    }
    fn manual() -> ExperimentStatus {
        ExperimentStatus {
            running: true,
            phase: Phase::Running,
            experiment_id: Some("run-a".into()),
            stage_index: 3,
            stage_kind: Some(StageKind::Manual),
            ..ExperimentStatus::idle()
        }
    }
    #[test]
    fn action_cannot_target_another_run_or_a_stage_waiting_for_ack() {
        let a = action(ActionKind::Confirm);
        assert!(a.validate(&manual()).is_ok());
        let mut s = manual();
        s.experiment_id = Some("run-b".into());
        assert!(a.validate(&s).is_err());
        s = manual();
        s.stage_index = 4;
        assert!(a.validate(&s).is_err());
        s = manual();
        s.phase = Phase::StageTransition;
        assert!(a.validate(&s).is_err());
    }
    #[test]
    fn protected_tail_cannot_be_skipped() {
        for kind in [StageKind::Purging, StageKind::Manual, StageKind::Cooling] {
            let mut s = manual();
            s.stage_kind = Some(kind);
            assert!(action(ActionKind::Skip).validate(&s).is_err());
        }
    }
    #[test]
    fn normal_end_rejects_reactive_or_nonfinite_gas() {
        assert!(validate_ending_gas([0.0; 4]).is_ok());
        assert!(validate_ending_gas([0.0, 5.0, 0.0, 0.0]).is_ok());
        for gas in [
            [0.0, 10.5, 0.0, 4.5],
            [0.0, f64::NAN, 0.0, 0.0],
            [0.0, 15.0, 0.0, 0.0],
        ] {
            assert!(validate_ending_gas(gas).is_err());
        }
    }
    fn run_storage() -> Storage {
        let db = Storage::open_in_memory().unwrap();
        db.insert_experiment(&ExperimentRow {
            experiment_id: "run-a".into(),
            experiment_name: "QA".into(),
            sample_id: None,
            sample_name: "QA".into(),
            sample_weight: 500.0,
            start_time: iso_now(),
            experiment_time: None,
            end_time: None,
            project_name: None,
            description: None,
            operator: None,
            experiment_type: Some("REDUCIBILITY".into()),
            analysis_results_json: None,
            program_snapshot_json: None,
            created_at: iso_now(),
            owner_user_id: None,
        })
        .unwrap();
        db
    }
    #[test]
    fn confirming_final_manual_step_keeps_full_cycle_time_and_actual_actor() {
        let mut db = run_storage();
        let mut p = standard_program("REDUCIBILITY").unwrap();
        p.stages = vec![p.stages.last().unwrap().clone()];
        let mut e = ExperimentEngine::start(p);
        e.confirm_stage_started(2.0);
        let mut a = action(ActionKind::Confirm);
        a.stage_index = 0;
        let c = tick_cycle(&mut e, Some(&a), &mut db, "run-a", 25.0, 17.0).unwrap();
        assert!(c.transition.completed);
        assert_eq!(e.total_elapsed_s(), 19.0);
        let meta: serde_json::Value =
            serde_json::from_str(&db.runtime_metadata("run-a").unwrap()).unwrap();
        assert_eq!(meta["stage_actions"][0]["elapsed_s"], 19.0);
        assert_eq!(meta["stage_actions"][0]["user_id"], "operator-a");
        assert_eq!(meta["stage_actions"][0]["notes"], "checked");
        let repeat = tick_cycle(&mut e, Some(&a), &mut db, "run-a", 25.0, 1.0).unwrap();
        assert!(repeat.rejected.is_some());
        assert_eq!(e.total_elapsed_s(), 19.0);
    }
    #[test]
    fn automatic_stage_transition_wins_over_a_queued_skip_and_is_not_double_advanced() {
        let mut db = run_storage();
        let mut e = ExperimentEngine::start(standard_program("REDUCIBILITY").unwrap());
        e.confirm_stage_started(0.0);
        let mut a = action(ActionKind::Skip);
        a.stage_index = 0;
        let c = tick_cycle(&mut e, Some(&a), &mut db, "run-a", 900.0, 7.0).unwrap();
        assert_eq!(c.transition.apply_stage, Some(1));
        assert!(c.rejected.is_some());
        assert_eq!(e.stage_index(), 1);
        assert_eq!(e.total_elapsed_s(), 7.0);
        assert!(!db
            .runtime_metadata("run-a")
            .unwrap()
            .contains("stage_actions"));
    }
    #[test]
    fn broken_action_record_does_not_release_manual_gate() {
        let mut db = run_storage();
        db.patch_runtime_metadata("run-a", r#"{"stage_actions":"corrupt"}"#)
            .unwrap();
        let mut p = standard_program("REDUCIBILITY").unwrap();
        p.stages = vec![p.stages.last().unwrap().clone()];
        let mut e = ExperimentEngine::start(p);
        e.confirm_stage_started(0.0);
        let mut a = action(ActionKind::Confirm);
        a.stage_index = 0;
        assert!(tick_cycle(&mut e, Some(&a), &mut db, "run-a", 25.0, 1.0).is_err());
        assert_eq!(e.stage_index(), 0);
        assert_eq!(e.phase(), Phase::Running);
    }
    #[test]
    fn normal_ending_requires_fresh_measured_reactive_gas_zero() {
        let cfg = CommConfig::from_json(DEFAULT_COMM_CONFIG).unwrap();
        let mut state = MfcActor::from_config(Box::new(MockTransport::mfc()), &cfg).snapshot();
        state.connected = true;
        for f in &mut state.freshness {
            f.success(Instant::now());
        }
        for g in &mut state.gas {
            g.pv = 0.0;
            g.sv = 0.0;
        }
        assert!(validate_ending_snapshot(&state, [0.0; 4]).is_ok());
        for pv in [4.5, f64::NAN, f64::INFINITY] {
            state.gas[3].pv = pv;
            assert!(validate_ending_snapshot(&state, [0.0; 4]).is_err());
        }
        state.gas[3].pv = 0.0;
        state.freshness[3].fail();
        assert!(validate_ending_snapshot(&state, [0.0; 4]).is_err());
    }
}
