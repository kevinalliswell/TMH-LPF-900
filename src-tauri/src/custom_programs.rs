//! Shared custom templates: authorized revisions, immutable history, execution gates.
use super::*;
use tmh_core::provenance::{
    prepare_custom_revision, validate_flow_resolution, validate_gas_limits, ProgramEditor,
};

pub(super) fn flow_steps(config: &CommConfig) -> [f64; 4] {
    let s = &config.flow_scaling;
    [s.h2 / 10.0, s.n2 / 10.0, s.co2 / 10.0, s.co / 10.0]
}

pub(super) fn validate_gas(state: &AppState, program: &Program) -> Result<(), String> {
    validate_gas_limits(program, state.limit_h2, state.limit_co)?;
    validate_flow_resolution(program, state.flow_steps)
}

fn ensure_editable(state: &AppState) -> Result<(), String> {
    if state.exp.closing.load(Ordering::SeqCst) {
        return Err("APP_CLOSING".into());
    }
    if state.exp.is_running() {
        return Err("EXPERIMENT_RUNNING".into());
    }
    if state.exp.updating.load(Ordering::SeqCst) {
        return Err("UPDATE_IN_PROGRESS".into());
    }
    Ok(())
}

pub(super) fn save(state: &AppState, mut program: Program, reason: &str) -> CommandResult<Program> {
    let _lifecycle = lock(&state.exp.lifecycle);
    let (operator, authorization) = command_audit::authorize(state, UserRole::Admin);
    let mut detail = serde_json::json!({"id":program.id});
    let outcome = (|| {
        let user = authorization?;
        ensure_editable(state)?;
        if STANDARD_MODES.contains(&program.id.trim()) {
            return Err("标准模式不可覆盖，请复制为自定义模式".into());
        }
        if program.id.trim().is_empty() {
            program.id = custom_program_id();
        }
        normalize_custom_program(&mut program);
        detail["id"] = serde_json::json!(program.id);
        let mut storage = lock(&state.storage);
        let previous_json = storage
            .get_custom_program(&program.id)
            .map_err(|_| CommandError::from("STORAGE_ERROR"))?;
        let previous: Option<Program> = previous_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|_| CommandError::new("PROGRAM_DATA_INVALID", "已保存模式版本损坏"))?;
        let program = prepare_custom_revision(
            program,
            previous.as_ref(),
            ProgramEditor {
                user_id: user.user_id.clone(),
                display_name: user.display_name.clone(),
                edited_at: iso_now(),
            },
            reason,
        )?;
        validate_executable_program(&program)?;
        validate_gas(state, &program)?;
        storage
            .save_custom_program_revision(&program, previous_json.as_deref())
            .map_err(|_| CommandError::from("STORAGE_ERROR"))?;
        detail["provenance"] = serde_json::json!(program.provenance);
        Ok(program)
    })();
    command_audit::finish(
        state,
        Category::Config,
        "save_custom_program",
        operator,
        detail,
        outcome,
    )
}

pub(super) fn history(state: &AppState, id: &str) -> CommandResult<Vec<Program>> {
    let (operator, authorization) = command_audit::authorize(state, UserRole::User);
    let outcome = (|| {
        authorization?;
        let revisions = lock(&state.storage)
            .custom_program_revisions(id)
            .map_err(|_| CommandError::from("STORAGE_ERROR"))?;
        revisions
            .into_iter()
            .map(|json| {
                serde_json::from_str(&json)
                    .map_err(|_| CommandError::new("PROGRAM_DATA_INVALID", "已保存模式版本损坏"))
            })
            .collect()
    })();
    command_audit::finish(
        state,
        Category::Config,
        "list_custom_program_revisions",
        operator,
        serde_json::json!({"id":id}),
        outcome,
    )
}

pub(super) fn delete(state: &AppState, id: &str) -> CommandResult<()> {
    let _lifecycle = lock(&state.exp.lifecycle);
    let (operator, authorization) = command_audit::authorize(state, UserRole::Admin);
    let detail = serde_json::json!({"id":id,"history_preserved":true});
    let outcome = (|| {
        authorization?;
        ensure_editable(state)?;
        if STANDARD_MODES.contains(&id) {
            return Err("标准模式不可删除".into());
        }
        if !lock(&state.storage)
            .delete_custom_program(id)
            .map_err(|_| CommandError::from("STORAGE_ERROR"))?
        {
            return Err("自定义模式不存在".into());
        }
        Ok(())
    })();
    command_audit::finish(
        state,
        Category::Config,
        "delete_custom_program",
        operator,
        detail,
        outcome,
    )
}
