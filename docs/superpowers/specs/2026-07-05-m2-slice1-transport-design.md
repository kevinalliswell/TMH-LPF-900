# M2 片1 设计规格：Transport 地基

- 日期：2026-07-05 · 状态：已定稿(用户授权自主推进) · 里程碑：M2 切片 1/4
- 依据：docs/04 §4.5(trait)、docs/08(mock/replay)、docs/03(并发模型)

## 范围
`crates/tmh-core/src/transport/`：`Transport` trait + `BusError` + `DeviceKind`；`MockTransport`(三设备字节级合成 + 故障注入)；`ReplayTransport`(JSONL 回放 + 解析错误计数)；`SerialTransport` 骨架(`#[cfg(feature="serial")]`,默认关,不联机)；`fixtures/dirty-session.jsonl`。

## 决策(全部取推荐)
1. **SerialTransport 用 `serial` feature 门控**,默认关;CI 加 `--features serial` 编译校验(ubuntu 装 libudev-dev)。默认构建/soak 无需 serialport/libudev。
2. **Mock 物理确定性**:朝可设目标一阶趋近、PV 趋近 SV、weight 可设;默认无随机噪声(可选种子 LCG)。完整"实验推断目标"耦合留 M2c/M4。
3. **脏会话 fixture 手工构造**(回声/半帧/坏校验/断线)。

## Transport trait / 错误
```
transact(tx, deadline) -> Result<Vec<u8>, BusError>   // 半双工请求/响应 + 天平 tare 下发
read_line(deadline)    -> Result<Vec<u8>, BusError>   // 天平连续行
reset_buffers()        -> Result<(), BusError>
reopen()               -> Result<(), BusError>
BusError = Timeout | Disconnected | PortInUse | PortNotFound | Io
```

## MockTransport(单类型,`DeviceKind` 分派;工作在"线上整数"域,缩放归 actor)
- **Temp**:9 路 raw u16 + 目标;`transact(read_all)` → 构造合法 18 字节响应帧(`crc16_modbus`);每事务朝目标一阶趋近。
- **Mfc**:4 路线上整数 SV(写入)+ PV(趋近 SV);`transact` 解析 CPL 读/写 → 构造 `<STX>addr00Xnn,value<ETX>ck\r\n` 响应 / 更新 SV。
- **Balance**:weight + tare 结局枚举;`read_line` 合成 `+w.www G S`;`transact(TARE_CMD)` 触发去皮,随后 read_line 依结局回 A00/近零/失败行。
- **`MockFaults { drop_rate, bad_crc_rate, echo, disconnect_after }`**:确定性计数触发。

## ReplayTransport
读 `{"t","dir":"tx|rx","bytes":"hex"}`;`transact` 返回下一条 rx,校对 tx(不一致 warn);`parse_error_count()` 暴露给测试;脏会话跑完不 panic。

## 验收(片1)
- Mock 三设备响应可被 `tmh-protocols` 正确解析;故障四态可触发。
- Replay 脏会话跑完、解析错误计数正确、无 panic。
- `--features serial` 下 SerialTransport 编译通过。
- fmt/clippy/workspace test 全绿 + CI(含 serial 编译校验)。

## 非目标(后续切片)
总线 actor / 健康机 / 重连 / 优先级 / 去皮时序编排(片3);采样/soak(片4);真机联调(硬件门)。
