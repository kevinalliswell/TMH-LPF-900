//! Transport abstraction: the seam between bus actors and the wire (docs/04 §4.5).
//! Three impls — synthetic `mock`, recorded-session `replay`, and real `serial`
//! (feature-gated). `tmh-core` stays headless-testable through mock/replay.
use std::time::Duration;

pub mod mock;
pub use mock::{MockFaults, MockTransport, TareOutcome};

/// Which device a mock/serial transport is standing in for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Temp,
    Mfc,
    Balance,
}

/// Transport-layer error. Bus actors map these onto health-state transitions.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BusError {
    #[error("timeout")]
    Timeout,
    #[error("device disconnected")]
    Disconnected,
    #[error("port in use: {0}")]
    PortInUse(String),
    #[error("port not found: {0}")]
    PortNotFound(String),
    #[error("io: {0}")]
    Io(String),
}

/// Half-duplex request/response + line reads for the three lab buses. `Send` so a
/// dedicated bus thread can own a `Box<dyn Transport>`.
pub trait Transport: Send {
    /// Send `tx`, read a response within `deadline`. Used by the request/response
    /// buses (temperature, MFC) and to push the tare command to the balance.
    fn transact(&mut self, tx: &[u8], deadline: Duration) -> Result<Vec<u8>, BusError>;
    /// Read one line within `deadline` (balance continuous output).
    fn read_line(&mut self, deadline: Duration) -> Result<Vec<u8>, BusError>;
    /// Clear input/output buffers.
    fn reset_buffers(&mut self) -> Result<(), BusError>;
    /// Close and reopen the underlying handle (reconnect / watchdog recovery).
    fn reopen(&mut self) -> Result<(), BusError>;
}
