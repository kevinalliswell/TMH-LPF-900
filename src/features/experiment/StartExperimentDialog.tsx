import { useState } from "react";
import { EXPERIMENT_MODES, startExperiment } from "../../ipc";
import { Modal } from "../../components/Modal";
import { toastError, useToasts } from "../../stores/toast";

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
  experiment_type: EXPERIMENT_MODES[0].id,
  project_name: "",
  notes: "",
};

export function StartExperimentDialog({ onClose }: { onClose: () => void }) {
  const [f, setF] = useState<Form>(empty);
  const [busy, setBusy] = useState(false);
  const push = useToasts((s) => s.push);
  const upd = (k: keyof Form, v: string) => setF((p) => ({ ...p, [k]: v }));

  const submit = async () => {
    setBusy(true);
    try {
      const id = await startExperiment({
        sample_id: f.sample_id,
        sample_name: f.sample_name,
        sample_weight: parseFloat(f.sample_weight) || 0,
        operator: f.operator,
        experiment_type: f.experiment_type,
        project_name: f.project_name || null,
        notes: f.notes || null,
      });
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

  return (
    <Modal title="开始实验" onClose={onClose}>
      <div className="grid grid-cols-2 gap-3">
        <label
          className="flex flex-col gap-1 text-[12px] col-span-2"
          style={{ color: "var(--fg-dim)" }}
        >
          实验模式
          <select
            value={f.experiment_type}
            onChange={(e) => upd("experiment_type", e.target.value)}
            className="px-2 py-1.5 rounded text-[13px]"
            style={{ background: "var(--bg)", border: "1px solid var(--border)", color: "var(--fg)" }}
          >
            {EXPERIMENT_MODES.map((m) => (
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
          disabled={busy}
          className="px-3 py-1.5 rounded text-[13px] font-medium"
          style={{ background: "var(--ok)", color: "#04140a", opacity: busy ? 0.6 : 1 }}
        >
          {busy ? "启动中…" : "开始"}
        </button>
      </div>
    </Modal>
  );
}
