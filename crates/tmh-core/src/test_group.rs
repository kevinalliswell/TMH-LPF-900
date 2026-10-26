//! Frozen evidence for a repeat-test group. Callers load authoritative rows and
//! authorize access before invoking this pure validator; no client result is accepted.
use crate::gb::CALCULATION_VERSION;
use crate::program::{validate_program, Program, ProgramSource};
use crate::repeat_group::{
    evaluate, GroupEvaluation, GroupPolicy, LimitSpec, MaterialKind, TrialIndices,
};
use crate::storage::ExperimentRow;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;

#[cfg_attr(feature = "typescript-export", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrialSelection {
    pub experiment_id: String,
    pub exclusion_reason: Option<String>,
}
#[cfg_attr(feature = "typescript-export", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GroupReportDetails {
    pub lab_name: String,
    pub lab_address: String,
    pub report_number: String,
    pub report_date: String,
    pub reviewer: String,
    pub deviations: String,
}
#[cfg_attr(feature = "typescript-export", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupRequest {
    pub group_id: Option<String>,
    pub expected_revision: Option<u64>,
    pub name: String,
    pub batch_id: String,
    pub same_batch_confirmed: bool,
    #[serde(default)]
    pub report_details: GroupReportDetails,
    pub material: MaterialKind,
    pub manual_limits: Option<LimitSpec>,
    pub trials: Vec<TrialSelection>,
    pub reason: String,
}
#[cfg_attr(feature = "typescript-export", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrozenTrial {
    #[serde(default)]
    #[cfg_attr(feature = "typescript-export", ts(optional = nullable))]
    pub process_assessment: Option<crate::process_evidence::ProcessAssessment>,
    pub experiment: ExperimentRow,
    #[cfg_attr(feature = "typescript-export", ts(type = "Record<string, unknown>"))]
    pub runtime: Value,
    pub analysis_revision_id: Option<i64>,
    pub exclusion_reason: Option<String>,
}
#[cfg_attr(feature = "typescript-export", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupSnapshot {
    pub schema_version: u32,
    pub name: String,
    pub batch_id: String,
    pub same_batch_confirmed: bool,
    pub policy: GroupPolicy,
    pub report_details: GroupReportDetails,
    /// Map evaluation decision positions to all frozen trials, including exclusions.
    pub valid_trial_indices: Vec<usize>,
    pub trials: Vec<FrozenTrial>,
    pub evaluation: GroupEvaluation,
    pub data_source: String,
    pub method_conformity: String,
    pub recorded_by: String,
    pub reason: String,
}

pub fn validate_request(request: &GroupRequest) -> Result<(), String> {
    if !request.same_batch_confirmed {
        return Err("请确认所列记录来自同一批试样的独立试验".into());
    }
    if [&request.name, &request.batch_id, &request.reason]
        .iter()
        .any(|s| s.trim().is_empty() || s.len() > 4096)
    {
        return Err("检验组名称、批次和本次保存原因必填，且每项不超过 4096 字节".into());
    }
    let details = serde_json::to_value(&request.report_details).map_err(|e| e.to_string())?;
    if details
        .as_object()
        .ok_or("报告资料无效")?
        .values()
        .any(|v| v.as_str().is_none_or(|s| s.len() > 4096))
    {
        return Err("组报告资料每项不能超过 4096 字节".into());
    }
    if request.trials.is_empty() || request.trials.len() > 20 {
        return Err("检验组需列出 1—20 次试验，其中有效试验最多 4 次".into());
    }
    let mut ids = HashSet::new();
    for selection in &request.trials {
        if selection.experiment_id.is_empty()
            || selection.experiment_id.len() > 128
            || !ids.insert(&selection.experiment_id)
        {
            return Err("试验编号无效或重复".into());
        }
        if selection
            .exclusion_reason
            .as_ref()
            .is_some_and(|s| s.trim().is_empty() || s.len() > 4096)
        {
            return Err("人工排除须填写具体原因，不能留空".into());
        }
    }
    Ok(())
}

fn program_of(trial: &FrozenTrial) -> Result<Program, String> {
    let program: Program = serde_json::from_str(
        trial
            .experiment
            .program_snapshot_json
            .as_deref()
            .ok_or("缺少执行程序快照")?,
    )
    .map_err(|_| "执行程序快照无效")?;
    validate_program(&program)?;
    Ok(program)
}
fn conditions(program: &Program) -> Value {
    let stages: Vec<_> = program
        .stages
        .iter()
        .map(|s| {
            json!({
                "kind":s.kind,"target_temp":s.target_temp,"tolerance":s.tolerance,
                "duration_min":s.duration_min,"ramp_rate":s.ramp_rate,"gas":s.gas
            })
        })
        .collect();
    json!({"base":program.base_standard_id,"year":program.base_standard_version,
        "template":program.template_revision,"source":program.source,"stages":stages,
        "parent":program.provenance.as_ref().map(|p|&p.parent_template)})
}
fn analysis_of(trial: &FrozenTrial, policy: &GroupPolicy) -> Result<TrialIndices, String> {
    let row = &trial.experiment;
    if row.end_time.is_none()
        || trial.runtime["outcome"] != "completed"
        || trial.runtime["data_complete"] != true
        || trial.runtime["ending_action"] != "normal"
    {
        return Err(format!(
            "{} 未正常完成或数据不完整，请先复核；无效试验可带原因排除",
            row.experiment_id
        ));
    }
    if trial.runtime["skipped"] == true
        || trial.runtime["stage_actions"]
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v["kind"] == "skip"))
    {
        return Err(format!(
            "{} 存在跳段，不能作为有效重复试验",
            row.experiment_id
        ));
    }
    if trial.analysis_revision_id.is_none_or(|v| v <= 0) {
        return Err("缺少可追溯的单次分析版本，请重新分析后再组检".into());
    }
    let analysis: Value = serde_json::from_str(
        row.analysis_results_json
            .as_deref()
            .ok_or("请先完成单次结果分析")?,
    )
    .map_err(|_| "单次结果无法读取")?;
    let (standard, year, key) = match policy.method.as_str() {
        "REDUCIBILITY" => (
            "GB/T 13241",
            "2017",
            if policy.custom { "rt" } else { "ri" },
        ),
        "LOW_TEMP_DEGRADATION" => ("GB/T 13242", "2017", "rdi_plus_3_15"),
        "FREE_SWELLING" => ("GB/T 13240", "2018", "swelling_index"),
        _ => return Err("未知基础标准".into()),
    };
    if analysis["standard"] != standard
        || analysis["standard_version"] != year
        || analysis["calculation_version"] != CALCULATION_VERSION
    {
        return Err("单次分析方法或计算版本不匹配，请先重新分析".into());
    }
    if policy.method == "LOW_TEMP_DEGRADATION" && analysis["external_conditions_met"] != true {
        return Err("转鼓/筛分条件未确认符合方法要求，不能作为有效重复试验".into());
    }
    let result = TrialIndices {
        experiment_id: row.experiment_id.clone(),
        primary: analysis[key].as_f64().ok_or("单次主指标缺失")?,
        rvi: analysis["reduction_rate_index"].as_f64(),
        rdi_plus_6_3: analysis["rdi_plus_6_3"].as_f64(),
        rdi_minus_0_5: analysis["rdi_minus_0_5"].as_f64(),
    };
    Ok(result)
}

fn started_at(trial: &FrozenTrial) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    // A stored absolute timestamp is authoritative. Never reinterpret a naive
    // historical wall-clock timestamp using the computer's current timezone.
    let value = match trial.runtime.get("started_at_utc") {
        Some(value) => value.as_str()?,
        None => &trial.experiment.start_time,
    };
    chrono::DateTime::parse_from_rfc3339(value).ok()
}

pub fn build_snapshot(
    request: &GroupRequest,
    previous: Option<&GroupSnapshot>,
    mut trials: Vec<FrozenTrial>,
    operator: &str,
) -> Result<GroupSnapshot, String> {
    validate_request(request)?;
    if trials.len() != request.trials.len() {
        return Err("检验组证据未完整读取".into());
    }
    let requested: HashSet<_> = request
        .trials
        .iter()
        .map(|s| s.experiment_id.as_str())
        .collect();
    let loaded: HashSet<_> = trials
        .iter()
        .map(|t| t.experiment.experiment_id.as_str())
        .collect();
    if requested != loaded || loaded.len() != trials.len() {
        return Err("检验组证据编号不匹配".into());
    }
    if let Some(previous) = previous {
        if previous.name != request.name.trim()
            || previous.batch_id != request.batch_id.trim()
            || previous.policy.material != request.material
            || previous.policy.manual_limits != request.manual_limits
        {
            return Err("已有检验组的批次、名称和限值保持固定；不同检验依据请另建组".into());
        }
        if previous
            .trials
            .iter()
            .any(|t| !requested.contains(t.experiment.experiment_id.as_str()))
        {
            return Err("旧成员须保留；无效试验请说明排除原因，不能移除原记录".into());
        }
    }
    for trial in &mut trials {
        // Freeze is a domain invariant, even if a caller has reloaded a newer
        // single-trial analysis. Only the explicit exclusion choice may change.
        if let Some(frozen) = previous.and_then(|p| {
            p.trials
                .iter()
                .find(|old| old.experiment.experiment_id == trial.experiment.experiment_id)
        }) {
            *trial = frozen.clone();
        }
        trial.exclusion_reason = request
            .trials
            .iter()
            .find(|s| s.experiment_id == trial.experiment.experiment_id)
            .and_then(|s| s.exclusion_reason.as_ref().map(|v| v.trim().to_string()));
    }
    if trials
        .iter()
        .any(|t| t.exclusion_reason.is_none() && started_at(t).is_none())
    {
        return Err(
            "有效试验缺少含时区的开始时间，无法确认重复试验顺序；旧记录可说明原因后排除".into(),
        );
    }
    // Absolute start time determines the repeat-test branch. Excluded legacy
    // evidence without a known time stays visible at the end, without a vote.
    trials.sort_by(|a, b| match (started_at(a), started_at(b)) {
        (Some(a_time), Some(b_time)) => a_time
            .cmp(&b_time)
            .then(a.experiment.experiment_id.cmp(&b.experiment.experiment_id)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.experiment.experiment_id.cmp(&b.experiment.experiment_id),
    });
    let valid: Vec<_> = trials
        .iter()
        .filter(|t| t.exclusion_reason.is_none())
        .collect();
    let first = valid.first().ok_or("至少需要一项有效试验")?;
    let first_program = program_of(first)?;
    let method = first_program
        .base_standard_id
        .clone()
        .ok_or("基础标准未记录")?;
    let expected_year = if method == "FREE_SWELLING" {
        "2018"
    } else {
        "2017"
    };
    if first_program.base_standard_version.as_deref() != Some(expected_year)
        || first_program.template_revision.is_none()
    {
        return Err("执行程序缺少已核对的标准年份或模板版本".into());
    }
    let source = first.runtime["device_mode"]
        .as_str()
        .ok_or("设备数据来源未记录")?;
    if !["mock", "browser-mock", "real", "replay"].contains(&source) {
        return Err("设备数据来源未知".into());
    }
    let policy = GroupPolicy {
        method,
        custom: first_program.source == ProgramSource::Custom,
        material: request.material.clone(),
        manual_limits: request.manual_limits.clone(),
    };
    if let Some(previous) = previous {
        if previous.policy != policy || previous.data_source != source {
            return Err("不能改变已保存检验组的方法或数据来源".into());
        }
    }
    let baseline = conditions(&first_program);
    if let Some(previous) = previous {
        let prior = previous
            .trials
            .iter()
            .find(|t| t.exclusion_reason.is_none())
            .ok_or("旧检验组缺少有效基准")?;
        if conditions(&program_of(prior)?) != baseline {
            return Err("不能改变已保存检验组的温度/气氛制度".into());
        }
    }
    let mut indices = Vec::new();
    for trial in &valid {
        if conditions(&program_of(trial)?) != baseline || trial.runtime["device_mode"] != source {
            return Err("有效试验的基础标准、温度/气氛制度或数据来源不一致，不能混组".into());
        }
        indices.push(analysis_of(trial, &policy)?);
    }
    let evaluation = evaluate(&policy, &indices)?;
    let valid_trial_indices = trials
        .iter()
        .enumerate()
        .filter_map(|(i, t)| t.exclusion_reason.is_none().then_some(i))
        .collect();
    let data_source = source.to_string();
    let method_conformity = if policy.custom {
        "custom_conditions"
    } else {
        "pending_process_verification"
    }
    .into();
    Ok(GroupSnapshot {
        schema_version: 1,
        name: request.name.trim().into(),
        batch_id: request.batch_id.trim().into(),
        same_batch_confirmed: true,
        policy,
        report_details: request.report_details.clone(),
        valid_trial_indices,
        trials,
        evaluation,
        data_source,
        method_conformity,
        recorded_by: operator.into(),
        reason: request.reason.trim().into(),
    })
}
