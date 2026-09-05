import { useEffect, useState } from "react";
import {
  generateSampleId,
  getExperimentStartDefaults,
  listPrograms,
  preflightExperiment,
  startExperiment,
} from "../../ipc";
import type { ExperimentParams, ExperimentPreflightResult, ExperimentStartDefaults, ProgramDto } from "../../ipc/types";
import { Modal } from "../../components/Modal";
import { toastError, useToasts } from "../../stores/toast";
import { ProgramPreview } from "./ProgramPreview";

type Form = {
  sample_id: string;
  sample_name: string;
  sample_weight: string;
  operator: string;
  experiment_type: string;
  experiment_time: string;
  project_name: string;
  notes: string;
};

const empty: Form = {
  sample_id: "",
  sample_name: "",
  sample_weight: "",
  operator: "",
  experiment_type: "",
  experiment_time: "",
  project_name: "",
  notes: "",
};

function toDatetimeInput(value: string): string {
  if (!value) return "";
  const date = new Date(value);
  if (!Number.isFinite(date.getTime())) return "";
  const local = new Date(date.getTime() - date.getTimezoneOffset() * 60000);
  return local.toISOString().slice(0, 16);
}

function formFromDefaults(defaults: ExperimentStartDefaults): Form {
  return {
    sample_id: defaults.sample_id,
    sample_name: defaults.sample_name,
    sample_weight:
      defaults.sample_weight != null && Number.isFinite(defaults.sample_weight)
        ? defaults.sample_weight.toFixed(4)
        : "",
    operator: defaults.operator,
    experiment_type: defaults.experiment_type,
    experiment_time: toDatetimeInput(defaults.experiment_time),
    project_name: defaults.project_name ?? "",
    notes: defaults.notes ?? "",
  };
}

function IssueList({
  title,
  items,
  tone,
}: {
  title: string;
  items: { code: string; message_zh: string }[];
  tone: "warn" | "err";
}) {
  if (!items.length) return null;
  return (
    <div
      className="rounded p-2.5 text-[12px]"
      style={{
        border: `1px solid ${tone === "err" ? "var(--danger)" : "var(--warn)"}`,
        background: tone === "err" ? "rgba(255,69,58,0.08)" : "rgba(214,164,25,0.08)",
      }}
    >
      <div className="mb-1 font-medium" style={{ color: tone === "err" ? "var(--danger)" : "var(--warn)" }}>
        {title}
      </div>
      <ul className="flex list-disc flex-col gap-1 pl-5" style={{ color: "var(--fg-dim)" }}>
        {items.map((item, i) => (
          <li key={`${item.code}-${i}`}>{item.message_zh}</li>
        ))}
      </ul>
    </div>
  );
}

function Field({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <>
      <label className="self-center text-[13px]" style={{ color: "var(--fg-dim)" }}>
        {label}
      </label>
      {children}
    </>
  );
}

export function StartExperimentDialog({ onClose }: { onClose: () => void }) {
  const [f, setF] = useState<Form>(empty);
  const [programs, setPrograms] = useState<ProgramDto[]>([]);
  const [preflight, setPreflight] = useState<ExperimentPreflightResult | null>(null);
  const [warningAcknowledged, setWarningAcknowledged] = useState(false);
  const [sampleIdManual, setSampleIdManual] = useState(false);
  const [busy, setBusy] = useState(false);
  const push = useToasts((s) => s.push);

  const loadDefaults = async (programId: string) => {
    const defaults = await getExperimentStartDefaults(programId);
    setF(formFromDefaults(defaults));
    setSampleIdManual(false);
    setPreflight(null);
    setWarningAcknowledged(false);
  };

  useEffect(() => {
    let cancelled = false;
    async function load() {
      const items = (await listPrograms()).filter((p) => p.enabled);
      if (cancelled) return;
      setPrograms(items);
      if (items[0]) await loadDefaults(items[0].id);
    }
    load().catch(toastError);
    return () => {
      cancelled = true;
    };
  }, []);

  const selected = programs.find((p) => p.id === f.experiment_type) ?? null;

  useEffect(() => {
    if (sampleIdManual || !f.experiment_type || !f.experiment_time) return;
    let cancelled = false;
    generateSampleId(f.experiment_type, f.experiment_time)
      .then((sample_id) => {
        if (!cancelled) setF((cur) => ({ ...cur, sample_id }));
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [f.experiment_type, f.experiment_time, sampleIdManual]);

  const upd = (k: keyof Form, v: string) => {
    setF((p) => ({ ...p, [k]: v }));
    if (k === "sample_id") setSampleIdManual(true);
    setPreflight(null);
    setWarningAcknowledged(false);
  };

  const inputStyle = {
    background: "var(--bg)",
    border: "1px solid var(--border)",
    color: "var(--fg)",
  };

  const input = (k: keyof Form, type = "text") => (
    <input
      type={type}
      readOnly={k === "operator" || k === "experiment_time"}
      value={f[k]}
      onChange={(e) => upd(k, e.target.value)}
      className="w-full rounded px-3 py-2 text-[13px]"
      style={inputStyle}
    />
  );

  const generateId = async () => {
    if (!f.experiment_type) return;
    try {
      const next = await generateSampleId(f.experiment_type, f.experiment_time || new Date().toISOString());
      setF((cur) => ({ ...cur, sample_id: next }));
      setSampleIdManual(false);
      setPreflight(null);
      setWarningAcknowledged(false);
    } catch (e) {
      toastError(e);
    }
  };

  const changeProgram = async (programId: string) => {
    setF((cur) => ({ ...cur, experiment_type: programId }));
    setPreflight(null);
    setWarningAcknowledged(false);
    if (!sampleIdManual) {
      try {
        const next = await generateSampleId(programId, f.experiment_time || new Date().toISOString());
        setF((cur) => ({ ...cur, experiment_type: programId, sample_id: next }));
      } catch (e) {
        toastError(e);
      }
    }
  };

  const params = (): ExperimentParams => ({
    sample_id: f.sample_id,
    sample_name: f.sample_name,
    sample_weight: Number(f.sample_weight),
    operator: f.operator,
    experiment_type: f.experiment_type,
    experiment_time: f.experiment_time || null,
    project_name: f.project_name || null,
    notes: f.notes || null,
  });

  const submit = async () => {
    setBusy(true);
    try {
      const p = params();
      const check = await preflightExperiment(p);
      setPreflight(check);
      if (!check.ok) return;
      if (check.warnings.length && !warningAcknowledged) {
        setWarningAcknowledged(true);
        return;
      }
      const id = await startExperiment(p);
      push(`实验已开始（${id}）`, "info");
      onClose();
    } catch (e) {
      toastError(e);
    } finally {
      setBusy(false);
    }
  };

  const canSubmit = !busy && !!selected;
  const submitLabel = busy
    ? "检查中..."
    : preflight?.warnings.length && warningAcknowledged
      ? "确认警告并开始"
      : "检查并开始";

  return (
    <Modal title="实验参数设置" onClose={onClose} wide>
      <div className="grid grid-cols-[minmax(360px,460px)_1fr] gap-4">
        <div className="flex flex-col gap-4">
          <section className="rounded p-3" style={{ border: "1px solid var(--border)" }}>
            <div className="section-title mb-3">基本信息</div>
            <div className="grid grid-cols-[86px_1fr] gap-x-3 gap-y-3">
              <Field label="项目名称">{input("project_name")}</Field>
              <Field label="样品名称">{input("sample_name")}</Field>
              <Field label="样品编号">
                <div className="grid grid-cols-[1fr_92px] gap-2">
                  {input("sample_id")}
                  <button
                    type="button"
                    onClick={generateId}
                    className="rounded px-3 py-2 text-[13px]"
                    style={{ background: "var(--panel-2)", border: "1px solid var(--border-bright)", color: "var(--fg)" }}
                  >
                    生成编号
                  </button>
                </div>
              </Field>
              <Field label="实验类型">
                <select
                  value={f.experiment_type}
                  onChange={(e) => void changeProgram(e.target.value)}
                  className="w-full rounded px-3 py-2 text-[13px]"
                  style={inputStyle}
                >
                  {programs.map((m) => (
                    <option key={m.id} value={m.id}>
                      {m.name}
                    </option>
                  ))}
                </select>
              </Field>
              <Field label="初重(g)">{input("sample_weight", "number")}</Field>
              <Field label="当前操作人员">{input("operator")}</Field>
              <Field label="启动时间（以实际开始为准）">{input("experiment_time", "datetime-local")}</Field>
            </div>
          </section>

          <section className="rounded p-3" style={{ border: "1px solid var(--border)" }}>
            <div className="section-title mb-3">备注说明</div>
            <textarea
              value={f.notes}
              onChange={(e) => upd("notes", e.target.value)}
              className="min-h-28 w-full rounded px-3 py-2 text-[13px]"
              style={inputStyle}
            />
          </section>

          <IssueList title="启动阻断项" items={preflight?.blocking ?? []} tone="err" />
          <IssueList title="启动警告项" items={preflight?.warnings ?? []} tone="warn" />
        </div>

        <div className="min-w-0">
          {selected ? (
            <ProgramPreview program={selected} compact />
          ) : (
            <div className="text-[12px]" style={{ color: "var(--fg-faint)" }}>
              暂无可用实验模式
            </div>
          )}
        </div>
      </div>

      <div className="mt-4 flex justify-end gap-2">
        <button
          type="button"
          onClick={() => f.experiment_type && void loadDefaults(f.experiment_type)}
          className="rounded px-4 py-2 text-[13px]"
          style={{ background: "var(--panel-2)", border: "1px solid var(--border-bright)", color: "var(--fg)" }}
        >
          重置
        </button>
        <button
          type="button"
          onClick={onClose}
          className="rounded px-4 py-2 text-[13px]"
          style={{ background: "var(--panel-2)", border: "1px solid var(--border-bright)", color: "var(--fg)" }}
        >
          取消
        </button>
        <button
          type="button"
          onClick={submit}
          disabled={!canSubmit}
          className="rounded px-4 py-2 text-[13px] font-medium"
          style={{ background: "var(--ok)", color: "#04140a", opacity: canSubmit ? 1 : 0.6 }}
        >
          {submitLabel}
        </button>
      </div>
    </Modal>
  );
}
