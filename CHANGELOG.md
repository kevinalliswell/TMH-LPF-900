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
