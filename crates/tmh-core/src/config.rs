//! Device communication config (`comm_config.json`, docs/06 §6.4 — legacy schema,
//! serde defaults as fallback). `validate()` is a hard gate: on failure the app
//! shows the config-error page and refuses to open buses; it NEVER silently
//! overwrites the user's file with defaults. Write-back is atomic (temp + rename).

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerialParams {
    pub port: String,
    pub baudrate: u32,
    pub bytesize: u8,
    pub parity: String,
    pub stopbits: u8,
    pub timeout: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sampling {
    pub interval_s: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Performance {
    pub data_collection_interval: f64,
    pub experiment_data_interval: f64,
    pub heartbeat_timeout: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    #[error("scale must be non-zero: {0}")]
    ZeroScale(&'static str),
    #[error("MFC slave addresses must be distinct")]
    SlaveAddrConflict,
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

    /// Atomically write back (temp file + rename). Caller does read-modify-write.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        let json =
            serde_json::to_string_pretty(self).map_err(|e| ConfigError::Parse(e.to_string()))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| ConfigError::Io(e.to_string()))?;
        std::fs::rename(&tmp, path).map_err(|e| ConfigError::Io(e.to_string()))?;
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
        if self.gas_safety_limits.h2 <= 0.0 {
            return Err(ConfigError::NonPositiveLimit("H2"));
        }
        if self.gas_safety_limits.co <= 0.0 {
            return Err(ConfigError::NonPositiveLimit("CO"));
        }
        if self.temp.scale == 0.0 {
            return Err(ConfigError::ZeroScale("COM_RS485_TEMP.scale"));
        }
        for (name, v) in [
            ("H2", self.flow_scaling.h2),
            ("N2", self.flow_scaling.n2),
            ("CO2", self.flow_scaling.co2),
            ("CO", self.flow_scaling.co),
        ] {
            if v == 0.0 {
                return Err(ConfigError::ZeroScale(name));
            }
        }
        let a = &self.slave_address_mfc;
        let addrs = [a.h2, a.n2, a.co2, a.co];
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
