# Changelog

All notable changes to this project are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versioning follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

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
