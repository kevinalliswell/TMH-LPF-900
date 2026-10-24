// Typed IPC boundary. In Tauri it calls real commands + a telemetry Channel; in a
// plain browser it dispatches to the synthetic mock (docs/08). A single `isTauri()`
// switch keeps the UI identical in both.

import { Channel, invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  ReductionInput,
  RdiInput,
  SwellingInput,
  ReportDetails,
  ClampResult,
  ExperimentDetail,
  ExperimentParams,
  ExperimentState,
  ExperimentSummary,
  GasLimits,
  RuntimeSettings,
  ProgramDto,
  ExperimentPreflightResult,
  ExperimentStartDefaults,
  SampleDto,
  TareResult,
  TelemetrySnapshot,
  UpdateCheck,
  AuthSnapshot,
  PublicUser,
  UserRole,
} from "./types";
import {
  mockAuthState,
  mockBootstrapAdmin,
  mockChangePassword,
  mockCreateUser,
  mockListUsers,
  mockLock,
  mockLogin,
  mockLogout,
  mockResetPassword,
  mockUpdateUser,
} from "./authMock";
import {
  mockSaveReportDetails,
  mockAnalyze13240,
  mockAnalyze13241,
  mockAnalyze13242,
  mockDeleteExperiment,
  mockExportText,
  mockGetExperimentDetail,
  mockGetExperimentSamples,
  mockListExperiments,
  mockListPrograms,
  mockSaveCustomProgram,
  mockListCustomProgramRevisions,
  mockDeleteCustomProgram,
  mockPreflightExperiment,
  mockGenerateSampleId,
  mockGetExperimentStartDefaults,
  mockReportHtml,
  mockSetFlow,
  mockSetOnOff,
  mockSetAllValves,
  mockShutoff,
  mockResetSafety,
  mockSkipStage,
  mockAcknowledgeStage,
  mockStartExperiment,
  mockStopExperiment,
  mockSetExperimentOwner,
  mockSubscribeExperiment,
  mockTare,
  mockTelemetry,
} from "./mock";

export * from "./types";
export { CommandFailure } from "./errors";
export type { CommandError } from "./errors";
import { normalizeCommandError } from "./errors";

async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await tauriInvoke<T>(command, args);
  } catch (cause) {
    throw normalizeCommandError(cause);
  }
}

export * from "./groupTypes";
export * from "./configTypes";
import type { CommConfig, ConfigSnapshot } from "./configTypes";

function requireNativeConfig(): void {
  if (!isTauri()) throw new Error("通讯配置需要在桌面应用中读取和保存；浏览器预览不连接设备。");
}
export async function getCommConfig(): Promise<ConfigSnapshot> {
  requireNativeConfig();
  return invoke("get_comm_config");
}
export async function saveCommConfig(expectedRevision: string, config: CommConfig, reason: string): Promise<ConfigSnapshot> {
  requireNativeConfig();
  return invoke("save_comm_config", { expected_revision: expectedRevision, config, reason });
}
export async function restartForConfig(expectedRevision: string): Promise<void> {
  requireNativeConfig();
  return invoke("restart_for_config", { expected_revision: expectedRevision });
}
import type { GroupRequest, GroupSnapshot, TestGroupRevision } from "./groupTypes";

function requireGroupDatabase(): void {
  if (!isTauri()) throw new Error("检验组需要桌面应用的实验数据库，请在桌面应用中使用此功能");
}
export async function previewTestGroup(request: GroupRequest): Promise<GroupSnapshot> {
  requireGroupDatabase();
  return invoke("preview_test_group", { request });
}
export async function saveTestGroup(request: GroupRequest): Promise<TestGroupRevision> {
  requireGroupDatabase();
  return invoke("save_test_group", { request });
}
export async function listTestGroups(offset = 0): Promise<TestGroupRevision[]> {
  requireGroupDatabase();
  return invoke("list_test_groups", { offset });
}
export async function getTestGroup(id: string, revision: number | null = null): Promise<TestGroupRevision> {
  requireGroupDatabase();
  return invoke("get_test_group", { id, revision });
}
export async function generateTestGroupReport(id: string, revision: number): Promise<string> {
  requireGroupDatabase();
  return invoke("generate_test_group_report", { id, revision });
}

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function getAuthState(): Promise<AuthSnapshot> {
  return isTauri() ? invoke("get_auth_state") : mockAuthState();
}

export async function bootstrapAdmin(
  username: string,
  displayName: string,
  password: string,
): Promise<PublicUser> {
  return isTauri()
    ? invoke("bootstrap_admin", {
        username,
        display_name: displayName,
        password,
      })
    : mockBootstrapAdmin(username, displayName, password);
}

export async function login(
  username: string,
  password: string,
): Promise<AuthSnapshot> {
  return isTauri()
    ? invoke("login", { username, password })
    : mockLogin(username, password);
}

export async function logout(): Promise<void> {
  if (isTauri()) await invoke("logout");
  else mockLogout();
}

export async function lockSession(): Promise<void> {
  if (isTauri()) await invoke("lock_session");
  else mockLock();
}

export async function touchSession(): Promise<void> {
  if (isTauri()) await invoke("touch_session");
}

export async function emergencySafeShutdown(): Promise<void> {
  if (isTauri()) await invoke("emergency_safe_shutdown");
  else mockShutoff();
}

export async function changeOwnPassword(
  currentPassword: string,
  newPassword: string,
): Promise<void> {
  if (isTauri()) {
    await invoke("change_own_password", {
      current_password: currentPassword,
      new_password: newPassword,
    });
  } else mockChangePassword(currentPassword, newPassword);
}

export async function listUsers(): Promise<PublicUser[]> {
  return isTauri() ? invoke("list_users") : mockListUsers();
}

export async function createUser(
  username: string,
  displayName: string,
  temporaryPassword: string,
  role: UserRole,
): Promise<PublicUser> {
  return isTauri()
    ? invoke("create_user", {
        username,
        display_name: displayName,
        temporary_password: temporaryPassword,
        role,
      })
    : mockCreateUser(username, displayName, temporaryPassword, role);
}

export async function updateUser(
  userId: string,
  displayName: string,
  role: UserRole,
  isActive: boolean,
): Promise<PublicUser> {
  return isTauri()
    ? invoke("update_user", {
        user_id: userId,
        display_name: displayName,
        role,
        is_active: isActive,
      })
    : mockUpdateUser(userId, displayName, role, isActive);
}

export async function resetUserPassword(
  userId: string,
  temporaryPassword: string,
): Promise<void> {
  if (isTauri())
    await invoke("reset_user_password", {
      user_id: userId,
      temporary_password: temporaryPassword,
    });
  else mockResetPassword(userId, temporaryPassword);
}

export async function setExperimentOwner(
  experimentId: string,
  ownerUserId: string | null,
): Promise<void> {
  if (!isTauri()) return void mockSetExperimentOwner(experimentId, ownerUserId);
  await invoke("set_experiment_owner", {
    experiment_id: experimentId,
    owner_user_id: ownerUserId,
  });
}

export type RuntimeMode = "real" | "mock" | "replay" | "web";

/** True runtime mode. In Tauri this asks the backend (TMH_DEVICE_MODE = real|mock|replay)
 *  so real hardware is never mislabelled as a simulation; a plain browser is always "web". */
export async function getRuntimeMode(): Promise<RuntimeMode> {
  if (!isTauri()) return "web";
  try {
    return await invoke<RuntimeMode>("get_runtime_mode");
  } catch {
    return "mock";
  }
}

export async function getGasSafetyLimits(): Promise<GasLimits> {
  return isTauri()
    ? invoke("get_gas_safety_limits")
    : { h2: 5, co: 5, flow_steps: [0.01, 0.1, 0.01, 0.01] };
}

/** Cadences from the backend's loaded config; the browser mock mirrors docs/06 §6.3. */
export async function getRuntimeSettings(): Promise<RuntimeSettings> {
  return isTauri()
    ? invoke("get_runtime_settings")
    : {
        sampling_interval_s: 1,
        telemetry_interval_s: 0.2,
        heartbeat_timeout_s: 5,
        idle_timeout_s: 900,
      };
}

export async function setGasFlow(
  gas: number,
  value: number,
): Promise<ClampResult> {
  return isTauri()
    ? invoke("set_gas_flow", { gas, value })
    : mockSetFlow(gas, value);
}

export async function setGasOnOff(gas: number, on: boolean): Promise<void> {
  if (!isTauri()) return void mockSetOnOff(gas, on);
  await invoke("set_gas_on_off", { gas, on });
}

export async function openAllValves(): Promise<void> {
  if (!isTauri()) return mockSetAllValves(true);
  await invoke("open_all_valves");
}

export async function closeAllValves(): Promise<void> {
  if (!isTauri()) return mockSetAllValves(false);
  await invoke("close_all_valves");
}

export async function shutoffFlammableGases(): Promise<void> {
  if (!isTauri()) return void mockShutoff();
  await invoke("shutoff_flammable_gases");
}

export async function tareBalance(): Promise<TareResult> {
  return isTauri() ? invoke("tare_balance") : mockTare();
}

export async function startExperiment(
  params: ExperimentParams,
): Promise<string> {
  return isTauri()
    ? invoke("start_experiment", { params })
    : mockStartExperiment(params);
}

export async function stopExperiment(reason: string): Promise<void> {
  if (!isTauri()) return void mockStopExperiment();
  await invoke("stop_experiment", { reason });
}

export async function skipStage(
  experiment_id: string,
  stage_index: number,
  reason: string,
): Promise<void> {
  if (!isTauri()) return void mockSkipStage(experiment_id, stage_index, reason);
  await invoke("skip_stage", { experiment_id, stage_index, reason });
}
export async function acknowledgeStage(
  experiment_id: string,
  stage_index: number,
  notes: string,
): Promise<void> {
  if (!isTauri())
    return void mockAcknowledgeStage(experiment_id, stage_index, notes);
  await invoke("acknowledge_stage", { experiment_id, stage_index, notes });
}

export async function listPrograms(): Promise<ProgramDto[]> {
  return isTauri() ? invoke("list_programs") : mockListPrograms();
}

export async function listExperiments(): Promise<ExperimentSummary[]> {
  return isTauri() ? invoke("list_experiments") : mockListExperiments();
}

export async function getExperimentDetail(
  id: string,
): Promise<ExperimentDetail> {
  return isTauri()
    ? invoke("get_experiment_detail", { id })
    : mockGetExperimentDetail(id);
}

export async function getExperimentSamples(
  id: string,
  maxPoints?: number,
): Promise<SampleDto[]> {
  return isTauri()
    ? invoke("get_experiment_samples", { id, max_points: maxPoints })
    : mockGetExperimentSamples(id, maxPoints);
}

export async function deleteExperiment(id: string): Promise<void> {
  if (!isTauri()) return void mockDeleteExperiment(id);
  await invoke("delete_experiment", { id });
}

export async function analyzeGb13241(
  id: string,
  input: ReductionInput,
): Promise<Record<string, unknown>> {
  return isTauri()
    ? invoke("analyze_gb13241", { id, input })
    : mockAnalyze13241(id, input);
}
export async function analyzeGb13242(
  id: string,
  input: RdiInput,
): Promise<Record<string, unknown>> {
  return isTauri()
    ? invoke("analyze_gb13242", { id, input })
    : mockAnalyze13242(id, input);
}
export async function analyzeGb13240(
  id: string,
  input: SwellingInput,
): Promise<Record<string, unknown>> {
  return isTauri()
    ? invoke("analyze_gb13240", { id, input })
    : mockAnalyze13240(id, input);
}

export type ExportFormat = "csv" | "txt" | "xlsx";

function downloadBlob(content: BlobPart, filename: string, mime: string): void {
  const url = URL.createObjectURL(new Blob([content], { type: mime }));
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

/** Generate the HTML report. Tauri writes+opens a file (returns path); web opens a tab. */
export async function generateReport(id: string): Promise<string | void> {
  if (isTauri()) return invoke<string>("generate_report", { id });
  const w = window.open("", "_blank");
  if (w) {
    w.document.write(mockReportHtml(id));
    w.document.close();
  } else {
    downloadBlob(mockReportHtml(id), `report-${id}.html`, "text/html");
  }
}

/** Export the sample series. Tauri writes+opens a file; web downloads (xlsx→csv). */
export async function exportExperiment(
  id: string,
  format: ExportFormat,
): Promise<string | void> {
  if (isTauri()) return invoke<string>("export_experiment", { id, format });
  // Browser mock can't build a real .xlsx — fall back to CSV (Excel-openable).
  const sep = format === "txt" ? "\t" : ",";
  const ext = format === "txt" ? "txt" : "csv";
  downloadBlob(
    mockExportText(id, sep),
    `${id}.${ext}`,
    "text/plain;charset=utf-8",
  );
}

const APP_VERSION = "0.2.0";

export async function getUpdaterStatus(): Promise<UpdateCheck> {
  return isTauri()
    ? invoke("get_updater_status")
    : { enabled: false, current_version: APP_VERSION, available: false };
}

export async function checkForUpdate(): Promise<UpdateCheck> {
  return isTauri()
    ? invoke("check_for_update")
    : {
        enabled: false,
        current_version: APP_VERSION,
        available: false,
        error: "浏览器预览模式不支持更新检查",
      };
}

export async function installUpdate(): Promise<void> {
  if (!isTauri()) throw new Error("浏览器预览模式不支持安装更新");
  await invoke("install_update");
}

export function subscribeExperimentState(
  cb: (s: ExperimentState) => void,
): () => void {
  if (!isTauri()) return mockSubscribeExperiment(cb);
  let cancelled = false;
  let un: (() => void) | null = null;
  let gotEvent = false;
  // If we unmount before listen() resolves, unlisten as soon as it does (no leak); and let
  // live events win over the initial snapshot so a late query can't clobber newer state (#8).
  void listen<ExperimentState>("experiment-state-changed", (e) => {
    gotEvent = true;
    cb(e.payload);
  }).then((u) => {
    if (cancelled) u();
    else un = u;
  });
  void invoke<ExperimentState>("get_experiment_state").then((s) => {
    if (!cancelled && !gotEvent) cb(s);
  });
  return () => {
    cancelled = true;
    un?.();
  };
}

/** Subscribe to the 0.2s telemetry stream. Returns an unsubscribe function. */
export function subscribeTelemetry(
  cb: (s: TelemetrySnapshot) => void,
): () => void {
  if (!isTauri()) return mockTelemetry(cb);
  const channel = new Channel<TelemetrySnapshot>();
  channel.onmessage = cb;
  void invoke("subscribe_telemetry", { channel });
  return () => {
    channel.onmessage = () => {};
    // Tell the backend to stop the pump thread — nulling onmessage alone leaks it (#8).
    void invoke("unsubscribe_telemetry");
  };
}

export async function saveCustomProgram(
  program: ProgramDto,
  reason: string,
): Promise<ProgramDto> {
  return isTauri()
    ? invoke("save_custom_program", { program, reason })
    : mockSaveCustomProgram(program, reason);
}

export async function listCustomProgramRevisions(
  id: string,
): Promise<ProgramDto[]> {
  return isTauri()
    ? invoke("list_custom_program_revisions", { id })
    : mockListCustomProgramRevisions(id);
}

export async function deleteCustomProgram(id: string): Promise<void> {
  if (!isTauri()) return void mockDeleteCustomProgram(id);
  await invoke("delete_custom_program", { id });
}

export async function preflightExperiment(
  params: ExperimentParams,
): Promise<ExperimentPreflightResult> {
  return isTauri()
    ? invoke("preflight_experiment", { params })
    : mockPreflightExperiment(params);
}

export async function generateSampleId(
  programId: string,
  experimentTime: string,
): Promise<string> {
  return isTauri()
    ? invoke("generate_sample_id", {
        program_id: programId,
        experiment_time: experimentTime,
      })
    : mockGenerateSampleId(programId, experimentTime);
}

export async function getExperimentStartDefaults(
  programId: string,
): Promise<ExperimentStartDefaults> {
  return isTauri()
    ? invoke("get_experiment_start_defaults", { program_id: programId })
    : mockGetExperimentStartDefaults(programId);
}

export async function saveReportDetails(
  id: string,
  details: ReportDetails,
): Promise<void> {
  if (isTauri()) await invoke("save_report_details", { id, details });
  else mockSaveReportDetails(id, details);
}

export async function resetSafetyInterlock(): Promise<void> {
  if (isTauri()) await invoke("reset_safety_interlock");
  else mockResetSafety();
}

export async function retryPendingSave(): Promise<void> {
  if (!isTauri())
    throw new Error("浏览器预览没有本地数据库，请在桌面应用中重试保存");
  await invoke("retry_pending_save");
}

export function subscribeAppNotices(cb: (message: string) => void): () => void {
  if (!isTauri()) return () => {};
  let cancelled = false;
  let unlisten: (() => void) | undefined;
  void listen<string>("app-notice", (e) => cb(e.payload)).then((stop) => {
    if (cancelled) stop();
    else unlisten = stop;
  });
  return () => {
    cancelled = true;
    unlisten?.();
  };
}
