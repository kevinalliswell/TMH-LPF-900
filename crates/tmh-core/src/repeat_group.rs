//! Repeat-result selection and reporting. Identity, saved evidence and access checks
//! belong to the application boundary; this module never declares method conformity.
use crate::gb::{repeat_decision, sequential_repeat_decision};
use crate::rounding::{rounded_mean, ROUNDING_VERSION};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

pub const GROUP_CALCULATION_VERSION: &str = "GB-GROUP-2026-09-08.1";

#[cfg_attr(feature = "typescript-export", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterialKind {
    Sinter,
    Pellet,
    Natural,
}

#[cfg_attr(feature = "typescript-export", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LimitSpec {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub basis: String,
}

#[cfg_attr(feature = "typescript-export", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupPolicy {
    pub method: String,
    pub custom: bool,
    pub material: MaterialKind,
    pub manual_limits: Option<LimitSpec>,
}

#[cfg_attr(feature = "typescript-export", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrialIndices {
    pub experiment_id: String,
    pub primary: f64,
    pub rvi: Option<f64>,
    pub rdi_plus_6_3: Option<f64>,
    pub rdi_minus_0_5: Option<f64>,
}

#[cfg_attr(feature = "typescript-export", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupDecision {
    pub required_count: usize,
    pub selected_indices: Vec<usize>,
}

#[cfg_attr(feature = "typescript-export", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupEvaluation {
    pub calculation_version: String,
    pub rounding_version: String,
    pub limits: LimitSpec,
    pub decision_values: Vec<f64>,
    pub decision: GroupDecision,
    /// Fixed-scale final strings; null means a required reference result is missing.
    pub reported: BTreeMap<String, Option<String>>,
    pub explanation: String,
}

fn limits(policy: &GroupPolicy) -> Result<LimitSpec, String> {
    let built_in = match (policy.method.as_str(), &policy.material) {
        ("REDUCIBILITY", MaterialKind::Sinter) => {
            Some((5.0, 6.0, 6.6, "GB/T 13241-2017 附录 C，烧结矿"))
        }
        ("REDUCIBILITY", MaterialKind::Pellet) => {
            Some((3.0, 3.6, 3.9, "GB/T 13241-2017 附录 C，球团矿"))
        }
        ("FREE_SWELLING", MaterialKind::Pellet) => {
            Some((3.0, 3.6, 3.9, "GB/T 13240-2018 §10.2 / 附录 A"))
        }
        ("REDUCIBILITY", MaterialKind::Natural) | ("LOW_TEMP_DEGRADATION", _) => None,
        _ => return Err("方法或材料类型不匹配".into()),
    };
    let result = if let Some((a, b, c, basis)) = built_in {
        if policy.manual_limits.is_some() {
            return Err("此材料采用标准固定重复性限值，不接受覆盖".into());
        }
        LimitSpec {
            a,
            b,
            c,
            basis: basis.into(),
        }
    } else {
        policy
            .manual_limits
            .clone()
            .ok_or("请录入本次检验的重复性限值及实验室/供需双方依据")?
    };
    if result.basis.trim().is_empty()
        || result.basis.len() > 4096
        || ![result.a, result.b, result.c]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.0)
        || result.a > result.b
        || result.b > result.c
    {
        return Err("限值须为有限非负数且 A≤B≤C，并填写来源依据".into());
    }
    Ok(result)
}

fn single_rdi(value: f64) -> Result<f64, String> {
    rounded_mean(&[value], 1)?
        .parse()
        .map_err(|_| "单次 RDI 修约失败".into())
}

pub fn evaluate(policy: &GroupPolicy, trials: &[TrialIndices]) -> Result<GroupEvaluation, String> {
    let limits = limits(policy)?;
    let is_rdi = policy.method == "LOW_TEMP_DEGRADATION";
    let is_ri = policy.method == "REDUCIBILITY";
    let mut seen = HashSet::new();
    if trials.is_empty() || trials.len() > 4 {
        return Err("检验组需要 1—4 次独立有效试验".into());
    }
    for trial in trials {
        if trial.experiment_id.trim().is_empty()
            || !seen.insert(&trial.experiment_id)
            || !trial.primary.is_finite()
            || trial.rvi.is_some_and(|v| !v.is_finite() || v <= 0.0)
            || (is_rdi
                && (![Some(trial.primary), trial.rdi_plus_6_3, trial.rdi_minus_0_5]
                    .iter()
                    .all(|v| v.is_some_and(|v| v.is_finite() && (0.0..=100.0).contains(&v)))))
        {
            return Err("试验编号重复、指标无效或缺少 RDI 参考指标".into());
        }
    }
    let values = trials
        .iter()
        .map(|t| {
            if is_rdi {
                single_rdi(t.primary)
            } else {
                Ok(t.primary)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    let decision = if policy.method == "FREE_SWELLING" {
        sequential_repeat_decision(&values, limits.a, limits.b, limits.c)?
    } else {
        repeat_decision(&values, limits.a, limits.b, limits.c)?
    };
    let selected: Vec<_> = decision
        .selected_indices
        .iter()
        .map(|i| &trials[*i])
        .collect();
    let mut reported = BTreeMap::new();
    let explanation = if selected.is_empty() {
        format!(
            "已有 {} 次有效试验，按重复性规则需补足 {} 次",
            trials.len(),
            decision.required_count
        )
    } else {
        let primary = if is_ri {
            if policy.custom {
                "Rt"
            } else {
                "RI"
            }
        } else if is_rdi {
            "RDI+3.15"
        } else {
            "VFS"
        };
        let chosen: Vec<_> = decision
            .selected_indices
            .iter()
            .map(|i| values[*i])
            .collect();
        reported.insert(primary.into(), Some(rounded_mean(&chosen, 1)?));
        if is_ri {
            let rates: Option<Vec<_>> = selected.iter().map(|t| t.rvi).collect();
            reported.insert(
                "RVI".into(),
                rates.map(|v| rounded_mean(&v, 2)).transpose()?,
            );
        }
        if is_rdi {
            for (key, values) in [
                (
                    "RDI+6.3",
                    selected
                        .iter()
                        .map(|t| t.rdi_plus_6_3)
                        .collect::<Option<Vec<_>>>(),
                ),
                (
                    "RDI−0.5",
                    selected
                        .iter()
                        .map(|t| t.rdi_minus_0_5)
                        .collect::<Option<Vec<_>>>(),
                ),
            ] {
                let singles = values
                    .ok_or("缺少 RDI 参考指标")?
                    .into_iter()
                    .map(single_rdi)
                    .collect::<Result<Vec<_>, _>>()?;
                reported.insert(key.into(), Some(rounded_mean(&singles, 1)?));
            }
        }
        if selected.len() < trials.len() {
            "按重复性规则剔除一个最大值和一个最小值，采用两个中间值；剔除记录仍保留".into()
        } else {
            format!("按重复性规则采用全部 {} 次有效试验", selected.len())
        }
    };
    Ok(GroupEvaluation {
        calculation_version: GROUP_CALCULATION_VERSION.into(),
        rounding_version: ROUNDING_VERSION.into(),
        limits,
        decision_values: values,
        decision: GroupDecision {
            required_count: decision.required_count,
            selected_indices: decision.selected_indices,
        },
        reported,
        explanation,
    })
}
