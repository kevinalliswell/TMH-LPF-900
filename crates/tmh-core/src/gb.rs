//! Calculations verified against the user-supplied final GB/T 13240-2018,
//! GB/T 13241-2017 and GB/T 13242-2017. See docs/13 for clause/page references.
use crate::rounding::range_within;
use serde::{Deserialize, Serialize};

pub const CALCULATION_VERSION: &str = "GB-2026-09-05.1";

pub const REDUCTION_SAMPLE_EVIDENCE_VERSION: &str = "REDUCTION-SAMPLES-2026-09-13.1";

/// Frozen facts about stored rows presented to the automatic reduction curve.
/// These counts do not certify a method or count the points determining a result.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReductionSampleEvidence {
    pub version: String,
    pub scope: String,
    pub total_sample_count: usize,
    pub reduction_sample_count: usize,
    pub valid_sample_count: usize,
    pub excluded_offline_rows: usize,
    pub excluded_other_quality_rows: usize,
    pub excluded_time_rows: usize,
    pub excluded_value_rows: usize,
    pub non_reduction_rows: usize,
    /// Subset of valid_sample_count; the baseline is excluded only from gas checks.
    pub valid_baseline_count: usize,
}

impl ReductionSampleEvidence {
    pub fn new(total_sample_count: usize) -> Self {
        Self {
            version: REDUCTION_SAMPLE_EVIDENCE_VERSION.into(),
            scope: "stored_reduction_rows".into(),
            total_sample_count,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Reducibility {
    pub rt: f64,
    pub delta_w: f64,
}

/// m0 is dry sample mass; m1/mt both include the same reduction tube.
/// W1 (ferrous content) and W2 (total iron) are mass percentages, e.g. 10 and 60.
pub fn reducibility(m0: f64, m1: f64, mt: f64, w1: f64, w2: f64) -> Option<Reducibility> {
    if ![m0, m1, mt, w1, w2].iter().all(|v| v.is_finite())
        || m0 <= 0.0
        || m1 < m0
        || mt <= 0.0
        || !(0.0..=100.0).contains(&w1)
        || w2 <= 0.0
        || w2 > 100.0
    {
        return None;
    }
    let delta_w = m1 - mt;
    let rt = (0.111 * w1 / (0.430 * w2) + delta_w / (m0 * 0.430 * w2) * 100.0) * 100.0;
    rt.is_finite().then_some(Reducibility { rt, delta_w })
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReductionRate {
    pub index: f64,
    pub t30: f64,
    pub endpoint_percent: f64,
    pub endpoint_min: f64,
    pub factor: f64,
}

/// First upward crossing by interpolation between adjacent valid points. Caller must
/// preserve missing points (NaN), preventing interpolation across a measurement gap.
fn crossing(points: &[(f64, f64)], threshold: f64) -> Option<f64> {
    for (i, &(t, y)) in points.iter().enumerate() {
        if !t.is_finite() || !y.is_finite() {
            continue;
        }
        if y == threshold {
            return Some(t);
        }
        if i > 0 {
            let (a, b) = points[i - 1];
            if a.is_finite() && b.is_finite() && a < t && b < threshold && y > threshold {
                return Some(a + (threshold - b) / (y - b) * (t - a));
            }
        }
    }
    None
}
pub fn reduction_rate(points: &[(f64, f64)]) -> Option<ReductionRate> {
    if points.iter().any(|(t, _)| !t.is_finite() || *t < 0.0)
        || points.windows(2).any(|w| w[0].0 >= w[1].0)
    {
        return None;
    }
    let t30 = crossing(points, 30.0)?;
    for (endpoint_percent, factor) in [(60.0, 33.6), (55.0, 26.5), (50.0, 20.0)] {
        if let Some(endpoint_min) = crossing(points, endpoint_percent) {
            if endpoint_min > t30 {
                return Some(ReductionRate {
                    index: factor / (endpoint_min - t30),
                    t30,
                    endpoint_percent,
                    endpoint_min,
                    factor,
                });
            }
        }
    }
    None
}
pub fn reduction_rate_index(points: &[(f64, f64)]) -> Option<f64> {
    reduction_rate(points).map(|r| r.index)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Rdi {
    pub rdi_plus_6_3: f64,
    pub rdi_plus_3_15: f64,
    pub rdi_minus_0_5: f64,
}
/// Denominator is the reduced test portion BEFORE tumbling. Three masses are separate
/// retained fractions; fines include unrecovered powder by subtraction (§9.1).
pub fn rdi(m0: f64, m1: f64, m2: f64, m3: f64) -> Option<Rdi> {
    if ![m0, m1, m2, m3].iter().all(|v| v.is_finite() && *v >= 0.0)
        || m0 <= 0.0
        || m1 + m2 + m3 > m0
    {
        return None;
    }
    Some(Rdi {
        rdi_plus_6_3: m1 / m0 * 100.0,
        rdi_plus_3_15: (m1 + m2) / m0 * 100.0,
        rdi_minus_0_5: (m0 - m1 - m2 - m3) / m0 * 100.0,
    })
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FreeSwelling {
    pub initial_volume: f64,
    pub final_volume: f64,
    pub swelling_index: f64,
}
/// Sum volumes of the SAME test portion; never average per-pellet percentages.
/// UI records the complete 18-pellet portion as one before/after pair.
pub fn free_swelling(volumes: &[(f64, f64)]) -> Option<FreeSwelling> {
    if volumes.is_empty()
        || !volumes
            .iter()
            .all(|(a, b)| a.is_finite() && b.is_finite() && *a > 0.0 && *b > 0.0)
    {
        return None;
    }
    let initial_volume: f64 = volumes.iter().map(|p| p.0).sum();
    let final_volume: f64 = volumes.iter().map(|p| p.1).sum();
    let swelling_index = (final_volume - initial_volume) / initial_volume * 100.0;
    swelling_index.is_finite().then_some(FreeSwelling {
        initial_volume,
        final_volume,
        swelling_index,
    })
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReductionInput {
    pub w1: f64,
    pub w2: f64,
    #[serde(default)]
    pub manual_m1: Option<f64>,
    #[serde(default)]
    pub manual_m180: Option<f64>,
    /// Total mass at the endpoint stored in the executed program snapshot.
    #[serde(default)]
    pub manual_mt: Option<f64>,
    #[serde(default)]
    pub manual_rate_points: Option<Vec<(f64, f64)>>,
    #[serde(default)]
    pub source_record: Option<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RdiInput {
    pub m0: f64,
    pub m1: f64,
    pub m2: f64,
    pub m3: f64,
    pub drum_rpm: f64,
    pub drum_revolutions: u32,
    pub sieve_seconds: f64,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SwellingInput {
    pub v0: f64,
    pub v1: f64,
    pub pellet_count: u32,
    pub volume_method: String,
    pub same_method: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RepeatDecision {
    pub required_count: usize,
    pub selected_indices: Vec<usize>,
    pub result: Option<f64>,
}
fn repeat_input(values: &[f64], a: f64, b: f64, c: f64) -> Result<(), String> {
    if values.len() > 4
        || values.iter().any(|v| !v.is_finite())
        || ![a, b, c].iter().all(|v| v.is_finite() && *v >= 0.0)
        || a > b
        || b > c
    {
        return Err("重复试验输入或阈值无效".into());
    }
    Ok(())
}
fn decision(values: &[f64], required_count: usize, middle: bool) -> Result<RepeatDecision, String> {
    if values.len() < required_count {
        return Ok(RepeatDecision {
            required_count,
            selected_indices: vec![],
            result: None,
        });
    }
    if values.len() > required_count {
        return Err("已有有效重复性决策，请勿混入额外试验".into());
    }
    let mut selected_indices: Vec<usize> = (0..values.len()).collect();
    if middle {
        selected_indices.sort_by(|a, b| values[*a].total_cmp(&values[*b]));
        selected_indices = selected_indices[1..3].to_vec();
    }
    let result = Some(
        selected_indices
            .iter()
            .map(|i| values[*i] / selected_indices.len() as f64)
            .sum(),
    );
    Ok(RepeatDecision {
        required_count,
        selected_indices,
        result,
    })
}
pub fn repeat_decision(values: &[f64], a: f64, b: f64, c: f64) -> Result<RepeatDecision, String> {
    repeat_input(values, a, b, c)?;
    if values.len() < 2 || range_within(&values[..2], a) {
        return decision(values, 2, false);
    }
    if range_within(&values[..2], b) && (values.len() < 3 || range_within(&values[..3], b)) {
        return decision(values, 3, false);
    }
    decision(
        values,
        4,
        !range_within(&values[..2], c) || (values.len() == 4 && !range_within(values, c)),
    )
}
pub fn sequential_repeat_decision(
    values: &[f64],
    a: f64,
    b: f64,
    c: f64,
) -> Result<RepeatDecision, String> {
    repeat_input(values, a, b, c)?;
    if values.len() < 2 || range_within(&values[..2], a) {
        return decision(values, 2, false);
    }
    if values.len() < 3 || range_within(&values[..3], b) {
        return decision(values, 3, false);
    }
    decision(values, 4, values.len() == 4 && !range_within(values, c))
}
