import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import * as ipc from "../../ipc";
import type { ConfigSnapshot } from "../../ipc";
import defaults from "../../../config/comm_config.json";
import { useAuth } from "../../stores/auth";
import { useExperiment } from "../../stores/experiment";
import { useNav } from "../../stores/nav";
import { SettingsView } from "./SettingsView";

vi.mock("../../ipc", async (original) => ({
  ...await original<typeof ipc>(),
  getCommConfig: vi.fn(), saveCommConfig: vi.fn(), restartForConfig: vi.fn(),
}));
const initial: ConfigSnapshot = { path: "/profile/config/comm_config.json", revision: "r1", applied: structuredClone(defaults), saved: structuredClone(defaults), restart_required: false };
beforeEach(() => {
  vi.resetAllMocks();
  useAuth.setState({ snapshot: { status: "AUTHENTICATED", idle_timeout_s: 900, user: {user_id: "admin", username: "admin", display_name: "管理员", role: "ADMIN", is_active: true, must_change_password: false, created_at: "2026-09-13"} } });
  useExperiment.setState({ state: { running: false } });
  vi.mocked(ipc.getCommConfig).mockResolvedValue(structuredClone(initial));
});
afterEach(cleanup);

it("saves the sampling alias with a revision and reason and shows pending rather than applied", async () => {
  const saved = structuredClone(initial);
  saved.saved.SAMPLING.interval_s = 2;
  saved.saved.PERFORMANCE_CONFIG.experiment_data_interval = 2;
  saved.revision = "r2"; saved.restart_required = true;
  vi.mocked(ipc.saveCommConfig).mockResolvedValue(saved);
  render(<SettingsView />);
  fireEvent.change(await screen.findByLabelText("实验采样间隔 (s)"), {target: {value: "2"}});
  fireEvent.change(screen.getByLabelText("修改原因"), {target: {value: "模拟验收调整采样"}});
  fireEvent.click(screen.getByRole("button", {name: "保存配置"}));
  await waitFor(() => expect(ipc.saveCommConfig).toHaveBeenCalledWith("r1", expect.objectContaining({SAMPLING: {interval_s: 2}, PERFORMANCE_CONFIG: expect.objectContaining({experiment_data_interval: 2})}), "模拟验收调整采样"));
  expect(await screen.findByText(/已保存，重启后生效/)).toBeTruthy();
  expect(screen.getByText(/当前实验采样：1 s/)).toBeTruthy();
  expect(ipc.restartForConfig).not.toHaveBeenCalled();
});

it("refreshes the saved state after an ambiguous write failure and preserves the error", async () => {
  const saved = {...structuredClone(initial), revision: "r2", restart_required: true};
  saved.saved.SAMPLING.interval_s = 2;
  vi.mocked(ipc.getCommConfig).mockResolvedValueOnce(structuredClone(initial)).mockResolvedValue(saved);
  vi.mocked(ipc.saveCommConfig).mockRejectedValue(new Error("文件已替换，但同步未确认"));
  render(<SettingsView />);
  fireEvent.change(await screen.findByLabelText("实验采样间隔 (s)"), {target: {value: "2"}});
  fireEvent.change(screen.getByLabelText("修改原因"), {target: {value: "模拟验收"}});
  fireEvent.click(screen.getByRole("button", {name: "保存配置"}));
  expect(await screen.findByRole("alert")).toHaveProperty("textContent", expect.stringContaining("同步未确认"));
  expect(await screen.findByText(/已保存，重启后生效/)).toBeTruthy();
  expect(ipc.restartForConfig).not.toHaveBeenCalled();
});

it("disables editing for ordinary users and during experiment persistence", async () => {
  render(<SettingsView />);
  const input = await screen.findByLabelText("实验采样间隔 (s)");
  act(() => useExperiment.setState({state: {running: false, persistence_pending: true}}));
  expect(input.closest("fieldset")?.disabled).toBe(true);
  act(() => {
    useExperiment.setState({state: {running: false}});
    const snapshot = useAuth.getState().snapshot!;
    useAuth.setState({snapshot: {...snapshot, user: {...snapshot.user!, role: "USER"}}});
  });
  expect(input.closest("fieldset")?.disabled).toBe(true);
  expect(ipc.saveCommConfig).not.toHaveBeenCalled();
});


it("allows discarding a local draft after an experiment starts", async () => {
  render(<SettingsView />);
  fireEvent.change(await screen.findByLabelText("实验采样间隔 (s)"), {target: {value: "2"}});
  expect(useNav.getState().leaveBlockReason).toContain("尚未保存");
  act(() => useExperiment.setState({state: {running: true}}));
  const discard = screen.getByRole("button", {name: "撤销未保存修改"});
  expect(discard.matches(":disabled")).toBe(false);
  fireEvent.click(discard);
  expect(useNav.getState().leaveBlockReason).toBeNull();
  expect(ipc.saveCommConfig).not.toHaveBeenCalled();
});

it("requires an explicit restart confirmation and reports a refused restart", async () => {
  vi.mocked(ipc.getCommConfig).mockResolvedValue({...structuredClone(initial), restart_required: true});
  vi.mocked(ipc.restartForConfig).mockRejectedValue(new Error("保护气氛确认失败"));
  render(<SettingsView />);
  fireEvent.click(await screen.findByRole("button", {name: "重启应用以生效"}));
  expect(ipc.restartForConfig).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", {name: "取消重启"}));
  expect(ipc.restartForConfig).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", {name: "重启应用以生效"}));
  fireEvent.click(screen.getByRole("button", {name: "确认保护并重启"}));
  expect(await screen.findByRole("alert")).toHaveProperty("textContent", expect.stringContaining("保护气氛确认失败"));
  expect(ipc.restartForConfig).toHaveBeenCalledWith("r1");
  expect(screen.getByRole("button", {name: "重启应用以生效"}).matches(":disabled")).toBe(false);
});
