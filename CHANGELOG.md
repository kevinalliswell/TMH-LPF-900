# Changelog

All notable changes to this project are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versioning follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- 整合 #38 / #39 及配套修复：统一账户、实验归属、样品资料、程序快照、管理员模式编辑和实际操作者审计。
- 自定义程序按国标模板派生，保存修改原因、不可变版本与制度差异；自定义还原终点单独计算并标识 Rₜ。
- 检验重复组支持同批确认、材料及限值依据、精确十进制修约、待补测决策、采用/排除原因和只追加版本。
- 单次及组报告支持人工补录；组报告冻结当时的原始证据、分析与过程核查版本，可导出离线 HTML / JSON 并打印为 PDF。
- 全过程记录核查覆盖阶段归属、数据质量、温度/PV 范围、SV 一致性、阶段时间、连续稳定、冷却及人工确认；不自动宣称整个方法符合国标。
- 独立候选构建与数据目录，禁用候选自动更新，生成提交与 SHA-256 清单。Windows 安装/启动/关闭/数据库/卸载及进程隔离冒烟，macOS 双架构与包完整性检查。

### Fixed
- 气体开启恢复本次运行中已获设备确认的非零设定；无恢复值时明确拒绝。全开逐路报告部分失败并审计实际生效值。
- 历史页随实验状态及保存结果更新列表和详情，迟到的旧详情响应不覆盖新选择。
- 实验采样保留固定时点，避免正常定时抖动漏采并误判 RI 终点缺测；阶段切换与长停顿不补造采样。
- 修复程序首次加载后立即复制时自定义草稿被初始化逻辑覆盖；切换和删除明确更新编辑状态，保留已有版本历史。
- 按用户提供的正式标准修正 RI 亚铁修正与称量基准、RVI 的 t30/t60 计算、RDI 筛分质量定义、FSI 整份 18 颗总体积变化及重复性规则。
- 国标流程使用连续恒温、气氛事务确认、固定还原/吹扫时长、严格冷却阈值及人工步骤；正常结束和故障保护分别处理。
- MFC 校验完整应答帧、校验和、地址、整数值和回显；紧急命令优先、故障锁存及代次号阻止旧排队指令重新开气。
- 实验启动、推进和保护复位检查设备数据新鲜度；无效温度、称量和流量不以零占位参与分析或绘图。
- SQLite schema 8：末批采样与运行终态原子保存，落盘失败可重试，持久恢复日志在重启时恢复；保留分析、补录、自定义和检验组版本。
- IPC 参数 snake_case、认证锁定计数、权限边界及实际操作者审计；跨用户组修订不能隐式分享成员证据。
- 组成员按绝对开始时间排序；旧无时区时间不能猜测为有效重复试验。保存后重分析或删除原实验不改写冻结证据。
- 检验组未保存时阻止页面切换和导出，防止草稿丢失或导出旧预览。
- 未实现的 real/replay 模式和无效现场配置明确拒绝启动，不再静默生成模拟数据。

### Known limitations
- 此批修复面向模拟软件与安装资产测试，尚无真实设备、实际国标实验或完整现场验收。
- 温控仪参数写入、MFC/天平实际协议与测点映射需以型号手册核对；通讯配置界面写回和总线热重启仍未完成。
- RDI 和天然铁矿石 RI 的重复性限值需要人工录入并给出依据；自动判断实验室全部外部操作与采购性能不在当前范围。
- 候选安装包仍显示源码版本 0.2.0，须用提交与哈希区分已发布旧包。没有 Windows 发布者证书或 Apple 公证；不发布到正式更新通道。
- 浏览器预览不具备原生检验组数据库。桌面模拟、核心测试及构建冒烟均不能代替完整人工操作和实机安全验收。

## [0.2.0] - 2026-08-31

### Security
- MFC 写命令现在等待并校验设备事务回执；超时、断线、空响应或错误回显不再被审计或提示为成功。
- Webview 不再直接拥有 updater/process 高权限 capability，Tauri 启用最小 CSP，更新与重启仅通过受控 Rust 命令。
- ECharts 升级至 6.1.0，CI 增加生产依赖审计和 Tauri 安全配置回归检查。

### Fixed
- 保护气氛、紧急切断、实验停止/完成、崩溃恢复及应用退出路径会等待 MFC 确认，并在失败时进入可观察错误状态。
- 四路气氛批处理在单路失败后仍尝试其余通道，避免局部故障阻断其它可燃气归零操作。

### Changed
- Node 固定为 24 LTS、Rust 固定为 1.95.0；版本同步覆盖 package、Cargo、Tauri 和 UI 显示点。
- 正式 Release 仅允许由指向当前 `main` 顶端的匹配 SemVer Tag 触发，并先生成草稿供人工核验。
- 增加 Dependabot、版本治理路线、回滚要求和现场验收退出条件。

### Known limitations
- 本版本仅作为 mock/安全控制面基线；real/replay 仍未完成真实传输与配置 fail-closed 验收（#19），不得宣称真实硬件可用。
- MFC “ON/全开”在没有可恢复非零设定值时可能确认写入 0（#28）；修复前不得用于真实设备操作。
- 完整命令边界校验、管理员删除授权和稳定错误 DTO 仍按 #21 的后续路线推进。
- Windows 安装包尚无 Authenticode；SmartScreen 可能显示“未知发布者”。更新端点仍需部署到匿名可达的公开托管位置。

### Added
- `tmh-protocols`: byte-exact codecs for the three device protocols (Modbus-RTU
  temperature, Azbil CPL MFC, RS232 balance), verified against all 26 golden
  vectors, with echo-skip / frame-tail / tare-branch edge tests and proptest
  fuzzing. Line coverage 99%. Runtime deps stay crc-only.
- `tmh-core` transport layer (M2 slice 1): `Transport` trait + `MockTransport`
  (synthetic, deterministic fault injection) + `ReplayTransport` (recorded sessions,
  dirty-session fixture) + feature-gated `SerialTransport` skeleton.
- `tmh-core` config/audit/storage (M2 slice 2): comm_config.json load + validate +
  atomic write-back; JSON audit log (date-rotated, never throws into control flow);
  rusqlite (bundled, WAL) persistence with schema migrations.
- `tmh-core` bus core (M2 slice 3): safety clamp (single choke point, compile-time
  absolute ceiling, zero always passes), health/reconnect state machine (5-fail
  offline, [1,2,5,10,30]s backoff, staleness), and temperature/MFC/balance actors
  (clamp integration, set-preempts-poll priority, tare sequence). Deterministic unit
  tests against MockTransport.
- `tmh-core` telemetry/sampler/soak (M2 slice 4): 0.2s telemetry aggregation; 1 Hz
  sampler (balance-offline rows written with weight=0 + annotation, never dropped);
  `examples/soak` headless fast-forward of the full pipeline (actors → aggregate →
  sample → rusqlite) plus dirty-session replay, wired into CI. M2 core complete.
- **M3 应用壳 + 综合控制页**: `DeviceHub` runs the bus actors on OS threads and bridges
  to tokio (watch snapshots + oneshot-reply commands). src-tauri exposes typed commands
  (set_gas_flow w/ clamp+audit+event, on/off, open/close all, shutoff_flammable, tare,
  start/stop_experiment, device health) + a 0.2 Hz telemetry Channel; single-instance
  focuses the first window and audits `start_blocked`. Live 综合控制页 (React + zustand +
  ECharts): 9 temperature tiles (T8 sample-highlighted), weight, 4 gas PV/SV cards, gas
  control with clamp toast, one-click all-open/close + red cut-flammable, tare, real-time
  temperature curve (ring buffer, monotonic axis), device-dot status bar. Start/stop
  experiment dialog (M3 manual atmosphere; safety atmosphere N₂=5/flammables=0 on stop),
  error-code→Chinese toast, disconnect greying. Browser mock via `isTauri()` for dev.
- **M4 实验引擎 + 国标分析 + 报告导出**: `tmh-core::gb` national-standard formulas
  (GB/T 13241 reducibility Rt + reduction-rate index, 13242 RDI, 13240 free-swelling),
  golden-tested (Rt 100/85/30 → 50.0). `tmh-core::engine` phase state machine, the three
  GB program tables, and stage-advancement rules (holds need temp+duration; cooling at
  T≤target+5). The live engine runs as a per-run tick task in src-tauri: reads T8, ticks
  the engine (sim-time accelerated ~180× in mock), applies each stage's atmosphere through
  the safety clamp, drives the mock furnace/balance (reduction loses mass), buffers samples
  to SQLite, and applies the safety atmosphere on EVERY exit — completion, operator stop,
  and startup crash recovery (any run left open). Commands: skip_stage, list_programs,
  list/detail/samples (decimated)/delete, analyze_gb1324{0,1,2} (persist analysis JSON),
  generate_report / export_experiment. `tmh-core::report`: CSV/TSV (BOM, Excel-ready),
  XLSX (rust_xlsxwriter), and a self-contained printable HTML report with metadata, GB
  result table, reduction-stage condition checks, and an inline SVG temp/mass chart;
  written to exports/ and opened via tauri-plugin-opener. Frontend: sidebar navigation
  across seven pages — dashboard, 综合控制 (now with a live stage stepper + skip),
  实验模式 (program tables), 历史查询 (list + replay chart + GB analysis forms + report/
  export + delete), 通讯设置, 帮助, 关于. Browser mock runs the full stage engine with a
  seeded in-memory history so `pnpm dev:web` is a faithful end-to-end demo.
- **M5 打包发布 + 软件更新**: NSIS offline installer (embedded WebView2, perMachine).
  `tauri-plugin-updater` + `tauri-plugin-process` with a minisign-signed update channel
  (public key baked into `tauri.conf.json`, `createUpdaterArtifacts` on). Update checking
  is OFF by default (docs/09 §9.3), enabled via `TMH_UPDATER`; gated commands
  get_updater_status / check_for_update / install_update audit + emit `update-status` and
  turn an unreachable manifest endpoint into a friendly offline message. About page gains a
  软件更新 card (version, on/off, check, download+install+restart). `TMH_DISABLE_GPU`
  escape hatch for low-end integrated graphics. Release workflow signs updater artifacts +
  `latest.json` (minisign private key in CI secret) and drafts NSIS + macOS bundles; tag
  vs `package.json` version verified. Delivery doc docs/11 (release steps, secrets, update
  endpoint hosting, Authenticode enablement, M5 acceptance checklist, field SOP).

## [0.1.0] - 2026-07-05

### Added
- Repository scaffold: Cargo workspace (`tmh-protocols`, `tmh-core`, `src-tauri`)
  and a Vite + React 19 + TypeScript (strict) frontend.
- Dark empty-shell Tauri window titled `TMH-LPF-900 v0.1.0`.
- Single-source version management: `package.json` -> `scripts/sync-version.mjs`
  (tauri.conf.json, index.html, App.tsx), with a `--check` CI guard.
- CI matrix (Ubuntu + Windows): fmt, clippy `-D warnings`, workspace tests,
  typecheck, lint, dependency-direction guard, version-consistency check.
- Release pipeline producing unsigned Windows NSIS and macOS universal `.dmg`.

[Unreleased]: https://github.com/kevinalliswell/TMH-LPF-900/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/kevinalliswell/TMH-LPF-900/releases/tag/v0.2.0
[0.1.0]: https://github.com/kevinalliswell/TMH-LPF-900/tree/4b88741
