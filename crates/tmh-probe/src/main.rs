//! `tmh-probe` — headless serial bring-up tool for the real devices (docs/09 hardware
//! gate). Drives the transport directly with the golden-tested codecs so every byte is
//! visible, and records each session as replayable JSONL. Point any device command at a
//! live `--port` OR at a recorded `--replay <file>` — the analysis is identical, so a
//! field capture can be re-diagnosed offline on a dev box. Read-only by default; only
//! `mfc --set` writes a setpoint.

use clap::{Args, Parser, Subcommand};
use serialport::{DataBits, Parity, StopBits};
use std::path::PathBuf;
use std::time::Duration;
use tmh_core::bus::MfcActor;
use tmh_core::clamp::{CO_ABS_MAX, H2_ABS_MAX};
use tmh_core::transport::{ReplayTransport, SerialConfig, SerialTransport, Transport};
use tmh_protocols::{balance, cpl, modbus};

/// Azbil CPL registers (match the MFC bus actor).
const REG_PV: u16 = 1206;

#[derive(Parser)]
#[command(
    name = "tmh-probe",
    version,
    about = "TMH-LPF-900 serial bring-up probe"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List available serial ports.
    List,
    /// Probe the 9-channel temperature controller (Modbus-RTU).
    Temp {
        #[command(flatten)]
        link: Link,
        #[arg(long, default_value_t = 0)]
        addr: u8,
        #[arg(long, default_value_t = 0.1)]
        scale: f64,
        #[arg(long)]
        signed: bool,
    },
    /// Probe the 4 Azbil MFC gas controllers (CPL). READ-ONLY unless --set is given.
    Mfc {
        #[command(flatten)]
        link: Link,
        /// Four slave addresses, comma-separated.
        #[arg(long, default_value = "1,2,3,4")]
        addrs: String,
        /// Four flow-scaling factors, comma-separated (order H2,N2,CO2,CO).
        #[arg(long, default_value = "0.1,1.0,0.1,0.1")]
        scaling: String,
        /// Bring-up writes: N2 0..5 L/min (`--set 1:VALUE`), or zero on any channel.
        /// Positive H2/CO/CO2 writes remain disabled pending field qualification.
        #[arg(long)]
        set: Option<String>,
    },
    /// Probe the RS232 balance (continuous line output).
    Balance {
        #[command(flatten)]
        link: Link,
        /// Send a tare (去皮) first and print the response.
        #[arg(long)]
        tare: bool,
    },
}

#[derive(Args)]
struct Link {
    /// Serial port, e.g. COM3 (Windows) or /dev/ttyUSB0. One of --port / --replay.
    #[arg(long)]
    port: Option<String>,
    /// Replay a recorded JSONL session instead of opening a port (offline diagnosis).
    #[arg(long)]
    replay: Option<PathBuf>,
    #[arg(long, default_value_t = 9600)]
    baud: u32,
    /// none | even | odd
    #[arg(long, default_value = "none")]
    parity: String,
    #[arg(long, default_value_t = 8)]
    databits: u8,
    #[arg(long, default_value_t = 1)]
    stopbits: u8,
    #[arg(long, default_value_t = 500)]
    timeout_ms: u64,
    #[arg(long, default_value_t = 500)]
    interval_ms: u64,
    /// How long to poll a live port before exiting (ignored for --replay).
    #[arg(long, default_value_t = 20)]
    seconds: u64,
    /// Record every tx/rx frame to this JSONL file (replayable). Live ports only.
    #[arg(long)]
    record: Option<PathBuf>,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    match Cli::parse().cmd {
        Cmd::List => list_ports(),
        Cmd::Temp {
            link,
            addr,
            scale,
            signed,
        } => {
            let plan = link.plan();
            let mut t = link.transport()?;
            probe_temp(t.as_mut(), addr, scale, signed, plan)
        }
        Cmd::Mfc {
            link,
            addrs,
            scaling,
            set,
        } => {
            let plan = link.plan();
            let t = link.transport()?;
            probe_mfc(t, &addrs, &scaling, set.as_deref(), plan)
        }
        Cmd::Balance { link, tare } => {
            let plan = link.plan();
            let mut t = link.transport()?;
            probe_balance(t.as_mut(), tare, plan)
        }
    }
}

/// Loop shape: live ports poll on a wall-clock budget; replay consumes the timeline.
struct Plan {
    cycles: u64,
    interval_ms: u64,
    timeout_ms: u64,
    stop_after_errs: Option<u32>,
}

impl Link {
    fn plan(&self) -> Plan {
        if self.replay.is_some() {
            Plan {
                cycles: 1_000_000,
                interval_ms: 0,
                timeout_ms: self.timeout_ms,
                stop_after_errs: Some(2), // timeline exhausted → stop
            }
        } else {
            let interval = self.interval_ms.max(1);
            Plan {
                cycles: (self.seconds * 1000 / interval).max(1),
                interval_ms: interval,
                timeout_ms: self.timeout_ms,
                stop_after_errs: None,
            }
        }
    }

    fn transport(&self) -> Result<Box<dyn Transport>, String> {
        match (&self.replay, &self.port) {
            (Some(path), _) => {
                let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
                let t = ReplayTransport::from_jsonl(&text)?;
                println!("replaying {} ...\n", path.display());
                Ok(Box::new(t))
            }
            (None, Some(port)) => {
                let cfg = SerialConfig {
                    port: port.clone(),
                    baud: self.baud,
                    data_bits: match self.databits {
                        5 => DataBits::Five,
                        6 => DataBits::Six,
                        7 => DataBits::Seven,
                        _ => DataBits::Eight,
                    },
                    parity: match self.parity.as_str() {
                        "even" => Parity::Even,
                        "odd" => Parity::Odd,
                        _ => Parity::None,
                    },
                    stop_bits: if self.stopbits == 2 {
                        StopBits::Two
                    } else {
                        StopBits::One
                    },
                    timeout: Duration::from_millis(self.timeout_ms),
                };
                println!(
                    "opening {} @ {} {}-{}-{} ...",
                    cfg.port, cfg.baud, self.databits, self.parity, self.stopbits
                );
                let mut t = SerialTransport::open(cfg).map_err(|e| format!("open failed: {e}"))?;
                if let Some(p) = &self.record {
                    t = t.with_recording(p).map_err(|e| e.to_string())?;
                    println!("recording raw frames → {}", p.display());
                }
                Ok(Box::new(t))
            }
            (None, None) => Err("need --port <COMx> or --replay <file.jsonl>".into()),
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_list<T: std::str::FromStr>(s: &str) -> Result<Vec<T>, String> {
    s.split(',')
        .map(|x| x.trim().parse::<T>().map_err(|_| format!("bad value: {x}")))
        .collect()
}

fn list_ports() -> Result<(), String> {
    let ports = serialport::available_ports().map_err(|e| e.to_string())?;
    if ports.is_empty() {
        println!("(no serial ports found)");
        return Ok(());
    }
    println!("available serial ports:");
    for p in ports {
        let kind = match p.port_type {
            serialport::SerialPortType::UsbPort(u) => format!(
                "USB {:04x}:{:04x} {}",
                u.vid,
                u.pid,
                u.product.unwrap_or_default()
            ),
            serialport::SerialPortType::PciPort => "PCI".into(),
            serialport::SerialPortType::BluetoothPort => "Bluetooth".into(),
            serialport::SerialPortType::Unknown => "unknown".into(),
        };
        println!("  {}  [{kind}]", p.port_name);
    }
    Ok(())
}

/// Run `cycle` for each tick of the plan, breaking after N consecutive errors (replay
/// timeline exhausted / device gone). `cycle` returns Ok(()) on a good read, Err on a bad one.
fn drive(plan: &Plan, mut cycle: impl FnMut() -> bool) {
    let mut consecutive = 0u32;
    for _ in 0..plan.cycles {
        let ok = cycle();
        consecutive = if ok { 0 } else { consecutive + 1 };
        if let Some(limit) = plan.stop_after_errs {
            if consecutive >= limit {
                break;
            }
        }
        if plan.interval_ms > 0 {
            std::thread::sleep(Duration::from_millis(plan.interval_ms));
        }
    }
}

fn probe_temp(
    t: &mut dyn Transport,
    addr: u8,
    scale: f64,
    signed: bool,
    plan: Plan,
) -> Result<(), String> {
    let req = modbus::build_read_all(addr);
    let timeout = Duration::from_millis(plan.timeout_ms);
    println!("TEMP addr={addr} scale={scale} signed={signed}\n");
    let (mut ok, mut err) = (0u32, 0u32);
    drive(&plan, || match t.transact(&req, timeout) {
        Ok(rx) => match modbus::extract_read_all(&rx, addr)
            .and_then(|f| modbus::parse_read_all(f, scale, signed))
        {
            Some(temps) => {
                ok += 1;
                let list = temps
                    .iter()
                    .map(|v| format!("{v:.1}"))
                    .collect::<Vec<_>>()
                    .join(",");
                println!("RX {} | T1..9=[{list}] | T8={:.1}", hex(&rx), temps[7]);
                true
            }
            None => {
                err += 1;
                println!(
                    "RX {} | ✗ parse failed (CRC/addr/frame) — check addr/scale/wiring",
                    hex(&rx)
                );
                false
            }
        },
        Err(e) => {
            err += 1;
            println!("TX {} | ✗ {e}", hex(&req));
            false
        }
    });
    println!("\nTEMP result: {ok} ok / {err} err");
    Ok(())
}

fn probe_mfc(
    mut t: Box<dyn Transport>,
    addrs: &str,
    scaling: &str,
    set: Option<&str>,
    plan: Plan,
) -> Result<(), String> {
    let addrs = parse_list::<u8>(addrs)?;
    let scaling = parse_list::<f64>(scaling)?;
    if addrs.len() != 4 || scaling.len() != 4 {
        return Err("need exactly 4 addrs and 4 scaling values".into());
    }
    let timeout = Duration::from_millis(plan.timeout_ms);

    if scaling
        .iter()
        .any(|value| !value.is_finite() || *value <= 0.0)
    {
        return Err("flow scaling must be finite and positive".into());
    }
    if let Some(spec) = set {
        let (gas, value) = parse_set(spec)?;
        let mut actor = MfcActor::new(
            t,
            addrs.clone().try_into().unwrap(),
            scaling.clone().try_into().unwrap(),
            H2_ABS_MAX,
            CO_ABS_MAX,
        );
        let preview = actor
            .preview_flow(gas, value)
            .map_err(|error| error.to_string())?;
        if gas == 1 && preview.applied > 5.0 {
            return Err("quantized N2 setpoint exceeds the probe bring-up limit".into());
        }
        let confirmed = actor
            .set_flow(gas, value)
            .map_err(|error| format!("MFC write not confirmed: {error}"))?;
        println!(
            "WRITING confirmed: gas {gas} requested {value}, applied {} L/min, clamped {}",
            confirmed.applied, confirmed.clamped
        );
        t = actor.into_transport();
    }

    println!("MFC PV (READ-ONLY) addrs={addrs:?} scaling={scaling:?}  [0=H2 1=N2 2=CO2 3=CO]\n");
    drive(&plan, || {
        let mut all_ok = true;
        for (i, &addr) in addrs.iter().enumerate() {
            let req = cpl::build_read(addr, REG_PV, 2);
            match t.transact(&req, timeout) {
                Ok(rx) => match cpl::parse_response(&rx) {
                    Some(v) => println!(
                        "MFC[{i}] addr {addr} | RX {} | PV {:.2} L/min",
                        hex(&rx),
                        v * scaling[i]
                    ),
                    None => {
                        all_ok = false;
                        println!("MFC[{i}] addr {addr} | RX {} | ✗ parse failed", hex(&rx));
                    }
                },
                Err(e) => {
                    all_ok = false;
                    println!("MFC[{i}] addr {addr} | ✗ {e}");
                }
            }
        }
        println!();
        all_ok
    });
    Ok(())
}

fn parse_set(spec: &str) -> Result<(usize, f64), String> {
    let (g, v) = spec
        .split_once(':')
        .ok_or("--set expects GAS:VALUE, e.g. 1:5.0")?;
    let gas: usize = g.trim().parse().map_err(|_| "bad gas index")?;
    let value: f64 = v.trim().parse().map_err(|_| "bad value")?;
    if gas > 3 {
        return Err("gas index must be 0..3".into());
    }
    if !value.is_finite() || value < 0.0 {
        return Err("setpoint must be finite and non-negative".into());
    }
    if (gas != 1 && value != 0.0) || (gas == 1 && value > 5.0) {
        return Err("probe writes permit N2 0..5 L/min or zero on any channel; other positive writes are disabled".into());
    }
    Ok((gas, value))
}

fn probe_balance(t: &mut dyn Transport, tare: bool, plan: Plan) -> Result<(), String> {
    let timeout = Duration::from_millis(plan.timeout_ms);
    if tare {
        println!("sending TARE (去皮) ...");
        let _ = t.transact(&balance::TARE_CMD, Duration::from_millis(1500));
        for _ in 0..15 {
            if let Ok(b) = t.read_line(Duration::from_millis(200)) {
                let line = String::from_utf8_lossy(&b);
                println!("  {}", line.trim_end());
                if line.contains("A00") {
                    break;
                }
            }
        }
    }
    println!("BALANCE lines\n");
    drive(&plan, || match t.read_line(timeout) {
        Ok(bytes) => {
            let line = String::from_utf8_lossy(&bytes);
            let trimmed = line.trim_end();
            match balance::parse_line(&line) {
                Some(w) => println!("RX {trimmed:?} | weight {w:.3} g"),
                None => println!("RX {trimmed:?} | (non-stable / unparsed)"),
            }
            true
        }
        Err(e) => {
            println!("✗ {e}");
            false
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tmh_core::transport::{BusError, MockTransport};
    struct TraceTransport {
        writes: Arc<Mutex<Vec<Vec<u8>>>>,
        ack: bool,
        inner: MockTransport,
    }
    impl Transport for TraceTransport {
        fn transact(&mut self, tx: &[u8], deadline: Duration) -> Result<Vec<u8>, BusError> {
            self.writes.lock().unwrap().push(tx.to_vec());
            if self.ack {
                self.inner.transact(tx, deadline)
            } else {
                Ok(b"not-an-ack".to_vec())
            }
        }
        fn read_line(&mut self, deadline: Duration) -> Result<Vec<u8>, BusError> {
            self.inner.read_line(deadline)
        }
        fn reset_buffers(&mut self) -> Result<(), BusError> {
            self.inner.reset_buffers()
        }
        fn reopen(&mut self) -> Result<(), BusError> {
            self.inner.reopen()
        }
    }
    fn probe(spec: &str, ack: bool) -> (Result<(), String>, Vec<Vec<u8>>) {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let transport = TraceTransport {
            writes: writes.clone(),
            ack,
            inner: MockTransport::mfc(),
        };
        let result = probe_mfc(
            Box::new(transport),
            "1,2,3,4",
            "0.1,1.0,0.1,0.1",
            Some(spec),
            Plan {
                cycles: 0,
                interval_ms: 0,
                timeout_ms: 500,
                stop_after_errs: None,
            },
        );
        let frames = writes.lock().unwrap().clone();
        (result, frames)
    }
    #[test]
    fn probe_writes_use_actor_clamp_and_ack_instead_of_a_raw_write_path() {
        let (result, frames) = probe("1:5.0", true);
        result.unwrap();
        assert_eq!(frames, vec![cpl::build_write(2, 1401, &[50])]);
        let (failed, _) = probe("1:2.0", false);
        assert!(failed.unwrap_err().contains("not confirmed"));
        for spec in ["0:2.0", "3:2.0", "2:2.0"] {
            let (result, frames) = probe(spec, true);
            assert!(result.is_err());
            assert!(frames.is_empty());
        }
    }

    #[test]
    fn invalid_probe_setpoints_never_reach_the_transport() {
        for spec in ["1:-1", "1:NaN", "1:inf", "1:5.01", "4:1"] {
            let (result, frames) = probe(spec, true);
            assert!(result.is_err());
            assert!(frames.is_empty());
        }
        for gas in 0..4 {
            let (zero, frames) = probe(&format!("{gas}:0"), true);
            zero.unwrap();
            assert_eq!(frames, vec![cpl::build_write(gas + 1, 1401, &[0])]);
        }
    }
    #[test]
    fn probe_rejects_nitrogen_quantization_above_its_bringup_limit() {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let t = TraceTransport {
            writes: writes.clone(),
            ack: true,
            inner: MockTransport::mfc(),
        };
        let result = probe_mfc(
            Box::new(t),
            "1,2,3,4",
            "0.1,4.0,0.1,0.1",
            Some("1:5"),
            Plan {
                cycles: 0,
                interval_ms: 0,
                timeout_ms: 500,
                stop_after_errs: None,
            },
        );
        assert!(result.is_err());
        assert!(writes.lock().unwrap().is_empty());
    }
}
