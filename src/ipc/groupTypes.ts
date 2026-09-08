// Frozen, versioned group DTOs from tmh_core::test_group / repeat_group / storage.
export type MaterialKind = "sinter" | "pellet" | "natural";
export interface GroupLimits { a: number; b: number; c: number; basis: string }
export interface GroupReportDetails {
  lab_name: string; lab_address: string; report_number: string;
  report_date: string; reviewer: string; deviations: string;
}
export interface GroupTrialSelection { experiment_id: string; exclusion_reason: string | null }
export interface GroupRequest {
  group_id: string | null; expected_revision: number | null;
  name: string; batch_id: string; same_batch_confirmed: boolean;
  report_details: GroupReportDetails; material: MaterialKind;
  manual_limits: GroupLimits | null; trials: GroupTrialSelection[]; reason: string;
}
export interface FrozenTrial {
  experiment: {
    experiment_id: string; experiment_name: string; sample_name: string; sample_weight: number;
    start_time: string; created_at: string; end_time: string | null;
    sample_id: string | null; project_name: string | null; experiment_time: string | null;
    program_snapshot_json: string | null; description: string | null; operator: string | null;
    experiment_type: string | null; analysis_results_json: string | null; owner_user_id: string | null;
  };
  runtime: Record<string, unknown>;
  analysis_revision_id: number | null;
  exclusion_reason: string | null;
}
export interface GroupSnapshot {
  schema_version: number; name: string; batch_id: string; same_batch_confirmed: boolean;
  policy: { method: string; custom: boolean; material: MaterialKind; manual_limits: GroupLimits | null };
  report_details: GroupReportDetails; valid_trial_indices: number[]; trials: FrozenTrial[];
  evaluation: {
    calculation_version: string; rounding_version: string; limits: GroupLimits; decision_values: number[];
    decision: { required_count: number; selected_indices: number[] };
    reported: Record<string, string | null>; explanation: string;
  };
  data_source: string; method_conformity: string; recorded_by: string; reason: string;
}
export interface TestGroupRevision {
  group_id: string; revision: number; owner_user_id: string; recorded_by_user_id: string;
  created_at: string; snapshot: GroupSnapshot;
}
