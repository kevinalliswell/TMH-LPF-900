//! MFC mass-flow controllers — Azbil CPL ASCII frames, RS485 half-duplex (shared).

const STX: u8 = 0x02;
const ETX: u8 = 0x03;
const HEX: &[u8; 16] = b"0123456789ABCDEF";

/// CPL checksum over the STX..=ETX span: two's complement of the low byte of the
/// sum, as two uppercase hex ASCII bytes.
pub fn cpl_checksum(stx_to_etx: &[u8]) -> [u8; 2] {
    let sum: u32 = stx_to_etx.iter().map(|&b| b as u32).sum();
    let ck = 0_u8.wrapping_sub(sum as u8);
    [HEX[(ck >> 4) as usize], HEX[(ck & 0x0F) as usize]]
}

fn frame(addr: u8, op: &str, reg: u16, tail: &str) -> Vec<u8> {
    // <STX>{addr:02X}{op}{reg}"W,"{tail}<ETX>{ck}\r\n   op = "00XRS," | "00XWS,"
    let mut body = vec![STX];
    body.extend_from_slice(format!("{addr:02X}").as_bytes());
    body.extend_from_slice(op.as_bytes());
    body.extend_from_slice(reg.to_string().as_bytes());
    body.extend_from_slice(b"W,");
    body.extend_from_slice(tail.as_bytes());
    body.push(ETX);
    let ck = cpl_checksum(&body);
    body.extend_from_slice(&ck);
    body.extend_from_slice(b"\r\n");
    body
}

/// Build a read request for `n` words at `reg` on slave `addr`.
pub fn build_read(addr: u8, reg: u16, n: u8) -> Vec<u8> {
    frame(addr, "00XRS,", reg, &n.to_string())
}

/// Build a write request setting `reg` to `values` (comma-joined) on slave `addr`.
pub fn build_write(addr: u8, reg: u16, values: &[i64]) -> Vec<u8> {
    let joined = values
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(",");
    frame(addr, "00XWS,", reg, &joined)
}

/// Parse a device response: STX..ETX content, comma-split, 2nd field / 10.0.
/// Tolerates any/missing frame tail. `None` on missing STX / field / bad number.
/// (The per-gas FLOW_SCALING multiply is applied in tmh-core, not here.)
pub fn parse_response(buf: &[u8]) -> Option<f64> {
    let start = buf.iter().position(|&b| b == STX)?;
    let after = &buf[start + 1..];
    let content = match after.iter().position(|&b| b == ETX) {
        Some(e) => &after[..e],
        None => after,
    };
    let s = core::str::from_utf8(content).ok()?;
    let field = s.split(',').nth(1)?.trim();
    Some(field.parse::<f64>().ok()? / 10.0)
}
