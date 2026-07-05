import { useState } from "react";
import { EXPERIMENT_MODES, skipStage, stopExperiment } from "../../ipc";
import { Modal } from "../../components/Modal";
import { useExperiment } from "../../stores/experiment";
import { toastError, useToasts } from "../../stores/toast";
import { StartExperimentDialog } from "../experiment/StartExperimentDialog";

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
  const push = useToasts((s) => s.push);

  const modeName = EXPERIMENT_MODES.find((m) => m.id === ex.mode)?.name ?? ex.mode ?? "";
  const stageNo = (ex.stage_index ?? 0) + 1;

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
      await skipStage();
      push("已跳至下一阶段", "info");
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
              onClick={doSkip}
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
