# Implementation Plan: 用户认证与实验数据隔离

## Architecture

1. `tmh-core` 提供用户模型、密码与 SQLite v2 数据访问，不依赖 Tauri。
2. `src-tauri` 保存单实例内存会话并集中实施命令授权、实验所有权和审计。
3. React 通过 typed IPC 展示初始化、登录、锁定、用户管理和按角色裁剪后的实验视图。
4. 浏览器 mock 镜像相同契约，生产安全边界始终在 Rust。

## Delivery order

1. 数据库迁移、账户模型和密码测试。
2. 会话状态、认证命令和权限矩阵。
3. 实验 owner、历史查询与运行控制授权。
4. 前端认证门、用户管理和隔离 UI。
5. mock、文档、全量门禁和审查。

## Risks

- 迁移失败：在事务内逐版本迁移，并以真实 v1 临时库测试数据保留。
- IPC 绕过：所有授权集中在 Rust helper，前端隐藏仅改善体验。
- 安全与可用性冲突：紧急命令单独 allowlist，只允许进入已定义保护态。
- 运行中换用户：owner 固化在实验记录，管理员可控制但不改写 owner。

