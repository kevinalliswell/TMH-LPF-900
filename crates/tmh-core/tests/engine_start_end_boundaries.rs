//! Complements manual_stage_requirements: initial gas ACK and the standard's
//! complete normal-end path. The gas assertions cover the engine/program contract
//! only; native finalization must separately prove it preserves these setpoints.

use tmh_core::engine::{
    standard_program, ExperimentEngine, Phase, Program, StageKind, TickResult, SAFETY_ATMOSPHERE,
};

fn two_stage_program(initial_kind: StageKind) -> Program {
    let mut program = standard_program("REDUCIBILITY").unwrap();
    let mut initial = program.stages[0].clone();
    initial.kind = initial_kind;
    initial.target_temp = 900.0;
    initial.tolerance = 5.0;
    initial.duration_min = if initial_kind == StageKind::Reducing {
        1.0
    } else {
        0.0
    };
    let mut final_stage = initial.clone();
    final_stage.kind = StageKind::Manual;
    final_stage.duration_min = 0.0;
    program.stages = vec![initial, final_stage];
    program
}

fn assert_no_advance(result: TickResult) {
    assert_eq!(result.apply_stage, None);
    assert!(!result.completed);
}

#[test]
fn initial_heating_cannot_advance_before_its_first_gas_ack() {
    let mut engine = ExperimentEngine::start(two_stage_program(StageKind::Heating));
    assert_no_advance(engine.tick(900.0, 1.0));
    assert_eq!(engine.stage_index(), 0);

    engine.confirm_stage_started(2.0);
    assert_eq!(engine.tick(900.0, 1.0).apply_stage, Some(1));
}

#[test]
fn initial_manual_stage_cannot_be_confirmed_before_its_first_gas_ack() {
    let mut engine = ExperimentEngine::start(two_stage_program(StageKind::Manual));
    assert!(engine.acknowledge_manual(0).is_err());
    assert_eq!(engine.stage_index(), 0);

    engine.confirm_stage_started(2.0);
    assert_eq!(engine.acknowledge_manual(0).unwrap().apply_stage, Some(1));
}

#[test]
fn initial_stage_cannot_be_skipped_before_its_first_gas_ack() {
    let mut engine = ExperimentEngine::start(two_stage_program(StageKind::Heating));
    assert_no_advance(engine.skip_stage());
    assert_eq!(engine.stage_index(), 0);

    engine.confirm_stage_started(2.0);
    assert_eq!(engine.skip_stage().apply_stage, Some(1));
}

#[test]
fn initial_reduction_exposure_does_not_accumulate_while_the_first_gas_ack_is_pending() {
    let mut engine = ExperimentEngine::start(two_stage_program(StageKind::Reducing));
    assert_no_advance(engine.tick(900.0, 59.0));
    assert_eq!(
        engine.stage_elapsed_s(),
        0.0,
        "pre-ACK waiting time cannot be reported as confirmed reduction exposure"
    );

    engine.confirm_stage_started(7.0);
    assert_no_advance(engine.tick(900.0, 59.0));
    assert_eq!(engine.tick(900.0, 1.0).apply_stage, Some(1));
}

/// Drive the actual standard program with explicit gas and operator ACKs. The
/// returned sequence is the setpoint contract emitted by the engine, not a fake
/// device's measurements or a substitute for testing the native completion path.
fn normal_standard_stage_sequence(mode: &str) -> Vec<(StageKind, [f64; 4])> {
    let program = standard_program(mode).unwrap();
    let maximum_stages = program.stages.len();
    let mut engine = ExperimentEngine::start(program);
    let mut acknowledged_stages = Vec::new();
    for _ in 0..maximum_stages {
        let stage_index = engine.stage_index();
        let stage = engine.current_stage().unwrap().clone();
        acknowledged_stages.push((stage.kind, stage.gas));
        engine.confirm_stage_started(0.25);
        let result = match stage.kind {
            StageKind::Manual => engine.acknowledge_manual(stage_index).unwrap(),
            StageKind::Cooling => engine.tick(stage.target_temp - 1.0, 1.0),
            StageKind::Heating => engine.tick(stage.target_temp, 1.0),
            StageKind::Stabilizing | StageKind::Reducing | StageKind::Purging => {
                engine.tick(stage.target_temp, stage.duration_min * 60.0)
            }
        };
        if result.completed {
            assert_eq!(result.apply_stage, None);
            assert_eq!(engine.phase(), Phase::Completing);
            return acknowledged_stages;
        }
        assert_eq!(result.apply_stage, Some(stage_index + 1), "{mode}");
    }
    panic!("{mode} did not complete its acknowledged standard procedure");
}

#[test]
fn reducibility_normal_completion_preserves_nitrogen_off_after_purge_and_tube_removal() {
    let stages = normal_standard_stage_sequence("REDUCIBILITY");
    assert!(stages.len() >= 2);
    assert_eq!(
        stages[stages.len() - 2],
        (StageKind::Purging, [0.0, 15.0, 0.0, 0.0])
    );
    assert_eq!(
        stages.last().unwrap(),
        &(StageKind::Manual, [0.0, 0.0, 0.0, 0.0])
    );
    assert_ne!(
        stages.last().unwrap().1,
        SAFETY_ATMOSPHERE,
        "13241 normal completion must not reopen N2 to the abnormal-protection flow"
    );
}

#[test]
fn degradation_normal_completion_retains_the_nitrogen_cooling_atmosphere() {
    let stages = normal_standard_stage_sequence("LOW_TEMP_DEGRADATION");
    assert_eq!(stages.last().unwrap().1, [0.0, 5.0, 0.0, 0.0]);
}

#[test]
fn swelling_normal_completion_retains_nitrogen_after_room_temperature_confirmation() {
    let stages = normal_standard_stage_sequence("FREE_SWELLING");
    assert_eq!(
        stages.last().unwrap(),
        &(StageKind::Manual, [0.0, 5.0, 0.0, 0.0])
    );
}
