import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import * as ipc from "../../ipc";
import type { ExperimentDetail, ExperimentSummary } from "../../ipc/types";
import { useExperiment } from "../../stores/experiment";
import { useAuth } from "../../stores/auth";
import { HistoryView } from "./HistoryView";

vi.mock("../../ipc", async (original) => ({
  ...await original<typeof ipc>(),
  listExperiments: vi.fn(),
  getExperimentDetail: vi.fn(),
  getExperimentSamples: vi.fn(),
  listUsers: vi.fn(),
  deleteExperiment: vi.fn(),
  setExperimentOwner: vi.fn(),
}));

const first: ExperimentSummary = {
  experiment_id: "ri-first", experiment_name: "RI", sample_name: "试样 A",
  sample_weight: 100, start_time: "2026-09-13T10:00:00+08:00",
  experiment_type: "GB13241", has_analysis: false,
};
const detailOf = (summary: ExperimentSummary): ExperimentDetail => ({
  ...summary, created_at: summary.start_time, sample_count: summary.end_time ? 61 : 1,
});

beforeEach(() => {
  vi.resetAllMocks();
  useAuth.setState({ snapshot: null, loading: false });
  useExperiment.setState({ state: { running: true, experiment_id: first.experiment_id } });
  vi.mocked(ipc.listExperiments).mockResolvedValue([first]);
  vi.mocked(ipc.getExperimentDetail).mockResolvedValue(detailOf(first));
  vi.mocked(ipc.getExperimentSamples).mockResolvedValue([]);
});
afterEach(cleanup);

function admin() {
  const user = { user_id: "admin", username: "admin", display_name: "管理员", role: "ADMIN" as const,
    is_active: true, must_change_password: false, created_at: first.start_time };
  useAuth.setState({ snapshot: {status: "AUTHENTICATED", idle_timeout_s: 900, user} });
  vi.mocked(ipc.listUsers).mockResolvedValue([user]);
}

it("refreshes the visible history and selected record after terminal persistence completes", async () => {
  render(<HistoryView />);
  expect(await screen.findByRole("button", { name: /试样 A.*进行中/ })).toBeTruthy();
  await screen.findByText("ri-first");
  act(() => useExperiment.setState({ state: { running: false, persistence_pending: true } }));
  const completed = { ...first, end_time: "2026-09-13T13:00:00+08:00" };
  vi.mocked(ipc.listExperiments).mockResolvedValue([completed]);
  vi.mocked(ipc.getExperimentDetail).mockResolvedValue(detailOf(completed));
  act(() => useExperiment.setState({ state: { running: false, persistence_pending: false } }));
  expect(await screen.findByRole("button", { name: /试样 A.*完成/ })).toBeTruthy();
  expect(await screen.findByText("61")).toBeTruthy();
});

it("keeps the newly selected record when an older detail request finishes late", async () => {
  const second = { ...first, experiment_id: "ri-second", sample_name: "试样 B" };
  vi.mocked(ipc.listExperiments).mockResolvedValue([first, second]);
  let resolveFirst!: (detail: ExperimentDetail) => void;
  vi.mocked(ipc.getExperimentDetail).mockImplementation((id) => id === first.experiment_id
    ? new Promise((resolve) => { resolveFirst = resolve; })
    : Promise.resolve(detailOf(second)));
  render(<HistoryView />);
  await waitFor(() => expect(resolveFirst).toBeTypeOf("function"));
  fireEvent.click(screen.getByRole("button", { name: /试样 B/ }));
  await screen.findByText("ri-second");
  await act(async () => { resolveFirst(detailOf(first)); });
  expect(screen.queryByText("ri-first")).toBeNull();
  expect(screen.getByText("ri-second")).toBeTruthy();
});

it("never exposes the previous record's delete action while a new selection is loading", async () => {
  admin();
  const second = { ...first, experiment_id: "ri-second", sample_name: "试样 B" };
  vi.mocked(ipc.listExperiments).mockResolvedValue([first, second]);
  vi.mocked(ipc.getExperimentDetail).mockImplementation((id) => id === first.experiment_id
    ? Promise.resolve(detailOf(first)) : new Promise(() => {}));
  render(<HistoryView />);
  await screen.findByText("ri-first");
  fireEvent.click(screen.getByRole("button", { name: /试样 B/ }));
  expect(screen.queryByText("ri-first")).toBeNull();
  expect(screen.queryByRole("button", { name: "删除" })).toBeNull();
});

it("does not let an old ownership save replace the currently selected detail", async () => {
  admin();
  const second = { ...first, experiment_id: "ri-second", sample_name: "试样 B" };
  vi.mocked(ipc.listExperiments).mockResolvedValue([first, second]);
  vi.mocked(ipc.getExperimentDetail).mockImplementation((id) => Promise.resolve(detailOf(id === first.experiment_id ? first : second)));
  let finishSave!: () => void;
  vi.mocked(ipc.setExperimentOwner).mockImplementation(() => new Promise((resolve) => { finishSave = resolve; }));
  render(<HistoryView />);
  await screen.findByText("ri-first");
  fireEvent.change(screen.getByLabelText("数据归属"), { target: { value: "admin" } });
  fireEvent.click(screen.getByRole("button", { name: /试样 B/ }));
  await screen.findByText("ri-second");
  await act(async () => finishSave());
  expect(screen.queryByText("ri-first")).toBeNull();
  expect(screen.getByText("ri-second")).toBeTruthy();
});

it("deletes only the record named in the confirmation even if selection changes", async () => {
  admin();
  const second = { ...first, experiment_id: "ri-second", sample_name: "试样 B" };
  vi.mocked(ipc.listExperiments).mockResolvedValue([first, second]);
  vi.mocked(ipc.getExperimentDetail).mockImplementation((id) => Promise.resolve(detailOf(id === first.experiment_id ? first : second)));
  vi.mocked(ipc.deleteExperiment).mockResolvedValue(undefined);
  render(<HistoryView />);
  await screen.findByText("ri-first");
  fireEvent.click(screen.getByRole("button", { name: "删除" }));
  fireEvent.click(screen.getByRole("button", { name: /试样 B/ }));
  await screen.findByText("ri-second");
  expect(screen.getByText(/确认删除实验「试样 A」/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "确认删除" }));
  await waitFor(() => expect(ipc.deleteExperiment).toHaveBeenCalledWith("ri-first"));
});
