//! Recovery is separated from the UI and hardware loop so restart/fault behavior
//! can be exercised against real SQLite and journal files.
use super::*;

pub(super) fn recover_unfinished(
    hub: &DeviceHub,
    storage: &mut Storage,
    audit: &AuditLog,
    exp: &ExpControl,
    dir: &std::path::Path,
) -> Result<(), String> {
    let result = recover(hub, storage, audit, exp, dir);
    if let Err(error) = &result {
        lock(&exp.recovery_errors).push(error.clone());
    }
    result
}

fn recover(
    hub: &DeviceHub,
    storage: &mut Storage,
    audit: &AuditLog,
    exp: &ExpControl,
    dir: &std::path::Path,
) -> Result<(), String> {
    let scan = recovery::scan(dir);
    let unfinished = storage
        .unfinished_experiments()
        .map_err(|e| e.to_string())?;
    if !scan.pending.is_empty()
        || !scan.errors.is_empty()
        || !unfinished.is_empty()
        || exp.persistence_pending()
    {
        let safety = apply_safety_atmosphere(hub);
        if safety.is_err() {
            exp.set_status(ExperimentStatus::safety_error("recovery", 0.0));
        }
        audit.write(
            &AuditEntry::new(
                Category::Experiment,
                "recovery_protection",
                if safety.is_ok() {
                    AuditResult::Success
                } else {
                    AuditResult::Failure
                },
            )
            .detail(serde_json::json!({"error":safety.as_ref().err()})),
        );
        retry_pending_completions(storage, exp, dir)?;
        // Read again after replay, propagating read failures instead of treating them as no rows.
        let unfinished = storage
            .unfinished_experiments()
            .map_err(|e| e.to_string())?;
        let mut errors = Vec::new();
        for run in unfinished {
            let completion = RunCompletion {
                token: tmh_core::auth::new_user_id(),
                experiment_id: run.experiment_id.clone(),
                end_time: iso_now(),
                rows: Vec::new(),
                metadata: serde_json::json!({"outcome":"recovered","data_complete":false,"safety_confirmed":safety.is_ok()}),
            };
            let journal = recovery::persist(dir, &completion);
            let committed = storage.commit_completion(&completion);
            audit.write(&AuditEntry::new(Category::Experiment, "crash_recovery",
                if committed.is_ok() {AuditResult::Success} else {AuditResult::Failure})
                .detail(serde_json::json!({"id":run.experiment_id,"database_error":committed.as_ref().err().map(|e|e.to_string()),"journal_error":journal.err()})));
            if let Err(e) = committed {
                errors.push(e.to_string());
                lock(&exp.pending).push(completion);
            } else if let Err(e) = recovery::clear(dir, &completion.token) {
                errors.push(e);
            }
        }
        if !errors.is_empty() {
            return Err(errors.join("；"));
        }
    }
    Ok(())
}

pub(super) fn retry_pending_completions(
    storage: &mut Storage,
    exp: &ExpControl,
    dir: &std::path::Path,
) -> Result<(), String> {
    let scan = recovery::scan(dir);
    let mut pending = lock(&exp.pending);
    let mut errors = scan.errors;
    for (_, item) in &scan.pending {
        if let Some(existing) = pending.iter().find(|p| p.token == item.token) {
            if serde_json::to_string(existing).ok() != serde_json::to_string(item).ok() {
                errors.push("待恢复记录 token 冲突".into());
            }
        } else {
            pending.push(item.clone());
        }
    }
    if !errors.is_empty() {
        *lock(&exp.recovery_errors) = errors.clone();
        return Err(errors.join("；"));
    }
    let mut failed = Vec::new();
    for item in pending.drain(..) {
        match storage.commit_completion(&item) {
            Ok(()) => {
                if let Err(e) = recovery::clear(dir, &item.token) {
                    errors.push(e);
                }
                for (path, source) in &scan.pending {
                    if source.token == item.token {
                        if let Err(e) = recovery::remove(path) {
                            errors.push(e);
                        }
                    }
                }
            }
            Err(e) => {
                errors.push(e.to_string());
                failed.push(item);
            }
        }
    }
    *pending = failed;
    *lock(&exp.recovery_errors) = errors.clone();
    if errors.is_empty() && pending.is_empty() {
        Ok(())
    } else {
        Err(format!("保存尚未恢复：{}", errors.join("；")))
    }
}
