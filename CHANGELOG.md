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
