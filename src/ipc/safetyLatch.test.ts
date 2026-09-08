import { beforeEach, expect, it, vi } from "vitest";
beforeEach(() => vi.resetModules());
it("emergency stops a run and refuses gas changes until an explicit reset", async () => {
  vi.useFakeTimers();
  try {
    const m = await import("./mock");
    const auth = await import("./authMock");
    auth.mockBootstrapAdmin("admin", "测试员", "test-password");
    m.mockStartExperiment({
      sample_id: "S1",
      sample_name: "sample",
      sample_weight: 500,
      experiment_type: "REDUCIBILITY",
    });
    m.mockShutoff();
    expect(() => m.mockSetFlow(0, 3)).toThrow("锁存");
    expect(() => m.mockSetOnOff(3, true)).toThrow("锁存");
    m.mockResetSafety();
    expect(m.mockSetFlow(0, 3).applied).toBe(3);
  } finally {
    vi.useRealTimers();
  }
});
