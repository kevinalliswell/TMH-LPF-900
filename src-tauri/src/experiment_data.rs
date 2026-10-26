//! Account-bound experiment mutations; preserve the existing calculation/revision data.
use super::*;

fn ensure_editable(state: &AppState) -> CommandResult<()> {
    if state.exp.closing.load(Ordering::SeqCst) {
        return Err(CommandError::from("APP_CLOSING"));
    }
    if state.exp.updating.load(Ordering::SeqCst) {
        return Err(CommandError::from("UPDATE_IN_PROGRESS"));
    }
    Ok(())
}

pub(super) fn set_experiment_owner(
    state: &AppState,
    experiment_id: String,
    owner_user_id: Option<String>,
) -> CommandResult<()> {
    let _lifecycle = lock(&state.exp.lifecycle);
    let (operator, authorization) = command_audit::authorize(state, UserRole::Admin);
    let mut detail = serde_json::json!({"id":experiment_id,"new_owner_user_id":owner_user_id});
    let outcome = (|| {
        authorization?;
        ensure_editable(state)?;
        if state.exp.snapshot().experiment_id.as_deref() == Some(experiment_id.as_str()) {
            return Err("无法变更正在运行实验的归属".into());
        }
        let storage = lock(&state.storage);
        let experiment = storage
            .get_experiment(&experiment_id)
            .map_err(|_| CommandError::from("STORAGE_ERROR"))?
            .ok_or_else(|| "实验不存在".to_string())?;
        detail["old_owner_user_id"] = serde_json::json!(experiment.owner_user_id);
        if let Some(owner_id) = owner_user_id.as_deref() {
            let owner = storage
                .get_user(owner_id)
                .map_err(|_| CommandError::from("STORAGE_ERROR"))?
                .ok_or_else(|| "USER_NOT_FOUND".to_string())?;
            if !owner.is_active {
                return Err(AuthCode::AccountDisabled.to_string().into());
            }
        }
        storage
            .set_experiment_owner(&experiment_id, owner_user_id.as_deref())
            .map_err(|_| CommandError::from("STORAGE_ERROR"))?;
        Ok(())
    })();
    command_audit::finish(
        state,
        Category::Experiment,
        "set_experiment_owner",
        operator,
        detail,
        outcome,
    )
}

pub(super) fn delete_experiment(state: &AppState, id: String) -> CommandResult<()> {
    let _lifecycle = lock(&state.exp.lifecycle);
    let (operator, authorization) = command_audit::authorize(state, UserRole::Admin);
    let detail = serde_json::json!({"id":id});
    let outcome = (|| {
        authorization?;
        ensure_editable(state)?;
        if state.exp.persistence_pending() {
            return Err(CommandError::new(
                "INVALID_STATE",
                "存在尚未保存的数据，请先重试保存",
            ));
        }
        // Never delete the run currently in progress.
        if state.exp.snapshot().experiment_id.as_deref() == Some(id.as_str()) {
            return Err(CommandError::new("INVALID_STATE", "无法删除正在运行的实验"));
        }
        let mut storage = lock(&state.storage);
        if storage
            .get_experiment(&id)
            .map_err(|_| CommandError::from("STORAGE_ERROR"))?
            .is_some_and(|r| r.end_time.is_none())
        {
            return Err(CommandError::new(
                "INVALID_STATE",
                "实验尚未完成保存，不能删除",
            ));
        }
        storage
            .delete_experiment(&id)
            .map_err(|_| CommandError::from("STORAGE_ERROR"))?;
        Ok(())
    })();
    command_audit::finish(
        state,
        Category::Experiment,
        "delete",
        operator,
        detail,
        outcome,
    )
}

pub(super) fn analyze_gb13241(
    state: &AppState,
    id: String,
    input: ReductionInput,
) -> CommandResult<serde_json::Value> {
    let _lifecycle = lock(&state.exp.lifecycle);
    let (operator, authorization) = command_audit::authorize(state, UserRole::User);
    let detail = serde_json::json!({"id":id,"standard":"GB/T 13241"});
    let outcome = (|| {
        let user = authorization?;
        ensure_editable(state)?;
        let (user, row, samples, meta) = analysis_context(state, &id, "REDUCIBILITY", user)?;
        let value = calculate_reduction(&row, &samples, &meta, &input)?;
        persist_analysis(state, &id, &user.display_name, &value)?;
        Ok(value)
    })();
    command_audit::finish(
        state,
        Category::Experiment,
        "analyze",
        operator,
        detail,
        outcome,
    )
}

pub(super) fn analyze_gb13242(
    state: &AppState,
    id: String,
    input: RdiInput,
) -> CommandResult<serde_json::Value> {
    let _lifecycle = lock(&state.exp.lifecycle);
    let (operator, authorization) = command_audit::authorize(state, UserRole::User);
    let detail = serde_json::json!({"id":id,"standard":"GB/T 13242"});
    let outcome = (|| {
        let user = authorization?;
        ensure_editable(state)?;
        let (user, _, _, meta) = analysis_context(state, &id, "LOW_TEMP_DEGRADATION", user)?;
        let result = rdi(input.m0, input.m1, input.m2, input.m3).ok_or_else(|| {
            CommandError::new(
                "INVALID_PARAMS",
                "入鼓质量或筛上质量无效：须为有限非负值且筛上合计不大于入鼓质量",
            )
        })?;
        if !input.drum_rpm.is_finite()
            || input.drum_rpm <= 0.0
            || input.drum_revolutions == 0
            || !input.sieve_seconds.is_finite()
            || input.sieve_seconds <= 0.0
        {
            return Err(CommandError::new(
                "INVALID_PARAMS",
                "请填写实际转鼓与筛分记录",
            ));
        }
        let external_conditions_met = (input.drum_rpm - 30.0).abs() <= 1.0
            && input.drum_revolutions == 300
            && input.sieve_seconds == 60.0;
        let value = serde_json::json!({"standard":"GB/T 13242","standard_version":"2017","calculation_version":CALCULATION_VERSION,
      "input":input,"rdi_plus_6_3":result.rdi_plus_6_3,"rdi_plus_3_15":result.rdi_plus_3_15,"rdi_minus_0_5":result.rdi_minus_0_5,
      "external_conditions_met":external_conditions_met,"runtime_evidence":meta,"result_scope":"single_trial","standard_conformity":"pending_group_and_process_verification","computed_at":iso_now()});
        persist_analysis(state, &id, &user.display_name, &value)?;
        Ok(value)
    })();
    command_audit::finish(
        state,
        Category::Experiment,
        "analyze",
        operator,
        detail,
        outcome,
    )
}

pub(super) fn analyze_gb13240(
    state: &AppState,
    id: String,
    input: SwellingInput,
) -> CommandResult<serde_json::Value> {
    let _lifecycle = lock(&state.exp.lifecycle);
    let (operator, authorization) = command_audit::authorize(state, UserRole::User);
    let detail = serde_json::json!({"id":id,"standard":"GB/T 13240"});
    let outcome = (|| {
        let user = authorization?;
        ensure_editable(state)?;
        let (user, _, _, meta) = analysis_context(state, &id, "FREE_SWELLING", user)?;
        if input.pellet_count != 18 || input.volume_method.trim().is_empty() || !input.same_method {
            return Err(CommandError::new(
                "INVALID_PARAMS",
                "须为同一份 18 颗球团的总体积，并确认还原前后采用相同体积测定方法",
            ));
        }
        let result = free_swelling(&[(input.v0, input.v1)])
            .ok_or_else(|| CommandError::new("INVALID_PARAMS", "整份体积必须为有限正值"))?;
        let value = serde_json::json!({"standard":"GB/T 13240","standard_version":"2018","calculation_version":CALCULATION_VERSION,
      "input":input,"swelling_index":result.swelling_index,"runtime_evidence":meta,
      "result_scope":"single_trial","standard_conformity":"pending_group_and_process_verification","computed_at":iso_now()});
        persist_analysis(state, &id, &user.display_name, &value)?;
        Ok(value)
    })();
    command_audit::finish(
        state,
        Category::Experiment,
        "analyze",
        operator,
        detail,
        outcome,
    )
}

pub(super) fn save_report_details(
    state: &AppState,
    id: String,
    details: ReportDetails,
) -> CommandResult<()> {
    let _lifecycle = lock(&state.exp.lifecycle);
    let (operator, authorization) = command_audit::authorize(state, UserRole::User);
    let detail = serde_json::json!({"id":id});
    let outcome = (|| {
        let user = authorization?;
        ensure_editable(state)?;
        authorize_experiment(state, &id, &user)?;
        let value =
            serde_json::to_value(&details).map_err(|_| CommandError::from("STORAGE_ERROR"))?;
        if value
            .as_object()
            .unwrap()
            .values()
            .any(|v| v.as_str().is_none_or(|s| s.len() > 4096))
        {
            return Err(CommandError::new("INVALID_PARAMS", "报告字段过长"));
        }
        let patch = serde_json::json!({"report_details":details,"report_details_operator":user.display_name,"report_details_updated_at":iso_now()});
        lock(&state.storage)
            .save_report_revision(&id, &patch.to_string())
            .map_err(|_| CommandError::from("STORAGE_ERROR"))?;
        Ok(())
    })();
    command_audit::finish(
        state,
        Category::Experiment,
        "report_details",
        operator,
        detail,
        outcome,
    )
}

fn analysis_context(
    state: &AppState,
    id: &str,
    expected: &str,
    user: UserRow,
) -> CommandResult<(UserRow, ExperimentRow, Vec<SampleRow>, serde_json::Value)> {
    let row = authorize_experiment(state, id, &user)?;
    let db = lock(&state.storage);
    if row.end_time.is_none() {
        return Err(CommandError::new("INVALID_STATE", "实验未结束，暂不可分析"));
    }
    let program: Option<Program> = row
        .program_snapshot_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|_| {
            CommandError::new(
                "ANALYSIS_DATA_INVALID",
                "执行程序快照损坏，不能确认分析方法",
            )
        })?;
    let method = program
        .as_ref()
        .and_then(|p| p.base_standard_id.as_deref())
        .or(row.experiment_type.as_deref());
    if method != Some(expected) {
        return Err(CommandError::new(
            "ANALYSIS_TYPE_MISMATCH",
            "分析方法与本次执行程序不匹配",
        ));
    }
    let meta = serde_json::from_str(
        &db.runtime_metadata(id)
            .map_err(|_| CommandError::from("STORAGE_ERROR"))?,
    )
    .map_err(|_| CommandError::new("ANALYSIS_DATA_INVALID", "实验运行证据损坏"))?;
    Ok((
        user,
        row,
        db.get_samples(id, 0)
            .map_err(|_| CommandError::from("STORAGE_ERROR"))?,
        meta,
    ))
}

pub(super) fn persist_analysis(
    state: &AppState,
    id: &str,
    operator: &str,
    value: &serde_json::Value,
) -> CommandResult<()> {
    let mut versioned = value.clone();
    versioned["computed_by"] = serde_json::json!(operator);
    let json =
        serde_json::to_string(&versioned).map_err(|_| CommandError::from("STORAGE_ERROR"))?;
    lock(&state.storage)
        .set_analysis_results(id, &json)
        .map_err(|_| CommandError::from("STORAGE_ERROR"))?;
    Ok(())
}
