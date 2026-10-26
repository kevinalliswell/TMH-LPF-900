// Generated from Rust IPC DTOs. Do not edit by hand.

export type AppInfo = { name: string, version: string, };
export type AuthSnapshot = { status: AuthStatus, user: PublicUser | null, idle_timeout_s: number, };
export type AuthStatus = "SETUP_REQUIRED" | "ANONYMOUS" | "AUTHENTICATED" | "PASSWORD_CHANGE_REQUIRED" | "LOCKED";
export type ClampResult = { 
/**
 * Value that will actually be sent.
 */
applied: number, 
/**
 * True if the request was reduced.
 */
clamped: boolean, 
/**
 * The effective limit (config ∧ absolute ceiling); `INFINITY` for unlimited gases.
 */
limit: number, };
export type CommConfig = { COM_RS485_MFC: SerialParams, COM_RS485_TEMP: TempParams, COM_RS232_Balance: SerialParams, SAMPLING: Sampling, PERFORMANCE_CONFIG: Performance, SLAVE_ADDRESS_MFC: SlaveAddrMfc, FLOW_SCALING: FlowScaling, GAS_SAFETY_LIMITS: GasSafetyLimits, };
export type CommandError = { code: string, message_zh: string, detail: string | null, };
export type ConfigSnapshot = { path: string, revision: string, applied: CommConfig, saved: CommConfig, restart_required: boolean, };
export type DeviceHealth = { safety_latched: boolean, temp_valid: boolean, mfc_valid: boolean, balance_valid: boolean, temp_connected: boolean, mfc_connected: boolean, balance_connected: boolean, };
export type DeviceHealthDto = { temp: HealthDot, mfc: HealthDot, balance: HealthDot, };
export type DeviceMode = "mock" | "real" | "replay";
export type ExperimentDetail = { revisions: Record<string, unknown>[], runtime: Record<string, unknown>, program_snapshot?: Program | null, sample_id?: string | null, project_name?: string | null, experiment_time?: string | null, experiment_id: string, experiment_name: string, sample_name: string, sample_weight: number, start_time: string, end_time?: string | null, description?: string | null, operator?: string | null, experiment_type?: string | null, analysis?: Record<string, unknown> | null, created_at: string, sample_count: number, owner_user_id?: string | null, };
export type ExperimentPreflightResult = { ok: boolean, blocking: Array<PreflightIssue>, warnings: Array<PreflightIssue>, program: Program | null, };
export type ExperimentRow = { sample_id?: string | null, project_name?: string | null, experiment_time?: string | null, program_snapshot_json?: string | null, experiment_id: string, experiment_name: string, sample_name: string, sample_weight: number, start_time: string, end_time?: string | null, description?: string | null, operator?: string | null, experiment_type?: string | null, analysis_results_json?: string | null, created_at: string, owner_user_id?: string | null, };
export type ExperimentStartDefaults = { sample_id: string, sample_name: string, sample_weight: number | null, operator: string, experiment_type: string, project_name: string | null, experiment_time: string, notes: string | null, };
export type ExperimentStatus = { updating: boolean, persistence_pending: boolean, running: boolean, experiment_id: string | null, mode: string | null, sample_name: string | null, started_at: string | null, phase: Phase, stage_index: number, total_stages: number, stage_name: string | null, stage_kind: StageKind | null, target_temp: number, tolerance: number, stage_elapsed_s: number, stable_elapsed_s: number, total_elapsed_s: number, owner_user_id: string | null, owner_display_name: string | null, can_control: boolean, occupied_by_other: boolean, };
export type ExperimentSummary = { sample_id?: string | null, project_name?: string | null, experiment_time?: string | null, experiment_id: string, experiment_name: string, sample_name: string, sample_weight: number, start_time: string, end_time?: string | null, operator?: string | null, experiment_type?: string | null, has_analysis: boolean, owner_user_id?: string | null, };
export type FlowScaling = { H2: number, N2: number, CO2: number, CO: number, };
export type FrozenTrial = { process_assessment?: ProcessAssessment | null, experiment: ExperimentRow, runtime: Record<string, unknown>, analysis_revision_id: number | null, exclusion_reason: string | null, };
export type GasLimits = { h2: number, co: number, flow_steps: Array<number>, };
export type GasReading = { pv: number, sv: number, on: boolean, };
export type GasSafetyLimits = { H2: number, CO: number, };
export type GroupDecision = { required_count: number, selected_indices: Array<number>, };
export type GroupEvaluation = { calculation_version: string, rounding_version: string, limits: LimitSpec, decision_values: Array<number>, decision: GroupDecision, 
/**
 * Fixed-scale final strings; null means a required reference result is missing.
 */
reported: { [key in string]: string | null }, explanation: string, };
export type GroupPolicy = { method: string, custom: boolean, material: MaterialKind, manual_limits: LimitSpec | null, };
export type GroupReportDetails = { lab_name: string, lab_address: string, report_number: string, report_date: string, reviewer: string, deviations: string, };
export type GroupRequest = { group_id: string | null, expected_revision: number | null, name: string, batch_id: string, same_batch_confirmed: boolean, report_details: GroupReportDetails, material: MaterialKind, manual_limits: LimitSpec | null, trials: Array<TrialSelection>, reason: string, };
export type GroupSnapshot = { schema_version: number, name: string, batch_id: string, same_batch_confirmed: boolean, policy: GroupPolicy, report_details: GroupReportDetails, 
/**
 * Map evaluation decision positions to all frozen trials, including exclusions.
 */
valid_trial_indices: Array<number>, trials: Array<FrozenTrial>, evaluation: GroupEvaluation, data_source: string, method_conformity: string, recorded_by: string, reason: string, };
export type HealthDot = { connected: boolean, consecutive_errors: number, };
export type JsonValue = number | string | boolean | Array<JsonValue> | { [key in string]: JsonValue } | null;
export type LimitSpec = { a: number, b: number, c: number, basis: string, };
export type MaterialKind = "sinter" | "pellet" | "natural";
export type Performance = { data_collection_interval: number, 
/**
 * Legacy JSON alias of SAMPLING.interval_s; validation requires equality.
 */
experiment_data_interval: number, heartbeat_timeout: number, };
export type Phase = "IDLE" | "CONFIGURING" | "RUNNING" | "STAGE_TRANSITION" | "COMPLETING" | "STOPPING" | "ERROR";
export type PreflightIssue = { code: string, message_zh: string, };
export type ProcessAssessment = { version: string, 
/**
 * Recorded persistence status, not a conclusion inferred from surviving rows.
 */
data_complete?: boolean | null, 
/**
 * Known buffer discards; recovery/legacy records may not know the count.
 */
dropped_samples?: number | null, unassigned_points: number, stages: Array<StageEvidence>, method_conformity: string, };
export type Program = { provenance?: ProgramProvenance | null, template_revision?: string | null, base_standard_id?: string | null, base_standard_version?: string | null, id: string, name: string, description: string, category: string, enabled: boolean, sample_prefix: string, source: ProgramSource, stages: Array<Stage>, };
export type ProgramChange = { stage_index: number, field: string, before: JsonValue, after: JsonValue, };
export type ProgramProvenance = { revision: number, parent_template: Program, edited_by_user_id: string, edited_by: string, edited_at: string, reason: string, changes: Array<ProgramChange>, };
export type ProgramSource = "standard" | "custom";
export type PublicSafetyStatus = { experiment_running: boolean, mfc_connected: boolean, temp_connected: boolean, };
export type PublicUser = { user_id: string, username: string, display_name: string, role: UserRole, is_active: boolean, must_change_password: boolean, created_at: string, last_login_at?: string | null, };
export type RdiInput = { m0: number, m1: number, m2: number, m3: number, drum_rpm: number, drum_revolutions: number, sieve_seconds: number, };
export type ReductionInput = { w1: number, w2: number, manual_m1?: number | null, manual_m180?: number | null, 
/**
 * Total mass at the endpoint stored in the executed program snapshot.
 */
manual_mt?: number | null, manual_rate_points?: Array<[number, number]> | null, source_record?: string | null, };
export type ReductionSampleEvidence = { version: string, scope: string, total_sample_count: number, reduction_sample_count: number, valid_sample_count: number, excluded_offline_rows: number, excluded_other_quality_rows: number, excluded_time_rows: number, excluded_value_rows: number, non_reduction_rows: number, 
/**
 * Subset of valid_sample_count; the baseline is excluded only from gas checks.
 */
valid_baseline_count: number, };
export type ReportDetails = { lab_name: string, lab_address: string, report_number: string, report_date: string, sample_preparation: string, equipment: string, gas_and_calibration: string, external_steps: string, deviations: string, reviewer: string, repeat_group_reference: string, };
export type RuntimeModeStatus = { requested_mode: DeviceMode, effective_mode: DeviceMode, };
export type RuntimeSettings = { sampling_interval_s: number, telemetry_interval_s: number, heartbeat_timeout_s: number, idle_timeout_s: number, };
export type SampleDto = { quality?: string | null, record?: Record<string, unknown> | null, timestamp: string, experiment_duration?: string | null, temperature: number, weight: number, weight_loss: number, co_flow: number, co2_flow: number, n2_flow: number, h2_flow: number, experiment_status?: string | null, };
export type Sampling = { interval_s: number, };
export type SerialParams = { port: string, baudrate: number, bytesize: number, parity: string, stopbits: number, timeout: number, };
export type SlaveAddrMfc = { H2: number, N2: number, CO2: number, CO: number, };
export type Stage = { name: string, kind: StageKind, 
/**
 * Gas setpoints [H2, N2, CO2, CO] L/min.
 */
gas: Array<number>, target_temp: number, tolerance: number, 
/**
 * Hold duration (min); 0 = dynamic stage.
 */
duration_min: number, ramp_rate: number, description: string, };
export type StageEvidence = { index: number, name: string, kind: StageKind, points: number, valid_temperature_points: number, valid_gas_points: number, temperature_range: Array<number> | null, gas_pv_ranges: Array<Array<number> | null>, gas_sv_mismatches: number, elapsed_s: number | null, stable_elapsed_s: number | null, maximum_observed_ramp_c_min: number | null, maximum_sample_gap_s: number | null, target_temp: number, tolerance: number, gas_targets: Array<number>, required_seconds: number | null, notes: Array<string>, };
export type StageKind = "HEATING" | "STABILIZING" | "REDUCING" | "COOLING" | "PURGING" | "MANUAL";
export type SwellingInput = { v0: number, v1: number, pellet_count: number, volume_method: string, same_method: boolean, };
export type TareResultDto = { success: boolean, a00_found: boolean, weight: number | null, };
export type TelemetrySnapshot = { seq: number, temps: Array<number | null>, gas: Array<GasReading>, weight: number | null, health: DeviceHealth, };
export type TempParams = { port: string, baudrate: number, bytesize: number, parity: string, stopbits: number, timeout: number, slave_address: number, start_reg: number, reg_count: number, TEMP_CHANNELS: Array<string>, scale: number, signed_registers: boolean, };
export type TestGroupRevision = { group_id: string, revision: number, owner_user_id: string, recorded_by_user_id: string, created_at: string, snapshot: GroupSnapshot, };
export type TrialIndices = { experiment_id: string, primary: number, rvi: number | null, rdi_plus_6_3: number | null, rdi_minus_0_5: number | null, };
export type TrialSelection = { experiment_id: string, exclusion_reason: string | null, };
export type UpdateCheck = { 
/**
 * Whether update checking is enabled (config/env gate; OFF by default).
 */
enabled: boolean, current_version: string, available: boolean, new_version?: string | null, notes?: string | null, 
/**
 * A friendly message when the manifest endpoint is unreachable (offline工控机).
 */
error?: string | null, };
export type UserRole = "ADMIN" | "USER";
