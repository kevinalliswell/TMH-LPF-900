# M0 脚手架设计规格（TMH-LPF-900 Next）

- 日期：2026-07-05
- 状态：已定稿（用户已批准核心设计与扩展范围）
- 里程碑：M0（docs/10 §M0）+ 用户追加要求（完善版本管理 / CI-CD / Windows·Mac 资产包管理）
- 下游：本 spec 交由 writing-plans 产出实施计划

---

## 1. 目标与范围

M0 是脚手架里程碑：初始化仓库，建立 Cargo workspace（`tmh-protocols` / `tmh-core` / `src-tauri`）+ Vite React TS 前端 + pnpm + 全链 CI，能打开一个暗色空壳窗口，并把本 docs 规格包纳入仓库。

用户在此基础上追加三项，纳入 M0 范围：

1. **完善版本管理**：`package.json` 单一来源，脚本同步 `tauri.conf.json`，`CHANGELOG.md`，tag 版本一致性校验。
2. **CI/CD**：CI（测试/静态检查矩阵）+ CD（tag 触发的构建发布流水线）。
3. **资产包管理（Windows + macOS）**：CI 产出 Windows NSIS 安装包与 macOS dmg，作为 GitHub Release 资产。

### 明确不做（留给后续里程碑）

- IPC / tauri-specta 绑定、任何自定义命令 → **M3**。
- 全部 Tauri 插件（含 single-instance、updater、capability 精简）→ **M3**。
- 协议编解码、金帧测试 → **M1**。
- 总线 actor、mock/replay、存储、审计、`examples/soak` → **M2**。
- **代码签名**（Windows Authenticode、macOS codesign/公证、Tauri updater minisign 密钥）、`latest.json` 更新清单、U 盘分发规程 → **M5**。M0 产出**未签名**安装包，仅打通打包+发布流水线。

---

## 2. 验收标准（客观可检查）

### 2.1 docs/10 §M0 原生条目

- [ ] `pnpm tauri dev` 打开暗色空壳窗口，标题 = `TMH-LPF-900 v0.1.0`。
- [ ] `cargo test --workspace` 绿；`pnpm typecheck && pnpm lint && pnpm test` 绿（本地 + CI）。
- [ ] `cargo clippy --workspace -- -D warnings` 通过。
- [ ] 依赖方向：`tmh-core` 不依赖 `tauri`（`tmh-protocols` 仅依赖 `crc`）。

### 2.2 用户追加条目

- [ ] 版本单一来源：`scripts/sync-version.mjs` 能把 `package.json.version` 写入 `tauri.conf.json`；`--check` 模式在不一致时非零退出，并进 CI。
- [ ] `CHANGELOG.md` 存在，遵循 Keep a Changelog，含 `[Unreleased]` 段。
- [ ] `release.yml` 在 tag `v*`（或手动 `workflow_dispatch`）触发，产出 **Windows NSIS** + **macOS dmg（universal）**，并作为 GitHub Release 资产上传。
- [ ] `release.yml` 校验 tag（去掉前缀 `v`）== `package.json` 版本，不一致直接 fail。

### 2.3 交付动作

- [ ] 仓库初始化并推送到 `https://github.com/kevinalliswell/TMH-LPF-900`（不存在则用 `gh` 创建）。
- [ ] GitHub Actions `ci.yml` 在推送后跑绿。

---

## 3. 仓库布局

```
TMH-LPF-900/
├─ Cargo.toml                 # workspace: members=[crates/*, src-tauri], resolver=2, [workspace.dependencies]
├─ Cargo.lock                 # 提交（应用型仓库）
├─ rust-toolchain.toml        # channel=stable, components=[clippy, rustfmt]
├─ package.json               # 版本单一来源 + 前端/tauri 脚本
├─ pnpm-lock.yaml             # 提交
├─ pnpm-workspace.yaml        # 可选；M0 单包也先建，锁定 pnpm 语义
├─ vite.config.ts
├─ tsconfig.json  tsconfig.node.json
├─ eslint.config.js           # flat config
├─ vitest.config.ts
├─ index.html
├─ CHANGELOG.md
├─ .gitignore  .nvmrc         # node 版本钉
├─ src/                       # React 前端
│  ├─ main.tsx
│  ├─ App.tsx                 # M0：暗色 splash，居中显示 “TMH-LPF-900 v0.1.0”
│  ├─ App.test.tsx            # vitest：断言 splash 文案
│  ├─ vite-env.d.ts
│  ├─ styles/theme.css        # Tailwind v4 + 亮/暗 CSS 变量，默认暗色
│  ├─ ipc/     .gitkeep       # M1+ 填充（tauri-specta 绑定）
│  ├─ stores/  .gitkeep       # M1+（zustand）
│  ├─ features/ .gitkeep      # M3+（按页面域）
│  └─ components/ .gitkeep    # 共享组件
├─ src-tauri/                 # 薄壳（M0：仅窗口，无插件/命令）
│  ├─ Cargo.toml              # 依赖 tauri v2 + tmh-core
│  ├─ build.rs                # tauri_build::build()
│  ├─ tauri.conf.json         # productName/version/窗口 title/暗色
│  ├─ capabilities/           # M0 留空目录（.gitkeep）；M3 精简白名单
│  ├─ icons/                  # 占位图标集（tauri icon 生成）
│  └─ src/
│     ├─ main.rs              # 调用 lib 的 run()
│     └─ lib.rs               # pub fn run()：构建并运行 Tauri app
├─ crates/
│  ├─ tmh-protocols/          # 仅依赖 crc；M0 空 lib + 占位测试
│  │  ├─ Cargo.toml
│  │  └─ src/lib.rs
│  └─ tmh-core/               # 禁 tauri；M0 空 lib + 占位测试
│     ├─ Cargo.toml
│     └─ src/lib.rs
├─ fixtures/  .gitkeep        # M2：golden_vectors 拷贝、脏会话 jsonl
├─ config/    .gitkeep        # M2：comm_config.json（legacy schema）
├─ scripts/
│  └─ sync-version.mjs        # package.json → tauri.conf.json 同步/校验
├─ .github/workflows/
│  ├─ ci.yml                  # 测试/静态检查矩阵（ubuntu + windows）
│  └─ release.yml             # tag 触发：出 Windows/macOS 包 + Release
├─ CLAUDE.md  README.md       # 已随规格包提供
└─ docs/                      # 规格包 01–10 + golden_vectors.json + superpowers/specs/
```

依赖方向（硬约束）：`src-tauri → tmh-core → tmh-protocols`。M0 各业务 crate 为空，但 `Cargo.toml` 先建立正确依赖边与守卫。

---

## 4. Rust 侧设计

### 4.1 workspace（根 `Cargo.toml`）

- `resolver = "2"`，`members = ["crates/tmh-protocols", "crates/tmh-core", "src-tauri"]`。
- `[workspace.package]` 统一 `edition = "2021"`、`rust-version = "1.77.2"`（Tauri v2 最低；实际用 stable 1.95）、license/authors。
- `[workspace.dependencies]` 集中版本：`crc`、`thiserror`、`serde`、`serde_json`、`tokio`、`tracing`、`tracing-subscriber`、`rusqlite`、`anyhow`、`tauri`、`tauri-build`。M0 只在需要的 crate 里 `.workspace = true` 引用，未用到的先声明不引用（避免未使用依赖告警）。

> M0 为保持 clippy `-D warnings` 全绿，各 crate 只引入**当前真正用到**的依赖；`tmh-core` 的 tokio/rusqlite/tracing 等骨架依赖等 M1/M2 真正使用时再加，**M0 不提前引入未使用依赖**。

### 4.2 tmh-protocols

- `Cargo.toml`：`[dependencies] crc.workspace = true`（唯一依赖）。
- `src/lib.rs`：占位 —— 例如 `crc16_modbus(bytes: &[u8]) -> u16` 的最小实现 + 一条 `#[test]`（给 M1 铺路且让 `cargo test` 非空）。`#![forbid(unsafe_code)]`。
- 该 crate 承诺零 I/O、零外部依赖（除 crc）。

### 4.3 tmh-core

- `Cargo.toml`：M0 仅 `[dependencies] thiserror.workspace = true`（类型化错误骨架），**绝不**含 `tauri`。
- `src/lib.rs`：占位 —— 一个 `#[derive(thiserror::Error)]` 的 `CoreError` 骨架 + 一条单测。`#![forbid(unsafe_code)]`。
- 硬约束：可无头编译/测试，不得依赖 tauri 或任何 GUI/webview crate。

### 4.4 src-tauri（应用壳，lib 化）

- `Cargo.toml`：`[lib] name="tmh_app"`、`[dependencies] tauri.workspace=true`、`tmh-core`（路径依赖）；`[build-dependencies] tauri-build`。
- `lib.rs`：`pub fn run()` —— `tauri::Builder::default().run(tauri::generate_context!())`。M0 不 `.plugin(...)`、不 `.invoke_handler(...)`。
- `main.rs`：`fn main() { tmh_app::run() }`。
- lib 化目的：docs/08 §8.3 命令层测试需要 src-tauri 可作为库被测（M3 起）。

### 4.5 依赖方向守卫（CI）

CI 专设步骤：用 `cargo tree` 断言 `tmh-core`、`tmh-protocols` 的依赖树中不含 `tauri`（`tmh-protocols` 仅允许 `crc` 及其传递依赖）。任一违反即非零退出。示例判定：`cargo tree -p tmh-core -e normal,build` 的输出中出现 `tauri` → fail。

---

## 5. 前端设计

- **栈**：Vite + React（最新稳定，≥18，strict）+ TypeScript strict（`strict: true`、`noUncheckedIndexedAccess`、`src/` 禁 `any`，eslint 规则 `@typescript-eslint/no-explicit-any: error`）。
- **样式**：Tailwind CSS v4，经 `@tailwindcss/vite` 插件接入；`styles/theme.css` 定义亮/暗两套 CSS 变量，`index.html` 根 `<html class="dark">` 默认暗色（docs/02）。
- **M0 UI**：`App.tsx` 渲染满屏暗色背景 + 居中标题 `TMH-LPF-900 v0.1.0`（静态；版本文案与 `package.json` 由 sync 脚本保证一致，M0 不经 IPC）。
- **package.json 脚本**：
  - `dev`（vite）、`build`（`tsc -b && vite build`）、`preview`
  - `typecheck`（`tsc --noEmit`）、`lint`（`eslint .`）、`test`（`vitest run`）
  - `tauri`（`tauri`）、`sync-version`（`node scripts/sync-version.mjs`）
- **devDependencies**：`@tauri-apps/cli`（提供 `pnpm tauri`）、`@tauri-apps/api`、`vite`、`typescript`、`eslint` + `typescript-eslint`、`vitest`、`@testing-library/react`、`jsdom`、`tailwindcss` + `@tailwindcss/vite`、`@types/*`。
- **占位测试**：`App.test.tsx` 用 testing-library 断言渲染出 `TMH-LPF-900 v0.1.0`，保证 `pnpm test` 非空且绿。

---

## 6. Tauri 壳配置（`tauri.conf.json`）

- `productName: "TMH-LPF-900"`，`version: "0.1.0"`（由 sync 脚本写入/校验）。
- `identifier: "cn.edu.ustb.tmh-lpf-900"`（反域名；用于 bundle）。
- `app.windows[0]`：`title: "TMH-LPF-900 v0.1.0"`、`width: 1280, height: 800`、`resizable: true`、`theme: "Dark"`、合理 `minWidth/minHeight`。
- `build`：`beforeDevCommand: "pnpm dev"`、`beforeBuildCommand: "pnpm build"`、`devUrl`/`frontendDist` 指向 Vite。
- `app.security.csp`：先给合理默认（M3 收紧）。
- `bundle`（见 §8 资产打包）。
- M0 `plugins: {}`，`capabilities`：默认最小（M3 精简）。

---

## 7. 版本管理

- **单一来源**：`package.json.version`。
- **同步脚本** `scripts/sync-version.mjs`（Node，零依赖）：
  - 默认模式：读 `package.json.version` → 写入 `src-tauri/tauri.conf.json` 的 `version` + 窗口 `title` 的版本段（`TMH-LPF-900 v<version>`）。
  - `--check` 模式：只比对，不一致则打印差异并 `process.exit(1)`（供 CI 与 pre-release 用）。
- **CHANGELOG.md**：Keep a Changelog 格式，含 `## [Unreleased]` 与 `## [0.1.0] - 2026-07-05`（M0 脚手架条目）。
- **发布流程**（docs/09 §9.4，M0 建立雏形）：`[Unreleased]` 累积 → 发版 PR（bump `package.json` + 归档 changelog + 跑 `sync-version`）→ 合并 main → 打 tag `v<version>` → `release.yml` 构建发布。
- **tag 一致性**：`release.yml` 第一步校验 `${GITHUB_REF#refs/tags/v}` == `package.json.version`，不等则 fail（对齐 docs/09）。

---

## 8. CI/CD 与资产打包

### 8.1 `ci.yml`（push / pull_request）

矩阵 `os: [ubuntu-latest, windows-latest]`。每个 job：

1. `actions/checkout`。
2. 装 Rust（stable + clippy + rustfmt，`Swatinem/rust-cache`）。
3. 装 pnpm（`pnpm/action-setup`）+ Node（`actions/setup-node`，`.nvmrc`，`cache: pnpm`）。
4. **仅 ubuntu**：`apt-get install` Tauri 构建依赖（`libwebkit2gtk-4.1-dev`、`libgtk-3-dev`、`librsvg2-dev`、`libayatana-appindicator3-dev`、`patchelf` 等）。
5. `cargo fmt --all --check`。
6. `cargo clippy --workspace --all-targets -- -D warnings`。
7. `cargo test --workspace`。
8. **依赖方向检查**（§4.5）。
9. `pnpm install --frozen-lockfile` → `pnpm typecheck && pnpm lint && pnpm test`。
10. `node scripts/sync-version.mjs --check`（版本一致性）。
11. `cargo build --workspace`（证明 src-tauri 壳在 Windows 可编）。

> M2 追加 soak 短跑步骤；M5 追加签名。M0 只覆盖 M0 验收项，确保全绿可达。

### 8.2 `release.yml`（tag `v*` / workflow_dispatch）

- **前置 job**：校验 tag 版本 == `package.json` 版本（不一致 fail）。
- **构建矩阵**（`tauri-apps/tauri-action`，自动建/更新 GitHub Release 并上传资产）：
  - `windows-latest`：产出 **NSIS**（`.exe` 安装包）。
  - `macos-latest`：`rustup target add x86_64-apple-darwin aarch64-apple-darwin`，`args: --target universal-apple-darwin`，产出 **universal dmg**。
- Release 说明取自 `CHANGELOG.md` 对应版本段。
- **M0 未签名**：不配置 Authenticode / codesign / minisign（无证书）；相关步骤以注释/文档形式预留，M5 挂接。

### 8.3 `tauri.conf.json` 的 bundle 配置

- `bundle.active: true`。
- `bundle.targets`：由 tauri-action 按 OS 选择；Windows 侧限定 `nsis`，macOS 侧 `dmg`/`app`。
- `bundle.windows.nsis.installMode: "perMachine"`（docs/09）。
- `bundle.windows.webviewInstallMode: { "type": "offlineInstaller" }`（docs/09 硬约束：目标机可能离线，内嵌 WebView2 离线安装器，约 +127MB）。
- `bundle.icon`：占位图标集（后续替换正式图标）。
- macOS `minimumSystemVersion` 合理默认。

---

## 9. 本机工具安装（全量验证所需，可逆）

- `corepack enable && corepack prepare pnpm@latest --activate`（pnpm）。
- `rustup component add clippy rustfmt`。
- `@tauri-apps/cli` 作为 devDependency（`pnpm tauri` 入口），首次 `pnpm tauri dev` 会触发 Rust 依赖下载与 macOS 原生壳编译（需 Xcode CLT）。

---

## 10. Git 与远程

1. `git init`（已完成）→ 本地身份用仓库级 `git config`（不动全局）：email `kevin.alliswell@gmail.com`。
2. 提交顺序（Conventional Commits，消息末尾按 CLAUDE.md 署名）：
   - `chore: initialize repository with spec package`（docs/、CLAUDE.md、README、.gitignore）
   - `docs(superpowers): add M0 scaffold design spec`（本文件）
   - 随后按实施计划分组提交脚手架。
3. `gh repo view kevinalliswell/TMH-LPF-900`（不存在则 `gh repo create kevinalliswell/TMH-LPF-900 --private --source=. --remote=origin`）。
4. 设 `origin` → `git push -u origin main`。
5. 观察 Actions `ci.yml` 跑绿（实测 M0「CI 全绿」验收）。

---

## 11. 验证计划（全量，本机 macOS）

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- 依赖方向：`cargo tree -p tmh-core -e normal,build`（确认无 tauri）
- `pnpm install` → `pnpm typecheck && pnpm lint && pnpm test`
- `node scripts/sync-version.mjs --check`
- **`pnpm tauri dev`**：真启动，截图确认暗色窗口 + 标题 `TMH-LPF-900 v0.1.0`，随后关闭。
- 推送后：GitHub Actions `ci.yml` 全绿。

---

## 12. 风险与应对

- **macOS 首次 Tauri 编译慢/缺 Xcode CLT**：若缺 `xcode-select`，提示用户安装；编译时间计入首次验证。
- **CI ubuntu 构建 tauri 缺系统库**：已在 §8.1 步骤 4 预装。
- **offlineInstaller 拉长 Windows 构建**：仅在 `release.yml` 生效（tag 时），`ci.yml` 不做 bundle，日常 CI 不受影响。
- **未使用依赖触发 clippy 告警**：M0 各 crate 只引真正用到的依赖（§4.1 注）。
