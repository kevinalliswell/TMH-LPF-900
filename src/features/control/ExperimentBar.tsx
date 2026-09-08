import { useTelemetry } from "../../stores/telemetry";
import { useEffect, useState } from "react";
import {
  EXPERIMENT_MODES,
  resetSafetyInterlock,
  retryPendingSave,
  getExperimentDetail,
  skipStage,
  acknowledgeStage,
  stopExperiment,
} from "../../ipc";
import type { ProgramDto, StageDto } from "../../ipc/types";
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
  const latched =
    useTelemetry((s) => s.snapshot?.health.safety_latched) ||
    ex.phase === "ERROR";
  const [showDialog, setShowDialog] = useState(false);
  const [retrying, setRetrying] = useState(false);
  const [confirmStop, setConfirmStop] = useState(false);
  const [confirmSkip, setConfirmSkip] = useState(false);
  const [actionTarget, setActionTarget] = useState<{
    id: string;
    index: number;
    stage: StageDto;
  } | null>(null);
  const [confirmManual, setConfirmManual] = useState(false);
  const [manualNotes, setManualNotes] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [skipReason, setSkipReason] = useState("");
  const [program, setProgram] = useState<ProgramDto | null>(null);
  const push = useToasts((s) => s.push);

  useEffect(() => {
    let cancelled = false;
    setProgram(null);
    if (ex.experiment_id)
      getExperimentDetail(ex.experiment_id)
        .then((detail) => {
          if (!cancelled) setProgram(detail.program_snapshot ?? null);
        })
        .catch(toastError);
    return () => {
      cancelled = true;
    };
  }, [ex.experiment_id]);

  const modeName =
    EXPERIMENT_MODES.find((m) => m.id === ex.mode)?.name ?? ex.mode ?? "";
  const stageNo = (ex.stage_index ?? 0) + 1;
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

  const beginAction = (manual: boolean) => {
    if (!curStage || !ex.experiment_id) return;
    setActionTarget({
      id: ex.experiment_id,
      index: ex.stage_index ?? 0,
      stage: curStage,
    });
    if (manual) {
      setManualNotes("");
      setConfirmManual(true);
    } else {
      setSkipReason("");
      setConfirmSkip(true);
    }
  };
  const doManual = async () => {
    if (!actionTarget) return;
    setSubmitting(true);
    try {
      await acknowledgeStage(actionTarget.id, actionTarget.index, manualNotes);
      push("人工确认已提交，执行结果以当前流程为准", "info");
      setConfirmManual(false);
    } catch (e) {
      toastError(e);
    } finally {
      setSubmitting(false);
    }
  };
  const doSkip = async () => {
    try {
      if (!actionTarget) return;
      setSubmitting(true);
      await skipStage(actionTarget.id, actionTarget.index, skipReason);
      push("跳段请求已提交，执行结果以当前流程为准", "info");
      setSkipReason("");
      setConfirmSkip(false);
    } catch (e) {
      toastError(e);
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <>
      {ex.updating && (
        <div role="status" className="px-4 py-2 text-[13px]">
          正在安装更新，实验与设备修改暂不可用。
        </div>
      )}
      {ex.persistence_pending && (
        <div
          role="alert"
          className="flex items-center gap-3 px-4 py-2 text-[13px]"
          style={{ color: "var(--danger)", background: "var(--panel-2)" }}
        >
          <span>
            实验数据尚未保存。请保持应用打开，检查磁盘空间或写入权限后重试。保存完成前不能开始实验或安装更新。
          </span>
          <button
            disabled={retrying || ex.running}
            onClick={() => {
              setRetrying(true);
              void retryPendingSave()
                .then(() => push("保存已恢复，请确认保护状态后复位", "info"))
                .catch(toastError)
                .finally(() => setRetrying(false));
            }}
          >
            {retrying ? "保存中…" : "重试保存"}
          </button>
        </div>
      )}
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
                style={{
                  width: 7,
                  height: 7,
                  borderRadius: "50%",
                  background: "var(--ok)",
                  boxShadow: "0 0 6px var(--ok)",
                }}
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
                阶段 {stageNo}/{ex.total_stages ?? "-"} · {ex.stage_name}
                {ex.stage_kind === "MANUAL"
                  ? " · 等待人工确认"
                  : ` → ${ex.target_temp ?? 0}℃`}
              </span>
            )}
            <span className="mono" style={{ color: "var(--fg-faint)" }}>
              时长 {fmtHms(ex.total_elapsed_s)}
            </span>
            <div className="flex-1" />
            {ex.stage_kind === "MANUAL" ? (
              <button
                disabled={
                  !curStage ||
                  ex.can_control === false ||
                  ex.phase !== "RUNNING" ||
                  latched
                }
                onClick={() => beginAction(true)}
                className="px-2.5 py-1 rounded"
                style={{ background: "var(--warn)", color: "#1a1204" }}
              >
                确认人工步骤
              </button>
            ) : (
              ["HEATING", "STABILIZING", "REDUCING"].includes(
                ex.stage_kind ?? "",
              ) && (
                <button
                  disabled={
                    !curStage ||
                    ex.can_control === false ||
                    ex.phase !== "RUNNING" ||
                    latched
                  }
                  onClick={() => beginAction(false)}
                  className="px-2.5 py-1 rounded"
                  style={{ background: "var(--panel-2)", color: "var(--fg)" }}
                >
                  跳过阶段
                </button>
              )
            )}
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
            <span
              style={{ color: latched ? "var(--danger)" : "var(--fg-faint)" }}
            >
              {latched
                ? "保护已锁存：确认安全与保存状态后复位"
                : "空闲 · 未开始实验（手动气氛可控）"}
            </span>
            {latched && (
              <button
                disabled={!!ex.persistence_pending}
                onClick={() => {
                  void resetSafetyInterlock()
                    .then(() => push("保护已复位", "info"))
                    .catch(toastError);
                }}
              >
                确认安全并复位
              </button>
            )}
            <div className="flex-1" />
            <button
              disabled={!!latched || !!ex.persistence_pending || !!ex.updating}
              onClick={() => setShowDialog(true)}
              className="px-2.5 py-1 rounded text-[12px] font-medium"
              style={{ background: "var(--ok)", color: "#04140a" }}
            >
              开始实验
            </button>
          </>
        )}
      </div>

      {showDialog && (
        <StartExperimentDialog onClose={() => setShowDialog(false)} />
      )}
      {confirmSkip && (
        <Modal title="跳过阶段" onClose={() => setConfirmSkip(false)}>
          <div className="flex flex-col gap-3 text-[13px]">
            <div style={{ color: "var(--fg-dim)" }}>
              确认跳过当前阶段?该操作会写入审计，并在下一轮控制周期下发下一阶段气氛。
            </div>
            <div
              className="rounded p-2.5"
              style={{
                background: "var(--bg)",
                border: "1px solid var(--border)",
              }}
            >
              <div className="text-[12px]" style={{ color: "var(--fg-faint)" }}>
                当前阶段
              </div>
              <div className="mt-1">
                {curStage
                  ? `${curStage.name} · ${stageCondition(curStage)}`
                  : (ex.stage_name ?? "—")}
              </div>
              <div
                className="text-[12px] mt-2"
                style={{ color: "var(--fg-faint)" }}
              >
                下一阶段
              </div>
              <div className="mt-1">
                {nextStage
                  ? `${nextStage.name} · ${stageCondition(nextStage)} · ${gasSummary(nextStage.gas)}`
                  : program
                    ? "完成实验并保留程序规定的末阶段气氛"
                    : "程序快照未取得，暂不可跳段"}
              </div>
            </div>
            <label
              className="flex flex-col gap-1 text-[12px]"
              style={{ color: "var(--fg-dim)" }}
            >
              跳段原因 *
              <textarea
                value={skipReason}
                onChange={(e) => setSkipReason(e.target.value)}
                className="px-2 py-1.5 rounded text-[13px] min-h-20"
                style={{
                  background: "var(--bg)",
                  border: "1px solid var(--border)",
                  color: "var(--fg)",
                }}
              />
            </label>
          </div>
          <div className="flex justify-end gap-2 mt-4">
            <button
              onClick={() => setConfirmSkip(false)}
              className="px-3 py-1.5 rounded text-[13px]"
              style={{
                background: "var(--panel-2)",
                border: "1px solid var(--border-bright)",
                color: "var(--fg)",
              }}
            >
              取消
            </button>
            <button
              onClick={doSkip}
              disabled={
                submitting ||
                !skipReason.trim() ||
                !program ||
                ex.can_control === false
              }
              className="px-3 py-1.5 rounded text-[13px]"
              style={{
                background: "var(--warn)",
                color: "#1a1204",
                opacity: skipReason.trim() ? 1 : 0.6,
              }}
            >
              确认跳过
            </button>
          </div>
        </Modal>
      )}
      {confirmManual && actionTarget && (
        <Modal
          title={`人工确认 · ${actionTarget.stage.name}`}
          onClose={() => setConfirmManual(false)}
        >
          <div className="text-[13px] flex flex-col gap-3">
            <p>{actionTarget.stage.description}</p>
            <p>
              程序气氛：
              {actionTarget.stage.gas.every((v) => v === 0)
                ? "四路停气"
                : `${gasSummary(actionTarget.stage.gas)} L/min`}
              。请完成上述实际操作后确认；系统将记录确认人和时间。
            </p>
            <label>
              操作备注（可补充外部测量记录）
              <textarea
                aria-label="人工步骤备注"
                maxLength={4096}
                value={manualNotes}
                onChange={(e) => setManualNotes(e.target.value)}
                className="w-full mt-1 p-2 rounded min-h-20"
                style={{
                  background: "var(--bg)",
                  border: "1px solid var(--border)",
                  color: "var(--fg)",
                }}
              />
            </label>
            <button
              disabled={submitting || ex.can_control === false}
              onClick={doManual}
              className="px-3 py-2 rounded self-end"
              style={{ background: "var(--warn)", color: "#1a1204" }}
            >
              {submitting ? "提交中…" : "已完成上述操作，确认继续"}
            </button>
          </div>
        </Modal>
      )}
      {confirmStop && (
        <Modal title="停止实验" onClose={() => setConfirmStop(false)}>
          <div className="text-[13px] mb-4" style={{ color: "var(--fg-dim)" }}>
            确认停止当前实验?系统将立即切换到保护气氛（N₂ 5.0
            L/min,可燃气归零）。
          </div>
          <div className="flex justify-end gap-2">
            <button
              onClick={() => setConfirmStop(false)}
              className="px-3 py-1.5 rounded text-[13px]"
              style={{
                background: "var(--panel-2)",
                border: "1px solid var(--border-bright)",
                color: "var(--fg)",
              }}
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
