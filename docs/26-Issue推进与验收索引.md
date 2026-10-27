# 26 Issue 推进与验收索引（2026-10-27）

代码基线为 main `d54b75a`（已合并PR #72）。下表区分已有实现、本轮待合并增量和未具备的外部证据。开放issue不等于全部功能缺失，测试通过也不等于现场验收。所有本轮PR保持开放、不合并。

## 批次依赖

| 批次 | PR/基线 | 交付与验证 |
|---|---|---|
| 1 | [#98](https://github.com/kevinalliswell/TMH-LPF-900/pull/98) → main | 统一命令错误、真实IPC拒绝不改数据；docs/24 |
| 2 | [#99](https://github.com/kevinalliswell/TMH-LPF-900/pull/99) → #98分支 | 启动错误页、严格模式及effective水印；docs/25 |
| 3 | [#100](https://github.com/kevinalliswell/TMH-LPF-900/pull/100) → #99分支 | IPC/发布脚本测试、CI证据归档、版本同步和本索引 |
| 4 | [#101](https://github.com/kevinalliswell/TMH-LPF-900/pull/101) → #100分支 | 燃气真实IPC矩阵、拒绝审计、5 L/min上限及探针ACK闭环 |
| 5 | [#102](https://github.com/kevinalliswell/TMH-LPF-900/pull/102) → #101分支 | RustSec依赖修复/门禁与锁定构建，docs/27 |
| 6 | [#103](https://github.com/kevinalliswell/TMH-LPF-900/pull/103) → #102分支 | 同源码source SBOM、候选全文件校验和正式草稿资产闭环，docs/28 |
| 7 | [#104](https://github.com/kevinalliswell/TMH-LPF-900/pull/104) → #103分支 | 设备控制审计、去皮称量基准、忙碌与关闭保护 |
| 8 | [#105](https://github.com/kevinalliswell/TMH-LPF-900/pull/105) → #104分支 | 实验控制完整审计、恢复互斥和过期回执保护 |
| 9 | [#106](https://github.com/kevinalliswell/TMH-LPF-900/pull/106) → #105分支 | 实验数据/分析/补录授权及终局审计、损坏执行快照拒绝 |
| 10 | [#107](https://github.com/kevinalliswell/TMH-LPF-900/pull/107) → #106分支 | 自定义程序/检验组保存审计及生命周期保护 |
| 11 | [#108](https://github.com/kevinalliswell/TMH-LPF-900/pull/108) → #107分支 | 单次报告、采样导出与检验组报告终局审计 |
| 12 | [#109](https://github.com/kevinalliswell/TMH-LPF-900/pull/109) → #108分支 | 浏览器 mock 权限、实验控制和数据归属与原生行为对齐 |
| 13 | [#110](https://github.com/kevinalliswell/TMH-LPF-900/pull/110) → #109分支 | 更新检查/安装权限、稳定错误码及终局审计 |
| 14 | [#111](https://github.com/kevinalliswell/TMH-LPF-900/pull/111) → #110分支 | 配置保存接受关闭边界、单次授权快照及故障审计分类 |
| 15 | [#112](https://github.com/kevinalliswell/TMH-LPF-900/pull/112) → #111分支 | 用户关闭来源及操作者归属 |
| 16 | [#113](https://github.com/kevinalliswell/TMH-LPF-900/pull/113) → #112分支 | Rust/TypeScript IPC DTO 单一映射 |
| 17 | [#114](https://github.com/kevinalliswell/TMH-LPF-900/pull/114) → #113分支 | 普通用户界面授权可见性回归 |
| 18 | [#115](https://github.com/kevinalliswell/TMH-LPF-900/pull/115) → #114分支 | #48 认证/权限、版本与现场验收文档校正 |
| 19 | [#116](https://github.com/kevinalliswell/TMH-LPF-900/pull/116) → #115分支 | #45/#83 认证生命周期、账户管理、公开安全命令及剩余受保护只读 IPC 回归 |
| 20 | [#117](https://github.com/kevinalliswell/TMH-LPF-900/pull/117) → #116；[#118](https://github.com/kevinalliswell/TMH-LPF-900/pull/118) → #117；[#119](https://github.com/kevinalliswell/TMH-LPF-900/pull/119) → #118 | #23/#89 公开 updater 地址与 main/tag 规则 |
| 21 | [#120](https://github.com/kevinalliswell/TMH-LPF-900/pull/120) → #119 | #19/#86 现场验收输入模板 |
| 22 | [#121](https://github.com/kevinalliswell/TMH-LPF-900/pull/121) → #120 | #44 通讯配置保存二次确认 |

每批均执行fmt/clippy、Rust普通/candidate工作区、前端typecheck/lint/test/build、IPC/安全/版本门禁、生产依赖审计、serial编译、dirty replay及5分钟mock soak。第三批另执行脚本回归与actionlint。自动日志按GitHub run保留30天，需长期归档的正式/现场证据应移入发布资产或实验室档案。

CI默认测试PR合成提交，candidate测试head提交；`verification/context.json`保留实际源码SHA、父提交、版本、工具链和平台，不能把两者混为同一提交。candidate验证产物manifest仍独立记录安装包SHA。程序不会在普通CI触碰真实串口。

## 历史问题与阶段对应

| Issue | 当前实现/证据 | 尚需完成 |
|---|---|---|
| #19 | 配置不回退已在main；#99严格拒绝模式、启动错误可见、requested/effective | 真实三串口组合根与握手；显式三总线replay会话；厂家协议/现场记录 |
| #21 | main的启动预检、国标输入与类型/管理员删除；#98补错误DTO及真实IPC回归；#116补剩余受保护读取/遥测命令六状态IPC矩阵，并验证方案参数查询先认证 | 真实设备故障注入和同批样品科学结果比对；历史PBKDF2要求已由Argon2id/RBAC规格替代 |
| #23 | PR#26版本来源/Node24/tag来源；第三批补严格SemVer、annotated tag与同SHA CI门禁；第五批RustSec和依赖修复；第六批SBOM/正式checksum代码；#117按公开仓库实况修正updater地址；#118启用main分支保护；#119启用`v*` tag创建及不可变规则 | 增加可审阅维护者、glib临时风险到期复查、签名与密钥负责人、正式发布/现场证据 |
| #28 | main的最后确认非零SV恢复、冷启动拒绝ON、批次部分失败审计；MFC actor与native gas tests | 厂家独立ON/OFF寄存器结论、真实设定/关/开和部分失败验证 |
| #41 | main的认证/账户拒绝审计、CONFIG/EXPORT类别、操作者快照；auth_audit_tests与config_commands_tests；#111补配置保存拒绝/数据库故障四元组；#112补用户/匿名退出归属 | 继续核查每个命令拒绝/成功四元组；现场审计链复核 |
| #42 | main的有效还原采样、冻结排除统计与报告；analysis_sample_tests、group_evidence、docs/22 | 实样连续终点数据与允许差异复核。按docs/13/16核对后的RI/RVI规则执行，不恢复旧最小二乘方案；有来源人工补录单独标识 |
| #43 | main的Instant经过时间、deadline采样、停顿不补造；lib.rs中deadline与采样边界测试 | sim_speed=1的现场长周期计时及墙钟偏差证据 |
| #44 | main的管理员保存/冲突/保护重启、saved/applied、节奏项接入；设置页覆盖通讯字段与安全上限，PR #121 补保存二次确认；config_store_contract与config_commands_tests、docs/21；#111补接受关闭后拒绝保存和文件故障稳定码 | 真实端口重启后的实际参数核验；当前采用整应用重启，未承诺进程内热换总线 |
| #45 | main的owner/admin/匿名/锁定/待改密守卫、AuthGate、存储/组/配置回归；#98增加真实IPC拒绝；#104–#112分别补设备、实验、数据、模板/检验组、导出、更新和配置的六主体命令矩阵；#109补浏览器mock隔离；#114补普通用户界面可见性；#116补账户/认证生命周期、公开安全查询/紧急停机和受保护只读/遥测入口的真实IPC覆盖 | 真实设备故障注入矩阵及现场安全停机验收仍待硬件条件；新增受保护命令需继续配套授权测试 |
| #48 | main已有docs/06/07认证和UI规则；#98同步错误DTO现状；#115校正docs/01/09/10/11/12、CHANGELOG和任务清单中的首启认证、设备能力、候选版本及现场限制，补充验收证据模板与认证规格验证记录 | 后续实现变化持续同步；历史验收文档不改写日期/结论 |
| #49 | 本表把审查项映射到现有实现与阶段 | 保持评估结论随新证据更新，不能批量当作已验收关闭 |
| #79 | #99完成可拒绝的软件边界，模式和配置测试 | 与#19同项硬件/会话缺口；不得标记real-ready |
| #80 | Probe工具与serial编译、模拟协议金帧回归 | 设备型号/手册/接线/原始抓包、冷启断线重连、完整四阶段真机soak |
| #81 | main保护锁存/抢占/ACK、恢复SV；hub_emergency_priority、device_lifecycle_tests；#112区分窗口关闭与系统退出审计来源 | 断电、拔线、真实ACK与安全气氛现场验证及ON/OFF协议结论 |
| #82 | #98统一错误DTO；#113将Rust/Serde wire DTO生成至TypeScript，并门禁62个已注册命令的返回DTO、59个invoke参数、handler注册；版本同步与docs/24 | main保护配置、跨平台CI与外部发布证据；非IPC UI草稿仍是前端模型 |
| #83 | main配置/权限/预检；#98稳定错误；#99初始化失败不进入控制台；#109对齐浏览器mock设备、实验及历史权限；#111补配置保存关闭/授权/故障边界；#116补认证生命周期、账户管理、公开安全及受保护只读/遥测命令真实IPC授权矩阵，并修正方案查询早于身份校验的错误顺序 | 真实设备配置重启验证、现场验收及新命令的持续覆盖 |
| #84 | main计时、冻结分析与账户审计；#98补分析拒绝契约；#112修复窗口退出误记system | 同#41/#42/#43现场结果和剩余审计复核 |
| #85 | 原生SQLite故障、锁中毒、幂等恢复、紧急优先级；第三批CI归档日志/上下文 | 桌面replay/real故障接口与逐命令矩阵；现场恢复/掉电证明 |
| #86 | 冻结检验组、报告与过程证据已实现 | 同批样品数量、允许差异阈值、旧系统原始数据、新旧并行实验及负责人签字 |
| #87 | 已有Windows NSIS离线候选构建/安装冒烟及macOS universal候选验证 | 目标工控机/驱动/离线部署、备份恢复、证书、升级/回滚演练、现场支持负责人 |
| #88 | main认证锁定/枚举防护/临时密码表单、存储上限及异常回归；文档已校正 | 长时间现场soak与后续文档状态维护 |
| #89 | tag-only release与candidate分离；第三批来源/测试门禁；第五批同源码RustSec审计及锁定构建；第六批SBOM与全资产清单；#110补更新检查/安装命令权限与终局审计；#117改用公开源仓库 Releases；#119保护`v*` tag创建及不可变性 | glib到期复查、签名/公证、密钥轮换与灾备、已安装桌面端更新/回滚实际演练 |
| #90 | 明确为后续方向池 | v1.0现场通过后，按场景、网络边界和负责人立项，不自动开发 |

## 仓库策略实况与发布阻断

2026-10-24读取GitHub branches/main API：`protected=false`，main SHA为`d54b75a`。2026-10-27确认源码仓库为public，匿名访问本仓库latest.json返回`v0.2.0`；原updater端点指向的`TMH-LPF-900-releases`仓库不存在。PR #117调整updater端点并修正文档，应用仍默认关闭更新检查，不创建或发布新Release。随后已为`main`启用并回读分支保护：必须经PR、严格要求分支最新、至少一名审批且新提交废弃旧审批；required checks 为`check (ubuntu-latest)`、`check (windows-latest)`、`RustSec CI / audit`；管理员也受规则约束，禁止force-push、删除和未解决会话。当前main SHA的push CI成功记录为run `34762466553`。直接协作者仅仓库所有者，因此当前stacked PR在增加第二名可审阅维护者前不能合并；不降低审批门槛。另为`refs/tags/v*`启用GitHub rulesets：ID `25939730` 仅允许仓库管理员创建版本tag；ID `25939735` 禁止所有人（含管理员）更新或删除已建版本tag。当前任务不合并或发布任何版本。

## 外部输入清单

可直接填写的设备联调、原始报文、样品对照和工控机验收表见[现场资料填写模板](29-现场资料填写模板.md)。

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

## 第七批设备控制与称量基准（2026-10-26）

第六批#103及第五批#102的Linux/Windows CI、RustSec、Windows安装冒烟和macOS universal候选已全部成功，#103同源码SBOM检查也已成功。各批仍保持开放、不合并。

本批补齐去皮及受保护手动切断的六主体真实IPC授权/审计矩阵、设备服务关闭和认证存储失败。运行中或待保存时拒绝去皮，避免破坏docs/13规定的m₁/mₜ同一称量基准。去皮任务独立持有忙碌状态到设备结果和审计完成，界面调用取消也不会提前解除。启动预检/实际启动、重复去皮、更新和关闭都有受控设备竞态回归；接受关闭后拒绝新操作，重复关闭幂等。

任何去皮尝试均使旧重量失效，包括无确认的失败；总线先发布新快照再唤醒回执等待者，之后仍须新的稳定读数。前端同步限制按钮及称量基准提示。运行协议和现场资格保持docs/25边界，未启动本地服务，待其余软件命令批次完成后集中验证。

其余实验停止、保护复位、阶段请求及数据命令的拒绝审计继续按独立批次推进；不能据此将完整命令矩阵或现场门勾为完成。

本机全面验证：前端89项、Node脚本26项，Rust普通/candidate工作区各413项（原生应用122、core单元110，另含协议与集成测试）通过；fmt、两种clippy、typecheck/lint/build、IPC/安全/版本门禁、生产依赖审计、RustSec策略、serial编译、dirty replay及300模拟秒/300行mock soak全部通过。模拟soak使用逻辑时钟，不是5分钟墙钟设备耐久试验。认证/审计、运行/待保存去皮、未完成去皮放行、关闭后再入队及重复退出均先复现再修复；另补ACK唤醒时同步观察快照的发布边界测试。glib临时例外和9项未维护警告仍按docs/27管理。

## 第八批实验控制与恢复边界（2026-10-26）

基于#104补齐启动、停止、保护复位及阶段请求的拒绝/失败审计，保持公共应急入口无需登录。阶段请求的成功仅表示已排队，先记请求再交给引擎；实际执行仍单独记stage_action。操作者和阶段证据使用同一次授权返回的UserRow，不重新读取会话。启动受理审计在工作线程启动前完成。

保护复位只有有效ACK且提交时仍无新实验/保存/更新/关闭/保护时才成功，否则返回RESET_SUPERSEDED。停止命令的降温提示和保护入队在授权边界内完成；迟到ACK只能修改其原有状态版本，不能覆盖新的错误、待保存或后来运行又结束的空闲状态。MFC命令区分真实队列满与已关闭服务，不再将已关闭误报为繁忙。

恢复保存现在在生命周期锁内完成认证、空闲检查、保护和恢复事务，防止新实验跨越恢复扫描/提交；运行/更新/关闭/在途去皮时拒绝。真实MFC写入屏障证明此前恢复期间启动锁可被取得；此回归没有冒称实际复现活动实验数据损坏。

新增13项回归：控制六主体矩阵、无效/重复阶段请求、设备服务关闭、复位过期/失败、恢复互斥和迟到停止。迟到停止通过实际设备写屏障及后续零流量ACK建立顺序；删除状态版本保护时，较新的Idle被旧失败覆盖，补充回归能检出。独立审查发现的问题均已修复。

全面本机验证：前端89、脚本26、Rust普通/candidate工作区各426项（应用135、core单元110及其余套件）；fmt/两种clippy、typecheck/lint/build、IPC/安全/版本、生产依赖审计、RustSec策略、serial、dirty replay、300模拟秒/300行soak通过。下一批为数据变更/分析、模板/检验组、导出及浏览器mock的一致性。未启动本地服务，仍待软件批次完成后集中验证。

## 第九批实验数据与分析审计（2026-10-26）

六项命令：归属变更、删除、RI/RDI/FSI分析和报告补录，统一记录受理/拒绝/失败及授权时操作者。数据操作移入experiment_data模块，Tauri入口保持原参数和DTO。生命周期锁覆盖权限、对象检查、写入和审计，阻止更新/关闭以及归属变更/删除跨越分析授权；已结束记录仍可在另一实验运行时处理。保留归属变更和报告资料原有准备时机，未额外要求全部记录结束。

SQL故障返回STORAGE_ERROR并记FAILURE，不再显示原始SQL诊断；触发器故障回归验证原记录、采样及历史分析版本回滚。存在但损坏的执行程序快照明确返回ANALYSIS_DATA_INVALID，不再回退为行上的实验类型；无快照旧记录保留现有兼容规则。计算公式、有效采样统计、computed_by和只追加历史结构不变，单次计算仍不冒称完整国标验收。

新增6组真实IPC回归涵盖六主体、合法写入、状态/类型/输入拒绝、SQLite故障、更新/退出及损坏快照；独立审查无剩余阻断。本机前端89、脚本26、Rust普通/candidate工作区各432项（应用141/core单元110及其余套件）；fmt/两种clippy、typecheck/lint/build、IPC/安全/版本、生产依赖/RustSec策略、serial、dirty replay及300模拟秒/300行soak通过。模板/检验组、导出和浏览器mock保持下一批；尚未启动本地服务。

## 第十批模板与检验组写入审计（2026-10-26）

自定义程序保存/删除、检验组版本保存现在在生命周期边界内使用一次授权身份，覆盖权限/目标版本检查、数据提交与单次终局审计。认证存储和SQLite故障分别记为FAILURE；SQL错误返回稳定STORAGE_ERROR。拒绝运行中、更新和接受关闭期间的自定义程序写入，检验组另保留待持久化保护。标准模板只读、版本乐观并发校验及派生关系保留；检验组仍只追加版本并冻结成员证据。预览与读取不写入、不审计；没有改动统计公式、成员选择或判定口径。

新增6项真实IPC回归：六类主体矩阵和合法版本写入、陈旧/非法/运行中/待更新/接受关闭拒绝、SQLite触发器导致原值和版本完整回滚、认证数据库故障、待保存实验保护。另修正Windows CI暴露的设备关闭测试同步条件：先消费最终快照，再等待watch sender明确关闭，避免把transport析构与线程结束的短暂间隔误判为设备未关闭；真实控制流程未改。

本机全面验证：前端89项、Node脚本26项，Rust普通/candidate工作区各438项（应用147、core单元110及其余协议/集成测试）通过；fmt、两种clippy、typecheck/lint/build、IPC/安全/版本门禁、生产依赖审计、RustSec策略、serial编译、dirty replay及300模拟秒/300行mock soak通过。RustSec仍有docs/27所列单一glib 0.18.5到期例外和9项未维护告警，不能表述为零风险。未启动本地服务。

后续只余导出命令终局审计及浏览器mock/native权限和状态一致性的软件批次，再启动本地mock服务走用户流程。真实硬件、协议与新旧系统同批样品验收继续等待现场资料。

## 第十一批报告与数据导出审计（2026-10-26）

单次HTML报告、CSV/TXT/XLSX采样导出和指定检验组版本HTML/JSON导出统一在生命周期锁内检查授权身份、对象归属及更新/关闭状态；只在文件写入后记SUCCESS。越权、未登录、非法格式和生命周期拒绝记REJECTED；SQLite、损坏报告证据及导出目录写入故障记FAILURE并返回稳定错误码，不反射SQL/操作系统诊断。文件打开属于写入成功后的尽力操作，打开失败仍返回已保存路径。导出只读冻结数据，不改实验记录、公式、分组版本或历史内容；文件名中的记录ID作路径组件编码。

新增9项Rust回归，覆盖三命令六主体IPC矩阵、CSV/TXT/XLSX格式、坏分析证据、数据库/认证/文件系统故障、更新/关闭拒绝和路径组件编码。本机前端89项、脚本26项、Rust普通/candidate工作区各447项（应用156、core单元110及其余测试）；fmt/普通与candidate clippy、typecheck/lint/build、IPC/安全/版本门禁、生产依赖审计、RustSec策略、serial、dirty replay和300模拟秒/300行soak通过。RustSec告警和临时例外仍见docs/27。

截至本批，#107远端Actions因仓库预算未能启动；后续#108的Linux/Windows、RustSec与candidate检查也因Actions预算被阻止启动。本机验证不替代远端构建。此批仍未启动本地服务。下批核对浏览器mock与原生权限/状态边界。

## 第十二批浏览器 mock 权限与状态一致性（2026-10-26）

浏览器 mock 对受保护命令统一要求已认证且完成临时密码修改；燃气控制、停止、去皮、阶段操作和正在运行实验的操作权按归属用户/管理员检查。去皮在实验运行中拒绝，避免重置样品质量基准；普通停止及可燃气切断需要登录，紧急保护停机仍保留匿名入口。历史列表要求登录并按owner过滤，详情/删除/归属变更执行权限及目标状态检查。程序版本与分析/报告命令也使用同一必需会话守卫。设备控制限幅、样品计算公式、阶段进度和浏览器内存数据范围未改变。

新增12项浏览器 mock 回归，覆盖匿名、锁定与临时密码、运行中去皮基准、owner/admin控制、历史数据隔离/维护、普通切断与紧急匿名停机分界和非法气路错误码。新增测试先复现权限差异；设备流量夹限和流程现有用例仍通过。该批不触碰Tauri控制逻辑或真实设备，不替代#45要求的完整原生逐命令矩阵。

本机阶段验证：前端 Vitest 101 项、Node脚本26项，Rust普通/candidate工作区各447项；typecheck、lint、build、IPC/安全/版本门禁、生产依赖审计、fmt、两种clippy、serial编译、dirty replay、300模拟秒/300行mock soak及RustSec策略均通过。RustSec保留一项glib临时例外及9项未维护告警，见docs/27。#108所有远端Actions均未启动，GitHub明确报告仓库Actions预算阻止运行；这不属于源码测试失败，也不能算远端通过。未启动本地服务。

## 第十三批更新命令权限与审计（2026-10-26）

更新检查在一次受保护会话快照下执行并对授权拒绝、关闭/安装期间请求、禁用状态和检查成功/离线失败记一条终局审计；更新关闭时仍返回原有状态DTO。用户可检查，安装限管理员。安装前在同一生命周期边界内获取更新租约，拒绝正在运行、待保存、去皮中、重复更新或应用关闭；安装成功审计先于重启，失败映射为稳定错误码且不暴露更新器内部诊断。更新过程仍先请求安全气氛，不更改真实更新包、签名和发布路径。

新增7项Rust回归：检查和安装六主体权限、禁用检查DTO、禁用安装、运行阻断、离线检查失败审计、稳定安装失败和成功审计先于重启。成功/失败闭环测试通过可替换restart回调验证，没有访问更新服务器、下载/安装包或真正重启应用。全面本机验证：前端101项、Node脚本26项，Rust普通/candidate工作区各454项（应用163、core单元110及其他协议/集成测试）；fmt/两种clippy、typecheck/lint/build、IPC/安全/版本门禁、生产依赖审计、RustSec策略、serial、dirty replay及300模拟秒/300行soak通过。RustSec仍有一项glib临时例外及9项未维护警告，见docs/27。现场更新通道、签名和回滚仍待#89交付验收。

远端状态：#109 head `a249245` 和#110代码 head `c3d6050` 的Ubuntu、Windows、两项RustSec及源码SBOM检查均因GitHub Actions预算阻止任务启动而失败，候选bundle跳过；#110随后文档提交 head `cf80a30` 的对应检查仍失败，Ubuntu详情再次明确为预算阻止启动。这些不是代码执行失败，也不能算远端CI通过。各PR的本机全量结果与远端状态分别记录。

## 第十四批配置保存关闭边界与故障审计（2026-10-26）

配置保存现在与应用接受退出、实验控制和更新使用同一生命周期边界；授权身份及权限来自一次会话检查。接受关闭后返回`APP_CLOSING`且不写配置；运行、待保存、更新中使用稳定错误码。过期版本/外部修改归为配置冲突，文件或同步故障用`CONFIG_SAVE_FAILED`并记`FAILURE`，授权SQLite故障使用`STORAGE_ERROR`并记`FAILURE`。原子替换与目录持久性确认语义保持不变。

新增3项Rust回归覆盖关闭后拒绝且不改配置、授权SQLite故障失败审计、配置文件丢失的稳定错误及失败审计；原有管理员/普通用户/锁定权限、运行/待保存/更新阻断、版本冲突、保护ACK失败和配置持久性用例均通过。全面本机验证：前端101项、Node脚本26项，Rust普通/candidate工作区各457项（应用166、core单元110及其他协议/集成测试）；fmt、两种clippy、typecheck/lint/build、IPC/安全/版本门禁、生产依赖审计、RustSec策略、serial编译、dirty replay与300模拟秒/300行soak通过。RustSec仍有docs/27所列glib临时例外及9项未维护警告。PR#110如前所述；PR#111代码head `ebd0940` 的Ubuntu、Windows、两项RustSec及源码SBOM检查均因Actions预算阻止任务启动，候选bundle跳过。不是远端代码测试失败，也不能算CI通过。未启动本地服务。

## 第十五批窗口退出操作者归属（2026-10-26）

窗口`CloseRequested`按用户关闭审计，使用事件发生时的会话快照；无有效会话记`anonymous`。独立`ExitRequested`按系统来源记`system`。退出已被接受时，后续重复通知不再追加记录，因此不会用后到的系统来源覆盖用户来源。安全气氛、停止与保存的退出行为保持不变。

新增3项Rust回归覆盖管理员窗口关闭、匿名窗口关闭、已登录用户下的系统退出，以及用户接受关闭后重复系统通知不改写归属。本机全面验证：前端101项、Node脚本26项，Rust普通/candidate工作区各460项（应用169、core单元110及其他协议/集成测试）；fmt、两种clippy、typecheck/lint/build、IPC/安全/版本门禁、生产依赖审计、RustSec策略、serial、dirty replay及300模拟秒/300行soak通过。RustSec仍有docs/27所列临时glib例外和9项未维护告警；现场退出/断电行为仍需目标设备验收。未启动本地服务。

远端状态：PR#112代码head `a0a585a` 的Ubuntu、Windows、两项RustSec及源码SBOM检查均因GitHub Actions预算阻止任务启动而失败，候选bundle跳过；这些不是远端代码测试失败，也不能算CI通过。

## 第十六批 Rust/TypeScript IPC DTO 单一映射（2026-10-26）

处理 #82：前端 IPC DTO 改由 Rust/Serde 类型生成，`typescript-export` 仅用于类型生成与检查，不进入默认生产构建。新增生成文件新鲜度检查，并将生成 DTO 纳入命令契约门禁；当前覆盖62个已注册命令及59处前端 invoke。新引用但未登记的响应 DTO 会使契约检查失败。仅将本来属于 IPC 的结构类型作为生成源；前端局部实验表单模型仍由 UI 自己维护。设备算法、实验流程、持久化语义和硬件控制未改动。

新增生成文件过期和未登记返回 DTO 的回归。本机全面验证：前端101项、Node脚本27项，Rust普通/candidate工作区各460项（各有1项既有跨进程探测忽略）；另执行类型生成新鲜度检查并通过。`cargo fmt`、普通与candidate clippy、typecheck/lint/build、IPC/安全/版本门禁、生产依赖审计、serial编译、dirty replay、300模拟秒/300行soak及actionlint通过。RustSec仍有docs/27所列glib临时例外及9项未维护告警。PR #113 的 Ubuntu/Windows、两项RustSec和候选SBOM检查在runner启动前被GitHub Actions预算阻止，bundle跳过；这不属于代码执行失败，也不能算远端CI通过。此阶段仍未启动服务。

PR #113：`https://github.com/kevinalliswell/TMH-LPF-900/pull/113`，基于PR #112分支，保持未合并。软件 Issue 推进进入收尾；本地模拟验收结果见下节。真实设备、现场资料、传感器校准及硬件联锁仍须现场验收。

## 本地服务与浏览器 mock 验收（2026-10-26）

`pnpm dev --host 127.0.0.1` 启动成功，`http://127.0.0.1:1420/` 返回HTTP 200。隔离的应用内浏览器完成首次管理员引导、登录、退出、锁定/解锁；检查综合控制、历史演示记录、三项标准流程阶段信息、用户管理，以及通讯配置和检验组的桌面原生边界提示。打开实验启动表核对GB/T 13241默认流程和阶段参数后取消，没有启动实验、操作气阀或去皮。屏幕显示持续更新的浏览器合成遥测，所有气路保持OFF。

另以`TMH_DEVICE_MODE=mock`和独立`TMH_DATA_DIR`启动Tauri进程，确认进程初始化了隔离配置与SQLite文件；本机UI控制工具未将该进程暴露为可操作窗口，因此不将这一步计作桌面GUI验收。当前会话未配置Chrome DevTools MCP，浏览器验证使用DOM可访问性树和截图，未采集浏览器控制台或网络面板。应用桌面最小窗口宽度为1024px，而嵌入式浏览器视口约823px；低于桌面最小宽度的预览里控制区显得拥挤，不能代替目标桌面尺寸的布局验收。模拟界面不代表真实设备、真实工艺或传感器结果。

## 公开仓库 Actions 远端验证更新（2026-10-26）

仓库设为 public 后，在 PR #113 head `b7da110b9728faa0d42a0353c974813efff4165c` 重跑 CI (`40312132798`) 和 Candidate installers (`40312132804`)，两条 workflow 均成功。CI 的 Ubuntu/Windows 全套检查及 RustSec；候选构建的 RustSec、源码 SBOM、Windows x64 安装冒烟及 macOS universal bundle/disk image 检查均成功。

候选 Windows x64、macOS universal 安装包、两平台验证记录和源码 SBOM 已作为 Actions artifacts 上传。它们是 PR 验证产物，不是正式 Release。PR #113 仍开放且未合并。

## 第十七批普通用户界面授权可见性回归（2026-10-27）

补强 #45 的前端隔离验收：管理员切换为普通用户后，用户管理导航项和管理页面均不可见；普通用户查看自己的历史记录时，不显示删除按钮或数据归属编辑器，也不会请求管理员用户列表。此前 `AuthGate` 的五种认证状态、自动锁定和紧急安全停机用例已覆盖；本批明确补足用户/删除界面的普通用户断言。没有改变授权实现或实验数据行为，命令层仍由 Rust 强制权限。

本批不宣称硬件故障矩阵完成。设备断线、真实 ACK/保护气氛、掉电恢复及校内试点仍需设备型号/协议资料和现场验证；对应缺口继续留在 #19/#45/#79/#80/#81/#85。

## 第十八批认证、版本与验收文档一致性（#48，2026-10-27）

校正首启流程为创建管理员（无默认账户/口令），把历史删除权限表述为管理员角色加二次确认，并说明串口枚举/连接试读尚未实现型号适配。统一 docs/09 与 docs/11 的现场步骤，给 docs/10 增加逐项证据记录模板，补认证规格的自动化证据链接。CHANGELOG区分已发布 v0.2.0 历史限制和当前软件状态，明确M3/M4/M5实现描述不表示完整里程碑验收；docs/12 与 CHANGELOG 将 `v0.2.1` 标记为未发布的软件修复目标，真实设备集成留在 `v0.3.x`，本批不改包版本。更新 `tasks/todo.md`，软件PR和现场资料状态分开追踪。

本机验证运行于 macOS arm64、Node `v26.5.0`、pnpm `11.10.0`、Rust `1.95.0`（代码基线为 PR #114 head `1c378ddfa18f2a06405e51831563eb623cf2f0b1`）：前端 Vitest 29 个文件、102项通过；脚本测试27项通过；Rust普通和candidate工作区各460项通过、各1项既有忽略。`pnpm typecheck/lint/build`、IPC契约与生成类型、Tauri安全、版本同步、生产依赖审计、`cargo fmt`、普通/candidate clippy、serial feature 编译、dirty replay及300模拟秒/300行mock soak均通过。RustSec以官方数据库当前提交 `3279c99911354c400e1d41e47c7d60e39258b147` 扫描通过：1项精确版本的临时glib unsound例外、9项未维护警告；未据此宣称零风险。Replay按夹具预期记录2条解析错误和1次超时，soak成功生成300行。

上述均为软件和文档门禁；它们不能替代真实设备协议、同批样品、长周期实验、Windows现场升级回滚及签名验收。目标包仍显示源码版本0.2.0，本批未打tag、未发布、未合并。

PR [#115](https://github.com/kevinalliswell/TMH-LPF-900/pull/115) 已开放并以 PR #114 分支为 base；本机验证通过，远端 CI/candidate 按该 PR 的实时 checks 跟踪。

## 第十九批认证与公开安全命令真实 IPC 授权/审计矩阵（2026-10-27）

为 #45/#83 补齐实际 Tauri invoke 边界回归：管理员用户列表及创建、更新、重置临时密码命令覆盖匿名、普通 owner、其他普通用户、管理员、锁定和待改密六种状态；账户变更拒绝返回稳定错误 DTO、审计一次且数据不变。另通过真实 IPC 覆盖首次管理员创建、重复初始化拒绝、登录成功/失败、登出、密码修改及认证状态 DTO；确认公开用户 DTO 和审计不泄露密码或哈希。补测未登录即可读取且带匿名审计的安全状态查询，以及匿名紧急停机成功、停止标记与实验审计归属。后续清点发现运行时/气体限制/设备健康、实验状态/预检/起始默认值、方案列表、更新状态及遥测订阅仍缺真实 IPC 主体覆盖，现以匿名、owner、其他用户、管理员、锁定和待改密六状态补齐。回归还发现 `generate_sample_id` 与 `get_experiment_start_defaults` 在身份校验前查询方案；现先认证再读取方案，避免无效参数掩盖认证错误。相关 AppHandle 命令仅泛型化以支持 MockRuntime，不改变生产运行时逻辑。

本机最终验证运行于 macOS arm64、Node `v26.5.0`、pnpm `11.10.0`、Rust `1.95.0`，PR #115 head `040ce88793ff1a498534b5580748217d8799e198` 为基线：前端102项、脚本27项；Rust普通/candidate各467项通过、各1项既有忽略。类型/lint/build、IPC契约与生成类型、Tauri安全、版本同步、生产依赖审计、RustSec、fmt、两种clippy、serial编译、dirty replay及300模拟秒/300行mock soak全部通过。该批除身份校验顺序外不改变权限策略或实验/分析结果；匿名紧急停机仍由设备回执决定，MockRuntime 测试不代表真机联锁验收。

PR [#116](https://github.com/kevinalliswell/TMH-LPF-900/pull/116) 已开放，以 PR #115 分支为 base；本机全量验证通过，跨平台 CI/candidate 以该 PR 最新 head checks 为准。

## 第二十批公开仓库 updater 端点与发布文档校正（#23/#89，2026-10-27）

仓库 owner 已将源码仓库设为 public。匿名请求读取 `https://github.com/kevinalliswell/TMH-LPF-900/releases/latest/download/latest.json` 成功，返回已发布 `v0.2.0` 清单；旧配置中的 `TMH-LPF-900-releases` 仓库由 GitHub API 返回404。修改 updater 端点指向可达的公开 Releases，并同步 CHANGELOG、docs/01、09、11、28 与本索引。已发布 `v0.2.0` 包仍保存旧地址，必须手动安装包含修复的后续版本才能恢复应用内更新；既有tag/资产不覆盖。更新检查仍默认关闭；本批不创建版本、不发布资产、不声称已通过安装端到端更新/回滚。

PR [#117](https://github.com/kevinalliswell/TMH-LPF-900/pull/117) 以 PR #116 为 base，保持 open；测试和远端构建状态以该 PR 最新 head 为准。

PR [#118](https://github.com/kevinalliswell/TMH-LPF-900/pull/118) 记录启用的 main 分支保护和单维护者导致的审阅阻塞，以 PR #117 为 base，保持 open。第二位维护者与正式发布责任仍待落实。

#118 本机全面验证：Node脚本28项、前端102项、Rust普通/candidate工作区各467项（含1项既有忽略）通过；typecheck、lint、build、Tauri安全、IPC契约与生成类型、生产依赖审计、版本同步、fmt、普通/candidate clippy、serial编译、dirty replay和300模拟秒/300行soak均通过。仅文档变更；GitHub CI/candidate状态以PR checks为准。

PR [#119](https://github.com/kevinalliswell/TMH-LPF-900/pull/119) 记录并审计 v* tag 创建/不可变规则，基于 PR #118，保持 open；未创建或移动任何 tag。

## 第二十一批现场输入模板（2026-10-27）

新增[现场资料填写模板](29-现场资料填写模板.md)，覆盖设备/串口参数、原始 TX/RX 帧、设备联调证据、同批新旧系统样品结果和Windows工控机安装/回滚。模板不猜测协议寄存器、样本数或实验阈值，要求引用厂家手册/批准SOP并保留原始附件；不收集密码或签名私钥。

PR [#120](https://github.com/kevinalliswell/TMH-LPF-900/pull/120) 以 PR #119 为 base，保持 open。本机全量门禁通过：Node脚本28项、前端102项、Rust普通/candidate工作区各467项（各1项既有忽略）、typecheck/lint/build、Tauri安全、IPC契约/类型、生产审计、版本同步、fmt、两套clippy、serial编译、dirty replay及模拟soak。

## 第二十二批通讯配置保存二次确认（#44，2026-10-27）

管理员填写修改原因并提交后，页面先展示待保存范围与原因；只有再次确认才调用保存命令。确认阶段冻结表单，取消不会写入；后端现有管理员授权、运行/保存期拒绝、配置校验、审计、版本冲突与原子写入规则保持不变。配置在重启前不生效，重启仍另有独立保护气氛确认。真机设备总线仍由完整串口集成和现场输入验收。

PR [#121](https://github.com/kevinalliswell/TMH-LPF-900/pull/121) 以 PR #120 为 base。macOS arm64 本机验证：前端 29 个文件/103 项、Node 脚本 28 项、Rust 普通和 candidate 工作区各 467 项（各含 1 项既有忽略）通过；typecheck/lint/build、Tauri安全、IPC契约/类型、版本同步、生产依赖审计、RustSec、fmt、两套clippy、serial编译、dirty replay和300模拟秒/300行mock soak均通过。当前RustSec报告仍有1项已评审且限期的glib 0.18.5 unsound例外和10项未维护警告，不据此宣称风险清零。远端CI及候选安装包checks以该PR当前head为准。
