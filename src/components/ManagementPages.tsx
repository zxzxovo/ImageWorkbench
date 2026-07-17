import { For, Show, createMemo, createSignal } from "solid-js";
import { createStore } from "solid-js/store";
import {
  CalendarDays,
  Check,
  Clipboard,
  Copy,
  Download,
  Filter,
  FolderOpen,
  Heart,
  Image as ImageIcon,
  Images,
  ListFilter,
  List as ListIcon,
  LayoutGrid,
  MoreHorizontal,
  Play,
  Plus,
  RotateCw,
  Save,
  Search,
  SlidersHorizontal,
  Sparkles,
  SquarePen,
  Trash2,
  GitCompare,
} from "lucide-solid";
import { api } from "../lib/api";
import type { TranslationKey } from "../lib/i18n";
import { getModelLabel, getModelsForProvider } from "../lib/models";
import type {
  CommonDescription,
  GeneratedAsset,
  GenerationPreset,
  HistoryRecord,
  Project,
  ProviderProfile,
} from "../types";
import { EmptyState, Field, IconButton, Modal, Toggle } from "./common";
import TaskDetailModal from "./TaskDetailModal";
import CompareModal from "./CompareModal";

interface BaseProps {
  project: Project;
  providers: ProviderProfile[];
  t: (key: TranslationKey) => string;
}

export function HistoryPage(props: BaseProps & {
  history: HistoryRecord[];
  onRerun: (record: HistoryRecord) => void;
  onContinue: (record: HistoryRecord) => void;
  onToggleFavorite: (recordId: string) => void;
  onDelete: (recordId: string) => void;
  onDeleteFailed: () => void;
}) {
  const [query, setQuery] = createSignal("");
  const [providerId, setProviderId] = createSignal("all");
  const [status, setStatus] = createSignal("all");
  const [model, setModel] = createSignal("all");
  const [dateFilter, setDateFilter] = createSignal<"all" | "30d" | "7d">("all");
  const [selectedRecord, setSelectedRecord] = createSignal<HistoryRecord | null>(null);
  const [compareMode, setCompareMode] = createSignal(false);
  const [selectedForCompare, setSelectedForCompare] = createSignal<Set<string>>(new Set());
  const [showCompareModal, setShowCompareModal] = createSignal(false);

  const records = createMemo(() => {
    const cutoff = dateFilter() === "30d"
      ? new Date(Date.now() - 30 * 86_400_000)
      : dateFilter() === "7d"
      ? new Date(Date.now() - 7 * 86_400_000)
      : null;
    return props.history
      .filter((record) => record.projectId === props.project.id)
      .filter((record) => !cutoff || new Date(record.createdAt) >= cutoff)
      .filter((record) => providerId() === "all" || record.providerId === providerId())
      .filter((record) => status() === "all" || record.status === status())
      .filter((record) => model() === "all" || record.model === model())
      .filter((record) => `${record.prompt} ${record.model}`.toLowerCase().includes(query().toLowerCase()));
  });
  const projectModels = createMemo(() => [...new Set(props.history.filter((item) => item.projectId === props.project.id).map((item) => item.model))]);

  const toggleCompareMode = () => {
    setCompareMode(!compareMode());
    setSelectedForCompare(new Set<string>());
  };

  const toggleRecordSelection = (recordId: string) => {
    const newSet = new Set<string>(selectedForCompare());
    if (newSet.has(recordId)) {
      newSet.delete(recordId);
    } else {
      if (newSet.size < 4) {
        newSet.add(recordId);
      }
    }
    setSelectedForCompare(newSet);
  };

  const openCompareModal = () => {
    setShowCompareModal(true);
  };

  const closeCompareModal = () => {
    setShowCompareModal(false);
    setCompareMode(false);
    setSelectedForCompare(new Set<string>());
  };

  const compareRecords = createMemo(() => {
    const ids = Array.from(selectedForCompare());
    return records().filter((r) => ids.includes(r.id));
  });

  return (
    <div class="page management-page history-page">
      <header class="page-header">
        <div><h1>{props.t("history")}</h1><p>{props.project.name}</p></div>
        <div style={{ display: "flex", gap: "8px" }}>
          <button
            class={`button ${compareMode() ? "primary" : "secondary"}`}
            type="button"
            onClick={toggleCompareMode}
          >
            <GitCompare size={16} />
            {compareMode() ? props.t("exitCompare") : props.t("compare")}
          </button>
          <button
            class={`button ${dateFilter() !== "all" ? "primary" : "secondary"}`}
            type="button"
            onClick={() => setDateFilter((f) => f === "30d" ? "all" : "30d")}
          >
            <CalendarDays size={16} />
            {dateFilter() === "30d" ? `${props.t("last30Days")} ✓` : props.t("last30Days")}
          </button>
          <Show when={props.history.some((r) => r.projectId === props.project.id && r.status === "failed")}>
            <button class="button secondary" type="button" onClick={props.onDeleteFailed}>
              <Trash2 size={16} />
              {props.t("deleteFailed")}
            </button>
          </Show>
        </div>
      </header>

      <section class="filter-bar">
        <label class="search-field"><Search size={16} /><input placeholder={props.t("searchHistory")} value={query()} onInput={(event) => setQuery(event.currentTarget.value)} /></label>
        <select value={providerId()} onChange={(event) => setProviderId(event.currentTarget.value)}>
          <option value="all">{props.t("allProviders")}</option>
          <For each={props.providers}>{(provider) => <option value={provider.id}>{provider.name}</option>}</For>
        </select>
        <select value={model()} onChange={(event) => setModel(event.currentTarget.value)}>
          <option value="all">{props.t("allModels")}</option>
          <For each={projectModels()}>{(item) => <option value={item}>{getModelLabel(item)}</option>}</For>
        </select>
        <select value={status()} onChange={(event) => setStatus(event.currentTarget.value)}>
          <option value="all">{props.t("allStatuses")}</option>
          <option value="completed">{props.t("statusCompleted")}</option>
          <option value="failed">{props.t("statusFailed")}</option>
          <option value="running">{props.t("statusRunning")}</option>
        </select>
        <IconButton label={props.t("filters")}><ListFilter size={16} /></IconButton>
      </section>

      <Show when={records().length > 0} fallback={<EmptyState icon={<Filter size={24} />} title={props.t("historyEmpty")} />}>
        <div class="history-table-wrap">
          <table class="history-table">
            <thead>
              <tr>
                <Show when={compareMode()}>
                  <th style={{ width: "50px" }}></th>
                </Show>
                <th>{props.t("preview")}</th>
                <th>{props.t("prompt")}</th>
                <th>{props.t("provider")}</th>
                <th>{props.t("model")}</th>
                <th>{props.t("mode")}</th>
                <th>{props.t("statusLabel")}</th>
                <th>{props.t("date")}</th>
                <th />
              </tr>
            </thead>
            <tbody>
              <For each={records()}>
                {(record) => (
                  <tr class={selectedForCompare().has(record.id) ? "is-selected" : ""}>
                    <Show when={compareMode()}>
                      <td>
                        <input
                          type="checkbox"
                          checked={selectedForCompare().has(record.id)}
                          disabled={!selectedForCompare().has(record.id) && selectedForCompare().size >= 4}
                          onChange={() => toggleRecordSelection(record.id)}
                        />
                      </td>
                    </Show>
                    <td><div class="history-thumb"><Show when={record.assets[0]} fallback={<ImageIcon size={18} />}><img src={record.assets[0]?.url} alt="" /></Show><Show when={record.assets.length > 1}><span>+{record.assets.length - 1}</span></Show></div></td>
                    <td><div class="history-prompt"><strong>{record.prompt}</strong><small>{record.count} {props.t("imageUnit")}{record.durationMs ? ` · ${(record.durationMs / 1000).toFixed(1)}s` : ""}</small></div></td>
                    <td>{record.providerName}</td>
                    <td><span class="model-pill">{getModelLabel(record.model)}</span></td>
                    <td>{record.mode}</td>
                    <td><span class={`status-chip status-${record.status}`}>{props.t((`status${record.status[0].toUpperCase()}${record.status.slice(1)}`) as TranslationKey)}</span></td>
                    <td>{new Date(record.createdAt).toLocaleString()}</td>
                    <td>
                      <Show when={!compareMode()}>
                        <div class="table-actions">
                          <IconButton label={props.t("favorite")} active={record.favorite} onClick={() => props.onToggleFavorite(record.id)}><Heart size={15} fill={record.favorite ? "currentColor" : "none"} /></IconButton>
                          <IconButton label={props.t("retry")} onClick={() => props.onRerun(record)}><RotateCw size={15} /></IconButton>
                          <Show when={record.interactionId}><IconButton label={props.t("continueEditing")} onClick={() => props.onContinue(record)}><SquarePen size={15} /></IconButton></Show>
                          <IconButton label={props.t("taskDetails")} onClick={() => setSelectedRecord(record)}><MoreHorizontal size={15} /></IconButton>
                          <IconButton label={props.t("delete")} onClick={() => props.onDelete(record.id)}><Trash2 size={15} /></IconButton>
                        </div>
                      </Show>
                    </td>
                  </tr>
                )}
              </For>
            </tbody>
          </table>
        </div>
      </Show>

      <Show when={compareMode() && selectedForCompare().size >= 2}>
        <div class="compare-action-bar">
          <span>{props.t("selectedCount").replace("{count}", selectedForCompare().size.toString())}</span>
          <button class="button primary" type="button" onClick={openCompareModal}>
            <GitCompare size={16} />
            {props.t("compareSelected")}
          </button>
        </div>
      </Show>

      <TaskDetailModal task={selectedRecord()} t={props.t} onClose={() => setSelectedRecord(null)} />
      <CompareModal records={showCompareModal() ? compareRecords() : []} t={props.t} onClose={closeCompareModal} />
    </div>
  );
}

export function DescriptionsPage(props: BaseProps & {
  onChange: (descriptions: CommonDescription[]) => void;
}) {
  const [selectedId, setSelectedId] = createSignal(props.project.descriptions[0]?.id ?? "");
  const [draft, setDraft] = createStore<CommonDescription>({
    id: "",
    title: "",
    content: "",
    enabled: true,
    placement: "suffix",
    createdAt: new Date().toISOString(),
  });

  const select = (item: CommonDescription) => {
    setSelectedId(item.id);
    setDraft({ ...item });
  };

  const add = () => {
    const item: CommonDescription = {
      id: crypto.randomUUID(),
      title: props.t("addDescription"),
      content: "",
      enabled: true,
      placement: "suffix",
      createdAt: new Date().toISOString(),
    };
    props.onChange([...props.project.descriptions, item]);
    select(item);
  };

  const save = () => {
    if (!draft.id) return;
    props.onChange(props.project.descriptions.map((item) => item.id === draft.id ? { ...draft } : item));
  };

  const remove = (id: string) => {
    const next = props.project.descriptions.filter((item) => item.id !== id);
    props.onChange(next);
    if (selectedId() === id) {
      if (next[0]) select(next[0]);
      else setSelectedId("");
    }
  };

  return (
    <div class="page management-page">
      <header class="page-header"><div><h1>{props.t("descriptionTitle")}</h1><p>{props.t("descriptionSubtitle")}</p></div><button class="button primary" type="button" onClick={add}><Plus size={16} />{props.t("addDescription")}</button></header>
      <div class="split-manager">
        <aside class="manager-list">
          <For each={props.project.descriptions}>
            {(item) => (
              <button type="button" class={`manager-list-item ${selectedId() === item.id ? "is-selected" : ""}`} onClick={() => select(item)}>
                <span class={`description-placement placement-${item.placement}`}>{item.placement === "prefix" ? props.t("prefix") : props.t("suffix")}</span>
                <span><strong>{item.title}</strong><small>{item.content || "-"}</small></span>
                <span class={`enabled-indicator ${item.enabled ? "is-enabled" : ""}`}><Check size={12} /></span>
              </button>
            )}
          </For>
          <Show when={props.project.descriptions.length === 0}><EmptyState icon={<Sparkles size={22} />} title={props.t("addDescription")} /></Show>
        </aside>
        <section class="manager-editor">
          <Show when={selectedId()} fallback={<EmptyState icon={<SlidersHorizontal size={24} />} title={props.t("addDescription")} />}>
            <div class="editor-heading"><div><span class="section-kicker">{props.t("edit")}</span><h2>{draft.title}</h2></div><IconButton label={props.t("delete")} onClick={() => remove(draft.id)}><Trash2 size={16} /></IconButton></div>
            <div class="editor-form">
              <Field label={props.t("name")}><input value={draft.title} onInput={(event) => setDraft("title", event.currentTarget.value)} /></Field>
              <Field label={props.t("placement")}><div class="segmented"><button type="button" class={draft.placement === "prefix" ? "is-active" : ""} onClick={() => setDraft("placement", "prefix")}>{props.t("prefix")}</button><button type="button" class={draft.placement === "suffix" ? "is-active" : ""} onClick={() => setDraft("placement", "suffix")}>{props.t("suffix")}</button></div></Field>
              <Field label={props.t("content")}><textarea rows="10" value={draft.content} onInput={(event) => setDraft("content", event.currentTarget.value)} /></Field>
              <Toggle checked={draft.enabled} onChange={(value) => setDraft("enabled", value)} label={props.t("enabled")} />
              <div class="editor-actions"><button class="button primary" type="button" onClick={save}><Save size={16} />{props.t("save")}</button></div>
            </div>
          </Show>
        </section>
      </div>
    </div>
  );
}

export function PresetsPage(props: BaseProps & {
  onChange: (presets: GenerationPreset[]) => void;
  onApply: (preset: GenerationPreset) => void;
}) {
  const [modalOpen, setModalOpen] = createSignal(false);
  const [draft, setDraft] = createStore<GenerationPreset>({
    id: "",
    name: "",
    description: "",
    providerId: props.providers[0]?.id ?? "",
    model: props.providers[0]?.models[0] ?? "",
    mode: "generate",
    aspectRatio: "1:1",
    size: "1K",
    quality: "auto",
    outputFormat: "png",
    promptTemplate: "",
    createdAt: new Date().toISOString(),
  });
  const selectedProvider = createMemo(() => props.providers.find((item) => item.id === draft.providerId));

  const openEditor = (preset?: GenerationPreset) => {
    if (preset) setDraft({ ...preset });
    else {
      const provider = props.providers.find((item) => item.enabled) ?? props.providers[0];
      setDraft({
        id: crypto.randomUUID(), name: "", description: "", providerId: provider?.id ?? "", model: provider?.models[0] ?? "", mode: "generate",
        aspectRatio: "1:1", size: "1K", quality: "auto", outputFormat: "png", promptTemplate: "", createdAt: new Date().toISOString(),
      });
    }
    setModalOpen(true);
  };

  const save = () => {
    if (!draft.name.trim()) return;
    const exists = props.project.presets.some((item) => item.id === draft.id);
    props.onChange(exists ? props.project.presets.map((item) => item.id === draft.id ? { ...draft } : item) : [...props.project.presets, { ...draft }]);
    setModalOpen(false);
  };

  const duplicate = (preset: GenerationPreset) => {
    props.onChange([...props.project.presets, { ...preset, id: crypto.randomUUID(), name: `${preset.name} Copy`, createdAt: new Date().toISOString() }]);
  };

  const footer = <><button class="button secondary" type="button" onClick={() => setModalOpen(false)}>{props.t("cancel")}</button><button class="button primary" type="button" onClick={save}><Save size={16} />{props.t("save")}</button></>;

  return (
    <div class="page management-page">
      <header class="page-header"><div><h1>{props.t("presetTitle")}</h1><p>{props.t("presetSubtitle")}</p></div><button class="button primary" type="button" onClick={() => openEditor()}><Plus size={16} />{props.t("addPreset")}</button></header>
      <Show when={props.project.presets.length > 0} fallback={<EmptyState icon={<SlidersHorizontal size={24} />} title={props.t("addPreset")} />}>
        <div class="preset-grid">
          <For each={props.project.presets}>
            {(preset) => (
              <article class="preset-card">
                <header><span class="preset-icon"><SlidersHorizontal size={17} /></span><IconButton label={props.t("more")}><MoreHorizontal size={16} /></IconButton></header>
                <div><h2>{preset.name}</h2><p>{preset.description}</p></div>
                <div class="preset-specs"><span>{getModelLabel(preset.model)}</span><span>{preset.aspectRatio}</span><span>{preset.size}</span><span>{preset.outputFormat.toUpperCase()}</span></div>
                <footer>
                  <button class="button primary compact" type="button" onClick={() => props.onApply(preset)}><Play size={15} />{props.t("applyPreset")}</button>
                  <IconButton label={props.t("edit")} onClick={() => openEditor(preset)}><SlidersHorizontal size={15} /></IconButton>
                  <IconButton label={props.t("duplicate")} onClick={() => duplicate(preset)}><Copy size={15} /></IconButton>
                  <IconButton label={props.t("delete")} onClick={() => props.onChange(props.project.presets.filter((item) => item.id !== preset.id))}><Trash2 size={15} /></IconButton>
                </footer>
              </article>
            )}
          </For>
        </div>
      </Show>

      <Modal open={modalOpen()} title={props.t("addPreset")} onClose={() => setModalOpen(false)} footer={footer} size="large">
        <div class="preset-form">
          <Field label={props.t("name")} required><input value={draft.name} onInput={(event) => setDraft("name", event.currentTarget.value)} /></Field>
          <Field label={props.t("projectDescription")}><input value={draft.description} onInput={(event) => setDraft("description", event.currentTarget.value)} /></Field>
          <div class="control-grid">
            <Field label={props.t("provider")}><select value={draft.providerId} onChange={(event) => { const providerId = event.currentTarget.value; const provider = props.providers.find((item) => item.id === providerId); setDraft({ providerId, model: provider?.models[0] ?? "" }); }}><For each={props.providers}>{(provider) => <option value={provider.id}>{provider.name}</option>}</For></select></Field>
            <Field label={props.t("model")}><select value={draft.model} onChange={(event) => setDraft("model", event.currentTarget.value)}><For each={getModelsForProvider(selectedProvider())}>{(model) => <option value={model.id}>{model.label}</option>}</For></select></Field>
            <Field label={props.t("mode")}><select value={draft.mode} onChange={(event) => setDraft("mode", event.currentTarget.value as GenerationPreset["mode"])}><option value="generate">{props.t("generateMode")}</option><option value="edit">{props.t("editMode")}</option><option value="mask">{props.t("maskMode")}</option><option value="variation">{props.t("variationMode")}</option></select></Field>
            <Field label={props.t("aspectRatio")}><input value={draft.aspectRatio} onInput={(event) => setDraft("aspectRatio", event.currentTarget.value)} /></Field>
            <Field label={props.t("size")}><input value={draft.size} onInput={(event) => setDraft("size", event.currentTarget.value)} /></Field>
            <Field label={props.t("outputFormat")}><input value={draft.outputFormat} onInput={(event) => setDraft("outputFormat", event.currentTarget.value)} /></Field>
          </div>
          <Field label={props.t("prompt")}><textarea rows="4" value={draft.promptTemplate} onInput={(event) => setDraft("promptTemplate", event.currentTarget.value)} /></Field>
        </div>
      </Modal>
    </div>
  );
}

export function ProjectSettingsPage(props: BaseProps & {
  onChange: (project: Project) => void;
  onClearHistory: () => void;
}) {
  const [draft, setDraft] = createStore<Project>({ ...props.project, settings: { ...props.project.settings } });
  const defaultProvider = createMemo(() => props.providers.find((item) => item.id === draft.settings.defaultProviderId));
  const [saved, setSaved] = createSignal(false);

  const chooseFolder = async () => {
    const path = await api.chooseDirectory(draft.storagePath);
    if (path) setDraft("storagePath", path);
  };

  const save = () => {
    props.onChange({ ...draft, updatedAt: new Date().toISOString() });
    setSaved(true);
    window.setTimeout(() => setSaved(false), 1400);
  };

  return (
    <div class="page management-page settings-page">
      <header class="page-header"><div><h1>{props.t("projectSettings")}</h1><p>{props.project.name}</p></div><button class="button primary" type="button" onClick={save}><Show when={saved()} fallback={<Save size={16} />}><Check size={16} /></Show>{saved() ? props.t("saved") : props.t("save")}</button></header>
      <div class="settings-layout">
        <section class="settings-section">
          <div class="settings-section-heading"><h2>{props.t("projectInfo")}</h2></div>
          <div class="settings-form">
            <Field label={props.t("projectName")}><input value={draft.name} onInput={(event) => setDraft("name", event.currentTarget.value)} /></Field>
            <Field label={props.t("projectDescription")}><textarea rows="3" value={draft.description} onInput={(event) => setDraft("description", event.currentTarget.value)} /></Field>
            <Field label={props.t("storagePath")}><div class="input-action-group"><input value={draft.storagePath} onInput={(event) => setDraft("storagePath", event.currentTarget.value)} /><button class="button secondary icon-only" type="button" onClick={chooseFolder}><FolderOpen size={17} /></button></div></Field>
          </div>
        </section>
        <section class="settings-section">
          <div class="settings-section-heading"><h2>{props.t("defaultModel")}</h2></div>
          <div class="settings-form control-grid">
            <Field label={props.t("provider")}><select value={draft.settings.defaultProviderId} onChange={(event) => { const providerId = event.currentTarget.value; const provider = props.providers.find((item) => item.id === providerId); setDraft("settings", { ...draft.settings, defaultProviderId: providerId, defaultModel: provider?.models[0] ?? "" }); }}><For each={props.providers}>{(provider) => <option value={provider.id}>{provider.name}</option>}</For></select></Field>
            <Field label={props.t("model")}><select value={draft.settings.defaultModel} onChange={(event) => setDraft("settings", "defaultModel", event.currentTarget.value)}><For each={getModelsForProvider(defaultProvider())}>{(model) => <option value={model.id}>{model.label}</option>}</For></select></Field>
            <Field label={props.t("namingPattern")} class="span-2">
              <input value={draft.settings.namingPattern} spellcheck={false} onInput={(event) => setDraft("settings", "namingPattern", event.currentTarget.value)} />
              <small class="field-hint">{props.t("namingPatternHint")}</small>
            </Field>
            <Field label={props.t("defaultStream")} class="span-2">
              <select
                value={draft.settings.defaultStream === null ? "auto" : draft.settings.defaultStream ? "on" : "off"}
                onChange={(event) => {
                  const v = event.currentTarget.value;
                  setDraft("settings", "defaultStream", v === "auto" ? null : v === "on");
                }}
              >
                <option value="auto">{props.t("streamAuto")}</option>
                <option value="on">{props.t("streamOn")}</option>
                <option value="off">{props.t("streamOff")}</option>
              </select>
            </Field>
          </div>
        </section>
        <section class="settings-section">
          <div class="settings-section-heading"><h2>{props.t("settings")}</h2></div>
          <div class="settings-toggle-list">
            <Toggle checked={draft.settings.useCommonDescriptions} onChange={(value) => setDraft("settings", "useCommonDescriptions", value)} label={props.t("commonDescriptionSetting")} />
            <Toggle checked={draft.settings.saveMetadata} onChange={(value) => setDraft("settings", "saveMetadata", value)} label={props.t("saveMetadata")} />
            <Toggle checked={draft.settings.saveRawResponse} onChange={(value) => setDraft("settings", "saveRawResponse", value)} label={props.t("saveRawResponse")} />
            <Toggle checked={draft.settings.autoOpenFolder} onChange={(value) => setDraft("settings", "autoOpenFolder", value)} label={props.t("autoOpenFolder")} />
            <div class="toggle-row">
              <Toggle checked={draft.settings.flatOutput} onChange={(value) => setDraft("settings", "flatOutput", value)} label={props.t("flatOutput")} />
              <small class="field-hint">{props.t("flatOutputHint")}</small>
            </div>
          </div>
        </section>
        <section class="settings-section danger-section">
          <div class="settings-section-heading"><h2>{props.t("dangerZone")}</h2></div>
          <div class="danger-row"><div><strong>{props.t("clearHistory")}</strong><small>{props.project.name}</small></div><button class="button danger" type="button" onClick={props.onClearHistory}><Trash2 size={16} />{props.t("clearHistory")}</button></div>
        </section>
      </div>
    </div>
  );
}

export function ResultsPage(props: BaseProps & {
  history: HistoryRecord[];
  onReveal: (path: string) => void;
  onOpenFolder: () => void;
  onDownload: (asset: GeneratedAsset) => void;
}) {
  const allAssets = createMemo(() =>
    props.history
      .filter((record) => record.projectId === props.project.id && record.status === "completed")
      .flatMap((record) => record.assets.map((asset) => ({ asset, record })))
      .sort((a, b) => new Date(b.record.createdAt).getTime() - new Date(a.record.createdAt).getTime())
  );

  const [copiedId, setCopiedId] = createSignal<string | null>(null);
  const [viewMode, setViewMode] = createSignal<"grid" | "list">("grid");

  const copyImage = async (asset: GeneratedAsset, assetUrl: string) => {
    try {
      const img = new Image();
      img.crossOrigin = "anonymous";
      await new Promise<void>((resolve, reject) => { img.onload = () => resolve(); img.onerror = reject; img.src = assetUrl; });
      const canvas = document.createElement("canvas");
      canvas.width = img.naturalWidth || 512;
      canvas.height = img.naturalHeight || 512;
      canvas.getContext("2d")!.drawImage(img, 0, 0);
      await new Promise<void>((resolve, reject) => canvas.toBlob(async (blob) => {
        if (!blob) { reject(new Error("canvas toBlob failed")); return; }
        await navigator.clipboard.write([new ClipboardItem({ "image/png": blob })]);
        resolve();
      }, "image/png"));
      setCopiedId(asset.id);
      window.setTimeout(() => setCopiedId(null), 1800);
    } catch (_err) {
      // clipboard write may be blocked in some browsers; fall back silently
    }
  };

  const assetActions = (asset: GeneratedAsset) => (
    <div class="result-item-actions">
      <IconButton
        label={copiedId() === asset.id ? props.t("copied") : props.t("copyImage")}
        class={copiedId() === asset.id ? "is-success" : ""}
        onClick={() => void copyImage(asset, asset.url)}
      >
        <Show when={copiedId() === asset.id} fallback={<Clipboard size={15} />}><Check size={15} /></Show>
      </IconButton>
      <IconButton label={props.t("reveal")} onClick={() => props.onReveal(asset.filePath)}><FolderOpen size={15} /></IconButton>
      <IconButton label={props.t("download")} onClick={() => props.onDownload(asset)}><Download size={15} /></IconButton>
    </div>
  );

  return (
    <div class="page management-page results-page">
      <header class="page-header">
        <div>
          <h1>{props.t("allResults")}</h1>
          <p>{props.t("resultCount").replace("{count}", String(allAssets().length))}</p>
        </div>
        <div class="header-actions results-header-actions">
          <div class="results-view-toggle" role="group" aria-label={props.t("allResults")}>
            <IconButton label={props.t("gridView")} active={viewMode() === "grid"} onClick={() => setViewMode("grid")}><LayoutGrid size={16} /></IconButton>
            <IconButton label={props.t("listView")} active={viewMode() === "list"} onClick={() => setViewMode("list")}><ListIcon size={16} /></IconButton>
          </div>
          <button class="button secondary" type="button" onClick={props.onOpenFolder}>
            <FolderOpen size={16} />
            {props.t("openProjectFolder")}
          </button>
        </div>
      </header>

      <Show
        when={allAssets().length > 0}
        fallback={<EmptyState icon={<Images size={24} />} title={props.t("noResults")} />}
      >
        <Show
          when={viewMode() === "grid"}
          fallback={(
            <div class="results-list-full">
              <For each={allAssets()}>
                {({ asset, record }) => (
                  <article class="result-list-row">
                    <div class="result-list-thumb"><img src={asset.url} alt={asset.prompt} loading="lazy" /></div>
                    <div class="result-list-copy">
                      <strong title={asset.prompt}>{asset.prompt || record.prompt}</strong>
                      <span>{record.model} · {asset.width} x {asset.height} · {asset.format.toUpperCase()}</span>
                    </div>
                    <time class="result-list-date" dateTime={asset.createdAt}>{new Date(asset.createdAt).toLocaleDateString()}</time>
                    {assetActions(asset)}
                  </article>
                )}
              </For>
            </div>
          )}
        >
          <div class="results-grid-full">
            <For each={allAssets()}>
              {({ asset, record }) => (
                <article class="result-gallery-card">
                  <div class="result-gallery-thumb">
                    <img src={asset.url} alt={asset.prompt} loading="lazy" />
                    <span>{asset.width} x {asset.height}</span>
                  </div>
                  <div class="result-gallery-body">
                    <div class="result-gallery-copy">
                      <strong title={asset.prompt}>{asset.prompt || record.prompt}</strong>
                      <div><span>{record.model}</span><time dateTime={asset.createdAt}>{new Date(asset.createdAt).toLocaleDateString()}</time></div>
                    </div>
                    {assetActions(asset)}
                  </div>
                </article>
              )}
            </For>
          </div>
        </Show>
      </Show>
    </div>
  );
}
