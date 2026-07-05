//! Device bus actors (docs/03, docs/04 §4.4). Each actor owns a `Box<dyn Transport>`
//! and a [`Health`](crate::health::Health) state machine, polls its device, and
//! exposes a serializable snapshot. Per-device disconnection keeps the last value
//! plus a stale flag (`connected=false`) rather than zeroing.
//!
//! The per-tick logic here is single-threaded and deterministic (unit-tested against
//! `MockTransport`). The OS-thread runner and the tokio telemetry bridge are wired in
//! M2 slice 4.

pub mod balance;
pub mod mfc;
pub mod temp;

pub use balance::{BalanceActor, BalanceSnapshot};
pub use mfc::{GasReading, MfcActor, MfcCommand, MfcSnapshot};
pub use temp::{TempActor, TempSnapshot};
