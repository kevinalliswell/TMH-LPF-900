# 27 Rust 依赖安全基线（2026-10-25）

关联 #23/#89。此批只交付依赖修复、审计门和证据，不创建tag/release，不修改实验计算或真实设备接入状态。

## 扫描范围与初始结果

使用 `cargo-audit 0.22.2` 扫描完整 `Cargo.lock`，不按操作系统或严重性过滤。RustSec官方库提交 `21d909e4c8bd4e14c7aea58bf980c881a0053ab1` 的初始结果为12条漏洞记录、3条unsound及9条未维护警告。12条包含同一ZIP公告命中两个旧版本，不能当作12个独立根因。

| 依赖路径 | 变更 | 对应公告 |
|---|---|---|
| updater → TLS | rustls 0.23.41→0.23.46 | RUSTSEC-2026-0285/0346/0348/0349 |
| updater → 证书验证/根库 | rustls-webpki 0.103.13→0.103.16；webpki-root-certs 1.0.8→1.0.10 | RUSTSEC-2026-0382/0383 |
| 运行时/网络 | tokio 1.52.3→1.53.3 | RUSTSEC-2026-0386（unsound） |
| Linux桌面通信 | event-listener 5.4.1→5.4.2 | RUSTSEC-2026-0221（unsound） |
| XLSX导出 | rust_xlsxwriter 0.79.4→0.101.0，使用zip 9.1.0 | 删除zip 2.4.2；RUSTSEC-2026-0367/0368/0369 |
| Windows更新 | updater关闭默认feature，仅保留rustls-tls | 删除zip 4.6.1及旧ZIP解压路径；同上 |

补丁范围按[官方RustSec公告](https://rustsec.org/advisories/)确定。XLSX依赖要求Rust 1.88，workspace声明下限同步为1.88；构建/验证继续固定1.95.0，此批未声称在1.88执行过完整验证。

`createUpdaterArtifacts=true` 使用Tauri v2原生Windows EXE/MSI；旧ZIP来自`v1Compatible`，本项目未启用。关闭updater的zip feature保留Windows原生安装程序、macOS原有tar.gz及Linux v2原生包路径；不再接受v1兼容压缩包。依据[官方更新器产物说明](https://v2.tauri.app/plugin/updater/#building)。当前仅构建Windows/macOS安装包，实际签名、在线升级和回滚仍未现场验收。

## 剩余glib风险与临时例外

[RUSTSEC-2024-0429](https://rustsec.org/advisories/RUSTSEC-2024-0429.html)仍存在于Linux GTK3所需的glib 0.18.5：`VariantStrIter`的迭代实现存在未定义行为。修复版本是0.20及以上，Tauri/GTK3的0.18类型体系无法通过一次兼容patch升级替换。此批没有修复该依赖。

当前应用以及所检查的Tauri/runtime/wry/tao/muda/gtk/gio/webkit2gtk源码未发现`array_iter_str`或`VariantStrIter`调用；Windows/macOS依赖树不编译glib。此为当前源码的静态核查，不能证明所有未来GUI路径绝不受影响。Linux目前用于CI，不是本项目已验收的发包平台。

`config/rust-audit-exceptions.json`因此只暂时接受该公告、该包、精确0.18.5版本，负责人为仓库维护者kevinalliswell；复查截止2026-11-20 UTC，当日零时起门禁失败。后续必须评估上游回补/迁移并删除或重新审查例外；不自动续期，不把临时接受写成“全部漏洞已修复”。PR仍需维护者按此风险记录审阅后决定是否合入，本任务不合并。

其余9条未维护警告涉及proc-macro-error、unic系列和Linux zbus的zvariant/zbus_names。完整公告仍在报告中。未维护不等于已修复或已知可利用漏洞，需在后续依赖升级时持续评估。

## 门禁与证据

`rust-audit.yml`在普通CI中扫描被测提交；candidate审计和构建均使用同一PR head；release在审计和版本/主线来源检查通过后才构建。每周及手动扫描也调用相同逻辑。只缓存固定工具二进制，每次重新取得官方数据库并记录Git SHA，不缓存公告结论。

扫描前先`cargo fetch --locked`准备完整锁文件的各平台registry metadata，再使用已取得的数据库执行`--no-fetch`扫描；缺失索引不能冒充“未撤包”。`scripts/audit-rust.mjs`保留未过滤的原始JSON和stderr。策略拒绝空/不完整报告、扫描忽略列表、平台/严重性过滤和未启用unsound/未维护/notice报告；任何未知漏洞/unsound均失败。例外必须有精确包版本、负责人、原因及最多30天期限，变更版本或过期均失败；例外仅影响决策，不删除原公告。新漏洞不能借既有glib例外通过。

每次归档源码SHA、Cargo.lock SHA256、工具版本、数据库SHA、原始报告、stderr和策略决定。失败也上传已有证据。GitHub日志/产物默认保留30天；正式版本必须另存长期档案，不能把临时artifact当作长期发版记录。

本机复现：

```bash
cargo install cargo-audit --version 0.22.2 --locked --no-default-features
git clone --depth 1 https://github.com/RustSec/advisory-db.git /tmp/tmh-rustsec-db
cargo fetch --locked
pnpm audit:rust /tmp/tmh-rustsec-db
```

已有数据库须先从官方origin更新。扫描失败时先核对工具/网络和原始报告；不设置`continue-on-error`，不通过删除公告或自动添加例外修复红灯。

## 验证记录

依赖修复后的完整扫描：0条漏洞记录，仍有上述1条临时接受的unsound和9条未维护警告。前端86项、脚本17项、原生107项、core单元109项及完整工作区普通/candidate套件通过；fmt、两种clippy、typecheck/lint/build、IPC/安全/版本、生产依赖审计、serial、replay、5分钟mock soak和actionlint通过。XLSX解包核验了工作表、数值、缺测和原始记录；全新CARGO_HOME验证fetch→完整审计通过。独立审查后的阻断项均已修复。

证据为本机软件验证，不构成真机安全或现场发布结论。新依赖的跨平台CI及candidate结果在本批PR跟踪，不能借用较早PR通过结果。
