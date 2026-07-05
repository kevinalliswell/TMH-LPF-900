//! Electronic balance — RS232 ASCII line protocol. Stable readings carry "G S".

/// Tare command bytes: ASCII "T \r\n".
pub const TARE_CMD: [u8; 4] = [0x54, 0x20, 0x0D, 0x0A];

/// Result of a tare command sequence.
#[derive(Debug, Clone, PartialEq)]
pub struct TareResult {
    pub success: bool,
    pub a00_found: bool,
    pub weight: Option<f64>,
}

/// Parse a weight from a stable line. Only lines containing the stability marker
/// "G S" count; extracts the first signed decimal `[+-]d+.d+`. No `regex` crate.
pub fn parse_line(line: &str) -> Option<f64> {
    if !line.contains("G S") {
        return None;
    }
    let b = line.as_bytes();
    for (start, &c) in b.iter().enumerate() {
        if c == b'+' || c == b'-' {
            if let Some(num) = scan_signed_decimal(b, start) {
                return core::str::from_utf8(num).ok()?.parse::<f64>().ok();
            }
        }
    }
    None
}

/// Match `[+-]\d+\.\d+` where `b[start]` is the sign; return the matched slice.
fn scan_signed_decimal(b: &[u8], start: usize) -> Option<&[u8]> {
    let mut i = start + 1;
    let int_begin = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == int_begin || i >= b.len() || b[i] != b'.' {
        return None;
    }
    i += 1;
    let frac_begin = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == frac_begin {
        return None;
    }
    Some(&b[start..i])
}

/// Judge a tare response (docs/04 §4.3): A00 present -> success; else a stable
/// near-zero reading (|w| < 0.15) -> success.
pub fn parse_tare_response(buf: &str) -> TareResult {
    let a00_found = buf.contains("A00");
    let weight = parse_line(buf);
    let success = a00_found || weight.is_some_and(|v| v.abs() < 0.15);
    TareResult {
        success,
        a00_found,
        weight,
    }
}
