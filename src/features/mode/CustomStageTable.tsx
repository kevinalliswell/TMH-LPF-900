import type { GasLimits, ProgramDto, StageDto } from "../../ipc/types";
import {
  GAS_LABELS,
  STAGE_KIND_LABEL,
  gasSummary,
  stageCondition,
} from "../experiment/programUtils";

const numbers = [
  ["target_temp", "温度 (℃)"],
  ["tolerance", "容差 (℃)"],
  ["duration_min", "时间 (min)"],
  ["ramp_rate", "升温率 (℃/min)"],
] as const;
const inputStyle = {
  background: "var(--bg)",
  border: "1px solid var(--border)",
  color: "var(--fg)",
};

export function CustomStageTable({
  program,
  editable,
  limits,
  onStage,
  onGas,
}: {
  program: ProgramDto;
  editable: boolean;
  limits: GasLimits | null;
  onStage: (index: number, patch: Partial<StageDto>) => void;
  onGas: (stage: number, gas: number, value: number) => void;
}) {
  return (
    <div className="overflow-auto">
      <p className="my-2 text-[12px]" style={{ color: "var(--fg-dim)" }}>
        温控程序由仪表执行；此处温度制度用于实验条件记录和读值判定。自定义可调整前三阶段，结束步骤按基础国标保留。
      </p>
      <table className="w-full min-w-[1000px] border-collapse text-[12px]">
        <thead>
          <tr className="text-left" style={{ color: "var(--fg-faint)" }}>
            <th className="p-2">阶段</th>
            <th className="p-2">类型 / 条件</th>
            {numbers.map(([key, label]) => (
              <th className="p-2" key={key}>
                {label}
              </th>
            ))}
            <th className="p-2">气体组成 (L/min)</th>
          </tr>
        </thead>
        <tbody>
          {program.stages.map((stage, i) => {
            const canEdit = editable && i < 3;
            return (
              <tr key={i} style={{ borderTop: "1px solid var(--border)" }}>
                <td className="p-2 min-w-[140px]">
                  <div className="mb-1" style={{ color: "var(--fg-faint)" }}>
                    {i + 1}
                    {i >= 3 ? " · 结束步骤只读" : ""}
                  </div>
                  {canEdit ? (
                    <input
                      aria-label={`第 ${i + 1} 阶段名称`}
                      value={stage.name}
                      onChange={(e) => onStage(i, { name: e.target.value })}
                      className="w-full rounded p-1.5"
                      style={inputStyle}
                    />
                  ) : (
                    stage.name
                  )}
                </td>
                <td className="p-2 min-w-[200px]">
                  <div>{STAGE_KIND_LABEL[stage.kind]}</div>
                  <div
                    className="mt-1 text-[11px]"
                    style={{ color: "var(--fg-dim)" }}
                  >
                    {stageCondition(stage)}
                  </div>
                  {canEdit && (
                    <input
                      aria-label={`第 ${i + 1} 阶段说明`}
                      value={stage.description}
                      onChange={(e) =>
                        onStage(i, { description: e.target.value })
                      }
                      className="mt-1 w-full rounded p-1.5"
                      style={inputStyle}
                    />
                  )}
                </td>
                {numbers.map(([key, label]) => {
                  const enabled =
                    canEdit &&
                    (key !== "duration_min" || stage.kind !== "HEATING") &&
                    (key !== "ramp_rate" || stage.kind === "HEATING");
                  return (
                    <td className="p-2 min-w-[90px]" key={key}>
                      {enabled ? (
                        <input
                          type="number"
                          step="0.1"
                          aria-label={`第 ${i + 1} 阶段${label}`}
                          value={stage[key]}
                          onChange={(e) =>
                            onStage(i, { [key]: Number(e.target.value) })
                          }
                          className="mono w-20 rounded p-1.5 text-right"
                          style={inputStyle}
                        />
                      ) : (
                        <span className="mono">
                          {stage.kind === "MANUAL" ||
                          (key === "tolerance" &&
                            ["COOLING", "PURGING"].includes(stage.kind))
                            ? "—"
                            : stage[key]}
                        </span>
                      )}
                    </td>
                  );
                })}
                <td className="p-2 min-w-[240px]">
                  {canEdit ? (
                    <div className="grid grid-cols-2 gap-2">
                      {GAS_LABELS.map((label, g) => (
                        <label className="flex items-center gap-1" key={label}>
                          <span className="w-7">{label}</span>
                          <input
                            type="number"
                            min="0"
                            step={limits?.flow_steps?.[g] ?? "any"}
                            aria-label={`第 ${i + 1} 阶段${label}流量`}
                            value={stage.gas[g] ?? 0}
                            onChange={(e) =>
                              onGas(i, g, Number(e.target.value))
                            }
                            className="mono w-16 rounded p-1 text-right"
                            style={inputStyle}
                          />
                        </label>
                      ))}
                    </div>
                  ) : (
                    <span className="mono" style={{ color: "var(--gas)" }}>
                      {gasSummary(stage.gas)}
                    </span>
                  )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
