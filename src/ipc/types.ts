// Types mirroring the Rust IPC DTOs (src-tauri/src/lib.rs). Kept hand-written for
// M3; a tauri-specta codegen pass can replace this file mechanically later.

/** One gas channel reading (display L/min). Order: H2, N2, CO2, CO. */
export interface GasReading {
  pv: number;
  sv: number;
  on: boolean;
}

export interface DeviceHealth {
  safety_latched?: boolean;
  temp_valid?: boolean;
  mfc_valid?: boolean;
  balance_valid?: boolean;
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

/** Cadences the backend loaded from `comm_config.json` (docs/06 §6.3). The UI mirrors
 *  them instead of duplicating the numbers as constants (#44). */
export interface RuntimeSettings {
  sampling_interval_s: number;
  telemetry_interval_s: number;
  heartbeat_timeout_s: number;
  idle_timeout_s: number;
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
  owner_user_id?: string | null;
  owner_display_name?: string | null;
  can_control?: boolean;
  occupied_by_other?: boolean;
}

export type UserRole = "ADMIN" | "USER";
export type AuthStatus =
  | "SETUP_REQUIRED"
  | "ANONYMOUS"
  | "AUTHENTICATED"
  | "PASSWORD_CHANGE_REQUIRED"
  | "LOCKED";

export interface PublicUser {
  user_id: string;
  username: string;
  display_name: string;
  role: UserRole;
  is_active: boolean;
  must_change_password: boolean;
  created_at: string;
  last_login_at?: string | null;
}

export interface AuthSnapshot {
  status: AuthStatus;
  user?: PublicUser | null;
  idle_timeout_s: number;
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

export type ProgramSource = "standard" | "custom";

export interface ProgramDto {
  base_standard_id?: string | null;
  base_standard_version?: string | null;
  description: string;
  category: string;
  enabled: boolean;
  sample_prefix: string;
  source: ProgramSource;
  id: string;
  name: string;
  stages: StageDto[];
}

/** History list row. */
export interface ExperimentSummary {
  sample_id?: string | null;
  project_name?: string | null;
  experiment_time?: string | null;
  experiment_id: string;
  experiment_name: string;
  sample_name: string;
  sample_weight: number;
  start_time: string;
  end_time?: string | null;
  operator?: string | null;
  experiment_type?: string | null;
  has_analysis: boolean;
  owner_user_id?: string | null;
}

export interface ExperimentDetail {
  revisions?: Record<string, unknown>[];
  runtime?: Record<string, unknown>;
  program_snapshot?: ProgramDto | null;
  sample_id?: string | null;
  project_name?: string | null;
  experiment_time?: string | null;
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
  owner_user_id?: string | null;
}

export interface SampleDto {
  quality?: string | null;
  record?: {
    telemetry?: {
      health?: {
        temp_valid?: boolean;
        balance_valid?: boolean;
        mfc_valid?: boolean;
      };
    };
  } | null;
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
  operator?: string;
  experiment_time?: string | null;
  sample_id: string;
  sample_name: string;
  sample_weight: number;
  experiment_type: string;
  project_name?: string | null;
  notes?: string | null;
}

/** Software-update check result (docs/09 §9.3). */
export interface UpdateCheck {
  enabled: boolean;
  current_version: string;
  available: boolean;
  new_version?: string | null;
  notes?: string | null;
  error?: string | null;
}

export const EXPERIMENT_MODES = [
  { id: "REDUCIBILITY", name: "GB/T 13241 还原性" },
  { id: "LOW_TEMP_DEGRADATION", name: "GB/T 13242 低温粉化" },
  { id: "FREE_SWELLING", name: "GB/T 13240 自由膨胀" },
] as const;

export interface PreflightIssue {
  code: string;
  message_zh: string;
}

export interface ExperimentPreflightResult {
  ok: boolean;
  blocking: PreflightIssue[];
  warnings: PreflightIssue[];
  program?: ProgramDto | null;
}

export interface ExperimentStartDefaults {
  sample_id: string;
  sample_name: string;
  sample_weight?: number | null;
  operator: string;
  experiment_type: string;
  project_name?: string | null;
  experiment_time: string;
  notes?: string | null;
}

export interface ReductionInput {
  w1: number;
  w2: number;
  manual_m1?: number | null;
  manual_m180?: number | null;
  manual_rate_points?: [number, number][] | null;
  source_record?: string | null;
}
export interface RdiInput {
  m0: number;
  m1: number;
  m2: number;
  m3: number;
  drum_rpm: number;
  drum_revolutions: number;
  sieve_seconds: number;
}
export interface SwellingInput {
  v0: number;
  v1: number;
  pellet_count: number;
  volume_method: string;
  same_method: boolean;
}

export interface ReportDetails {
  lab_name: string;
  lab_address: string;
  report_number: string;
  report_date: string;
  sample_preparation: string;
  equipment: string;
  gas_and_calibration: string;
  external_steps: string;
  deviations: string;
  reviewer: string;
  repeat_group_reference: string;
}
