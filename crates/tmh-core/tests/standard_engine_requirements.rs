//! Independent F05/F06 regression cases against the supplied standards.
//!
//! Sources: GB/T 13241-2017 8.3.2-8.3.3, GB/T 13242-2017 8.3.2-8.3.3,
//! GB/T 13240-2018 9.2; see docs/13 for the verified source-page mapping.
//! These tests exercise the real public engine without devices or wall-clock waits.
//! A one-stage fixture isolates its temperature/time gate: it does not stand in
//! for the external tube-removal or furnace-power confirmations the app needs.

use tmh_core::engine::{standard_program, ExperimentEngine, StageKind, STANDARD_MODES};

fn isolated_stage(mode: &str, kind: StageKind) -> ExperimentEngine {
    let mut program = standard_program(mode).expect("known standard mode");
    let selected = program
        .stages
        .iter()
        .find(|stage| stage.kind == kind)
        .expect("standard contains the requested stage")
        .clone();
    program.stages = vec![selected];
    let mut engine = ExperimentEngine::start(program);
    engine.confirm_stage_started(0.0);
    engine
}

fn assert_pending(engine: &mut ExperimentEngine, temperature: f64, seconds: f64, context: &str) {
    let result = engine.tick(temperature, seconds);
    assert!(!result.completed, "{context}: stage finished prematurely");
    assert!(
        result.apply_stage.is_none(),
        "{context}: advanced before the stage condition was satisfied"
    );
}

#[test]
fn uninterrupted_stabilization_completes_after_its_full_hold() {
    for mode in STANDARD_MODES {
        let mut engine = isolated_stage(mode, StageKind::Stabilizing);
        let stage = engine.current_stage().unwrap().clone();
        let hold_seconds = (stage.duration_min * 60.0) as usize;
        for _ in 0..hold_seconds - 1 {
            assert_pending(&mut engine, stage.target_temp, 1.0, mode);
        }
        assert!(engine.tick(stage.target_temp, 1.0).completed, "{mode}");
    }
}

#[test]
fn stabilization_does_not_credit_time_before_temperature_is_in_range() {
    for mode in STANDARD_MODES {
        let mut engine = isolated_stage(mode, StageKind::Stabilizing);
        let stage = engine.current_stage().unwrap().clone();
        let hold_seconds = (stage.duration_min * 60.0) as usize;
        for _ in 0..hold_seconds {
            assert_pending(
                &mut engine,
                stage.target_temp - stage.tolerance - 1.0,
                1.0,
                mode,
            );
        }
        assert_pending(&mut engine, stage.target_temp, 1.0, mode);
    }
}

#[test]
fn stabilization_requires_a_new_full_hold_after_an_excursion() {
    for mode in STANDARD_MODES {
        let mut engine = isolated_stage(mode, StageKind::Stabilizing);
        let stage = engine.current_stage().unwrap().clone();
        let hold_seconds = (stage.duration_min * 60.0) as usize;
        for _ in 0..hold_seconds - 1 {
            assert_pending(&mut engine, stage.target_temp, 1.0, mode);
        }
        assert_pending(
            &mut engine,
            stage.target_temp + stage.tolerance + 1.0,
            1.0,
            mode,
        );
        // Returning to the band must not retain the almost-complete previous hold.
        for _ in 0..hold_seconds - 1 {
            assert_pending(&mut engine, stage.target_temp, 1.0, mode);
        }
        assert!(engine.tick(stage.target_temp, 1.0).completed, "{mode}");
    }
}

#[test]
fn stabilization_does_not_bridge_an_invalid_temperature_sample() {
    for mode in STANDARD_MODES {
        let mut engine = isolated_stage(mode, StageKind::Stabilizing);
        let stage = engine.current_stage().unwrap().clone();
        let hold_seconds = (stage.duration_min * 60.0) as usize;
        for _ in 0..hold_seconds - 1 {
            assert_pending(&mut engine, stage.target_temp, 1.0, mode);
        }
        // The current public API accepts f64, so NaN represents a sample with no
        // valid numeric temperature. No fake Option-based interface is introduced.
        assert_pending(&mut engine, f64::NAN, 1.0, mode);
        for _ in 0..hold_seconds - 1 {
            assert_pending(&mut engine, stage.target_temp, 1.0, mode);
        }
        assert!(engine.tick(stage.target_temp, 1.0).completed, "{mode}");
    }
}

#[test]
fn swelling_stabilization_accepts_the_specified_ten_degree_band() {
    let mut engine = isolated_stage("FREE_SWELLING", StageKind::Stabilizing);
    // 909 C is within the 900 +/- 10 C band required by 13240 9.2.
    for _ in 0..15 * 60 - 1 {
        assert_pending(&mut engine, 909.0, 1.0, "13240 stabilization");
    }
    assert!(engine.tick(909.0, 1.0).completed);
}

fn reducibility_at_normal_reduction_end() -> ExperimentEngine {
    let mut program = standard_program("REDUCIBILITY").unwrap();
    let reduction_index = program
        .stages
        .iter()
        .position(|stage| stage.kind == StageKind::Reducing)
        .unwrap();
    // Start a fixture at reduction, preserving the actual standard's entire tail.
    // No skip command is used: this is the ordinary timed completion path.
    program.stages.drain(..reduction_index);
    let mut engine = ExperimentEngine::start(program);
    engine.confirm_stage_started(0.0);
    let result = engine.tick(900.0, 180.0 * 60.0);
    assert!(!result.completed, "13241 requires a post-reduction purge");
    assert!(result.apply_stage.is_some());
    engine.confirm_stage_started(0.0);
    engine
}

#[test]
fn reducibility_normal_end_first_uses_fifteen_litres_of_nitrogen() {
    let engine = reducibility_at_normal_reduction_end();
    assert_eq!(
        engine.current_stage().unwrap().gas,
        [0.0, 15.0, 0.0, 0.0],
        "13241 8.3.3 requires N2 15 L/min before closing it after five minutes"
    );
}

#[test]
fn reducibility_cannot_finish_before_the_five_minute_purge_even_if_cold() {
    let mut engine = reducibility_at_normal_reduction_end();
    // Purge duration is mandatory; a low temperature must not bypass it.
    assert_pending(&mut engine, 25.0, 299.0, "13241 purge");
}

#[test]
fn degradation_cooling_temperature_gate_is_satisfied_below_one_hundred() {
    let mut engine = isolated_stage("LOW_TEMP_DEGRADATION", StageKind::Cooling);
    assert!(
        engine.tick(99.0, 1.0).completed,
        "13242 8.3.3 ends cooling below 100 C; it does not require 25-30 C"
    );
}

#[test]
fn degradation_cooling_temperature_gate_does_not_accept_one_hundred() {
    let mut engine = isolated_stage("LOW_TEMP_DEGRADATION", StageKind::Cooling);
    assert_pending(&mut engine, 100.0, 1.0, "13242 strict cooling limit");
}

#[test]
fn swelling_cooling_temperature_gate_does_not_accept_fifty() {
    let mut engine = isolated_stage("FREE_SWELLING", StageKind::Cooling);
    assert_pending(&mut engine, 50.0, 1.0, "13240 strict cooling limit");
}

#[test]
fn swelling_cooling_temperature_gate_does_not_add_a_five_degree_margin() {
    let mut engine = isolated_stage("FREE_SWELLING", StageKind::Cooling);
    assert_pending(&mut engine, 55.0, 1.0, "13240 strict cooling limit");
}

#[test]
fn swelling_cooling_temperature_gate_accepts_room_temperature_below_fifty() {
    let mut engine = isolated_stage("FREE_SWELLING", StageKind::Cooling);
    assert!(engine.tick(25.0, 1.0).completed);
}
