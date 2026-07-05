//! Byte-exact acceptance against docs/golden_vectors.json (M1: all 26 vectors).
use serde_json::Value;
use tmh_protocols::{cpl, modbus};

const GOLDEN: &str = include_str!("../../../docs/golden_vectors.json");

fn root() -> Value {
    serde_json::from_str(GOLDEN).unwrap()
}
fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}
fn unhex(s: &str) -> Vec<u8> {
    s.split_whitespace()
        .map(|h| u8::from_str_radix(h, 16).unwrap())
        .collect()
}
/// First case in `section` whose "case" equals `name`.
fn case<'a>(root: &'a Value, section: &str, name: &str) -> &'a Value {
    root[section]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["case"].as_str() == Some(name))
        .unwrap_or_else(|| panic!("case not found: {name}"))
}

#[test]
fn modbus_golden() {
    let r = root();
    let sect = "modbus_rtu_temp";

    for (name, addr) in [
        ("build_read_all addr=0", 0u8),
        ("build_read_all addr=1", 1),
        ("build_read_all addr=3", 3),
    ] {
        let expect = case(&r, sect, name)["tx_hex"].as_str().unwrap();
        assert_eq!(hex(&modbus::build_read_all(addr)), expect, "{name}");
    }

    let c = case(&r, sect, "parse_read_all 9regs scale=0.1");
    let frame = unhex(c["rx_hex"].as_str().unwrap());
    let got = modbus::parse_read_all(&frame, 0.1, false).expect("parse ok");
    for (k, &g) in got.iter().enumerate() {
        let key = format!("T{}", k + 1);
        let e = c["expect"][&key].as_f64().unwrap();
        assert!((g - e).abs() < 1e-9, "{key}: {g} vs {e}");
    }

    let c = case(&r, sect, "extract_read_all skips echoed request");
    let buf = unhex(c["rx_hex"].as_str().unwrap());
    let frame = modbus::extract_read_all(&buf, 0).expect("frame found");
    assert_eq!(hex(frame), c["expect_frame_hex"].as_str().unwrap());
}

#[test]
fn cpl_golden() {
    let r = root();
    let sect = "azbil_cpl_mfc";

    for (name, addr, reg, n) in [
        ("read PV H2 addr=1 reg=1206", 1u8, 1206u16, 2u8),
        ("read PV N2 addr=2 reg=1206", 2, 1206, 2),
        ("read PV CO2 addr=3 reg=1206", 3, 1206, 2),
        ("read PV CO addr=4 reg=1206", 4, 1206, 2),
        ("read SV H2 reg=1401", 1, 1401, 2),
    ] {
        let expect = case(&r, sect, name)["tx_hex"].as_str().unwrap();
        assert_eq!(hex(&cpl::build_read(addr, reg, n)), expect, "{name}");
    }

    let expect = case(&r, sect, "write SV H2 3.0L/min scale0.1 -> raw300")["tx_hex"]
        .as_str()
        .unwrap();
    assert_eq!(hex(&cpl::build_write(1, 1401, &[300])), expect);

    for c in r[sect]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["case"].as_str() == Some("parse_response"))
    {
        let buf = unhex(c["rx_hex"].as_str().unwrap());
        let got = cpl::parse_response(&buf);
        match c["expect"].as_f64() {
            Some(e) => assert!((got.unwrap() - e).abs() < 1e-9, "rx={:?}", c["rx_hex"]),
            None => assert_eq!(got, None, "rx={:?}", c["rx_hex"]),
        }
    }
}
