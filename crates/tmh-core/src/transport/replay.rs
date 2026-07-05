//! Replay a recorded serial session (docs/08). Each JSONL line is
//! `{"t": secs, "dir": "tx"|"rx"|"gap", "bytes": "hex"}`. `transact`/`read_line`
//! walk the timeline: `rx` yields recorded bytes, `gap` yields a read timeout
//! (disconnect segment), `tx` records are checked against what was sent.

use super::{BusError, Transport};
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dir {
    Tx,
    Rx,
    Gap,
}

struct Record {
    dir: Dir,
    bytes: Vec<u8>,
}

/// Deterministic replay of a recorded (or hand-crafted) session.
pub struct ReplayTransport {
    records: Vec<Record>,
    cursor: usize,
    verify_tx: bool,
    tx_mismatches: usize,
}

impl ReplayTransport {
    /// Parse a JSONL session. `verify_tx` off by default; enable to flag when a
    /// sent request differs from what the recording expected.
    pub fn from_jsonl(text: &str) -> Result<Self, String> {
        let mut records = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let v: serde_json::Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
            let dir = match v["dir"].as_str() {
                Some("tx") => Dir::Tx,
                Some("rx") => Dir::Rx,
                Some("gap") => Dir::Gap,
                other => return Err(format!("bad dir: {other:?}")),
            };
            let bytes = match v["bytes"].as_str() {
                Some(h) => unhex(h)?,
                None => Vec::new(),
            };
            records.push(Record { dir, bytes });
        }
        Ok(Self {
            records,
            cursor: 0,
            verify_tx: false,
            tx_mismatches: 0,
        })
    }

    /// Enable checking that sent requests match the recorded `tx` frames.
    pub fn verify_tx(mut self, on: bool) -> Self {
        self.verify_tx = on;
        self
    }

    /// Number of sent requests that did not match the recorded `tx` (only counted
    /// when `verify_tx` is enabled).
    pub fn tx_mismatches(&self) -> usize {
        self.tx_mismatches
    }

    fn next_response(&mut self, tx: &[u8]) -> Result<Vec<u8>, BusError> {
        while self.cursor < self.records.len() {
            let idx = self.cursor;
            self.cursor += 1;
            match self.records[idx].dir {
                Dir::Tx => {
                    if self.verify_tx && self.records[idx].bytes != tx {
                        self.tx_mismatches += 1;
                    }
                }
                Dir::Rx => return Ok(self.records[idx].bytes.clone()),
                Dir::Gap => return Err(BusError::Timeout),
            }
        }
        Err(BusError::Disconnected)
    }
}

impl Transport for ReplayTransport {
    fn transact(&mut self, tx: &[u8], _deadline: Duration) -> Result<Vec<u8>, BusError> {
        self.next_response(tx)
    }

    fn read_line(&mut self, _deadline: Duration) -> Result<Vec<u8>, BusError> {
        self.next_response(&[])
    }

    fn reset_buffers(&mut self) -> Result<(), BusError> {
        Ok(())
    }

    fn reopen(&mut self) -> Result<(), BusError> {
        Ok(()) // cursor already advanced past the gap; retry continues the timeline
    }
}

fn unhex(s: &str) -> Result<Vec<u8>, String> {
    s.split_whitespace()
        .map(|h| u8::from_str_radix(h, 16).map_err(|e| e.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tmh_protocols::modbus;

    const DIRTY: &str = include_str!("../../../../fixtures/dirty-session.jsonl");

    #[test]
    fn dirty_session_runs_to_completion_with_correct_error_count() {
        let mut r = ReplayTransport::from_jsonl(DIRTY).unwrap();
        let req = modbus::build_read_all(0);
        let (mut ok, mut parse_err, mut timeouts) = (0, 0, 0);
        loop {
            match r.transact(&req, Duration::from_millis(10)) {
                Ok(bytes) => {
                    if modbus::extract_read_all(&bytes, 0).is_some() {
                        ok += 1;
                    } else {
                        parse_err += 1;
                    }
                }
                Err(BusError::Timeout) => timeouts += 1,
                Err(BusError::Disconnected) => break,
                Err(e) => panic!("unexpected: {e:?}"),
            }
        }
        assert_eq!(ok, 2, "echo-prefixed valid + recovery frame");
        assert_eq!(parse_err, 2, "half-frame + bad-crc");
        assert_eq!(timeouts, 1, "one disconnect gap");
    }

    #[test]
    fn verify_tx_flags_mismatch() {
        let session = r#"{"t":0,"dir":"tx","bytes":"01 02 03"}
{"t":0.1,"dir":"rx","bytes":"aa bb"}"#;
        let mut r = ReplayTransport::from_jsonl(session)
            .unwrap()
            .verify_tx(true);
        let _ = r.transact(&[0xFF], Duration::from_millis(1)); // does not match "01 02 03"
        assert_eq!(r.tx_mismatches(), 1);
    }
}
