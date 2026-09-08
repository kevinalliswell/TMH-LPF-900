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
    if state.exp.is_running() {
        return Err("实验运行中不可修改模式".into());
    }
    if state.exp.updating.load(Ordering::SeqCst) {
        return Err("正在安装更新，不能修改模式".into());
    }
    Ok(())
}

pub(super) fn save(
    state: &AppState,
    mut program: Program,
    reason: &str,
) -> Result<Program, String> {
    let _lifecycle = lock(&state.exp.lifecycle);
    let user = current_admin(state)?;
    ensure_editable(state)?;
    if STANDARD_MODES.contains(&program.id.trim()) {
        return Err("标准模式不可覆盖，请复制为自定义模式".into());
    }
    if program.id.trim().is_empty() {
        program.id = custom_program_id();
    }
    normalize_custom_program(&mut program);
    let mut storage = lock(&state.storage);
    let previous_json = storage
        .get_custom_program(&program.id)
        .map_err(|e| e.to_string())?;
    let previous: Option<Program> = previous_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|e| e.to_string())?;
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
    validate_gas(state, &program)?;
    storage
        .save_custom_program_revision(&program, previous_json.as_deref())
        .map_err(|e| e.to_string())?;
    state.audit.write(
        &AuditEntry::new(
            Category::Config,
            "save_custom_program",
            AuditResult::Success,
        )
        .operator(user.display_name)
        .detail(serde_json::json!({"id":program.id,"provenance":program.provenance})),
    );
    Ok(program)
}

pub(super) fn history(state: &AppState, id: &str) -> Result<Vec<Program>, String> {
    current_user(state)?;
    lock(&state.storage)
        .custom_program_revisions(id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|json| serde_json::from_str(&json).map_err(|e| e.to_string()))
        .collect()
}

pub(super) fn delete(state: &AppState, id: &str) -> Result<(), String> {
    let _lifecycle = lock(&state.exp.lifecycle);
    let user = current_admin(state)?;
    ensure_editable(state)?;
    if STANDARD_MODES.contains(&id) {
        return Err("标准模式不可删除".into());
    }
    if !lock(&state.storage)
        .delete_custom_program(id)
        .map_err(|e| e.to_string())?
    {
        return Err("自定义模式不存在".into());
    }
    state.audit.write(
        &AuditEntry::new(
            Category::Config,
            "delete_custom_program",
            AuditResult::Success,
        )
        .operator(user.display_name)
        .detail(serde_json::json!({"id":id,"history_preserved":true})),
    );
    Ok(())
}
