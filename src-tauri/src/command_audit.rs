//! One terminal record for a protected command, separate from later engine events.
use super::*;

pub(super) fn finish<T>(
    state: &AppState,
    category: Category,
    action: &str,
    operator: String,
    mut detail: serde_json::Value,
    outcome: CommandResult<T>,
) -> CommandResult<T> {
    let result = match &outcome {
        Ok(_) => AuditResult::Success,
        Err(error) => {
            detail["code"] = serde_json::json!(error.code);
            if matches!(
                error.code.as_str(),
                "STORAGE_ERROR"
                    | "DEVICE_OFFLINE"
                    | "DEVICE_COMMAND_FAILED"
                    | "RECOVERY_FAILED"
                    | "EXPORT_FAILED"
            ) {
                AuditResult::Failure
            } else {
                AuditResult::Rejected
            }
        }
    };
    state.audit.write(
        &AuditEntry::new(category, action, result)
            .operator(operator)
            .detail(detail),
    );
    outcome
}
