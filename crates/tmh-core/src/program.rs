//! Versioned national-standard stage definitions, shared with the UI.
use serde::{Deserialize, Serialize};

/// Stage type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StageKind {
    Heating,
    Stabilizing,
    Reducing,
    Cooling,
    Purging,
    Manual,
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
    pub template_revision: Option<String>,
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

/// Shared by native execution and browser preview; one authoritative stage table.
pub fn standard_program(mode_id: &str) -> Option<Program> {
    let programs: Vec<Program> =
        serde_json::from_str(include_str!("../../../config/standard_programs.json"))
            .expect("bundled standard programs must be valid");
    programs.into_iter().find(|p| p.id == mode_id)
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
        if !total_flow.is_finite() || (total_flow <= 0.0 && stage.kind != StageKind::Manual) {
            return Err(format!("第 {n} 阶段总流量必须大于 0"));
        }
    }
    Ok(())
}
