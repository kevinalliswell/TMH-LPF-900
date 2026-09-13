import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ExperimentDetail } from "../../ipc/types";
import { AnalyzePanel } from "./AnalyzePanel";

vi.mock("../../ipc", () => ({
  analyzeGb13240: vi.fn(),
  analyzeGb13241: vi.fn(),
  analyzeGb13242: vi.fn(),
}));

afterEach(cleanup);

const sampleEvidence = {
  version: "REDUCTION-SAMPLES-2026-09-13.1",
  scope: "stored_reduction_rows",
  total_sample_count: 12,
  reduction_sample_count: 10,
  valid_sample_count: 4,
  excluded_offline_rows: 2,
  excluded_other_quality_rows: 1,
  excluded_time_rows: 2,
  excluded_value_rows: 1,
  non_reduction_rows: 2,
  valid_baseline_count: 1,
};

const savedAnalysis = {
  standard: "GB/T 13241",
  standard_version: "2017",
  calculation_version: "GB-2026-09-05.1",
  ri: 61,
  rt: 61,
  endpoint_min: 180,
  reduction_rate_index: 1.1,
  result_scope: "single_trial",
};

function detailWith(analysis: Record<string, unknown>): ExperimentDetail {
  return {
    experiment_id: "sample-evidence-42",
    experiment_name: "还原性试验",
    sample_name: "试样 A",
    sample_weight: 100,
    experiment_type: "REDUCIBILITY",
    start_time: "2026-09-13T10:00:00+08:00",
    end_time: "2026-09-13T13:00:00+08:00",
    created_at: "2026-09-13T10:00:00+08:00",
    sample_count: 12,
    analysis,
  };
}

function visibleText(container: HTMLElement): string {
  return container.textContent?.replace(/\s+/g, "") ?? "";
}

describe("GB/T 13241 saved sampling evidence", () => {
  it("shows usable weighing rows and the Chinese exclusion breakdown", () => {
    const { container } = render(
      <AnalyzePanel
        detail={detailWith({ ...savedAnalysis, sample_evidence: sampleEvidence })}
        onAnalyzed={vi.fn()}
      />,
    );

    const text = visibleText(container);
    expect(text).toMatch(/还原曲线可用称量点[：:]?4(?!\d)/);
    expect(text).toMatch(/离线(?:排除|记录|行)?[：:]?2(?!\d)/);
    expect(text).toMatch(/其[他它]质量(?:标记排除|排除|异常|记录|行)?[：:]?1(?!\d)/);
    expect(text).toMatch(/(?:缺少阶段时间排除|时间(?:排除|无效|异常|记录|行)?)[：:]?2(?!\d)/);
    expect(text).toMatch(/(?:称量值|数值)(?:无法计算排除|排除|无效|异常|记录|行)?[：:]?1(?!\d)/);
    expect(text).toMatch(/非还原(?:段记录|阶段|记录|行)?[：:]?2(?!\d)/);
    expect(screen.queryByText("采样筛选统计：未记录")).toBeNull();
  });

  it("does not add manually supplied endpoint, baseline or rate points to stored usable rows", () => {
    const { container } = render(
      <AnalyzePanel
        detail={detailWith({
          ...savedAnalysis,
          sample_evidence: sampleEvidence,
          manual_used: true,
          input: {
            w1: 10,
            w2: 60,
            manual_m1: 1100,
            manual_mt: 1090,
            manual_rate_points: [[0, 30], [10, 60]],
            source_record: "LAB-MANUAL-42",
          },
        })}
        onAnalyzed={vi.fn()}
      />,
    );

    // This is a stored-row count, independent of how many manual points determine
    // the reported endpoint or reduction rate.
    expect(visibleText(container)).toMatch(/还原曲线可用称量点[：:]?4(?!\d)/);
  });

  it("marks legacy analyses as unrecorded rather than inventing zero usable samples", () => {
    const { container } = render(
      <AnalyzePanel detail={detailWith(savedAnalysis)} onAnalyzed={vi.fn()} />,
    );

    expect(screen.getByText("采样筛选统计：未记录")).toBeTruthy();
    expect(visibleText(container)).not.toMatch(/还原曲线可用称量点[：:]?0(?!\d)/);
  });
});
