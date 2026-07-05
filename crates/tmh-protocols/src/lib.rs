#![forbid(unsafe_code)]
//! Pure frame codecs for TMH-LPF-900 lab devices. Zero I/O, no deps beyond `crc`.
//! M0: CRC-16/MODBUS helper only; full device codecs land in M1 (golden-vector tested).

use crc::{Crc, CRC_16_MODBUS};

/// CRC-16/MODBUS checksum over `data` (poly 0x8005, init 0xFFFF, reflected).
///
/// Returned as a `u16`; Modbus-RTU frames carry it low-byte-first on the wire.
pub fn crc16_modbus(data: &[u8]) -> u16 {
    const MODBUS: Crc<u16> = Crc::<u16>::new(&CRC_16_MODBUS);
    MODBUS.checksum(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc16_modbus_standard_check_value() {
        // The canonical CRC-16/MODBUS check value for b"123456789" is 0x4B37.
        assert_eq!(crc16_modbus(b"123456789"), 0x4B37);
    }

    #[test]
    fn crc16_modbus_empty_is_init() {
        // MODBUS init register is 0xFFFF; empty input yields the init value.
        assert_eq!(crc16_modbus(&[]), 0xFFFF);
    }
}
