// Typed IPC boundary. In Tauri it calls real commands + a telemetry Channel; in a
// plain browser it dispatches to the synthetic mock (docs/08). A single `isTauri()`
// switch keeps the UI identical in both.

import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  ClampResult,
  ExperimentDetail,
  ExperimentParams,
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
  mockDeleteExperiment,
  mockExportText,
  mockGetExperimentDetail,
  mockGetExperimentSamples,
  mockListExperiments,
  mockListPrograms,
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

export function runtimeMode(): "mock" | "web" {
  return isTauri() ? "mock" : "web";
}

export async function getGasSafetyLimits(): Promise<GasLimits> {
  return isTauri() ? invoke("get_gas_safety_limits") : { h2: 5, co: 5 };
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

export async function stopExperiment(reason: string): Promise<void> {
  if (!isTauri()) return void mockStopExperiment();
  await invoke("stop_experiment", { reason });
}

export async function skipStage(): Promise<void> {
  if (!isTauri()) return void mockSkipStage();
  await invoke("skip_stage");
}

export async function listPrograms(): Promise<ProgramDto[]> {
  return isTauri() ? invoke("list_programs") : mockListPrograms();
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
  void invoke<ExperimentState>("get_experiment_state").then(cb);
  let un = () => {};
  void listen<ExperimentState>("experiment-state-changed", (e) => cb(e.payload)).then((u) => {
    un = u;
  });
  return () => un();
}

/** Subscribe to the 0.2s telemetry stream. Returns an unsubscribe function. */
export function subscribeTelemetry(cb: (s: TelemetrySnapshot) => void): () => void {
  if (!isTauri()) return mockTelemetry(cb);
  const channel = new Channel<TelemetrySnapshot>();
  channel.onmessage = cb;
  void invoke("subscribe_telemetry", { channel });
  return () => {
    channel.onmessage = () => {};
  };
}
