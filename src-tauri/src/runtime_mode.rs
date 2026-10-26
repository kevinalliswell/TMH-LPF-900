//! Supported requests are parsed before any durable state or device threads are opened.
use crate::command_error::{CommandError, CommandResult};

#[cfg_attr(feature = "typescript-export", derive(ts_rs::TS))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum DeviceMode {
    Mock,
    Real,
    Replay,
}

impl DeviceMode {
    pub(crate) fn parse(value: &str) -> CommandResult<Self> {
        match value {
            "mock" => Ok(Self::Mock),
            "real" => Ok(Self::Real),
            "replay" => Ok(Self::Replay),
            _ => Err(CommandError::new(
                "INVALID_RUNTIME_MODE",
                "TMH_DEVICE_MODE 必须为 mock、real 或 replay",
            )),
        }
    }

    pub(crate) fn require_available(self) -> CommandResult<Self> {
        match self {
            Self::Mock => Ok(self),
            Self::Real => Err(CommandError::new(
                "REAL_NOT_READY",
                "真实仪表型号与厂家协议尚未核实，real 模式不可用；未启动设备总线",
            )),
            Self::Replay => Err(CommandError::new(
                "REPLAY_NOT_READY",
                "桌面回放尚未接入明确会话，replay 模式不可用；未启动设备总线",
            )),
        }
    }
}

#[cfg_attr(feature = "typescript-export", derive(ts_rs::TS))]
#[derive(Clone, serde::Serialize)]
pub(crate) struct RuntimeModeStatus {
    pub requested_mode: DeviceMode,
    pub effective_mode: DeviceMode,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn modes_are_strict_and_unsupported_transports_never_fall_back() {
        assert_eq!(
            DeviceMode::parse("mock")
                .unwrap()
                .require_available()
                .unwrap(),
            DeviceMode::Mock
        );
        for (value, code) in [("real", "REAL_NOT_READY"), ("replay", "REPLAY_NOT_READY")] {
            assert_eq!(
                DeviceMode::parse(value)
                    .unwrap()
                    .require_available()
                    .unwrap_err()
                    .code,
                code
            );
        }
        for value in ["", "Mock", "real ", "unknown"] {
            assert_eq!(
                DeviceMode::parse(value).unwrap_err().code,
                "INVALID_RUNTIME_MODE"
            );
        }
    }
}
