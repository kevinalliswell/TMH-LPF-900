//! Real serial-port transport (serialport crate). Feature-gated behind `serial`
//! and NOT hardware-verified in CI — compiled only. Supports a recording mode that
//! appends every tx/rx as JSONL, so a field session can be replayed on a dev box.

use super::{BusError, Transport};
use serialport::SerialPort;
use std::io::{Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

/// Serial line parameters (from comm_config.json in M2 slice 2).
#[derive(Debug, Clone)]
pub struct SerialConfig {
    pub port: String,
    pub baud: u32,
    pub data_bits: serialport::DataBits,
    pub parity: serialport::Parity,
    pub stop_bits: serialport::StopBits,
    pub timeout: Duration,
}

struct Recorder {
    file: std::fs::File,
    start: Instant,
}

/// A real serial bus. One dedicated bus thread owns it; blocking I/O never runs on
/// the async runtime (docs/03).
pub struct SerialTransport {
    config: SerialConfig,
    port: Box<dyn SerialPort>,
    record: Option<Recorder>,
}

impl SerialTransport {
    /// Open the port with `config`.
    pub fn open(config: SerialConfig) -> Result<Self, BusError> {
        let port = open_port(&config)?;
        Ok(Self {
            config,
            port,
            record: None,
        })
    }

    /// Enable recording: every tx/rx is appended to `path` as a JSONL session.
    pub fn with_recording(mut self, path: &Path) -> Result<Self, BusError> {
        let file = std::fs::File::create(path).map_err(|e| BusError::Io(e.to_string()))?;
        self.record = Some(Recorder {
            file,
            start: Instant::now(),
        });
        Ok(self)
    }

    fn record(&mut self, dir: &str, bytes: &[u8]) {
        if let Some(r) = &mut self.record {
            let t = r.start.elapsed().as_secs_f64();
            let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02x}")).collect();
            // Recording must never disturb control flow — ignore write errors.
            let _ = writeln!(
                r.file,
                r#"{{"t":{t:.3},"dir":"{dir}","bytes":"{}"}}"#,
                hex.join(" ")
            );
        }
    }
}

impl Transport for SerialTransport {
    fn transact(&mut self, tx: &[u8], deadline: Duration) -> Result<Vec<u8>, BusError> {
        self.port
            .write_all(tx)
            .map_err(|e| BusError::Io(e.to_string()))?;
        self.port.flush().map_err(|e| BusError::Io(e.to_string()))?;
        self.record("tx", tx);

        let mut buf = Vec::new();
        let mut chunk = [0u8; 256];
        let start = Instant::now();
        while start.elapsed() < deadline {
            match self.port.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                    if !buf.is_empty() {
                        break;
                    }
                }
                Err(e) => return Err(BusError::Io(e.to_string())),
            }
        }
        self.record("rx", &buf);
        if buf.is_empty() {
            Err(BusError::Timeout)
        } else {
            Ok(buf)
        }
    }

    fn read_line(&mut self, deadline: Duration) -> Result<Vec<u8>, BusError> {
        let mut buf = Vec::new();
        let mut byte = [0u8; 1];
        let start = Instant::now();
        while start.elapsed() < deadline {
            match self.port.read(&mut byte) {
                Ok(0) => break,
                Ok(_) => {
                    buf.push(byte[0]);
                    if byte[0] == b'\n' {
                        break;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => break,
                Err(e) => return Err(BusError::Io(e.to_string())),
            }
        }
        self.record("rx", &buf);
        if buf.is_empty() {
            Err(BusError::Timeout)
        } else {
            Ok(buf)
        }
    }

    fn reset_buffers(&mut self) -> Result<(), BusError> {
        self.port
            .clear(serialport::ClearBuffer::All)
            .map_err(|e| BusError::Io(e.to_string()))
    }

    fn reopen(&mut self) -> Result<(), BusError> {
        self.port = open_port(&self.config)?;
        Ok(())
    }
}

fn open_port(config: &SerialConfig) -> Result<Box<dyn SerialPort>, BusError> {
    serialport::new(&config.port, config.baud)
        .data_bits(config.data_bits)
        .parity(config.parity)
        .stop_bits(config.stop_bits)
        .timeout(config.timeout)
        .open()
        .map_err(map_serial_err)
}

fn map_serial_err(e: serialport::Error) -> BusError {
    let s = e.to_string();
    if matches!(e.kind(), serialport::ErrorKind::NoDevice) {
        BusError::PortNotFound(s)
    } else if s.contains("in use") || s.contains("Access is denied") || s.contains("busy") {
        BusError::PortInUse(s)
    } else {
        BusError::Io(s)
    }
}
