import {
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
import { retryPendingSave } from "../../ipc";
vi.mock("../../ipc", async (original) => ({
  ...(await original<typeof import("../../ipc")>()),
  retryPendingSave: vi.fn(),
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
