//! Standalone, printable group reports built only from a saved immutable revision.
use crate::storage::TestGroupRevision;
use crate::test_group::GroupSnapshot;
use serde_json::Value;

fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
fn row(label: &str, value: &str) -> String {
    format!(
        "<tr><th>{}</th><td>{}</td></tr>",
        esc(label),
        esc(if value.trim().is_empty() {
            "待补录"
        } else {
            value
        })
    )
}
pub fn render(record: &TestGroupRevision) -> Result<String, String> {
    let snapshot: GroupSnapshot =
        serde_json::from_value(record.snapshot.clone()).map_err(|_| "检验组快照无法读取")?;
    if snapshot.schema_version != 1 {
        return Err("检验组快照版本不受支持".into());
    }
    let evaluation = &snapshot.evaluation;
    let selected = evaluation
        .decision
        .selected_indices
        .iter()
        .map(|i| {
            snapshot
                .valid_trial_indices
                .get(*i)
                .copied()
                .ok_or("采用试验位置无效")
        })
        .collect::<Result<Vec<_>, _>>()?;
    if snapshot.valid_trial_indices.iter().any(|i| {
        snapshot
            .trials
            .get(*i)
            .map_or(true, |t| t.exclusion_reason.is_some())
    }) {
        return Err("检验组有效试验映射无效".into());
    }
    let method = match snapshot.policy.method.as_str() {
        "REDUCIBILITY" => "GB/T 13241-2017",
        "LOW_TEMP_DEGRADATION" => "GB/T 13242-2017",
        "FREE_SWELLING" => "GB/T 13240-2018",
        _ => return Err("检验组方法未知".into()),
    };
    let source = if snapshot.data_source == "real" {
        "真实设备记录，仍需过程与实验室复核"
    } else {
        "模拟 / 回放数据，仅用于软件测试"
    };
    let conformity = if snapshot.policy.custom {
        "自定义条件，不能据此声明符合国标方法"
    } else {
        "尚未判定：需全过程、测点映射、校准与外部操作证据复核"
    };
    let material = match snapshot.policy.material {
        crate::repeat_group::MaterialKind::Sinter => "烧结矿",
        crate::repeat_group::MaterialKind::Pellet => "球团矿",
        crate::repeat_group::MaterialKind::Natural => "天然块矿",
    };
    let mut metadata = String::new();
    for (key, value) in [
        ("检验组名称", snapshot.name.as_str()),
        ("批次 / 试样标识", &snapshot.batch_id),
        ("检验组编号", &record.group_id),
        ("版本", &record.revision.to_string()),
        ("版本保存时间", &record.created_at),
        ("实际录入者", &snapshot.recorded_by),
        ("保存原因", &snapshot.reason),
        ("方法依据", method),
        ("材料", material),
        ("实验室", &snapshot.report_details.lab_name),
        ("实验室地址", &snapshot.report_details.lab_address),
        ("报告编号", &snapshot.report_details.report_number),
        ("报告日期", &snapshot.report_details.report_date),
        (
            "复核 / 签署人（人工填写）",
            &snapshot.report_details.reviewer,
        ),
        ("偏离及处理说明", &snapshot.report_details.deviations),
    ] {
        metadata.push_str(&row(key, value));
    }
    let mut results = String::new();
    for (key, value) in &evaluation.reported {
        let unit = if key == "RVI" { "%/min" } else { "%" };
        results.push_str(&row(
            key,
            &value
                .as_ref()
                .map(|v| format!("{v} {unit}"))
                .unwrap_or_else(|| "缺少采用试验的参考指标，不以零或其他试验替代".into()),
        ));
    }
    if results.is_empty() {
        results = row("最终组结果", "待补测，尚无最终数值");
    }
    let limits = &evaluation.limits;
    let criteria = format!(
        "A={}、B={}、C={} 个百分点；{}",
        limits.a, limits.b, limits.c, limits.basis
    );
    let mut trials = String::new();
    let primary_key = match snapshot.policy.method.as_str() {
        "REDUCIBILITY" => {
            if snapshot.policy.custom {
                "rt"
            } else {
                "ri"
            }
        }
        "LOW_TEMP_DEGRADATION" => "rdi_plus_3_15",
        _ => "swelling_index",
    };
    for (i, trial) in snapshot.trials.iter().enumerate() {
        let experiment = &trial.experiment;
        let analysis: Option<Value> = experiment
            .analysis_results_json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok());
        let raw = analysis
            .as_ref()
            .and_then(|v| v[primary_key].as_f64())
            .map(|v| v.to_string())
            .unwrap_or_else(|| "缺失".into());
        let decision_value = snapshot
            .valid_trial_indices
            .iter()
            .position(|v| *v == i)
            .and_then(|p| evaluation.decision_values.get(p))
            .map(|v| v.to_string())
            .unwrap_or_else(|| "—".into());
        let status = if let Some(reason) = &trial.exclusion_reason {
            format!("人工排除：{reason}")
        } else if selected.is_empty() {
            "待补测".into()
        } else if selected.contains(&i) {
            "采用".into()
        } else {
            "按重复性规则去极值".into()
        };
        trials.push_str("<tr>");
        for value in [
            (i + 1).to_string(),
            experiment.sample_id.clone().unwrap_or_default(),
            experiment.experiment_id.clone(),
            experiment.start_time.clone(),
            trial
                .analysis_revision_id
                .map(|v| v.to_string())
                .unwrap_or_else(|| "未记录".into()),
            raw,
            decision_value,
            status,
        ] {
            trials.push_str(&format!("<td>{}</td>", esc(&value)));
        }
        trials.push_str("</tr>");
    }
    let evidence = serde_json::to_string_pretty(record).map_err(|e| e.to_string())?;
    let html = format!(
        r#"<!doctype html><html lang="zh-CN"><meta charset="utf-8"><title>检验组报告 — {title}</title>
<style>
body{{font-family:"Noto Sans CJK SC","Microsoft YaHei",sans-serif;color:#182532;line-height:1.65;max-width:1050px;margin:36px auto;padding:0 24px}}h1{{font-size:26px;margin-bottom:8px}}h2{{font-size:18px;margin-top:28px;border-bottom:2px solid #1e6370;padding-bottom:6px}}.notice{{padding:12px 16px;background:#eef5f6;border-left:4px solid #1e6370}}table{{width:100%;border-collapse:collapse;margin:12px 0}}th,td{{padding:8px;border:1px solid #cbd5db;text-align:left;vertical-align:top;overflow-wrap:anywhere}}th{{background:#f2f5f7;font-weight:600}}.kv th{{width:240px}}.trials{{font-size:12px}}pre{{white-space:pre-wrap;overflow-wrap:anywhere;font-size:11px}}summary,button{{cursor:pointer}}button{{padding:8px 18px;border:1px solid #1e6370;background:white;border-radius:4px}}.muted{{font-size:12px;color:#526370}}@media print{{@page{{size:A4 landscape;margin:14mm}}body{{margin:0;padding:0;max-width:none;font-size:11px}}button{{display:none}}thead{{display:table-header-group}}tr{{break-inside:avoid}}h2{{break-after:avoid}}}}
</style><body><button onclick="window.print()">打印 / 保存为 PDF</button><h1>检验组报告</h1>
<p class="notice">{source}</p><table class="kv">{metadata}</table>
<h2>结果与重复性</h2><table class="kv">{results}{criteria_row}{decision_row}{formula_row}</table>
<p>同批与独立性由录入者确认。低温粉化按一位小数单次结果参与重复性；还原性与自由膨胀保留单次计算精度至最终取值。报出值采用明确的 GB/T 8170 修约规则。</p>
<h2>采用与排除记录</h2><table class="trials"><thead><tr><th>顺序</th><th>样品编号</th><th>试验编号</th><th>开始时间</th><th>分析版本</th><th>单次计算值</th><th>判定用值</th><th>采用 / 排除依据</th></tr></thead><tbody>{trials}</tbody></table>
<h2>结论适用范围</h2><table class="kv">{conformity_row}{purchase_row}</table>
<p class="muted">已采用记录通过软件的正常结束、完整落盘与组内制度一致性检查；这不代替全过程国标符合性复核。人工签署人姓名不等同于电子签名。自定义指标只供相同研究条件下比较。</p>
<details><summary>查看本版本完整证据（单次输入、程序、运行和报告补录记录）</summary><pre>{evidence}</pre></details>
<p class="muted">本报告读取保存时的组快照，后续单次重新分析或删除不会改变本版本。需要打印完整证据时，请先展开证据部分。</p></body></html>"#,
        title = esc(&snapshot.name),
        source = esc(source),
        metadata = metadata,
        results = results,
        trials = trials,
        evidence = esc(&evidence),
        criteria_row = row("重复性限值与依据", &criteria),
        decision_row = row("重复性决策", &evaluation.explanation),
        formula_row = row(
            "计算 / 修约版本",
            &format!(
                "{} / {}",
                evaluation.calculation_version, evaluation.rounding_version
            )
        ),
        conformity_row = row("完整方法符合性", conformity),
        purchase_row = row(
            "材料采购性能合格性",
            "尚未判定：本软件未采用采购合同性能限值"
        )
    );
    Ok(html)
}
