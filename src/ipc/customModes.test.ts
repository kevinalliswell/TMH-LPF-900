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
    expect(() => m.mockSaveCustomProgram(draft)).toThrow("AUTH_REQUIRED");
    auth.mockBootstrapAdmin("admin", "管理员", "test-password");
    const saved = m.mockSaveCustomProgram(draft);
    saved.stages[0]!.target_temp = 750;
    m.mockSaveCustomProgram(saved);
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
      const saved = m.mockSaveCustomProgram({
        ...m.mockListPrograms()[0]!,
        id: "",
        source: "custom",
        name: "温度变体",
      });
      const id = m.mockStartExperiment({
        sample_id: "S1",
        sample_name: "sample",
        sample_weight: 500,
        experiment_type: saved.id,
      });
      expect(() => m.mockSkipStage(id, 0, "")).toThrow("跳段原因必填");
      expect(() => m.mockSaveCustomProgram(saved)).toThrow(
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
