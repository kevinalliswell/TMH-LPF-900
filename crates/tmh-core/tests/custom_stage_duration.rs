//! Unsupported legacy combinations must fail validation, never acquire new timing semantics.
use tmh_core::engine::{standard_program, validate_program, ProgramSource};
#[test]
fn reject_positive_heating_and_cooling_durations() {
    for index in [0, 4] {
        let mut p = standard_program("LOW_TEMP_DEGRADATION").unwrap();
        p.id = "custom-legacy".into();
        p.source = ProgramSource::Custom;
        p.stages[index].duration_min = 10.0;
        assert!(
            validate_program(&p).is_err(),
            "unsupported duration on {:?}",
            p.stages[index].kind
        );
    }
}
#[test]
fn reject_zero_duration_reaction_and_stabilization_or_purge() {
    for index in [1, 2, 3] {
        let mut p = standard_program("REDUCIBILITY").unwrap();
        p.id = "custom-legacy".into();
        p.source = ProgramSource::Custom;
        p.stages[index].duration_min = 0.0;
        assert!(
            validate_program(&p).is_err(),
            "zero duration on {:?}",
            p.stages[index].kind
        );
    }
}
#[test]
fn manual_confirmation_does_not_accept_a_fake_scheduled_duration() {
    let mut p = standard_program("REDUCIBILITY").unwrap();
    p.id = "custom-legacy".into();
    p.source = ProgramSource::Custom;
    p.stages[4].duration_min = 1.0;
    assert!(validate_program(&p).is_err());
}
