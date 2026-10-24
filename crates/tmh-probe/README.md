# tmh-probe — 真机串口通讯探针

无 GUI、无需完整应用,直接用**金标准编解码器**逐台读真实设备,打印读事务原始帧；写事务经actor确认，原始帧用`--record`留存,
并可录成 JSONL 会话;录下的会话能在开发机上**回放复盘**(与实际设备字节/时序一致)。
读为默认。`mfc --set` 仅允许 N2 0–5 L/min，或任意气路设0；H2/CO/CO2正流量在现场协议合格前禁止。写入经MFC actor统一量化和完整ACK校验，空帧/错帧/超时不会显示成功。

## 拿到 exe(工控机不用装工具链)

GitHub → Actions → **Probe (Windows exe)** → Run workflow → 下载 `tmh-probe-windows` 工件,
里面是 `tmh-probe.exe`,U 盘拷到设备电脑即可。(有 Rust 工具链的话也可 `cargo run -p tmh-probe -- …`。)

## 用法

```powershell
# 1) 先看有哪些串口
tmh-probe.exe list

# 2) 温度控制器(Modbus-RTU,9 通道)——同时录会话发我复盘
tmh-probe.exe temp --port COM3 --baud 9600 --addr 1 --scale 0.1 --seconds 20 --record temp.jsonl
#   参数不对就试 --parity even / --signed / 改 --addr;每行会打印 RX 原始字节 + T1..T9 + T8

# 3) MFC(Azbil CPL,4 路)——默认只读 PV,不通气
tmh-probe.exe mfc --port COM4 --parity even --addrs 1,2,3,4 --scaling 0.1,1.0,0.1,0.1 --seconds 20 --record mfc.jsonl
#   仅在确认安全后写设定值(会通气,先只用 N2):
tmh-probe.exe mfc --port COM4 --parity even --set 1:5.0        # gas 1=N2 → 5.0 L/min

# 4) 电子天平(RS232)
tmh-probe.exe balance --port COM5 --baud 1200 --seconds 15 --record bal.jsonl
tmh-probe.exe balance --port COM5 --baud 1200 --tare          # 先去皮再读

# 5) 回放(开发机上,无需硬件)——把上面录的 *.jsonl 发我,我这样复盘:
tmh-probe.exe temp --replay temp.jsonl --addr 1 --scale 0.1
```

## 配合流程

1. 你在设备电脑上按上面命令逐台跑,**把控制台输出 + 录下的 `*.jsonl` 发我**。
2. 我在本机 `--replay` 你的会话,复现 CRC/解析/超时问题、改码,并把会话固化成回归用例。
3. 三台都读通后,再把串口 hub 接进完整应用,出安装包上真机。

## 参数速查

| 通用 | 说明 |
|---|---|
| `--port` / `--replay` | 二选一:真实串口 / 回放文件 |
| `--baud --parity --databits --stopbits --timeout-ms` | 串口线参数(默认 9600 8N1 / 500ms) |
| `--interval-ms --seconds` | 轮询间隔 / 时长(真机;回放忽略) |
| `--record <file>` | 录制原始收发为 JSONL |

安全:MFC先只读；仅N2允许受限调试正流量，所有气路0设定仍可请求关闭。完整应用/核心actor的H₂/CO编译期绝对上限为5 L/min，现场配置只能更低；当前桌面real模式仍禁用。原始写帧请用--record留存；仅软件ACK校验不能代替厂家协议核对和现场确认。
