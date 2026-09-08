import type { ProgramDto } from "./types";
export function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;");
}
const row = (label: string, value: string) =>
  `<tr><th>${escapeHtml(label)}</th><td>${escapeHtml(value)}</td></tr>`;
export function programSnapshotHtml(program?: ProgramDto | null): string {
  if (!program) return "<p>未记录执行程序快照。</p>";
  let html = `<h2>本次执行程序</h2><table>${row("程序", program.name)}${row("模板版本", program.template_revision ?? "未记录")}</table>`;
  const p = program.provenance;
  if (program.source === "custom") {
    html += "<p>自定义结果用于研究比较，不能据此声明符合国标方法。</p>";
    if (p) {
      html += `<table>${row("自定义版本", String(p.revision))}${row("基础国标", p.parent_template.name)}${row("保存人", p.edited_by)}${row("保存时间", p.edited_at)}${row("修改原因", p.reason)}</table>`;
      const labels: Record<string, string> = {
        target_temp: "温度 (℃)",
        duration_min: "时长 (min)",
        tolerance: "容差 (℃)",
        ramp_rate: "升温率 (℃/min)",
        gas: "气体 [H₂,N₂,CO₂,CO] (L/min)",
        name: "阶段名称",
        description: "说明",
        kind: "类型",
      };
      html += `<table><tr><th>阶段 / 条件</th><th>基础国标条件</th><th>自定义条件</th></tr>${p.changes.map((c) => `<tr><td>${c.stage_index + 1} / ${escapeHtml(labels[c.field] ?? c.field)}</td><td>${escapeHtml(String(c.before))}</td><td>${escapeHtml(String(c.after))}</td></tr>`).join("")}</table>`;
    } else html += "<p>旧程序未记录版本依据与编辑者。</p>";
  }
  return html;
}
