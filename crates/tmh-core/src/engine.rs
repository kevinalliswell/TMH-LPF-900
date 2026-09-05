//! Experiment engine (docs/05 §5.1–5.4): phase state machine, the three GB standard
//! program tables, and stage-advancement rules. Pure logic — the orchestration layer
//! (src-tauri) ticks it with the current sample temperature (T8) and applies each
//! stage's atmosphere through the safety clamp. Sampling/DB/crash-recovery wire in
//! at the command layer.

use serde::{Deserialize, Serialize};

/// Protection atmosphere applied on EVERY experiment exit (docs/05 §5.5).
/// Order [H2, N2, CO2, CO] L/min.
pub const SAFETY_ATMOSPHERE: [f64; 4] = [0.0, 5.0, 0.0, 0.0];

/// Experiment phase (docs/05 §5.1).
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

/// Stage type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StageKind {
    Heating,
    Stabilizing,
    Reducing,
    Cooling,
}

/// One program stage (docs/05 §5.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramSource {
    Standard,
    Custom,
}

/// One program stage (docs/05 §5.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stage {
    pub name: String,
    pub kind: StageKind,
    /// Gas setpoints [H2, N2, CO2, CO] L/min.
    pub gas: [f64; 4],
    pub target_temp: f64,
    pub tolerance: f64,
    /// Hold duration (min); 0 = dynamic stage.
    pub duration_min: f64,
    pub ramp_rate: f64,
    pub description: String,
}

/// A full experiment program.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Program {
    #[serde(default)]
    pub base_standard_id: Option<String>,
    #[serde(default)]
    pub base_standard_version: Option<String>,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_program_category")]
    pub category: String,
    #[serde(default = "default_program_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub sample_prefix: String,
    #[serde(default = "default_program_source")]
    pub source: ProgramSource,
    pub stages: Vec<Stage>,
}

fn default_program_category() -> String {
    "custom".into()
}

fn default_program_enabled() -> bool {
    true
}

fn default_program_source() -> ProgramSource {
    ProgramSource::Custom
}

#[allow(clippy::too_many_arguments)]
fn stage(
    name: &str,
    kind: StageKind,
    gas: [f64; 4],
    target: f64,
    tol: f64,
    dur: f64,
    ramp: f64,
    desc: &str,
) -> Stage {
    Stage {
        name: name.into(),
        kind,
        gas,
        target_temp: target,
        tolerance: tol,
        duration_min: dur,
        ramp_rate: ramp,
        description: desc.into(),
    }
}

/// The three GB standard programs (docs/05 §5.3), by mode id.
pub fn standard_program(mode_id: &str) -> Option<Program> {
    use StageKind::*;
    let p = match mode_id {
        "REDUCIBILITY" => Program {
            base_standard_id: Some("REDUCIBILITY".into()),
            base_standard_version: Some("2017".into()),
            id: "REDUCIBILITY".into(),
            name: "GB/T 13241 还原性".into(),
            description: "GB/T 13241-2017 铁矿石还原性的测定方法".into(),
            category: "standard".into(),
            enabled: true,
            sample_prefix: "RED".into(),
            source: ProgramSource::Standard,
            stages: vec![
                stage(
                    "升温",
                    Heating,
                    [0.0, 5.0, 0.0, 0.0],
                    900.0,
                    5.0,
                    0.0,
                    10.0,
                    "升温至900℃,N₂保护",
                ),
                stage(
                    "恒温",
                    Stabilizing,
                    [0.0, 15.0, 0.0, 0.0],
                    900.0,
                    5.0,
                    30.0,
                    0.0,
                    "900℃恒温30min,N₂气氛",
                ),
                stage(
                    "还原",
                    Reducing,
                    [0.0, 10.5, 0.0, 4.5],
                    900.0,
                    5.0,
                    180.0,
                    0.0,
                    "还原180min,30%CO+70%N₂",
                ),
                stage(
                    "冷却",
                    Cooling,
                    [0.0, 5.0, 0.0, 0.0],
                    25.0,
                    0.0,
                    0.0,
                    -10.0,
                    "N₂保护冷却至室温",
                ),
            ],
        },
        "LOW_TEMP_DEGRADATION" => Program {
            base_standard_id: Some("LOW_TEMP_DEGRADATION".into()),
            base_standard_version: Some("2017".into()),
            id: "LOW_TEMP_DEGRADATION".into(),
            name: "GB/T 13242 低温粉化".into(),
            description: "GB/T 13242-2017 铁矿石低温粉化试验方法".into(),
            category: "standard".into(),
            enabled: true,
            sample_prefix: "RDI".into(),
            source: ProgramSource::Standard,
            stages: vec![
                stage(
                    "升温",
                    Heating,
                    [0.0, 5.0, 0.0, 0.0],
                    500.0,
                    5.0,
                    0.0,
                    10.0,
                    "升温至500℃,N₂保护",
                ),
                stage(
                    "恒温",
                    Stabilizing,
                    [0.0, 15.0, 0.0, 0.0],
                    500.0,
                    5.0,
                    30.0,
                    0.0,
                    "500℃恒温30min,N₂气氛",
                ),
                stage(
                    "还原",
                    Reducing,
                    [0.0, 9.0, 3.0, 3.0],
                    500.0,
                    5.0,
                    60.0,
                    0.0,
                    "还原60min,20%CO+20%CO₂+60%N₂",
                ),
                stage(
                    "冷却",
                    Cooling,
                    [0.0, 5.0, 0.0, 0.0],
                    25.0,
                    0.0,
                    0.0,
                    -10.0,
                    "N₂保护冷却至室温",
                ),
            ],
        },
        "FREE_SWELLING" => Program {
            base_standard_id: Some("FREE_SWELLING".into()),
            base_standard_version: Some("2018".into()),
            id: "FREE_SWELLING".into(),
            name: "GB/T 13240 自由膨胀".into(),
            description: "GB/T 13240-2018 球团矿自由膨胀指数测定方法".into(),
            category: "standard".into(),
            enabled: true,
            sample_prefix: "FSI".into(),
            source: ProgramSource::Standard,
            stages: vec![
                stage(
                    "升温",
                    Heating,
                    [0.0, 10.0, 0.0, 0.0],
                    900.0,
                    5.0,
                    0.0,
                    10.0,
                    "升温至900℃,N₂保护",
                ),
                stage(
                    "恒温",
                    Stabilizing,
                    [0.0, 15.0, 0.0, 0.0],
                    900.0,
                    5.0,
                    15.0,
                    0.0,
                    "900℃恒温15min,N₂气氛",
                ),
                stage(
                    "还原",
                    Reducing,
                    [0.0, 10.5, 0.0, 4.5],
                    900.0,
                    5.0,
                    60.0,
                    0.0,
                    "还原60min,30%CO+70%N₂",
                ),
                stage(
                    "冷却",
                    Cooling,
                    [0.0, 5.0, 0.0, 0.0],
                    50.0,
                    0.0,
                    0.0,
                    -10.0,
                    "N₂保护冷却至50℃",
                ),
            ],
        },
        _ => return None,
    };
    Some(p)
}

/// All standard mode ids.
pub const STANDARD_MODES: [&str; 3] = ["REDUCIBILITY", "LOW_TEMP_DEGRADATION", "FREE_SWELLING"];

/// Validate an operator-defined program before saving or running it.
pub fn validate_program(program: &Program) -> Result<(), String> {
    if program.id.trim().is_empty() {
        return Err("模式 ID 必填".into());
    }
    if program.name.trim().is_empty() {
        return Err("模式名称必填".into());
    }
    if program.stages.is_empty() {
        return Err("至少需要一个阶段".into());
    }
    for (i, stage) in program.stages.iter().enumerate() {
        let n = i + 1;
        if stage.name.trim().is_empty() {
            return Err(format!("第 {n} 阶段名称必填"));
        }
        if !stage.target_temp.is_finite() {
            return Err(format!("第 {n} 阶段目标温度无效"));
        }
        if !stage.tolerance.is_finite() || stage.tolerance < 0.0 {
            return Err(format!("第 {n} 阶段容差必须为非负数"));
        }
        if !stage.duration_min.is_finite() || stage.duration_min < 0.0 {
            return Err(format!("第 {n} 阶段时长必须为非负数"));
        }
        if !stage.ramp_rate.is_finite() {
            return Err(format!("第 {n} 阶段升温率无效"));
        }
        let mut total_flow = 0.0;
        for (gas, value) in stage.gas.iter().enumerate() {
            if !value.is_finite() || *value < 0.0 {
                return Err(format!("第 {n} 阶段第 {} 路气体流量必须为非负数", gas + 1));
            }
            total_flow += value;
        }
        if total_flow <= 0.0 {
            return Err(format!("第 {n} 阶段总流量必须大于 0"));
        }
    }
    Ok(())
}

/// A running experiment. Tick it once per second with the current T8.
pub struct ExperimentEngine {
    program: Program,
    phase: Phase,
    stage_index: usize,
    stage_elapsed_s: f64,
    total_elapsed_s: f64,
}

/// Result of one engine tick.
#[derive(Debug, Clone, PartialEq)]
pub struct TickResult {
    /// The stage just changed (or the run just started): apply this stage's atmosphere.
    pub apply_stage: Option<usize>,
    /// The run completed on this tick (apply the safety atmosphere).
    pub completed: bool,
}

impl ExperimentEngine {
    /// Start a run. The caller should apply stage 0's atmosphere immediately.
    pub fn start(program: Program) -> Self {
        Self {
            program,
            phase: Phase::Running,
            stage_index: 0,
            stage_elapsed_s: 0.0,
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

    /// Advance `dt` seconds with sample temperature `t8`. Returns which stage's
    /// atmosphere to (re)apply and whether the run completed.
    pub fn tick(&mut self, t8: f64, dt: f64) -> TickResult {
        if self.phase != Phase::Running && self.phase != Phase::StageTransition {
            return TickResult {
                apply_stage: None,
                completed: false,
            };
        }
        self.phase = Phase::Running;
        self.stage_elapsed_s += dt;
        self.total_elapsed_s += dt;

        let reached = self
            .current_stage()
            .map(|s| stage_reached(s, t8, self.stage_elapsed_s))
            .unwrap_or(true);
        if !reached {
            return TickResult {
                apply_stage: None,
                completed: false,
            };
        }

        // Advance.
        self.stage_index += 1;
        if self.stage_index >= self.program.stages.len() {
            self.phase = Phase::Completing;
            return TickResult {
                apply_stage: None,
                completed: true,
            };
        }
        self.stage_elapsed_s = 0.0;
        self.phase = Phase::StageTransition;
        TickResult {
            apply_stage: Some(self.stage_index),
            completed: false,
        }
    }

    /// Manual skip to the next stage (operator, with confirm + audit at the UI/command
    /// layer). Returns the new stage to apply, or completion.
    pub fn skip_stage(&mut self) -> TickResult {
        self.stage_index += 1;
        if self.stage_index >= self.program.stages.len() {
            self.phase = Phase::Completing;
            return TickResult {
                apply_stage: None,
                completed: true,
            };
        }
        self.stage_elapsed_s = 0.0;
        self.phase = Phase::StageTransition;
        TickResult {
            apply_stage: Some(self.stage_index),
            completed: false,
        }
    }
}

/// Stage arrival test (docs/05 §5.4).
fn stage_reached(stage: &Stage, t8: f64, elapsed_s: f64) -> bool {
    let at_temp = (t8 - stage.target_temp).abs() <= stage.tolerance;
    if stage.duration_min > 0.0 {
        // hold stages: at target AND held long enough
        at_temp && elapsed_s >= stage.duration_min * 60.0
    } else if stage.kind == StageKind::Cooling {
        // cooling (new rule): T ≤ target + 5.0 (old tolerance=0 never triggered)
        t8 <= stage.target_temp + 5.0
    } else {
        // dynamic heating: at target
        at_temp
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
        assert_eq!(p.stages.len(), 4);
        assert_eq!(p.stages[0].gas, [0.0, 5.0, 0.0, 0.0]);
        assert_eq!(p.stages[0].target_temp, 900.0);
        assert_eq!(p.stages[1].duration_min, 30.0);
        assert_eq!(p.stages[2].gas, [0.0, 10.5, 0.0, 4.5]); // CO=4.5 N2=10.5
        assert_eq!(p.stages[2].duration_min, 180.0);
        assert_eq!(p.stages[3].kind, StageKind::Cooling);
        assert_eq!(p.stages[3].target_temp, 25.0);

        let d = standard_program("LOW_TEMP_DEGRADATION").unwrap();
        assert_eq!(d.sample_prefix, "RDI");
        assert_eq!(d.stages[0].target_temp, 500.0);
        assert_eq!(d.stages[2].gas, [0.0, 9.0, 3.0, 3.0]); // N2=9 CO2=3 CO=3
        assert_eq!(d.stages[2].duration_min, 60.0);

        let s = standard_program("FREE_SWELLING").unwrap();
        assert_eq!(s.sample_prefix, "FSI");
        assert_eq!(s.stages[0].gas, [0.0, 10.0, 0.0, 0.0]); // N2=10
        assert_eq!(s.stages[1].duration_min, 15.0);
        assert_eq!(s.stages[3].target_temp, 50.0);

        assert!(standard_program("NOPE").is_none());
    }

    #[test]
    fn legal_transitions() {
        assert!(is_legal_transition(Phase::Idle, Phase::Configuring));
        assert!(is_legal_transition(Phase::Running, Phase::Stopping));
        assert!(is_legal_transition(Phase::Completing, Phase::Idle));
        assert!(!is_legal_transition(Phase::Idle, Phase::Running));
        assert!(!is_legal_transition(Phase::Completing, Phase::Running));
    }

    #[test]
    fn cooling_reaches_at_target_plus_5() {
        let cool = &standard_program("REDUCIBILITY").unwrap().stages[3]; // target 25, cooling
        assert!(stage_reached(cool, 29.0, 0.0)); // 29 <= 25+5
        assert!(!stage_reached(cool, 31.0, 0.0)); // 31 > 30
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
        let mut applied = vec![0usize]; // stage 0 applied at start
        let mut guard = 0;
        loop {
            guard += 1;
            assert!(guard < 100, "should complete quickly");
            // Snap temperature to the stage target (cooling target+ε still ≤ target+5).
            let t8 = e.current_stage().unwrap().target_temp;
            let r = e.tick(t8, 3600.0); // 1h per tick — clears any hold
            if let Some(idx) = r.apply_stage {
                applied.push(idx);
            }
            if r.completed {
                break;
            }
        }
        assert_eq!(applied, vec![0, 1, 2, 3]); // all 4 stages entered in order
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
}
