import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { ExperimentState } from "./types";
let mock: typeof import("./mock");
let state: ExperimentState;
beforeEach(async () => {
  vi.resetModules();
  vi.useFakeTimers();
  vi.spyOn(Math, "random").mockReturnValue(0.5);
  const auth = await import("./authMock");
  auth.mockBootstrapAdmin("procedure", "流程测试员", "procedure-test-2026");
  mock = await import("./mock");
  mock.mockSubscribeExperiment((s) => {
    state = s;
  });
  mock.mockTelemetry(() => {});
});
afterEach(() => {
  vi.clearAllTimers();
  vi.useRealTimers();
  vi.restoreAllMocks();
});
function start(mode: string) {
  return mock.mockStartExperiment({
    sample_id: "QA",
    sample_name: "模拟流程验证",
    sample_weight: 500,
    experiment_type: mode,
  });
}
function until(kind: string) {
  for (let i = 0; i < 1000 && state.stage_kind !== kind && state.running; i++)
    vi.advanceTimersByTime(400);
  expect(state.stage_kind).toBe(kind);
}
it("RI waits for an explicit tube-removal acknowledgement and ends with nitrogen off", () => {
  const id = start("REDUCIBILITY");
  until("MANUAL");
  const index = state.stage_index!;
  vi.advanceTimersByTime(60000);
  expect(state.running).toBe(true);
  expect(state.stage_index).toBe(index);
  expect(() => mock.mockSkipStage(id, index, "不能跳过")).toThrow();
  expect(() => mock.mockAcknowledgeStage("old-run", index, "")).toThrow();
  mock.mockAcknowledgeStage(id, index, "还原管已取出");
  expect(state.running).toBe(false);
  expect(mock.tick().gas.map((g) => g.sv)).toEqual([0, 0, 0, 0]);
  const detail = mock.mockGetExperimentDetail(id);
  expect(detail.runtime?.outcome).toBe("completed");
  expect(detail.runtime?.stage_actions).toEqual([
    expect.objectContaining({
      experiment_id: id,
      stage_index: index,
      notes: "还原管已取出",
      operator: "流程测试员",
    }),
  ]);
  expect(() => mock.mockAcknowledgeStage(id, index, "duplicate")).toThrow();
});
it("FSI waits for both external operations and rejects a stale first acknowledgement", () => {
  const id = start("FREE_SWELLING");
  until("MANUAL");
  const first = state.stage_index!;
  mock.mockAcknowledgeStage(id, first, "关炉取管");
  until("MANUAL");
  expect(state.stage_index).toBeGreaterThan(first);
  expect(() => mock.mockAcknowledgeStage(id, first, "duplicate")).toThrow();
  expect(mock.tick().temps[7]!).toBeLessThan(50);
  mock.mockAcknowledgeStage(id, state.stage_index!, "达到室温");
  expect(state.running).toBe(false);
  expect(mock.tick().gas.map((g) => g.sv)).toEqual([0, 5, 0, 0]);
});
