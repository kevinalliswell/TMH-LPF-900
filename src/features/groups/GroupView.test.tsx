import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { GroupView } from "./GroupView";
import type { GroupSnapshot, TestGroupRevision } from "../../ipc";
const api = vi.hoisted(() => ({
  isTauri: vi.fn(() => true), listTestGroups: vi.fn(), listExperiments: vi.fn(),
  previewTestGroup: vi.fn(), saveTestGroup: vi.fn(), getTestGroup: vi.fn(), generateTestGroupReport: vi.fn(),
}));
vi.mock("../../ipc", () => api);
const snapshot: GroupSnapshot = {
  schema_version:1, name:"Batch group", batch_id:"B1", same_batch_confirmed:true,
  policy:{method:"REDUCIBILITY",custom:false,material:"pellet",manual_limits:null},
  report_details:{lab_name:"",lab_address:"",report_number:"",report_date:"",reviewer:"",deviations:""},
  valid_trial_indices:[],trials:[], evaluation:{calculation_version:"test",rounding_version:"test",
    limits:{a:3,b:3.6,c:3.9,basis:"GB/T 13241"},decision_values:[],decision:{required_count:2,selected_indices:[]},
    reported:{RI:"42.5"},explanation:"2 次结果满足重复性要求"},
  data_source:"mock",method_conformity:"pending_process_verification",recorded_by:"Alice",reason:"Create",
};
const record: TestGroupRevision = {group_id:"group-ui",revision:1,owner_user_id:"alice",recorded_by_user_id:"alice",created_at:"2026-09-08",snapshot};
afterEach(cleanup);
beforeEach(() => {
  vi.clearAllMocks(); api.isTauri.mockReturnValue(true);
  api.listTestGroups.mockResolvedValue([]);
  api.listExperiments.mockResolvedValue([{experiment_id:"a",sample_id:"A",sample_name:"Pellets",start_time:"2026-09-08",end_time:"2026-09-08",has_analysis:true}]);
  api.previewTestGroup.mockResolvedValue(snapshot);api.saveTestGroup.mockResolvedValue(record);
  api.generateTestGroupReport.mockResolvedValue("/exports/report-group-ui-v1.html");
});
it("creates a version from selected records and exports the saved version", async () => {
  render(<GroupView />);
  await screen.findByLabelText("选择 A");
  fireEvent.change(screen.getByLabelText("检验组名称"),{target:{value:"Batch group"}});
  fireEvent.change(screen.getByLabelText("批次 / 试样标识"),{target:{value:"B1"}});
  fireEvent.change(screen.getByLabelText("本次保存原因"),{target:{value:"Create"}});
  fireEvent.click(screen.getByLabelText("确认来自同一批试样的独立试验"));
  fireEvent.click(screen.getByLabelText("选择 A"));
  fireEvent.click(screen.getByRole("button",{name:"预览重复性结果"}));
  await screen.findByText("42.5 %");
  expect(api.previewTestGroup.mock.calls[0]?.[0].trials).toEqual([{experiment_id:"a",exclusion_reason:null}]);
  fireEvent.click(screen.getByRole("button",{name:"保存新版本"}));
  await waitFor(() => expect(api.saveTestGroup).toHaveBeenCalledTimes(1));
  const button = await screen.findByRole("button",{name:"导出 v1 报告"});
  await waitFor(() => expect(button.hasAttribute("disabled")).toBe(false));
  fireEvent.click(button);
  await waitFor(() => expect(api.generateTestGroupReport).toHaveBeenCalledWith("group-ui",1));
});
it("clears a stale preview when evidence or report inputs change", async () => {
  render(<GroupView />);await screen.findByLabelText("选择 A");
  fireEvent.click(screen.getByRole("button",{name:"预览重复性结果"}));
  await screen.findByText("42.5 %");
  fireEvent.change(screen.getByLabelText("报告编号"),{target:{value:"Changed"}});
  expect(screen.queryByText("42.5 %")).toBeNull();
});
it("shows the backend validation error and keeps the draft", async () => {
  api.previewTestGroup.mockRejectedValueOnce(new Error("试验未正常完成"));
  render(<GroupView />);await screen.findByLabelText("选择 A");
  fireEvent.change(screen.getByLabelText("检验组名称"),{target:{value:"Retain me"}});
  fireEvent.click(screen.getByRole("button",{name:"预览重复性结果"}));
  expect(await screen.findByRole("alert")).toHaveProperty("textContent","试验未正常完成");
  expect(screen.getByLabelText("检验组名称")).toHaveProperty("value","Retain me");
  expect(api.saveTestGroup).not.toHaveBeenCalled();
});
it("explains the desktop requirement without fabricating a browser result", () => {
  api.isTauri.mockReturnValue(false);render(<GroupView />);
  expect(screen.getByText(/请在桌面应用中使用检验组/)).toBeTruthy();
  expect(api.listTestGroups).not.toHaveBeenCalled();
});
