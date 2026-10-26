//! Generated TypeScript declarations for data crossing the Tauri IPC boundary.
//!
//! Rust + Serde types are authoritative. The `typescript-export` feature is enabled
//! only by the contract check; normal application builds do not include ts-rs.

use ts_rs::{Config, TS};

pub(crate) fn declarations() -> String {
    macro_rules! decls {
        ($($ty:ty),+ $(,)?) => {{
            // Tauri's JSON bridge delivers JavaScript numbers; mirror that wire shape.
            let config = Config::new()
                .with_large_int("number")
                .with_array_tuple_limit(0);
            let mut declarations = vec![$(
                format!("export {}", <$ty as TS>::decl(&config))
            ),+];
            declarations.sort();
            format!(
                "// Generated from Rust IPC DTOs. Do not edit by hand.\n\n{}\n",
                declarations.join("\n")
            )
        }};
    }

    decls![
        super::AppInfo,
        super::GasLimits,
        super::RuntimeSettings,
        super::HealthDot,
        super::DeviceHealthDto,
        super::TareResultDto,
        super::PublicSafetyStatus,
        super::ExperimentStatus,
        super::PreflightIssue,
        super::ExperimentPreflightResult,
        super::ExperimentStartDefaults,
        super::ExperimentSummary,
        super::ExperimentDetail,
        super::SampleDto,
        super::ReportDetails,
        super::UpdateCheck,
        super::runtime_mode::DeviceMode,
        super::runtime_mode::RuntimeModeStatus,
        super::CommandError,
        serde_json::Value,
        super::auth_service::PublicUser,
        super::auth_service::AuthStatus,
        super::auth_service::AuthSnapshot,
        tmh_core::clamp::ClampResult,
        tmh_core::bus::GasReading,
        tmh_core::telemetry::DeviceHealth,
        tmh_core::telemetry::TelemetrySnapshot,
        tmh_core::engine::Phase,
        tmh_core::program::StageKind,
        tmh_core::program::ProgramSource,
        tmh_core::program::Stage,
        tmh_core::program::Program,
        tmh_core::provenance::ProgramChange,
        tmh_core::provenance::ProgramProvenance,
        tmh_core::auth::UserRole,
        tmh_core::gb::ReductionSampleEvidence,
        tmh_core::gb::ReductionInput,
        tmh_core::gb::RdiInput,
        tmh_core::gb::SwellingInput,
        tmh_core::config::CommConfig,
        tmh_core::config::SerialParams,
        tmh_core::config::TempParams,
        tmh_core::config::Sampling,
        tmh_core::config::Performance,
        tmh_core::config::SlaveAddrMfc,
        tmh_core::config::FlowScaling,
        tmh_core::config::GasSafetyLimits,
        tmh_core::config_store::ConfigSnapshot,
        tmh_core::repeat_group::MaterialKind,
        tmh_core::repeat_group::LimitSpec,
        tmh_core::repeat_group::GroupPolicy,
        tmh_core::repeat_group::GroupDecision,
        tmh_core::repeat_group::GroupEvaluation,
        tmh_core::repeat_group::TrialIndices,
        tmh_core::test_group::TrialSelection,
        tmh_core::test_group::GroupReportDetails,
        tmh_core::test_group::GroupRequest,
        tmh_core::test_group::FrozenTrial,
        tmh_core::test_group::GroupSnapshot,
        tmh_core::storage::TestGroupRevision,
        tmh_core::storage::ExperimentRow,
        tmh_core::process_evidence::StageEvidence,
        tmh_core::process_evidence::ProcessAssessment,
    ]
}

#[cfg(test)]
mod tests {
    use super::declarations;
    use std::path::PathBuf;

    const CHECKED_IN: &str = include_str!("../../src/ipc/types.generated.ts");

    #[test]
    fn generated_types_match_rust_serde_dtos() {
        let generated = declarations();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../src/ipc/types.generated.ts");

        if std::env::var_os("UPDATE_TMH_IPC_TYPES").is_some() {
            std::fs::write(path, &generated).expect("write generated IPC types");
            return;
        }

        assert_eq!(CHECKED_IN, generated, "run `pnpm generate:ipc-types`");
    }
}
