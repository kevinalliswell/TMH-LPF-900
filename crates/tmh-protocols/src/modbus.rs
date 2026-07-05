//! Temperature controller — Modbus-RTU (function 0x03), RS485 half-duplex.

use crate::crc16_modbus;

/// Build the "read all 9 temperature registers" request:
/// `[addr, 0x03, 0x0000, 0x0009, crc_lo, crc_hi]` (CRC-16/MODBUS, little-endian).
pub fn build_read_all(addr: u8) -> Vec<u8> {
    let mut frame = vec![addr, 0x03, 0x00, 0x00, 0x00, 0x09];
    let crc = crc16_modbus(&frame);
    frame.push((crc & 0xFF) as u8);
    frame.push((crc >> 8) as u8);
    frame
}

/// Scan `buf` for a CRC-valid function-0x03 response from `addr`, skipping the
/// half-duplex echoed request. Returns the response frame slice, or `None` when no
/// valid frame is present (= read failed; NOT an empty-but-successful read).
pub fn extract_read_all(buf: &[u8], addr: u8) -> Option<&[u8]> {
    if buf.len() < 5 {
        return None;
    }
    for i in 0..=buf.len() - 5 {
        if buf[i] != addr || buf[i + 1] != 0x03 {
            continue;
        }
        let bc = buf[i + 2] as usize;
        if bc == 0 {
            continue; // echoed request carries 0x00 here
        }
        let end = i + 3 + bc + 2;
        if end > buf.len() {
            continue;
        }
        let crc = crc16_modbus(&buf[i..end - 2]);
        if buf[end - 2] == (crc & 0xFF) as u8 && buf[end - 1] == (crc >> 8) as u8 {
            return Some(&buf[i..end]);
        }
    }
    None
}

/// Parse a read-all response into 9 temperatures (`raw * scale`). `signed`
/// interprets each register as `i16` before scaling. Validates length/count/CRC.
pub fn parse_read_all(frame: &[u8], scale: f64, signed: bool) -> Option<[f64; 9]> {
    if frame.len() != 23 || frame[1] != 0x03 || frame[2] != 18 {
        return None;
    }
    let crc = crc16_modbus(&frame[..21]);
    if frame[21] != (crc & 0xFF) as u8 || frame[22] != (crc >> 8) as u8 {
        return None;
    }
    let mut out = [0.0_f64; 9];
    for (k, slot) in out.iter_mut().enumerate() {
        let raw = ((frame[3 + k * 2] as u16) << 8) | frame[4 + k * 2] as u16;
        *slot = if signed {
            (raw as i16) as f64 * scale
        } else {
            raw as f64 * scale
        };
    }
    Some(out)
}
