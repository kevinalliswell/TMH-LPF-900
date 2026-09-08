import type { ProgramDto } from "../../ipc/types";
import {
  STAGE_KIND_COLOR,
  STAGE_KIND_LABEL,
  durationSummary,
  formatNumber,
  gasSummary,
  stageCondition,
} from "./programUtils";

export function ProgramPreview({
  program,
  compact = false,
}: {
  program: ProgramDto;
  compact?: boolean;
}) {
  return (
    <div
      className={
        compact ? "flex flex-col gap-2" : "panel p-3 flex flex-col gap-3"
      }
    >
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="text-[13px] font-semibold truncate">
            {program.name}
          </div>
          <div
            className="text-[11px] mt-0.5"
            style={{ color: "var(--fg-faint)" }}
          >
            {program.source === "custom" ? "自定义模式" : "标准模式"} /{" "}
            {program.stages.length} 阶段 / 前缀{" "}
            <span className="mono">{program.sample_prefix}</span>
            {!program.enabled && (
              <span style={{ color: "var(--danger)" }}> / 已停用</span>
            )}
          </div>
          {program.description && (
            <div
              className="text-[12px] mt-1 line-clamp-2"
              style={{ color: "var(--fg-dim)" }}
            >
              {program.description}
            </div>
          )}
        </div>
        <div
          className="mono text-[11px] text-right shrink-0"
          style={{ color: "var(--fg-dim)" }}
        >
          {durationSummary(program)}
        </div>
      </div>

      <div className="overflow-x-auto">
        <table className="w-full text-[12px] border-collapse">
          <thead>
            <tr style={{ color: "var(--fg-faint)" }} className="text-left">
              <th className="py-1.5 pr-3 font-medium">阶段</th>
              <th className="py-1.5 pr-3 font-medium">类型</th>
              <th className="py-1.5 pr-3 font-medium text-right">目标</th>
              <th className="py-1.5 pr-3 font-medium text-right">容差</th>
              <th className="py-1.5 pr-3 font-medium text-right">时长</th>
              <th className="py-1.5 pr-3 font-medium">气氛</th>
              <th className="py-1.5 font-medium">判定 / 说明</th>
            </tr>
          </thead>
          <tbody>
            {program.stages.map((s, i) => (
              <tr
                key={`${s.name}-${i}`}
                style={{ borderTop: "1px solid var(--border)" }}
              >
                <td className="py-1.5 pr-3 whitespace-nowrap">
                  <span className="mono" style={{ color: "var(--fg-faint)" }}>
                    {i + 1}
                  </span>{" "}
                  {s.name}
                </td>
                <td className="py-1.5 pr-3 whitespace-nowrap">
                  <span
                    className="px-1.5 py-0.5 rounded text-[11px]"
                    style={{
                      background: "var(--panel-2)",
                      color: STAGE_KIND_COLOR[s.kind],
                    }}
                  >
                    {STAGE_KIND_LABEL[s.kind]}
                  </span>
                </td>
                <td className="py-1.5 pr-3 mono text-right whitespace-nowrap">
                  {s.kind === "MANUAL"
                    ? "—"
                    : `${formatNumber(s.target_temp)}℃`}
                </td>
                <td
                  className="py-1.5 pr-3 mono text-right whitespace-nowrap"
                  style={{ color: "var(--fg-faint)" }}
                >
                  {["MANUAL", "COOLING"].includes(s.kind)
                    ? "—"
                    : `±${formatNumber(s.tolerance)}`}
                </td>
                <td className="py-1.5 pr-3 mono text-right whitespace-nowrap">
                  {s.kind === "MANUAL"
                    ? "待确认"
                    : s.duration_min > 0
                      ? `${formatNumber(s.duration_min)} min`
                      : "动态"}
                </td>
                <td
                  className="py-1.5 pr-3 mono whitespace-nowrap"
                  style={{ color: "var(--gas)" }}
                >
                  {gasSummary(s.gas)}
                </td>
                <td
                  className="py-1.5 min-w-[180px]"
                  style={{ color: "var(--fg-dim)" }}
                >
                  {stageCondition(s)}
                  {s.description && s.description !== stageCondition(s)
                    ? ` / ${s.description}`
                    : ""}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
