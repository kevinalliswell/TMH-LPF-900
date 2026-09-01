# Changelog

All notable changes to this project are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versioning follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- 首次启动创建管理员、用户名/密码登录、临时密码强制修改、空闲锁定和管理员用户管理。
- SQLite schema v2 用户表与实验 owner；普通用户实验数据隔离，管理员可管理全部数据及旧数据归属。
- `scripts/check-ipc-contract.mjs`（`pnpm check:ipc-contract`，已接入 CI）：从 Rust 命令签名
  推导每个 IPC 参数键并与 `src/ipc/index.ts` 比对，Rust/TS 命名漂移直接失败。
- `get_runtime_settings` 命令与通讯设置页的"采集节奏"卡片，展示后端实际装载的配置值。
- 命令层授权测试矩阵（匿名/owner/他人/管理员/锁定/临时密码/停用账户）与 AuthGate 前端测试。

### Fixed
- **IPC 参数名不匹配（#40）**：Tauri 默认把命令参数键转成 lowerCamelCase，而前端一直发送
  snake_case。首次创建管理员、改密、用户管理、实验归属、GB/T 13241 与 13242 分析在真实
  Tauri 运行时全部报 `missing required key`；受影响命令改用
  `rename_all = "snake_case"`。GB 分析的这两条自 v0.2.0 起即已损坏。
- **GB/T 13241 采样选取（#42）**：还原度不再取整场最后一条采样。Wt 改取还原段内最后一条
  有效称量，速率指数只对还原段内有效采样做最小二乘并以还原段起点为时间基准；天平离线占位行
  （weight=0）被排除，无有效采样时拒绝出结果而不是写入被夸大数倍的 Rt。
- **实验计时漂移（#43）**：控制回路改为单调时钟驱动——引擎按真实经过时间推进，唤醒按固定
  deadline 对齐并在长时间停顿后重新同步。此前按 tick 计数推进，硬件模式下每个保温段都会比
  国标要求长出该轮采样与入库的耗时。
- **存储锁中毒与无界缓冲（#47）**：实验线程改用防中毒锁助手；采样缓冲设上限并统计丢弃行数；
  持续写失败与恢复都会写审计，不再静默丢失整场采样或让实验永远处于"未结束"。
- **锁定计数不清零（#46）**：账户锁定期满后失败计数归零，解锁后一次手误不再立即再锁 15 分钟。
- `comm_config.json` 的 `SAMPLING.interval_s`、`PERFORMANCE_CONFIG.data_collection_interval`
  与 `heartbeat_timeout` 此前被解析后完全忽略，现分别驱动实验采样周期、遥测推送周期与前端
  陈旧判定阈值；配置校验新增区间为正且有限的检查（#44 的一半）。

### Security
- 密码采用 Argon2id 随机盐哈希；连续 5 次失败锁定 15 分钟，账户仅软停用且最后一个管理员受保护。
- Rust IPC 对实验查询、采样、分析、报告、导出、删除和运行控制实施会话、角色与 owner 校验；未登录仅保留公开安全状态和紧急安全停机。
- 审计补齐操作人归属（#41）：受保护动作记录当前登录用户显示名，紧急停机记 `anonymous`，
  仅进程自身动作记 `system`；认证与用户管理动作写 `AUTH` 类别，报告/导出写 `EXPORT` 类别。
  此前 30 处审计中仅 3 处带操作人，改密/建号/改角色/重置密码完全不审计。
- 登录时用户名不存在也执行一次等价哈希校验，消除按响应时间枚举账户的侧信道（#46）。
- 重置密码改用带掩码输入与二次确认的模态框，不再用 `window.prompt` 明文录入（#46）。
- 空闲锁定超时可由 `TMH_IDLE_TIMEOUT_S` 配置，钳制在 1 分钟–8 小时。

### Changed
- 文档与实现对齐（#48）：`docs/06 §6.6` 重写为 Argon2id + 角色授权模型，删除已废弃的
  PBKDF2 / `config/auth.json` / 默认口令 `1952`；`docs/06 §6.5` 列出真实审计动作名与
  operator 要求；`docs/07`、`docs/10 M4` 的"删除需口令"更正为管理员角色，并把未实现的
  通讯设置写回明确标注为未完成。
- CLAUDE.md 增加"新增/修改 Tauri 命令"约定：必须保持 IPC 参数名契约并补授权测试。

### Known limitations
- 通讯设置页仍是只读展示，配置写回与总线热重启未实现（#44）；M4 该条验收未通过。
- real/replay 模式仍构造 MockTransport，无效现场配置仍被静默默认值替代（#19）；不得宣称真实硬件可用。
- MFC "ON/全开"在没有可恢复非零设定值时可能确认写入 0（#28）。
- 命令边界的启动新鲜度、GB 分析输入范围校验与稳定错误 DTO 仍按 #21 推进。
- 授权测试覆盖的是所有命令共用的授权 helper；`#[tauri::command]` 包装层本身仍需真实 Tauri
  运行时才能测，目前由 IPC 契约检查间接兜底。

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
