//! Authorization, frozen reads, file creation and terminal audit for local exports.
use super::*;
use std::path::Path;

fn run_export<T>(
    state: &AppState,
    action: &str,
    mut detail: serde_json::Value,
    export: impl FnOnce(&AppState, &UserRow, &mut serde_json::Value) -> CommandResult<T>,
) -> CommandResult<T> {
    let _lifecycle = lock(&state.exp.lifecycle);
    let (operator, authorization) = command_audit::authorize(state, UserRole::User);
    let outcome = (|| {
        let user = authorization?;
        if state.exp.closing.load(Ordering::SeqCst) {
            return Err(CommandError::from("APP_CLOSING"));
        }
        if state.exp.updating.load(Ordering::SeqCst) {
            return Err(CommandError::from("UPDATE_IN_PROGRESS"));
        }
        export(state, &user, &mut detail)
    })();
    command_audit::finish(state, Category::Export, action, operator, detail, outcome)
}

fn export_failed() -> CommandError {
    CommandError::new(
        "EXPORT_FAILED",
        "导出文件未能保存，请检查导出目录权限和磁盘空间",
    )
}

fn report_data_invalid() -> CommandError {
    CommandError::new("REPORT_DATA_INVALID", "记录证据无法读取，不能生成报告")
}

fn storage_error() -> CommandError {
    CommandError::from("STORAGE_ERROR")
}

fn safe_component(value: &str) -> CommandResult<String> {
    if value.is_empty() || value.len() > 128 {
        return Err(report_data_invalid());
    }
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    if encoded.len() > 180 {
        return Err(report_data_invalid());
    }
    Ok(encoded)
}

fn output_directory(state: &AppState, user: &UserRow) -> CommandResult<std::path::PathBuf> {
    let directory = state.exports_dir.join(safe_component(&user.user_id)?);
    std::fs::create_dir_all(&directory).map_err(|_| export_failed())?;
    Ok(directory)
}

fn write_file(path: &Path, contents: impl AsRef<[u8]>) -> CommandResult<()> {
    std::fs::write(path, contents).map_err(|_| export_failed())
}

pub(super) fn generate_test_group_report(
    state: &AppState,
    id: &str,
    revision: u64,
) -> CommandResult<String> {
    run_export(
        state,
        "generate_test_group_report",
        serde_json::json!({"group_id":id,"revision":revision}),
        |state, user, detail| {
            let record = lock(&state.storage)
                .test_group_revision(id, Some(revision))
                .map_err(|_| storage_error())?
                .ok_or_else(|| CommandError::from("检验组版本不存在"))?;
            if !may_access_experiment(user.role, &user.user_id, Some(&record.owner_user_id)) {
                return Err(CommandError::from("FORBIDDEN"));
            }
            let html =
                tmh_core::group_report::render(&record).map_err(|_| report_data_invalid())?;
            let evidence = serde_json::to_vec_pretty(&record).map_err(|_| report_data_invalid())?;
            let file_group = safe_component(&record.group_id)?;
            let directory = output_directory(state, user)?;
            let path = directory.join(format!("report-{file_group}-v{}.html", record.revision));
            let evidence_path = path.with_extension("json");
            write_file(&evidence_path, evidence)?;
            if let Err(error) = write_file(&path, html) {
                let _ = std::fs::remove_file(evidence_path);
                return Err(error);
            }
            let path = path.to_string_lossy().into_owned();
            detail["path"] = serde_json::json!(path);
            Ok(path)
        },
    )
}

pub(super) fn generate_report(state: &AppState, id: &str) -> CommandResult<String> {
    run_export(
        state,
        "generate_report",
        serde_json::json!({"id":id}),
        |state, user, detail| {
            let experiment = authorize_experiment(state, id, user).map_err(CommandError::from)?;
            let (samples, runtime_json) = {
                let storage = lock(&state.storage);
                (
                    storage.get_samples(id, 0).map_err(|_| storage_error())?,
                    storage.runtime_metadata(id).map_err(|_| storage_error())?,
                )
            };
            let runtime: serde_json::Value =
                serde_json::from_str(&runtime_json).map_err(|_| report_data_invalid())?;
            let mut analysis = experiment
                .analysis_results_json
                .as_deref()
                .map(serde_json::from_str::<serde_json::Value>)
                .transpose()
                .map_err(|_| report_data_invalid())?;
            let report = analysis.get_or_insert_with(|| serde_json::json!({}));
            report["report_details"] = runtime["report_details"].clone();
            report["runtime_evidence"] = runtime;
            let html = tmh_core::report::render_report_html(
                &experiment,
                &samples,
                analysis.as_ref(),
                reducing_target(experiment.experiment_type.as_deref()),
            );
            let file_id = safe_component(id)?;
            let directory = output_directory(state, user)?;
            let path = directory.join(format!("report-{file_id}.html"));
            write_file(&path, html)?;
            let path = path.to_string_lossy().into_owned();
            detail["path"] = serde_json::json!(path);
            Ok(path)
        },
    )
}

pub(super) fn export_experiment(state: &AppState, id: &str, format: &str) -> CommandResult<String> {
    run_export(
        state,
        "export_data",
        serde_json::json!({"id":id,"format":format}),
        |state, user, detail| {
            let experiment = authorize_experiment(state, id, user).map_err(CommandError::from)?;
            let samples = lock(&state.storage)
                .get_samples(id, 0)
                .map_err(|_| storage_error())?;
            let (bytes, extension) = match format {
                "csv" => (
                    tmh_core::report::samples_to_csv(&samples).into_bytes(),
                    "csv",
                ),
                "txt" => (
                    tmh_core::report::samples_to_tsv(&samples).into_bytes(),
                    "txt",
                ),
                "xlsx" => (
                    tmh_core::report::samples_to_xlsx(&experiment, &samples)
                        .map_err(|_| report_data_invalid())?,
                    "xlsx",
                ),
                _ => return Err(CommandError::from("INVALID_PARAMS")),
            };
            let file_id = safe_component(id)?;
            let directory = output_directory(state, user)?;
            let path = directory.join(format!("{file_id}.{extension}"));
            write_file(&path, bytes)?;
            let path = path.to_string_lossy().into_owned();
            detail["path"] = serde_json::json!(path);
            Ok(path)
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_name_components_cannot_escape_their_directory_or_overflow_a_filename() {
        assert_eq!(
            safe_component("../../outside").unwrap(),
            "%2E%2E%2F%2E%2E%2Foutside"
        );
        assert_ne!(
            safe_component("a/b").unwrap(),
            safe_component("a%2Fb").unwrap()
        );
        assert_eq!(safe_component("").unwrap_err().code, "REPORT_DATA_INVALID");
        assert_eq!(
            safe_component(&".".repeat(100)).unwrap_err().code,
            "REPORT_DATA_INVALID"
        );
    }
}
