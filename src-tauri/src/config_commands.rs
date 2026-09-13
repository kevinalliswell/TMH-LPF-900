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

fn authorize_admin(state: &AppState) -> (String, Result<UserRow, String>) {
    let storage = lock(&state.storage);
    let outcome = state
        .auth
        .require_admin(&storage, epoch_now())
        .map_err(auth_error);
    let operator = outcome
        .as_ref()
        .ok()
        .map(|actor| actor.display_name.clone())
        .or_else(|| {
            state
                .auth
                .snapshot(&storage, epoch_now())
                .ok()
                .and_then(|snapshot| snapshot.user)
                .map(|user| user.display_name)
        })
        .unwrap_or_else(|| "anonymous".into());
    (operator, outcome)
}

pub(crate) fn save(
    state: &AppState,
    expected: &str,
    config: CommConfig,
    reason: &str,
) -> Result<ConfigSnapshot, String> {
    let (operator, authorization) = authorize_admin(state);
    let mut result = AuditResult::Rejected;
    let _lifecycle = lock(&state.exp.lifecycle);
    let outcome = (|| {
        authorization?;
        if state.exp.is_running()
            || state.exp.persistence_pending()
            || state.exp.updating.load(Ordering::SeqCst)
        {
            return Err("实验、数据保存或重启更新期间不能修改通讯配置".into());
        }
        if reason.trim().is_empty() || reason.chars().count() > 500 {
            return Err("请填写1至500字的配置修改原因".into());
        }
        match lock(&state.config).save(expected, config) {
            Ok(saved) => {
                result = AuditResult::Success;
                Ok(saved)
            }
            Err(error) => {
                result = match &error {
                    ConfigStoreError::Io(_) | ConfigStoreError::DurabilityUnconfirmed(_) => {
                        AuditResult::Failure
                    }
                    _ => AuditResult::Rejected,
                };
                Err(config_error(error))
            }
        }
    })();
    let saved = lock(&state.config).snapshot();
    state.audit.write(&AuditEntry::new(Category::Config, "save_comm_config", result)
        .operator(operator).detail(serde_json::json!({"reason": reason.chars().take(500).collect::<String>(), "expected_revision": expected, "revision": saved.revision,
            "path": saved.path, "saved": saved.saved, "restart_required": saved.restart_required, "error": outcome.as_ref().err()})));
    outcome
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
) -> Result<UpdateLease, String> {
    let (operator, authorization) = authorize_admin(state);
    let mut result = AuditResult::Rejected;
    let verify = || {
        let store = lock(&state.config);
        if store.snapshot().revision != expected {
            return Err("配置版本已变化，请重新读取".to_string());
        }
        store.verify_saved_file().map_err(config_error)
    };
    let outcome: Result<UpdateLease, String> = async {
        authorization?;
        let lease = UpdateLease::begin(state.exp.clone())?;
        verify()?;
        result = AuditResult::Failure;
        state
            .hub
            .request_shutoff_flammable()
            .await
            .map_err(|_| "保护气氛未获确认，不能重启".to_string())?
            .map_err(|_| "保护气氛未获确认，不能重启".to_string())?;
        verify()?;
        if state.exp.is_running() || state.exp.persistence_pending() {
            return Err("实验或数据尚未保存，不能重启".into());
        }
        result = AuditResult::Success;
        Ok(lease)
    }
    .await;
    state.audit.write(
        &AuditEntry::new(Category::Config, "restart_for_config", result)
            .operator(operator)
            .detail(serde_json::json!({"revision": expected, "error": outcome.as_ref().err()})),
    );
    outcome
}

#[cfg(test)]
#[path = "config_commands_tests.rs"]
mod tests;
