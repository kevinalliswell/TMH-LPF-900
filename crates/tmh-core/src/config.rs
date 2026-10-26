//! Device communication config (`comm_config.json`, docs/06 §6.4 — legacy schema,
//! serde defaults as fallback). `validate()` is a hard gate: on failure the app
//! shows the config-error page and refuses to open buses; it NEVER silently
//! overwrites the user's file with defaults. Write-back is atomic (temp + rename).

use serde::{Deserialize, Serialize};
use std::path::Path;

#[cfg_attr(feature = "typescript-export", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommConfig {
    #[serde(rename = "COM_RS485_MFC")]
    pub mfc: SerialParams,
    #[serde(rename = "COM_RS485_TEMP")]
    pub temp: TempParams,
    #[serde(rename = "COM_RS232_Balance")]
    pub balance: SerialParams,
    #[serde(rename = "SAMPLING")]
    pub sampling: Sampling,
    #[serde(rename = "PERFORMANCE_CONFIG")]
    pub performance: Performance,
    #[serde(rename = "SLAVE_ADDRESS_MFC")]
    pub slave_address_mfc: SlaveAddrMfc,
    #[serde(rename = "FLOW_SCALING", default = "default_flow_scaling")]
    pub flow_scaling: FlowScaling,
    #[serde(rename = "GAS_SAFETY_LIMITS", default = "default_gas_limits")]
    pub gas_safety_limits: GasSafetyLimits,
}

#[cfg_attr(feature = "typescript-export", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SerialParams {
    pub port: String,
    pub baudrate: u32,
    pub bytesize: u8,
    pub parity: String,
    pub stopbits: u8,
    pub timeout: f64,
}

#[cfg_attr(feature = "typescript-export", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TempParams {
    pub port: String,
    pub baudrate: u32,
    pub bytesize: u8,
    pub parity: String,
    pub stopbits: u8,
    pub timeout: f64,
    pub slave_address: u8,
    pub start_reg: u16,
    pub reg_count: u16,
    #[serde(rename = "TEMP_CHANNELS")]
    pub temp_channels: Vec<String>,
    pub scale: f64,
    #[serde(default)]
    pub signed_registers: bool,
}

#[cfg_attr(feature = "typescript-export", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sampling {
    pub interval_s: f64,
}

#[cfg_attr(feature = "typescript-export", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Performance {
    pub data_collection_interval: f64,
    /// Legacy JSON alias of SAMPLING.interval_s; validation requires equality.
    pub experiment_data_interval: f64,
    pub heartbeat_timeout: f64,
}

#[cfg_attr(feature = "typescript-export", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlaveAddrMfc {
    #[serde(rename = "H2")]
    pub h2: u8,
    #[serde(rename = "N2")]
    pub n2: u8,
    #[serde(rename = "CO2")]
    pub co2: u8,
    #[serde(rename = "CO")]
    pub co: u8,
}

#[cfg_attr(feature = "typescript-export", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlowScaling {
    #[serde(rename = "H2")]
    pub h2: f64,
    #[serde(rename = "N2")]
    pub n2: f64,
    #[serde(rename = "CO2")]
    pub co2: f64,
    #[serde(rename = "CO")]
    pub co: f64,
}

#[cfg_attr(feature = "typescript-export", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GasSafetyLimits {
    #[serde(rename = "H2")]
    pub h2: f64,
    #[serde(rename = "CO")]
    pub co: f64,
}

fn default_flow_scaling() -> FlowScaling {
    FlowScaling {
        h2: 0.1,
        n2: 0.1,
        co2: 0.1,
        co: 0.1,
    }
}

fn default_gas_limits() -> GasSafetyLimits {
    GasSafetyLimits { h2: 5.0, co: 5.0 }
}

/// Reasons a config is rejected. Kept specific so the UI can point at the field.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ConfigError {
    #[error("empty port: {0}")]
    EmptyPort(&'static str),
    #[error("non-positive safety limit: {0}")]
    NonPositiveLimit(&'static str),
    #[error("scale must be positive and finite: {0}")]
    ZeroScale(&'static str),
    #[error("MFC slave addresses must be distinct")]
    SlaveAddrConflict,
    #[error("interval must be finite and within 0.01..=86400 seconds: {0}")]
    NonPositiveInterval(&'static str),
    #[error("invalid configuration field: {0}")]
    InvalidField(&'static str),
    #[error("PERFORMANCE_CONFIG.experiment_data_interval must equal SAMPLING.interval_s")]
    SamplingAliasMismatch,
    #[error("parse error: {0}")]
    Parse(String),
    #[error("io error: {0}")]
    Io(String),
}

impl CommConfig {
    /// Parse + validate from a JSON file. Does not mutate the file.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|e| ConfigError::Io(e.to_string()))?;
        let cfg = Self::from_json(&text)?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Parse from a JSON string (defaults fill missing FLOW_SCALING / limits).
    pub fn from_json(text: &str) -> Result<Self, ConfigError> {
        serde_json::from_str(text).map_err(|e| ConfigError::Parse(e.to_string()))
    }

    /// Compatibility convenience writer. New read/edit/save consumers must retain
    /// a ConfigStore and pass its revision so a stale editing session cannot win.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        use crate::config_store::{ConfigStore, ConfigStoreError};
        let map_error = |error: ConfigStoreError| match error {
            ConfigStoreError::Config(error) => error,
            other => ConfigError::Io(other.to_string()),
        };
        self.validate()?;
        let mut store = ConfigStore::open(path.to_path_buf(), Some(self)).map_err(map_error)?;
        let snapshot = store.snapshot();
        if snapshot.saved != *self {
            store
                .save(&snapshot.revision, self.clone())
                .map_err(map_error)?;
        }
        Ok(())
    }

    /// Hard validation gate (docs/06 §6.4).
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.mfc.port.trim().is_empty() {
            return Err(ConfigError::EmptyPort("COM_RS485_MFC"));
        }
        if self.temp.port.trim().is_empty() {
            return Err(ConfigError::EmptyPort("COM_RS485_TEMP"));
        }
        if self.balance.port.trim().is_empty() {
            return Err(ConfigError::EmptyPort("COM_RS232_Balance"));
        }
        if !self.gas_safety_limits.h2.is_finite() || self.gas_safety_limits.h2 <= 0.0 {
            return Err(ConfigError::NonPositiveLimit("H2"));
        }
        if !self.gas_safety_limits.co.is_finite() || self.gas_safety_limits.co <= 0.0 {
            return Err(ConfigError::NonPositiveLimit("CO"));
        }
        if self.gas_safety_limits.h2 > crate::clamp::H2_ABS_MAX {
            return Err(ConfigError::InvalidField(
                "GAS_SAFETY_LIMITS.H2 exceeds absolute ceiling",
            ));
        }
        if self.gas_safety_limits.co > crate::clamp::CO_ABS_MAX {
            return Err(ConfigError::InvalidField(
                "GAS_SAFETY_LIMITS.CO exceeds absolute ceiling",
            ));
        }
        if !self.temp.scale.is_finite() || self.temp.scale <= 0.0 {
            return Err(ConfigError::ZeroScale("COM_RS485_TEMP.scale"));
        }
        for (name, v) in [
            ("H2", self.flow_scaling.h2),
            ("N2", self.flow_scaling.n2),
            ("CO2", self.flow_scaling.co2),
            ("CO", self.flow_scaling.co),
        ] {
            if !v.is_finite() || v <= 0.0 || !(v / 10.0).is_finite() || v / 10.0 <= 0.0 {
                return Err(ConfigError::ZeroScale(name));
            }
        }
        // The fixed emergency nitrogen atmosphere must survive configuration
        // quantization exactly, including the protocol's signed integer bound.
        let (n2, scale) = crate::rounding::decimal(self.flow_scaling.n2);
        let protective_raw =
            num_bigint::BigInt::from(50u8) * num_bigint::BigInt::from(10u8).pow(scale);
        if &protective_raw % &n2 != num_bigint::BigInt::from(0u8)
            || i64::try_from(protective_raw / n2).is_err()
        {
            return Err(ConfigError::InvalidField(
                "FLOW_SCALING.N2 must exactly encode protection 5 within i64",
            ));
        }
        // Cadences are consumed by the sampler, the telemetry pump and the staleness
        // check; a zero or negative value would spin those loops (#44).
        for (name, v) in [
            ("SAMPLING.interval_s", self.sampling.interval_s),
            (
                "PERFORMANCE_CONFIG.data_collection_interval",
                self.performance.data_collection_interval,
            ),
            (
                "PERFORMANCE_CONFIG.experiment_data_interval",
                self.performance.experiment_data_interval,
            ),
            (
                "PERFORMANCE_CONFIG.heartbeat_timeout",
                self.performance.heartbeat_timeout,
            ),
        ] {
            if !v.is_finite() || !(0.01..=86_400.0).contains(&v) {
                return Err(ConfigError::NonPositiveInterval(name));
            }
        }
        if self.performance.experiment_data_interval != self.sampling.interval_s {
            return Err(ConfigError::SamplingAliasMismatch);
        }
        validate_serial("COM_RS485_MFC", &self.mfc)?;
        validate_serial("COM_RS232_Balance", &self.balance)?;
        validate_serial(
            "COM_RS485_TEMP",
            &SerialParams {
                port: self.temp.port.clone(),
                baudrate: self.temp.baudrate,
                bytesize: self.temp.bytesize,
                parity: self.temp.parity.clone(),
                stopbits: self.temp.stopbits,
                timeout: self.temp.timeout,
            },
        )?;
        // The first release reads exactly registers 0..9 into the nine fixed channels.
        // Address zero remains accepted for legacy files, pending hardware validation.
        if self.temp.slave_address > 247 || self.temp.start_reg != 0 || self.temp.reg_count != 9 {
            return Err(ConfigError::InvalidField(
                "COM_RS485_TEMP register/address layout",
            ));
        }
        if self.temp.temp_channels != (1..=9).map(|i| format!("T{i}")).collect::<Vec<_>>() {
            return Err(ConfigError::InvalidField("COM_RS485_TEMP.TEMP_CHANNELS"));
        }
        let ports = [&self.mfc.port, &self.temp.port, &self.balance.port].map(|port| {
            let upper = port.to_ascii_uppercase();
            if cfg!(windows)
                || upper.strip_prefix("COM").is_some_and(|suffix| {
                    !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit())
                })
            {
                upper
            } else {
                port.clone()
            }
        });
        if ports[0] == ports[1] || ports[0] == ports[2] || ports[1] == ports[2] {
            return Err(ConfigError::InvalidField("serial ports must be distinct"));
        }
        let a = &self.slave_address_mfc;
        let addrs = [a.h2, a.n2, a.co2, a.co];
        if addrs.iter().any(|a| !(1..=99).contains(a)) {
            return Err(ConfigError::InvalidField("SLAVE_ADDRESS_MFC (1..99)"));
        }
        for i in 0..addrs.len() {
            for j in i + 1..addrs.len() {
                if addrs[i] == addrs[j] {
                    return Err(ConfigError::SlaveAddrConflict);
                }
            }
        }
        Ok(())
    }
}

fn validate_serial(name: &'static str, serial: &SerialParams) -> Result<(), ConfigError> {
    if serial.port.trim() != serial.port
        || serial.port.len() > 512
        || serial.port.chars().any(char::is_control)
        || serial.baudrate == 0
        || !(5..=8).contains(&serial.bytesize)
        || !matches!(serial.parity.as_str(), "N" | "E" | "O")
        || !matches!(serial.stopbits, 1 | 2)
        || !serial.timeout.is_finite()
        || !(0.001..=60.0).contains(&serial.timeout)
    {
        return Err(ConfigError::InvalidField(name));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r#"{
      "COM_RS485_MFC":    { "port": "COM12", "baudrate": 9600, "bytesize": 8, "parity": "E", "stopbits": 1, "timeout": 0.5 },
      "COM_RS485_TEMP":   { "port": "COM10", "baudrate": 9600, "bytesize": 8, "parity": "N", "stopbits": 1, "timeout": 1.0,
                            "slave_address": 0, "start_reg": 0, "reg_count": 9,
                            "TEMP_CHANNELS": ["T1","T2","T3","T4","T5","T6","T7","T8","T9"],
                            "scale": 0.1, "signed_registers": false },
      "COM_RS232_Balance":{ "port": "COM15", "baudrate": 1200, "bytesize": 8, "parity": "N", "stopbits": 1, "timeout": 1.0 },
      "SAMPLING":         { "interval_s": 1.0 },
      "PERFORMANCE_CONFIG": { "data_collection_interval": 0.2, "experiment_data_interval": 1.0, "heartbeat_timeout": 5.0 },
      "SLAVE_ADDRESS_MFC": { "H2": 1, "N2": 2, "CO2": 3, "CO": 4 },
      "FLOW_SCALING":      { "H2": 0.1, "N2": 1.0, "CO2": 0.1, "CO": 0.1 },
      "GAS_SAFETY_LIMITS": { "H2": 5.0, "CO": 5.0 }
    }"#;

    #[test]
    fn example_parses_and_validates() {
        let c = CommConfig::from_json(EXAMPLE).unwrap();
        c.validate().unwrap();
        assert_eq!(c.slave_address_mfc.co, 4);
        assert_eq!(c.temp.temp_channels.len(), 9);
        assert_eq!(c.flow_scaling.n2, 1.0);
    }

    #[test]
    fn missing_optional_sections_use_defaults() {
        let mut v: serde_json::Value = serde_json::from_str(EXAMPLE).unwrap();
        v.as_object_mut().unwrap().remove("FLOW_SCALING");
        v.as_object_mut().unwrap().remove("GAS_SAFETY_LIMITS");
        let c = CommConfig::from_json(&v.to_string()).unwrap();
        assert_eq!(c.flow_scaling.h2, 0.1);
        assert_eq!(c.flow_scaling.n2, 0.1); // default is 0.1, not the example's 1.0
        assert_eq!(c.gas_safety_limits.h2, 5.0);
        assert_eq!(c.gas_safety_limits.co, 5.0);
    }

    fn base() -> CommConfig {
        CommConfig::from_json(EXAMPLE).unwrap()
    }

    #[test]
    fn empty_port_is_rejected() {
        let mut c = base();
        c.temp.port = "  ".into();
        assert_eq!(c.validate(), Err(ConfigError::EmptyPort("COM_RS485_TEMP")));
    }

    #[test]
    fn zero_scale_is_rejected() {
        let mut c = base();
        c.temp.scale = 0.0;
        assert_eq!(
            c.validate(),
            Err(ConfigError::ZeroScale("COM_RS485_TEMP.scale"))
        );
    }

    #[test]
    fn nonpositive_limit_is_rejected() {
        let mut c = base();
        c.gas_safety_limits.h2 = 0.0;
        assert_eq!(c.validate(), Err(ConfigError::NonPositiveLimit("H2")));
    }

    #[test]
    fn nonpositive_or_nonfinite_interval_is_rejected() {
        let mut c = base();
        c.sampling.interval_s = 0.0;
        assert_eq!(
            c.validate(),
            Err(ConfigError::NonPositiveInterval("SAMPLING.interval_s"))
        );
        let mut c = base();
        c.performance.heartbeat_timeout = f64::NAN;
        assert_eq!(
            c.validate(),
            Err(ConfigError::NonPositiveInterval(
                "PERFORMANCE_CONFIG.heartbeat_timeout"
            ))
        );
    }

    #[test]
    fn slave_address_conflict_is_rejected() {
        let mut c = base();
        c.slave_address_mfc.co = c.slave_address_mfc.h2;
        assert_eq!(c.validate(), Err(ConfigError::SlaveAddrConflict));
    }

    #[test]
    fn save_then_load_roundtrips() {
        let dir = std::env::temp_dir().join(format!("tmh_cfg_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("comm_config.json");
        base().save(&path).unwrap();
        let loaded = CommConfig::load(&path).unwrap();
        assert_eq!(loaded.slave_address_mfc.co2, 3);
        std::fs::remove_dir_all(&dir).ok();
    }
}
