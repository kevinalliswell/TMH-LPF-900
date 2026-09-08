import { beforeEach, describe, expect, it, vi } from "vitest";

beforeEach(() => vi.resetModules());
describe("custom mode workflow", () => {
  it("refuses anonymous edits and leaves standard templates unchanged after edits", async () => {
    const m = await import("./mock");
    const auth = await import("./authMock");
    const standard = m.mockListPrograms()[0]!;
    const draft = {
      ...structuredClone(standard),
      id: "",
      source: "custom" as const,
    };
    expect(() => m.mockSaveCustomProgram(draft, "创建程序")).toThrow(
      "AUTH_REQUIRED",
    );
    auth.mockBootstrapAdmin("admin", "管理员", "test-password");
    const saved = m.mockSaveCustomProgram(draft, "创建程序");
    saved.stages[0]!.target_temp = 750;
    m.mockSaveCustomProgram(saved, "温度调整");
    expect(
      m.mockListPrograms().find((p) => p.id === saved.id)?.stages[0]
        ?.target_temp,
    ).toBe(750);
    expect(m.mockListPrograms()[0]?.stages[0]?.target_temp).toBe(
      standard.stages[0]?.target_temp,
    );
    m.mockDeleteCustomProgram(saved.id);
    expect(m.mockListPrograms().find((p) => p.id === saved.id)).toBeUndefined();
  });

  it("archives the executed custom program and requires a reason to skip", async () => {
    vi.useFakeTimers();
    try {
      const m = await import("./mock");
      const auth = await import("./authMock");
      auth.mockBootstrapAdmin("admin", "管理员", "test-password");
      const saved = m.mockSaveCustomProgram(
        {
          ...m.mockListPrograms()[0]!,
          id: "",
          source: "custom",
          name: "温度变体",
        },
        "研究温度影响",
      );
      const id = m.mockStartExperiment({
        sample_id: "S1",
        sample_name: "sample",
        sample_weight: 500,
        experiment_type: saved.id,
      });
      expect(() => m.mockSkipStage(id, 0, "")).toThrow("跳段原因必填");
      expect(() => m.mockSaveCustomProgram(saved, "温度调整")).toThrow(
        "实验运行中不可修改模式",
      );
      expect(m.mockGetExperimentDetail(id).program_snapshot?.name).toBe(
        "温度变体",
      );
      expect(m.mockGetExperimentDetail(id).end_time).toBeNull();
      m.mockStopExperiment();
      expect(m.mockGetExperimentDetail(id).program_snapshot?.name).toBe(
        "温度变体",
      );
    } finally {
      vi.useRealTimers();
    }
  });
});

it("requires a reason, stamps actual actor, rejects stale revisions and retains history", async () => {
  const m = await import("./mock");
  const auth = await import("./authMock");
  const user = auth.mockBootstrapAdmin("admin", "实际编辑者", "test-password");
  const draft = {
    ...m.mockListPrograms()[0]!,
    id: "",
    source: "custom" as const,
  };
  expect(() => m.mockSaveCustomProgram(draft, " ")).toThrow("修改原因");
  const first = m.mockSaveCustomProgram(draft, "温度比较");
  expect(first.provenance?.edited_by).toBe("实际编辑者");
  expect(first.provenance?.edited_by_user_id).toBe(user.user_id);
  const second = m.mockSaveCustomProgram(
    {
      ...first,
      stages: first.stages.map((s, i) =>
        i === 2 ? { ...s, target_temp: 850 } : s,
      ),
    },
    "850 ℃比较",
  );
  expect(second.provenance?.revision).toBe(2);
  expect(second.provenance?.changes).toContainEqual({
    stage_index: 2,
    field: "target_temp",
    before: 900,
    after: 850,
  });
  expect(() => m.mockSaveCustomProgram(first, "旧版本")).toThrow("版本已变化");
  m.mockDeleteCustomProgram(first.id);
  expect(m.mockListCustomProgramRevisions(first.id)).toEqual([second, first]);
});

it("rejects altered endings and conditions that would be clamped or rounded", async () => {
  const m = await import("./mock");
  const auth = await import("./authMock");
  auth.mockBootstrapAdmin("admin", "管理员", "test-password");
  for (const [stageIndex, gasIndex, flow] of [
    [3, 1, 5],
    [2, 0, 6],
    [2, 1, 10.55],
  ]) {
    const draft = structuredClone(m.mockListPrograms()[0]!);
    draft.id = "";
    draft.source = "custom";
    draft.stages[stageIndex!]!.gas[gasIndex!] = flow!;
    expect(() => m.mockSaveCustomProgram(draft, "调整气氛")).toThrow();
  }
});

it("uses the custom endpoint in manual analysis and escapes snapshot provenance in reports", async () => {
  const m = await import("./mock");
  const auth = await import("./authMock");
  auth.mockBootstrapAdmin("admin", "Lab <A>", "test-password");
  const draft = structuredClone(m.mockListPrograms()[0]!);
  draft.id = "";
  draft.source = "custom";
  draft.stages[2]!.duration_min = 60;
  const saved = m.mockSaveCustomProgram(draft, "Compare <60>");
  const id = m.mockStartExperiment({
    sample_id: "S-C",
    sample_name: "custom",
    sample_weight: 500,
    experiment_type: saved.id,
  });
  m.mockStopExperiment();
  const input = {
    w1: 10,
    w2: 60,
    manual_m1: 1500,
    manual_mt: 1450,
    source_record: "60 min record",
  };
  const result = m.mockAnalyze13241(id, input);
  expect(result.endpoint_min).toBe(60);
  expect(result.ri).toBeUndefined();
  expect(result.rt).toBeCloseTo(43.06201550387597, 8);
  expect(() =>
    m.mockAnalyze13241(id, { ...input, manual_mt: null, manual_m180: 1450 }),
  ).toThrow("180 分钟");
  const html = m.mockReportHtml(id);
  expect(html).toContain("Compare &lt;60&gt;");
  expect(html).toContain("Lab &lt;A&gt;");
  expect(html).toContain("基础国标条件");
  expect(html).toContain("不能据此声明符合国标方法");
});
