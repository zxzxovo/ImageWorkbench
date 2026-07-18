import { For } from "solid-js";
import {
  BookOpen,
  FolderKanban,
  History,
  Images,
  Palette,
  PlugZap,
  Settings,
  SlidersHorizontal,
  TextQuote,
  WandSparkles,
  Wrench,
} from "lucide-solid";
import type { Locale } from "../types";

interface HelpSection {
  id: string;
  title: string;
  summary: string;
  steps: string[];
  tip?: string;
  icon: typeof BookOpen;
}

interface HelpDocument {
  title: string;
  subtitle: string;
  contents: string;
  quickStart: string;
  quickStartIntro: string;
  quickSteps: string[];
  tipLabel: string;
  sections: HelpSection[];
}

const helpDocuments: Record<Locale, HelpDocument> = {
  "zh-CN": {
    title: "帮助与使用指南",
    subtitle: "从创建项目到排查错误，了解 ImageWorkbench 的完整工作流程",
    contents: "章节导航",
    quickStart: "快速开始",
    quickStartIntro: "第一次使用时，按下面的顺序即可完成一次图片生成。",
    quickSteps: [
      "创建项目并选择一个用于保存数据库、素材和结果的目录。",
      "在右上角打开供应商管理，点击加号选择模板，填写凭据并启用需要的模型。",
      "返回创作页，选择供应商、模型和模式，在提示词编写区描述目标画面。",
      "点击生成，在任务队列查看进度，并到全部结果或历史记录中管理输出。",
    ],
    tipLabel: "提示",
    sections: [
      {
        id: "projects",
        title: "项目与项目栏",
        summary: "项目把设置、提示词上下文、生成记录和本地文件组织在一个可迁移的目录中。",
        icon: FolderKanban,
        steps: [
          "使用项目栏底部的新建和打开按钮创建项目，或重新打开已有项目目录。",
          "点击项目名称切换当前项目；名称下方的数字表示该项目已经生成的图片数量。",
          "项目右侧菜单可以修改名称、描述和颜色，也可以执行完整复制或仅配置复制。",
          "删除时默认只从应用列表移除项目；勾选文件删除后，才会清理应用管理的数据库、素材和结果。",
        ],
        tip: "添加本地参考素材时，应用会把文件复制到项目的 inputs 目录，因此原文件移动后也不会影响项目。",
      },
      {
        id: "providers",
        title: "供应商管理",
        summary: "为 OpenAI、xAI、Gemini 或兼容网关建立独立连接，并控制实际可用的模型。",
        icon: PlugZap,
        steps: [
          "点击右上角供应商按钮，再点击供应商列表旁的加号，从弹出的模板中选择供应商类型。",
          "填写实例名称、Base URL 和 API Key；自定义网关通常需要选择 OpenAI compatible 模式。",
          "先测试连接，再同步模型。同步到的新模型默认不会启用，需要在模型列表中主动勾选。",
          "高级连接设置可配置代理、超时、鉴权方式、自定义请求头及模型能力覆盖。",
        ],
        tip: "API Key 和标记为敏感的请求头保存在系统凭据存储中，不写入工作区快照。",
      },
      {
        id: "creator",
        title: "创作",
        summary: "创作页用于选择模型能力、编写最终提示词、添加参考素材并发起生成。",
        icon: WandSparkles,
        steps: [
          "先选择供应商和模型，再选择生成、编辑、蒙版、变体或视频参考模式；页面会按模型能力显示可用参数。",
          "直接在右侧提示词编写区域输入内容。启用的通用描述会在提交时与当前提示词组合。",
          "纯生成模式不接受参考图或蒙版；需要参考素材时切换到编辑等受支持的模式，页面下方会给出校验提示。",
          "展开高级选项可设置格式、透明背景、流式返回、远程任务及供应商特有参数，然后点击生成。",
        ],
        tip: "提交前留意提示词下方的校验信息；不受模型支持的参数不会被静默发送。",
      },
      {
        id: "descriptions",
        title: "通用描述",
        summary: "把反复使用的角色、画风、构图和规避词拆成可复用的项目上下文。",
        icon: TextQuote,
        steps: [
          "每条描述可以同时填写前置内容、后置内容和额外的否定词，并可单独启用或停用。",
          "多条描述同时启用时，先按列表顺序组合全部前置内容，再放入当前提示词，最后组合全部后置内容。",
          "否定词同样按列表顺序组合，当前任务填写的否定词位于最后。",
          "使用上移和下移按钮调整稳定顺序；项目设置中的“使用项目描述”控制创作时是否自动应用。",
        ],
      },
      {
        id: "presets",
        title: "预设",
        summary: "保存常用的模型、尺寸、质量和输出组合，减少重复配置。",
        icon: SlidersHorizontal,
        steps: [
          "新建预设后选择供应商、模型和常用参数，并使用清晰的名称标记用途。",
          "在预设卡片上点击应用，会把参数载入创作页；正式生成前仍可继续调整。",
          "模型能力或供应商配置发生变化后，应重新检查旧预设中的尺寸和高级参数。",
        ],
      },
      {
        id: "history",
        title: "历史记录与任务",
        summary: "任务队列展示当前执行状态，历史记录保存可复现的请求快照和响应信息。",
        icon: History,
        steps: [
          "创作页的任务队列显示排队、运行、完成和失败状态；暂停队列只阻止新任务开始，不会终止正在运行的任务。",
          "历史页可按提示词、供应商、模型和状态筛选，并可收藏常用记录。",
          "重新运行使用记录中的参数快照；继续编辑会把提示词、参考素材和参数恢复到创作页。",
          "对比功能可并排检查多条记录；删除失败记录或清空历史前请确认是否仍需其中的诊断信息。",
        ],
      },
      {
        id: "results",
        title: "全部结果",
        summary: "集中查看当前项目的所有生成图片，并在大图卡片和紧凑列表之间切换。",
        icon: Images,
        steps: [
          "使用页面右上角的视图按钮切换大图卡片或列表视图，图片会限制在可用区域内。",
          "每个结果都可以复制到剪贴板、导出到指定位置，或在文件管理器中显示。",
          "进入多选模式后可批量下载或删除图片；删除只影响选中的结果，同一任务中的其他图片和历史记录会保留。",
          "打开项目文件夹可直接查看 inputs、outputs 和 previews 等项目资产目录。",
        ],
        tip: "远程供应商返回但尚未保存到本地的结果可能无法直接导出，详细原因会进入诊断中心。",
      },
      {
        id: "settings",
        title: "项目设置",
        summary: "管理项目资料、默认模型、文件命名和输出保存行为。",
        icon: Settings,
        steps: [
          "项目信息可修改名称、描述和存储路径；更改路径不会自动搬移已有文件，请先确认目录内容已正确迁移。",
          "默认模型和流式模式会作为新任务的起点，创作页中的单次设置可以覆盖它们。",
          "“保存到同一目录”会让所有输出直接进入统一输出目录，而不是按任务 ID 建立子目录。",
          "危险区域可以清空当前项目的生成历史；该操作与从项目栏删除整个项目不同。",
        ],
        tip: "顶部栏还可以切换中英文和明暗主题，这些偏好会随工作区保存。",
      },
      {
        id: "diagnostics",
        title: "诊断与错误排查",
        summary: "当安装包设备、项目数据库或供应商请求出现问题时，从诊断中心收集完整线索。",
        icon: Wrench,
        steps: [
          "点击右上角诊断按钮查看应用版本、平台、数据和日志目录、凭据存储状态及项目数据库状态。",
          "最近错误会记录发生时间、操作上下文、错误代码和消息；后端日志提供更完整的执行过程。",
          "使用复制诊断报告后再反馈问题，可以避免只看到 builder error 或 project not open 等概括提示。",
          "跨设备运行安装包时，先检查项目目录读写权限、凭据是否需要重新录入，以及日志中的首个失败原因。",
        ],
      },
    ],
  },
  "en-US": {
    title: "Help and user guide",
    subtitle: "Learn the complete ImageWorkbench workflow, from creating a project to diagnosing failures",
    contents: "Chapters",
    quickStart: "Quick start",
    quickStartIntro: "Follow these steps to create your first image.",
    quickSteps: [
      "Create a project and choose a folder for its database, media, and results.",
      "Open provider management, select Add, choose a template, enter credentials, and enable the models you need.",
      "Return to Create, choose a provider, model, and mode, then write the desired image in the prompt editor.",
      "Generate the image, watch progress in the task queue, and manage the output in All results or History.",
    ],
    tipLabel: "Tip",
    sections: [
      {
        id: "projects",
        title: "Projects and the project bar",
        summary: "A project keeps settings, prompt context, generation history, and local files together in a portable folder.",
        icon: FolderKanban,
        steps: [
          "Use New project or Open project at the bottom of the project bar to create or reopen a project folder.",
          "Select a project by name. The number below it is the count of generated images in that project.",
          "The project action menu edits its name, description, and color, or creates a full or configuration-only copy.",
          "Delete removes only the app entry by default. App-managed databases and media are removed only when file deletion is selected.",
        ],
        tip: "Local reference media is copied into the project's inputs folder, so moving the original file will not break the project.",
      },
      {
        id: "providers",
        title: "Provider management",
        summary: "Create independent OpenAI, xAI, Gemini, or compatible gateway connections and control the models they expose.",
        icon: PlugZap,
        steps: [
          "Open provider management in the top right, select Add beside the provider list, and choose a template from the popup.",
          "Enter an instance name, Base URL, and API Key. Custom gateways commonly use OpenAI compatible mode.",
          "Test the connection, then sync models. Newly discovered models remain disabled until you explicitly select them.",
          "Advanced settings cover proxy, timeout, authentication, custom headers, and model capability overrides.",
        ],
        tip: "API keys and sensitive headers are stored in the system credential store, not in the workspace snapshot.",
      },
      {
        id: "creator",
        title: "Create",
        summary: "Choose model capabilities, compose the final prompt, add reference media, and submit a generation request.",
        icon: WandSparkles,
        steps: [
          "Choose a provider and model, then select Generate, Edit, Mask, Variation, or Video reference. Available controls follow model capabilities.",
          "Write directly in the prompt editor. Enabled common descriptions are composed with the prompt when the request is submitted.",
          "Generate mode cannot accept reference media or a mask. Switch to a supported edit mode when references are required.",
          "Use Advanced for formats, transparency, streaming, remote tasks, and provider-specific controls, then select Generate.",
        ],
        tip: "Read the validation message below the prompt before submitting. Unsupported parameters are not silently sent.",
      },
      {
        id: "descriptions",
        title: "Common descriptions",
        summary: "Turn recurring characters, styles, compositions, and avoidance terms into reusable project context.",
        icon: TextQuote,
        steps: [
          "Each entry can contain a prefix, suffix, and additional negative terms, and can be enabled independently.",
          "When several entries are enabled, all prefixes are composed in list order, followed by the task prompt, then all suffixes.",
          "Negative terms follow the same list order, with task-specific negative guidance appended last.",
          "Use Move up and Move down for deterministic ordering. Use project settings to control whether descriptions apply automatically.",
        ],
      },
      {
        id: "presets",
        title: "Presets",
        summary: "Save common model, size, quality, and output combinations to avoid repetitive setup.",
        icon: SlidersHorizontal,
        steps: [
          "Create a preset, choose its provider, model, and common parameters, and give it a purpose-specific name.",
          "Apply a preset to load it into Create. You can still adjust individual values before generating.",
          "Review older presets after model capabilities or provider settings change.",
        ],
      },
      {
        id: "history",
        title: "History and tasks",
        summary: "The task queue shows live execution, while History keeps reproducible request snapshots and response details.",
        icon: History,
        steps: [
          "The task queue shows queued, running, completed, and failed work. Pausing prevents new work from starting but does not stop in-flight tasks.",
          "Filter History by prompt, provider, model, and status, and favorite records you use often.",
          "Run again uses the saved parameter snapshot. Continue editing restores the prompt, references, and controls to Create.",
          "Compare records side by side. Preserve diagnostic information before deleting failed records or clearing history.",
        ],
      },
      {
        id: "results",
        title: "All results",
        summary: "Browse every generated image in the current project using large cards or a compact list.",
        icon: Images,
        steps: [
          "Switch between large-card and list views with the controls in the page header. Images remain bounded by the available area.",
          "Copy an image to the clipboard, export it to another location, or reveal its local file.",
          "Use selection mode to download or delete several images. Deletion affects only selected results and keeps sibling images and the history record.",
          "Open the project folder to inspect project assets such as inputs, outputs, and previews.",
        ],
        tip: "A remote result that has not been saved locally may not be exportable; the exact reason is recorded in Diagnostics.",
      },
      {
        id: "settings",
        title: "Project settings",
        summary: "Manage project information, default models, file naming, and output storage behavior.",
        icon: Settings,
        steps: [
          "Edit the name, description, and storage path. Changing the path does not move existing files, so migrate them first.",
          "The default model and streaming mode initialize new tasks; one-off Create settings can override them.",
          "Save to single directory places every result directly in the shared output folder instead of run-ID subfolders.",
          "The danger zone clears generation history for the current project; it is different from deleting the whole project.",
        ],
        tip: "Language and light/dark appearance are available in the top bar and persist with the workspace.",
      },
      {
        id: "diagnostics",
        title: "Diagnostics and troubleshooting",
        summary: "Collect actionable evidence when an installed build, project database, or provider request fails.",
        icon: Wrench,
        steps: [
          "Open Diagnostics in the top right to inspect version, platform, data and log folders, credential status, and project database health.",
          "Recent errors include time, operation context, code, and message. Backend logs provide the longer execution sequence.",
          "Copy the diagnostic report before reporting a problem instead of relying on summaries such as builder error or project not open.",
          "On another device, check project-folder permissions, re-enter credentials when needed, and start with the first failure in the log.",
        ],
      },
    ],
  },
};

export default function HelpPage(props: { locale: Locale }) {
  const document = () => helpDocuments[props.locale];

  return (
    <div class="page help-page">
      <header class="help-hero">
        <span class="help-hero-icon"><BookOpen size={25} /></span>
        <div>
          <h1>{document().title}</h1>
          <p>{document().subtitle}</p>
        </div>
        <Palette class="help-hero-mark" size={54} />
      </header>

      <div class="help-layout">
        <aside class="help-toc" aria-label={document().contents}>
          <strong>{document().contents}</strong>
          <a href="#help-quick-start"><BookOpen size={14} />{document().quickStart}</a>
          <For each={document().sections}>
            {(section) => {
              const SectionIcon = section.icon;
              return <a href={`#help-${section.id}`}><SectionIcon size={14} />{section.title}</a>;
            }}
          </For>
        </aside>

        <main class="help-content">
          <section class="help-quick-start" id="help-quick-start">
            <div class="help-section-heading">
              <span><BookOpen size={18} /></span>
              <div><h2>{document().quickStart}</h2><p>{document().quickStartIntro}</p></div>
            </div>
            <ol class="help-steps help-quick-steps">
              <For each={document().quickSteps}>{(step) => <li>{step}</li>}</For>
            </ol>
          </section>

          <For each={document().sections}>
            {(section) => {
              const SectionIcon = section.icon;
              return (
                <article class="help-section" id={`help-${section.id}`}>
                  <div class="help-section-heading">
                    <span><SectionIcon size={18} /></span>
                    <div><h2>{section.title}</h2><p>{section.summary}</p></div>
                  </div>
                  <ol class="help-steps">
                    <For each={section.steps}>{(step) => <li>{step}</li>}</For>
                  </ol>
                  <For each={section.tip ? [section.tip] : []}>
                    {(tip) => <p class="help-tip"><strong>{document().tipLabel}</strong><span>{tip}</span></p>}
                  </For>
                </article>
              );
            }}
          </For>
        </main>
      </div>
    </div>
  );
}
