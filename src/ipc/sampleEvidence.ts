import type { ReductionSampleEvidence } from "./types";

const fields = [
  ["total_sample_count", "分析时存储记录"],
  ["reduction_sample_count", "还原段记录"],
  ["valid_sample_count", "还原曲线可用称量点"],
  ["excluded_offline_rows", "天平离线排除"],
  ["excluded_other_quality_rows", "其他质量标记排除"],
  ["excluded_time_rows", "缺少阶段时间排除"],
  ["excluded_value_rows", "称量值无法计算排除"],
  ["non_reduction_rows", "非还原段记录"],
  ["valid_baseline_count", "其中基准称量点"],
] as const;

export const SAMPLE_EVIDENCE_NOTE = "统计覆盖分析时存储的全部还原段记录（含终点后记录），不代表决定终点或速率的点数。人工补录、插值点和缺测断点不计入采样数量；基准称量是可用点的子集。统计不能推定全过程符合国标。";

function isEvidence(value: unknown): value is ReductionSampleEvidence {
  if (!value || typeof value !== "object") return false;
  const record = value as Record<string, unknown>;
  return record.version === "REDUCTION-SAMPLES-2026-09-13.1"
    && record.scope === "stored_reduction_rows"
    && fields.every(([key]) => typeof record[key] === "number"
      && Number.isSafeInteger(record[key]) && record[key] >= 0);
}

/** Legacy/malformed evidence remains unknown instead of silently becoming zero. */
export function sampleEvidenceRows(analysis: Record<string, unknown>): readonly (readonly [string, number])[] | null {
  const evidence = analysis.sample_evidence;
  return isEvidence(evidence) ? fields.map(([key, label]) => [label, evidence[key]] as const) : null;
}
