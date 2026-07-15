# ImageWorkbench 项目评估

评估日期：2026-07-16
评估依据：`docs/plan.md`、`docs/needs.md`、`docs/provider-capabilities.md` 与当前代码库实现。

## 一、总体结论

项目已完成从空仓库到可运行桌面应用的核心搭建，架构、存储、安全、供应商适配与生成管道均已落地，属于**核心功能完整、外围功能待补全**的阶段。计划文档中列出的三家供应商能力覆盖、项目/记录系统、通用描述、生成预设、队列管理均已实现；尚未完成的主要集中在历史页高级功能、蒙版编辑器、Gemini 交错响应展示、以及测试覆盖。

## 二、已实现（对照 plan.md）

### 架构与基础设施
- Tauri 2 + SolidJS 双进程架构，Specta 自动生成类型安全命令桥（`src/bindings.ts`）。
- SQLite 双层存储：全局库（供应商、设置、最近项目）+ 每项目独立便携库（`.imageworkbench/project.sqlite3`）。
- OS keyring 密钥存储 + 请求/响应脱敏（`redact_json`）；快照与日志中不含明文密钥。
- 路径安全校验（`validate_open_project_path`，防路径遍历）。
- 双运行时模式（Tauri 桌面 + 浏览器 demo），UI 可脱离后端运行测试。
- 完整中英 i18n。

### 核心功能
- 三家供应商（OpenAI、xAI、Gemini）+ OpenAI 兼容适配器，统一 `ProviderAdapter` trait。
- 版本化能力注册表（`CAPABILITY_REGISTRY`，版本 2026-07-12），驱动动态表单。
- 项目系统：便携目录结构、完整 SQLite 迁移。
- 供应商管理 UI（`ProviderModal`：增删改、连接测试、模型同步）。
- 生成管道（`runtime/executor.rs`）：Realtime / Background / ProviderBatch 三种执行模式。
- 实时流式事件（`generation-event`，含 partial images / checkpoints）。
- 队列管理：按供应商实例并发限流、暂停/取消、指数退避、单任务重试。
- 通用描述：多条、排序、启停、前置/后置。
- 生成预设：创建/应用，参数优先级链（当前表单 > 预设 > 项目默认 > 模型默认）。
- 创作页参数区：基础区 + 按能力动态展示的高级区。
- 结果区“打开文件夹”按钮（已修复）。

### 当前活跃开发中（未提交改动）
- `TaskDetailModal`（新增 `.test.tsx`，功能迭代中）。
- `bridge.rs` / `domain/types.rs` / `providers/types.rs` / `runtime/executor.rs`（提供商能力与执行层）。
- `CreatorPage` / `ProviderModal`（UI 调整）。

## 三、待完成 / 需补全（对照 plan.md）

| 功能 | 计划来源 | 状态 |
|---|---|---|
| 历史页：筛选、对比、继续编辑、显示原始响应、清理远程文件 | plan.md §界面与参数 | 部分（基础历史页已有，高级能力待补） |
| Gemini 交错响应（文字+图片+Thought+引用同屏；搜索建议隔离沙箱视图） | plan.md §界面与参数 | 未实现 |
| xAI `<IMAGE_n>` 提示词插入、参考图拖拽排序 | plan.md §界面与参数 | 未实现 |
| 内置蒙版编辑器（画笔/橡皮/缩放/平移/反转/清空/外部导入） | plan.md §界面与参数 | 待确认 |
| 测试覆盖：Mock HTTP（SSE/后台轮询/Batch/限流/审核/临时 URL 下载）、组件测试、Playwright 多分辨率 | plan.md §验证与交付 | 部分（存在若干单测，覆盖不全） |
| 错误提示 UI | 当前 bug | 需改进（见下） |

## 四、已知 Bug

### 下载按钮无效

调用链：`onDownload` → `api.exportAsset(sourcePath, suggestedName, asset.url)` → `plugin-dialog.save()` → `invoke("export_asset")` → `validate_open_project_path` → `tokio::fs::copy`。

**根因分析（按可能性排序）：**
1. Windows 上 `save()` 保存对话框被主窗口遮挡，用户未察觉，返回 `null` 后静默 `return false`。
2. `validate_open_project_path` 校验失败（路径分隔符 `/` vs `\` 差异等），Rust 抛 `CommandError`。
3. 远端存储资产（xAI Files / Gemini Files）`filePath` 为空，触发 `export source is not a file`。

**放大问题**：当前 `backendError` 仅渲染为顶栏一个带 `title` tooltip 的图标徽章（显示 `backendUnavailable`），用户看不到具体错误，因此所有失败都表现为“无作用”。

**修复计划：**
1. 错误可见性：将 `backendError` 改为可关闭的行内错误条 / toast，显示完整错误信息（对所有操作生效）。
2. Windows 对话框焦点：`save()` 前将窗口提到前台（`getCurrentWindow().setFocus()`）。
3. 防御 `filePath` 为空：下载前校验，缺本地文件时明确提示。
4. Rust 端在 `export_asset` 校验失败时补充诊断日志。

## 五、建议优先级

1. 立即：修复下载按钮（先做错误可见性，以确认真实失败点）。
2. 近期：改善全局错误报告 UI。
3. 中期：历史页高级功能（筛选/继续编辑/原始响应）。
4. 中期：蒙版编辑器。
5. 后期：Gemini 交错响应与搜索建议隔离视图（最复杂）。
6. 持续：补齐测试覆盖。
