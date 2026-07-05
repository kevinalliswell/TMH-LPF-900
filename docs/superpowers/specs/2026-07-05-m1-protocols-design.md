# M1 协议层设计规格（tmh-protocols）

- 日期：2026-07-05
- 状态：已定稿（用户已批准）
- 里程碑：M1（docs/10 §M1）
- 依据：docs/04-通讯协议规范.md + `docs/golden_vectors.json`（字节级硬验收，26 条）
- 下游：交由 writing-plans 产出 TDD 实施计划

---

## 1. 目标与范围

在 `tmh-protocols` crate 实现三设备协议的**纯函数编解码**：温度 Modbus-RTU、MFC Azbil CPL、天平 RS232 行协议。零 I/O、零运行时依赖（除 `crc`）。26 条金帧向量逐字节/逐值通过。

### 非目标（M2 tmh-core）
`Transport` trait、总线纪律（轮询/失败计数/重连退避/watchdog/优先级队列）、安全钳制、`×FLOW_SCALING` 缩放链、去皮命令时序编排、健康状态机。M1 只做**无状态纯函数**。

---

## 2. 验收标准（docs/10 §M1）

- [ ] `docs/golden_vectors.json` **26 条全部逐字节/逐值通过**（构帧 hex 相等；解析结果相等）。
- [ ] 补充单元测试：回声跳过（纯噪声 / 半帧 / 多帧）、CPL 帧尾变体（`\r\n` / `\r` / 无尾）、天平三分支（A00 / 近零 / 失败）；任意字节输入不 panic。
- [ ] 覆盖率：protocols crate 行覆盖 **≥ 90%**（`cargo-llvm-cov` 量测）。
- [ ] `cargo test --workspace`、clippy `-D warnings`、fmt 全绿（沿用 M0 门禁）。

---

## 3. 模块结构

```
crates/tmh-protocols/
├─ Cargo.toml            # [dependencies] crc；[dev-dependencies] serde_json, proptest
└─ src/
   ├─ lib.rs             # mod 声明 + pub re-export + crc16_modbus（M0 已有，保留在此）
   ├─ modbus.rs          # 温度
   ├─ cpl.rs             # MFC（Azbil CPL）
   └─ balance.rs         # 天平
   tests/
   ├─ golden_vectors.rs  # 数据驱动：include_str! docs/golden_vectors.json，逐条断言
   └─ edge_cases.rs      # 回声/帧尾/天平分支/proptest 不 panic
```

每协议一个模块，单一职责、可独立测试。`crc16_modbus`（M0 已实现）留在 `lib.rs`，`modbus`/`cpl` 复用。

---

## 4. 公开 API 与算法

### 4.1 modbus.rs（温度，Modbus-RTU）

```rust
pub fn build_read_all(addr: u8) -> Vec<u8>;
pub fn extract_read_all(buf: &[u8], addr: u8) -> Option<&[u8]>;
pub fn parse_read_all(frame: &[u8], scale: f64, signed: bool) -> Option<[f64; 9]>;
```

- **build_read_all**：`body=[addr,0x03,0x00,0x00,0x00,0x09]`；`crc=crc16_modbus(&body)`；追加 `crc_lo=(crc&0xFF)`、`crc_hi=(crc>>8)`。共 8 字节。
  金帧：addr=0→`00 03 00 00 00 09 84 1d`；addr=1→`01 03 00 00 00 09 85 cc`；addr=3→`03 03 00 00 00 09 84 2e`。
- **extract_read_all**（回声跳过，docs/04 §4.1）：在 `buf` 上滑动扫描；对每个 `i`：若 `buf[i]==addr && buf[i+1]==0x03`，取 `bc=buf[i+2]`；`bc==0` → 跳过（回显请求此位为 0x00）；`end=i+3+bc+2`；越界 → 跳过；`crc16_modbus(&buf[i..end-2])` 的小端等于 `buf[end-2..end]` → 返回 `Some(&buf[i..end])`。全程无匹配 → `None`（= 本轮读取失败，非"空数据成功"）。
  金帧 `extract_read_all skips echoed request`：8 字节回显请求 + 23 字节真实响应 → 提取出 23 字节真实响应。
- **parse_read_all**：校验 `frame[1]==0x03`、`bc=frame[2]==18`、`frame.len()==3+18+2==23`、CRC 有效；否则 `None`。取 `frame[3..21]` 的 9 个大端 `u16`；`signed==false` → `u16 as f64 * scale`；`signed==true` → `(u16 as i16) as f64 * scale`。返回 `[f64;9]`。
  金帧：scale=0.1，`00 03 12 09c4 0bbd 1194 231f 2328 2332 00fa 0131 2327 <crc>` → T1..T9 = 250.0, 300.5, 450.0, 899.1, 900.0, 901.0, 25.0, 30.5, 899.9000000000001（注：`8999*0.1` 的 f64，测试用 1e-9 容差）。

### 4.2 cpl.rs（MFC，Azbil CPL ASCII）

```rust
pub fn cpl_checksum(stx_to_etx: &[u8]) -> [u8; 2];
pub fn build_read(addr: u8, reg: u16, n: u8) -> Vec<u8>;
pub fn build_write(addr: u8, reg: u16, values: &[i64]) -> Vec<u8>;
pub fn parse_response(buf: &[u8]) -> Option<f64>;
```

- **cpl_checksum**：入参为 **STX..ETX（含两端）** 的字节；`sum = Σ bytes`（u32/wrapping）；`low = sum & 0xFF`；`ck = (0u8).wrapping_sub(low as u8)`（低字节二补数）；格式化为 **2 位大写十六进制 ASCII** 返回 `[u8;2]`。
  验证：`read PV H2`（STX..ETX 求和 = 0x36D，low=0x6D，二补数=0x93）→ `"93"` = `[0x39,0x33]`。
- **build_read**：`STX(0x02)` + `addr` 的 2 位大写 hex ASCII + 字面 `"00XRS,"` + `reg` 十进制 ASCII + `"W,"` + `n` 十进制 ASCII + `ETX(0x03)`；对该 STX..ETX 段算 `cpl_checksum` 追加 2 字节；再追加 `"\r\n"`（0x0D 0x0A）。
  金帧：`read PV H2 addr=1 reg=1206 n=2` → `\x020100XRS,1206W,2\x0393\r\n`。
- **build_write**：同上，字面改 `"00XWS,"`；`values` 以 `,` 连接的十进制 ASCII（支持多值 `v1[,v2..]`）。
  金帧：`write SV H2 raw300`（addr=1,reg=1401,values=[300]）→ `\x020100XWS,1401W,300\x0330\r\n`。
- **parse_response**（容忍真实帧尾，docs/04 §4.2）：找首个 `STX`，无 → `None`；取 STX 之后到首个 `ETX` 之前的字节（无 ETX 则取到末尾）为 content；按 `,` 分割，取**第 2 段**，`trim` 后 `parse::<f64>()`，成功则 `/ 10.0` 返回；字段不足或解析失败 → `None`。
  金帧：`\x020100X20,30\x03F3\r\n`→3.0；`\x020500X00,52\x03EB\r\n`→5.2；`\x02OK,1234\x03`→123.4；空→`None`；`Invalid data`（无 STX）→`None`。
  注：`×FLOW_SCALING` 由 tmh-core 施加，本层只做 `/10`。

### 4.3 balance.rs（天平，RS232 行协议）

```rust
pub const TARE_CMD: [u8; 4] = [0x54, 0x20, 0x0D, 0x0A]; // "T \r\n"
pub fn parse_line(line: &str) -> Option<f64>;
pub fn parse_tare_response(buf: &str) -> TareResult;

pub struct TareResult { pub success: bool, pub a00_found: bool, pub weight: Option<f64> }
```

- **parse_line**：若 `!line.contains("G S")` → `None`；否则**手写扫描**首个 `[+-]\d+\.\d+`（符号 + 一位以上数字 + `.` + 一位以上数字），`parse::<f64>()` 返回；无匹配 → `None`。**不引 `regex` crate**。
  金帧：`+123.456 G S`→123.456；`-0.002 G S`→-0.002；`+123.456 G`（无 "G S"）→`None`；`garbage`→`None`；`   +88.100   G S  `→88.1。
- **parse_tare_response**（判定优先级，docs/04 §4.3）：
  ```
  a00 = buf.contains("A00")
  w   = parse_line(buf)                       // Option<f64>
  success = a00 || (w.map_or(false, |v| v.abs() < 0.15))
  TareResult { success, a00_found: a00, weight: w }
  ```
  金帧：`A00\r\n+0.000 G S`→(true,true,0.0)；`+0.001 G S`→(true,false,0.001)；`+5.000 G S`→(false,false,5.0)；`no data`→(false,false,None)。

---

## 5. 测试策略

### 5.1 金帧数据驱动（tests/golden_vectors.rs）
`include_str!("../../../docs/golden_vectors.json")`（单一来源，不拷贝到 fixtures/）+ `serde_json`（dev-dep）解析。遍历三 section 逐 case 断言：
- 构帧类：`hex(build_*) == tx_hex`（去空格比较）。
- `parse_read_all`：9 值逐个 `≈`（1e-9 容差）。
- `extract_read_all`：`hex(extract) == expect_frame_hex`。
- `parse_response` / `parse_line` / `parse_tare_response`：值/结构相等（`null`→`None`）。

### 5.2 补充单测（tests/edge_cases.rs）
- **回声跳过**：纯噪声（`[0xAA;16]`→`None`）、半帧（截断的 0x03 帧→`None`）、多帧（回显+响应→取真实响应；两真实帧→取第一个 CRC 有效帧）。
- **CPL 帧尾变体**：`...ETX ck \r\n` / `...ETX ck \r` / `...ETX ck`（无尾）三者 `parse_response` 结果一致。
- **天平三分支**：A00 / 近零(<0.15) / 失败——金帧已覆盖，另加边界 `|w|==0.15`（不 <0.15 → 失败）。
- **proptest**（dev-dep）：对每个 parser 喂任意 `Vec<u8>` / `String`，断言不 panic（`extract_read_all` / `parse_read_all` / `parse_response` / `parse_line` / `parse_tare_response`）。

### 5.3 覆盖率
`rustup component add llvm-tools-preview` + `cargo install cargo-llvm-cov`；`cargo llvm-cov -p tmh-protocols`（行覆盖 ≥90%）。M1 本地量测并报数；CI 覆盖门为可选（不阻塞 M1）。

---

## 6. 依赖

- **运行时**：`crc`（唯一，M0 workspace.dependencies 已声明）。
- **dev**：`serde_json`（金帧 JSON）、`proptest`（随机字节不 panic）。二者加入 `[workspace.dependencies]`，在 `tmh-protocols/[dev-dependencies]` 引用。
- 不引 `regex`、不引 `serde`（运行时）——守住"零依赖除 crc"。

---

## 7. 风险与应对

- **f64 精度**：金帧温度含 `899.9000000000001` 这类 f64 尾数；`raw as f64 * scale` 与之同源，1e-9 容差稳过。
- **include_str! 路径**：`tests/` 距仓库根 3 级 → `../../../docs/golden_vectors.json`。
- **手写数字扫描**：需处理无符号数字（如 `A00`、`G`）不误匹配（模式强制前导 `+`/`-`）；`88.100`→`88.1`（f64）。
- **CRC 小端**：`build` 追加与 `extract` 校验都按 lo,hi；已在 M0 `crc16_modbus` 基础上验证金帧。
