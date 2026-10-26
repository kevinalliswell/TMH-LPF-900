import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import * as ipc from "../../ipc";
import { useTelemetry } from "../../stores/telemetry";
import { ControlPanel } from "./ControlPanel";
import { MonitorPanel } from "./MonitorPanel";

vi.mock("../../ipc", async (original) => ({
  ...await original<typeof ipc>(),
  getGasSafetyLimits: vi.fn().mockResolvedValue({h2: 5, co: 5, flow_steps: [0.01, 0.1, 0.01, 0.01]}),
  setGasFlow: vi.fn().mockResolvedValue({applied: 1.24, clamped: false, limit: 5}),
}));
afterEach(cleanup);
it("displays the acknowledged setpoint without hiding hundredths", async () => {
  render(<ControlPanel />);
  fireEvent.change(screen.getAllByRole("spinbutton")[0]!, {target: {value: "1.237"}});
  fireEvent.click(screen.getAllByRole("button", {name: "设定"})[0]!);
  expect(await screen.findByText("H₂ 设定为 1.24 L/min")).toBeTruthy();
});
it("preserves setpoint resolution in telemetry, including values below 0.01", () => {
  useTelemetry.setState({lastAt: Date.now(), snapshot: {seq: 1, temps: [], weight: 100,
    health: {safety_latched: false, temp_valid: true, mfc_valid: true, balance_valid: true,
      temp_connected: true, mfc_connected: true, balance_connected: true},
    gas: [{pv: 1.24, sv: 1.24, on: true}, {pv: 0.001, sv: 0.001, on: true}]}});
  render(<MonitorPanel />);
  expect(screen.getByText("SV 1.24")).toBeTruthy();
  expect(screen.getByText("SV 0.001")).toBeTruthy();
});
