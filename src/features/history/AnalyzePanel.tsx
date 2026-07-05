import { useState } from "react";
import { analyzeGb13240, analyzeGb13241, analyzeGb13242 } from "../../ipc";
import type { ExperimentDetail } from "../../ipc/types";
import { toastError, useToasts } from "../../stores/toast";
import { SectionTitle } from "../../components/Page";

function fmt(v: unknown, digits = 2): string {
  return typeof v === "number" && Number.isFinite(v) ? v.toFixed(digits) : "—";
}

function Field({
  label,
  value,
  onChange,
  suffix,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  suffix?: string;
}) {
  return (
    <label className="flex flex-col gap-1 text-[12px]">
      <span style={{ color: "var(--fg-faint)" }}>{label}</span>
      <div className="flex items-center gap-1">
        <input
          value={value}
          onChange={(e) => onChange(e.target.value)}
          inputMode="decimal"
          className="mono w-full px-2 py-1 rounded outline-none"
          style={{ background: "var(--panel-2)", border: "1px solid var(--border-bright)", color: "var(--fg)" }}
        />
        {suffix && (
          <span className="text-[11px]" style={{ color: "var(--fg-faint)" }}>
            {suffix}
          </span>
        )}
      </div>
    </label>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex flex-col">
      <span className="text-[11px]" style={{ color: "var(--fg-faint)" }}>
        {label}
      </span>
      <span className="mono text-[16px]" style={{ color: "var(--heat)" }}>
        {value}
      </span>
    </div>
  );
}

function ResultBlock({ analysis }: { analysis: Record<string, unknown> }) {
  const std = analysis.standard;
  return (
    <div className="rounded p-3 mb-3" style={{ background: "var(--panel-2)" }}>
      <div className="flex flex-wrap gap-6">
        {std === "GB/T 13241" && (
          <>
            <Metric label="还原度 Rt" value={`${fmt(analysis.rt)} %`} />
            <Metric label="失重 ΔW" value={`${fmt(analysis.delta_w)} g`} />
            <Metric label="还原速率指数" value={`${fmt(analysis.reduction_rate_index, 3)} %/min`} />
          </>
        )}
        {std === "GB/T 13242" && (
          <>
            <Metric label="RDI +6.3" value={`${fmt(analysis.rdi_plus_6_3)} %`} />
            <Metric label="RDI +3.15" value={`${fmt(analysis.rdi_plus_3_15)} %`} />
            <Metric label="RDI −0.5" value={`${fmt(analysis.rdi_minus_0_5)} %`} />
          </>
        )}
        {std === "GB/T 13240" && (
          <Metric label="平均自由膨胀指数" value={`${fmt(analysis.average)} %`} />
        )}
      </div>
      {analysis.sieve_warning === true && (
        <div className="mt-2 text-[12px]" style={{ color: "var(--warn)" }}>
          ⚠ 筛分总质量与 m₀ 偏差超过 2%，请复核。
        </div>
      )}
      {std === "GB/T 13240" && Array.isArray(analysis.per_pellet) && (
        <div className="mt-2 mono text-[12px]" style={{ color: "var(--fg-dim)" }}>
          单球: {(analysis.per_pellet as number[]).map((v) => v.toFixed(1)).join(" · ")} %
        </div>
      )}
      {typeof analysis.computed_at === "string" && (
        <div className="mt-2 mono text-[11px]" style={{ color: "var(--fg-faint)" }}>
          计算于 {analysis.computed_at.replace("T", " ").slice(0, 19)}
        </div>
      )}
    </div>
  );
}

export function AnalyzePanel({
  detail,
  onAnalyzed,
}: {
  detail: ExperimentDetail;
  onAnalyzed: () => void;
}) {
  const push = useToasts((s) => s.push);
  const [busy, setBusy] = useState(false);
  // 13241
  const [o2, setO2] = useState("28.5");
  // 13242
  const [m0, setM0] = useState(String(detail.sample_weight));
  const [mGt63, setMGt63] = useState("");
  const [m31563, setM31563] = useState("");
  const [m05315, setM05315] = useState("");
  const [mLt05, setMLt05] = useState("");
  // 13240
  const [volumes, setVolumes] = useState("");

  const run = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await fn();
      push("分析完成，结果已保存", "info");
      onAnalyzed();
    } catch (e) {
      toastError(e);
    } finally {
      setBusy(false);
    }
  };

  const type = detail.experiment_type;
  const analysis = (detail.analysis ?? null) as Record<string, unknown> | null;

  return (
    <div className="panel p-4">
      <SectionTitle>结果分析</SectionTitle>
      {analysis && <ResultBlock analysis={analysis} />}

      {type === "REDUCIBILITY" && (
        <div className="flex items-end gap-3">
          <div className="w-40">
            <Field label="全铁含氧量 O₂%" value={o2} onChange={setO2} suffix="%" />
          </div>
          <button
            disabled={busy}
            onClick={() => run(() => analyzeGb13241(detail.experiment_id, Number(o2)))}
            className="px-3 py-1.5 rounded text-[13px]"
            style={{ background: "var(--gas)", color: "#04140f" }}
          >
            计算还原度
          </button>
        </div>
      )}

      {type === "LOW_TEMP_DEGRADATION" && (
        <>
          <div className="grid grid-cols-3 gap-3 mb-3">
            <Field label="m₀ 初始" value={m0} onChange={setM0} suffix="g" />
            <Field label="+6.3mm" value={mGt63} onChange={setMGt63} suffix="g" />
            <Field label="3.15–6.3mm" value={m31563} onChange={setM31563} suffix="g" />
            <Field label="0.5–3.15mm" value={m05315} onChange={setM05315} suffix="g" />
            <Field label="−0.5mm" value={mLt05} onChange={setMLt05} suffix="g" />
          </div>
          <button
            disabled={busy}
            onClick={() =>
              run(() =>
                analyzeGb13242(detail.experiment_id, {
                  m0: Number(m0),
                  mGt63: Number(mGt63),
                  m31563: Number(m31563),
                  m05315: Number(m05315),
                  mLt05: Number(mLt05),
                }),
              )
            }
            className="px-3 py-1.5 rounded text-[13px]"
            style={{ background: "var(--gas)", color: "#04140f" }}
          >
            计算 RDI
          </button>
        </>
      )}

      {type === "FREE_SWELLING" && (
        <>
          <Field
            label="试样体积对 V0,V1（每行一对，逗号分隔）"
            value={volumes}
            onChange={setVolumes}
          />
          <div className="text-[11px] mt-1 mb-2" style={{ color: "var(--fg-faint)" }}>
            例：10,12 换行 10,11
          </div>
          <button
            disabled={busy}
            onClick={() =>
              run(() => {
                const pairs = volumes
                  .split(/[\n;]+/)
                  .map((line) => line.split(",").map((x) => Number(x.trim())))
                  .filter((p) => p.length === 2 && p.every((n) => Number.isFinite(n)))
                  .map((p) => [p[0], p[1]] as [number, number]);
                if (!pairs.length) throw new Error("请输入至少一组有效体积");
                return analyzeGb13240(detail.experiment_id, pairs);
              })
            }
            className="px-3 py-1.5 rounded text-[13px]"
            style={{ background: "var(--gas)", color: "#04140f" }}
          >
            计算自由膨胀指数
          </button>
        </>
      )}
    </div>
  );
}
