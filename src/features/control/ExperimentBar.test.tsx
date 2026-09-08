import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { ExperimentBar } from "./ExperimentBar";
import { useExperiment } from "../../stores/experiment";
import { useTelemetry } from "../../stores/telemetry";
import { useToasts } from "../../stores/toast";
import {
  retryPendingSave,
  getExperimentDetail,
  acknowledgeStage,
} from "../../ipc";
vi.mock("../../ipc", async (original) => ({
  ...(await original<typeof import("../../ipc")>()),
  retryPendingSave: vi.fn(),
  getExperimentDetail: vi.fn(),
  acknowledgeStage: vi.fn(),
}));
beforeEach(() => {
  vi.clearAllMocks();
  useExperiment.setState({
    state: { running: false, persistence_pending: true },
  });
  useTelemetry.setState({ snapshot: null });
  useToasts.setState({ toasts: [] });
});
afterEach(cleanup);
it("blocks a new run and retains the pending warning until native save succeeds", async () => {
  vi.mocked(retryPendingSave).mockRejectedValueOnce(new Error("磁盘空间不足"));
  render(<ExperimentBar />);
  expect((screen.getByText("开始实验") as HTMLButtonElement).disabled).toBe(
    true,
  );
  const retry = screen.getByRole("button", { name: "重试保存" });
  fireEvent.click(retry);
  await waitFor(() =>
    expect(
      useToasts.getState().toasts.some((t) => t.msg.includes("磁盘空间不足")),
    ).toBe(true),
  );
  expect(screen.getByRole("alert").textContent).toContain("尚未保存");
  expect(useExperiment.getState().state.persistence_pending).toBe(true);
});
it("offers retry without falsely clearing backend state in the UI", async () => {
  vi.mocked(retryPendingSave).mockResolvedValueOnce(undefined);
  render(<ExperimentBar />);
  fireEvent.click(screen.getByRole("button", { name: "重试保存" }));
  await waitFor(() => expect(retryPendingSave).toHaveBeenCalledOnce());
  expect(useExperiment.getState().state.persistence_pending).toBe(true);
});

it("shows protected manual instructions and submits the stage that the operator actually reviewed", async () => {
  const programs = (await import("../../../config/standard_programs.json"))
    .default;
  const program = programs.find((p) => p.id === "FREE_SWELLING")!;
  vi.mocked(getExperimentDetail).mockResolvedValue({
    program_snapshot: program,
  } as unknown as Awaited<ReturnType<typeof getExperimentDetail>>);
  vi.mocked(acknowledgeStage).mockResolvedValue(undefined);
  useExperiment.setState({
    state: {
      running: true,
      experiment_id: "run-a",
      phase: "RUNNING",
      stage_kind: "MANUAL",
      stage_index: 3,
      can_control: true,
    },
  });
  render(<ExperimentBar />);
  const button = screen.getByRole("button", { name: "确认人工步骤" });
  await waitFor(() =>
    expect((button as HTMLButtonElement).disabled).toBe(false),
  );
  expect(screen.queryByRole("button", { name: "跳过阶段" })).toBeNull();
  fireEvent.click(button);
  expect(screen.getByText(program.stages[3]!.description)).toBeTruthy();
  fireEvent.change(screen.getByRole("textbox", { name: "人工步骤备注" }), {
    target: { value: "已关炉取管" },
  });
  // The modal keeps the reviewed stage; a later status update cannot retarget the click.
  act(() =>
    useExperiment.setState({
      state: {
        running: true,
        experiment_id: "run-a",
        phase: "RUNNING",
        stage_kind: "MANUAL",
        stage_index: 5,
        can_control: true,
      },
    }),
  );
  fireEvent.click(
    screen.getByRole("button", { name: "已完成上述操作，确认继续" }),
  );
  await waitFor(() =>
    expect(acknowledgeStage).toHaveBeenCalledWith("run-a", 3, "已关炉取管"),
  );
});
