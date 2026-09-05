import { useAuth } from "../../stores/auth";
import { useExperiment } from "../../stores/experiment";
import { useEffect, useMemo, useState } from "react";

import {
  deleteCustomProgram,
  getGasSafetyLimits,
  getRuntimeMode,
  listPrograms,
  saveCustomProgram,
  type RuntimeMode,
} from "../../ipc";
import type { GasLimits, ProgramDto, StageDto, StageKind } from "../../ipc/types";
import { toastError, useToasts } from "../../stores/toast";
import { Page, SectionTitle } from "../../components/Page";
import { Modal } from "../../components/Modal";
import { ProgramPreview } from "../experiment/ProgramPreview";
import {
  GAS_LABELS,
  STAGE_KIND_LABEL,
  blankStage,
  cloneProgram,
  copyAsCustom,
  createBlankCustomProgram,
  durationSummary,
  gasSummary,
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

const stageKinds: StageKind[] = ["HEATING", "STABILIZING", "REDUCING", "COOLING"];

function IconButton({
  title,
  onClick,
  disabled = false,
  danger = false,
  children,
}: {
  title: string;
  onClick: () => void;
  disabled?: boolean;
  danger?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      title={title}
      aria-label={title}
      onClick={onClick}
      disabled={disabled}
      className="inline-flex h-7 w-7 items-center justify-center rounded"
      style={{ ...(danger ? dangerBtn : btn), opacity: disabled ? 0.45 : 1 }}
    >
      {children}
    </button>
  );
}

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
    ? { background: "var(--ok)", color: "#04140a", opacity: disabled ? 0.55 : 1 }
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

function NumberInput({
  value,
  onChange,
  disabled,
  step = "0.1",
}: {
  value: number;
  onChange: (value: number) => void;
  disabled?: boolean;
  step?: string;
}) {
  return (
    <input
      type="number"
      step={step}
      value={value}
      disabled={disabled}
      onChange={(e) => onChange(Number(e.target.value))}
      className="mono w-full rounded px-2 py-1 text-right text-[12px]"
      style={{
        background: disabled ? "rgba(255,255,255,0.025)" : "var(--bg)",
        border: "1px solid var(--border)",
        color: disabled ? "var(--fg-dim)" : "var(--fg)",
      }}
    />
  );
}

function DryRunModal({ program, onClose }: { program: ProgramDto; onClose: () => void }) {
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
              <tr key={step.stage} style={{ borderTop: "1px solid var(--border)" }}>
                <td className="py-1.5 pr-3">{step.stage}</td>
                <td className="py-1.5 pr-3" style={{ color: "var(--fg-dim)" }}>
                  {step.condition}
                </td>
                <td className="mono py-1.5 pr-3" style={{ color: "var(--gas)" }}>
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
  const admin = useAuth(s => s.snapshot?.user?.role === "ADMIN");
  const running = useExperiment(s => s.state.running);
  const [programs, setPrograms] = useState<ProgramDto[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [draft, setDraft] = useState<ProgramDto | null>(null);
  const [limits, setLimits] = useState<GasLimits | null>(null);
  const [runtimeMode, setRuntimeMode] = useState<RuntimeMode | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<ProgramDto | null>(null);
  const [dryRun, setDryRun] = useState<ProgramDto | null>(null);
  const push = useToasts((s) => s.push);

  const refresh = async (nextSelectedId?: string) => {
    const items = await listPrograms();
    setPrograms(items);
    setSelectedId((cur) => nextSelectedId ?? (cur && items.some((p) => p.id === cur) ? cur : items[0]?.id ?? null));
  };

  useEffect(() => {
    void refresh();
    getGasSafetyLimits().then(setLimits).catch(() => {});
    getRuntimeMode().then(setRuntimeMode).catch(() => setRuntimeMode("mock"));
  }, []);

  const selected = useMemo(
    () => (selectedId ? programs.find((p) => p.id === selectedId) ?? null : null),
    [programs, selectedId],
  );

  useEffect(() => {
    if (!selectedId) return;
    if (selected) setDraft(cloneProgram(selected));
    else setDraft(null);
  }, [selected, selectedId]);

  const active = draft ?? selected;
  const standard = programs.filter((p) => p.source === "standard");
  const custom = programs.filter((p) => p.source === "custom");
  const canEdit = admin && !running && active?.source === "custom";
  const validation = useMemo(
    () => (active ? validateProgramDraft(active, limits) : { errors: [], warnings: [] }),
    [active, limits],
  );
  const dirty = !!active && (!selected || JSON.stringify(active) !== JSON.stringify(cloneProgram(selected)));
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
    if (!active || !canEdit) return;
    setDraft({
      ...active,
      stages: active.stages.map((stage, i) => (i === index ? { ...stage, ...patch } : stage)),
    });
  };

  const patchGas = (stageIndex: number, gasIndex: number, value: number) => {
    if (!active || !canEdit) return;
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

  const moveStage = (index: number, delta: -1 | 1) => {
    if (!active || !canEdit) return;
    const next = [...active.stages];
    const target = index + delta;
    if (target < 0 || target >= next.length) return;
    const current = next[index];
    const swap = next[target];
    if (!current || !swap) return;
    next[index] = swap;
    next[target] = current;
    setDraft({ ...active, stages: next });
  };

  const duplicateStage = (index: number) => {
    if (!active || !canEdit) return;
    const stage = active.stages[index];
    if (!stage) return;
    const next = [...active.stages];
    next.splice(index + 1, 0, { ...stage, name: `${stage.name} 副本`, gas: [...stage.gas] });
    setDraft({ ...active, stages: next });
  };

  const addStage = () => {
    if (!active || !canEdit) return;
    setDraft({ ...active, stages: [...active.stages, blankStage(active.stages.length)] });
  };

  const removeStage = (index: number) => {
    if (!active || !canEdit) return;
    setDraft({ ...active, stages: active.stages.filter((_, i) => i !== index) });
  };

  const save = async () => {
    if (!active || active.source !== "custom" || validation.errors.length) return;
    try {
      const saved = await saveCustomProgram(sanitizeProgram(active));
      push(`已保存自定义模式：${saved.name}`, "info");
      setDraft(cloneProgram(saved));
      await refresh(saved.id);
    } catch (e) {
      toastError(e);
    }
  };

  const copySelected = () => {
    if (!active) return;
    setSelectedId(null);
    setDraft(copyAsCustom(active));
  };

  const addCustom = () => {
    setSelectedId(null);
    setDraft(createBlankCustomProgram());
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
          <div className="border-b px-4 py-3" style={{ borderColor: "var(--border)" }}>
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
                        background: program.source === "standard" ? "rgba(76,157,255,0.14)" : "rgba(63,185,80,0.14)",
                        color: program.source === "standard" ? "var(--accent)" : "var(--ok)",
                      }}
                    >
                      {program.source === "standard" ? "标准" : "自定义"}
                    </span>
                    {!program.enabled && (
                      <span className="text-[10px]" style={{ color: "var(--danger)" }}>
                        停用
                      </span>
                    )}
                  </div>
                  <div className="mt-1 truncate">{program.name}</div>
                  <div className="mono mt-1 text-[11px]" style={{ color: "var(--fg-faint)" }}>
                    {program.sample_prefix} / {durationSummary(program)}
                  </div>
                </button>
              );
            })}
          </div>

          <div className="grid grid-cols-3 gap-2 border-t p-3" style={{ borderColor: "var(--border)" }}>
            <ActionButton onClick={copySelected} disabled={!active || !admin || running}>
              <span aria-hidden="true">⧉</span> 复制
            </ActionButton>
            <ActionButton onClick={addCustom} disabled={!admin || running} primary>
              <span aria-hidden="true">＋</span> 添加
            </ActionButton>
            <ActionButton
              onClick={() => active && active.source === "custom" && active.id ? setDeleteTarget(active) : undefined}
              disabled={!admin || running || !active || active.source !== "custom" || !active.id}
              danger
            >
              <span aria-hidden="true">×</span> 删除
            </ActionButton>
          </div>
        </aside>

        <main className="flex min-w-0 flex-col gap-4">
          {!active ? (
            <div className="panel p-4 text-[13px]" style={{ color: "var(--fg-faint)" }}>
              暂无可用实验模式
            </div>
          ) : (
            <>
              <section className="panel p-4">
                <div className="flex items-center justify-between gap-3">
                  <SectionTitle>基本信息</SectionTitle>
                  <div className="flex items-center gap-2">
                    {canSimulate && (
                      <ActionButton onClick={() => setDryRun(active)}>
                        <span aria-hidden="true">▷</span> 模拟快跑
                      </ActionButton>
                    )}
                    {active.source === "standard" && (
                      <ActionButton onClick={copySelected} disabled={!admin || running}>
                        <span aria-hidden="true">⧉</span> 复制为自定义
                      </ActionButton>
                    )}
                    {canEdit && selected && (
                      <ActionButton onClick={() => setDraft(cloneProgram(selected))} disabled={!dirty}>
                        <span aria-hidden="true">↶</span> 重置
                      </ActionButton>
                    )}
                    {canEdit && (
                      <ActionButton onClick={save} disabled={!dirty || validation.errors.length > 0} primary>
                        <span aria-hidden="true">保存</span> 保存修改
                      </ActionButton>
                    )}
                  </div>
                </div>

                <div className="mt-3 grid grid-cols-[92px_1fr_92px_160px] gap-x-3 gap-y-3 text-[12px]">
                  <label className="self-center" style={{ color: "var(--fg-dim)" }}>
                    名称
                  </label>
                  <TextInput value={active.name} disabled={!canEdit} onChange={(name) => patchProgram({ name })} />
                  <label className="self-center" style={{ color: "var(--fg-dim)" }}>
                    前缀
                  </label>
                  <TextInput
                    value={active.sample_prefix}
                    disabled={!canEdit}
                    mono
                    onChange={(sample_prefix) => patchProgram({ sample_prefix: sample_prefix.toUpperCase() })}
                  />

                  <label className="self-start pt-2" style={{ color: "var(--fg-dim)" }}>
                    描述
                  </label>
                  <textarea
                    value={active.description}
                    disabled={!canEdit}
                    onChange={(e) => patchProgram({ description: e.target.value })}
                    className="min-h-20 rounded px-2 py-1.5 text-[13px]"
                    style={{
                      background: canEdit ? "var(--bg)" : "rgba(255,255,255,0.025)",
                      border: "1px solid var(--border)",
                      color: canEdit ? "var(--fg)" : "var(--fg-dim)",
                    }}
                  />
                  <label className="self-center" style={{ color: "var(--fg-dim)" }}>
                    状态
                  </label>
                  <label className="flex items-center gap-2 text-[13px]">
                    <input
                      type="checkbox"
                      checked={active.enabled}
                      disabled={!canEdit}
                      onChange={(e) => patchProgram({ enabled: e.target.checked })}
                    />
                    启用
                  </label>

                  <label className="self-center" style={{ color: "var(--fg-dim)" }}>
                    类别
                  </label>
                  <TextInput value={active.category} disabled={!canEdit} onChange={(category) => patchProgram({ category })} />
                  <label className="self-center" style={{ color: "var(--fg-dim)" }}>
                    来源
                  </label>
                  <div className="mono self-center text-[13px]" style={{ color: "var(--fg-dim)" }}>
                    {active.source}
                  </div>
                </div>

                {(validation.errors.length > 0 || validation.warnings.length > 0) && (
                  <div className="mt-3 grid grid-cols-2 gap-3 text-[12px]">
                    <div className="rounded p-2.5" style={{ border: "1px solid var(--danger)" }}>
                      <div className="mb-1 font-medium" style={{ color: "var(--danger)" }}>
                        保存阻断
                      </div>
                      {validation.errors.length ? validation.errors.join("；") : "无"}
                    </div>
                    <div className="rounded p-2.5" style={{ border: "1px solid var(--warn)" }}>
                      <div className="mb-1 font-medium" style={{ color: "var(--warn)" }}>
                        安全警告
                      </div>
                      {validation.warnings.length ? validation.warnings.join("；") : "无"}
                    </div>
                  </div>
                )}
              </section>

              <section className="panel min-h-0 p-4">
                <div className="flex items-center justify-between gap-3">
                  <SectionTitle>实验阶段</SectionTitle>
                  <div className="mono text-[11px]" style={{ color: "var(--fg-faint)" }}>
                    {active.stages.length} 阶段 / {durationSummary(active)}
                  </div>
                </div>

                <div className="overflow-auto">
                  <table className="min-w-[980px] w-full border-collapse text-[12px]">
                    <thead>
                      <tr style={{ color: "var(--fg-faint)" }} className="text-left">
                        <th className="py-2 pr-2 font-medium">#</th>
                        <th className="py-2 pr-2 font-medium">阶段</th>
                        <th className="py-2 pr-2 font-medium">类型</th>
                        <th className="py-2 pr-2 font-medium">描述</th>
                        <th className="py-2 pr-2 font-medium text-right">温度(℃)</th>
                        <th className="py-2 pr-2 font-medium text-right">时间(min)</th>
                        <th className="py-2 pr-2 font-medium text-right">升温速率</th>
                        <th className="py-2 pr-2 font-medium">气体组成(L/min)</th>
                        <th className="py-2 font-medium">操作</th>
                      </tr>
                    </thead>
                    <tbody>
                      {active.stages.map((stage, i) => (
                        <tr key={`${stage.name}-${i}`} style={{ borderTop: "1px solid var(--border)" }}>
                          <td className="mono py-2 pr-2" style={{ color: "var(--fg-faint)" }}>
                            {i + 1}
                          </td>
                          <td className="py-2 pr-2 min-w-[110px]">
                            {canEdit ? (
                              <TextInput value={stage.name} onChange={(name) => patchStage(i, { name })} />
                            ) : (
                              stage.name
                            )}
                          </td>
                          <td className="py-2 pr-2 min-w-[110px]">
                            {canEdit ? (
                              <select
                                value={stage.kind}
                                onChange={(e) => patchStage(i, { kind: e.target.value as StageKind })}
                                className="w-full rounded px-2 py-1.5 text-[12px]"
                                style={{ background: "var(--bg)", border: "1px solid var(--border)", color: "var(--fg)" }}
                              >
                                {stageKinds.map((kind) => (
                                  <option key={kind} value={kind}>
                                    {STAGE_KIND_LABEL[kind]}
                                  </option>
                                ))}
                              </select>
                            ) : (
                              STAGE_KIND_LABEL[stage.kind]
                            )}
                          </td>
                          <td className="py-2 pr-2 min-w-[180px]">
                            {canEdit ? (
                              <TextInput value={stage.description} onChange={(description) => patchStage(i, { description })} />
                            ) : (
                              <span style={{ color: "var(--fg-dim)" }}>{stage.description}</span>
                            )}
                          </td>
                          <td className="py-2 pr-2 min-w-[90px]">
                            {canEdit ? (
                              <NumberInput value={stage.target_temp} onChange={(target_temp) => patchStage(i, { target_temp })} />
                            ) : (
                              <div className="mono text-right">{stage.target_temp}±{stage.tolerance}</div>
                            )}
                          </td>
                          <td className="py-2 pr-2 min-w-[88px]">
                            {canEdit ? (
                              <NumberInput value={stage.duration_min} onChange={(duration_min) => patchStage(i, { duration_min })} />
                            ) : (
                              <div className="mono text-right">{stage.duration_min > 0 ? stage.duration_min : "动态"}</div>
                            )}
                          </td>
                          <td className="py-2 pr-2 min-w-[88px]">
                            {canEdit ? (
                              <NumberInput value={stage.ramp_rate} onChange={(ramp_rate) => patchStage(i, { ramp_rate })} />
                            ) : (
                              <div className="mono text-right">{stage.ramp_rate}</div>
                            )}
                          </td>
                          <td className="py-2 pr-2 min-w-[260px]">
                            {canEdit ? (
                              <div className="grid grid-cols-4 gap-1">
                                {GAS_LABELS.map((label, gasIndex) => (
                                  <label key={label} className="flex items-center gap-1">
                                    <span className="w-8 text-[11px]" style={{ color: "var(--fg-faint)" }}>
                                      {label}
                                    </span>
                                    <NumberInput
                                      value={stage.gas[gasIndex] ?? 0}
                                      onChange={(value) => patchGas(i, gasIndex, value)}
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
                          <td className="py-2">
                            <div className="flex gap-1">
                              <IconButton title="上移" onClick={() => moveStage(i, -1)} disabled={!canEdit || i === 0}>
                                <span aria-hidden="true">↑</span>
                              </IconButton>
                              <IconButton
                                title="下移"
                                onClick={() => moveStage(i, 1)}
                                disabled={!canEdit || i === active.stages.length - 1}
                              >
                                <span aria-hidden="true">↓</span>
                              </IconButton>
                              <IconButton title="复制阶段" onClick={() => duplicateStage(i)} disabled={!canEdit}>
                                <span aria-hidden="true">⧉</span>
                              </IconButton>
                              <IconButton title="删除阶段" onClick={() => removeStage(i)} disabled={!canEdit} danger>
                                <span aria-hidden="true">×</span>
                              </IconButton>
                            </div>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>

                {canEdit && (
                  <div className="mt-3 flex justify-end gap-2">
                    <ActionButton onClick={addStage}>
                      <span aria-hidden="true">＋</span> 添加阶段
                    </ActionButton>
                    <ActionButton onClick={save} disabled={!dirty || validation.errors.length > 0} primary>
                      <span aria-hidden="true">保存</span> 保存修改
                    </ActionButton>
                  </div>
                )}
              </section>
            </>
          )}
        </main>
      </div>

      {deleteTarget && (
        <Modal title="删除自定义模式" onClose={() => setDeleteTarget(null)}>
          <div className="mb-4 text-[13px]" style={{ color: "var(--fg-dim)" }}>
            确认删除“{deleteTarget.name}”？已完成实验的程序快照不会受影响。
          </div>
          <div className="flex justify-end gap-2">
            <ActionButton onClick={() => setDeleteTarget(null)}>取消</ActionButton>
            <ActionButton onClick={deleteProgram} danger>
              <span aria-hidden="true">×</span> 确认删除
            </ActionButton>
          </div>
        </Modal>
      )}

      {dryRun && <DryRunModal program={dryRun} onClose={() => setDryRun(null)} />}
    </Page>
  );
}
