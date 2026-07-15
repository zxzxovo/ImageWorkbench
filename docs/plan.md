# ImageWorkbench 跨平台生图桌面程序

## 总结

从空仓库构建本地优先的桌面应用，技术栈为 Rust 2024、Tauri 2、Bun、SolidJS、TypeScript 和 SQLite。首版支持 Windows、macOS、Linux，完整覆盖 OpenAI、xAI Grok、Google Gemini 的图片生成相关能力，并提供 OpenAI Images-compatible 自定义供应商模板。

文档能力快照日期为 2026-07-12：

| 引擎 | 首版覆盖能力 |
|---|---|
| OpenAI | Image API 生成、编辑、多参考图、蒙版、DALL·E 2 变体；Responses API 多轮编辑、文件输入、流式局部图、后台任务和 Batch；质量、尺寸、格式、压缩、背景、审核等模型特有参数。[生成接口](https://developers.openai.com/api/reference/resources/images/methods/generate) [图片指南](https://developers.openai.com/api/docs/guides/image-generation) [编辑接口](https://developers.openai.com/api/reference/resources/images/methods/edit) |
| xAI | 生成、单图/最多三图编辑、比例、1K/2K、1–10 张、URL/Base64、Files 持久化与公共 URL、官方 Batch；不虚构蒙版、流式或独立质量参数。[Imagine](https://docs.x.ai/developers/model-capabilities/imagine) [REST 图片接口](https://docs.x.ai/developers/rest-api-reference/inference/images) |
| Gemini | Gemini 3.1 Flash/Lite、3 Pro、2.5 Flash Image；生成、编辑、多轮、最多 14 张参考图、交错文图、Thinking、Web/Image Search、Video-to-Image、Files、流式、后台和 generateContent Batch。[图片指南](https://ai.google.dev/gemini-api/docs/image-generation) [Interactions API](https://ai.google.dev/api/interactions-api) [Batch API](https://ai.google.dev/gemini-api/docs/batch-api) |

按已确认范围，不实现即将于 2026-08-17 停服的 Imagen 4，也不实现视频输出；Gemini 3.1 Flash 的视频参考生图仍纳入。

## 核心实现

- 前端采用 SolidJS、Vite、`@kobalte/core`、`lucide-solid` 和完整中英 i18n；所有网络、文件、数据库和密钥操作只在 Rust 后端执行。
- Rust 使用 `reqwest`、Tokio、Serde、SQLx SQLite、`keyring`、`image`、`tracing`；通过 Specta 生成 Tauri 命令、事件和 TypeScript DTO，避免双份类型。
- 定义 `ProviderAdapter`：`test_connection`、`list_models`、`validate`、`execute`、`submit_batch`、`poll_batch`、`cancel`、`download_asset`。
- 公共类型包括 `ProviderProfile`、`ModelCapability`、`GenerationRequest`、`InputAsset`、`OutputSpec`、`RunEvent`、`OutputPart`、`UsageRecord` 和 `ProviderError`。
- 操作类型固定为 `Generate`、`Edit`、`Variation`、`ConversationContinue`、`VideoReferenceToImage`；执行类型为 `Realtime`、`Background`、`ProviderBatch`。
- 供应商模板包含 OpenAI、xAI、Gemini、OpenAI Images-compatible。每种模板可创建任意多个实例，支持 Base URL、API Key、可选请求头、超时、代理及模板特有字段。
- OpenAI 增加可选 Organization、Project；Gemini 支持 `v1`/`v1beta`；自定义兼容模板可配置鉴权方式、模型路径和受保护字段之外的扩展 JSON。
- API Key 存入系统钥匙串；SQLite、项目文件、日志和导出配置中只保存凭据引用，不保存明文。

模型能力采用三层合并：

1. 官方模型 API 同步当前账号可用 ID。
2. 应用内版本化 capability registry 提供参数、枚举、限制和官方文档链接。
3. 用户覆盖未知模型能力；优先级为用户覆盖 > 精确目录项 > 别名/模式 > 通用协议默认。

不在运行时抓取官网。OpenAI `/models`、xAI `/image-generation-models`、Gemini `/v1beta/models` 只用于发现模型，不能替代能力目录。

## 项目、记录与任务

- 每个项目是便携目录，结构固定为 `.imageworkbench/project.sqlite3`、`assets/inputs`、`assets/outputs/<date>/<run-id>`、`assets/previews`。
- 导入的本地文件和远程输入默认复制到项目内并计算 SHA-256；输出先写临时文件，再原子重命名。
- 全局数据库只保存供应商、应用设置、最近项目和项目路径；项目迁移到其他机器后通过“供应商重映射”恢复执行环境。
- 项目数据库保存上下文描述、生成预设、会话、运行组、请求任务、输入资产、全部响应部分、引用、用量、错误、远程任务和远程文件。
- 每条记录保存原始提示词、最终拼接提示词、描述快照、预设快照、能力目录版本、脱敏请求/响应、模型版本、请求 ID、费用及文件哈希，可一键重跑或继续编辑。
- 项目通用描述支持多条、排序、启停、前置/后置；生成前显示最终提示词预览。参数优先级固定为“当前表单 > 生成预设 > 项目默认 > 模型默认”。
- 生成数量统一表示期望输出数：OpenAI/xAI 尽量使用原生 `n`，Gemini 或不支持批量的模式拆成多个任务。
- 实时队列按供应商实例限流，默认并发 2；支持暂停、取消、指数退避和单任务重试。
- 同步任务在应用退出后标记为中断并可重试；OpenAI/Gemini 后台任务及三家官方 Batch 保存远程 ID，重启后继续轮询。

## 界面与参数

- 主窗口采用项目侧栏、生成工作区、结果区和属性检查器；标签页为“创作、历史、通用描述、生成预设、项目设置”。
- 基础区固定展示模式、供应商、模型、提示词、参考素材、输出数量、比例/尺寸、质量或分辨率以及生成按钮。
- 默认数量为 1；OpenAI 使用 Auto 尺寸/质量，xAI 使用 Auto 比例与 1K，Gemini 默认 `gemini-3.1-flash-image`、1K、仅图片、minimal Thinking、关闭 Search。
- 高级区按能力动态展示格式、压缩、背景、审核、DALL·E 风格、partial images、Responses action、xAI Files/TTL、Gemini Thinking/Search/输出模态、远程存储、后台和 Batch。
- 参考素材支持本地文件、URL、Base64 和供应商 File ID；Gemini 支持对象/角色/风格标签及视频输入，xAI 支持拖拽排序和 `<IMAGE_n>` 插入。
- 内置 OpenAI 蒙版编辑器，提供画笔、橡皮、缩放、平移、反转、清空和外部 mask 导入；发送前检查尺寸、格式、大小和 alpha 通道。
- Gemini 交错响应必须展示全部文本、图片、Thought 和引用；Image Search 的 `search_suggestions` 在禁脚本、严格白名单的隔离视图中展示。
- 历史页支持筛选、对比、继续编辑、复制参数、显示原始响应、打开输出目录和清理供应商远程文件。

## 验证与交付

- 为提示词合并、参数优先级、能力合并、任务拆分、路径安全、密钥脱敏和数据库迁移编写 Rust 单元测试。
- 使用模拟 HTTP 服务覆盖三家生成、编辑、流式 SSE、后台轮询、Batch、限流、审核拦截、部分成功和临时 URL 下载。
- 覆盖 OpenAI 自定义尺寸与 DALL·E 限制、xAI 三图/TTL 校验、Gemini 参考图额度、Search 限制、20MB inline 上限及 Lite 仅开放稳定 1K。
- Solid 组件测试覆盖动态表单、上下文选择、预设覆盖、蒙版编辑器和记录重放；Playwright 检查 1024×640、1280×800、1440×900 下无重叠或文字溢出。
- 真实 API 测试由环境变量显式启用，不进入普通 CI；CI 默认运行格式检查、Rust/TS 测试、模拟集成测试和三平台打包检查。
- 验收标准：三家全部已纳入的图片模式均可生成本地资产；不发送模型不支持的参数；项目可整体移动；后台任务可恢复；所有记录可复现；日志和项目中不存在明文密钥。

工作名使用 `ImageWorkbench`，应用标识暂定 `dev.imageworkbench.desktop`；首版为单用户、本地优先、无账号、无云同步、无遥测，品牌信息可在发布签名前统一替换。
