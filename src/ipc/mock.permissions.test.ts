import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

let auth: typeof import("./authMock");
let mock: typeof import("./mock");

beforeEach(async () => {
  vi.resetModules();
  vi.useFakeTimers();
  auth = await import("./authMock");
  mock = await import("./mock");
});

afterEach(() => {
  vi.clearAllTimers();
  vi.useRealTimers();
});

function startRun() {
  return mock.mockStartExperiment({
    sample_id: "QA-1",
    sample_name: "权限回归样品",
    sample_weight: 500,
    experiment_type: "REDUCIBILITY",
  });
}

function addOperator() {
  auth.mockBootstrapAdmin("admin", "管理员", "admin-password");
  const operator = auth.mockCreateUser("operator", "操作员", "temp-password", "USER");
  auth.mockLogin("operator", "temp-password");
  auth.mockChangePassword("temp-password", "operator-password");
  auth.mockLogin("admin", "admin-password");
  return operator;
}

describe("browser mock authorization parity", () => {
  it("requires a logged-in user for device, experiment and history commands", () => {
    expect(() => mock.mockSetFlow(0, 1)).toThrow("AUTH_REQUIRED");
    expect(() => mock.mockSetOnOff(0, true)).toThrow("AUTH_REQUIRED");
    expect(() => mock.mockSetAllValves(true)).toThrow("AUTH_REQUIRED");
    expect(() => mock.mockTare()).toThrow("AUTH_REQUIRED");
    expect(() => mock.mockStopExperiment()).toThrow("AUTH_REQUIRED");
    expect(() => mock.mockResetSafety()).toThrow("AUTH_REQUIRED");
    expect(() => mock.mockListExperiments()).toThrow("AUTH_REQUIRED");
    expect(() => mock.mockPreflightExperiment({} as never)).toThrow("AUTH_REQUIRED");
    expect(() => mock.mockGetExperimentStartDefaults("REDUCIBILITY")).toThrow("AUTH_REQUIRED");
  });

  it("allows emergency shutoff without login but keeps reset protected", () => {
    expect(() => mock.mockShutoff()).not.toThrow();
    expect(mock.tick().gas[1]).toMatchObject({ sv: 5 });
    expect(() => mock.mockResetSafety()).toThrow("AUTH_REQUIRED");
  });

  it("keeps the manual gas shutoff protected while emergency shutdown remains public", async () => {
    const ipc = await import("./index");
    await expect(ipc.shutoffFlammableGases()).rejects.toThrow("AUTH_REQUIRED");
    await expect(ipc.emergencySafeShutdown()).resolves.toBeUndefined();
    auth.mockBootstrapAdmin("admin", "管理员", "admin-password");
    await expect(ipc.shutoffFlammableGases()).resolves.toBeUndefined();
  });

  it("rejects protected commands in a locked session while leaving emergency shutoff open", () => {
    auth.mockBootstrapAdmin("admin", "管理员", "admin-password");
    auth.mockLock();
    expect(() => mock.mockSetFlow(0, 1)).toThrow("AUTH_REQUIRED");
    expect(() => mock.mockStopExperiment()).toThrow("AUTH_REQUIRED");
    expect(() => mock.mockShutoff()).not.toThrow();
  });

  it("blocks protected commands while the user must change a temporary password", () => {
    auth.mockBootstrapAdmin("admin", "管理员", "admin-password");
    auth.mockCreateUser("operator", "操作员", "temp-password", "USER");
    auth.mockLogin("operator", "temp-password");
    expect(() => mock.mockSetFlow(0, 1)).toThrow("PASSWORD_CHANGE_REQUIRED");
    expect(() => mock.mockTare()).toThrow("PASSWORD_CHANGE_REQUIRED");
    expect(() => mock.mockListExperiments()).toThrow("PASSWORD_CHANGE_REQUIRED");
    expect(() => auth.mockListUsers()).toThrow("PASSWORD_CHANGE_REQUIRED");
  });

  it("allows a normal user to control and stop their own run", () => {
    const operator = addOperator();
    auth.mockLogin("operator", "operator-password");
    const id = startRun();
    expect(mock.mockSetFlow(0, 1).applied).toBe(1);
    mock.mockStopExperiment();
    expect(mock.mockGetExperimentDetail(id).owner_user_id).toBe(operator.user_id);
  });

  it("allows an authenticated idle tare", () => {
    auth.mockBootstrapAdmin("admin", "管理员", "admin-password");
    expect(mock.mockTare()).toMatchObject({ success: true, weight: 0 });
    expect(mock.tick().weight).toBe(0);
  });

  it("protects the run owner’s tare baseline and restricts live control to owner or admin", () => {
    const operator = addOperator();
    const id = startRun();
    expect(() => mock.mockTare()).toThrow("EXPERIMENT_RUNNING");
    expect(mock.tick().weight).toBe(500);

    auth.mockLogin("operator", "operator-password");
    expect(() => mock.mockSetFlow(0, 1)).toThrow("FORBIDDEN");
    expect(() => mock.mockStopExperiment()).toThrow("FORBIDDEN");
    expect(() => mock.mockShutoffFlammableGases()).toThrow("FORBIDDEN");
    expect(() => mock.mockTare()).toThrow("FORBIDDEN");
    expect(() => mock.mockAcknowledgeStage(id, 0, "确认")).toThrow("FORBIDDEN");

    auth.mockLogin("admin", "admin-password");
    expect(mock.mockSetFlow(0, 1).applied).toBe(1);
    expect(operator.role).toBe("USER");
  });

  it("requires an authenticated owner or admin to read experiment history", () => {
    const operator = addOperator();
    const id = startRun();
    auth.mockLogin("operator", "operator-password");
    expect(() => mock.mockGetExperimentDetail(id)).toThrow("FORBIDDEN");
    expect(() => mock.mockGetExperimentSamples(id)).toThrow("FORBIDDEN");
    auth.mockLogin("admin", "admin-password");
    mock.mockStopExperiment();
    auth.mockLogout();
    expect(() => mock.mockGetExperimentDetail(id)).toThrow("AUTH_REQUIRED");

    auth.mockLogin("operator", "operator-password");
    expect(mock.mockListExperiments().some((item) => item.experiment_id === id)).toBe(false);
    expect(() => mock.mockGetExperimentDetail(id)).toThrow("FORBIDDEN");

    auth.mockLogin("admin", "admin-password");
    expect(mock.mockGetExperimentDetail(id).owner_user_id).not.toBe(operator.user_id);
  });

  it("limits history ownership changes and deletion to valid, idle admin actions", () => {
    const operator = addOperator();
    const id = startRun();
    expect(() => mock.mockDeleteExperiment(id)).toThrow("INVALID_STATE");
    expect(() => mock.mockSetExperimentOwner(id, operator.user_id)).toThrow("无法变更正在运行实验的归属");
    mock.mockStopExperiment();
    mock.mockSetExperimentOwner(id, operator.user_id);

    auth.mockLogin("operator", "operator-password");
    expect(mock.mockGetExperimentDetail(id).owner_user_id).toBe(operator.user_id);
    expect(() => mock.mockDeleteExperiment(id)).toThrow("FORBIDDEN");

    auth.mockLogin("admin", "admin-password");
    expect(() => mock.mockSetExperimentOwner(id, "missing-user")).toThrow("USER_NOT_FOUND");
    mock.mockDeleteExperiment(id);
    expect(() => mock.mockDeleteExperiment(id)).toThrow("实验不存在");
  });

  it("keeps emergency shutoff operational after logout and archives the stopped run", () => {
    auth.mockBootstrapAdmin("admin", "管理员", "admin-password");
    const id = startRun();
    auth.mockLogout();
    expect(() => mock.mockShutoff()).not.toThrow();
    expect(mock.tick().gas.map((gas) => gas.sv)).toEqual([0, 5, 0, 0]);
    auth.mockLogin("admin", "admin-password");
    expect(mock.mockGetExperimentDetail(id).runtime?.outcome).toBe("stopped");
  });

  it("rejects invalid gas channels with the native command error code", () => {
    auth.mockBootstrapAdmin("admin", "管理员", "admin-password");
    expect(() => mock.mockSetFlow(4, 1)).toThrow("INVALID_PARAMS");
    expect(() => mock.mockSetOnOff(4, true)).toThrow("INVALID_PARAMS");
  });
});
