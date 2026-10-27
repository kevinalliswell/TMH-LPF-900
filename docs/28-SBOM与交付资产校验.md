# 28 SBOM 与交付资产校验（2026-10-25）

关联 #23/#89。此批补全source SBOM、候选包完整校验和正式草稿资产清单；不创建tag、release或执行发布，不代表现场验收完成。

## 源码依赖清单的范围

`source-sbom.yml`固定使用Syft 1.55.0，并在解压执行前验证Linux工具压缩包的硬编码SHA256。只将`Cargo.lock`、`pnpm-lock.yaml`、`Cargo.toml`、`package.json`复制到临时扫描目录；不扫描工作区用户数据、真实设备会话、环境文件或凭据。禁用工具自身更新检查。

生成CycloneDX 1.7 JSON后，检查工具版本、所有Cargo.lock包/版本均存在，以及前端生产依赖有已解析版本与npm PURL。输出记录Git SHA、应用版本、实际工作区dirty标记和四份输入SHA256。当前真实工具扫描识别914个组件；数量随锁文件变化，不写死为验收阈值。

该清单包括全部平台的锁定依赖以及开发/构建依赖，**不是安装包的二进制组件清单**。它不证明每个包均进入产物，也不完整列举SQLite内嵌C代码、WebView2、操作系统库或串口驱动；许可证及现场运行依赖仍需发布维护者核对。生成工具来自[Syft官方版本](https://github.com/anchore/syft/releases/tag/v1.55.0)。

candidate与release分别对同一待构建SHA生成清单。candidate源为PR head，正式源为已通过版本/来源门的tag提交。工具二进制校验和、版本及源码输入记录用于追溯；SBOM包含时间/标识符，不承诺再次生成时逐字节相同。

## 候选包

每个平台交付目录包括：安装包、`source-sbom.cdx.json`、两份原有候选验收文档、本交付校验说明、`manifest.json`、`SHA256SUMS.txt`。保留mock-only、关闭updater、独立数据目录等现有约束。

打包入口检查实际Git检出干净，并要求SBOM的源码SHA、版本、dirty=false与当前检出一致；仅伪造一个“clean”属性不能掩盖之后未提交的修改。源码不符、缺少/多个安装包均拒绝。输出目录必须为空，不能混入上一次构建的文件。

`manifest.files`记录全部交付payload的文件名、字节数、SHA256，覆盖安装包、SBOM和文档。`SHA256SUMS.txt`进一步覆盖manifest本身；checksum文件不包含自身，manifest不包含自己，避免自引用。摘要以流式方式计算，不整包读入内存；子目录、符号链接和无法安全表示的文件名会拒绝。

本机调用格式变为：

```bash
node scripts/generate-sbom.mjs /absolute/path/to/syft
node scripts/package-candidate.mjs BUNDLE_DIR windows-x64 verification/source-sbom.cdx.json
```

macOS universal使用`macos-universal`。源码有未提交修改时可以生成带dirty=true的开发清单，但不得作为候选/正式包的有效来源。真实安装包仍由candidate工作流构建和冒烟测试。

## 正式草稿资产

两个平台构建成功后，inventory job下载同一run生成的SBOM与tag草稿中的实际资产。发布脚本要求：草稿状态、tag、package版本、latest.json版本一致；Windows x64与macOS两个架构都有updater项；对应HTTPS URL的文件名存在于实际资产，`.sig`文件内容与latest.json一致；另有DMG。缺项或非草稿时失败。

随后复制源码SBOM、CHANGELOG、部署说明、版本治理、issue证据索引、依赖安全基线和本校验说明；生成全资产manifest和SHA256SUMS，再上传至同一草稿。上传前再次检查草稿状态。重复执行不会调用发布操作；source SBOM artifact允许同run重建以支持重跑，最终inventory按run/attempt另存失败或成功证据。临时artifact保留30天，真正发版时需保留草稿资产和对应CI的长期归档。

此步骤核对签名文件与清单的一致性，**不宣称已完成密码学验证、Authenticode、Apple公证或实际更新**。GitHub Draft Release 对匿名客户端不可见；只有已发布资产才可作为公开 updater 端点。公开更新托管、密钥保管、离线安装、升级/回滚和真机验收仍按docs/09、12、26完成。清单内明确保存这些限制。

下载后可在交付目录使用`sha256sum --check SHA256SUMS.txt`或macOS的`shasum -a 256 --check SHA256SUMS.txt`核对所有字节。摘要用于完整性核对，不替代发布者签名和可信分发来源。

## 软件验证

Node脚本回归使用真实临时Git和二进制fixture，验证Windows/macOS两个候选输出、全部交付文件及manifest的摘要、SBOM错来源/版本/dirty、源码后续修改、缺少/多个安装包。正式草稿使用离线CLI fixture核对跨架构updater、签名文件、版本、DMG及全资产校验；不会调用GitHub或生成真实tag/release。

全阶段仍运行普通/candidate完整Rust工作区、fmt/clippy、前端类型/lint/test/build、IPC/安全/版本、依赖审计、serial编译、dirty replay及5分钟mock soak。本机结果和新工作流跨平台结果随关联PR记录，模拟验证不能替代现场结论。
