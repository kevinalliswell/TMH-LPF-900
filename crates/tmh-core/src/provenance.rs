//! Server-authored custom-program revisions and comparisons with their standard parent.
use crate::program::{standard_program, validate_program, Program, ProgramSource};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg_attr(feature = "typescript-export", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProgramChange {
    pub stage_index: usize,
    pub field: String,
    pub before: Value,
    pub after: Value,
}

#[cfg_attr(feature = "typescript-export", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProgramProvenance {
    pub revision: u64,
    pub parent_template: Box<Program>,
    pub edited_by_user_id: String,
    pub edited_by: String,
    pub edited_at: String,
    pub reason: String,
    pub changes: Vec<ProgramChange>,
}

pub struct ProgramEditor {
    pub user_id: String,
    pub display_name: String,
    pub edited_at: String,
}

/// `previous` comes from storage; `editor` comes from authentication. The request's
/// revision is only an optimistic concurrency token, never trusted audit evidence.
pub fn prepare_custom_revision(
    mut program: Program,
    previous: Option<&Program>,
    editor: ProgramEditor,
    reason: &str,
) -> Result<Program, String> {
    if reason.trim().is_empty() || reason.chars().count() > 4096 {
        return Err("请填写修改原因（1—4096 字）".into());
    }
    if !program.id.starts_with("custom-") {
        return Err("自定义程序编号无效".into());
    }
    let revision = match previous {
        Some(previous) => {
            if previous.id != program.id || previous.base_standard_id != program.base_standard_id {
                return Err("已保存的程序不能更换基础标准，请另建程序".into());
            }
            let expected = previous.provenance.as_ref().map(|p| p.revision);
            if program.provenance.as_ref().map(|p| p.revision) != expected {
                return Err("程序版本已变化，请重新读取后编辑".into());
            }
            expected
                .unwrap_or(0)
                .checked_add(1)
                .ok_or("程序版本超出范围")?
        }
        None => 1,
    };
    let parent = previous
        .and_then(|p| p.provenance.as_ref())
        .map(|p| p.parent_template.as_ref().clone())
        .or_else(|| {
            program
                .base_standard_id
                .as_deref()
                .and_then(standard_program)
        })
        .ok_or("须选择已核实的基础国标")?;
    validate_parent_and_shape(&program, &parent)?;
    program.source = ProgramSource::Custom;
    validate_program(&program)?;
    let changes = condition_changes(&program, &parent)?;
    program.provenance = Some(ProgramProvenance {
        revision,
        parent_template: Box::new(parent),
        edited_by_user_id: editor.user_id,
        edited_by: editor.display_name,
        edited_at: editor.edited_at,
        reason: reason.trim().into(),
        changes,
    });
    Ok(program)
}

fn validate_parent_and_shape(program: &Program, parent: &Program) -> Result<(), String> {
    if parent.source != ProgramSource::Standard
        || program.base_standard_id.as_deref() != Some(&parent.id)
        || program.base_standard_version != parent.base_standard_version
        || program.template_revision != parent.template_revision
        || parent.template_revision.is_none()
    {
        return Err("基础标准年份或模板版本不匹配，请从当前国标模板复制".into());
    }
    if program.stages.len() != parent.stages.len()
        || program.stages.len() < 3
        || program
            .stages
            .iter()
            .zip(&parent.stages)
            .any(|(a, b)| a.kind != b.kind)
    {
        return Err("首版自定义保留国标阶段顺序和类型；请调整阶段内温度、时长或气氛".into());
    }
    if program.stages[3..] != parent.stages[3..] {
        return Err("国标结束步骤不可更改或省略".into());
    }
    Ok(())
}

pub fn condition_changes(
    program: &Program,
    parent: &Program,
) -> Result<Vec<ProgramChange>, String> {
    if program.stages.len() != parent.stages.len() {
        return Err("程序阶段数量与基础模板不一致".into());
    }
    let mut changes = Vec::new();
    for (stage_index, (actual, base)) in program.stages.iter().zip(&parent.stages).enumerate() {
        let actual = serde_json::to_value(actual).map_err(|e| e.to_string())?;
        let base = serde_json::to_value(base).map_err(|e| e.to_string())?;
        for field in [
            "name",
            "description",
            "kind",
            "target_temp",
            "tolerance",
            "duration_min",
            "ramp_rate",
            "gas",
        ] {
            if actual[field] != base[field] {
                changes.push(ProgramChange {
                    stage_index,
                    field: field.into(),
                    before: base[field].clone(),
                    after: actual[field].clone(),
                });
            }
        }
    }
    Ok(changes)
}

/// Historical snapshots remain readable; only current, verified programs can start
/// a new run. Updating a saved revision never silently replaces its original parent.
pub fn validate_executable_program(program: &Program) -> Result<(), String> {
    validate_program(program)?;
    if program.source == ProgramSource::Standard {
        return if standard_program(&program.id).as_ref() == Some(program) {
            Ok(())
        } else {
            Err("标准程序与当前内置模板不一致，不能作为国标程序执行".into())
        };
    }
    let provenance = program
        .provenance
        .as_ref()
        .ok_or("旧自定义程序缺少版本依据，请从当前国标重新复制")?;
    let current = program
        .base_standard_id
        .as_deref()
        .and_then(standard_program)
        .ok_or("基础国标未知")?;
    if provenance.parent_template.as_ref() != &current {
        return Err("基础模板已有更新，请从当前国标另建程序；旧记录保留原版本".into());
    }
    validate_parent_and_shape(program, &current)?;
    if provenance.revision == 0 || provenance.changes != condition_changes(program, &current)? {
        return Err("自定义版本或条件差异记录无效，请重新保存".into());
    }
    Ok(())
}

pub fn validate_gas_limits(program: &Program, h2: f64, co: f64) -> Result<(), String> {
    if !h2.is_finite() || !co.is_finite() || h2 < 0.0 || co < 0.0 {
        return Err("气体安全上限配置无效".into());
    }
    let h2 = h2.min(crate::clamp::H2_ABS_MAX);
    let co = co.min(crate::clamp::CO_ABS_MAX);
    for (index, stage) in program.stages.iter().enumerate() {
        if stage.gas[0] > h2 || stage.gas[3] > co {
            return Err(format!(
                "第 {} 阶段 H₂/CO 超出安全上限，不能按此程序执行",
                index + 1
            ));
        }
    }
    Ok(())
}

/// Reject values that would be rounded by the configured MFC scaling. A saved
/// condition must agree with the setpoint acknowledged on the wire.
pub fn validate_flow_resolution(program: &Program, steps: [f64; 4]) -> Result<(), String> {
    if steps.iter().any(|step| !step.is_finite() || *step <= 0.0) {
        return Err("气体流量分辨率配置无效".into());
    }
    for (index, stage) in program.stages.iter().enumerate() {
        for (gas, (value, step)) in stage.gas.iter().zip(steps).enumerate() {
            let raw = value / step;
            if !raw.is_finite()
                || raw < 0.0
                || raw >= i64::MAX as f64
                || (raw - raw.round()).abs() > 1e-8
            {
                return Err(format!(
                    "第 {} 阶段 {} 流量须为 {} L/min 的整数倍且在协议范围内",
                    index + 1,
                    ["H₂", "N₂", "CO₂", "CO"][gas],
                    step
                ));
            }
        }
    }
    Ok(())
}
