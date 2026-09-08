//! Independent F05/F06 tests for the proposed manual-stage acknowledgement API.
//! Tests deliberately target the agreed Purging/Manual additions before their
//! implementation. They do not command hardware or invent furnace-control APIs.

use tmh_core::engine::{standard_program, ExperimentEngine, Phase, StageKind, TickResult};

fn program_with_stages(kinds: &[StageKind]) -> ExperimentEngine {
    let mut program = standard_program("REDUCIBILITY").unwrap();
    let template = program.stages[0].clone();
    program.stages = kinds
        .iter()
        .enumerate()
        .map(|(index, kind)| {
            let mut stage = template.clone();
            stage.name = format!("Test stage {index}");
            stage.kind = *kind;
            stage.target_temp = 900.0;
            stage.tolerance = 5.0;
            stage.gas = [0.0, 5.0, 0.0, 0.0];
            stage.duration_min = match kind {
                StageKind::Stabilizing | StageKind::Reducing => 1.0,
                StageKind::Purging => 5.0,
                _ => 0.0,
            };
            if *kind == StageKind::Cooling {
                stage.target_temp = 50.0;
            }
            stage
        })
        .collect();
    let mut engine = ExperimentEngine::start(program);
    engine.confirm_stage_started(0.0);
    engine
}

fn assert_no_advance(result: TickResult) {
    assert_eq!(result.apply_stage, None);
    assert!(!result.completed);
}

#[test]
fn manual_stage_never_completes_from_temperature_or_elapsed_time() {
    let mut engine = program_with_stages(&[StageKind::Manual, StageKind::Cooling]);
    for temperature in [900.0, 25.0, 0.0, 1000.0, f64::NAN] {
        assert_no_advance(engine.tick(temperature, 24.0 * 60.0 * 60.0));
        assert_eq!(engine.stage_index(), 0);
        assert_eq!(engine.current_stage().unwrap().kind, StageKind::Manual);
    }
}

#[test]
fn acknowledging_the_current_manual_stage_advances_exactly_once() {
    let mut engine =
        program_with_stages(&[StageKind::Manual, StageKind::Manual, StageKind::Cooling]);
    let first = engine.acknowledge_manual(0).unwrap();
    assert_eq!(first.apply_stage, Some(1));
    assert!(!first.completed);
    assert_eq!(engine.stage_index(), 1);

    assert!(engine.acknowledge_manual(0).is_err());
    assert_eq!(
        engine.stage_index(),
        1,
        "a duplicate click must not approve the next action"
    );

    engine.confirm_stage_started(0.0);
    let second = engine.acknowledge_manual(1).unwrap();
    assert_eq!(second.apply_stage, Some(2));
    assert!(!second.completed);
}

#[test]
fn manual_acknowledgement_rejects_future_and_out_of_range_indices() {
    let mut engine = program_with_stages(&[StageKind::Manual, StageKind::Cooling]);
    for index in [1, usize::MAX] {
        assert!(engine.acknowledge_manual(index).is_err());
        assert_eq!(engine.stage_index(), 0);
        assert_eq!(engine.phase(), Phase::Running);
    }
}

#[test]
fn acknowledgement_cannot_advance_non_manual_stages() {
    for kind in [
        StageKind::Heating,
        StageKind::Stabilizing,
        StageKind::Reducing,
        StageKind::Purging,
        StageKind::Cooling,
    ] {
        let mut engine = program_with_stages(&[kind, StageKind::Manual]);
        assert!(engine.acknowledge_manual(0).is_err(), "{kind:?}");
        assert_eq!(engine.stage_index(), 0, "{kind:?}");
        assert_eq!(engine.phase(), Phase::Running, "{kind:?}");
    }
}

#[test]
fn a_new_manual_stage_can_be_acknowledged_after_its_gas_switch_without_an_extra_tick() {
    let mut engine =
        program_with_stages(&[StageKind::Purging, StageKind::Manual, StageKind::Cooling]);
    assert_eq!(engine.tick(900.0, 300.0).apply_stage, Some(1));
    // The acknowledged gas switch makes this an active stage immediately;
    // waiting for another timer tick is not an additional prerequisite.
    engine.confirm_stage_started(0.5);
    let result = engine.acknowledge_manual(1).unwrap();
    assert_eq!(result.apply_stage, Some(2));
    assert!(!result.completed);
}

#[test]
fn an_unacknowledged_gas_switch_cannot_be_bypassed_by_manual_confirmation_or_a_tick() {
    let mut engine =
        program_with_stages(&[StageKind::Purging, StageKind::Manual, StageKind::Cooling]);
    assert_eq!(engine.tick(900.0, 300.0).apply_stage, Some(1));
    assert!(engine.acknowledge_manual(1).is_err());
    assert_eq!(engine.stage_index(), 1);

    assert_no_advance(engine.tick(25.0, 1.0));
    assert!(
        engine.acknowledge_manual(1).is_err(),
        "a timer tick cannot stand in for a confirmed gas switch"
    );
    assert_eq!(engine.stage_index(), 1);

    engine.confirm_stage_started(0.5);
    assert_eq!(engine.acknowledge_manual(1).unwrap().apply_stage, Some(2));
}

#[test]
fn final_manual_acknowledgement_completes_and_repeated_acknowledgement_is_rejected() {
    let mut engine = program_with_stages(&[StageKind::Manual]);
    let result = engine.acknowledge_manual(0).unwrap();
    assert!(result.completed);
    assert_eq!(result.apply_stage, None);
    assert_eq!(engine.phase(), Phase::Completing);
    let completed_index = engine.stage_index();

    assert!(engine.acknowledge_manual(0).is_err());
    assert!(engine.acknowledge_manual(completed_index).is_err());
    assert_eq!(engine.stage_index(), completed_index);
    assert_eq!(engine.phase(), Phase::Completing);
}

#[test]
fn normal_end_stages_cannot_be_bypassed_by_skip() {
    for kind in [StageKind::Purging, StageKind::Manual, StageKind::Cooling] {
        let mut engine = program_with_stages(&[kind, StageKind::Manual]);
        let before_phase = engine.phase();
        let before_elapsed = engine.stage_elapsed_s();
        assert_no_advance(engine.skip_stage());
        assert_eq!(engine.stage_index(), 0, "{kind:?}");
        assert_eq!(engine.phase(), before_phase, "{kind:?}");
        assert_eq!(engine.stage_elapsed_s(), before_elapsed, "{kind:?}");
    }
}

#[test]
fn a_terminal_normal_end_stage_cannot_be_marked_complete_by_skip() {
    for kind in [StageKind::Purging, StageKind::Manual, StageKind::Cooling] {
        let mut engine = program_with_stages(&[kind]);
        assert_no_advance(engine.skip_stage());
        assert_eq!(engine.stage_index(), 0, "{kind:?}");
        assert_eq!(engine.phase(), Phase::Running, "{kind:?}");
    }
}

#[test]
fn authorized_skip_of_an_ordinary_stage_still_enters_the_protected_tail() {
    let mut engine =
        program_with_stages(&[StageKind::Reducing, StageKind::Purging, StageKind::Manual]);
    let result = engine.skip_stage();
    assert_eq!(result.apply_stage, Some(1));
    assert!(!result.completed);
    assert_eq!(engine.current_stage().unwrap().kind, StageKind::Purging);
    assert_no_advance(engine.skip_stage());
    assert_eq!(engine.stage_index(), 1);
}

#[test]
fn reduction_ends_at_the_fixed_duration_even_if_the_last_temperature_is_out_of_band() {
    let mut engine = program_with_stages(&[StageKind::Reducing, StageKind::Purging]);
    assert_no_advance(engine.tick(900.0, 59.0));
    let result = engine.tick(880.0, 1.0);
    assert_eq!(result.apply_stage, Some(1));
    assert!(!result.completed);
    assert_eq!(engine.total_elapsed_s(), 60.0);
}

#[test]
fn a_brief_reduction_temperature_excursion_does_not_restart_its_duration() {
    let mut engine = program_with_stages(&[StageKind::Reducing, StageKind::Purging]);
    assert_no_advance(engine.tick(900.0, 29.0));
    assert_no_advance(engine.tick(880.0, 1.0));
    assert_no_advance(engine.tick(900.0, 29.0));
    assert_eq!(engine.tick(900.0, 1.0).apply_stage, Some(1));
    assert_eq!(engine.total_elapsed_s(), 60.0);
}

#[test]
fn purge_duration_is_fixed_and_does_not_depend_on_remaining_at_reduction_temperature() {
    let mut engine = program_with_stages(&[StageKind::Purging, StageKind::Manual]);
    assert_no_advance(engine.tick(25.0, 299.0));
    assert_eq!(engine.tick(25.0, 1.0).apply_stage, Some(1));
    assert_eq!(engine.current_stage().unwrap().kind, StageKind::Manual);
}
