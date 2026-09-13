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
