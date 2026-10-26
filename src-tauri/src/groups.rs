//! Account-bound group commands. Read all evidence and append a revision under the
//! same database lock so reanalysis cannot race a group's frozen result selection.
use super::*;
use tmh_core::storage::TestGroupRevision;
#[cfg(test)]
pub(super) use tmh_core::test_group::TrialSelection;
use tmh_core::test_group::{build_snapshot, validate_request, FrozenTrial};
pub(super) use tmh_core::test_group::{GroupRequest, GroupSnapshot};

fn authorize(record: &TestGroupRevision, user: &UserRow) -> Result<(), String> {
    if may_access_experiment(user.role, &user.user_id, Some(&record.owner_user_id)) {
        Ok(())
    } else {
        Err(AuthCode::Forbidden.to_string())
    }
}
fn editable(state: &AppState) -> Result<(), String> {
    if state.exp.closing.load(Ordering::SeqCst) {
        return Err("APP_CLOSING".into());
    }
    if state.exp.updating.load(Ordering::SeqCst) {
        return Err("UPDATE_IN_PROGRESS".into());
    }
    if state.exp.persistence_pending() {
        return Err("PERSISTENCE_PENDING".into());
    }
    Ok(())
}
fn prepare(
    db: &Storage,
    user: &UserRow,
    request: &GroupRequest,
) -> Result<(String, Option<TestGroupRevision>, GroupSnapshot), String> {
    validate_request(request)?;
    let id = request
        .group_id
        .clone()
        .unwrap_or_else(|| format!("group-{}", tmh_core::auth::new_user_id()));
    let previous = db
        .test_group_revision(&id, None)
        .map_err(|_| "STORAGE_ERROR".to_string())?;
    if let Some(previous) = &previous {
        authorize(previous, user)?;
    }
    if previous.as_ref().map(|r| r.revision) != request.expected_revision
        || (request.group_id.is_some() && previous.is_none())
    {
        return Err("检验组版本已变化或不存在，请重新读取后保存".into());
    }
    // A revision remains visible to its owner. Administrator edit permission must
    // not silently share another user's source records with that owner.
    let group_owner = if let Some(previous) = &previous {
        db.get_user(&previous.owner_user_id)
            .map_err(|_| "STORAGE_ERROR".to_string())?
            .ok_or("检验组归属用户不存在")?
    } else {
        user.clone()
    };
    let previous_snapshot: Option<GroupSnapshot> = previous
        .as_ref()
        .map(|r| serde_json::from_value(r.snapshot.clone()))
        .transpose()
        .map_err(|_| "已保存的检验组版本无法读取")?;
    let mut trials = Vec::new();
    for selection in &request.trials {
        if let Some(frozen) = previous_snapshot.as_ref().and_then(|s| {
            s.trials
                .iter()
                .find(|t| t.experiment.experiment_id == selection.experiment_id)
        }) {
            trials.push(frozen.clone());
            continue;
        }
        let row = db
            .get_experiment(&selection.experiment_id)
            .map_err(|_| "STORAGE_ERROR".to_string())?
            .ok_or("试验不存在")?;
        if !may_access_experiment(user.role, &user.user_id, row.owner_user_id.as_deref())
            || !may_access_experiment(
                group_owner.role,
                &group_owner.user_id,
                row.owner_user_id.as_deref(),
            )
        {
            return Err(AuthCode::Forbidden.to_string());
        }
        let runtime = serde_json::from_str(
            &db.runtime_metadata(&row.experiment_id)
                .map_err(|_| "STORAGE_ERROR".to_string())?,
        )
        .map_err(|_| "试验运行记录无法读取")?;
        let analysis_revision_id = db
            .current_analysis_revision_id(&row.experiment_id)
            .map_err(|_| "STORAGE_ERROR".to_string())?;
        let process_assessment = tmh_core::process_evidence::from_record(
            &row,
            &db.get_samples(&row.experiment_id, 0)
                .map_err(|_| "STORAGE_ERROR".to_string())?,
            &runtime,
        );
        trials.push(FrozenTrial {
            process_assessment,
            experiment: row,
            runtime,
            analysis_revision_id,
            exclusion_reason: selection.exclusion_reason.clone(),
        });
    }
    let snapshot = build_snapshot(
        request,
        previous_snapshot.as_ref(),
        trials,
        &user.display_name,
    )?;
    Ok((id, previous, snapshot))
}

pub(super) fn preview(state: &AppState, request: GroupRequest) -> Result<GroupSnapshot, String> {
    let _lifecycle = lock(&state.exp.lifecycle);
    let user = current_user(state)?;
    editable(state)?;
    let (_, _, snapshot) = prepare(&lock(&state.storage), &user, &request)?;
    Ok(snapshot)
}
pub(super) fn save(state: &AppState, request: GroupRequest) -> CommandResult<TestGroupRevision> {
    let _lifecycle = lock(&state.exp.lifecycle);
    let (operator, authorization) = command_audit::authorize(state, UserRole::User);
    let mut detail = serde_json::json!({"group_id":request.group_id,"reason":request.reason});
    let outcome = (|| {
        let user = authorization?;
        editable(state)?;
        let mut db = lock(&state.storage);
        let (id, previous, snapshot) = prepare(&db, &user, &request)?;
        let record = TestGroupRevision {
            group_id: id,
            revision: previous
                .as_ref()
                .map_or(Some(1), |r| r.revision.checked_add(1))
                .ok_or("检验组版本超出范围")?,
            owner_user_id: previous
                .as_ref()
                .map(|r| r.owner_user_id.clone())
                .unwrap_or_else(|| user.user_id.clone()),
            recorded_by_user_id: user.user_id.clone(),
            created_at: iso_now(),
            snapshot: serde_json::to_value(&snapshot)
                .map_err(|_| CommandError::from("STORAGE_ERROR"))?,
        };
        db.save_test_group_revision(&record, request.expected_revision)
            .map_err(|_| CommandError::from("STORAGE_ERROR"))?;
        let selected_ids: Vec<_> = snapshot
            .evaluation
            .decision
            .selected_indices
            .iter()
            .map(|i| {
                &snapshot.trials[snapshot.valid_trial_indices[*i]]
                    .experiment
                    .experiment_id
            })
            .collect();
        detail = serde_json::json!({"group_id":record.group_id,"revision":record.revision,"reason":snapshot.reason,"selected_experiment_ids":selected_ids});
        Ok(record)
    })();
    command_audit::finish(
        state,
        Category::Experiment,
        "save_test_group",
        operator,
        detail,
        outcome,
    )
}
pub(super) fn get(
    state: &AppState,
    id: &str,
    revision: Option<u64>,
) -> Result<TestGroupRevision, String> {
    let user = current_user(state)?;
    let record = lock(&state.storage)
        .test_group_revision(id, revision)
        .map_err(|e| e.to_string())?
        .ok_or("检验组不存在")?;
    authorize(&record, &user)?;
    Ok(record)
}
pub(super) fn list(state: &AppState, offset: u32) -> Result<Vec<TestGroupRevision>, String> {
    let user = current_user(state)?;
    lock(&state.storage)
        .list_test_groups(
            if user.role.is_admin() {
                None
            } else {
                Some(&user.user_id)
            },
            offset,
        )
        .map_err(|e| e.to_string())
}

/// Export exactly one immutable revision; authorization precedes all filesystem I/O.
#[cfg(test)]
pub(super) fn export_report(state: &AppState, id: &str, revision: u64) -> Result<String, String> {
    report_exports::generate_test_group_report(state, id, revision).map_err(|e| e.to_string())
}
