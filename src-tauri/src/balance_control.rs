//! An accepted tare remains in flight even if its IPC caller disappears.
use super::*;

pub(super) struct TareLease(Arc<ExpControl>);

impl TareLease {
    /// Called within the device lifecycle boundary, after controller authorization.
    pub(super) fn begin(exp: Arc<ExpControl>) -> Result<Self, String> {
        if exp.is_running() || exp.persistence_pending() {
            return Err("EXPERIMENT_RUNNING".into());
        }
        if exp.tare_pending.swap(true, Ordering::SeqCst) {
            return Err("BALANCE_BUSY".into());
        }
        Ok(Self(exp))
    }
}

impl Drop for TareLease {
    fn drop(&mut self) {
        self.0.tare_pending.store(false, Ordering::SeqCst);
    }
}

pub(super) async fn complete<R: tauri::Runtime>(
    app: AppHandle<R>,
    operator: String,
    lease: TareLease,
    outcome: impl std::future::Future<Output = CommandResult<TareResultDto>>,
) -> CommandResult<TareResultDto> {
    // The detached task owns the lease through the actual bus outcome and audit.
    let _lease = lease;
    let result = outcome.await;
    app.state::<AppState>().audit.write(
        &AuditEntry::new(
            Category::Balance,
            "tare",
            if result.as_ref().is_ok_and(|r| r.success) {
                AuditResult::Success
            } else {
                AuditResult::Failure
            },
        )
        .operator(operator)
        .detail(serde_json::json!({
            "success": result.as_ref().ok().map(|r| r.success),
            "a00_found": result.as_ref().ok().map(|r| r.a00_found),
            "weight": result.as_ref().ok().map(|r| r.weight),
            "code": result.as_ref().err().map(|e| e.code.as_str()),
        })),
    );
    result
}
