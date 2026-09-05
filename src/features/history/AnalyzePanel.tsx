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
          style={{
            background: "var(--panel-2)",
            border: "1px solid var(--border-bright)",
            color: "var(--fg)",
          }}
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
  const metrics = [
    ["ri", "还原性 RI（R₁₈₀）", "%"],
    ["reduction_rate_index", "还原速率指数", "%/min"],
    ["rdi_plus_6_3", "RDI +6.3", "%"],
    ["rdi_plus_3_15", "RDI +3.15", "%"],
    ["rdi_minus_0_5", "RDI −0.5", "%"],
    ["swelling_index", "整份自由膨胀指数", "%"],
  ];
  return (
    <div className="rounded p-3 mb-3" style={{ background: "var(--panel-2)" }}>
      <div className="flex flex-wrap gap-6">
        {metrics
          .filter(([key]) => analysis[key!] !== undefined)
          .map(([key, label, unit]) => (
            <Metric
              key={key}
              label={label!}
              value={`${fmt(analysis[key!])} ${unit}`}
            />
          ))}
      </div>
      <p className="text-[12px] mt-2" style={{ color: "var(--warn)" }}>
        单次试验计算；最终结果仍需过程、外部步骤及重复性验收。
      </p>
      {!analysis.calculation_version && <p>旧版本计算结果须复核后重算。</p>}
      {analysis.external_conditions_met === false && (
        <p>转鼓或筛分记录偏离标准条件。</p>
      )}
    </div>
  );
}

const initialFields: Record<string, string> = {
  w1: "",
  w2: "",
  manual_m1: "",
  manual_m180: "",
  manual_rate_points: "",
  source_record: "",
  m0: "",
  m1: "",
  m2: "",
  m3: "",
  drum_rpm: "",
  drum_revolutions: "",
  sieve_seconds: "",
  v0: "",
  v1: "",
  pellet_count: "18",
  volume_method: "",
};
function number(value: string): number {
  if (!value.trim() || !Number.isFinite(Number(value)))
    throw new Error("请完整填写有效数值，缺测不能作为 0");
  return Number(value);
}
export function AnalyzePanel({
  detail,
  onAnalyzed,
}: {
  detail: ExperimentDetail;
  onAnalyzed: () => void;
}) {
  const [fields, setFields] = useState(initialFields);
  const [sameMethod, setSameMethod] = useState(false);
  const [busy, setBusy] = useState(false);
  const push = useToasts((s) => s.push);
  const type =
    detail.program_snapshot?.base_standard_id ?? detail.experiment_type;
  const field = (key: string, label: string, suffix?: string) => (
    <Field
      key={key}
      label={label}
      suffix={suffix}
      value={fields[key] ?? ""}
      onChange={(v) => setFields((f) => ({ ...f, [key]: v }))}
    />
  );
  const submit = async () => {
    setBusy(true);
    try {
      const n = (key: string) => number(fields[key] ?? "");
      if (type === "REDUCIBILITY") {
        const manual = fields.manual_rate_points?.trim();
        const points = manual
          ? manual.split(/[;；\n]+/).map((line) => {
              const pair = line.split(/[,，]/);
              if (pair.length !== 2)
                throw new Error("速率曲线每行需为时间(min),还原度(%)");
              return [number(pair[0]!), number(pair[1]!)] as [number, number];
            })
          : null;
        await analyzeGb13241(detail.experiment_id, {
          w1: n("w1"),
          w2: n("w2"),
          manual_m1: fields.manual_m1?.trim() ? n("manual_m1") : null,
          manual_m180: fields.manual_m180?.trim() ? n("manual_m180") : null,
          manual_rate_points: points,
          source_record: fields.source_record,
        });
      }
      if (type === "LOW_TEMP_DEGRADATION")
        await analyzeGb13242(detail.experiment_id, {
          m0: n("m0"),
          m1: n("m1"),
          m2: n("m2"),
          m3: n("m3"),
          drum_rpm: n("drum_rpm"),
          drum_revolutions: n("drum_revolutions"),
          sieve_seconds: n("sieve_seconds"),
        });
      if (type === "FREE_SWELLING")
        await analyzeGb13240(detail.experiment_id, {
          v0: n("v0"),
          v1: n("v1"),
          pellet_count: n("pellet_count"),
          volume_method: fields.volume_method ?? "",
          same_method: sameMethod,
        });
      push("单次计算已保存，原始输入与计算版本已归档", "info");
      onAnalyzed();
    } catch (e) {
      toastError(e);
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="panel p-4">
      <SectionTitle>结果分析</SectionTitle>
      {detail.analysis && (
        <ResultBlock analysis={detail.analysis as Record<string, unknown>} />
      )}
      {type === "REDUCIBILITY" && (
        <>
          <p className="text-[12px] mb-3">
            m₀ 为干试料质量；m₁ 自动取还原开始前有效总称量（含还原管）。W₁、W₂
            按质量百分数输入，例如 10、60。
          </p>
          <div className="grid grid-cols-2 gap-3 mb-3">
            {field("w1", "W₁ 亚铁含量（GB/T 6730.8）", "%")}
            {field("w2", "W₂ 全铁含量（GB/T 6730.5）", "%")}
          </div>
          <details className="mb-3">
            <summary>人工补录缺失称量或速率数据（可选）</summary>
            <div className="grid grid-cols-2 gap-3 mt-2">
              {field("manual_m1", "m₁ 还原开始前总称量", "g")}
              {field("manual_m180", "m₁₈₀ 还原 180 分钟总称量", "g")}
              {field(
                "manual_rate_points",
                "速率数据：时间(min),还原度(%)；分号分隔",
              )}
              {field("source_record", "原始记录编号 / 来源说明")}
            </div>
            <p className="text-[12px]">
              人工值优先于自动值，并在报告中标明。m₁ 与 m₁₈₀
              必须包含同一还原管。
            </p>
          </details>
        </>
      )}
      {type === "LOW_TEMP_DEGRADATION" && (
        <>
          <p className="text-[12px] mb-3">
            m₀ 需还原后、转鼓前重新称量。三个筛上级分分别称量；−0.5 mm
            指数按差量计算，包含未回收粉末。
          </p>
          <div className="grid grid-cols-3 gap-3 mb-3">
            {field("m0", "m₀ 还原后、转鼓前质量", "g")}
            {field("m1", "m₁ 6.3 mm 筛上质量", "g")}
            {field("m2", "m₂ 3.15 mm 筛上质量（不含 m₁）", "g")}
            {field("m3", "m₃ 0.5 mm 筛上质量（不含前两项）", "g")}
            {field("drum_rpm", "实际转鼓转速", "r/min")}
            {field("drum_revolutions", "实际转数", "r")}
            {field("sieve_seconds", "实际筛分时间", "s")}
          </div>
        </>
      )}
      {type === "FREE_SWELLING" && (
        <>
          <p className="text-[12px] mb-3">
            记录同一份 18 颗球团的还原前后总体积。
          </p>
          <div className="grid grid-cols-2 gap-3 mb-3">
            {field("v0", "V₀ 整份还原前体积", "mL")}
            {field("v1", "V₁ 整份还原后体积", "mL")}
            {field("pellet_count", "本份颗粒数")}
            {field("volume_method", "体积测定方法与记录编号")}
          </div>
          <label className="text-[12px]">
            <input
              type="checkbox"
              checked={sameMethod}
              onChange={(e) => setSameMethod(e.target.checked)}
            />{" "}
            确认还原前后采用相同测定方法
          </label>
        </>
      )}
      <div className="mt-3">
        <button
          disabled={busy || !detail.end_time}
          onClick={submit}
          className="px-3 py-1.5 rounded text-[13px]"
          style={{ background: "var(--gas)", color: "#04140f" }}
        >
          {busy ? "计算中…" : "保存单次计算"}
        </button>
        {!detail.end_time && <span> 实验结束后可分析</span>}
      </div>
    </div>
  );
}
