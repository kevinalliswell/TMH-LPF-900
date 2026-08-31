import { useEffect, useState } from "react";
import { EXPERIMENT_MODES, listPrograms, skipStage, stopExperiment } from "../../ipc";
import type { ProgramDto } from "../../ipc/types";
import { Modal } from "../../components/Modal";
import { useExperiment } from "../../stores/experiment";
import { toastError, useToasts } from "../../stores/toast";
import { StartExperimentDialog } from "../experiment/StartExperimentDialog";
import { gasSummary, stageCondition } from "../experiment/programUtils";

/** Format an elapsed second count as HH:MM:SS. */
export function fmtHms(totalS: number | undefined): string {
  const s = Math.max(0, Math.floor(totalS ?? 0));
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(Math.floor(s / 3600))}:${p(Math.floor((s % 3600) / 60))}:${p(s % 60)}`;
}

export function ExperimentBar() {
  const ex = useExperiment((s) => s.state);
  const [showDialog, setShowDialog] = useState(false);
  const [confirmStop, setConfirmStop] = useState(false);
  const [confirmSkip, setConfirmSkip] = useState(false);
  const [skipReason, setSkipReason] = useState("");
  const [programs, setPrograms] = useState<ProgramDto[]>([]);
  const push = useToasts((s) => s.push);

  useEffect(() => {
    listPrograms()
      .then(setPrograms)
      .catch(() => {});
  }, []);

  const modeName = EXPERIMENT_MODES.find((m) => m.id === ex.mode)?.name ?? ex.mode ?? "";
  const stageNo = (ex.stage_index ?? 0) + 1;
  const program = programs.find((p) => p.id === ex.mode);
  const curStage = program?.stages[ex.stage_index ?? 0];
  const nextStage = program?.stages[(ex.stage_index ?? 0) + 1];

  const doStop = async () => {
    try {
      await stopExperiment("manual");
      push("实验已停止,已切换保护气氛（N₂ 5.0 · 可燃气 0）", "warn");
    } catch (e) {
      toastError(e);
    } finally {
      setConfirmStop(false);
    }
  };

  const doSkip = async () => {
    try {
      await skipStage(skipReason);
      push("已跳至下一阶段", "info");
      setSkipReason("");
      setConfirmSkip(false);
    } catch (e) {
      toastError(e);
    }
  };

  return (
    <>
      <div
        className="flex items-center gap-3 px-4 h-9 border-b text-[12px]"
        style={{
          borderColor: "var(--border)",
          background: ex.running ? "rgba(63,185,80,0.06)" : "transparent",
        }}
      >
        {ex.running ? (
          <>
            <span className="flex items-center gap-1.5">
              <span
                style={{ width: 7, height: 7, borderRadius: "50%", background: "var(--ok)", boxShadow: "0 0 6px var(--ok)" }}
              />
              <span style={{ color: "var(--ok)" }}>实验运行中</span>
            </span>
            <span style={{ color: "var(--fg-dim)" }}>{modeName}</span>
            <span className="mono" style={{ color: "var(--fg-faint)" }}>
              {ex.sample_name}
            </span>
            {ex.stage_name && (
              <span
                className="px-2 py-0.5 rounded"
                style={{ background: "var(--panel-2)", color: "var(--heat)" }}
              >
                阶段 {stageNo}/{ex.total_stages ?? "-"} · {ex.stage_name} → {ex.target_temp ?? 0}℃
              </span>
            )}
            <span className="mono" style={{ color: "var(--fg-faint)" }}>
              时长 {fmtHms(ex.total_elapsed_s)}
            </span>
            <div className="flex-1" />
            <button
              onClick={() => setConfirmSkip(true)}
              className="px-2.5 py-1 rounded text-[12px]"
              style={{ background: "var(--panel-2)", border: "1px solid var(--border-bright)", color: "var(--fg)" }}
            >
              跳过阶段
            </button>
            <button
              onClick={() => setConfirmStop(true)}
              className="px-2.5 py-1 rounded text-[12px]"
              style={{ background: "var(--danger)", color: "#fff" }}
            >
              停止实验
            </button>
          </>
        ) : (
          <>
            <span style={{ color: "var(--fg-faint)" }}>空闲 · 未开始实验（手动气氛可控）</span>
            <div className="flex-1" />
            <button
              onClick={() => setShowDialog(true)}
              className="px-2.5 py-1 rounded text-[12px] font-medium"
              style={{ background: "var(--ok)", color: "#04140a" }}
            >
              开始实验
            </button>
          </>
        )}
      </div>

      {showDialog && <StartExperimentDialog onClose={() => setShowDialog(false)} />}
      {confirmSkip && (
        <Modal title="跳过阶段" onClose={() => setConfirmSkip(false)}>
          <div className="flex flex-col gap-3 text-[13px]">
            <div style={{ color: "var(--fg-dim)" }}>
              确认跳过当前阶段?该操作会写入审计，并在下一轮控制周期下发下一阶段气氛。
            </div>
            <div className="rounded p-2.5" style={{ background: "var(--bg)", border: "1px solid var(--border)" }}>
              <div className="text-[12px]" style={{ color: "var(--fg-faint)" }}>
                当前阶段
              </div>
              <div className="mt-1">
                {curStage ? `${curStage.name} · ${stageCondition(curStage)}` : ex.stage_name ?? "—"}
              </div>
              <div className="text-[12px] mt-2" style={{ color: "var(--fg-faint)" }}>
                下一阶段
              </div>
              <div className="mt-1">
                {nextStage
                  ? `${nextStage.name} · ${stageCondition(nextStage)} · ${gasSummary(nextStage.gas)}`
                  : "完成实验并切换保护气氛（N₂ 5.0，H₂/CO₂/CO 0）"}
              </div>
            </div>
            <label className="flex flex-col gap-1 text-[12px]" style={{ color: "var(--fg-dim)" }}>
              跳段原因 *
              <textarea
                value={skipReason}
                onChange={(e) => setSkipReason(e.target.value)}
                className="px-2 py-1.5 rounded text-[13px] min-h-20"
                style={{ background: "var(--bg)", border: "1px solid var(--border)", color: "var(--fg)" }}
              />
            </label>
          </div>
          <div className="flex justify-end gap-2 mt-4">
            <button
              onClick={() => setConfirmSkip(false)}
              className="px-3 py-1.5 rounded text-[13px]"
              style={{ background: "var(--panel-2)", border: "1px solid var(--border-bright)", color: "var(--fg)" }}
            >
              取消
            </button>
            <button
              onClick={doSkip}
              disabled={!skipReason.trim()}
              className="px-3 py-1.5 rounded text-[13px]"
              style={{ background: "var(--warn)", color: "#1a1204", opacity: skipReason.trim() ? 1 : 0.6 }}
            >
              确认跳过
            </button>
          </div>
        </Modal>
      )}
      {confirmStop && (
        <Modal title="停止实验" onClose={() => setConfirmStop(false)}>
          <div className="text-[13px] mb-4" style={{ color: "var(--fg-dim)" }}>
            确认停止当前实验?系统将立即切换到保护气氛（N₂ 5.0 L/min,可燃气归零）。
          </div>
          <div className="flex justify-end gap-2">
            <button
              onClick={() => setConfirmStop(false)}
              className="px-3 py-1.5 rounded text-[13px]"
              style={{ background: "var(--panel-2)", border: "1px solid var(--border-bright)", color: "var(--fg)" }}
            >
              取消
            </button>
            <button
              onClick={doStop}
              className="px-3 py-1.5 rounded text-[13px]"
              style={{ background: "var(--danger)", color: "#fff" }}
            >
              确认停止
            </button>
          </div>
        </Modal>
      )}
    </>
  );
}
