// Copy to src/features/groups/GroupNavigation.review.test.tsx.
// Exercises the real App navigation, GroupView and useNav store. Only IPC and
// unrelated views are substituted; no production files are edited by this task.
import type { ReactNode } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import App from "../../App";
import { useAuth } from "../../stores/auth";
import { useNav } from "../../stores/nav";
import type { GroupSnapshot, TestGroupRevision } from "../../ipc";

const api = vi.hoisted(() => ({
  listTestGroups: vi.fn(),
  listExperiments: vi.fn(),
  previewTestGroup: vi.fn(),
  saveTestGroup: vi.fn(),
  getTestGroup: vi.fn(),
  generateTestGroupReport: vi.fn(),
}));
vi.mock("../../ipc", async (original) => ({
  ...(await original<typeof import("../../ipc")>()),
  ...api,
  isTauri: () => true,
  subscribeTelemetry: () => () => {},
  subscribeExperimentState: () => () => {},
  subscribeAppNotices: () => () => {},
  getRuntimeSettings: async () => ({ heartbeat_timeout_s: 5 }),
}));
vi.mock("../auth/AuthGate", () => ({
  AuthGate: ({ children }: { children: ReactNode }) => children,
}));
vi.mock("../control/ExperimentBar", () => ({ ExperimentBar: () => null }));
vi.mock("../control/StatusBar", () => ({ StatusBar: () => null }));
vi.mock("../history/HistoryView", () => ({
  HistoryView: () => <div>History page used for the navigation regression</div>,
}));

const snapshot: GroupSnapshot = {
  schema_version: 1,
  name: "Navigation test group",
  batch_id: "BATCH-NAV",
  same_batch_confirmed: true,
  policy: {
    method: "REDUCIBILITY",
    custom: false,
    material: "pellet",
    manual_limits: null,
  },
  report_details: {
    lab_name: "Lab retained until save",
    lab_address: "",
    report_number: "",
    report_date: "",
    reviewer: "",
    deviations: "",
  },
  valid_trial_indices: [],
  trials: [],
  evaluation: {
    calculation_version: "test",
    rounding_version: "test",
    limits: { a: 3, b: 3.6, c: 3.9, basis: "GB/T 13241" },
    decision_values: [],
    decision: { required_count: 2, selected_indices: [] },
    reported: {},
    explanation: "Awaiting a repeat",
  },
  data_source: "mock",
  method_conformity: "pending_process_verification",
  recorded_by: "Alice",
  reason: "Create",
};
const saved: TestGroupRevision = {
  group_id: "group-navigation",
  revision: 1,
  owner_user_id: "alice",
  recorded_by_user_id: "alice",
  created_at: "2026-09-08T12:00:00Z",
  snapshot,
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
function historyButton() {
  return screen.getByRole("button", { name: "历史查询" }) as HTMLButtonElement;
}
async function ready() {
  await screen.findByLabelText("选择 A");
  await waitFor(() => expect(useNav.getState().leaveBlockReason).toBeNull());
}
function editDraft() {
  fireEvent.change(screen.getByLabelText("检验组名称"), {
    target: { value: "Navigation test group" },
  });
  fireEvent.change(screen.getByLabelText("批次 / 试样标识"), {
    target: { value: "BATCH-NAV" },
  });
  fireEvent.change(screen.getByLabelText("实验室"), {
    target: { value: "Lab retained until save" },
  });
  fireEvent.change(screen.getByLabelText("本次保存原因"), {
    target: { value: "Create" },
  });
  fireEvent.click(screen.getByLabelText("确认来自同一批试样的独立试验"));
  fireEvent.click(screen.getByLabelText("选择 A"));
}

beforeEach(() => {
  vi.resetAllMocks();
  useNav.setState({ view: "groups", leaveBlockReason: null });
  useAuth.setState({
    loading: false,
    snapshot: {
      status: "AUTHENTICATED",
      idle_timeout_s: 900,
      user: {
        user_id: "alice",
        username: "alice",
        display_name: "Alice",
        role: "USER",
        is_active: true,
        must_change_password: false,
        created_at: "2026-09-01",
      },
    },
  });
  api.listTestGroups.mockResolvedValue([]);
  api.listExperiments.mockResolvedValue([
    {
      experiment_id: "a",
      sample_id: "A",
      sample_name: "Pellets",
      start_time: "2026-09-08T08:00:00Z",
      end_time: "2026-09-08T12:00:00Z",
      has_analysis: true,
    },
  ]);
  api.saveTestGroup.mockResolvedValue(saved);
});
afterEach(() => {
  cleanup();
  useNav.setState({ view: "control", leaveBlockReason: null });
});

it("keeps unsaved report fields when main navigation is attempted and allows leaving after undo", async () => {
  render(<App />);
  await ready();
  editDraft();
  await waitFor(() => expect(historyButton().disabled).toBe(true));
  expect(useNav.getState().leaveBlockReason).toBeTruthy();
  fireEvent.click(historyButton());
  act(() => useNav.getState().setView("history"));
  expect(useNav.getState().view).toBe("groups");
  expect(screen.getByLabelText("实验室")).toHaveProperty(
    "value",
    "Lab retained until save",
  );

  fireEvent.click(screen.getByRole("button", { name: "撤销未保存修改" }));
  await waitFor(() => expect(historyButton().disabled).toBe(false));
  expect(useNav.getState().leaveBlockReason).toBeNull();
  fireEvent.click(historyButton());
  expect(
    await screen.findByText("History page used for the navigation regression"),
  ).toBeTruthy();
});

it("blocks navigation during initial evidence loading and releases it after loading completes", async () => {
  const loading = deferred<TestGroupRevision[]>();
  api.listTestGroups.mockReturnValueOnce(loading.promise);
  render(<App />);
  await waitFor(() => expect(historyButton().disabled).toBe(true));
  act(() => useNav.getState().setView("history"));
  expect(useNav.getState().view).toBe("groups");
  await act(async () => {
    loading.resolve([]);
  });
  await ready();
  expect(historyButton().disabled).toBe(false);
});

it("keeps a pending save on screen and releases navigation only once the saved version is loaded", async () => {
  const saving = deferred<TestGroupRevision>();
  api.saveTestGroup.mockReturnValueOnce(saving.promise);
  render(<App />);
  await ready();
  editDraft();
  fireEvent.click(screen.getByRole("button", { name: "保存新版本" }));
  await waitFor(() => expect(api.saveTestGroup).toHaveBeenCalledOnce());
  expect(historyButton().disabled).toBe(true);
  expect(
    (
      screen.getByRole("button", {
        name: "撤销未保存修改",
      }) as HTMLButtonElement
    ).disabled,
  ).toBe(true);
  act(() => useNav.getState().setView("history"));
  expect(useNav.getState().view).toBe("groups");

  await act(async () => {
    saving.resolve(saved);
  });
  await screen.findByRole("button", { name: "导出 v1 报告" });
  await waitFor(() => expect(historyButton().disabled).toBe(false));
  expect(useNav.getState().leaveBlockReason).toBeNull();
  expect(screen.getByLabelText("实验室")).toHaveProperty(
    "value",
    "Lab retained until save",
  );
  expect(screen.queryByRole("button", { name: "撤销未保存修改" })).toBeNull();
});

it("removes the navigation block when the group screen is unmounted", async () => {
  const view = render(<App />);
  await ready();
  editDraft();
  await waitFor(() => expect(useNav.getState().leaveBlockReason).toBeTruthy());
  view.unmount();
  expect(useNav.getState().leaveBlockReason).toBeNull();
  act(() => useNav.getState().setView("history"));
  expect(useNav.getState().view).toBe("history");
});
