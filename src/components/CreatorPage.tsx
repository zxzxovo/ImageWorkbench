import { For, Show, createEffect, createMemo, createSignal, onMount, onCleanup } from "solid-js";
import type { SetStoreFunction } from "solid-js/store";
import {
  AlertTriangle,
  ArrowDownToLine,
  Braces,
  ChevronDown,
  Clock3,
  Copy,
  FileVideo2,
  FolderOpen,
  GripVertical,
  Image as ImageIcon,
  Images,
  Link2,
  LoaderCircle,
  MoreHorizontal,
  KeyRound,
  Paintbrush2,
  Pause,
  Play,
  Plus,
  SlidersHorizontal,
  Sparkles,
  SquarePen,
  WandSparkles,
  X,
} from "lucide-solid";
import type { TranslationKey } from "../lib/i18n";
import { api, formatError } from "../lib/api";
import { getModelCapabilities, getModelsForProvider } from "../lib/models";
import { composePrompt, normalizeDraftForModel, validateGenerationDraft } from "../lib/prompt";
import type {
  GenerationDraft,
  GenerationMode,
  GenerationTask,
  GeneratedAsset,
  HistoryRecord,
  Project,
  ProviderProfile,
  ReferenceAsset,
} from "../types";
import { EmptyState, Field, IconButton, Toggle } from "./common";
import MaskCanvas from "./MaskCanvas";
import TaskDetailModal from "./TaskDetailModal";

interface CreatorPageProps {
  project: Project;
  providers: ProviderProfile[];
  draft: GenerationDraft;
  setDraft: SetStoreFunction<GenerationDraft>;
  tasks: GenerationTask[];
  history: HistoryRecord[];
  queuePaused: boolean;
  queueControlBusy: boolean;
  t: (key: TranslationKey) => string;
  promptOverride?: string;
  onPromptOverrideClear: () => void;
  onGenerate: (composedPrompt: string) => Promise<void>;
  onCancelTask: (taskId: string) => void;
  onToggleQueue: () => void;
  onManageProviders: () => void;
  onError: (error: unknown, context: string) => void;
  onReveal: (path: string) => void;
  onDownload: (asset: GeneratedAsset) => void;
}

const modeIcons: Record<GenerationMode, typeof Sparkles> = {
  generate: Sparkles,
  edit: SquarePen,
  mask: Paintbrush2,
  variation: Copy,
  video: FileVideo2,
};

export default function CreatorPage(props: CreatorPageProps) {
  let referenceInput!: HTMLInputElement;
  let promptInput!: HTMLTextAreaElement;
  const [advancedOpen, setAdvancedOpen] = createSignal(false);
  const [validationMessage, setValidationMessage] = createSignal("");
  const [selectedTask, setSelectedTask] = createSignal<GenerationTask | null>(null);
  const [referenceEntryMode, setReferenceEntryMode] = createSignal<"url" | "base64" | "file-id" | null>(null);
  const [referenceValue, setReferenceValue] = createSignal("");
  const [referenceMimeType, setReferenceMimeType] = createSignal("image/png");
  const [draggedReferenceId, setDraggedReferenceId] = createSignal("");
  const enabledProviders = createMemo(() => props.providers.filter((provider) => provider.enabled));
  const provider = createMemo(() => enabledProviders().find((item) => item.id === props.draft.providerId) ?? enabledProviders()[0]);
  const capabilities = createMemo(() => getModelCapabilities(provider(), props.draft.model));
  const models = createMemo(() => getModelsForProvider(provider()));
  const continuesConversation = createMemo(() => Boolean(
    props.draft.previousResponseId.trim() || props.draft.previousInteractionId.trim(),
  ));
  const generationInputConflict = createMemo(() => props.draft.mode === "generate"
    && !continuesConversation()
    && (props.draft.references.length > 0 || Boolean(props.draft.maskDataUrl?.trim())));
  const composedPrompt = createMemo(() => props.promptOverride ?? composePrompt(
      props.draft.prompt,
      props.project.descriptions,
      props.project.settings.useCommonDescriptions,
    ));
  const projectTasks = createMemo(() => props.tasks.filter((task) => task.projectId === props.project.id));
  const resultAssets = createMemo(() => props.history
    .filter((record) => record.projectId === props.project.id && record.status === "completed")
    .flatMap((record) => record.assets.map((asset) => ({ asset, record })))
    .slice(0, 8));
  const partialResults = createMemo(() => projectTasks()
    .filter((task) => task.status === "running" || task.status === "queued")
    .flatMap((task) => (task.partialImages ?? []).map((image) => ({ ...image, taskId: task.id }))));

  createEffect(() => {
    props.project.id;
    setAdvancedOpen(false);
    setValidationMessage("");
    setSelectedTask(null);
    setReferenceEntryMode(null);
    setReferenceValue("");
    setDraggedReferenceId("");
  });

  const setProvider = (providerId: string) => {
    const nextProvider = enabledProviders().find((item) => item.id === providerId);
    const nextModel = nextProvider?.models[0] ?? "";
    props.setDraft({ ...normalizeDraftForModel({ ...props.draft, providerId, model: nextModel }, getModelCapabilities(nextProvider, nextModel)) });
  };

  const setModel = (model: string) => {
    props.setDraft({ ...normalizeDraftForModel({ ...props.draft, model }, getModelCapabilities(provider(), model)) });
  };

  const setMode = (mode: GenerationMode) => {
    props.setDraft("mode", mode);
    if (mode !== "mask") props.setDraft("maskDataUrl", "");
    setValidationMessage("");
  };

  const referenceRole = (mimeType: string): ReferenceAsset["role"] => mimeType.startsWith("video/")
    ? "video"
    : ["edit", "mask", "variation"].includes(props.draft.mode) ? "source" : "object";

  const appendReference = (reference: ReferenceAsset) => {
    if (props.draft.references.length >= capabilities().maxReferences) return;
    props.setDraft("references", (items) => [...items, reference]);
  };

  // Allow pasting images directly from the clipboard into the reference list.
  onMount(() => {
    const handlePaste = (event: ClipboardEvent) => {
      const items = Array.from(event.clipboardData?.items ?? []);
      const imageFiles = items
        .filter((item) => item.type.startsWith("image/"))
        .map((item) => item.getAsFile())
        .filter((f): f is File => f !== null);
      if (imageFiles.length > 0) addFiles(imageFiles);
    };
    window.addEventListener("paste", handlePaste);
    onCleanup(() => window.removeEventListener("paste", handlePaste));
  });

  const addFiles = (files: FileList | File[] | null) => {
    if (!files) return;
    const remaining = Math.max(0, capabilities().maxReferences - props.draft.references.length);
    Array.from(files).slice(0, remaining).forEach((file) => {
      if (props.draft.mode === "variation" && file.type !== "image/png") {
        setValidationMessage(props.t("variationValidation"));
        return;
      }
      const reader = new FileReader();
      reader.onload = () => {
        const url = String(reader.result);
        const commit = (width?: number, height?: number) => {
          if (props.draft.mode === "variation" && width !== undefined && height !== undefined && width !== height) {
            setValidationMessage(props.t("variationValidation"));
            return;
          }
          const reference: ReferenceAsset = {
            id: crypto.randomUUID(),
            name: file.name,
            url,
            mimeType: file.type,
            sourceType: "base64",
            role: file.type.startsWith("video/") ? "video" : props.draft.mode === "edit" || props.draft.mode === "mask" || props.draft.mode === "variation" ? "source" : "object",
            width,
            height,
          };
          appendReference(reference);
        };
        if (file.type.startsWith("image/")) {
          const image = new Image();
          image.onload = () => commit(image.naturalWidth, image.naturalHeight);
          image.onerror = () => commit();
          image.src = url;
        } else {
          commit();
        }
      };
      reader.onerror = () => {
        const error = reader.error ?? new Error(props.t("referenceReadFailed"));
        setValidationMessage(formatError(error));
        props.onError(error, "reference.read");
      };
      reader.readAsDataURL(file);
    });
    referenceInput.value = "";
  };

  const addLocalReferences = async () => {
    if (api.isDemo) {
      referenceInput.click();
      return;
    }
    try {
      const selected = await api.chooseReferenceFiles(props.project.id, props.draft.mode === "video", props.draft.mode === "variation");
      const remaining = Math.max(0, capabilities().maxReferences - props.draft.references.length);
      selected.slice(0, remaining).forEach((item) => appendReference({
        ...item,
        id: crypto.randomUUID(),
        role: referenceRole(item.mimeType),
      }));
    } catch (error) {
      setValidationMessage(formatError(error));
      props.onError(error, "reference.import");
    }
  };

  const addValueReference = () => {
    const value = referenceValue().trim();
    const mode = referenceEntryMode();
    if (!value || !mode) return;
    if (mode === "url" && !/^https?:\/\//i.test(value)) {
      setValidationMessage(props.t("referenceUrlValidation"));
      return;
    }
    const mimeType = referenceMimeType();
    const url = mode === "base64"
      ? value.startsWith("data:") ? value : `data:${mimeType};base64,${value.replace(/\s+/g, "")}`
      : mode === "url" ? value : "";
    const fileId = mode === "file-id" ? value : undefined;
    const name = mode === "url"
      ? value.split(/[?#]/, 1)[0].split("/").filter(Boolean).at(-1) ?? "remote-reference"
      : mode === "file-id" ? value : "base64-reference";
    const commit = (width?: number, height?: number) => {
      appendReference({
        id: crypto.randomUUID(),
        name,
        url,
        fileId,
        mimeType,
        sourceType: mode,
        role: referenceRole(mimeType),
        width,
        height,
      });
      setReferenceValue("");
      setReferenceEntryMode(null);
      setValidationMessage("");
    };
    if (url && mimeType.startsWith("image/")) {
      const image = new Image();
      image.onload = () => commit(image.naturalWidth, image.naturalHeight);
      image.onerror = () => commit();
      image.src = url;
    } else {
      commit();
    }
  };

  const reorderReference = (targetId: string) => {
    const sourceId = draggedReferenceId();
    if (!sourceId || sourceId === targetId) return;
    props.setDraft("references", (items) => {
      const next = [...items];
      const sourceIndex = next.findIndex((item) => item.id === sourceId);
      const targetIndex = next.findIndex((item) => item.id === targetId);
      if (sourceIndex < 0 || targetIndex < 0) return items;
      const [source] = next.splice(sourceIndex, 1);
      next.splice(targetIndex, 0, source);
      return next;
    });
    setDraggedReferenceId("");
  };

  const insertImageToken = (index: number) => {
    const token = `<IMAGE_${index + 1}>`;
    const start = promptInput.selectionStart ?? props.draft.prompt.length;
    const end = promptInput.selectionEnd ?? start;
    const prefix = props.draft.prompt.slice(0, start);
    const suffix = props.draft.prompt.slice(end);
    const spacer = prefix && !/\s$/.test(prefix) ? " " : "";
    const next = `${prefix}${spacer}${token}${suffix}`;
    props.onPromptOverrideClear();
    props.setDraft("prompt", next);
    queueMicrotask(() => {
      const caret = prefix.length + spacer.length + token.length;
      promptInput.focus();
      promptInput.setSelectionRange(caret, caret);
    });
  };

  const removeReference = (id: string) => {
    props.setDraft("references", (items) => items.filter((item) => item.id !== id));
  };

  const generate = async () => {
    const errors = validateGenerationDraft(props.draft, capabilities());
    if (errors.includes("prompt")) {
      setValidationMessage(props.t("validationPrompt"));
      return;
    }
    if (errors.includes("reference")) {
      setValidationMessage(props.t("validationReference"));
      return;
    }
    if (errors.includes("generation-input")) {
      setValidationMessage(props.t("generationReferenceValidation"));
      return;
    }
    if (errors.includes("variation-input")) {
      setValidationMessage(props.t("variationValidation"));
      return;
    }
    if (errors.includes("custom-size")) {
      setValidationMessage(props.t("customSizeValidation"));
      return;
    }
    if (errors.includes("mask")) {
      setValidationMessage(props.t("maskValidation"));
      return;
    }
    if (errors.includes("reference-dimensions")) {
      setValidationMessage(props.t("referenceDimensionsValidation"));
      return;
    }
    if (errors.includes("count")) {
      setValidationMessage(props.t("countValidation").replace("{max}", String(capabilities().maxCount)));
      return;
    }
    if (errors.length) {
      setValidationMessage(errors.join(", "));
      return;
    }
    setValidationMessage("");
    await props.onGenerate(composedPrompt());
  };

  const modeLabel = (mode: GenerationMode): TranslationKey => ({
    generate: "generateMode",
    edit: "editMode",
    mask: "maskMode",
    variation: "variationMode",
    video: "videoMode",
  })[mode] as TranslationKey;

  const referenceAccept = createMemo(() => props.draft.mode === "video"
    ? "video/mp4,video/webm"
    : props.draft.mode === "variation"
      ? "image/png"
      : "image/png,image/jpeg,image/webp,image/heic,image/heif");

  return (
    <div class="page create-page">
      <header class="page-header create-page-header">
        <div>
          <h1>{props.t("createTitle")}</h1>
          <p>{props.t("createSubtitle")}</p>
        </div>
        <div class="header-actions">
          <button class="button secondary" type="button" onClick={() => props.onReveal(props.project.storagePath)}><FolderOpen size={16} />{props.t("openFolder")}</button>
        </div>
      </header>

      <div class="creator-layout">
        <section class="work-panel composer-panel">
          <div class="panel-section provider-section">
            <div class="creator-section-heading">
              <span class="creator-step">1</span>
              <div><h2>{props.t("generationSetup")}</h2><p>{props.t("generationSetupHint")}</p></div>
            </div>
            <div class="control-grid provider-controls">
              <Field label={props.t("provider")}>
                <div class="select-with-action">
                  <select value={provider()?.id ?? ""} onChange={(event) => setProvider(event.currentTarget.value)}>
                    <For each={enabledProviders()}>{(item) => <option value={item.id} selected={item.id === provider()?.id}>{item.name}</option>}</For>
                  </select>
                  <IconButton label={props.t("manageProviders")} onClick={props.onManageProviders}><SlidersHorizontal size={16} /></IconButton>
                </div>
              </Field>
              <Field label={props.t("model")}>
                <select value={props.draft.model} onChange={(event) => setModel(event.currentTarget.value)}>
                  <For each={models()}>{(item) => <option value={item.id} selected={item.id === props.draft.model}>{item.label}</option>}</For>
                </select>
              </Field>
            </div>

            <Field label={props.t("mode")} class="creator-mode-field">
              <div class="segmented mode-segmented">
                <For each={capabilities().modes}>
                  {(mode) => {
                    const ModeIcon = modeIcons[mode];
                    return (
                      <button type="button" class={props.draft.mode === mode ? "is-active" : ""} onClick={() => setMode(mode)}>
                        <ModeIcon size={15} />{props.t(modeLabel(mode))}
                      </button>
                    );
                  }}
                </For>
              </div>
            </Field>

            <div class="control-grid generation-basics">
              <Field label={props.t("aspectRatio")}>
                <select value={props.draft.aspectRatio} onChange={(event) => props.setDraft("aspectRatio", event.currentTarget.value)}>
                  <For each={capabilities().aspectRatios}>{(value) => <option value={value} selected={value === props.draft.aspectRatio}>{value === "auto" ? props.t("auto") : value}</option>}</For>
                </select>
              </Field>
              <Field label={props.t("size")}>
                <select value={props.draft.size} onChange={(event) => props.setDraft("size", event.currentTarget.value)}>
                  <For each={capabilities().sizes}>{(value) => <option value={value} selected={value === props.draft.size}>{value === "auto" ? props.t("auto") : value}</option>}</For>
                  <Show when={capabilities().supportsCustomSize}><option value="custom" selected={props.draft.size === "custom"}>{props.t("customSize")}</option></Show>
                </select>
              </Field>
              <Show when={capabilities().qualityOptions.length > 0}>
                <Field label={props.t("quality")}>
                  <select value={props.draft.quality} onChange={(event) => props.setDraft("quality", event.currentTarget.value)}>
                    <For each={capabilities().qualityOptions}>{(value) => <option value={value} selected={value === props.draft.quality}>{value}</option>}</For>
                  </select>
                </Field>
              </Show>
              <Field label={props.t("imageCount")}>
                <input type="number" min="1" max={capabilities().maxCount} value={props.draft.count} onInput={(event) => props.setDraft("count", Number(event.currentTarget.value))} />
              </Field>
              <Show when={props.draft.size === "custom" && capabilities().supportsCustomSize}>
                <Field label={props.t("customWidth")}>
                  <input type="number" min={capabilities().customSizeRule?.multipleOf ?? 16} max={capabilities().customSizeRule?.maxEdge ?? 3840} step={capabilities().customSizeRule?.multipleOf ?? 16} value={props.draft.customWidth} onInput={(event) => props.setDraft("customWidth", Number(event.currentTarget.value))} />
                </Field>
                <Field label={props.t("customHeight")}>
                  <input type="number" min={capabilities().customSizeRule?.multipleOf ?? 16} max={capabilities().customSizeRule?.maxEdge ?? 3840} step={capabilities().customSizeRule?.multipleOf ?? 16} value={props.draft.customHeight} onInput={(event) => props.setDraft("customHeight", Number(event.currentTarget.value))} />
                </Field>
              </Show>
            </div>
          </div>

          <div class="panel-section reference-section">
            <div class="section-row creator-reference-header">
              <div class="creator-section-heading">
                <span class="creator-step">2</span>
                <div><h2>{props.t("references")}</h2><p>{props.t("referenceSetupHint")}</p></div>
                <span class="section-count">{props.draft.references.length}/{capabilities().maxReferences}</span>
              </div>
              <div class="button-row">
                <Show when={props.draft.references.length > 0}>
                  <button class="button ghost compact" type="button" onClick={() => props.setDraft("references", [])}>{props.t("clear")}</button>
                </Show>
                <button class="button secondary compact" type="button" disabled={props.draft.references.length >= capabilities().maxReferences} onClick={addLocalReferences}><Plus size={15} />{props.t("addLocalReference")}</button>
                <button class="button ghost compact" type="button" disabled={props.draft.references.length >= capabilities().maxReferences} onClick={() => { setReferenceEntryMode("url"); setReferenceValue(""); }}><Link2 size={14} />{props.t("addUrlReference")}</button>
                <button class="button ghost compact icon-only" type="button" title={props.t("addBase64Reference")} disabled={props.draft.references.length >= capabilities().maxReferences} onClick={() => { setReferenceEntryMode("base64"); setReferenceValue(""); }}><Braces size={14} /></button>
                <button class="button ghost compact icon-only" type="button" title={props.t("addFileIdReference")} disabled={props.draft.references.length >= capabilities().maxReferences} onClick={() => { setReferenceEntryMode("file-id"); setReferenceValue(""); }}><KeyRound size={14} /></button>
              </div>
            </div>
            <input ref={referenceInput} class="visually-hidden" type="file" multiple accept={referenceAccept()} onChange={(event) => addFiles(event.currentTarget.files)} />
            <Show when={referenceEntryMode()}>
              <div class="reference-entry-row">
                <Show when={referenceEntryMode() === "base64"} fallback={
                  <input
                    aria-label={referenceEntryMode() === "url" ? props.t("referenceUrl") : props.t("providerFileId")}
                    placeholder={referenceEntryMode() === "url" ? "https://..." : props.t("providerFileId")}
                    value={referenceValue()}
                    spellcheck={false}
                    onInput={(event) => setReferenceValue(event.currentTarget.value)}
                  />
                }>
                  <textarea aria-label={props.t("referenceBase64")} rows="2" placeholder={props.t("referenceBase64")} value={referenceValue()} spellcheck={false} onInput={(event) => setReferenceValue(event.currentTarget.value)} />
                </Show>
                <select aria-label={props.t("mimeType")} value={referenceMimeType()} onChange={(event) => setReferenceMimeType(event.currentTarget.value)}>
                  <option value="image/png">image/png</option>
                  <option value="image/jpeg">image/jpeg</option>
                  <option value="image/webp">image/webp</option>
                  <option value="video/mp4">video/mp4</option>
                  <option value="video/webm">video/webm</option>
                </select>
                <button class="button primary compact" type="button" onClick={addValueReference}>{props.t("addReference")}</button>
                <IconButton label={props.t("close")} onClick={() => setReferenceEntryMode(null)}><X size={14} /></IconButton>
              </div>
            </Show>
            <div class="reference-strip">
              <For each={props.draft.references}>
                {(reference, index) => (
                  <article
                    class="reference-item"
                    draggable={true}
                    onDragStart={() => setDraggedReferenceId(reference.id)}
                    onDragOver={(event) => event.preventDefault()}
                    onDrop={() => reorderReference(reference.id)}
                  >
                    <span class="reference-drag" title={props.t("reorderReference")}><GripVertical size={13} /></span>
                    <div class="reference-preview">
                      <Show when={!reference.mimeType.startsWith("video/")} fallback={<FileVideo2 size={28} />}>
                        <Show when={api.referencePreviewUrl(reference)} fallback={<KeyRound size={25} />}>
                          {(previewUrl) => <img src={previewUrl()} alt={reference.name} />}
                        </Show>
                      </Show>
                      <IconButton label={props.t("removeReference")} onClick={() => removeReference(reference.id)}><X size={13} /></IconButton>
                    </div>
                    <span title={reference.name}>{reference.name}</span>
                    <select value={reference.role} onChange={(event) => props.setDraft("references", (items) => items.map((item) => item.id === reference.id ? { ...item, role: event.currentTarget.value as ReferenceAsset["role"] } : item))}>
                      <option value="source">{props.t("source")}</option>
                      <option value="object">{props.t("object")}</option>
                      <option value="character">{props.t("character")}</option>
                      <option value="style">{props.t("style")}</option>
                      <option value="video">{props.t("video")}</option>
                    </select>
                    <Show when={provider()?.kind === "xai"}>
                      <button class="reference-token" type="button" title={props.t("insertImageToken")} onClick={() => insertImageToken(index())}>{`<IMAGE_${index() + 1}>`}</button>
                    </Show>
                  </article>
                )}
              </For>
              <Show when={props.draft.references.length === 0}>
                <button class="reference-dropzone" type="button" onClick={addLocalReferences}>
                  <Images size={22} /><span>{props.t("addReference")}</span>
                </button>
              </Show>
            </div>
            <Show when={props.draft.mode === "generate" && !continuesConversation() && capabilities().maxReferences > 0}>
              <div class={`reference-mode-hint ${generationInputConflict() ? "is-error" : ""}`} role={generationInputConflict() ? "alert" : "note"}>
                <AlertTriangle size={15} />
                <span>{props.t(generationInputConflict() ? "generationReferenceValidation" : "generationReferenceHint")}</span>
                <Show when={capabilities().modes.includes("edit")}>
                  <button class="button ghost compact" type="button" onClick={() => setMode("edit")}>{props.t("switchToEditMode")}</button>
                </Show>
              </div>
            </Show>
          </div>

          <Show when={props.draft.mode === "mask"}>
            <div class="panel-section mask-section">
              <div class="section-row"><h2>{props.t("maskEditor")}</h2></div>
              <Show when={props.draft.references[0]?.width && props.draft.references[0]?.height} fallback={<p class="mask-validation-inline">{props.t("referenceDimensionsValidation")}</p>}>
                <MaskCanvas
                  t={props.t}
                  sourceUrl={props.draft.references[0] ? api.referencePreviewUrl(props.draft.references[0]) : undefined}
                  sourceWidth={props.draft.references[0]?.width ?? 1}
                  sourceHeight={props.draft.references[0]?.height ?? 1}
                  initialMaskDataUrl={props.draft.maskDataUrl}
                  onChange={(dataUrl) => props.setDraft("maskDataUrl", dataUrl)}
                />
              </Show>
            </div>
          </Show>

          <section class={`advanced-section ${advancedOpen() ? "is-open" : ""}`}>
            <button class="advanced-toggle" type="button" onClick={() => setAdvancedOpen((value) => !value)}>
              <span><SlidersHorizontal size={16} />{props.t("advanced")}</span><ChevronDown size={17} />
            </button>
            <Show when={advancedOpen()}>
              <div class="advanced-content">
                <div class="advanced-group">
                  <h3>{props.t("output")}</h3>
                  <div class="control-grid">
                    <Field label={props.t("outputFormat")}>
                      <select value={props.draft.outputFormat} onChange={(event) => props.setDraft("outputFormat", event.currentTarget.value)}>
                        <For each={capabilities().outputFormats}>{(value) => <option value={value}>{value}</option>}</For>
                      </select>
                    </Field>
                    <Show when={capabilities().responseFormats.length > 0}>
                      <Field label={props.t("responseFormat")}>
                        <select value={props.draft.responseFormat} onChange={(event) => props.setDraft("responseFormat", event.currentTarget.value)}>
                          <For each={capabilities().responseFormats}>{(value) => <option value={value}>{value}</option>}</For>
                        </select>
                      </Field>
                    </Show>
                    <Show when={capabilities().backgrounds.length > 1}>
                      <Field label={props.t("background")}>
                        <select value={props.draft.background} onChange={(event) => props.setDraft("background", event.currentTarget.value)}>
                          <For each={capabilities().backgrounds}>{(value) => <option value={value}>{props.t((value === "transparent" ? "transparent" : value === "opaque" ? "opaque" : "auto") as TranslationKey)}</option>}</For>
                        </select>
                      </Field>
                    </Show>
                    <Show when={["jpeg", "webp"].includes(props.draft.outputFormat)}>
                      <Field label={props.t("compression")}>
                        <div class="range-field"><input type="range" min="1" max="100" value={props.draft.compression} onInput={(event) => props.setDraft("compression", Number(event.currentTarget.value))} /><output>{props.draft.compression}</output></div>
                      </Field>
                    </Show>
                    <Show when={capabilities().supportsInputFidelity}>
                      <Field label={props.t("inputFidelity")}>
                        <select value={props.draft.inputFidelity} onChange={(event) => props.setDraft("inputFidelity", event.currentTarget.value)}><option value="low">{props.t("low")}</option><option value="high">{props.t("high")}</option></select>
                      </Field>
                    </Show>
                    <Show when={capabilities().supportsSeed}>
                      <Field label={props.t("seed")}><input inputmode="numeric" placeholder={props.t("auto")} value={props.draft.seed} onInput={(event) => props.setDraft("seed", event.currentTarget.value)} /></Field>
                    </Show>
                    <Show when={capabilities().supportsPartialImages}>
                      <Field label={props.t("partialImages")}><input type="number" min="0" max="3" value={props.draft.partialImages} onInput={(event) => props.setDraft("partialImages", Number(event.currentTarget.value))} /></Field>
                    </Show>
                  </div>
                </div>

                <Show when={provider()?.kind === "openai" && (capabilities().supportsResponsesApi || capabilities().moderationOptions.length > 0 || capabilities().styleOptions.length > 0)}>
                  <div class="advanced-group">
                    <h3>OpenAI</h3>
                    <div class="control-grid">
                      <Show when={capabilities().moderationOptions.length > 0}>
                        <Field label={props.t("moderation")}>
                          <select value={props.draft.moderation} onChange={(event) => props.setDraft("moderation", event.currentTarget.value)}><For each={capabilities().moderationOptions}>{(value) => <option value={value}>{value}</option>}</For></select>
                        </Field>
                      </Show>
                      <Show when={capabilities().styleOptions.length > 0}>
                        <Field label={props.t("styleOption")}>
                          <select value={props.draft.style} onChange={(event) => props.setDraft("style", event.currentTarget.value)}><For each={capabilities().styleOptions}>{(value) => <option value={value}>{value}</option>}</For></select>
                        </Field>
                      </Show>
                      <Show when={capabilities().supportsResponsesApi}>
                        <Field label={props.t("responseModel")}>
                          <input placeholder="gpt-4.1" value={props.draft.responseModel} spellcheck={false} onInput={(event) => props.setDraft("responseModel", event.currentTarget.value)} />
                        </Field>
                        <Field label={props.t("imageGenerationAction")}>
                          <select value={props.draft.imageGenerationAction} onChange={(event) => props.setDraft("imageGenerationAction", event.currentTarget.value)}><For each={capabilities().imageGenerationActions}>{(value) => <option value={value}>{value}</option>}</For></select>
                        </Field>
                        <Field label={props.t("previousResponseId")} class="span-2">
                          <input placeholder={props.t("auto")} value={props.draft.previousResponseId} spellcheck={false} onInput={(event) => { const value = event.currentTarget.value; props.setDraft("previousResponseId", value); if (value.trim()) props.setDraft("useResponsesApi", true); }} />
                        </Field>
                      </Show>
                    </div>
                    <Show when={capabilities().supportsResponsesApi}><Toggle checked={props.draft.useResponsesApi} onChange={(value) => props.setDraft("useResponsesApi", value)} label={props.t("useResponsesApi")} /></Show>
                  </div>
                </Show>

                <Show when={capabilities().supportsRemoteFiles}>
                  <div class="advanced-group">
                    <h3>xAI Files</h3>
                    <div class="control-grid">
                      <Field label={props.t("storageFilename")}><input value={props.draft.storageFilename} spellcheck={false} onInput={(event) => props.setDraft("storageFilename", event.currentTarget.value)} /></Field>
                      <Field label={props.t("ttlSeconds")}><input type="number" min="3600" max="2592000" value={props.draft.ttlSeconds} onInput={(event) => props.setDraft("ttlSeconds", Number(event.currentTarget.value))} /></Field>
                    </div>
                    <div class="toggle-grid">
                      <Toggle checked={props.draft.persistRemoteFile} onChange={(value) => props.setDraft("persistRemoteFile", value)} label={props.t("persistRemoteFile")} />
                      <Toggle checked={props.draft.publicFileUrl} onChange={(value) => props.setDraft("publicFileUrl", value)} label={props.t("publicFileUrl")} />
                    </div>
                  </div>
                </Show>

                <Show when={provider()?.kind === "gemini" && (capabilities().supportsThinking || capabilities().supportsWebSearch || capabilities().supportsTextOutput)}>
                  <div class="advanced-group">
                    <h3>Gemini</h3>
                    <Show when={capabilities().supportsThinking}>
                      <Field label={props.t("thinkingLevel")}>
                        <select value={props.draft.thinkingLevel} onChange={(event) => props.setDraft("thinkingLevel", event.currentTarget.value)}>
                          <For each={capabilities().thinkingLevels}>{(value) => <option value={value}>{props.t((value === "minimal" ? "minimal" : "high") as TranslationKey)}</option>}</For>
                        </select>
                      </Field>
                    </Show>
                    <div class="control-grid compact-grid">
                      <Field label={props.t("temperature")}><div class="range-field"><input type="range" min="0" max="2" step="0.05" value={props.draft.temperature} onInput={(event) => props.setDraft("temperature", Number(event.currentTarget.value))} /><output>{props.draft.temperature.toFixed(2)}</output></div></Field>
                      <Field label={props.t("topP")}><div class="range-field"><input type="range" min="0" max="1" step="0.01" value={props.draft.topP} onInput={(event) => props.setDraft("topP", Number(event.currentTarget.value))} /><output>{props.draft.topP.toFixed(2)}</output></div></Field>
                    </div>
                    <div class="toggle-grid">
                      <Show when={capabilities().supportsWebSearch}><Toggle checked={props.draft.webSearch} onChange={(value) => props.setDraft("webSearch", value)} label={props.t("webSearch")} /></Show>
                      <Show when={capabilities().supportsImageSearch}><Toggle checked={props.draft.imageSearch} onChange={(value) => props.setDraft("imageSearch", value)} label={props.t("imageSearch")} /></Show>
                      <Show when={capabilities().supportsTextOutput}><Toggle checked={props.draft.includeText} onChange={(value) => { props.setDraft("includeText", value); props.setDraft("outputModalities", value ? ["image", "text"] : ["image"]); }} label={props.t("includeText")} /></Show>
                      <Toggle checked={props.draft.storeInteraction} onChange={(value) => props.setDraft("storeInteraction", value)} label={props.t("storeInteraction")} />
                      <Show when={capabilities().supportsRemoteStore}><Toggle checked={props.draft.remoteStore} onChange={(value) => props.setDraft("remoteStore", value)} label={props.t("remoteStore")} /></Show>
                      <Show when={capabilities().supportsConversation}><Toggle checked={props.draft.useInteractionsApi} onChange={(value) => props.setDraft("useInteractionsApi", value)} label={props.t("useInteractionsApi")} /></Show>
                    </div>
                    <Show when={capabilities().supportsOutputModalities}>
                      <Field label={props.t("outputModalities")}><div class="modality-list"><label><input type="checkbox" checked disabled />{props.t("imageModality")}</label><label><input type="checkbox" checked={props.draft.includeText} onChange={(event) => { const value = event.currentTarget.checked; props.setDraft("includeText", value); props.setDraft("outputModalities", value ? ["image", "text"] : ["image"]); }} />{props.t("textModality")}</label></div></Field>
                    </Show>
                    <Show when={capabilities().supportsConversation}>
                      <div class="control-grid">
                        <Field label={props.t("previousInteractionId")}><input placeholder={props.t("auto")} value={props.draft.previousInteractionId} spellcheck={false} onInput={(event) => { const value = event.currentTarget.value; props.setDraft("previousInteractionId", value); if (value.trim()) props.setDraft("useInteractionsApi", true); }} /></Field>
                        <Field label={props.t("lastEventId")}><input placeholder={props.t("auto")} value={props.draft.lastEventId} spellcheck={false} onInput={(event) => props.setDraft("lastEventId", event.currentTarget.value)} /></Field>
                      </div>
                    </Show>
                  </div>
                </Show>

                <div class="advanced-group">
                  <h3>{props.t("negativePrompt")}</h3>
                  <textarea rows="3" placeholder={props.t("negativePromptPlaceholder")} value={props.draft.negativePrompt} onInput={(event) => { props.onPromptOverrideClear(); props.setDraft("negativePrompt", event.currentTarget.value); }} />
                  <Field label={props.t("outputFilename")}>
                    <input
                      placeholder={props.t("outputFilenameHint")}
                      value={props.draft.outputFilename}
                      spellcheck={false}
                      onInput={(event) => props.setDraft("outputFilename", event.currentTarget.value)}
                    />
                  </Field>
                  <div class="toggle-grid">
                    <Show when={capabilities().supportsStreaming}><Toggle checked={props.draft.stream} onChange={(value) => props.setDraft("stream", value)} label={props.t("stream")} /></Show>
                    <Show when={capabilities().supportsBackground}><Toggle checked={props.draft.backgroundTask} onChange={(value) => props.setDraft("backgroundTask", value)} label={props.t("backgroundTask")} /></Show>
                    <Show when={capabilities().supportsBatch}><Toggle checked={props.draft.batch} onChange={(value) => props.setDraft("batch", value)} label={props.t("batch")} /></Show>
                  </div>
                  <Show when={capabilities().supportsServiceTier}>
                    <Field label={props.t("serviceTier")}>
                      <select value={props.draft.serviceTier} onChange={(event) => props.setDraft("serviceTier", event.currentTarget.value)}><option value="standard">{props.t("standard")}</option><option value="priority">{props.t("priority")}</option><option value="flex">{props.t("flex")}</option></select>
                    </Field>
                  </Show>
                  <Field label={props.t("customJson")}>
                    <textarea class="code-input" rows="3" spellcheck={false} value={props.draft.customJson} onInput={(event) => props.setDraft("customJson", event.currentTarget.value)} />
                  </Field>
                </div>
              </div>
            </Show>
          </section>
        </section>

        <aside class="creator-side">
          <section class="work-panel prompt-editor-panel">
            <div class="panel-heading creator-prompt-heading">
              <div>
                <span class="creator-step">3</span>
                <span class="creator-prompt-title"><h2>{props.t("promptEditor")}</h2><small>{props.t("promptEditorHint")}</small></span>
              </div>
            </div>
            <div class="prompt-editor-body">
              <Show when={props.project.settings.useCommonDescriptions && props.project.descriptions.some((item) => item.enabled && (item.prefixContent.trim() || item.suffixContent.trim() || item.negativeContent.trim()))}>
                <span class="context-label">{props.t("projectContext")}</span>
              </Show>
              <textarea
                ref={promptInput}
                class="prompt-input prompt-editor-input"
                rows="8"
                placeholder={props.t("promptPlaceholder")}
                value={props.draft.prompt}
                onInput={(event) => {
                  props.onPromptOverrideClear();
                  props.setDraft("prompt", event.currentTarget.value);
                }}
              />
              <div class="prompt-meta">
                <span>{props.t("characterCount").replace("{count}", props.draft.prompt.length.toLocaleString())}</span>
                <Show when={validationMessage()}><span class="validation-message">{validationMessage()}</span></Show>
              </div>
            </div>
            <div class="request-summary">
              <span>{provider()?.name}</span><span>{props.draft.model}</span><span>{props.draft.aspectRatio}</span><span>{props.draft.size}</span><span>×{props.draft.count}</span>
            </div>
            <button class="button primary full-width" type="button" disabled={!provider() || provider()!.models.length === 0} onClick={generate}><WandSparkles size={17} />{props.t("generate")}</button>
          </section>

          <section class="work-panel task-panel">
            <div class="panel-heading">
              <div><Clock3 size={16} /><h2>{props.t("taskQueue")}</h2></div>
              <div class="task-panel-controls">
                <Show when={props.queuePaused}><span class="queue-paused-badge">{props.t("queuePaused")}</span></Show>
                <span class="panel-count">{projectTasks().length}</span>
                <IconButton
                  label={props.t(props.queuePaused ? "resumeQueueTooltip" : "pauseQueueTooltip")}
                  active={props.queuePaused}
                  disabled={props.queueControlBusy}
                  onClick={props.onToggleQueue}
                >
                  <Show when={props.queuePaused} fallback={<Pause size={15} />}><Play size={15} /></Show>
                </IconButton>
              </div>
            </div>
            <Show when={projectTasks().length > 0} fallback={<EmptyState icon={<Clock3 size={22} />} title={props.t("noTasks")} />}>
              <div class="task-list">
                <For each={projectTasks().slice(0, 5)}>
                  {(task) => (
                    <article class="task-row">
                      <span class={`task-status-icon status-${task.status}`}>
                        <Show when={task.status === "running"} fallback={task.status === "completed" ? <ImageIcon size={15} /> : task.status === "failed" ? <X size={15} /> : <Clock3 size={15} />}><LoaderCircle class="spin" size={15} /></Show>
                      </span>
                      <div><strong>{task.model}</strong><p>{task.prompt}</p><div class="task-progress"><span style={{ width: `${task.progress}%` }} /></div><Show when={(task.partialImages?.length ?? 0) > 0}><div class="task-partials"><For each={task.partialImages}>{(image) => <img src={image.url} alt="" />}</For></div></Show></div>
                      <div class="task-row-actions">
                        <Show when={task.status === "running" || task.status === "queued"}>
                          <IconButton label={props.t("cancel")} onClick={() => props.onCancelTask(task.id)}><X size={15} /></IconButton>
                        </Show>
                        <Show when={task.status === "completed" || task.status === "failed" || task.responseParts.length > 0}>
                          <IconButton label={props.t("taskDetails")} onClick={() => setSelectedTask(task)}><MoreHorizontal size={15} /></IconButton>
                        </Show>
                      </div>
                    </article>
                  )}
                </For>
              </div>
            </Show>
          </section>
        </aside>
      </div>

      <section class="results-section">
        <div class="section-row results-heading"><div><h2>{props.t("results")}</h2><span class="section-count">{resultAssets().length}</span></div></div>
        <Show when={partialResults().length > 0}>
          <div class="partial-results"><span class="section-kicker">{props.t("partialResults")}</span><div><For each={partialResults()}>{(image) => <img src={image.url} alt="" />}</For></div></div>
        </Show>
        <Show when={resultAssets().length > 0} fallback={<EmptyState icon={<ImageIcon size={24} />} title={props.t("noResults")} />}>
          <div class="result-grid">
            <For each={resultAssets()}>
              {({ asset, record }) => (
                <article class="result-card">
                  <div class="result-image"><img src={asset.url} alt={asset.prompt} /><span>{asset.width}×{asset.height}</span></div>
                  <div class="result-card-body">
                    <div><strong>{record.model}</strong><p>{record.prompt}</p></div>
                    <div class="result-actions">
                      <IconButton label={props.t("reveal")} onClick={() => props.onReveal(asset.filePath)}><FolderOpen size={15} /></IconButton>
                      <IconButton label={props.t("download")} onClick={() => props.onDownload(asset)}><ArrowDownToLine size={15} /></IconButton>
                      <IconButton label={props.t("taskDetails")} onClick={() => setSelectedTask(record)}><MoreHorizontal size={15} /></IconButton>
                    </div>
                  </div>
                </article>
              )}
            </For>
          </div>
        </Show>
      </section>
      <TaskDetailModal task={selectedTask()} t={props.t} onClose={() => setSelectedTask(null)} />
    </div>
  );
}
