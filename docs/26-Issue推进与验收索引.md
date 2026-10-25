# 26 Issue 推进与验收索引（2026-10-24）

代码基线为 main `d54b75a`（已合并PR #72）。下表区分已有实现、本轮待合并增量和未具备的外部证据。开放issue不等于全部功能缺失，测试通过也不等于现场验收。所有本轮PR保持开放、不合并。

## 批次依赖

| 批次 | PR/基线 | 交付与验证 |
|---|---|---|
| 1 | [#98](https://github.com/kevinalliswell/TMH-LPF-900/pull/98) → main | 统一命令错误、真实IPC拒绝不改数据；docs/24 |
| 2 | [#99](https://github.com/kevinalliswell/TMH-LPF-900/pull/99) → #98分支 | 启动错误页、严格模式及effective水印；docs/25 |
| 3 | [#100](https://github.com/kevinalliswell/TMH-LPF-900/pull/100) → #99分支 | IPC/发布脚本测试、CI证据归档、版本同步和本索引 |
| 4 | [#101](https://github.com/kevinalliswell/TMH-LPF-900/pull/101) → #100分支 | 燃气真实IPC矩阵、拒绝审计、5 L/min上限及探针ACK闭环 |
| 5 | [#102](https://github.com/kevinalliswell/TMH-LPF-900/pull/102) → #101分支 | RustSec依赖修复/门禁与锁定构建，docs/27 |
| 6 | 基于#102分支，见关联PR | 同源码source SBOM、候选全文件校验和正式草稿资产闭环，docs/28 |

每批均执行fmt/clippy、Rust普通/candidate工作区、前端typecheck/lint/test/build、IPC/安全/版本门禁、生产依赖审计、serial编译、dirty replay及5分钟mock soak。第三批另执行脚本回归与actionlint。自动日志按GitHub run保留30天，需长期归档的正式/现场证据应移入发布资产或实验室档案。

CI默认测试PR合成提交，candidate测试head提交；`verification/context.json`保留实际源码SHA、父提交、版本、工具链和平台，不能把两者混为同一提交。candidate验证产物manifest仍独立记录安装包SHA。程序不会在普通CI触碰真实串口。

## 历史问题与阶段对应

| Issue | 当前实现/证据 | 尚需完成 |
|---|---|---|
| #19 | 配置不回退已在main；#99严格拒绝模式、启动错误可见、requested/effective | 真实三串口组合根与握手；显式三总线replay会话；厂家协议/现场记录 |
| #21 | main的启动预检、国标输入与类型/管理员删除；#98补错误DTO及真实IPC回归 | 命令逐项拒绝审计和全面授权矩阵需继续复核；历史PBKDF2要求已由Argon2id/RBAC规格替代 |
| #23 | PR#26版本来源/Node24/tag来源；第三批补严格SemVer、annotated tag、main保护与同SHA成功CI门禁；第五批RustSec和依赖修复；第六批SBOM/正式checksum代码 | main保护实际启用、glib临时风险到期复查、签名与密钥负责人、正式发布/现场证据 |
| #28 | main的最后确认非零SV恢复、冷启动拒绝ON、批次部分失败审计；MFC actor与native gas tests | 厂家独立ON/OFF寄存器结论、真实设定/关/开和部分失败验证 |
| #41 | main的认证/账户拒绝审计、CONFIG/EXPORT类别、操作者快照；auth_audit_tests与config_commands_tests | 继续核查每个命令拒绝/成功四元组；现场审计链复核 |
| #42 | main的有效还原采样、冻结排除统计与报告；analysis_sample_tests、group_evidence、docs/22 | 实样连续终点数据与允许差异复核。按docs/13/16核对后的RI/RVI规则执行，不恢复旧最小二乘方案；有来源人工补录单独标识 |
| #43 | main的Instant经过时间、deadline采样、停顿不补造；lib.rs中deadline与采样边界测试 | sim_speed=1的现场长周期计时及墙钟偏差证据 |
| #44 | main的管理员保存/冲突/保护重启、saved/applied、节奏项接入；config_store_contract与config_commands_tests、docs/21 | 真实端口重启后的实际参数核验；当前采用整应用重启，未承诺进程内热换总线 |
| #45 | main的owner/admin/匿名/锁定/待改密守卫、AuthGate、存储/组/配置回归；#98增加真实IPC拒绝 | 完整逐命令矩阵及真实设备故障场景持续扩展 |
| #48 | main已更新docs/06/07/10/11；#98同步错误DTO现状 | 后续实现变化持续同步；历史验收文档不改写日期/结论 |
| #49 | 本表把审查项映射到现有实现与阶段 | 保持评估结论随新证据更新，不能批量当作已验收关闭 |
| #79 | #99完成可拒绝的软件边界，模式和配置测试 | 与#19同项硬件/会话缺口；不得标记real-ready |
| #80 | Probe工具与serial编译、模拟协议金帧回归 | 设备型号/手册/接线/原始抓包、冷启断线重连、完整四阶段真机soak |
| #81 | main保护锁存/抢占/ACK、恢复SV；hub_emergency_priority、device_lifecycle_tests | 断电、拔线、真实ACK与安全气氛现场验证及ON/OFF协议结论 |
| #82 | IPC参数/注册/错误返回门禁、版本同步、docs/24；第三批为门禁本身补回归 | DTO仍手工映射，新增DTO需序列化/IPC断言；main实际保护与外部发布证据 |
| #83 | main配置/权限/预检；#98稳定错误；#99初始化失败不进入控制台 | 逐命令拒绝审计与授权矩阵剩余项、真机配置重启验证 |
| #84 | main计时、冻结分析与账户审计；#98补分析拒绝契约 | 同#41/#42/#43现场结果和剩余审计复核 |
| #85 | 原生SQLite故障、锁中毒、幂等恢复、紧急优先级；第三批CI归档日志/上下文 | 桌面replay/real故障接口与逐命令矩阵；现场恢复/掉电证明 |
| #86 | 冻结检验组、报告与过程证据已实现 | 同批样品数量、允许差异阈值、旧系统原始数据、新旧并行实验及负责人签字 |
| #87 | 已有Windows NSIS离线候选构建/安装冒烟及macOS universal候选验证 | 目标工控机/驱动/离线部署、备份恢复、证书、升级/回滚演练、现场支持负责人 |
| #88 | main认证锁定/枚举防护/临时密码表单、存储上限及异常回归；文档已校正 | 长时间现场soak与后续文档状态维护 |
| #89 | tag-only release与candidate分离；第三批来源/测试门禁；第五批同源码RustSec审计及锁定构建；第六批SBOM与全资产清单 | glib到期复查、签名/公证、密钥轮换与灾备、正式资产/发布/回滚实际演练 |
| #90 | 明确为后续方向池 | v1.0现场通过后，按场景、网络边界和负责人立项，不自动开发 |

## 仓库策略实况与发布阻断

2026-10-24读取GitHub branches/main API：`protected=false`，main SHA为`d54b75a`。这只是只读核验，未修改仓库策略。正式release要求main已保护且该提交的push CI成功；条件未满足即失败，不能用YAML存在替代实际保护。需按docs/12启用PR、至少一名审批、Ubuntu/Windows required checks、禁止force-push及删除。当前任务不合并或发布任何版本。

## 外部输入清单

1. 9路温控、4路MFC、天平的型号/固件与厂家通讯手册路径；特别是从站0、字宽、缩放、ACK/异常帧、ON/OFF寄存器。
2. 三总线独立TX/RX原始会话、串口参数、时间与保护回执；探针先按既有只读流程执行。
3. 目标Windows工控机、驱动与离线安装/恢复环境。
4. 新旧系统同批样品、操作规程、样本数与差异阈值、负责人和签字流程。
5. 签名证书/更新密钥的授权维护者与恢复责任人；密钥本体不得进入仓库或测试日志。

以上缺口保留未完成。软件批次可继续推进，不等待无关现场条件；依赖设备的数据与结论必须等待实际输入。

## 第三批本机结果

2026-10-24：Node24.18.0、pnpm11.10.0、Rust1.95.0、macOS arm64。11项Node脚本回归、前端85项、原生应用102项及工作区普通/candidate套件全部通过；fmt/clippy/typecheck/lint/build、IPC/安全/版本、生产依赖审计、serial编译、dirty replay、5分钟mock soak、actionlint均通过。预发布版本后缀缺陷在修改前复现失败。独立审查发现并修复构建元数据误判预发布、浅克隆父提交丢失两项问题，新增真实临时仓库回归。

PR#98的Ubuntu CI已通过；其余Windows/candidate及后续批次跨平台状态以PR checks实时记录为准，尚未作为本机验证结论的一部分。后续可继续推进逐命令矩阵、拒绝审计、Rust安全审计、SBOM与正式资产校验；不等待无关现场输入。

## 第四批燃气边界复核

复现发现：命令负流量此前被actor防御性归零并当作成功；编译期上限10与规格5 L/min不符；探针另行构造写帧并将任意返回字节视作ACK。修复在命令边界拒绝无效值、记录REJECTED；actor最终上限5、配置只允许降低。Probe写入经过同一actor与ACK解析，只允许N2 0–5 L/min或所有气路归零，现场协议合格前不开放H₂/CO/CO₂正流量。标准程序CO 4.5 L/min保留。旧配置若H₂/CO限值大于5会明确拒绝，请按实验室规定核对配置后再启动，不自动改写文件。

此批增加真实燃气IPC授权矩阵和审计四元组、无效设定不写设备、探针错ACK/非法值/关断、编译期与配置上限回归；完整应用命令矩阵仍作为后续项。


第四批审查补充：探针先由actor同一构帧/量化路径预览actual applied，拒绝N2量化后超过5（scaling=4、requested=5→5.2的用例已先复现）；无效负限值下归零仍可放行。燃气拒绝审计与权限校验使用同一次会话决定，避免等待生命周期锁期间账户切换造成归属漂移。探针示例补明确MFC偶校验、天平1200波特；这些仍是协议规范示例，不是现场参数确认记录。

第四批本机最终验证（2026-10-24）：前端86项、Node脚本12项、原生应用107项、core单元108项及工作区普通/candidate所有套件通过，probe新增3项回归。fmt/clippy/typecheck/lint/build、IPC/安全/版本、生产依赖审计、serial编译、dirty replay与5分钟mock soak、actionlint均通过。独立安全审查未发现新增阻断。负流量、10→5编译上限、负限值归零、N2量化超限均先复现失败再修复。此证据仍为软件模拟验证。

远端状态更新：PR#98 Linux/Windows CI、Windows NSIS安装冒烟与macOS universal候选均已成功；#99 Linux/Windows CI及macOS候选成功，Windows候选仍以实时checks为准；#100 Linux/Windows CI及macOS候选成功，Windows候选仍以checks为准。#98是较早批次，不能作为第四批新代码已跨平台通过的证据。

## 第五批依赖安全与最终状态更新（2026-10-25）

PR#98–#101的Linux/Windows CI、Windows NSIS安装冒烟和macOS universal候选现已全部成功；#101另有probe Windows构建成功。结果分别对应各PR的被测源码，不能替代第五批新依赖的跨平台验证。

第五批定向升级TLS/证书、Tokio、event-listener及XLSX依赖，移除未使用的旧ZIP更新解压路径。固定RustSec工具、官方数据库SHA、原始报告与stderr归档；已知漏洞和unsound必须经策略门，精确例外不隐藏公告。当前完整扫描为0条漏洞、1条glib0.18.5临时接受的unsound及9条未维护警告；例外2026-11-20 UTC到期，不能据此宣称风险清零，详见docs/27。

本机全面验证通过：前端86项、Node脚本17项、原生应用107项、core单元109项及普通/candidate工作区全部套件；fmt、普通/candidate clippy、typecheck/lint/build、IPC/安全/版本、生产依赖审计、serial编译、dirty replay、5分钟mock soak、actionlint均通过。新增XLSX实际解包验证数值/缺测/原始证据。独立审查修复了扫描stderr误放行、审计后构建未锁依赖、干净runner缺registry metadata三项，并用全新CARGO_HOME执行fetch→完整审计通过。第五批跨平台状态在本批PR持续跟踪。

## 第六批SBOM与交付证据（2026-10-25）

固定且校验工具字节的Syft 1.55生成CycloneDX 1.7源码锁清单，实际扫描914组件，覆盖所有Cargo.lock包/版本及前端生产依赖。它包含其他平台和开发/构建依赖，不是二进制组成证明。candidate打包和正式草稿inventory同时检查实际检出干净、SBOM源码SHA/版本/dirty一致，再对所有交付字节及manifest计算校验和。正式脚本检查三个updater架构项、签名文件一致性、DMG和草稿/版本，不自动发布；离线CLI fixture不创建tag/release。

本机前端86项、Node脚本26项、原生107项、core单元109项及普通/candidate全部工作区、fmt/两种clippy、typecheck/lint/build、IPC/安全/版本、生产依赖审计、当前RustSec策略、serial、dirty replay、5分钟mock soak、actionlint均通过。真实Syft执行通过；clean SBOM后修改源码仍被接受的问题已先复现再修复。实际构建SHA的干净SBOM会在提交后再次核对。第五批#102的Linux/Windows CI、两次RustSec和probe已通过，candidate仍按实时checks跟踪。

重新核验历史状态：`v0.2.0`实际已于2026-08-31发布，指向`cbed665`且有原生EXE/DMG/tar.gz与updater资产。#23描述中的“没有任何tag”是旧基线事实，不能沿用到当前；docs/12已校正。main仍为`d54b75a`且protected=false。未修改历史发布、仓库策略、main、tag，未将任何现场门勾为完成。
