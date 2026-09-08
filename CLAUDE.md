# TMH-LPF-900 Next

Iron ore metallurgical performance testing & control system (USTB). Tauri v2 desktop app:
React + TypeScript + Vite frontend, Rust core controlling serial lab devices
(9-channel temperature controller via Modbus-RTU, 4x Azbil MFC gas flow controllers via CPL,
electronic balance via RS232), implementing Chinese national standards GB/T 13240/13241/13242.

**The complete specification lives in `docs/` — read the relevant spec before implementing any
subsystem. `docs/golden_vectors.json` is the byte-level acceptance standard for protocol code.**

## Quick Reference

```bash
pnpm install                 # frontend deps
pnpm tauri dev               # run app (TMH_DEVICE_MODE=mock pnpm tauri dev for no hardware)
pnpm tauri build             # produce installer
cargo test --workspace       # all Rust tests (protocols golden tests included)
cargo run -p tmh-core --example soak -- --replay fixtures/dirty-session.jsonl  # headless soak
pnpm test                    # frontend unit tests (vitest)
pnpm typecheck && pnpm lint  # TS gate
```

## Architecture (do not violate)

```
React UI (webview, untrusted)  ── tauri-specta typed bindings
   │  invoke (actions/queries) / Channel (telemetry stream) / events (state transitions)
src-tauri (thin glue: commands, channel pumps, plugin registration)
   │
tmh-core (bus actors, safety clamp, sampler, storage, audit, experiment engine — NO tauri dep)
   │
tmh-protocols (pure frame codecs, NO I/O — golden-tested against docs/golden_vectors.json)
```

- One dedicated blocking thread per serial bus (temp / mfc / balance), bridged to tokio via channels.
- `tmh-core` must compile and soak-test headless (no webview, mock/replay transports).

## Hard rules (safety)

- H2/CO setpoint clamp lives in the MFC bus actor at frame-build time — the ONLY choke point.
  Frontend displays limits, never enforces them. Compiled-in absolute ceiling backs the config.
- Normal completion follows the selected standard's acknowledged final atmosphere (RI: all zero; RDI/FSI: N2=5.0, others zero). See docs/16. Stops, errors, crash recovery and application exit request protective N2=5.0 L/min, CO=CO2=H2=0.
- Audit log writes must never throw into control flow.
- Zero setpoint always passes the clamp (emergency shutoff must never be blocked).

## Adding or changing a Tauri command

Tauri converts each command's argument key to lowerCamelCase unless the command declares
`#[tauri::command(rename_all = "snake_case")]`. A mismatch is invisible to both test suites
(the frontend tests run against the browser mock) and fails only at runtime, so:

- keep multi-word parameters snake_case on both sides via `rename_all`, and
- run `pnpm check:ipc-contract` (CI does) — it recomputes every expected key from the Rust
  signatures and compares them with `src/ipc/index.ts`.

Every protected command must also gain an authorization test. Authorization lives in the
`current_user` / `current_admin` / `authorize_experiment` / `require_experiment_controller`
helpers in `src-tauri/src/lib.rs`; the tests at the bottom of that file build a real
`AppState` and assert the anonymous / owner / other-user / admin / locked matrix.

Every audited action must name the operator who performed it (`.operator(...)`) — see
docs/06 §6.5.

## Conventions

- Conventional Commits: `feat(core):`, `fix(ui):`, `feat(protocols):`, `chore(release):`.
- English in code/comments/commits; Chinese (zh-CN) in UI strings and user-facing docs.
- Rust: thiserror in core crates, tracing (not println/log) for diagnostics.
- TS: strict mode, no `any` in src/, zustand for state, ECharts for charts.
- Version source of truth: `package.json` version — CI stamps `tauri.conf.json` from it.
- Config file `config/comm_config.json` keeps the legacy schema (see docs/06) — field wiring
  documentation depends on it.
