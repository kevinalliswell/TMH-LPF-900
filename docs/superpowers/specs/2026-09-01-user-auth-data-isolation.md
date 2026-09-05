# Spec: 用户登录、用户管理与实验数据隔离

## Objective

为单机离线 Tauri 应用增加由 Rust/SQLite 强制执行的身份认证与 RBAC。首次启动创建管理员；普通用户只能访问自己创建的实验，管理员可管理所有用户和实验。未登录时仅保留进入安全态的紧急停机能力。

## Decisions

- 隔离边界是应用 UI 与 Rust IPC，不加密 SQLite，也不抵御同一 OS 账户直接读取数据文件。
- 账户由管理员创建，无自助注册；用户只软停用，不物理删除。
- 普通用户可创建、查看、分析、报告和导出自己的实验，不能删除；管理员可处理全部数据并调整归属。
- 旧实验迁移后保持无归属，仅管理员可见；重新分配必须审计，历史 `operator` 快照不改写。
- 实验归启动者；运行期间只有启动者和管理员可控制。其他普通用户只看到设备占用，管理员可接管。
- 会话仅存在 Rust 内存，空闲 15 分钟锁定；实验在锁定或登出后继续运行。
- 未登录可以调用唯一的 `emergency_safe_shutdown`，它只能停止实验并应用保护气氛/降温。

## Contracts

- `UserRole`: `ADMIN | USER`。
- `AuthState`: `SETUP_REQUIRED | ANONYMOUS | AUTHENTICATED | PASSWORD_CHANGE_REQUIRED | LOCKED`。
- 新增认证命令：`get_auth_state`、`bootstrap_admin`、`login`、`logout`、`touch_session`、`change_own_password`。
- 新增管理命令：`list_users`、`create_user`、`update_user`、`reset_user_password`、`set_experiment_owner`。
- 新增公开安全命令：`get_public_safety_status`、`emergency_safe_shutdown`。
- 稳定错误码：`AUTH_REQUIRED`、`AUTH_FAILED`、`ACCOUNT_LOCKED`、`ACCOUNT_DISABLED`、`SESSION_EXPIRED`、`PASSWORD_CHANGE_REQUIRED`、`FORBIDDEN`、`USERNAME_EXISTS`、`LAST_ADMIN`。
- `ExperimentParams` 不再接收可伪造的 `operator`；实验摘要/详情包含可空 owner 字段；运行状态包含 `can_control` 和 `occupied_by_other`。

## Storage and Security

- SQLite schema v2 新增 `users`，为 `experiments` 增加可空 `owner_user_id` 外键和索引，v1 数据无损迁移。
- 登录名为 3–32 位 ASCII 字母、数字、点、下划线或连字符，按小写唯一；显示名为 1–64 位 Unicode。
- 密码 10–128 字符，使用 Argon2id PHC 字符串和随机盐；任何日志/错误不得包含密码或哈希。
- 连续 5 次失败锁定 15 分钟；成功登录清零。管理员创建/重置密码后，目标用户下次登录必须改密。
- 禁止停用或降级当前管理员及最后一个有效管理员。
- 每个受保护的 Tauri 命令在 Rust 侧检查会话、用户启用状态、角色、实验所有权和会话期限。

## Acceptance

- v1 数据库升级到 v2 后实验、采样均保留，旧实验只对管理员可见。
- 直接调用 IPC 不能越过列表、详情、采样、分析、报告、导出、删除或运行控制权限。
- 普通用户 A 永远不能读取普通用户 B 的实验；管理员可以访问两者及无归属数据。
- 会话锁定后受保护命令失败，但紧急安全停机成功且被审计。
- Rust、前端、构建、安全检查、生产依赖审计、serial 编译和 headless soak 全部通过。

