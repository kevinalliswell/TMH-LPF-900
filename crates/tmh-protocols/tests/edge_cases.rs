//! M1 edge cases: echo-skip variants, CPL frame-tail tolerance, tare boundary,
//! and proptest fuzz (arbitrary input must never panic).
use proptest::prelude::*;
use tmh_protocols::{balance, cpl, modbus};

fn unhex(s: &str) -> Vec<u8> {
    s.split_whitespace()
        .map(|h| u8::from_str_radix(h, 16).unwrap())
        .collect()
}

#[test]
fn echo_skip_pure_noise_is_none() {
    assert_eq!(modbus::extract_read_all(&[0xAA; 16], 0), None);
    assert_eq!(modbus::extract_read_all(&[], 0), None);
}

#[test]
fn echo_skip_half_frame_is_none() {
    // header claims byte_count=0x12 but buffer is truncated
    let buf = [0x00, 0x03, 0x12, 0x09, 0xc4];
    assert_eq!(modbus::extract_read_all(&buf, 0), None);
}

#[test]
fn echo_skip_multi_frame_picks_real_response() {
    let buf = unhex(
        "00 03 00 00 00 09 84 1d 00 03 12 09 c4 0b bd 11 94 23 1f 23 28 23 32 00 fa 01 31 23 27 a9 7a",
    );
    let frame = modbus::extract_read_all(&buf, 0).unwrap();
    assert_eq!(
        frame[2], 18,
        "must skip the echoed request (bc=0) and take the response"
    );
    assert_eq!(frame.len(), 23);
}

#[test]
fn cpl_tail_variants_parse_identically() {
    let crlf = unhex("02 30 31 30 30 58 32 30 2c 33 30 03 46 33 0d 0a");
    let cr = unhex("02 30 31 30 30 58 32 30 2c 33 30 03 46 33 0d");
    let bare = unhex("02 30 31 30 30 58 32 30 2c 33 30 03");
    assert_eq!(cpl::parse_response(&crlf), Some(3.0));
    assert_eq!(cpl::parse_response(&cr), Some(3.0));
    assert_eq!(cpl::parse_response(&bare), Some(3.0));
}

#[test]
fn tare_boundary_015_is_failure() {
    // |w| == 0.15 is NOT < 0.15 -> failure
    let r = balance::parse_tare_response("+0.150 G S");
    assert!(!r.success);
    assert!((r.weight.unwrap() - 0.15).abs() < 1e-9);
}

proptest! {
    #[test]
    fn no_panic_on_arbitrary_bytes(buf in proptest::collection::vec(any::<u8>(), 0..64)) {
        let _ = modbus::extract_read_all(&buf, 0);
        let _ = modbus::parse_read_all(&buf, 0.1, false);
        let _ = cpl::parse_response(&buf);
    }

    #[test]
    fn no_panic_on_arbitrary_text(s in ".{0,64}") {
        let _ = balance::parse_line(&s);
        let _ = balance::parse_tare_response(&s);
    }
}
