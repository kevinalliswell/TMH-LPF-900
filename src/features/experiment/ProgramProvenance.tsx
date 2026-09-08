import type { ProgramDto, StageDto } from "../../ipc/types";
import { conditionChanges, standardParent } from "../../ipc/programRules";
import { gasSummary } from "./programUtils";

const labels: Record<keyof StageDto, string> = {
  name: "阶段名称",
  description: "说明",
  kind: "类型",
  target_temp: "温度 (℃)",
  tolerance: "容差 (℃)",
  duration_min: "时长 (min)",
  ramp_rate: "升温率 (℃/min)",
  gas: "气体 (L/min)",
};
function display(value: string | number | number[]) {
  return Array.isArray(value) ? gasSummary(value) : String(value);
}
export function ProgramProvenance({
  program,
  draft = false,
}: {
  program: ProgramDto;
  draft?: boolean;
}) {
  if (program.source !== "custom") return null;
  const p = program.provenance;
  const parent = p?.parent_template ?? standardParent(program);
  const changes =
    draft && parent ? conditionChanges(program, parent) : (p?.changes ?? []);
  return (
    <div
      className="rounded p-3 text-[12px]"
      style={{
        border: "1px solid var(--border)",
        background: "var(--panel-2)",
      }}
    >
      <div className="font-medium">
        {draft ? "待保存的条件差异" : "自定义程序依据"}
        {p ? ` · ${draft ? "基于" : ""}版本 ${p.revision}` : " · 尚无保存版本"}
      </div>
      <p className="mt-1" style={{ color: "var(--fg-dim)" }}>
        基础：{parent?.name ?? "未记录"} /{" "}
        {program.template_revision ?? "模板版本未记录"}
      </p>
      {p && (
        <p className="mt-1">
          {draft ? "上次保存" : "保存"}：{p.edited_by} · {p.edited_at}
          <br />
          修改原因：{p.reason}
        </p>
      )}
      {!p && program.id && (
        <p className="mt-1" style={{ color: "var(--warn)" }}>
          旧程序缺少版本依据，请从当前国标复制后使用；原记录保留。
        </p>
      )}
      <p className="my-2" style={{ color: "var(--fg-dim)" }}>
        自定义结果用于研究比较；继承国标模板不表示本次试验符合国标方法。
      </p>
      {changes.length ? (
        <div className="overflow-auto">
          <table className="w-full text-left">
            <thead>
              <tr>
                <th className="p-1">阶段 / 条件</th>
                <th className="p-1">基础国标</th>
                <th className="p-1">自定义</th>
              </tr>
            </thead>
            <tbody>
              {changes.map((c) => (
                <tr
                  key={`${c.stage_index}-${c.field}`}
                  style={{ borderTop: "1px solid var(--border)" }}
                >
                  <td className="p-1">
                    {c.stage_index + 1} · {labels[c.field]}
                  </td>
                  <td className="p-1">{display(c.before)}</td>
                  <td className="p-1">{display(c.after)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : (
        <p>{p || draft ? "实验条件与基础模板一致。" : "未记录条件差异。"}</p>
      )}
    </div>
  );
}
