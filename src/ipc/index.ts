// Typed IPC boundary. In Tauri it calls real commands + a telemetry Channel; in a
// plain browser it dispatches to the synthetic mock (docs/08). A single `isTauri()`
// switch keeps the UI identical in both.

import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  ClampResult,
  ExperimentParams,
  ExperimentState,
  GasLimits,
  TareResult,
  TelemetrySnapshot,
} from "./types";
import {
  mockSetFlow,
  mockSetOnOff,
  mockShutoff,
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
