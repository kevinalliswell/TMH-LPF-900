import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import * as ipc from "../../ipc";
import { useExperiment } from "../../stores/experiment";
import { useTelemetry } from "../../stores/telemetry";
import { ControlPanel } from "./ControlPanel";

vi.mock("../../ipc", async (original) => ({
  ...await original<typeof ipc>(),
  getGasSafetyLimits: vi.fn().mockResolvedValue({ h2: 5, co: 5, flow_steps: [0.01, 0.1, 0.01, 0.01] }),
  tareBalance: vi.fn(), setGasFlow: vi.fn(), shutoffFlammableGases: vi.fn(),
}));
beforeEach(() => {
  vi.clearAllMocks();
  useExperiment.setState({ state: { running: false } });
  useTelemetry.setState({ snapshot: null });
  vi.mocked(ipc.tareBalance).mockResolvedValue({ success: true, a00_found: true, weight: 0 });
});
afterEach(cleanup);

it("blocks tare while running or awaiting persistence and explains the mass basis", async () => {
  useExperiment.setState({ state: { running: true, can_control: true } });
  await act(async () => { render(<ControlPanel />); });
  const button = screen.getByRole("button", { name: "去皮 (Tare)" }) as HTMLButtonElement;
  expect(button.disabled).toBe(true);
  expect(screen.getByText(/运行期间不可去皮.*称量基准/)).toBeTruthy();
  fireEvent.click(button);
  expect(ipc.tareBalance).not.toHaveBeenCalled();
  act(() => useExperiment.setState({ state: { running: false, persistence_pending: true } }));
  expect(button.disabled).toBe(true);
  expect(screen.getByText(/待保存数据/)).toBeTruthy();
  act(() => useExperiment.setState({ state: { running: false } }));
  expect(button.disabled).toBe(false);
});

it("awaits an idle tare acknowledgement and reports its confirmed result", async () => {
  render(<ControlPanel />);
  fireEvent.click(screen.getByRole("button", { name: "去皮 (Tare)" }));
  await waitFor(() => expect(ipc.tareBalance).toHaveBeenCalledOnce());
  expect(await screen.findByText("去皮成功（0.000 g）")).toBeTruthy();
});

it("disables ordinary controls for another owner or an update, leaving no actionable tare", async () => {
  useExperiment.setState({ state: { running: true, can_control: false, occupied_by_other: true } });
  await act(async () => { render(<ControlPanel />); });
  for (const button of screen.getAllByRole("button")) expect((button as HTMLButtonElement).disabled).toBe(true);
  expect(screen.getAllByRole("spinbutton").every((input) => (input as HTMLInputElement).disabled)).toBe(true);
  act(() => useExperiment.setState({ state: { running: false, updating: true } }));
  expect((screen.getByRole("button", { name: "去皮 (Tare)" }) as HTMLButtonElement).disabled).toBe(true);
});
