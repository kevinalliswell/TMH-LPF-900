# Changelog

All notable changes to this project are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versioning follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

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

[Unreleased]: https://github.com/kevinalliswell/TMH-LPF-900/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/kevinalliswell/TMH-LPF-900/releases/tag/v0.1.0
