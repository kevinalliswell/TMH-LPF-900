# M1 协议层 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 `tmh-protocols` 实现三设备协议的纯函数编解码,对 `docs/golden_vectors.json` 26 条金帧逐字节/逐值通过,并满足回声/帧尾/天平分支边界测试、任意输入不 panic、行覆盖 ≥90%。

**Architecture:** 一协议一模块(`modbus.rs`/`cpl.rs`/`balance.rs`),`lib.rs` 保留 M0 的 `crc16_modbus` 并声明模块。测试用**数据驱动金帧 harness**(`tests/golden_vectors.rs`,`include_str!` 直读 canonical JSON),按协议增量 TDD。零运行时依赖(除 `crc`);`serde_json`+`proptest` 仅 dev。

**Tech Stack:** Rust 1.95 / crc / serde_json(dev) / proptest(dev) / cargo-llvm-cov。

**参考:** [spec](../specs/2026-07-05-m1-protocols-design.md);协议 docs/04;金帧 docs/golden_vectors.json。

**工作目录:** 仓库根 `/Users/kevin/ClaudeCoworkSpace/TMH-LPF-900`。所有 crate 命令可加 `-p tmh-protocols`。

**提交 message 末尾都带:**
```
Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
```

---

## Task 1: 加 dev 依赖(serde_json + proptest)

**Files:**
- Modify: `Cargo.toml`(根:`[workspace.dependencies]` 加 serde_json、proptest)
- Modify: `crates/tmh-protocols/Cargo.toml`(加 `[dev-dependencies]`)

- [ ] **Step 1: 根 `Cargo.toml` 的 `[workspace.dependencies]` 末尾追加**

```toml
serde_json = "1"
proptest = "1"
```

- [ ] **Step 2: `crates/tmh-protocols/Cargo.toml` 追加 dev 依赖**

在文件末尾追加:
```toml
[dev-dependencies]
serde_json.workspace = true
proptest.workspace = true
```

- [ ] **Step 3: 验证现有测试仍绿(dev 依赖不影响运行时)**

Run:
```bash
cargo test -p tmh-protocols
```
Expected: 现有 2 条 crc 测试 pass(首次会下载 serde_json/proptest)。

- [ ] **Step 4: 提交**

```bash
git add Cargo.toml Cargo.lock crates/tmh-protocols/Cargo.toml
git commit -m "chore(protocols): add serde_json + proptest dev-dependencies

Golden-vector harness (serde_json) and fuzz-no-panic tests (proptest); dev-only,
runtime deps stay crc-only.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: 温度 Modbus 模块(modbus.rs)+ 金帧 harness(modbus 段)

**Files:**
- Create: `crates/tmh-protocols/tests/golden_vectors.rs`
- Create: `crates/tmh-protocols/src/modbus.rs`
- Modify: `crates/tmh-protocols/src/lib.rs`(加 `pub mod modbus;` + 更新顶部文档)

- [ ] **Step 1: 写金帧 harness(仅 modbus 段)—— TDD 红**

创建 `crates/tmh-protocols/tests/golden_vectors.rs`:
```rust
//! Byte-exact acceptance against docs/golden_vectors.json (M1: all 26 vectors).
use serde_json::Value;
use tmh_protocols::modbus;

const GOLDEN: &str = include_str!("../../../docs/golden_vectors.json");

fn root() -> Value {
    serde_json::from_str(GOLDEN).unwrap()
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
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
    for k in 0..9 {
        let key = format!("T{}", k + 1);
        let e = c["expect"][&key].as_f64().unwrap();
        assert!((got[k] - e).abs() < 1e-9, "{key}: {} vs {}", got[k], e);
    }

    let c = case(&r, sect, "extract_read_all skips echoed request");
    let buf = unhex(c["rx_hex"].as_str().unwrap());
    let frame = modbus::extract_read_all(&buf, 0).expect("frame found");
    assert_eq!(hex(frame), c["expect_frame_hex"].as_str().unwrap());
}
```

- [ ] **Step 2: 跑测试确认红**

Run:
```bash
cargo test -p tmh-protocols --test golden_vectors
```
Expected: 编译失败——`unresolved import` / `modbus` 模块与函数不存在。

- [ ] **Step 3: 写 `crates/tmh-protocols/src/modbus.rs`**

```rust
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
```

- [ ] **Step 4: `lib.rs` 声明模块并更新文档**

把 `crates/tmh-protocols/src/lib.rs` 顶部替换为(保留 `crc16_modbus` 与其测试不动):
```rust
#![forbid(unsafe_code)]
//! Pure frame codecs for TMH-LPF-900 lab devices. Zero I/O, no runtime deps beyond
//! `crc`. Byte-exact against docs/golden_vectors.json (M1). Bus discipline, safety
//! clamp and scaling live in tmh-core (M2); everything here is stateless.

pub mod modbus;

use crc::{Crc, CRC_16_MODBUS};
```
(即在原 `use crc::...` 之前插入 `pub mod modbus;` 与新文档注释;删除旧的两行文档注释。)

- [ ] **Step 5: 跑测试确认绿**

Run:
```bash
cargo test -p tmh-protocols --test golden_vectors && cargo test -p tmh-protocols
```
Expected: `modbus_golden` pass;crc 测试仍 pass。

- [ ] **Step 6: fmt + clippy**

Run:
```bash
cargo fmt -p tmh-protocols && cargo clippy -p tmh-protocols --all-targets -- -D warnings
```
Expected: 无 warning。

- [ ] **Step 7: 提交**

```bash
git add crates/tmh-protocols/src/lib.rs crates/tmh-protocols/src/modbus.rs crates/tmh-protocols/tests/golden_vectors.rs
git commit -m "feat(protocols): Modbus temperature codec (build/extract/parse)

build_read_all, echo-skipping extract_read_all, parse_read_all (9 regs x scale).
Data-driven golden harness (modbus section) passes byte-exact.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: MFC Azbil CPL 模块(cpl.rs)+ harness(cpl 段)

**Files:**
- Create: `crates/tmh-protocols/src/cpl.rs`
- Modify: `crates/tmh-protocols/src/lib.rs`(加 `pub mod cpl;`)
- Modify: `crates/tmh-protocols/tests/golden_vectors.rs`(加 `cpl_golden`)

- [ ] **Step 1: harness 追加 cpl 段 —— TDD 红**

在 `tests/golden_vectors.rs` 顶部 use 改为:
```rust
use tmh_protocols::{cpl, modbus};
```
在文件末尾追加:
```rust
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
```

- [ ] **Step 2: 跑确认红**

Run:
```bash
cargo test -p tmh-protocols --test golden_vectors
```
Expected: 编译失败——`cpl` 模块不存在。

- [ ] **Step 3: 写 `crates/tmh-protocols/src/cpl.rs`**

```rust
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
```

- [ ] **Step 4: `lib.rs` 加 `pub mod cpl;`**

在 `pub mod modbus;` 下一行加:
```rust
pub mod cpl;
```

- [ ] **Step 5: 跑确认绿 + fmt + clippy**

Run:
```bash
cargo test -p tmh-protocols --test golden_vectors && \
cargo fmt -p tmh-protocols && \
cargo clippy -p tmh-protocols --all-targets -- -D warnings
```
Expected: `cpl_golden` + `modbus_golden` pass;无 warning。

- [ ] **Step 6: 提交**

```bash
git add crates/tmh-protocols/src/lib.rs crates/tmh-protocols/src/cpl.rs crates/tmh-protocols/tests/golden_vectors.rs
git commit -m "feat(protocols): Azbil CPL MFC codec (checksum/read/write/parse)

cpl_checksum (two's-complement low byte), build_read/build_write, parse_response
tolerant of frame tails. Golden CPL section passes byte-exact.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: 天平模块(balance.rs)+ harness(balance 段)

**Files:**
- Create: `crates/tmh-protocols/src/balance.rs`
- Modify: `crates/tmh-protocols/src/lib.rs`(加 `pub mod balance;`)
- Modify: `crates/tmh-protocols/tests/golden_vectors.rs`(加 `balance_golden`)

- [ ] **Step 1: harness 追加 balance 段 —— TDD 红**

`tests/golden_vectors.rs` 顶部 use 改为:
```rust
use tmh_protocols::{balance, cpl, modbus};
```
文件末尾追加:
```rust
#[test]
fn balance_golden() {
    let r = root();
    let sect = "balance_rs232";

    let expect = case(&r, sect, "TARE_CMD bytes")["tx_hex"].as_str().unwrap();
    assert_eq!(hex(&balance::TARE_CMD), expect);

    for (name, input, expected) in [
        ("parse_line '+123.456 G S'", "+123.456 G S", Some(123.456)),
        ("parse_line '-0.002 G S'", "-0.002 G S", Some(-0.002)),
        ("parse_line '+123.456 G'", "+123.456 G", None),
        ("parse_line 'garbage'", "garbage", None),
        ("parse_line '   +88.100   G S  '", "   +88.100   G S  ", Some(88.1)),
    ] {
        assert_eq!(case(&r, sect, name)["expect"].as_f64(), expected, "json {name}");
        match (balance::parse_line(input), expected) {
            (Some(g), Some(e)) => assert!((g - e).abs() < 1e-9, "{name}"),
            (g, e) => assert_eq!(g, e, "{name}"),
        }
    }

    for (name, input) in [
        ("parse_tare_response 'A00\\r\\n+0.000 G S'", "A00\r\n+0.000 G S"),
        ("parse_tare_response '+0.001 G S'", "+0.001 G S"),
        ("parse_tare_response '+5.000 G S'", "+5.000 G S"),
        ("parse_tare_response 'no data'", "no data"),
    ] {
        let exp = &case(&r, sect, name)["expect"];
        let got = balance::parse_tare_response(input);
        assert_eq!(got.success, exp["success"].as_bool().unwrap(), "{name} success");
        assert_eq!(got.a00_found, exp["a00_found"].as_bool().unwrap(), "{name} a00");
        match exp["weight"].as_f64() {
            Some(e) => assert!((got.weight.unwrap() - e).abs() < 1e-9, "{name} weight"),
            None => assert_eq!(got.weight, None, "{name} weight"),
        }
    }
}
```

- [ ] **Step 2: 跑确认红**

Run:
```bash
cargo test -p tmh-protocols --test golden_vectors
```
Expected: 编译失败——`balance` 模块不存在。

- [ ] **Step 3: 写 `crates/tmh-protocols/src/balance.rs`**

```rust
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
```

- [ ] **Step 4: `lib.rs` 加 `pub mod balance;`**

在 `pub mod cpl;` 下一行加:
```rust
pub mod balance;
```

- [ ] **Step 5: 跑确认绿 + fmt + clippy**

Run:
```bash
cargo test -p tmh-protocols --test golden_vectors && \
cargo fmt -p tmh-protocols && \
cargo clippy -p tmh-protocols --all-targets -- -D warnings
```
Expected: `modbus_golden`+`cpl_golden`+`balance_golden` 全 pass(26 金帧全绿);无 warning。

- [ ] **Step 6: 提交**

```bash
git add crates/tmh-protocols/src/lib.rs crates/tmh-protocols/src/balance.rs crates/tmh-protocols/tests/golden_vectors.rs
git commit -m "feat(protocols): balance RS232 codec (parse_line/tare) — 26 golden green

TARE_CMD, parse_line (hand-rolled signed-decimal scan, no regex), parse_tare_response
(A00 / near-zero priority). All 26 golden vectors now pass byte-exact.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 5: 补充边界测试 + proptest 不 panic

**Files:**
- Create: `crates/tmh-protocols/tests/edge_cases.rs`

- [ ] **Step 1: 写 `crates/tmh-protocols/tests/edge_cases.rs`**

```rust
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
    assert_eq!(frame[2], 18, "must skip the echoed request (bc=0) and take the response");
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
```

- [ ] **Step 2: 跑确认全绿**

Run:
```bash
cargo test -p tmh-protocols
```
Expected: golden_vectors(3 tests)+ edge_cases(5 单测 + 2 proptest)全 pass。

- [ ] **Step 3: fmt + clippy**

Run:
```bash
cargo fmt -p tmh-protocols && cargo clippy -p tmh-protocols --all-targets -- -D warnings
```
Expected: 无 warning。

- [ ] **Step 4: 提交**

```bash
git add crates/tmh-protocols/tests/edge_cases.rs
git commit -m "test(protocols): edge cases (echo/tail/tare boundary) + proptest no-panic

Pure-noise/half-frame/multi-frame echo-skip, CPL tail variants (crlf/cr/none),
tare 0.15 boundary; proptest fuzz asserts no parser panics on arbitrary input.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 6: 覆盖率 ≥90%(cargo-llvm-cov)

**Files:** 可能 Modify: `crates/tmh-protocols/tests/edge_cases.rs`(补齐未覆盖分支)

- [ ] **Step 1: 装覆盖率工具**

Run:
```bash
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov --locked
```
Expected: 安装成功(cargo-llvm-cov 编译需数分钟)。

- [ ] **Step 2: 量测 protocols 行覆盖**

Run:
```bash
cargo llvm-cov -p tmh-protocols --summary-only
```
Expected: 表格输出;记下 `tmh-protocols` 的 Lines 覆盖率。

- [ ] **Step 3: 若 <90%,补测未覆盖分支**

查看逐行:
```bash
cargo llvm-cov -p tmh-protocols --show-missing-lines
```
针对未覆盖行补断言到 `tests/edge_cases.rs`。常见缺口:`parse_read_all` 的 `signed=true` 分支、非法长度/坏 CRC 返回 `None`、`parse_response` 空/无 STX、`scan_signed_decimal` 的无整数位/无小数位早退。示例补测:
```rust
#[test]
fn parse_read_all_rejects_bad_frames() {
    assert_eq!(modbus::parse_read_all(&[0u8; 10], 0.1, false), None); // wrong length
    let mut f = unhex("00 03 12 09 c4 0b bd 11 94 23 1f 23 28 23 32 00 fa 01 31 23 27 a9 7a");
    f[22] ^= 0xFF; // corrupt CRC
    assert_eq!(modbus::parse_read_all(&f, 0.1, false), None);
}

#[test]
fn parse_read_all_signed_negative() {
    // register 0xFFFF as signed = -1 -> -0.1 at scale 0.1
    let mut f = unhex("00 03 12 09 c4 0b bd 11 94 23 1f 23 28 23 32 00 fa 01 31 23 27 a9 7a");
    f[3] = 0xFF;
    f[4] = 0xFF;
    // recompute CRC so the frame validates
    let crc = tmh_protocols::crc16_modbus(&f[..21]);
    f[21] = (crc & 0xFF) as u8;
    f[22] = (crc >> 8) as u8;
    let got = modbus::parse_read_all(&f, 0.1, true).unwrap();
    assert!((got[0] - -0.1).abs() < 1e-9);
}
```
重跑 Step 2 直到 Lines ≥ 90%。

- [ ] **Step 4: fmt + clippy + 提交(若补了测试)**

Run:
```bash
cargo fmt -p tmh-protocols && \
cargo clippy -p tmh-protocols --all-targets -- -D warnings && \
cargo test -p tmh-protocols
```
Then（若有改动）:
```bash
git add crates/tmh-protocols/tests/edge_cases.rs
git commit -m "test(protocols): cover signed parse + rejection paths (>=90% lines)

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 7: 收尾(CHANGELOG + 全门禁 + 推送 + CI)

**Files:**
- Modify: `CHANGELOG.md`

- [ ] **Step 1: CHANGELOG `[Unreleased]` 加 M1 条目**

把 `## [Unreleased]` 段替换为:
```markdown
## [Unreleased]

### Added
- `tmh-protocols`: byte-exact codecs for the three device protocols (Modbus-RTU
  temperature, Azbil CPL MFC, RS232 balance), verified against all 26 golden
  vectors, with echo-skip / frame-tail / tare-branch edge tests and proptest
  fuzzing. Runtime deps stay crc-only.
```

- [ ] **Step 2: 全 workspace 门禁(与 CI 一致)**

Run:
```bash
cargo fmt --all --check && \
cargo clippy --workspace --all-targets -- -D warnings && \
cargo test --workspace && \
pnpm build >/dev/null 2>&1 && echo "frontend dist ok"
```
Expected: 全绿(前端未变,dist 供 src-tauri context)。

- [ ] **Step 3: 依赖方向复核(protocols 仍仅 crc)**

Run:
```bash
cargo tree -p tmh-protocols -e normal | grep -viE 'tmh-protocols|crc' | grep -iE 'serde|proptest|regex' && echo "UNEXPECTED runtime dep" || echo "runtime deps clean (crc only)"
```
Expected: `runtime deps clean (crc only)`(serde_json/proptest 只在 dev,`-e normal` 不显示)。

- [ ] **Step 4: 提交并推送**

```bash
git add CHANGELOG.md
git commit -m "docs(changelog): record M1 protocol layer

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
git push origin main
```

- [ ] **Step 5: 观察 CI 全绿**

Run:
```bash
gh run watch "$(gh run list --workflow=ci.yml --limit 1 --json databaseId --jq '.[0].databaseId')" --exit-status
```
Expected: ubuntu + windows 全绿。若红,读日志修复(注意:金帧 harness 依赖 `../../../docs/golden_vectors.json` 路径与 LF 换行,已被 .gitattributes 覆盖)。

---

## 附:M1 验收 → 任务映射(自查覆盖)

| M1 验收(docs/10) | 由哪个 Task 落地 |
|---|---|
| 26 金帧逐字节/逐值通过 | Task 2(modbus 5)+ Task 3(cpl 11)+ Task 4(balance 10) |
| 回声跳过(纯噪声/半帧/多帧) | Task 5 |
| CPL 帧尾变体(\r\n/\r/无尾) | Task 5 |
| 天平三分支(A00/近零/失败) | Task 4 金帧 + Task 5 边界 |
| 随机字节输入不 panic | Task 5 proptest |
| 行覆盖 ≥90% | Task 6 |
| fmt/clippy/workspace test 绿 + CI | Task 7 |
