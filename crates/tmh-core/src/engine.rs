//! Experiment engine (docs/05 §5.1–5.4): phase state machine, the three GB standard
//! program tables, and stage-advancement rules. Pure logic — the orchestration layer
//! (src-tauri) ticks it with the current sample temperature (T8) and applies each
//! stage's atmosphere through the safety clamp. Sampling/DB/crash-recovery wire in
//! at the command layer.

use serde::{Deserialize, Serialize};

/// Protective atmosphere for interrupted or failed experiments. Normal completion
/// retains the final stage atmosphere defined by the selected standard.
/// Order [H2, N2, CO2, CO] L/min.
pub const SAFETY_ATMOSPHERE: [f64; 4] = [0.0, 5.0, 0.0, 0.0];

/// Experiment phase (docs/05 §5.1).
#[cfg_attr(feature = "typescript-export", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Phase {
    Idle,
    Configuring,
    Running,
    StageTransition,
    Completing,
    Stopping,
    Error,
}

/// Whether a transition is legal (docs/05 §5.1). Any other pair is `InvalidTransition`.
pub fn is_legal_transition(from: Phase, to: Phase) -> bool {
    use Phase::*;
    matches!(
        (from, to),
        (Idle, Configuring)
            | (Configuring, Running)
            | (Configuring, StageTransition)
            | (Configuring, Idle)
            | (Running, StageTransition)
            | (Running, Completing)
            | (Running, Stopping)
            | (Running, Error)
            | (StageTransition, Running)
            | (StageTransition, Completing)
            | (StageTransition, Stopping)
            | (StageTransition, Error)
            | (Completing, Idle)
            | (Stopping, Idle)
            | (Error, Idle)
    )
}

pub use crate::program::{
    standard_program, validate_program, Program, ProgramSource, Stage, StageKind, STANDARD_MODES,
};

/// A running experiment. Tick it once per second with the current T8.
pub struct ExperimentEngine {
    program: Program,
    phase: Phase,
    stage_index: usize,
    stage_elapsed_s: f64,
    stable_elapsed_s: f64,
    total_elapsed_s: f64,
}

/// Result of one engine tick.
#[derive(Debug, Clone, PartialEq)]
pub struct TickResult {
    /// The stage just changed (or the run just started): apply this stage's atmosphere.
    pub apply_stage: Option<usize>,
    /// The run completed on this tick; retain the validated final stage atmosphere.
    pub completed: bool,
}

impl ExperimentEngine {
    /// Start a run. The caller should apply stage 0's atmosphere immediately.
    pub fn start(program: Program) -> Self {
        Self {
            program,
            phase: Phase::StageTransition,
            stage_index: 0,
            stage_elapsed_s: 0.0,
            stable_elapsed_s: 0.0,
            total_elapsed_s: 0.0,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn stage_index(&self) -> usize {
        self.stage_index
    }
    pub fn total_stages(&self) -> usize {
        self.program.stages.len()
    }
    pub fn program(&self) -> &Program {
        &self.program
    }
    pub fn current_stage(&self) -> Option<&Stage> {
        self.program.stages.get(self.stage_index)
    }
    pub fn stage_elapsed_s(&self) -> f64 {
        self.stage_elapsed_s
    }
    pub fn total_elapsed_s(&self) -> f64 {
        self.total_elapsed_s
    }

    pub fn stable_elapsed_s(&self) -> f64 {
        self.stable_elapsed_s
    }

    /// A stage's exposure clock starts only when its gas switch is acknowledged.
    /// Transport delay remains part of the complete experiment timeline.
    pub fn confirm_stage_started(&mut self, switching_s: f64) {
        if self.phase != Phase::StageTransition || !switching_s.is_finite() || switching_s < 0.0 {
            return;
        }
        if switching_s.is_finite() && switching_s >= 0.0 {
            self.total_elapsed_s += switching_s;
        }
        self.stage_elapsed_s = 0.0;
        self.stable_elapsed_s = 0.0;
        if self.phase == Phase::StageTransition {
            self.phase = Phase::Running;
        }
    }

    /// Advance `dt` seconds with sample temperature `t8`. Returns which stage's
    /// atmosphere to (re)apply and whether the run completed.
    pub fn tick(&mut self, t8: f64, dt: f64) -> TickResult {
        if self.phase != Phase::Running || !dt.is_finite() || dt <= 0.0 {
            return TickResult {
                apply_stage: None,
                completed: false,
            };
        }
        self.phase = Phase::Running;
        self.stage_elapsed_s += dt;
        self.total_elapsed_s += dt;

        let at_temp = self
            .current_stage()
            .is_some_and(|s| t8.is_finite() && (t8 - s.target_temp).abs() <= s.tolerance);
        self.stable_elapsed_s = if at_temp {
            self.stable_elapsed_s + dt
        } else {
            0.0
        };

        let reached = self
            .current_stage()
            .map(|s| {
                stage_reached(
                    s,
                    t8,
                    if s.kind == StageKind::Stabilizing {
                        self.stable_elapsed_s
                    } else {
                        self.stage_elapsed_s
                    },
                )
            })
            .unwrap_or(true);
        if !reached {
            return TickResult {
                apply_stage: None,
                completed: false,
            };
        }

        self.advance()
    }

    /// Manual skip to the next stage (operator, with confirm + audit at the UI/command
    /// layer). Returns the new stage to apply, or completion.
    pub fn skip_stage(&mut self) -> TickResult {
        if self.phase != Phase::Running
            || !self.current_stage().is_some_and(|s| {
                matches!(
                    s.kind,
                    StageKind::Heating | StageKind::Stabilizing | StageKind::Reducing
                )
            })
        {
            return TickResult {
                apply_stage: None,
                completed: false,
            };
        }
        self.advance()
    }

    pub fn acknowledge_manual(&mut self, stage_index: usize) -> Result<TickResult, String> {
        if self.phase != Phase::Running
            || self.stage_index != stage_index
            || !self
                .current_stage()
                .is_some_and(|s| s.kind == StageKind::Manual)
        {
            return Err("当前阶段不接受此人工确认，或阶段气氛尚未确认".into());
        }
        Ok(self.advance())
    }

    fn advance(&mut self) -> TickResult {
        self.stage_index += 1;
        if self.stage_index >= self.program.stages.len() {
            self.phase = Phase::Completing;
            return TickResult {
                apply_stage: None,
                completed: true,
            };
        }
        self.stage_elapsed_s = 0.0;
        self.stable_elapsed_s = 0.0;
        self.phase = Phase::StageTransition;
        TickResult {
            apply_stage: Some(self.stage_index),
            completed: false,
        }
    }
}

/// Stage arrival test (docs/05 §5.4).
fn stage_reached(stage: &Stage, t8: f64, elapsed_s: f64) -> bool {
    if !t8.is_finite() {
        return false;
    }
    let at_temp = (t8 - stage.target_temp).abs() <= stage.tolerance;
    match stage.kind {
        StageKind::Manual => false,
        StageKind::Purging | StageKind::Reducing => elapsed_s >= stage.duration_min * 60.0,
        StageKind::Stabilizing => at_temp && elapsed_s >= stage.duration_min * 60.0,
        StageKind::Cooling => t8 < stage.target_temp,
        StageKind::Heating => at_temp,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_tables_match_docs_05() {
        let p = standard_program("REDUCIBILITY").unwrap();
        assert_eq!(p.source, ProgramSource::Standard);
        assert_eq!(p.sample_prefix, "RED");
        assert!(p.enabled);
        assert_eq!(p.stages.len(), 5);
        assert_eq!(p.stages[0].gas, [0.0, 5.0, 0.0, 0.0]);
        assert_eq!(p.stages[0].target_temp, 900.0);
        assert_eq!(p.stages[1].duration_min, 30.0);
        assert_eq!(p.stages[2].gas, [0.0, 10.5, 0.0, 4.5]); // CO=4.5 N2=10.5
        assert_eq!(p.stages[2].duration_min, 180.0);
        assert_eq!(p.stages[3].kind, StageKind::Purging);
        assert_eq!(p.stages[4].kind, StageKind::Manual);

        let d = standard_program("LOW_TEMP_DEGRADATION").unwrap();
        assert_eq!(d.sample_prefix, "RDI");
        assert_eq!(d.stages[0].target_temp, 500.0);
        assert_eq!(d.stages[2].gas, [0.0, 9.0, 3.0, 3.0]); // N2=9 CO2=3 CO=3
        assert_eq!(d.stages[2].duration_min, 60.0);

        let s = standard_program("FREE_SWELLING").unwrap();
        assert_eq!(s.sample_prefix, "FSI");
        assert_eq!(s.stages[0].gas, [0.0, 10.0, 0.0, 0.0]); // N2=10
        assert_eq!(s.stages[1].duration_min, 15.0);
        assert_eq!(s.stages[4].target_temp, 50.0);

        assert!(standard_program("NOPE").is_none());
    }

    #[test]
    fn legal_transitions() {
        assert!(is_legal_transition(Phase::Idle, Phase::Configuring));
        assert!(is_legal_transition(
            Phase::Configuring,
            Phase::StageTransition
        ));
        assert!(is_legal_transition(Phase::Running, Phase::Stopping));
        assert!(is_legal_transition(Phase::Completing, Phase::Idle));
        assert!(!is_legal_transition(Phase::Idle, Phase::Running));
        assert!(!is_legal_transition(Phase::Completing, Phase::Running));
    }

    #[test]
    fn cooling_requires_strictly_below_limit() {
        let cool = &standard_program("LOW_TEMP_DEGRADATION").unwrap().stages[4];
        assert!(stage_reached(cool, 99.9, 0.0));
        assert!(!stage_reached(cool, 100.0, 0.0));
    }

    #[test]
    fn hold_stage_needs_temp_and_duration() {
        let hold = &standard_program("REDUCIBILITY").unwrap().stages[1]; // 900±5, 30 min
        assert!(!stage_reached(hold, 900.0, 60.0)); // at temp but only 1 min
        assert!(!stage_reached(hold, 880.0, 2000.0)); // long enough but not at temp
        assert!(stage_reached(hold, 899.0, 1800.0)); // at temp AND 30 min
    }

    #[test]
    fn hold_stages_measure_real_elapsed_time_not_tick_count() {
        // The orchestration layer now feeds the engine the time that actually passed rather
        // than a fixed constant per loop iteration (#43). Uneven dt must therefore land the
        // 30 min hold at 30 min of accumulated time, no earlier and no later.
        let mut e = ExperimentEngine::start(standard_program("REDUCIBILITY").unwrap());
        e.confirm_stage_started(0.0);
        // Stage 0 is a dynamic heat to 900 °C: hold at 800 °C so nothing advances yet.
        let uneven = [0.7_f64, 1.4, 0.9, 1.0, 2.0];
        let mut elapsed = 0.0;
        for (i, dt) in uneven.iter().cycle().take(50).enumerate() {
            let r = e.tick(800.0, *dt);
            elapsed += dt;
            assert!(
                r.apply_stage.is_none(),
                "heating must not advance at 800°C ({i})"
            );
        }
        assert!((e.total_elapsed_s() - elapsed).abs() < 1e-9);

        // Reach the target: the heating stage completes and stage 1 (900 °C, 30 min) begins.
        assert_eq!(e.tick(900.0, 1.0).apply_stage, Some(1));
        e.confirm_stage_started(0.0);
        let mut held = 0.0;
        for dt in uneven.iter().cycle() {
            if held + dt >= 30.0 * 60.0 {
                break;
            }
            assert!(
                e.tick(900.0, *dt).apply_stage.is_none(),
                "hold released early at {held}s"
            );
            held += dt;
        }
        // One more tick crosses 30 min and releases the hold.
        assert_eq!(e.tick(900.0, 2.0).apply_stage, Some(2));
        assert!(
            e.total_elapsed_s() >= elapsed + 1.0 + 30.0 * 60.0,
            "total elapsed tracks the summed dt"
        );
    }

    #[test]
    fn fast_forward_runs_13240_to_completion() {
        // Simulate a full FREE_SWELLING run: temperatures snap to each stage's target,
        // and we tick with dt large enough to satisfy holds.
        let mut e = ExperimentEngine::start(standard_program("FREE_SWELLING").unwrap());
        e.confirm_stage_started(0.0);
        let mut applied = vec![0usize]; // stage 0 applied at start
        let mut guard = 0;
        loop {
            guard += 1;
            assert!(guard < 100, "should complete quickly");
            // Snap temperature to the stage target (cooling target+ε still ≤ target+5).
            let stage = e.current_stage().unwrap();
            let r = if stage.kind == StageKind::Manual {
                e.acknowledge_manual(e.stage_index()).unwrap()
            } else {
                e.tick(
                    stage.target_temp
                        - if stage.kind == StageKind::Cooling {
                            1.0
                        } else {
                            0.0
                        },
                    3600.0,
                )
            };
            if let Some(idx) = r.apply_stage {
                applied.push(idx);
                e.confirm_stage_started(0.0);
            }
            if r.completed {
                break;
            }
        }
        assert_eq!(applied, vec![0, 1, 2, 3, 4, 5]);
        assert_eq!(e.phase(), Phase::Completing);
    }

    #[test]
    fn custom_program_validation_catches_bad_shapes() {
        let mut p = standard_program("REDUCIBILITY").unwrap();
        p.id = "custom-1".into();
        p.name = "自定义".into();
        p.source = ProgramSource::Custom;
        validate_program(&p).unwrap();

        p.stages[0].gas = [0.0, 0.0, 0.0, 0.0];
        assert!(validate_program(&p).is_err());
        p.stages[0].gas = [0.0, 5.0, 0.0, 0.0];
        p.stages[0].duration_min = -1.0;
        assert!(validate_program(&p).is_err());
    }

    #[test]
    fn older_program_json_uses_metadata_defaults() {
        let json = r#"{
            "id":"custom-legacy",
            "name":"legacy",
            "stages":[{
                "name":"heat",
                "kind":"HEATING",
                "gas":[0.0,5.0,0.0,0.0],
                "target_temp":900.0,
                "tolerance":5.0,
                "duration_min":0.0,
                "ramp_rate":10.0,
                "description":""
            }]
        }"#;
        let p: Program = serde_json::from_str(json).unwrap();
        assert_eq!(p.description, "");
        assert_eq!(p.category, "custom");
        assert!(p.enabled);
        assert_eq!(p.sample_prefix, "");
        assert_eq!(p.source, ProgramSource::Custom);
    }
    #[test]
    fn gas_acknowledgement_delay_is_not_reduction_exposure() {
        let mut e = ExperimentEngine::start(standard_program("REDUCIBILITY").unwrap());
        e.confirm_stage_started(0.0);
        e.skip_stage();
        e.confirm_stage_started(0.0);
        e.skip_stage();
        e.confirm_stage_started(17.0);
        assert_eq!(e.total_elapsed_s(), 17.0);
        assert_eq!(e.stage_elapsed_s(), 0.0);
        assert!(!e.tick(900.0, 10800.0 - 17.0).completed);
        assert_eq!(e.stage_index(), 2);
        assert_eq!(e.tick(900.0, 17.0).apply_stage, Some(3));
    }
}
