import { beforeEach, expect, it, vi } from "vitest";
import type { ReportDetails } from "./types";
beforeEach(() => vi.resetModules());
it("calculates manual reduction data, preserves provenance, and exports escaped supplements", async () => {
  const m = await import("./mock");
  const auth = await import("./authMock");
  auth.mockBootstrapAdmin("admin", "测试员", "test-password");
  const input = {
    w1: 10,
    w2: 60,
    manual_m1: 1100,
    manual_m180: 1090,
    manual_rate_points: [
      [0, 30],
      [10, 60],
    ] as [number, number][],
    source_record: "LAB-001",
  };
  expect(() =>
    m.mockAnalyze13241("exp-demo-2", { ...input, source_record: "" }),
  ).toThrow("来源");
  expect(() => m.mockAnalyze13241("exp-demo-1", input)).toThrow("方法不匹配");
  const result = m.mockAnalyze13241("exp-demo-2", input);
  expect(result.rt).toBeCloseTo(43.0620155039, 8);
  expect(result.reduction_rate_index).toBeCloseTo(3.36, 10);
  const details: ReportDetails = {
    lab_name: "<script>Lab</script>",
    lab_address: "地址",
    report_number: "R-001",
    report_date: "2026-09-05",
    sample_preparation: "",
    equipment: "",
    gas_and_calibration: "",
    external_steps: "",
    deviations: "",
    reviewer: "",
    repeat_group_reference: "",
  };
  m.mockSaveReportDetails("exp-demo-2", details);
  m.mockSaveReportDetails("exp-demo-2", { ...details, lab_name: "Lab B" });
  const history = m.mockGetExperimentDetail("exp-demo-2");
  expect(history.revisions).toHaveLength(3);
  expect(
    (history.revisions?.[1]?.report_details as ReportDetails).lab_name,
  ).toBe("<script>Lab</script>");
  m.mockSaveReportDetails("exp-demo-2", details);
  const report = m.mockReportHtml("exp-demo-2");
  expect(report).toContain("&lt;script&gt;");
  expect(report).not.toContain("<script>");
  expect(report).toContain("LAB-001");
});
