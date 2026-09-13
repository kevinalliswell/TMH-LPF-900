import { SAMPLE_EVIDENCE_NOTE, sampleEvidenceRows } from "../../ipc/sampleEvidence";

export function AnalysisSampleEvidence({ analysis }: { analysis: Record<string, unknown> }) {
  if (analysis.standard !== "GB/T 13241") return null;
  const rows = sampleEvidenceRows(analysis);
  if (!rows) return <p className="text-xs mt-3">采样筛选统计：未记录</p>;
  return <section className="mt-4 text-xs">
    <h3 className="font-semibold mb-2">采样筛选统计</h3>
    <dl className="grid grid-cols-2 gap-x-6 gap-y-1">
      {rows.map(([label, value]) => <div key={label} className="flex justify-between gap-3">
        <dt style={{ color: "var(--fg-dim)" }}>{label}</dt>
        <dd className="mono">{value}</dd>
      </div>)}
    </dl>
    <p className="mt-2" style={{ color: "var(--fg-dim)" }}>{SAMPLE_EVIDENCE_NOTE}</p>
  </section>;
}
