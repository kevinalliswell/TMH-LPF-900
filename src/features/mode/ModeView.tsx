import { customShapeError } from "../../ipc/programRules";
import { useAuth } from "../../stores/auth";
import { useExperiment } from "../../stores/experiment";
import { useEffect, useMemo, useState } from "react";

import {
  deleteCustomProgram,
  getGasSafetyLimits,
  getRuntimeMode,
  listPrograms,
  listCustomProgramRevisions,
  saveCustomProgram,
  type RuntimeMode,
} from "../../ipc";
import type { GasLimits, ProgramDto, StageDto } from "../../ipc/types";
import { toastError, useToasts } from "../../stores/toast";
import { Page, SectionTitle } from "../../components/Page";
import { Modal } from "../../components/Modal";
import { CustomStageTable } from "./CustomStageTable";
import { ProgramProvenance } from "../experiment/ProgramProvenance";
import { ProgramPreview } from "../experiment/ProgramPreview";
import {
  cloneProgram,
  copyAsCustom,
  durationSummary,
  sanitizeProgram,
  simulateProgram,
  validateProgramDraft,
} from "../experiment/programUtils";

const btn = {
  background: "var(--panel-2)",
  border: "1px solid var(--border-bright)",
  color: "var(--fg)",
} as const;

const dangerBtn = {
  background: "var(--panel-2)",
  border: "1px solid var(--danger)",
  color: "var(--danger)",
} as const;

function ActionButton({
  children,
  onClick,
  disabled = false,
  danger = false,
  primary = false,
}: {
  children: React.ReactNode;
  onClick: () => void;
  disabled?: boolean;
  danger?: boolean;
  primary?: boolean;
}) {
  const style = primary
    ? {
        background: "var(--ok)",
        color: "#04140a",
        opacity: disabled ? 0.55 : 1,
      }
    : { ...(danger ? dangerBtn : btn), opacity: disabled ? 0.45 : 1 };
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className="inline-flex items-center justify-center gap-1.5 rounded px-3 py-1.5 text-[12px] font-medium"
      style={style}
    >
      {children}
    </button>
  );
}

function TextInput({
  value,
  onChange,
  disabled,
  mono = false,
}: {
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
  mono?: boolean;
}) {
  return (
    <input
      value={value}
      disabled={disabled}
      onChange={(e) => onChange(e.target.value)}
      className={`w-full rounded px-2 py-1.5 text-[13px] ${mono ? "mono" : ""}`}
      style={{
        background: disabled ? "rgba(255,255,255,0.025)" : "var(--bg)",
        border: "1px solid var(--border)",
        color: disabled ? "var(--fg-dim)" : "var(--fg)",
      }}
    />
  );
}

function DryRunModal({
  program,
  onClose,
}: {
  program: ProgramDto;
  onClose: () => void;
}) {
  const steps = simulateProgram(program);
  return (
    <Modal title="模拟快跑" onClose={onClose} wide>
      <ProgramPreview program={program} compact />
      <div className="mt-4 overflow-x-auto">
        <table className="w-full border-collapse text-[12px]">
          <thead>
            <tr style={{ color: "var(--fg-faint)" }} className="text-left">
              <th className="py-1.5 pr-3 font-medium">节点</th>
              <th className="py-1.5 pr-3 font-medium">推进条件</th>
              <th className="py-1.5 pr-3 font-medium">气氛</th>
              <th className="py-1.5 font-medium">目标</th>
            </tr>
          </thead>
          <tbody>
            {steps.map((step) => (
              <tr
                key={step.stage}
                style={{ borderTop: "1px solid var(--border)" }}
              >
                <td className="py-1.5 pr-3">{step.stage}</td>
                <td className="py-1.5 pr-3" style={{ color: "var(--fg-dim)" }}>
                  {step.condition}
                </td>
                <td
                  className="mono py-1.5 pr-3"
                  style={{ color: "var(--gas)" }}
                >
                  {step.atmosphere}
                </td>
                <td className="mono py-1.5">{step.target}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Modal>
  );
}

export function ModeView() {
  const admin = useAuth((s) => s.snapshot?.user?.role === "ADMIN");
  const running = useExperiment((s) => s.state.running);
  const [programs, setPrograms] = useState<ProgramDto[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [draft, setDraft] = useState<ProgramDto | null>(null);
  const [limits, setLimits] = useState<GasLimits | null>(null);
  const [limitsFailed, setLimitsFailed] = useState(false);
  const [runtimeMode, setRuntimeMode] = useState<RuntimeMode | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<ProgramDto | null>(null);
  const [reason, setReason] = useState("");
  const [saving, setSaving] = useState(false);
  const [revisions, setRevisions] = useState<ProgramDto[] | null>(null);
  const [loadingHistory, setLoadingHistory] = useState(false);
  const [dryRun, setDryRun] = useState<ProgramDto | null>(null);
  const push = useToasts((s) => s.push);

  const refresh = async (nextSelectedId?: string) => {
    const items = await listPrograms();
    setPrograms(items);
    setSelectedId(
      (cur) =>
        nextSelectedId ??
        (cur && items.some((p) => p.id === cur) ? cur : (items[0]?.id ?? null)),
    );
  };

  useEffect(() => {
    void refresh().catch(toastError);
    getGasSafetyLimits()
      .then(setLimits)
      .catch(() => setLimitsFailed(true));
    getRuntimeMode()
      .then(setRuntimeMode)
      .catch(() => setRuntimeMode("mock"));
  }, []);

  const selected = useMemo(
    () =>
      selectedId ? (programs.find((p) => p.id === selectedId) ?? null) : null,
    [programs, selectedId],
  );

  useEffect(() => {
    setReason("");
    if (!selectedId) return;
    if (selected) setDraft(cloneProgram(selected));
    else setDraft(null);
  }, [selected, selectedId]);

  const active = draft ?? selected;
  const standard = programs.filter((p) => p.source === "standard");
  const custom = programs.filter((p) => p.source === "custom");
  const canEdit = admin && !running && !saving && active?.source === "custom";
  const validation = useMemo(
    () =>
      active
        ? validateProgramDraft(active, limits)
        : { errors: [], warnings: [] },
    [active, limits],
  );
  const dirty =
    !!active &&
    (!selected ||
      JSON.stringify(active) !== JSON.stringify(cloneProgram(selected)));
  const canSimulate = runtimeMode !== "real";

  const selectProgram = (program: ProgramDto) => {
    setSelectedId(program.id);
    setDraft(cloneProgram(program));
  };

  const patchProgram = (patch: Partial<ProgramDto>) => {
    if (!active) return;
    setDraft({ ...active, ...patch });
  };

  const patchStage = (index: number, patch: Partial<StageDto>) => {
    if (!active || !canEdit || index >= 3) return;
    setDraft({
      ...active,
      stages: active.stages.map((stage, i) =>
        i === index ? { ...stage, ...patch } : stage,
      ),
    });
  };

  const patchGas = (stageIndex: number, gasIndex: number, value: number) => {
    if (!active || !canEdit || stageIndex >= 3) return;
    setDraft({
      ...active,
      stages: active.stages.map((stage, i) => {
        if (i !== stageIndex) return stage;
        const gas = [0, 1, 2, 3].map((g) => Number(stage.gas[g] ?? 0));
        gas[gasIndex] = value;
        return { ...stage, gas };
      }),
    });
  };

  const save = async () => {
    if (!active || !canEdit || !reason.trim() || validation.errors.length)
      return;
    setSaving(true);
    try {
      const saved = await saveCustomProgram(sanitizeProgram(active), reason);
      push(`已保存自定义模式：${saved.name}`, "info");
      setDraft(cloneProgram(saved));
      await refresh(saved.id);
      setReason("");
    } catch (e) {
      toastError(e);
    } finally {
      setSaving(false);
    }
  };

  const copySelected = () => {
    if (!active) return;
    setSelectedId(null);
    const base =
      active.source === "standard"
        ? active
        : programs.find(
            (p) => p.id === active.base_standard_id && p.source === "standard",
          );
    if (base) {
      const source = !customShapeError(active, base) ? active : base;
      setDraft(copyAsCustom(source));
      if (source !== active)
        push("已从当前国标新建，请重新填写研究条件", "info");
    }
    setReason("");
  };

  const addCustom = () => {
    setSelectedId(null);
    const base = programs.find((p) => p.source === "standard");
    if (base) setDraft(copyAsCustom(base));
    setReason("");
  };

  const deleteProgram = async () => {
    if (!deleteTarget) return;
    try {
      await deleteCustomProgram(deleteTarget.id);
      push(`已删除自定义模式：${deleteTarget.name}`, "warn");
      setDeleteTarget(null);
      await refresh();
    } catch (e) {
      toastError(e);
    }
  };

  return (
    <Page wide>
      <div className="grid min-h-[calc(100vh-170px)] grid-cols-[284px_1fr] gap-4">
        <aside className="panel flex min-h-0 flex-col overflow-hidden">
          <div
            className="border-b px-4 py-3"
            style={{ borderColor: "var(--border)" }}
          >
            <SectionTitle>实验模式</SectionTitle>
            <div className="text-[12px]" style={{ color: "var(--fg-dim)" }}>
              标准只读，自定义可编辑
            </div>
          </div>

          <div className="min-h-0 flex-1 overflow-auto py-2">
            {[...standard, ...custom].map((program) => {
              const activeId = active?.id && active.id === program.id;
              return (
                <button
                  key={program.id}
                  type="button"
                  onClick={() => selectProgram(program)}
                  className="w-full border-b px-4 py-3 text-left text-[13px]"
                  style={{
                    borderColor: "var(--border)",
                    background: activeId ? "var(--panel-2)" : "transparent",
                    color: activeId ? "var(--fg)" : "var(--fg-dim)",
                  }}
                >
                  <div className="flex items-center gap-2">
                    <span
                      className="rounded px-1.5 py-0.5 text-[10px]"
                      style={{
                        background:
                          program.source === "standard"
                            ? "rgba(76,157,255,0.14)"
                            : "rgba(63,185,80,0.14)",
                        color:
                          program.source === "standard"
                            ? "var(--accent)"
                            : "var(--ok)",
                      }}
                    >
                      {program.source === "standard" ? "标准" : "自定义"}
                    </span>
                    {!program.enabled && (
                      <span
                        className="text-[10px]"
                        style={{ color: "var(--danger)" }}
                      >
                        停用
                      </span>
                    )}
                  </div>
                  <div className="mt-1 truncate">{program.name}</div>
                  <div
                    className="mono mt-1 text-[11px]"
                    style={{ color: "var(--fg-faint)" }}
                  >
                    {program.sample_prefix} / {durationSummary(program)}
                  </div>
                </button>
              );
            })}
          </div>

          <div
            className="grid grid-cols-3 gap-2 border-t p-3"
            style={{ borderColor: "var(--border)" }}
          >
            <ActionButton
              onClick={copySelected}
              disabled={!active || !admin || running}
            >
              <span aria-hidden="true">⧉</span> 复制
            </ActionButton>
            <ActionButton
              onClick={addCustom}
              disabled={!admin || running}
              primary
            >
              <span aria-hidden="true">＋</span> 添加
            </ActionButton>
            <ActionButton
              onClick={() =>
                active && active.source === "custom" && active.id
                  ? setDeleteTarget(active)
                  : undefined
              }
              disabled={
                !admin ||
                running ||
                !active ||
                active.source !== "custom" ||
                !active.id
              }
              danger
            >
              <span aria-hidden="true">×</span> 删除
            </ActionButton>
          </div>
        </aside>

        <main className="flex min-w-0 flex-col gap-4">
          {!active ? (
            <div
              className="panel p-4 text-[13px]"
              style={{ color: "var(--fg-faint)" }}
            >
              暂无可用实验模式
            </div>
          ) : (
            <>
              <section className="panel p-4">
                <div className="flex items-center justify-between gap-3">
                  <SectionTitle>基本信息</SectionTitle>
                  <div className="flex items-center gap-2">
                    {active.source === "custom" && active.id && (
                      <ActionButton
                        disabled={loadingHistory}
                        onClick={() => {
                          setLoadingHistory(true);
                          listCustomProgramRevisions(active.id)
                            .then(setRevisions)
                            .catch(toastError)
                            .finally(() => setLoadingHistory(false));
                        }}
                      >
                        {loadingHistory ? "读取中…" : "版本历史"}
                      </ActionButton>
                    )}
                    {canSimulate && (
                      <ActionButton onClick={() => setDryRun(active)}>
                        <span aria-hidden="true">▷</span> 模拟快跑
                      </ActionButton>
                    )}
                    {active.source === "standard" && (
                      <ActionButton
                        onClick={copySelected}
                        disabled={!admin || running}
                      >
                        <span aria-hidden="true">⧉</span> 复制为自定义
                      </ActionButton>
                    )}
                    {canEdit && selected && (
                      <ActionButton
                        onClick={() => setDraft(cloneProgram(selected))}
                        disabled={!dirty}
                      >
                        <span aria-hidden="true">↶</span> 重置
                      </ActionButton>
                    )}
                    {canEdit && (
                      <ActionButton
                        onClick={save}
                        disabled={
                          !dirty ||
                          !reason.trim() ||
                          validation.errors.length > 0 ||
                          !limits ||
                          saving
                        }
                        primary
                      >
                        保存修改
                      </ActionButton>
                    )}
                  </div>
                </div>

                <div className="mt-3 grid grid-cols-[92px_1fr_92px_160px] gap-x-3 gap-y-3 text-[12px]">
                  <label
                    className="self-center"
                    style={{ color: "var(--fg-dim)" }}
                  >
                    名称
                  </label>
                  <TextInput
                    value={active.name}
                    disabled={!canEdit}
                    onChange={(name) => patchProgram({ name })}
                  />
                  <label
                    className="self-center"
                    style={{ color: "var(--fg-dim)" }}
                  >
                    前缀
                  </label>
                  <TextInput
                    value={active.sample_prefix}
                    disabled={!canEdit}
                    mono
                    onChange={(sample_prefix) =>
                      patchProgram({
                        sample_prefix: sample_prefix.toUpperCase(),
                      })
                    }
                  />

                  <label
                    className="self-start pt-2"
                    style={{ color: "var(--fg-dim)" }}
                  >
                    描述
                  </label>
                  <textarea
                    value={active.description}
                    disabled={!canEdit}
                    onChange={(e) =>
                      patchProgram({ description: e.target.value })
                    }
                    className="min-h-20 rounded px-2 py-1.5 text-[13px]"
                    style={{
                      background: canEdit
                        ? "var(--bg)"
                        : "rgba(255,255,255,0.025)",
                      border: "1px solid var(--border)",
                      color: canEdit ? "var(--fg)" : "var(--fg-dim)",
                    }}
                  />
                  <label
                    className="self-center"
                    style={{ color: "var(--fg-dim)" }}
                  >
                    状态
                  </label>
                  <label className="flex items-center gap-2 text-[13px]">
                    <input
                      type="checkbox"
                      checked={active.enabled}
                      disabled={!canEdit}
                      onChange={(e) =>
                        patchProgram({ enabled: e.target.checked })
                      }
                    />
                    启用
                  </label>

                  <label
                    className="self-center"
                    style={{ color: "var(--fg-dim)" }}
                  >
                    类别
                  </label>
                  <TextInput
                    value={active.category}
                    disabled={!canEdit}
                    onChange={(category) => patchProgram({ category })}
                  />
                  <label
                    className="self-center"
                    style={{ color: "var(--fg-dim)" }}
                  >
                    来源
                  </label>
                  <div
                    className="mono self-center text-[13px]"
                    style={{ color: "var(--fg-dim)" }}
                  >
                    {active.source}
                  </div>
                </div>

                {canEdit && (
                  <label className="mt-3 block text-[12px]">
                    本次修改原因（必填）
                    <textarea
                      aria-label="本次修改原因"
                      value={reason}
                      maxLength={4096}
                      onChange={(e) => setReason(e.target.value)}
                      className="mt-1 min-h-16 w-full rounded p-2"
                      style={{
                        background: "var(--bg)",
                        border: "1px solid var(--border)",
                        color: "var(--fg)",
                      }}
                    />
                  </label>
                )}
                {active.source === "custom" && (
                  <div className="mt-3">
                    <ProgramProvenance program={active} draft={dirty} />
                  </div>
                )}
                {!limits && (
                  <p
                    className="mt-2 text-[12px]"
                    style={{ color: "var(--warn)" }}
                  >
                    {limitsFailed
                      ? "气体限值读取失败，保存尚不可用。"
                      : "正在读取气体限值；读取成功后可保存。"}
                    {limitsFailed && (
                      <button
                        className="ml-2 underline"
                        onClick={() => {
                          setLimitsFailed(false);
                          getGasSafetyLimits()
                            .then(setLimits)
                            .catch(() => setLimitsFailed(true));
                        }}
                      >
                        重试读取
                      </button>
                    )}
                  </p>
                )}
                {(validation.errors.length > 0 ||
                  validation.warnings.length > 0) && (
                  <div className="mt-3 grid grid-cols-2 gap-3 text-[12px]">
                    <div
                      className="rounded p-2.5"
                      style={{ border: "1px solid var(--danger)" }}
                    >
                      <div
                        className="mb-1 font-medium"
                        style={{ color: "var(--danger)" }}
                      >
                        保存阻断
                      </div>
                      {validation.errors.length
                        ? validation.errors.join("；")
                        : "无"}
                    </div>
                    <div
                      className="rounded p-2.5"
                      style={{ border: "1px solid var(--warn)" }}
                    >
                      <div
                        className="mb-1 font-medium"
                        style={{ color: "var(--warn)" }}
                      >
                        安全警告
                      </div>
                      {validation.warnings.length
                        ? validation.warnings.join("；")
                        : "无"}
                    </div>
                  </div>
                )}
              </section>

              <section className="panel min-h-0 p-4">
                <div className="flex items-center justify-between gap-3">
                  <SectionTitle>实验阶段</SectionTitle>
                  <div
                    className="mono text-[11px]"
                    style={{ color: "var(--fg-faint)" }}
                  >
                    {active.stages.length} 阶段 / {durationSummary(active)}
                  </div>
                </div>

                <CustomStageTable
                  program={active}
                  editable={!!canEdit}
                  limits={limits}
                  onStage={patchStage}
                  onGas={patchGas}
                />
              </section>
            </>
          )}
        </main>
      </div>

      {deleteTarget && (
        <Modal title="删除自定义模式" onClose={() => setDeleteTarget(null)}>
          <div className="mb-4 text-[13px]" style={{ color: "var(--fg-dim)" }}>
            确认删除“{deleteTarget.name}
            ”？已保存的版本历史与实验程序快照均会保留。
          </div>
          <div className="flex justify-end gap-2">
            <ActionButton onClick={() => setDeleteTarget(null)}>
              取消
            </ActionButton>
            <ActionButton onClick={deleteProgram} danger>
              <span aria-hidden="true">×</span> 确认删除
            </ActionButton>
          </div>
        </Modal>
      )}

      {revisions && (
        <Modal
          title="自定义程序版本历史"
          onClose={() => setRevisions(null)}
          wide
        >
          {revisions.length ? (
            <div className="flex flex-col gap-3">
              {revisions.map((p, i) => (
                <details key={i} open={i === 0}>
                  <summary className="cursor-pointer py-2">
                    版本 {p.provenance?.revision ?? "0（未记录）"} ·{" "}
                    {p.provenance?.edited_by ?? "编辑者未记录"}
                  </summary>
                  <ProgramPreview program={p} compact />
                </details>
              ))}
            </div>
          ) : (
            <p>尚无保存的版本记录。</p>
          )}
        </Modal>
      )}
      {dryRun && (
        <DryRunModal program={dryRun} onClose={() => setDryRun(null)} />
      )}
    </Page>
  );
}
