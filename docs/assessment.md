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
- 任务详情弹窗（TaskDetailModal）：请求 ID、失败原因、完整响应部件（图片/文本/思考/引用/搜索建议/用量）。

## 三、待完成 / 需补全（对照 plan.md）

| 功能 | 计划来源 | 状态 |
|---|---|---|
| 历史页：继续编辑（Gemini 多轮）、原始响应查看、清理供应商远程文件 | plan.md §界面与参数 | 部分（基础过滤+重跑已有；高级能力待补） |
| Gemini 交错响应（文字+图片+Thought+引用同屏；搜索建议隔离沙箱） | plan.md §界面与参数 | TaskDetailModal 框架已有，待完善 |
| xAI `<IMAGE_n>` 提示词插入、参考图拖拽排序 | plan.md §界面与参数 | 未实现 |
| 内置蒙版编辑器（画笔/橡皮/缩放/平移/反转/清空/外部导入） | plan.md §界面与参数 | 已实现（`MaskCanvas`，创作页 mask 模式挂载） |
| 测试覆盖：Mock HTTP（SSE/后台轮询/Batch/限流/审核/临时 URL 下载）、Playwright 多分辨率 | plan.md §验证与交付 | 部分（7 个测试文件，19 个用例；覆盖不全） |

## 四、已修复（2026-07-16）

### 下载按钮无效 ✅
- 修复 `api.exportAsset`：`save()` 调用前先 `getCurrentWindow().setFocus()`，解决 Windows 对话框被窗口遮挡问题。
- `onDownload` 处理器增加 `filePath` 空值防御，缺本地文件时显示明确提示。
- `export_asset` Rust 命令在路径校验失败时补充 `tracing::warn` 诊断日志。

### 错误提示不可见 ✅
- `backendError` 由仅显示通用 tooltip 的角标改为显示完整错误文本的可关闭按钮，支持点击关闭。

### i18n 硬编码字符串 ✅
- 历史页表头（Preview/Status/Date/image count）、预设卡片 More 按钮、项目模态 Color 字段、任务详情 Date 行、远程任务 Status 列、用量 Image tokens — 全部改用 i18n 键。
- 新增键：`preview`、`date`、`last30Days`、`imageUnit`、`more`、`color`、`imageTokens`、`dismissError`、`exportFailed`、`noLocalFile`。

## 五、建议下一步优先级

1. 历史页：原始响应查看、供应商远程文件清理按钮。
2. xAI 参考图排序 + `<IMAGE_n>` 插入。
3. 补充 Rust 集成测试（Mock HTTP 覆盖 SSE 流式、Batch 轮询、限流重试）。
4. Gemini 交错响应渲染完善（TaskDetailModal 框架已有，需 CSS 样式和沙箱 iframe 测试）。
