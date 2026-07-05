// Types mirroring the Rust IPC DTOs (src-tauri/src/lib.rs). Kept hand-written for
// M3; a tauri-specta codegen pass can replace this file mechanically later.

/** One gas channel reading (display L/min). Order: H2, N2, CO2, CO. */
export interface GasReading {
  pv: number;
  sv: number;
  on: boolean;
}

export interface DeviceHealth {
  temp_connected: boolean;
  mfc_connected: boolean;
  balance_connected: boolean;
}

/** Merged 0.2s telemetry frame from `subscribe_telemetry`. */
export interface TelemetrySnapshot {
  seq: number;
  temps: (number | null)[]; // 9 channels, T8 (index 7) = sample temperature
  gas: GasReading[]; // 4 channels
  weight: number | null;
  health: DeviceHealth;
}

export interface ClampResult {
  applied: number;
  clamped: boolean;
  limit: number;
}

export interface TareResult {
  success: boolean;
  a00_found: boolean;
  weight: number | null;
}

export interface GasLimits {
  h2: number;
  co: number;
}

/** Gas channel indices, matching the Rust actor ordering. */
export const GAS = { H2: 0, N2: 1, CO2: 2, CO: 3 } as const;
export const GAS_NAMES = ["H₂", "N₂", "CO₂", "CO"] as const;
/** Flammable gases cut by the emergency shutoff. */
export const FLAMMABLE = [GAS.H2, GAS.CO2, GAS.CO];

export type Phase =
  | "IDLE"
  | "CONFIGURING"
  | "RUNNING"
  | "STAGE_TRANSITION"
  | "COMPLETING"
  | "STOPPING"
  | "ERROR";

export type StageKind = "HEATING" | "STABILIZING" | "REDUCING" | "COOLING";

export interface ExperimentState {
  running: boolean;
  experiment_id?: string | null;
  mode?: string | null;
  sample_name?: string | null;
  started_at?: string | null;
  phase?: Phase;
  stage_index?: number;
  total_stages?: number;
  stage_name?: string | null;
  stage_kind?: StageKind | null;
  target_temp?: number;
  tolerance?: number;
  stage_elapsed_s?: number;
  total_elapsed_s?: number;
}

/** One program stage (mirrors tmh_core::engine::Stage). */
export interface StageDto {
  name: string;
  kind: StageKind;
  gas: number[]; // [H2, N2, CO2, CO] L/min
  target_temp: number;
  tolerance: number;
  duration_min: number;
  ramp_rate: number;
  description: string;
}

export interface ProgramDto {
  id: string;
  name: string;
  stages: StageDto[];
}

/** History list row. */
export interface ExperimentSummary {
  experiment_id: string;
  experiment_name: string;
  sample_name: string;
  sample_weight: number;
  start_time: string;
  end_time?: string | null;
  operator?: string | null;
  experiment_type?: string | null;
  has_analysis: boolean;
}

export interface ExperimentDetail {
  experiment_id: string;
  experiment_name: string;
  sample_name: string;
  sample_weight: number;
  start_time: string;
  end_time?: string | null;
  description?: string | null;
  operator?: string | null;
  experiment_type?: string | null;
  analysis?: Record<string, unknown> | null;
  created_at: string;
  sample_count: number;
}

export interface SampleDto {
  timestamp: string;
  experiment_duration?: string | null;
  temperature: number;
  weight: number;
  weight_loss: number;
  co_flow: number;
  co2_flow: number;
  n2_flow: number;
  h2_flow: number;
  experiment_status?: string | null;
}

export interface ExperimentParams {
  sample_id: string;
  sample_name: string;
  sample_weight: number;
  operator: string;
  experiment_type: string;
  project_name?: string | null;
  notes?: string | null;
}

export const EXPERIMENT_MODES = [
  { id: "REDUCIBILITY", name: "GB/T 13241 还原性" },
  { id: "LOW_TEMP_DEGRADATION", name: "GB/T 13242 低温粉化" },
  { id: "FREE_SWELLING", name: "GB/T 13240 自由膨胀" },
] as const;
