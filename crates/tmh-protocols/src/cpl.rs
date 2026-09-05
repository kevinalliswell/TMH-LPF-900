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

/// Strict response decoder for the repository's simulated CPL profile. Real device
/// success/exception codes and write acknowledgement semantics require its manual.
/// Requires one complete frame, the known header, checksum and CRLF. No NaN,
/// ambiguous extra fields, truncated tails, or unsolicited frames are accepted.
pub fn parse_response(buf: &[u8]) -> Option<f64> {
    if buf.len() < 15 || buf.len() > 128 || buf.first() != Some(&STX) {
        return None;
    }
    let end = buf.iter().position(|&b| b == ETX)?;
    if end + 5 != buf.len()
        || &buf[end + 3..] != b"\r\n"
        || buf[end + 1..end + 3] != cpl_checksum(&buf[..=end])
    {
        return None;
    }
    let text = core::str::from_utf8(&buf[1..end]).ok()?;
    let (header, value) = text.split_once(',')?;
    if header.len() != 7
        || !header.as_bytes()[..2].iter().all(u8::is_ascii_hexdigit)
        || &header[2..] != "00X20"
    {
        return None;
    }
    // This profile uses an integer word in tenths. Reject fractional, exponential
    // and nonfinite values instead of letting a float comparison acknowledge them.
    let value = value.parse::<i64>().ok()? as f64 / 10.0;
    value.is_finite().then_some(value)
}

/// Match the addressed response to a new request; permit only an exact local echo
/// followed by a valid response. An echo alone never confirms a write.
pub fn parse_transaction(buf: &[u8], request: &[u8], address: u8) -> Option<f64> {
    let response = buf.strip_prefix(request).unwrap_or(buf);
    let value = parse_response(response)?;
    let actual = u8::from_str_radix(core::str::from_utf8(&response[1..3]).ok()?, 16).ok()?;
    (actual == address).then_some(value)
}
