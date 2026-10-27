# Issue 推进清单

> 分批实现、测试和 PR 记录见 [docs/26](../docs/26-Issue推进与验收索引.md)；GitHub PR 状态以仓库为准。用户要求 PR 保持未合并。

## 当前软件批次

- [x] PR #114：普通用户界面授权可见性回归；本机综合测试及 Ubuntu/Windows CI、RustSec、SBOM、Windows/macOS candidate 检查通过，保持 open。
- [x] PR #115：修正认证/授权文档漂移，核对 Unreleased 目标版本与现场限制；本机全面验证通过，PR 已创建并保持 open，远端 CI/candidate 以 PR checks 跟踪。
- [x] PR #116：首启/登录/改密、账户管理、公开安全命令和其余受保护只读/遥测入口真实 IPC 授权回归；修复实验起始查询先读方案后认证的顺序；普通/candidate Rust、前端与全量门禁通过，PR 保持 open，远端 checks 跟踪最新 head。
- [x] PR #117：按仓库 public 状态把 updater 端点改为当前公开 Releases，删除不存在的独立仓库占位，并同步分发限制文档；本机全量门禁通过，远端 checks 随 PR 跟踪。
- [x] PR #118：启用并回读验证 main 的 PR/审批/CI 保护及禁强推/删除规则，记录只存在一位直接维护者造成的合并阻塞；远端 checks 随 PR 跟踪。
- [x] PR #119：为 `v*` tag 启用管理员创建规则和无例外的更新/删除保护，API 回读验证且未修改现有 tag；远端 checks 随 PR 跟踪。
- [x] PR #120：补充可填写的现场联调、样品对照和部署验收资料模板，避免猜测厂家协议和实验阈值；本机全量门禁通过，远端 checks 随 PR 跟踪。
- [x] PR [#121](https://github.com/kevinalliswell/TMH-LPF-900/pull/121)：通讯配置保存二次确认；本机全量门禁及远端 CI/candidate 检查通过，保持 open。
- [x] PR [#122](https://github.com/kevinalliswell/TMH-LPF-900/pull/122)：登录失败与账户锁定审计回归；本机全量门禁及远端 CI/candidate 检查通过，保持 open。
- [x] PR [#123](https://github.com/kevinalliswell/TMH-LPF-900/pull/123)：通讯配置 IPC 六主体矩阵与审计回归；本机全量门禁及远端 CI/candidate 检查通过，保持 open。
- [x] PR [#124](https://github.com/kevinalliswell/TMH-LPF-900/pull/124)：配置保护重启入口 IPC 六状态回归；本机全量门禁及远端 CI/candidate 检查通过，保持 open。
- [x] PR [#125](https://github.com/kevinalliswell/TMH-LPF-900/pull/125)：剩余受保护 IPC 六主体矩阵；跨平台检查发现三处测试快照时序问题，已由 `5dc56c2`/`5d7ed41`/`4b3436d` 修正；普通/candidate 全工作区本机复测通过，最新 CI/candidate 检查运行中，保持 open。
- [ ] 本地浏览器 mock 的首管理员/普通用户工作流：服务已运行；首次设置凭据和创建测试用户由维护者在本地页面完成，不共享密码。
- [x] PR [#126](https://github.com/kevinalliswell/TMH-LPF-900/pull/126)：为 `docs/10` 勾选项加非空验收证据 CI 门禁，并同步批次跟踪；本机全面门禁通过，跨平台 CI/candidate 检查运行中，保持 open。
- [x] PR [#127](https://github.com/kevinalliswell/TMH-LPF-900/pull/127)：为公开 `get_app_info` 增加匿名真实 IPC 回归，精确核对包名和 Cargo 版本；本机全面验证通过，远端检查随最新 head 跟踪，保持 open。
- [x] PR [#128](https://github.com/kevinalliswell/TMH-LPF-900/pull/128)：按 #88 将 CI mock soak 扩展至24小时逻辑时间（86,400条样本）；本机全面门禁通过，远端 CI/candidate 检查运行中，保持 open。

## 现场输入与验收

- [ ] #19/#79/#80/#81：设备型号、通讯手册、接线/端口参数、协议抓包、探针 ACK、断线与安全气氛现场记录。
- [ ] #28：由厂家手册确认独立 ON/OFF 寄存器语义，并做真机设定值关闭/恢复验证。
- [ ] #42/#43/#86：同批样品数据、实机长周期计时记录、新旧系统对照和实验室签字。
- [ ] #44/#87：目标设备参数重启核验、Windows 离线工控机安装/升级/回滚和备份恢复。
- [ ] #23/#89：签名/公证凭据与负责人、正式发版、匿名更新链路、长期资产和回滚证据。

自动化测试或候选安装包通过不等于以上现场条目完成；不得据此声明真实设备可用或现场合格。
