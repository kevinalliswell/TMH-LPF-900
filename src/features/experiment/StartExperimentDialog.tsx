import { useEffect, useState } from "react";
import { listPrograms, preflightExperiment, startExperiment } from "../../ipc";
import type { ExperimentParams, ExperimentPreflightResult, ProgramDto } from "../../ipc/types";
import { Modal } from "../../components/Modal";
import { toastError, useToasts } from "../../stores/toast";
import { ProgramPreview } from "./ProgramPreview";

type Form = {
  sample_id: string;
  sample_name: string;
  sample_weight: string;
  operator: string;
  experiment_type: string;
  project_name: string;
  notes: string;
};

const empty: Form = {
  sample_id: "",
  sample_name: "",
  sample_weight: "",
  operator: "",
  experiment_type: "REDUCIBILITY",
  project_name: "",
  notes: "",
};

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
      <div className="font-medium mb-1" style={{ color: tone === "err" ? "var(--danger)" : "var(--warn)" }}>
        {title}
      </div>
      <ul className="list-disc pl-5 flex flex-col gap-1" style={{ color: "var(--fg-dim)" }}>
        {items.map((item, i) => (
          <li key={`${item.code}-${i}`}>{item.message_zh}</li>
        ))}
      </ul>
    </div>
  );
}

export function StartExperimentDialog({ onClose }: { onClose: () => void }) {
  const [f, setF] = useState<Form>(empty);
  const [programs, setPrograms] = useState<ProgramDto[]>([]);
  const [preflight, setPreflight] = useState<ExperimentPreflightResult | null>(null);
  const [warningAcknowledged, setWarningAcknowledged] = useState(false);
  const [busy, setBusy] = useState(false);
  const push = useToasts((s) => s.push);

  useEffect(() => {
    listPrograms()
      .then((items) => {
        setPrograms(items);
        setF((cur) => ({
          ...cur,
          experiment_type: items.some((p) => p.id === cur.experiment_type)
            ? cur.experiment_type
            : items[0]?.id ?? cur.experiment_type,
        }));
      })
      .catch(toastError);
  }, []);

  const selected = programs.find((p) => p.id === f.experiment_type) ?? null;

  const upd = (k: keyof Form, v: string) => {
    setF((p) => ({ ...p, [k]: v }));
    setPreflight(null);
    setWarningAcknowledged(false);
  };

  const params = (): ExperimentParams => ({
    sample_id: f.sample_id,
    sample_name: f.sample_name,
    sample_weight: Number(f.sample_weight),
    operator: f.operator,
    experiment_type: f.experiment_type,
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

  const field = (label: string, k: keyof Form, type = "text") => (
    <label className="flex flex-col gap-1 text-[12px]" style={{ color: "var(--fg-dim)" }}>
      {label}
      <input
        type={type}
        value={f[k]}
        onChange={(e) => upd(k, e.target.value)}
        className="mono px-2 py-1.5 rounded text-[13px]"
        style={{ background: "var(--bg)", border: "1px solid var(--border)", color: "var(--fg)" }}
      />
    </label>
  );

  const canSubmit = !busy && !!selected;
  const submitLabel = busy
    ? "检查中…"
    : preflight?.warnings.length && warningAcknowledged
      ? "确认警告并开始"
      : "检查并开始";

  return (
    <Modal title="开始实验" onClose={onClose} wide>
      <div className="grid grid-cols-[300px_1fr] gap-4">
        <div className="flex flex-col gap-3">
          <label className="flex flex-col gap-1 text-[12px]" style={{ color: "var(--fg-dim)" }}>
            实验模式
            <select
              value={f.experiment_type}
              onChange={(e) => upd("experiment_type", e.target.value)}
              className="px-2 py-1.5 rounded text-[13px]"
              style={{ background: "var(--bg)", border: "1px solid var(--border)", color: "var(--fg)" }}
            >
              {programs.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.name}
                </option>
              ))}
            </select>
          </label>
          {field("样品编号 *", "sample_id")}
          {field("样品名称 *", "sample_name")}
          {field("样品重量 g *", "sample_weight", "number")}
          {field("操作员 *", "operator")}
          {field("项目名", "project_name")}
          {field("备注", "notes")}

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

      <div className="flex justify-end gap-2 mt-4">
        <button
          onClick={onClose}
          className="px-3 py-1.5 rounded text-[13px]"
          style={{ background: "var(--panel-2)", border: "1px solid var(--border-bright)", color: "var(--fg)" }}
        >
          取消
        </button>
        <button
          onClick={submit}
          disabled={!canSubmit}
          className="px-3 py-1.5 rounded text-[13px] font-medium"
          style={{ background: "var(--ok)", color: "#04140a", opacity: canSubmit ? 1 : 0.6 }}
        >
          {submitLabel}
        </button>
      </div>
    </Modal>
  );
}
