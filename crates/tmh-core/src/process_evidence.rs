//! Stage-by-stage facts from recorded samples. This is not laboratory certification.
use crate::{
    program::{Program, StageKind},
    storage::SampleRow,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const VERSION: &str = "PROCESS-EVIDENCE-2026-09-08.1";
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageEvidence {
    pub index: usize,
    pub name: String,
    pub kind: StageKind,
    pub points: usize,
    pub valid_temperature_points: usize,
    pub valid_gas_points: usize,
    pub temperature_range: Option<[f64; 2]>,
    pub gas_pv_ranges: [Option<[f64; 2]>; 4],
    pub gas_sv_mismatches: usize,
    pub elapsed_s: Option<f64>,
    pub stable_elapsed_s: Option<f64>,
    pub maximum_observed_ramp_c_min: Option<f64>,
    pub maximum_sample_gap_s: Option<f64>,
    pub target_temp: f64,
    pub tolerance: f64,
    pub gas_targets: [f64; 4],
    pub required_seconds: Option<f64>,
    pub notes: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessAssessment {
    pub version: String,
    pub unassigned_points: usize,
    pub stages: Vec<StageEvidence>,
    pub method_conformity: String,
}
fn finite(value: &Value) -> Option<f64> {
    value.as_f64().filter(|v| v.is_finite())
}
fn time(value: &Value) -> Option<f64> {
    finite(value).filter(|v| *v >= 0.0)
}
fn include(range: &mut Option<[f64; 2]>, value: f64) {
    *range = Some(range.map_or([value, value], |r| [r[0].min(value), r[1].max(value)]));
}
fn maximum(current: &mut Option<f64>, value: Option<f64>) {
    if let Some(value) = value.filter(|v| v.is_finite()) {
        *current = Some(current.map_or(value, |old| old.max(value)));
    }
}
fn confirmed(runtime: &Value, index: usize) -> bool {
    runtime["stage_actions"].as_array().is_some_and(|events| {
        events.iter().any(|e| {
            e["kind"] == "confirm"
                && e["stage_index"].as_u64() == Some(index as u64)
                && ["operator", "notes"]
                    .iter()
                    .all(|key| e[key].as_str().is_some_and(|s| !s.trim().is_empty()))
        })
    })
}

pub fn assess(program: &Program, samples: &[SampleRow], runtime: &Value) -> ProcessAssessment {
    let mut grouped: Vec<Vec<Value>> = vec![Vec::new(); program.stages.len()];
    let mut unassigned_points = 0;
    for sample in samples {
        let value: Value = serde_json::from_str(sample.record_json.as_deref().unwrap_or("null"))
            .unwrap_or_default();
        if value["sample_role"] == "reduction_baseline" {
            continue;
        }
        match value["stage_index"]
            .as_u64()
            .and_then(|i| usize::try_from(i).ok())
            .and_then(|i| grouped.get_mut(i))
        {
            Some(points) => points.push(value),
            None => unassigned_points += 1,
        }
    }
    let steps: Option<Vec<f64>> = runtime["gas_safety_limits"]["flow_steps"]
        .as_array()
        .filter(|v| v.len() == 4)
        .and_then(|values| {
            values
                .iter()
                .map(|v| finite(v).filter(|s| *s > 0.0))
                .collect()
        });
    let stages = program
        .stages
        .iter()
        .enumerate()
        .map(|(index, stage)| {
            let points = &mut grouped[index];
            // Storage returns insertion order. Missing clocks must not move an
            // actual final invalid/hot sample ahead of an earlier cool sample.
            let mut summary = StageEvidence {
                index,
                name: stage.name.clone(),
                kind: stage.kind,
                points: points.len(),
                valid_temperature_points: 0,
                valid_gas_points: 0,
                temperature_range: None,
                gas_pv_ranges: [None; 4],
                gas_sv_mismatches: 0,
                elapsed_s: None,
                stable_elapsed_s: None,
                maximum_observed_ramp_c_min: None,
                maximum_sample_gap_s: None,
                target_temp: stage.target_temp,
                tolerance: stage.tolerance,
                gas_targets: stage.gas,
                required_seconds: (stage.duration_min * 60.0)
                    .is_finite()
                    .then_some(stage.duration_min * 60.0),
                notes: Vec::new(),
            };
            let mut outside = 0;
            let mut last_temperature = None;
            let mut prior_temperature: Option<(f64, f64)> = None;
            let mut prior_time = None;
            let mut missing_time = 0;
            for point in points.iter() {
                let elapsed = time(&point["stage_elapsed_s"]);
                if elapsed.is_none() {
                    missing_time += 1;
                }
                if let (Some(before), Some(now)) = (prior_time, elapsed) {
                    if now >= before {
                        maximum(&mut summary.maximum_sample_gap_s, Some(now - before));
                    } else {
                        summary
                            .notes
                            .push("阶段计时回退，不能据此确认连续过程".into());
                    }
                }
                prior_time = elapsed;
                maximum(&mut summary.elapsed_s, elapsed);
                maximum(
                    &mut summary.stable_elapsed_s,
                    time(&point["stable_elapsed_s"]),
                );
                let telemetry = &point["telemetry"];
                let temperature = if telemetry["health"]["temp_valid"] == true {
                    finite(&telemetry["temps"][7])
                } else {
                    None
                };
                last_temperature = temperature;
                if let Some(temperature) = temperature {
                    summary.valid_temperature_points += 1;
                    include(&mut summary.temperature_range, temperature);
                    if (temperature - stage.target_temp).abs() > stage.tolerance {
                        outside += 1;
                    }
                    if let (Some((before_t, before_v)), Some(now)) = (prior_temperature, elapsed) {
                        if now > before_t {
                            maximum(
                                &mut summary.maximum_observed_ramp_c_min,
                                Some(((temperature - before_v) / (now - before_t) * 60.0).max(0.0)),
                            );
                        }
                    }
                    prior_temperature = elapsed.map(|t| (t, temperature));
                } else {
                    prior_temperature = None;
                }
                let gas = telemetry["gas"].as_array().filter(|v| v.len() == 4);
                let good = telemetry["health"]["mfc_valid"] == true
                    && gas.is_some_and(|channels| {
                        channels.iter().all(|c| {
                            finite(&c["pv"]).is_some_and(|v| v >= 0.0)
                                && finite(&c["sv"]).is_some_and(|v| v >= 0.0)
                        })
                    });
                if good {
                    summary.valid_gas_points += 1;
                    let channels = gas.expect("four valid gas channels");
                    for (i, c) in channels.iter().enumerate() {
                        include(&mut summary.gas_pv_ranges[i], c["pv"].as_f64().unwrap());
                    }
                    if let Some(steps) = &steps {
                        if channels.iter().enumerate().any(|(i, c)| {
                            (c["sv"].as_f64().unwrap() / steps[i]).round()
                                != (stage.gas[i] / steps[i]).round()
                        }) {
                            summary.gas_sv_mismatches += 1;
                        }
                    }
                }
            }
            if summary.required_seconds.is_none() {
                summary
                    .notes
                    .push("时长无法核定：程序时长换算超出可表示范围".into());
            }
            if summary.points == 0 {
                summary.notes.push("缺少阶段采样，无法核查本段记录".into());
            }
            if missing_time > 0 {
                summary
                    .notes
                    .push(format!("{missing_time} 条记录缺少阶段计时"));
            }
            if summary.valid_temperature_points < summary.points {
                summary.notes.push(format!(
                    "{} 条温度记录缺失或无效",
                    summary.points - summary.valid_temperature_points
                ));
            }
            if summary.valid_gas_points < summary.points {
                summary.notes.push(format!(
                    "{} 条气体记录缺失或无效",
                    summary.points - summary.valid_gas_points
                ));
            }
            if steps.is_none() {
                summary
                    .notes
                    .push("缺少本次气体设定分辨率，未核定 SV 一致性".into());
            }
            if summary.gas_sv_mismatches > 0 {
                summary.notes.push(format!(
                    "{} 条记录的气体 SV 与程序设定不一致",
                    summary.gas_sv_mismatches
                ));
            }
            match stage.kind {
                StageKind::Stabilizing => {
                    if summary
                        .stable_elapsed_s
                        .zip(summary.required_seconds)
                        .map_or(true, |(s, required)| s < required)
                    {
                        summary
                            .notes
                            .push("连续稳定计时不足或缺失，不能用总停留时间替代".into());
                    }
                    if outside > 0 {
                        summary.notes.push(format!(
                            "{outside} 个采样点在温度设定范围外；需结合连续稳定计时复核"
                        ));
                    }
                }
                StageKind::Reducing | StageKind::Purging => {
                    if summary
                        .elapsed_s
                        .zip(summary.required_seconds)
                        .map_or(true, |(s, required)| s < required)
                    {
                        summary.notes.push("固定阶段计时不足或缺失".into());
                    }
                    if stage.kind == StageKind::Reducing && outside > 0 {
                        summary
                            .notes
                            .push(format!("{outside} 个还原采样点超出本次温度设定范围"));
                    }
                }
                StageKind::Cooling => {
                    if last_temperature.map_or(true, |t| t >= stage.target_temp) {
                        summary
                            .notes
                            .push("冷却末点未低于阈值，或末点温度缺失/无效".into());
                    }
                }
                StageKind::Manual => {
                    if !confirmed(runtime, index) {
                        summary
                            .notes
                            .push("缺少本段人工确认、实际操作者或操作说明".into());
                    }
                }
                StageKind::Heating => {
                    if summary
                        .maximum_observed_ramp_c_min
                        .is_some_and(|r| r > stage.ramp_rate)
                    {
                        summary.notes.push(
                            "相邻有效采样估算升温速度超过程序设定，需复核仪表制度与采样间隔".into(),
                        );
                    }
                }
            }
            summary
        })
        .collect();
    ProcessAssessment {
        version: VERSION.into(),
        unassigned_points,
        stages,
        method_conformity: "not_determined".into(),
    }
}

/// Render recorded facts and unresolved evidence, without a pass/fail certificate.
pub fn render(assessment: &ProcessAssessment) -> String {
    fn esc(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    }
    fn range(value: Option<[f64; 2]>) -> String {
        value
            .map(|v| format!("{:.2}—{:.2}", v[0], v[1]))
            .unwrap_or_else(|| "缺失".into())
    }
    fn seconds(value: Option<f64>) -> String {
        value
            .map(|v| format!("{v:.2} s"))
            .unwrap_or_else(|| "缺失".into())
    }
    let mut rows = String::new();
    for stage in &assessment.stages {
        let gas = stage
            .gas_pv_ranges
            .iter()
            .enumerate()
            .map(|(i, v)| {
                format!(
                    "{} {}（设定 {}）",
                    ["H₂", "N₂", "CO₂", "CO"][i],
                    range(*v),
                    stage.gas_targets[i]
                )
            })
            .collect::<Vec<_>>()
            .join("；");
        let notes = if stage.notes.is_empty() {
            "已列记录未发现上述缺项；仍须复核连续过程和外部证据".into()
        } else {
            stage.notes.join("；")
        };
        rows.push_str("<tr>");
        for value in [
            format!("{}. {}", stage.index + 1, stage.name),
            format!(
                "{} 点；有效温度 {} / 气体 {}",
                stage.points, stage.valid_temperature_points, stage.valid_gas_points
            ),
            format!(
                "{} ℃；程序 {} ± {} ℃",
                range(stage.temperature_range),
                stage.target_temp,
                stage.tolerance
            ),
            format!(
                "总计时 {}；连续稳定 {}；要求 {}",
                seconds(stage.elapsed_s),
                seconds(stage.stable_elapsed_s),
                seconds(stage.required_seconds)
            ),
            gas,
            format!(
                "{}；最大间隔 {}；相邻升温速度 {} ℃/min",
                notes,
                seconds(stage.maximum_sample_gap_s),
                stage
                    .maximum_observed_ramp_c_min
                    .map(|v| format!("{v:.2}"))
                    .unwrap_or_else(|| "缺失".into())
            ),
        ] {
            rows.push_str(&format!("<td>{}</td>", esc(&value)));
        }
        rows.push_str("</tr>");
    }
    format!("<p>记录核查版本：{}。未能归属阶段的采样：{} 条。T8 测点对应、设备校准、气体纯度/标态和外部操作仍需实验室复核；该摘要不能证明国标方法完全符合。</p><table class=\"kv3\"><thead><tr><th>阶段</th><th>采样证据</th><th>温度范围</th><th>计时</th><th>气体 PV 范围（L/min）</th><th>缺项与记录提示</th></tr></thead><tbody>{}</tbody></table><p>连续稳定计时来自控制过程记录；采样最大间隔和升温估计只覆盖已保存记录。气体设定按本次配置分辨率核对，PV 允差不由软件推测。动态或人工阶段的要求时长为 0，表示由条件/人工操作推进。</p>",esc(&assessment.version),assessment.unassigned_points,rows)
}

/// Missing or invalid legacy program snapshots cannot support a stage assessment.
pub fn from_record(
    experiment: &crate::storage::ExperimentRow,
    samples: &[SampleRow],
    runtime: &Value,
) -> Option<ProcessAssessment> {
    let program: Program =
        serde_json::from_str(experiment.program_snapshot_json.as_deref()?).ok()?;
    crate::program::validate_program(&program).ok()?;
    Some(assess(&program, samples, runtime))
}
