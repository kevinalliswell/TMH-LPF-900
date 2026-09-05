//! Report & export generation (docs/05 §5.7). Pure producers — a CSV string and a
//! self-contained printable HTML report — golden-tested here; the command layer just
//! fetches rows and writes bytes. No I/O, no tauri.

use crate::storage::{ExperimentRow, SampleRow};
use rust_xlsxwriter::{Format, Workbook, Worksheet, XlsxError};

const COLUMNS: &str =
    "时间,实验时长,温度(℃),质量(g),失重(g),CO(L/min),CO2(L/min),N2(L/min),H2(L/min),状态";

fn samples_delimited(rows: &[SampleRow], sep: char) -> String {
    let mut out = String::from("\u{FEFF}");
    out.push_str(&COLUMNS.replace(',', &sep.to_string()));
    out.push_str("\r\n");
    for r in rows {
        let fields = [
            r.timestamp.clone(),
            r.experiment_duration.clone().unwrap_or_default(),
            format!("{:.1}", r.temperature),
            format!("{:.3}", r.weight),
            format!("{:.3}", r.weight_loss),
            format!("{:.2}", r.co_flow),
            format!("{:.2}", r.co2_flow),
            format!("{:.2}", r.n2_flow),
            format!("{:.2}", r.h2_flow),
            r.experiment_status.clone().unwrap_or_default(),
        ];
        out.push_str(&fields.join(&sep.to_string()));
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
            if let Some(v) = f("average") {
                push("平均自由膨胀指数", format!("{v:.2} %"));
            }
            if let Some(arr) = a.get("per_pellet").and_then(|v| v.as_array()) {
                let s = arr
                    .iter()
                    .filter_map(|v| v.as_f64())
                    .map(|v| format!("{v:.1}"))
                    .collect::<Vec<_>>()
                    .join(" · ");
                push("单球指数", format!("{s} %"));
            }
        }
        _ => {}
    }
    if rows.is_empty() {
        return "<p class=\"muted\">尚未进行结果分析。</p>".to_string();
    }
    format!("<table class=\"kv\">{rows}</table>")
}

fn parse_hms_min(s: &str) -> Option<f64> {
    let mut it = s.split(':');
    let h: f64 = it.next()?.parse().ok()?;
    let m: f64 = it.next()?.parse().ok()?;
    let sec: f64 = it.next()?.parse().ok()?;
    Some(h * 60.0 + m + sec / 60.0)
}

/// Report-only condition checks over the reduction stage (docs/05 §5.6): temperature
/// mean within target±10, std ≤5, total flow 15±0.5. Non-blocking — informational.
fn conditions_html(samples: &[SampleRow], reducing_target: Option<f64>) -> String {
    let red: Vec<&SampleRow> = samples
        .iter()
        .filter(|s| s.experiment_status.as_deref() == Some("REDUCING"))
        .collect();
    if red.is_empty() {
        return "<p class=\"muted\">无还原段采样，跳过条件核查。</p>".to_string();
    }
    let n = red.len() as f64;
    let mean = red.iter().map(|s| s.temperature).sum::<f64>() / n;
    let std = (red
        .iter()
        .map(|s| (s.temperature - mean).powi(2))
        .sum::<f64>()
        / n)
        .sqrt();
    let flow = red
        .iter()
        .map(|s| s.co_flow + s.co2_flow + s.n2_flow + s.h2_flow)
        .sum::<f64>()
        / n;
    let dur = {
        let first = red
            .first()
            .and_then(|s| s.experiment_duration.as_deref())
            .and_then(parse_hms_min);
        let last = red
            .last()
            .and_then(|s| s.experiment_duration.as_deref())
            .and_then(parse_hms_min);
        match (first, last) {
            (Some(a), Some(b)) => (b - a).max(0.0),
            _ => 0.0,
        }
    };
    let tgt = reducing_target.unwrap_or_else(|| mean.round());

    let badge = |pass: Option<bool>| match pass {
        Some(true) => "<td style=\"color:#1f9d2f\">合格</td>",
        Some(false) => "<td style=\"color:#c0392b\">超差</td>",
        None => "<td class=\"muted\">—</td>",
    };
    let row = |label: String, val: String, pass: Option<bool>| {
        format!(
            "<tr><th>{}</th><td>{}</td>{}</tr>",
            esc(&label),
            val,
            badge(pass)
        )
    };
    let rows = [
        row(
            format!("还原段温度均值 (目标 {tgt:.0}±10℃)"),
            format!("{mean:.1} ℃"),
            Some((mean - tgt).abs() <= 10.0),
        ),
        row(
            "温度标准差 (≤5)".into(),
            format!("{std:.2}"),
            Some(std <= 5.0),
        ),
        row(
            "总流量均值 (15±0.5 L/min)".into(),
            format!("{flow:.2} L/min"),
            Some((flow - 15.0).abs() <= 0.5),
        ),
        row("还原段时长".into(), format!("{dur:.0} min"), None),
    ]
    .concat();
    format!("<table class=\"kv3\"><tr><th>项目</th><th>数值</th><th>判定</th></tr>{rows}</table>")
}

/// Inline SVG temperature/mass overlay (decimated to ≤200 points).
fn svg_chart(samples: &[SampleRow]) -> String {
    if samples.is_empty() {
        return "<p class=\"muted\">无采样数据。</p>".to_string();
    }
    let (w, h, pad) = (760.0_f64, 260.0_f64, 40.0_f64);
    let stride = (samples.len() / 200).max(1);
    let pts: Vec<&SampleRow> = samples.iter().step_by(stride).collect();
    let m = pts.len().max(2);
    let max_w = pts.iter().map(|s| s.weight).fold(1.0_f64, f64::max);
    let xat = |i: usize| pad + (i as f64 / (m - 1) as f64) * (w - 2.0 * pad);
    let y_temp = |t: f64| (h - pad) - (t / 1000.0) * (h - 2.0 * pad);
    let y_mass = |g: f64| (h - pad) - (g / max_w) * (h - 2.0 * pad);
    let line = |f: &dyn Fn(&SampleRow) -> f64| {
        pts.iter()
            .enumerate()
            .map(|(i, s)| format!("{:.1},{:.1}", xat(i), f(s)))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let temp = line(&|s| y_temp(s.temperature));
    let mass = line(&|s| y_mass(s.weight));
    format!(
        "<svg viewBox=\"0 0 {w} {h}\" class=\"chart\" xmlns=\"http://www.w3.org/2000/svg\">\
         <rect x=\"{pad}\" y=\"10\" width=\"{iw}\" height=\"{ih}\" fill=\"none\" stroke=\"#ccc\"/>\
         <polyline fill=\"none\" stroke=\"#d9631f\" stroke-width=\"1.6\" points=\"{temp}\"/>\
         <polyline fill=\"none\" stroke=\"#1f9d8f\" stroke-width=\"1.6\" points=\"{mass}\"/>\
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
<h2>结果分析</h2>{analysis}\
<h2>条件核查</h2>{conditions}\
<h2>温度 / 质量曲线</h2>{chart}\
<div class=\"foot\">本报告由 TMH-LPF-900 自动生成</div>\
</body></html>",
        id = esc(&exp.experiment_id),
        meta = meta,
        analysis = analysis_html(analysis),
        conditions = conditions_html(samples, reducing_target),
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
    let headers = [
        "时间",
        "实验时长",
        "温度(℃)",
        "质量(g)",
        "失重(g)",
        "CO",
        "CO2",
        "N2",
        "H2",
        "状态",
    ];
    for (c, h) in headers.iter().enumerate() {
        data.write_string_with_format(0, c as u16, *h, &bold)?;
    }
    for (i, r) in rows.iter().enumerate() {
        let row = (i + 1) as u32;
        data.write_string(row, 0, &r.timestamp)?;
        data.write_string(row, 1, r.experiment_duration.as_deref().unwrap_or(""))?;
        data.write_number(row, 2, r.temperature)?;
        data.write_number(row, 3, r.weight)?;
        data.write_number(row, 4, r.weight_loss)?;
        data.write_number(row, 5, r.co_flow)?;
        data.write_number(row, 6, r.co2_flow)?;
        data.write_number(row, 7, r.n2_flow)?;
        data.write_number(row, 8, r.h2_flow)?;
        data.write_string(row, 9, r.experiment_status.as_deref().unwrap_or(""))?;
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
        assert!(html.contains("<polyline"));
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
        assert!(html.contains("还原段温度均值"));
        assert!(html.contains("合格"));
    }

    #[test]
    fn xlsx_produces_zip_bytes() {
        let bytes = samples_to_xlsx(&exp(), &[sample(900.0, 88.0)]).unwrap();
        assert_eq!(&bytes[0..2], b"PK"); // XLSX is a zip archive
        assert!(bytes.len() > 1000);
    }
}
