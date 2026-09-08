//! Report & export generation (docs/05 §5.7). Pure producers — a CSV string and a
//! self-contained printable HTML report — golden-tested here; the command layer just
//! fetches rows and writes bytes. No I/O, no tauri.

use crate::storage::{ExperimentRow, SampleRow};
use rust_xlsxwriter::{Format, Workbook, Worksheet, XlsxError};

const COLUMNS:&str="时间,实验时长,温度(℃),质量(g),失重(g),CO(L/min),CO2(L/min),N2(L/min),H2(L/min),状态,数据质量,原始记录JSON,T1,T2,T3,T4,T5,T6,T7,T8,T9,H2_SV,N2_SV,CO2_SV,CO_SV";
fn measurement_valid(row: &SampleRow, key: &str) -> bool {
    if let Some(raw) = &row.record_json {
        return serde_json::from_str::<serde_json::Value>(raw)
            .ok()
            .and_then(|v| v["telemetry"]["health"][key].as_bool())
            == Some(true);
    }
    let Some(message) = row.system_message.as_deref() else {
        return true;
    };
    let balance = message.contains("天平");
    let temp = message.contains("温度");
    let mfc = message.contains("MFC") || message.contains("气体");
    if !balance && !temp && !mfc {
        return false;
    }
    match key {
        "balance_valid" => !balance,
        "temp_valid" => !temp,
        "mfc_valid" => !mfc,
        _ => false,
    }
}

fn sample_fields(r: &SampleRow) -> Vec<String> {
    let record: serde_json::Value =
        serde_json::from_str(r.record_json.as_deref().unwrap_or("{}")).unwrap_or_default();
    let telemetry = &record["telemetry"];
    let legacy = r.record_json.is_none();
    let good = |k: &str| measurement_valid(r, k);
    let n = |v: f64, d: usize, valid: bool| {
        if valid && v.is_finite() {
            format!("{v:.d$}")
        } else {
            String::new()
        }
    };
    let mut fields = vec![
        r.timestamp.clone(),
        r.experiment_duration.clone().unwrap_or_default(),
        n(r.temperature, 1, good("temp_valid")),
        n(r.weight, 3, good("balance_valid")),
        n(r.weight_loss, 3, good("balance_valid")),
        n(r.co_flow, 2, good("mfc_valid")),
        n(r.co2_flow, 2, good("mfc_valid")),
        n(r.n2_flow, 2, good("mfc_valid")),
        n(r.h2_flow, 2, good("mfc_valid")),
        r.experiment_status.clone().unwrap_or_default(),
        r.system_message.clone().unwrap_or_else(|| {
            if legacy {
                "旧记录：未保存完整质量码".into()
            } else {
                "有效".into()
            }
        }),
        r.record_json.clone().unwrap_or_default(),
    ];
    for i in 0..9 {
        fields.push(if good("temp_valid") {
            telemetry["temps"][i]
                .as_f64()
                .map(|v| v.to_string())
                .unwrap_or_default()
        } else {
            String::new()
        });
    }
    for i in 0..4 {
        fields.push(
            telemetry["gas"][i]["sv"]
                .as_f64()
                .map(|v| v.to_string())
                .unwrap_or_default(),
        );
    }
    fields
}
fn delimited_field(s: &str, sep: char) -> String {
    if s.contains([sep, '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}
fn samples_delimited(rows: &[SampleRow], sep: char) -> String {
    let mut out = format!("\u{FEFF}{}\r\n", COLUMNS.replace(',', &sep.to_string()));
    for r in rows {
        out.push_str(
            &sample_fields(r)
                .iter()
                .map(|s| delimited_field(s, sep))
                .collect::<Vec<_>>()
                .join(&sep.to_string()),
        );
        out.push_str("\r\n");
    }
    out
}

/// CSV of the sample series. UTF-8 BOM + CRLF so Excel opens the Chinese headers.
pub fn samples_to_csv(rows: &[SampleRow]) -> String {
    samples_delimited(rows, ',')
}

/// Tab-separated variant for a .txt export.
pub fn samples_to_tsv(rows: &[SampleRow]) -> String {
    samples_delimited(rows, '\t')
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn meta_row(label: &str, value: &str) -> String {
    format!("<tr><th>{}</th><td>{}</td></tr>", esc(label), esc(value))
}

/// Format the stored analysis JSON as an HTML block (standard-specific rows).
fn analysis_html(analysis: Option<&serde_json::Value>) -> String {
    let Some(a) = analysis else {
        return "<p class=\"muted\">尚未进行结果分析。</p>".to_string();
    };
    let f = |k: &str| a.get(k).and_then(|v| v.as_f64());
    let std = a.get("standard").and_then(|v| v.as_str()).unwrap_or("");
    let mut rows = String::new();
    let mut push = |label: &str, val: String| {
        rows.push_str(&format!("<tr><th>{}</th><td>{}</td></tr>", esc(label), val));
    };
    match std {
        "GB/T 13241" => {
            if let Some(v) = f("rt") {
                push("还原度 Rt", format!("{v:.2} %"));
                if let Some(t) = f("endpoint_min") {
                    push("计算终点", format!("{t} min"));
                }
            }
            if let Some(v) = f("delta_w") {
                push("失重 ΔW", format!("{v:.3} g"));
            }
            if let Some(v) = f("reduction_rate_index") {
                push("还原速率指数", format!("{v:.3} %/min"));
            }
            if let Some(v) = f("o2_pct") {
                push("全铁含氧量 O₂%", format!("{v:.1} %"));
            }
        }
        "GB/T 13242" => {
            if let Some(v) = f("rdi_plus_6_3") {
                push("RDI +6.3", format!("{v:.2} %"));
            }
            if let Some(v) = f("rdi_plus_3_15") {
                push("RDI +3.15", format!("{v:.2} %"));
            }
            if let Some(v) = f("rdi_minus_0_5") {
                push("RDI −0.5", format!("{v:.2} %"));
            }
            if a.get("sieve_warning").and_then(|v| v.as_bool()) == Some(true) {
                push("提示", "筛分总质量与 m₀ 偏差 &gt; 2%".to_string());
            }
        }
        "GB/T 13240" => {
            if let Some(v) = f("swelling_index") {
                push("整份自由膨胀指数", format!("{v:.2} %"));
            }
        }
        _ => {}
    }
    if rows.is_empty() {
        return "<p class=\"muted\">尚未进行结果分析。</p>".to_string();
    }
    let raw = serde_json::to_string_pretty(a.get("input").unwrap_or(&serde_json::Value::Null))
        .unwrap_or_default();
    format!("<table class=\"kv\">{rows}</table><p>单次计算结果；最终检验需过程记录、外部步骤与重复性验收。</p><p>计算版本：{}</p><h3>原始分析输入（人工录入）</h3><pre>{}</pre>",esc(a["calculation_version"].as_str().unwrap_or("旧版：待复核")),esc(&raw))
}

/// Check each measured reduction point against the actual standard method.
/// Averages never substitute for point-wise bounds; missing channel mapping is explicit.
fn conditions_html(
    exp: &ExperimentRow,
    samples: &[SampleRow],
    target: Option<f64>,
    analysis: Option<&serde_json::Value>,
) -> String {
    let red: Vec<_> = samples
        .iter()
        .filter(|r| r.experiment_status.as_deref() == Some("REDUCING"))
        .filter(|r| {
            !r.record_json
                .as_deref()
                .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
                .is_some_and(|v| v["sample_role"] == "reduction_baseline")
        })
        .collect();
    if red.is_empty() {
        return "<p>无还原段采样，跳过条件核查。缺少过程证据。</p>".into();
    }
    let program: Option<crate::engine::Program> = exp
        .program_snapshot_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok());
    let target = program
        .as_ref()
        .and_then(|p| {
            p.stages
                .iter()
                .find(|s| s.kind == crate::engine::StageKind::Reducing)
        })
        .map(|s| s.target_temp)
        .or(target);
    let method = program
        .as_ref()
        .and_then(|p| p.base_standard_id.as_deref())
        .or(exp.experiment_type.as_deref())
        .unwrap_or("");
    let (temperature, tolerance, flow_tolerance, ratios) = match method {
        "REDUCIBILITY" => (900.0, 5.0, 0.5, [0.0, 70.0, 0.0, 30.0]),
        "LOW_TEMP_DEGRADATION" => (500.0, 5.0, 0.5, [0.0, 60.0, 20.0, 20.0]),
        "FREE_SWELLING" => (900.0, 10.0, 1.0, [0.0, 70.0, 0.0, 30.0]),
        _ => return "<p>未知基础标准，不能自动判定条件。</p>".into(),
    };
    let actual_minutes = red
        .iter()
        .filter_map(|r| {
            r.record_json
                .as_deref()
                .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
                .and_then(|v| v["stage_elapsed_s"].as_f64())
        })
        .filter(|v| v.is_finite() && *v >= 0.0)
        .reduce(f64::max)
        .map(|v| v / 60.0);
    let expected_minutes = if method == "REDUCIBILITY" {
        180.0
    } else {
        60.0
    };
    let runtime = analysis.and_then(|v| v.get("runtime_evidence"));
    let skip = runtime
        .and_then(|v| v["skipped"].as_bool())
        .unwrap_or(false);
    let duration = meta_row(
        "还原时长",
        &format!(
            "实测 {}；基础国标 {} min{}",
            actual_minutes
                .map(|v| format!("{v:.2} min"))
                .unwrap_or_else(|| "无阶段时间证据".into()),
            expected_minutes,
            if method == "FREE_SWELLING" {
                "（标准允许的较短时长须说明依据）"
            } else {
                ""
            }
        ),
    );
    let skip_notes = runtime
        .and_then(|v| v["stage_actions"].as_array())
        .map(|events| {
            events
                .iter()
                .filter(|e| e["kind"] == "skip")
                .filter_map(|e| e["notes"].as_str())
                .collect::<Vec<_>>()
                .join("；")
        })
        .filter(|s| !s.is_empty());
    let skip_detail = meta_row(
        "跳段及原因",
        if skip {
            skip_notes
                .as_deref()
                .or_else(|| runtime.and_then(|v| v["skip_reason"].as_str()))
                .unwrap_or("已跳段，原因未记录")
        } else if runtime.is_none() {
            "旧记录：未保存跳段信息"
        } else {
            "未记录跳段"
        },
    );
    let mut bad_temp = 0;
    let mut bad_flow = 0;
    let mut bad_ratio = 0;
    let mut missing = 0;
    for r in &red {
        if !r.temperature.is_finite() || (r.temperature - temperature).abs() > tolerance {
            bad_temp += 1;
        }
        let gas = [r.h2_flow, r.n2_flow, r.co2_flow, r.co_flow];
        let total: f64 = gas.iter().sum();
        if !total.is_finite() || (total - 15.0).abs() > flow_tolerance {
            bad_flow += 1;
        }
        if total <= 0.0
            || !total.is_finite()
            || gas
                .iter()
                .zip(ratios)
                .any(|(v, expected)| !v.is_finite() || (*v / total * 100.0 - expected).abs() > 0.5)
        {
            bad_ratio += 1;
        }
        if r.system_message.is_some() || r.record_json.is_none() {
            missing += 1;
        }
    }
    let verdict = |n: usize| {
        if n == 0 {
            "已记录点未发现超差"
        } else {
            "超差"
        }
    };
    let custom = program
        .as_ref()
        .is_some_and(|p| p.source == crate::engine::ProgramSource::Custom);
    format!("<p>{}</p><table class=\"kv\">{duration}{skip_detail}{}{}{}{}{} </table><p>切换还原气前的基准称量不纳入气氛核查。本表仅核查已记录的 T8 与流量点；料层上、中、下测点对应、标态换算、气体纯度与校准记录须在人工补录及复核中确认。不能由统计结果推定全过程符合国标。</p>",
      if custom{"自定义实验：以下为相对基础国标的偏离核查"}else{"国标模板条件核查"},
      meta_row(&format!("T8 温度（{temperature}±{tolerance}℃）"),&format!("{} / {} 点超差；{}",bad_temp,red.len(),verdict(bad_temp))),
      meta_row(&format!("总流量（15±{flow_tolerance} L/min）"),&format!("{} 点；{}",bad_flow,verdict(bad_flow))),
      meta_row("气体体积分数（±0.5 个百分点）",&format!("{} 点；{}",bad_ratio,verdict(bad_ratio))),
      meta_row("缺测或缺少质量记录",&missing.to_string()),
      meta_row("执行程序还原目标",&target.map(|v|format!("{v} ℃")).unwrap_or_else(||"待确认".into())))
}

fn custom_program_evidence_html(program: &crate::engine::Program) -> String {
    if program.source != crate::engine::ProgramSource::Custom {
        return String::new();
    }
    let mut html = String::from(
        "<h3>自定义程序依据</h3><p>自定义结果用于研究比较，不能据此声明符合国标方法。</p>",
    );
    let Some(p) = &program.provenance else {
        html.push_str(
            "<p>历史程序没有保存版本依据、编辑者及条件差异；不能推定与当前模板一致。</p>",
        );
        return html;
    };
    html.push_str(&format!(
        "<table class=\"kv\">{}{}{}{}{}</table>",
        meta_row("自定义版本", &p.revision.to_string()),
        meta_row("基础国标", &p.parent_template.name),
        meta_row("保存人", &p.edited_by),
        meta_row("保存时间", &p.edited_at),
        meta_row("修改原因", &p.reason)
    ));
    if p.changes.is_empty() {
        html.push_str("<p>实验条件与保存的基础国标模板一致。</p>");
        return html;
    }
    html.push_str("<table class=\"kv3\"><tr><th>阶段 / 条件</th><th>基础国标条件</th><th>自定义条件</th></tr>");
    for c in &p.changes {
        let label = match c.field.as_str() {
            "target_temp" => "温度 (℃)",
            "tolerance" => "容差 (℃)",
            "duration_min" => "时长 (min)",
            "ramp_rate" => "升温率 (℃/min)",
            "gas" => "气体 [H₂, N₂, CO₂, CO] (L/min)",
            "name" => "阶段名称",
            "description" => "说明",
            "kind" => "类型",
            _ => &c.field,
        };
        let display = |value: &serde_json::Value| {
            value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string())
        };
        html.push_str(&format!(
            "<tr><td>{} / {}</td><td>{}</td><td>{}</td></tr>",
            c.stage_index + 1,
            esc(label),
            esc(&display(&c.before)),
            esc(&display(&c.after))
        ));
    }
    html.push_str("</table>");
    html
}

fn procedure_evidence_html(exp: &ExperimentRow, analysis: Option<&serde_json::Value>) -> String {
    let runtime = analysis.and_then(|a| a.get("runtime_evidence"));
    let program: Option<crate::engine::Program> = exp
        .program_snapshot_json
        .as_deref()
        .and_then(|raw| serde_json::from_str(raw).ok());
    let mut html = format!(
        "<table class=\"kv\">{}{}{}{}</table>",
        meta_row(
            "程序版本",
            program
                .as_ref()
                .and_then(|p| p.template_revision.as_deref())
                .unwrap_or("历史程序，未记录版本")
        ),
        meta_row(
            "设备模式",
            runtime
                .and_then(|r| r["device_mode"].as_str())
                .unwrap_or("未记录")
        ),
        meta_row(
            "结束结果",
            runtime
                .and_then(|r| r["outcome"].as_str())
                .unwrap_or("未记录")
        ),
        meta_row(
            "结束气氛动作",
            runtime
                .and_then(|r| r["ending_action"].as_str())
                .unwrap_or("未记录")
        )
    );
    if let Some(program) = &program {
        html.push_str(&custom_program_evidence_html(program));
    }
    let events = runtime.and_then(|r| r["stage_actions"].as_array());
    if let Some(events) = events.filter(|events| !events.is_empty()) {
        html.push_str("<table class=\"kv3\"><tr><th>步骤</th><th>操作</th><th>确认人 / 时间</th><th>备注</th></tr>");
        for event in events {
            let stage = event["stage_index"]
                .as_u64()
                .and_then(|i| program.as_ref().and_then(|p| p.stages.get(i as usize)))
                .map(|s| s.name.as_str())
                .unwrap_or("阶段未记录");
            let action = if event["kind"] == "confirm" {
                "人工确认"
            } else {
                "跳段"
            };
            html.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td>{} / {}</td><td>{}</td></tr>",
                esc(stage),
                action,
                esc(event["operator"].as_str().unwrap_or("未记录")),
                esc(event["confirmed_at"].as_str().unwrap_or("未记录")),
                esc(event["notes"].as_str().unwrap_or(""))
            ));
        }
        html.push_str("</table>");
    } else {
        html.push_str(
            "<p>没有已保存的人工步骤确认记录。旧记录不能据此推定已执行取管、关炉或室温确认。</p>",
        );
    }
    html.push_str("<p>人工确认记录反映操作员声明；模拟运行用于验证软件流程，不能替代真实仪器、校准和国标实验验证。</p>");
    html
}

fn report_details_html(analysis: Option<&serde_json::Value>) -> String {
    let details = analysis.and_then(|a| a.get("report_details"));
    let labels = [
        ("lab_name", "实验室名称"),
        ("lab_address", "实验室地址"),
        ("report_number", "报告编号"),
        ("report_date", "报告日期"),
        ("sample_preparation", "试样制备、粒度、干燥与称量"),
        ("equipment", "仪器设备与测点对应"),
        ("gas_and_calibration", "气体纯度、标态与校准记录"),
        ("external_steps", "提炉、冷却、转鼓/体积等人工步骤"),
        ("deviations", "条件偏离及处理"),
        ("reviewer", "复核与签署人（人工填写）"),
        ("repeat_group_reference", "重复试验组与验收记录"),
    ];
    let rows = labels
        .iter()
        .map(|(key, label)| {
            meta_row(
                label,
                details
                    .and_then(|d| d.get(key))
                    .and_then(|v| v.as_str())
                    .filter(|v| !v.trim().is_empty())
                    .unwrap_or("待补录"),
            )
        })
        .collect::<String>();
    let runtime = analysis.and_then(|a| a.get("runtime_evidence"));
    format!("<table class=\"kv\">{rows}{}{}</table><p>补录操作员与时间随历史版本保存。人工补录内容由操作员填写；签署人姓名不等同于电子签名。未完成重复性及过程复核时，本文件为单次试验记录。</p>",meta_row("数据来源",runtime.and_then(|v|v["device_mode"].as_str()).unwrap_or("旧记录：来源待确认")),meta_row("过程终态",runtime.and_then(|v|v["outcome"].as_str()).unwrap_or("旧记录：终态未记录")))
}

/// Inline SVG temperature/mass overlay (decimated to ≤200 points).
fn svg_chart(samples: &[SampleRow]) -> String {
    if samples.is_empty() {
        return "<p class=\"muted\">无采样数据。</p>".to_string();
    }
    let (w, h, pad) = (760.0_f64, 260.0_f64, 40.0_f64);
    let m = samples.len().max(2);
    let max_w = samples.iter().map(|s| s.weight).fold(1.0_f64, f64::max);
    let xat = |i: usize| pad + (i as f64 / (m - 1) as f64) * (w - 2.0 * pad);
    let y_temp = |t: f64| (h - pad) - (t / 1000.0) * (h - 2.0 * pad);
    let y_mass = |g: f64| (h - pad) - (g / max_w) * (h - 2.0 * pad);
    let line = |field: &str, f: &dyn Fn(&SampleRow) -> f64| {
        let stride = (samples.len() / 200).max(1);
        let mut path = String::new();
        let mut connected = false;
        for (i, s) in samples.iter().enumerate() {
            let valid = measurement_valid(s, field);
            if !valid || !f(s).is_finite() {
                connected = false;
                continue;
            }
            if connected && i % stride != 0 && i + 1 != samples.len() {
                continue;
            }
            path.push_str(&format!(
                "{} {:.1},{:.1} ",
                if connected { "L" } else { "M" },
                xat(i),
                f(s)
            ));
            connected = true;
        }
        path
    };
    let temp = line("temp_valid", &|s| y_temp(s.temperature));
    let mass = line("balance_valid", &|s| y_mass(s.weight));
    format!(
        "<svg viewBox=\"0 0 {w} {h}\" class=\"chart\" xmlns=\"http://www.w3.org/2000/svg\">\
         <rect x=\"{pad}\" y=\"10\" width=\"{iw}\" height=\"{ih}\" fill=\"none\" stroke=\"#ccc\"/>\
         <path fill=\"none\" stroke=\"#d9631f\" stroke-width=\"1.6\" d=\"{temp}\"/>\
         <path fill=\"none\" stroke=\"#1f9d8f\" stroke-width=\"1.6\" d=\"{mass}\"/>\
         <text x=\"{pad}\" y=\"8\" font-size=\"11\" fill=\"#666\">温度 ℃ (橙) / 质量 g (青)</text>\
         </svg>",
        iw = w - 2.0 * pad,
        ih = h - pad - 10.0,
    )
}

/// A self-contained, printable HTML report for one experiment.
pub fn render_report_html(
    exp: &ExperimentRow,
    samples: &[SampleRow],
    analysis: Option<&serde_json::Value>,
    reducing_target: Option<f64>,
) -> String {
    let meta = [
        meta_row("实验编号", &exp.experiment_id),
        meta_row("样品编号", exp.sample_id.as_deref().unwrap_or("—")),
        meta_row("项目名称", exp.project_name.as_deref().unwrap_or("—")),
        meta_row("样品名称", &exp.sample_name),
        meta_row("实验类型", exp.experiment_type.as_deref().unwrap_or("—")),
        meta_row("操作员", exp.operator.as_deref().unwrap_or("—")),
        meta_row("样品重量", &format!("{:.3} g", exp.sample_weight)),
        meta_row("实验时间", exp.experiment_time.as_deref().unwrap_or("—")),
        meta_row("开始时间", &exp.start_time),
        meta_row("结束时间", exp.end_time.as_deref().unwrap_or("—")),
        meta_row("采样点数", &samples.len().to_string()),
        meta_row("备注", exp.description.as_deref().unwrap_or("—")),
    ]
    .concat();

    format!(
        "<!DOCTYPE html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\">\
<title>检测报告 {id}</title><style>\
body{{font-family:'Microsoft YaHei',system-ui,sans-serif;color:#1a1a1a;max-width:820px;margin:24px auto;padding:0 16px}}\
h1{{font-size:20px;margin:0}}h2{{font-size:14px;margin:24px 0 8px;border-bottom:2px solid #d9631f;padding-bottom:4px}}\
.sub{{color:#666;font-size:12px;margin-top:4px}}\
table{{border-collapse:collapse;width:100%;font-size:13px}}\
.kv th{{text-align:left;width:140px;color:#555;font-weight:600;padding:5px 8px;background:#f6f6f6;border:1px solid #e2e2e2}}\
.kv td{{padding:5px 8px;border:1px solid #e2e2e2}}\
.kv3 th,.kv3 td{{padding:5px 8px;border:1px solid #e2e2e2;text-align:left}}\
.kv3 th{{background:#f6f6f6;color:#555;font-weight:600}}\
.chart{{width:100%;height:auto;border:1px solid #eee;margin-top:8px}}\
.muted{{color:#999;font-size:13px}}\
.foot{{margin-top:32px;color:#999;font-size:11px;text-align:center}}\
@media print{{body{{margin:0}}}}</style></head><body>\
<h1>铁矿石冶金性能检测报告</h1>\
<div class=\"sub\">TMH-LPF-900 · 北京科技大学</div>\
<h2>实验信息</h2><table class=\"kv\">{meta}</table>\
<h2>报告信息与人工补录</h2>{report_details}<h2>结果分析</h2>{analysis}\
<h2>流程与人工操作记录</h2>{procedure}<h2>条件核查</h2>{conditions}\
<h2>温度 / 质量曲线</h2>{chart}\
<div class=\"foot\">本报告由 TMH-LPF-900 自动生成</div>\
</body></html>",
        id = esc(&exp.experiment_id),
        meta = meta,
        analysis = analysis_html(analysis),
        conditions = conditions_html(exp, samples, reducing_target, analysis),
        report_details = report_details_html(analysis),
        procedure = procedure_evidence_html(exp, analysis),
        chart = svg_chart(samples),
    )
}

/// XLSX workbook (info sheet + data sheet) as in-memory bytes. Excel-openable.
pub fn samples_to_xlsx(exp: &ExperimentRow, rows: &[SampleRow]) -> Result<Vec<u8>, String> {
    build_xlsx(exp, rows).map_err(|e| e.to_string())
}

fn build_xlsx(exp: &ExperimentRow, rows: &[SampleRow]) -> Result<Vec<u8>, XlsxError> {
    let bold = Format::new().set_bold();

    // Standalone worksheets avoid two simultaneous &mut borrows of the workbook.
    let mut info = Worksheet::new();
    info.set_name("实验信息")?;
    let meta: [(&str, String); 10] = [
        ("实验编号", exp.experiment_id.clone()),
        ("样品编号", exp.sample_id.clone().unwrap_or_default()),
        ("项目名称", exp.project_name.clone().unwrap_or_default()),
        ("样品名称", exp.sample_name.clone()),
        ("实验类型", exp.experiment_type.clone().unwrap_or_default()),
        ("操作员", exp.operator.clone().unwrap_or_default()),
        ("样品重量(g)", format!("{:.3}", exp.sample_weight)),
        ("实验时间", exp.experiment_time.clone().unwrap_or_default()),
        ("开始时间", exp.start_time.clone()),
        ("结束时间", exp.end_time.clone().unwrap_or_default()),
    ];
    for (i, (k, v)) in meta.iter().enumerate() {
        info.write_string_with_format(i as u32, 0, *k, &bold)?;
        info.write_string(i as u32, 1, v)?;
    }

    let mut data = Worksheet::new();
    data.set_name("数据")?;
    for (c, h) in COLUMNS.split(',').enumerate() {
        data.write_string_with_format(0, c as u16, h, &bold)?;
    }
    for (i, r) in rows.iter().enumerate() {
        for (c, value) in sample_fields(r).iter().enumerate() {
            if (2..9).contains(&c) || c >= 12 {
                if let Ok(n) = value.parse::<f64>() {
                    data.write_number(i as u32 + 1, c as u16, n)?;
                    continue;
                }
            }
            data.write_string(i as u32 + 1, c as u16, value)?;
        }
    }

    let mut wb = Workbook::new();
    wb.push_worksheet(info);
    wb.push_worksheet(data);
    wb.save_to_buffer()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exp() -> ExperimentRow {
        ExperimentRow {
            experiment_id: "e1".into(),
            experiment_name: "s".into(),
            sample_id: Some("RED-20260705-001".into()),
            sample_name: "鞍钢球团".into(),
            sample_weight: 100.0,
            start_time: "2026-07-05T10:00:00".into(),
            experiment_time: Some("2026-07-05T09:58:00".into()),
            end_time: Some("2026-07-05T14:00:00".into()),
            project_name: Some("proj".into()),
            description: None,
            operator: Some("张工".into()),
            experiment_type: Some("REDUCIBILITY".into()),
            analysis_results_json: None,
            program_snapshot_json: None,
            created_at: "2026-07-05T10:00:00".into(),
            owner_user_id: None,
        }
    }

    fn sample(t: f64, w: f64) -> SampleRow {
        SampleRow {
            record_json: None,
            experiment_id: "e1".into(),
            timestamp: "2026-07-05T10:00:01".into(),
            experiment_duration: Some("00:00:01".into()),
            temperature: t,
            weight: w,
            weight_loss: 100.0 - w,
            co_flow: 4.5,
            co2_flow: 0.0,
            n2_flow: 10.5,
            h2_flow: 0.0,
            experiment_status: Some("RUNNING".into()),
            system_message: None,
        }
    }

    #[test]
    fn csv_has_bom_header_and_rows() {
        let csv = samples_to_csv(&[sample(900.0, 88.0)]);
        assert!(csv.starts_with('\u{FEFF}'));
        assert!(csv.contains("温度(℃)"));
        assert!(csv.contains("900.0,88.000,12.000"));
        assert_eq!(csv.matches("\r\n").count(), 2); // header + 1 row
    }

    fn reducing_sample(t: f64, dur: &str) -> SampleRow {
        SampleRow {
            experiment_duration: Some(dur.into()),
            experiment_status: Some("REDUCING".into()),
            ..sample(t, 90.0)
        }
    }

    #[test]
    fn html_report_embeds_metadata_analysis_and_chart() {
        let analysis =
            serde_json::json!({ "standard": "GB/T 13241", "rt": 42.11, "delta_w": 12.0 });
        let samples = [sample(25.0, 100.0), sample(900.0, 88.0)];
        let html = render_report_html(&exp(), &samples, Some(&analysis), Some(900.0));
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("鞍钢球团"));
        assert!(html.contains("还原度 Rt"));
        assert!(html.contains("42.11 %"));
        assert!(html.contains("<svg"));
        assert!(html.contains("<path"));
    }

    #[test]
    fn html_report_without_analysis_is_valid() {
        let html = render_report_html(&exp(), &[], None, None);
        assert!(html.contains("尚未进行结果分析"));
        assert!(html.contains("无采样数据"));
        assert!(html.contains("跳过条件核查"));
    }

    #[test]
    fn condition_check_passes_for_stable_reduction() {
        // 900 °C, total flow 15 (CO 4.5 + N2 10.5), long hold.
        let samples = [
            reducing_sample(900.0, "01:00:00"),
            reducing_sample(901.0, "02:30:00"),
            reducing_sample(899.0, "04:00:00"),
        ];
        let html = render_report_html(&exp(), &samples, None, Some(900.0));
        assert!(html.contains("条件核查"));
        assert!(html.contains("T8 温度"));
        assert!(html.contains("已记录点未发现超差"));
    }

    #[test]
    fn xlsx_produces_zip_bytes() {
        let bytes = samples_to_xlsx(&exp(), &[sample(900.0, 88.0)]).unwrap();
        assert_eq!(&bytes[0..2], b"PK"); // XLSX is a zip archive
        assert!(bytes.len() > 1000);
    }
    #[test]
    fn isolated_temperature_excursion_is_not_hidden_by_the_mean() {
        let mut e = exp();
        e.experiment_type = Some("LOW_TEMP_DEGRADATION".into());
        let mut samples = vec![sample(500.0, 88.0); 100];
        for r in &mut samples {
            r.temperature = 500.0;
            r.experiment_status = Some("REDUCING".into());
            r.co_flow = 3.0;
            r.co2_flow = 3.0;
            r.n2_flow = 9.0;
            r.h2_flow = 0.0;
        }
        samples[50].temperature = 510.0;
        let html = render_report_html(&e, &samples, None, Some(500.0));
        assert!(html.contains("1 / 100 点超差"));
        assert!(!html.contains("合格</td>"));
    }
    #[test]
    fn report_includes_supplement_and_escapes_user_text() {
        let a = serde_json::json!({"standard":"GB/T 13240","swelling_index":33.3333333333,"input":{"v0":27,"v1":36},"calculation_version":"test-v1","report_details":{"lab_name":"<script>lab</script>"}});
        let html = render_report_html(&exp(), &[], Some(&a), None);
        assert!(html.contains("33.33 %"));
        assert!(html.contains("&lt;script&gt;lab&lt;/script&gt;"));
        assert!(!html.contains("<script>"));
        assert!(html.contains("待补录"));
        assert!(html.contains("test-v1"));
    }
    #[test]
    fn exported_missing_measurements_are_empty_with_quality_and_raw_evidence() {
        let mut r = sample(900.0, 88.0);
        r.system_message = Some("天平离线".into());
        r.weight = 0.0;
        r.record_json=Some(serde_json::json!({"telemetry":{"health":{"temp_valid":true,"mfc_valid":true,"balance_valid":false},"temps":[1,2,3,4,5,6,7,8,9],"gas":[{"sv":0},{"sv":15},{"sv":0},{"sv":0}]}}).to_string());
        let fields = sample_fields(&r);
        assert_eq!(fields[3], "");
        assert_eq!(fields[4], "");
        assert_eq!(fields[10], "天平离线");
        assert_eq!(fields[20], "9");
        assert!(samples_to_csv(&[r]).contains("\"\"telemetry\"\""));
    }
    #[test]
    fn legacy_balance_failure_preserves_temperature_and_flow() {
        let mut row = sample(900.0, 0.0);
        row.system_message = Some("天平离线".into());
        let fields = sample_fields(&row);
        assert_eq!(fields[2], "900.0");
        assert_eq!(fields[3], "");
        assert_eq!(fields[5], "4.50");
    }
    #[test]
    fn report_exposes_short_reduction_and_skip_reason() {
        let mut row = reducing_sample(900.0, "00:30:00");
        row.record_json=Some(serde_json::json!({"stage_elapsed_s":1800,"telemetry":{"health":{"temp_valid":true,"mfc_valid":true,"balance_valid":true}}}).to_string());
        let a =
            serde_json::json!({"runtime_evidence":{"skipped":true,"skip_reason":"操作员提前取样"}});
        let html = render_report_html(&exp(), &[row], Some(&a), Some(900.0));
        assert!(html.contains("还原时长"));
        assert!(html.contains("30.00 min"));
        assert!(html.contains("180 min"));
        assert!(html.contains("操作员提前取样"));
    }
    #[test]
    fn custom_report_preserves_executed_revision_reason_and_condition_changes() {
        use crate::provenance::{prepare_custom_revision, ProgramEditor};
        let mut p = crate::engine::standard_program("REDUCIBILITY").unwrap();
        p.id = "custom-report".into();
        p.stages[2].target_temp = 850.0;
        let saved = prepare_custom_revision(
            p,
            None,
            ProgramEditor {
                user_id: "a".into(),
                display_name: "Lab <A>".into(),
                edited_at: "2026-09-08T12:00:00Z".into(),
            },
            "Compare <850>",
        )
        .unwrap();
        let mut row = exp();
        row.program_snapshot_json = Some(serde_json::to_string(&saved).unwrap());
        let html = render_report_html(&row, &[], None, None);
        assert!(html.contains("自定义版本"));
        assert!(html.contains("Compare &lt;850&gt;"));
        assert!(html.contains("Lab &lt;A&gt;"));
        assert!(html.contains("基础国标条件"));
        assert!(html.contains("850"));
        assert!(html.contains("不能据此声明符合国标方法"));
        let mut point = sample(850.0, 99.0);
        point.experiment_status = Some("REDUCING".into());
        let conditions = conditions_html(&row, &[point], Some(900.0), None);
        assert!(conditions.contains("<th>执行程序还原目标</th><td>850 ℃</td>"));
    }
    #[test]
    fn manual_operation_evidence_is_visible_and_escaped() {
        let mut exp = exp();
        exp.program_snapshot_json = Some(
            serde_json::to_string(&crate::engine::standard_program("REDUCIBILITY").unwrap())
                .unwrap(),
        );
        let analysis = serde_json::json!({"runtime_evidence":{"device_mode":"mock","outcome":"completed","ending_action":"normal","stage_actions":[{"kind":"confirm","stage_index":4,"operator":"Lab <A>","confirmed_at":"2026-09-08T12:00:00Z","notes":"Tube <removed>"}]}});
        let html = procedure_evidence_html(&exp, Some(&analysis));
        assert!(html.contains("取出还原管"));
        assert!(html.contains("Lab &lt;A&gt;"));
        assert!(html.contains("Tube &lt;removed&gt;"));
        assert!(html.contains("2026-09-08T12:00:00Z"));
        assert!(html.contains("不能替代真实仪器"));
    }
}
