// Typed IPC boundary. In Tauri it calls real commands + a telemetry Channel; in a
// plain browser it dispatches to the synthetic mock (docs/08). A single `isTauri()`
// switch keeps the UI identical in both.

import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  ClampResult,
  AdminAuthStatus,
  CommConfigDto,
  ExperimentDetail,
  ExperimentParams,
  ExperimentPreflightResult,
  ExperimentStartDefaults,
  ExperimentState,
  ExperimentSummary,
  GasLimits,
  ProgramDto,
  SampleDto,
  TareResult,
  TelemetrySnapshot,
  UpdateCheck,
} from "./types";
import {
  mockAnalyze13240,
  mockAnalyze13241,
  mockAnalyze13242,
  mockChangeAdminPassword,
  mockGenerateSampleId,
  mockGetAdminAuthStatus,
  mockGetCommConfig,
  mockGetExperimentStartDefaults,
  mockDeleteCustomProgram,
  mockDeleteExperiment,
  mockExportText,
  mockGetExperimentDetail,
  mockGetExperimentSamples,
  mockListExperiments,
  mockListPrograms,
  mockPreflightExperiment,
  mockSaveCustomProgram,
  mockUpdateCommConfig,
  mockReportHtml,
  mockSetFlow,
  mockSetOnOff,
  mockShutoff,
  mockSkipStage,
  mockStartExperiment,
  mockStopExperiment,
  mockSubscribeExperiment,
  mockTare,
  mockTelemetry,
} from "./mock";

export * from "./types";

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
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
  return isTauri() ? invoke("get_gas_safety_limits") : { h2: 5, co: 5 };
}

export async function getCommConfig(): Promise<CommConfigDto> {
  return isTauri() ? invoke("get_comm_config") : mockGetCommConfig();
}

export async function getAdminAuthStatus(): Promise<AdminAuthStatus> {
  return isTauri() ? invoke("get_admin_auth_status") : mockGetAdminAuthStatus();
}

export async function changeAdminPassword(oldPassword: string, newPassword: string): Promise<void> {
  if (!isTauri()) return void mockChangeAdminPassword(oldPassword, newPassword);
  await invoke("change_admin_password", { old_password: oldPassword, new_password: newPassword });
}

export async function updateCommConfig(config: CommConfigDto, password: string): Promise<CommConfigDto> {
  return isTauri()
    ? invoke("update_comm_config", { config, password })
    : mockUpdateCommConfig(config, password);
}

export async function setGasFlow(gas: number, value: number): Promise<ClampResult> {
  return isTauri() ? invoke("set_gas_flow", { gas, value }) : mockSetFlow(gas, value);
}

export async function setGasOnOff(gas: number, on: boolean): Promise<void> {
  if (!isTauri()) return void mockSetOnOff(gas, on);
  await invoke("set_gas_on_off", { gas, on });
}

export async function openAllValves(): Promise<void> {
  if (!isTauri()) return void [0, 1, 2, 3].forEach((g) => mockSetOnOff(g, true));
  await invoke("open_all_valves");
}

export async function closeAllValves(): Promise<void> {
  if (!isTauri()) return void [0, 1, 2, 3].forEach((g) => mockSetOnOff(g, false));
  await invoke("close_all_valves");
}

export async function shutoffFlammableGases(): Promise<void> {
  if (!isTauri()) return void mockShutoff();
  await invoke("shutoff_flammable_gases");
}

export async function tareBalance(): Promise<TareResult> {
  return isTauri() ? invoke("tare_balance") : mockTare();
}

export async function startExperiment(params: ExperimentParams): Promise<string> {
  return isTauri() ? invoke("start_experiment", { params }) : mockStartExperiment(params);
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
    ? invoke("generate_sample_id", { program_id: programId, experiment_time: experimentTime })
    : mockGenerateSampleId(programId, experimentTime);
}

export async function getExperimentStartDefaults(
  programId: string,
): Promise<ExperimentStartDefaults> {
  return isTauri()
    ? invoke("get_experiment_start_defaults", { program_id: programId })
    : mockGetExperimentStartDefaults(programId);
}

export async function stopExperiment(reason: string): Promise<void> {
  if (!isTauri()) return void mockStopExperiment();
  await invoke("stop_experiment", { reason });
}

export async function skipStage(reason: string): Promise<void> {
  if (!isTauri()) return void mockSkipStage(reason);
  await invoke("skip_stage", { reason });
}

export async function listPrograms(): Promise<ProgramDto[]> {
  return isTauri() ? invoke("list_programs") : mockListPrograms();
}

export async function saveCustomProgram(program: ProgramDto): Promise<ProgramDto> {
  return isTauri()
    ? invoke("save_custom_program", { program })
    : mockSaveCustomProgram(program);
}

export async function deleteCustomProgram(id: string): Promise<void> {
  if (!isTauri()) return void mockDeleteCustomProgram(id);
  await invoke("delete_custom_program", { id });
}

export async function listExperiments(): Promise<ExperimentSummary[]> {
  return isTauri() ? invoke("list_experiments") : mockListExperiments();
}

export async function getExperimentDetail(id: string): Promise<ExperimentDetail> {
  return isTauri() ? invoke("get_experiment_detail", { id }) : mockGetExperimentDetail(id);
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
  o2Pct: number,
): Promise<Record<string, unknown>> {
  return isTauri()
    ? invoke("analyze_gb13241", { id, o2_pct: o2Pct })
    : mockAnalyze13241(id, o2Pct);
}

export async function analyzeGb13242(
  id: string,
  masses: { m0: number; mGt63: number; m31563: number; m05315: number; mLt05: number },
): Promise<Record<string, unknown>> {
  return isTauri()
    ? invoke("analyze_gb13242", {
        id,
        m0: masses.m0,
        m_gt63: masses.mGt63,
        m_315_63: masses.m31563,
        m_05_315: masses.m05315,
        m_lt05: masses.mLt05,
      })
    : mockAnalyze13242(id, masses);
}

export async function analyzeGb13240(
  id: string,
  volumes: [number, number][],
): Promise<Record<string, unknown>> {
  return isTauri()
    ? invoke("analyze_gb13240", { id, volumes })
    : mockAnalyze13240(id, volumes);
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
export async function exportExperiment(id: string, format: ExportFormat): Promise<string | void> {
  if (isTauri()) return invoke<string>("export_experiment", { id, format });
  // Browser mock can't build a real .xlsx — fall back to CSV (Excel-openable).
  const sep = format === "txt" ? "\t" : ",";
  const ext = format === "txt" ? "txt" : "csv";
  downloadBlob(mockExportText(id, sep), `${id}.${ext}`, "text/plain;charset=utf-8");
}

const APP_VERSION = "0.1.0";

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

export function subscribeExperimentState(cb: (s: ExperimentState) => void): () => void {
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
export function subscribeTelemetry(cb: (s: TelemetrySnapshot) => void): () => void {
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
