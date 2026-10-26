use crate::*;
use tmh_core::config_store::{ConfigSnapshot, ConfigStore, ConfigStoreError};

pub(crate) fn open_store(
    data_root: &std::path::Path,
    explicit: Option<PathBuf>,
) -> Result<ConfigStore, String> {
    let path = explicit
        .clone()
        .unwrap_or_else(|| data_root.join("config/comm_config.json"));
    let result = if explicit.is_some() {
        ConfigStore::open(path.clone(), None)
    } else {
        let defaults =
            CommConfig::from_json(DEFAULT_COMM_CONFIG).map_err(|error| error.to_string())?;
        ConfigStore::open(path.clone(), Some(&defaults))
    };
    result.map_err(|error| format!("{}\n{}", path.display(), config_error(error)))
}

fn config_error(error: ConfigStoreError) -> String {
    match error {
        ConfigStoreError::StaleRevision => "配置版本已变化，请重新读取后再保存".into(),
        ConfigStoreError::ExternalChange => {
            "配置文件已被外部修改；请关闭应用，核对文件后重新打开".into()
        }
        ConfigStoreError::DurabilityUnconfirmed(detail) => {
            format!("配置文件已替换，但磁盘同步未确认；请核对已保存内容：{detail}")
        }
        other => format!("通讯配置不可用：{other}"),
    }
}

pub(crate) fn snapshot(state: &AppState) -> Result<ConfigSnapshot, String> {
    current_user(state)?;
    Ok(lock(&state.config).snapshot())
}

fn config_command_error(error: ConfigStoreError) -> CommandError {
    let code = match &error {
        ConfigStoreError::StaleRevision
        | ConfigStoreError::ExternalChange
        | ConfigStoreError::MissingFile(_) => "CONFIG_CONFLICT",
        ConfigStoreError::Config(_) | ConfigStoreError::TooLarge => "INVALID_PARAMS",
        ConfigStoreError::InvalidPath
        | ConfigStoreError::Locked(_)
        | ConfigStoreError::DurabilityUnconfirmed(_)
        | ConfigStoreError::Io(_) => "CONFIG_SAVE_FAILED",
    };
    CommandError::new(code, config_error(error))
}

pub(crate) fn save(
    state: &AppState,
    expected: &str,
    config: CommConfig,
    reason: &str,
) -> CommandResult<ConfigSnapshot> {
    let _lifecycle = lock(&state.exp.lifecycle);
    let (operator, authorization) = command_audit::authorize(state, UserRole::Admin);
    let mut detail = serde_json::json!({
        "reason": reason.chars().take(500).collect::<String>(),
        "expected_revision": expected,
    });
    let outcome = (|| {
        authorization?;
        if state.exp.closing.load(Ordering::SeqCst) {
            return Err(CommandError::from("APP_CLOSING"));
        }
        if state.exp.updating.load(Ordering::SeqCst) {
            return Err(CommandError::from("UPDATE_IN_PROGRESS"));
        }
        if state.exp.is_running() {
            return Err(CommandError::from("EXPERIMENT_RUNNING"));
        }
        if state.exp.persistence_pending() {
            return Err(CommandError::from("PERSISTENCE_PENDING"));
        }
        if reason.trim().is_empty() || reason.chars().count() > 500 {
            return Err(CommandError::new(
                "INVALID_PARAMS",
                "请填写1至500字的配置修改原因",
            ));
        }
        lock(&state.config)
            .save(expected, config)
            .map_err(config_command_error)
    })();
    let saved = lock(&state.config).snapshot();
    detail["revision"] = serde_json::json!(saved.revision);
    detail["path"] = serde_json::json!(saved.path);
    detail["saved"] = serde_json::json!(saved.saved);
    detail["restart_required"] = serde_json::json!(saved.restart_required);
    command_audit::finish(
        state,
        Category::Config,
        "save_comm_config",
        operator,
        detail,
        outcome,
    )
}

pub(crate) fn start_blocker(state: &AppState) -> Option<String> {
    let store = lock(&state.config);
    if store.snapshot().restart_required {
        return Some("通讯配置已保存但尚未生效，请管理员在通讯设置中重启应用".into());
    }
    store.verify_saved_file().err().map(config_error)
}

pub(crate) async fn prepare_restart(
    state: &AppState,
    expected: &str,
) -> CommandResult<UpdateLease> {
    let verify = || {
        let store = lock(&state.config);
        if store.snapshot().revision != expected {
            return Err(CommandError::new(
                "CONFIG_CONFLICT",
                "配置版本已变化，请重新读取",
            ));
        }
        store.verify_saved_file().map_err(config_command_error)
    };
    let (operator, lease_result) = {
        let _lifecycle = lock(&state.exp.lifecycle);
        let (operator, authorization) = command_audit::authorize(state, UserRole::Admin);
        let lease = authorization
            .and_then(|_| UpdateLease::begin_locked(&state.exp).map_err(CommandError::from));
        (operator, lease)
    };
    let outcome: CommandResult<UpdateLease> = async {
        let lease = lease_result?;
        verify()?;
        state
            .hub
            .request_shutoff_flammable()
            .await
            .map_err(|_| CommandError::new("DEVICE_COMMAND_FAILED", "保护气氛未获确认，不能重启"))?
            .map_err(|_| {
                CommandError::new("DEVICE_COMMAND_FAILED", "保护气氛未获确认，不能重启")
            })?;
        verify()?;
        if state.exp.is_running() {
            return Err(CommandError::from("EXPERIMENT_RUNNING"));
        }
        if state.exp.persistence_pending() {
            return Err(CommandError::from("PERSISTENCE_PENDING"));
        }
        Ok(lease)
    }
    .await;
    command_audit::finish(
        state,
        Category::Config,
        "restart_for_config",
        operator,
        serde_json::json!({"revision": expected}),
        outcome,
    )
}

#[cfg(test)]
#[path = "config_commands_tests.rs"]
mod tests;
