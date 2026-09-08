import { useEffect, useState } from "react";
import { getExperimentDetail } from "../../ipc";
import type { ProgramDto } from "../../ipc/types";
import { useExperiment } from "../../stores/experiment";
import { fmtHms } from "./ExperimentBar";

/** Horizontal stage stepper shown while an experiment runs. */
export function StageProgress() {
  const ex = useExperiment((s) => s.state);
  const [program, setProgram] = useState<ProgramDto | null>(null);
  useEffect(() => {
    let cancelled = false;
    setProgram(null);
    if (ex.experiment_id)
      getExperimentDetail(ex.experiment_id)
        .then((detail) => {
          if (!cancelled) setProgram(detail.program_snapshot ?? null);
        })
        .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [ex.experiment_id]);

  if (!ex.running) return null;
  const stages = program?.stages ?? [];
  const cur = ex.stage_index ?? 0;

  return (
    <div className="panel p-3 shrink-0">
      <div className="flex items-center justify-between mb-3">
        <div className="text-[13px] font-medium">实验进度</div>
        <div className="mono text-[12px]" style={{ color: "var(--fg-faint)" }}>
          总时长 {fmtHms(ex.total_elapsed_s)}
        </div>
      </div>
      <div className="flex items-stretch gap-1">
        {stages.map((s, i) => {
          const done = i < cur;
          const active = i === cur;
          return (
            <div
              key={s.name}
              className="flex-1 flex flex-col items-center gap-1.5"
            >
              <div
                className="w-full rounded"
                style={{
                  height: 6,
                  background: done
                    ? "var(--ok)"
                    : active
                      ? "var(--heat)"
                      : "var(--panel-2)",
                  boxShadow: active ? "0 0 8px var(--heat)" : "none",
                }}
              />
              <div
                className="text-[11px]"
                style={{
                  color: active
                    ? "var(--heat)"
                    : done
                      ? "var(--ok)"
                      : "var(--fg-faint)",
                }}
              >
                {s.name}
              </div>
              <div
                className="mono text-[10px]"
                style={{ color: "var(--fg-faint)" }}
              >
                {s.kind === "MANUAL" ? "人工操作" : `${s.target_temp}℃`}
              </div>
            </div>
          );
        })}
      </div>
      {ex.stage_name && (
        <div
          className="mt-3 flex gap-5 text-[12px]"
          style={{ color: "var(--fg-dim)" }}
        >
          <span>
            当前阶段
            <span style={{ color: "var(--heat)" }}> {ex.stage_name}</span>
          </span>
          {ex.stage_kind !== "MANUAL" && (
            <span className="mono">
              {ex.stage_kind === "COOLING"
                ? `低于 ${ex.target_temp}℃`
                : `目标 ${ex.target_temp}℃ ±${ex.tolerance}`}
            </span>
          )}
          <span className="mono">阶段时长 {fmtHms(ex.stage_elapsed_s)}</span>
          {ex.stage_kind === "STABILIZING" && (
            <span className="mono">
              连续合温 {fmtHms(ex.stable_elapsed_s)} /{" "}
              {stages[cur]?.duration_min ?? 0} min
            </span>
          )}
          {ex.stage_kind === "MANUAL" && (
            <span>{stages[cur]?.description}</span>
          )}
          {ex.phase === "STAGE_TRANSITION" && <span>正在确认阶段气氛…</span>}
        </div>
      )}
    </div>
  );
}
