#![forbid(unsafe_code)]
//! Core control-plane logic for TMH-LPF-900: bus actors, safety clamp, sampler,
//! storage, audit, experiment engine. MUST compile and run headless — no Tauri,
//! no webview dependency ever.

pub mod audit;
pub mod auth;
pub mod bus;
pub mod clamp;
pub mod config;
pub mod engine;
pub mod gb;
pub mod health;
pub mod report;
pub mod sampler;
pub mod storage;
pub mod telemetry;
pub mod transport;

use thiserror::Error;

/// Top-level error type for the core crate. Variants grow each milestone.
#[derive(Debug, Error)]
pub enum CoreError {
    /// A device bus is offline or unreachable.
    #[error("device offline: {0}")]
    DeviceOffline(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_offline_error_message_is_stable() {
        let err = CoreError::DeviceOffline("temp".to_string());
        assert_eq!(err.to_string(), "device offline: temp");
    }
}

pub mod recovery;

pub mod program;
