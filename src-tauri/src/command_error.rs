//! Public command failures. Internal diagnostics are never used as UI control codes.
use serde::Serialize;

pub(crate) type CommandResult<T> = Result<T, CommandError>;

#[cfg_attr(feature = "typescript-export", derive(ts_rs::TS))]
#[derive(Clone, Debug, Serialize)]
pub(crate) struct CommandError {
    pub code: String,
    pub message_zh: String,
    pub detail: Option<String>,
}

impl CommandError {
    pub(crate) fn new(code: impl Into<String>, message_zh: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message_zh: message_zh.into(),
            detail: None,
        }
    }
}

impl From<String> for CommandError {
    fn from(message: String) -> Self {
        Self::from(message.as_str())
    }
}

impl From<&str> for CommandError {
    fn from(message: &str) -> Self {
        let translated = match message {
            "AUTH_REQUIRED" => "请先登录或解锁会话",
            "AUTH_FAILED" => "用户名或密码错误",
            "ACCOUNT_LOCKED" => "登录失败次数过多，账户已锁定15分钟",
            "ACCOUNT_DISABLED" => "账户已停用，请联系管理员",
            "SESSION_EXPIRED" => "会话已过期，请重新登录",
            "PASSWORD_CHANGE_REQUIRED" => "请先修改临时密码",
            "FORBIDDEN" => "当前账户无权执行此操作",
            "ALREADY_INITIALIZED" => "管理员已初始化，请登录",
            "USERNAME_EXISTS" => "用户名已存在",
            "USER_NOT_FOUND" => "用户不存在",
            "LAST_ADMIN" => "必须保留至少一个可用管理员",
            "STORAGE_ERROR" => "数据存储失败，请检查磁盘并重试",
            "INVALID_PARAMS" => "参数校验失败，请核对输入",
            "DEVICE_OFFLINE" => "设备离线，请检查连接",
            "EXPERIMENT_RUNNING" => "实验运行中，禁止此操作",
            "BALANCE_BUSY" => "天平去皮尚未完成，请等待回执",
            "APP_CLOSING" => "应用正在关闭，暂不能执行此操作",
            "UPDATE_IN_PROGRESS" => "正在安装更新，请等待完成",
            "PERSISTENCE_PENDING" => "存在待保存数据，请先恢复保存",
            "REPORT_DATA_INVALID" => "记录证据无法读取，不能生成报告",
            "EXPORT_FAILED" => "导出文件未能保存，请检查目录权限和磁盘空间",
            "SAFETY_LATCHED" => "保护仍锁存，请确认安全条件后重试",
            "DEVICE_BUSY" => "设备命令队列已满，请稍后重试",
            "DEVICE_COMMAND_FAILED" => "设备操作未完成，请检查设备状态",
            "UPDATE_DISABLED" => "更新功能未启用",
            "UPDATE_NOT_AVAILABLE" => "当前没有可安装的更新",
            "UPDATE_FAILED" => "更新未能完成，请检查网络、签名和磁盘状态",
            "UPDATE_CHECK_FAILED" => "无法连接更新服务器（离线环境属正常）",
            "CONFIG_CONFLICT" => "配置已变化，请重新读取后再操作",
            "CONFIG_SAVE_FAILED" => "通讯配置未能可靠保存，请检查文件访问和磁盘状态",
            "PORT_IN_USE" => "串口被占用，请关闭其他串口程序",
            // Preserve existing localized diagnostics during the boundary migration.
            // Callers must use COMMAND_FAILED, never parse these messages for logic.
            _ => return Self::new("COMMAND_FAILED", message),
        };
        Self::new(message, translated)
    }
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message_zh)
    }
}

impl std::error::Error for CommandError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_codes_have_localized_messages_without_reflecting_credentials() {
        for code in ["AUTH_REQUIRED", "AUTH_FAILED", "FORBIDDEN", "STORAGE_ERROR"] {
            let value = serde_json::to_value(CommandError::from(code)).unwrap();
            assert_eq!(value["code"], code);
            assert_ne!(value["message_zh"], code);
            assert!(value["detail"].is_null());
        }
    }
}
