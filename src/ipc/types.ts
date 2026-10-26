/**
 * Rust DTOs are generated in `types.generated.ts` from the Serde types that cross IPC.
 * These remaining declarations describe browser/UI state that is not a wire DTO.
 */
export type {
  AuthSnapshot,
  AuthStatus,
  ClampResult,
  DeviceHealth,
  ExperimentDetail,
  ExperimentPreflightResult,
  ExperimentStartDefaults,
  ExperimentStatus as ExperimentStateDto,
  ExperimentSummary,
  GasLimits,
  GasReading,
  Phase,
  PreflightIssue,
  ProgramChange,
  Program as ProgramDto,
  ProgramProvenance,
  ProgramSource,
  PublicUser,
  RdiInput,
  ReductionInput,
  ReductionSampleEvidence,
  ReportDetails,
  RuntimeSettings,
  SampleDto,
  Stage as StageDto,
  StageKind,
  SwellingInput,
  TareResultDto as TareResult,
  TelemetrySnapshot,
  UpdateCheck,
  UserRole,
  DeviceMode,
  RuntimeModeStatus,
} from "./types.generated";

import type { ExperimentStatus as ExperimentStateDto } from "./types.generated";

/** The UI also accepts partial state while stores and component tests initialize. */
export type ExperimentState = Partial<ExperimentStateDto> &
  Pick<ExperimentStateDto, "running">;

/** Gas channel indices, matching the Rust actor ordering. */
export const GAS = { H2: 0, N2: 1, CO2: 2, CO: 3 } as const;
export const GAS_NAMES = ["H₂", "N₂", "CO₂", "CO"] as const;
/** Flammable gases cut by the emergency shutoff. */
export const FLAMMABLE = [GAS.H2, GAS.CO2, GAS.CO];

export const EXPERIMENT_MODES = [
  { id: "REDUCIBILITY", name: "GB/T 13241 还原性" },
  { id: "LOW_TEMP_DEGRADATION", name: "GB/T 13242 低温粉化" },
  { id: "FREE_SWELLING", name: "GB/T 13240 自由膨胀" },
] as const;

/** UI start-form model; the IPC command receives its fields as separate arguments. */
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
