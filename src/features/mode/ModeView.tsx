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

function ProgramCard({
  program,
  canSimulate,
  onCopy,
  onEdit,
  onDelete,
  onSimulate,
}: {
  program: ProgramDto;
  canSimulate: boolean;
  onCopy: () => void;
  onEdit?: () => void;
  onDelete?: () => void;
  onSimulate: () => void;
}) {
  return (
    <div className="panel p-4 flex flex-col gap-3">
      <ProgramPreview program={program} compact />
      <div className="flex justify-end gap-2">
        {canSimulate && (
          <button onClick={onSimulate} className="px-2.5 py-1 rounded text-[12px]" style={btn}>
            模拟快跑
          </button>
        )}
        <button onClick={onCopy} className="px-2.5 py-1 rounded text-[12px]" style={btn}>
          复制为自定义
        </button>
        {onEdit && (
          <button onClick={onEdit} className="px-2.5 py-1 rounded text-[12px]" style={btn}>
            编辑
          </button>
        )}
        {onDelete && (
          <button onClick={onDelete} className="px-2.5 py-1 rounded text-[12px]" style={dangerBtn}>
            删除
          </button>
        )}
      </div>
    </div>
  );
}

function NumberInput({
  value,
  onChange,
  step = "0.1",
}: {
  value: number;
  onChange: (value: number) => void;
  step?: string;
}) {
  return (
    <input
      type="number"
      step={step}
      value={value}
      onChange={(e) => onChange(Number(e.target.value))}
      className="mono w-full px-2 py-1 rounded text-[12px] text-right"
      style={{ background: "var(--bg)", border: "1px solid var(--border)", color: "var(--fg)" }}
    />
  );
}

function ProgramEditor({
  initial,
  limits,
  onClose,
  onSave,
}: {
  initial: ProgramDto;
  limits: GasLimits | null;
  onClose: () => void;
  onSave: (program: ProgramDto) => Promise<void>;
}) {
  const [draft, setDraft] = useState<ProgramDto>(() => cloneProgram(initial));
  const [busy, setBusy] = useState(false);
  const validation = useMemo(() => validateProgramDraft(draft, limits), [draft, limits]);

  const patchStage = (index: number, patch: Partial<StageDto>) => {
    setDraft((cur) => ({
      ...cur,
      stages: cur.stages.map((stage, i) => (i === index ? { ...stage, ...patch } : stage)),
    }));
  };

  const patchGas = (stageIndex: number, gasIndex: number, value: number) => {
    setDraft((cur) => ({
      ...cur,
      stages: cur.stages.map((stage, i) => {
        if (i !== stageIndex) return stage;
        const gas = [0, 1, 2, 3].map((g) => Number(stage.gas[g] ?? 0));
        gas[gasIndex] = value;
        return { ...stage, gas };
      }),
    }));
  };

  const moveStage = (index: number, delta: -1 | 1) => {
    setDraft((cur) => {
      const next = [...cur.stages];
      const target = index + delta;
      if (target < 0 || target >= next.length) return cur;
      const current = next[index];
      const swap = next[target];
      if (!current || !swap) return cur;
      next[index] = swap;
      next[target] = current;
      return { ...cur, stages: next };
    });
  };

  const save = async () => {
    if (validation.errors.length) return;
    setBusy(true);
    try {
      await onSave(sanitizeProgram(draft));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal title="自定义实验模式" onClose={onClose} wide>
      <div className="flex flex-col gap-3">
        <label className="flex flex-col gap-1 text-[12px]" style={{ color: "var(--fg-dim)" }}>
          模式名称
          <input
            value={draft.name}
            onChange={(e) => setDraft((cur) => ({ ...cur, name: e.target.value }))}
            className="px-2 py-1.5 rounded text-[13px]"
            style={{ background: "var(--bg)", border: "1px solid var(--border)", color: "var(--fg)" }}
          />
        </label>

        <div className="overflow-x-auto">
          <table className="w-full text-[12px] border-collapse">
            <thead>
              <tr style={{ color: "var(--fg-faint)" }} className="text-left">
                <th className="py-1.5 pr-2 font-medium">阶段</th>
                <th className="py-1.5 pr-2 font-medium">类型</th>
                <th className="py-1.5 pr-2 font-medium">温度</th>
                <th className="py-1.5 pr-2 font-medium">容差</th>
                <th className="py-1.5 pr-2 font-medium">时长</th>
                <th className="py-1.5 pr-2 font-medium">升温率</th>
                {GAS_LABELS.map((g) => (
                  <th key={g} className="py-1.5 pr-2 font-medium">
                    {g}
                  </th>
                ))}
                <th className="py-1.5 pr-2 font-medium">说明</th>
                <th className="py-1.5 font-medium">操作</th>
              </tr>
            </thead>
            <tbody>
              {draft.stages.map((stage, i) => (
                <tr key={`${stage.name}-${i}`} style={{ borderTop: "1px solid var(--border)" }}>
                  <td className="py-2 pr-2 min-w-[86px]">
                    <input
                      value={stage.name}
                      onChange={(e) => patchStage(i, { name: e.target.value })}
                      className="w-full px-2 py-1 rounded"
                      style={{ background: "var(--bg)", border: "1px solid var(--border)", color: "var(--fg)" }}
                    />
                  </td>
                  <td className="py-2 pr-2 min-w-[92px]">
                    <select
                      value={stage.kind}
                      onChange={(e) => patchStage(i, { kind: e.target.value as StageKind })}
                      className="w-full px-2 py-1 rounded"
                      style={{ background: "var(--bg)", border: "1px solid var(--border)", color: "var(--fg)" }}
                    >
                      {stageKinds.map((kind) => (
                        <option key={kind} value={kind}>
                          {STAGE_KIND_LABEL[kind]}
                        </option>
                      ))}
                    </select>
                  </td>
                  <td className="py-2 pr-2 min-w-[78px]">
                    <NumberInput value={stage.target_temp} onChange={(v) => patchStage(i, { target_temp: v })} />
                  </td>
                  <td className="py-2 pr-2 min-w-[68px]">
                    <NumberInput value={stage.tolerance} onChange={(v) => patchStage(i, { tolerance: v })} />
                  </td>
                  <td className="py-2 pr-2 min-w-[74px]">
                    <NumberInput value={stage.duration_min} onChange={(v) => patchStage(i, { duration_min: v })} />
                  </td>
                  <td className="py-2 pr-2 min-w-[74px]">
                    <NumberInput value={stage.ramp_rate} onChange={(v) => patchStage(i, { ramp_rate: v })} />
                  </td>
                  {GAS_LABELS.map((g, gi) => (
                    <td key={g} className="py-2 pr-2 min-w-[64px]">
                      <NumberInput value={stage.gas[gi] ?? 0} onChange={(v) => patchGas(i, gi, v)} />
                    </td>
                  ))}
                  <td className="py-2 pr-2 min-w-[160px]">
                    <input
                      value={stage.description}
                      onChange={(e) => patchStage(i, { description: e.target.value })}
                      className="w-full px-2 py-1 rounded"
                      style={{ background: "var(--bg)", border: "1px solid var(--border)", color: "var(--fg)" }}
                    />
                  </td>
                  <td className="py-2 min-w-[168px]">
                    <div className="flex gap-1">
                      <button onClick={() => moveStage(i, -1)} className="px-2 py-1 rounded" style={btn}>
                        上移
                      </button>
                      <button onClick={() => moveStage(i, 1)} className="px-2 py-1 rounded" style={btn}>
                        下移
                      </button>
                      <button
                        onClick={() =>
                          setDraft((cur) => ({ ...cur, stages: cur.stages.filter((_, idx) => idx !== i) }))
                        }
                        className="px-2 py-1 rounded"
                        style={dangerBtn}
                      >
                        删除
                      </button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>

        <div className="flex gap-2">
          <button
            onClick={() => setDraft((cur) => ({ ...cur, stages: [...cur.stages, blankStage(cur.stages.length)] }))}
            className="px-3 py-1.5 rounded text-[12px]"
            style={btn}
          >
            新增阶段
          </button>
          <button
            onClick={() => {
              setDraft((cur) => {
                const last = cur.stages[cur.stages.length - 1] ?? blankStage(0);
                return { ...cur, stages: [...cur.stages, { ...last, gas: [...last.gas] }] };
              });
            }}
            className="px-3 py-1.5 rounded text-[12px]"
            style={btn}
          >
            复制末阶段
          </button>
        </div>

        {(validation.errors.length > 0 || validation.warnings.length > 0) && (
          <div className="grid grid-cols-2 gap-3">
            <div className="rounded p-2.5 text-[12px]" style={{ border: "1px solid var(--danger)" }}>
              <div className="font-medium mb-1" style={{ color: "var(--danger)" }}>
                保存阻断
              </div>
              {validation.errors.length ? validation.errors.join("；") : "无"}
            </div>
            <div className="rounded p-2.5 text-[12px]" style={{ border: "1px solid var(--warn)" }}>
              <div className="font-medium mb-1" style={{ color: "var(--warn)" }}>
                安全警告
              </div>
              {validation.warnings.length ? validation.warnings.join("；") : "无"}
            </div>
          </div>
        )}
      </div>

      <div className="flex justify-end gap-2 mt-4">
        <button onClick={onClose} className="px-3 py-1.5 rounded text-[13px]" style={btn}>
          取消
        </button>
        <button
          onClick={save}
          disabled={busy || validation.errors.length > 0}
          className="px-3 py-1.5 rounded text-[13px] font-medium"
          style={{ background: "var(--ok)", color: "#04140a", opacity: busy || validation.errors.length ? 0.6 : 1 }}
        >
          {busy ? "保存中…" : "保存模式"}
        </button>
      </div>
    </Modal>
  );
}

function DryRunModal({ program, onClose }: { program: ProgramDto; onClose: () => void }) {
  const steps = simulateProgram(program);
  return (
    <Modal title="模拟快跑" onClose={onClose} wide>
      <ProgramPreview program={program} compact />
      <div className="mt-4 overflow-x-auto">
        <table className="w-full text-[12px] border-collapse">
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
                <td className="py-1.5 pr-3 mono" style={{ color: "var(--gas)" }}>
                  {step.atmosphere}
                </td>
                <td className="py-1.5 mono">{step.target}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Modal>
  );
}

export function ModeView() {
  const [programs, setPrograms] = useState<ProgramDto[]>([]);
  const [limits, setLimits] = useState<GasLimits | null>(null);
  const [runtimeMode, setRuntimeMode] = useState<RuntimeMode | null>(null);
  const [editor, setEditor] = useState<ProgramDto | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<ProgramDto | null>(null);
  const [dryRun, setDryRun] = useState<ProgramDto | null>(null);
  const push = useToasts((s) => s.push);

  const refresh = () => listPrograms().then(setPrograms).catch(toastError);

  useEffect(() => {
    void refresh();
    getGasSafetyLimits().then(setLimits).catch(() => {});
    getRuntimeMode().then(setRuntimeMode).catch(() => setRuntimeMode("mock"));
  }, []);

  const standard = programs.filter((p) => p.source === "standard");
  const custom = programs.filter((p) => p.source === "custom");
  const canSimulate = runtimeMode !== "real";

  const saveProgram = async (program: ProgramDto) => {
    const saved = await saveCustomProgram(program);
    push(`已保存自定义模式：${saved.name}`, "info");
    setEditor(null);
    await refresh();
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
      <div className="text-[13px]" style={{ color: "var(--fg-dim)" }}>
        标准模式只读；自定义模式会保存到本机配置，并与标准模式走同一套实验引擎和安全钳制。
      </div>

      <section className="flex flex-col gap-3">
        <div className="flex items-center justify-between">
          <SectionTitle>标准模式</SectionTitle>
          <span className="text-[11px]" style={{ color: "var(--fg-faint)" }}>
            依据 GB/T 13240 / 13241 / 13242
          </span>
        </div>
        {standard.map((p) => (
          <ProgramCard
            key={p.id}
            program={p}
            canSimulate={canSimulate}
            onCopy={() => setEditor(copyAsCustom(p))}
            onSimulate={() => setDryRun(p)}
          />
        ))}
      </section>

      <section className="flex flex-col gap-3">
        <div className="flex items-center justify-between">
          <SectionTitle>自定义模式</SectionTitle>
          <button
            onClick={() => setEditor(createBlankCustomProgram())}
            className="px-3 py-1.5 rounded text-[12px] font-medium"
            style={{ background: "var(--ok)", color: "#04140a" }}
          >
            新建模式
          </button>
        </div>
        {custom.length === 0 ? (
          <div className="panel p-4 text-[13px]" style={{ color: "var(--fg-faint)" }}>
            暂无自定义模式。可从标准模式复制，也可新建阶段程序。
          </div>
        ) : (
          custom.map((p) => (
            <ProgramCard
              key={p.id}
              program={p}
              canSimulate={canSimulate}
              onCopy={() => setEditor(copyAsCustom(p))}
              onEdit={() => setEditor(p)}
              onDelete={() => setDeleteTarget(p)}
              onSimulate={() => setDryRun(p)}
            />
          ))
        )}
      </section>

      {editor && (
        <ProgramEditor
          initial={editor}
          limits={limits}
          onClose={() => setEditor(null)}
          onSave={saveProgram}
        />
      )}

      {deleteTarget && (
        <Modal title="删除自定义模式" onClose={() => setDeleteTarget(null)}>
          <div className="text-[13px] mb-4" style={{ color: "var(--fg-dim)" }}>
            确认删除「{deleteTarget.name}」? 已完成实验的程序快照不会受影响。
          </div>
          <div className="flex justify-end gap-2">
            <button onClick={() => setDeleteTarget(null)} className="px-3 py-1.5 rounded text-[13px]" style={btn}>
              取消
            </button>
            <button onClick={deleteProgram} className="px-3 py-1.5 rounded text-[13px]" style={dangerBtn}>
              确认删除
            </button>
          </div>
        </Modal>
      )}

      {dryRun && <DryRunModal program={dryRun} onClose={() => setDryRun(null)} />}
    </Page>
  );
}
