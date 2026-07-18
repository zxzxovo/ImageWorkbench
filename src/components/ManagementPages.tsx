import { For, Show, createEffect, createMemo, createSignal } from "solid-js";
import { createStore } from "solid-js/store";
import {
  ArrowDown,
  ArrowUp,
  CalendarDays,
  Check,
  Clipboard,
  Copy,
  Download,
  Filter,
  FolderOpen,
  Image as ImageIcon,
  Images,
  ListChecks,
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
  X,
  GitCompare,
} from "lucide-solid";
import { formatError } from "../lib/api";
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
  onError?: (error: unknown, context: string) => void;
}

export function HistoryPage(props: BaseProps & {
  history: HistoryRecord[];
  onRerun: (record: HistoryRecord) => void;
  onContinue: (record: HistoryRecord) => void;
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
  createEffect(() => {
    props.project.id;
    setProviderId("all");
    setStatus("all");
    setModel("all");
    setDateFilter("all");
    setSelectedRecord(null);
    setCompareMode(false);
    setSelectedForCompare(new Set<string>());
    setShowCompareModal(false);
  });

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
            <button class="button secondary" type="button" onClick={() => window.confirm(props.t("confirmDeleteFailed")) && props.onDeleteFailed()}>
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
                          <IconButton label={props.t("retry")} onClick={() => props.onRerun(record)}><RotateCw size={15} /></IconButton>
                          <Show when={record.interactionId}><IconButton label={props.t("continueEditing")} onClick={() => props.onContinue(record)}><SquarePen size={15} /></IconButton></Show>
                          <IconButton label={props.t("taskDetails")} onClick={() => setSelectedRecord(record)}><MoreHorizontal size={15} /></IconButton>
                          <IconButton label={props.t("delete")} onClick={() => window.confirm(props.t("confirmDeleteHistoryRecord")) && props.onDelete(record.id)}><Trash2 size={15} /></IconButton>
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
    prefixContent: "",
    suffixContent: "",
    negativeContent: "",
    enabled: true,
    createdAt: new Date().toISOString(),
  });

  createEffect(() => {
    props.project.id;
    const first = props.project.descriptions[0];
    setSelectedId(first?.id ?? "");
    setDraft(first ? { ...first } : {
      id: "",
      title: "",
      prefixContent: "",
      suffixContent: "",
      negativeContent: "",
      enabled: true,
      createdAt: new Date().toISOString(),
    });
  });

  const select = (item: CommonDescription) => {
    setSelectedId(item.id);
    setDraft({ ...item });
  };

  const add = () => {
    const item: CommonDescription = {
      id: crypto.randomUUID(),
      title: props.t("addDescription"),
      prefixContent: "",
      suffixContent: "",
      negativeContent: "",
      enabled: true,
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
    if (!window.confirm(props.t("confirmDeleteDescription"))) return;
    const next = props.project.descriptions.filter((item) => item.id !== id);
    props.onChange(next);
    if (selectedId() === id) {
      if (next[0]) select(next[0]);
      else setSelectedId("");
    }
  };

  const move = (offset: -1 | 1) => {
    const index = props.project.descriptions.findIndex((item) => item.id === draft.id);
    const target = index + offset;
    if (index < 0 || target < 0 || target >= props.project.descriptions.length) return;
    const next = [...props.project.descriptions];
    [next[index], next[target]] = [next[target], next[index]];
    props.onChange(next);
  };

  const descriptionPreview = (item: CommonDescription) => [item.prefixContent, item.suffixContent, item.negativeContent]
    .map((value) => value.trim())
    .filter(Boolean)
    .join(" / ");

  return (
    <div class="page management-page">
      <header class="page-header"><div><h1>{props.t("descriptionTitle")}</h1><p>{props.t("descriptionSubtitle")}</p></div><button class="button primary" type="button" onClick={add}><Plus size={16} />{props.t("addDescription")}</button></header>
      <div class="split-manager">
        <aside class="manager-list">
          <For each={props.project.descriptions}>
            {(item) => (
              <button type="button" class={`manager-list-item ${selectedId() === item.id ? "is-selected" : ""}`} onClick={() => select(item)}>
                <span class="description-part-badges">
                  <Show when={item.prefixContent.trim()}><small class="placement-prefix">P</small></Show>
                  <Show when={item.suffixContent.trim()}><small class="placement-suffix">S</small></Show>
                  <Show when={item.negativeContent.trim()}><small class="placement-negative">N</small></Show>
                </span>
                <span><strong>{item.title}</strong><small>{descriptionPreview(item) || "-"}</small></span>
                <span class={`enabled-indicator ${item.enabled ? "is-enabled" : ""}`}><Check size={12} /></span>
              </button>
            )}
          </For>
          <Show when={props.project.descriptions.length === 0}><EmptyState icon={<Sparkles size={22} />} title={props.t("addDescription")} /></Show>
        </aside>
        <section class="manager-editor">
          <Show when={selectedId()} fallback={<EmptyState icon={<SlidersHorizontal size={24} />} title={props.t("addDescription")} />}>
            <div class="editor-heading">
              <div><span class="section-kicker">{props.t("edit")}</span><h2>{draft.title}</h2></div>
              <div class="editor-heading-actions">
                <IconButton label={props.t("moveUp")} disabled={props.project.descriptions[0]?.id === draft.id} onClick={() => move(-1)}><ArrowUp size={16} /></IconButton>
                <IconButton label={props.t("moveDown")} disabled={props.project.descriptions.at(-1)?.id === draft.id} onClick={() => move(1)}><ArrowDown size={16} /></IconButton>
                <IconButton label={props.t("delete")} onClick={() => remove(draft.id)}><Trash2 size={16} /></IconButton>
              </div>
            </div>
            <div class="editor-form">
              <Field label={props.t("name")}><input value={draft.title} onInput={(event) => setDraft("title", event.currentTarget.value)} /></Field>
              <div class="description-content-grid">
                <Field label={props.t("prefixContent")}><textarea rows="6" value={draft.prefixContent} onInput={(event) => setDraft("prefixContent", event.currentTarget.value)} /></Field>
                <Field label={props.t("suffixContent")}><textarea rows="6" value={draft.suffixContent} onInput={(event) => setDraft("suffixContent", event.currentTarget.value)} /></Field>
              </div>
              <details class="description-extra-settings" open={Boolean(draft.negativeContent)}>
                <summary><SlidersHorizontal size={15} />{props.t("descriptionExtraSettings")}</summary>
                <Field label={props.t("negativeContent")}><textarea rows="4" value={draft.negativeContent} onInput={(event) => setDraft("negativeContent", event.currentTarget.value)} /></Field>
              </details>
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
      const provider = props.providers.find((item) => item.enabled && item.models.length > 0);
      setDraft({
        id: crypto.randomUUID(), name: "", description: "", providerId: provider?.id ?? "", model: provider?.models[0] ?? "", mode: "generate",
        aspectRatio: "1:1", size: "1K", quality: "auto", outputFormat: "png", promptTemplate: "", createdAt: new Date().toISOString(),
      });
    }
    setModalOpen(true);
  };

  const save = () => {
    if (!draft.name.trim()) return;
    const provider = props.providers.find((item) => item.id === draft.providerId && item.enabled && item.models.includes(draft.model));
    if (!provider) return;
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
                <header><span class="preset-icon"><SlidersHorizontal size={17} /></span></header>
                <div><h2>{preset.name}</h2><p>{preset.description}</p></div>
                <div class="preset-specs"><span>{getModelLabel(preset.model)}</span><span>{preset.aspectRatio}</span><span>{preset.size}</span><span>{preset.outputFormat.toUpperCase()}</span></div>
                <footer>
                  <button class="button primary compact" type="button" onClick={() => props.onApply(preset)}><Play size={15} />{props.t("applyPreset")}</button>
                  <IconButton label={props.t("edit")} onClick={() => openEditor(preset)}><SlidersHorizontal size={15} /></IconButton>
                  <IconButton label={props.t("duplicate")} onClick={() => duplicate(preset)}><Copy size={15} /></IconButton>
                  <IconButton label={props.t("delete")} onClick={() => window.confirm(props.t("confirmDeletePreset")) && props.onChange(props.project.presets.filter((item) => item.id !== preset.id))}><Trash2 size={15} /></IconButton>
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
            <Field label={props.t("provider")}><select value={draft.providerId} onChange={(event) => { const providerId = event.currentTarget.value; const provider = props.providers.find((item) => item.id === providerId); setDraft({ providerId, model: provider?.models[0] ?? "" }); }}><For each={props.providers.filter((provider) => provider.enabled && provider.models.length > 0)}>{(provider) => <option value={provider.id}>{provider.name}</option>}</For></select></Field>
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
  onChange: (project: Project) => void | Promise<void>;
  onClearHistory: () => void;
}) {
  const [draft, setDraft] = createStore<Project>({ ...props.project, settings: { ...props.project.settings } });
  const defaultProvider = createMemo(() => props.providers.find((item) => item.id === draft.settings.defaultProviderId));
  const [saved, setSaved] = createSignal(false);
  const [saving, setSaving] = createSignal(false);
  const [saveError, setSaveError] = createSignal("");

  createEffect(() => {
    props.project.id;
    setDraft({ ...props.project, settings: { ...props.project.settings } });
    setSaved(false);
    setSaveError("");
  });

  const save = async () => {
    if (!draft.name.trim() || !draft.settings.defaultProviderId || !draft.settings.defaultModel) {
      setSaveError(props.t("projectSettingsValidation"));
      return;
    }
    setSaving(true);
    setSaveError("");
    try {
      await props.onChange({ ...draft, name: draft.name.trim(), updatedAt: new Date().toISOString() });
      setSaved(true);
      window.setTimeout(() => setSaved(false), 1400);
    } catch (error) {
      setSaveError(formatError(error));
      props.onError?.(error, "project_settings.save");
    } finally {
      setSaving(false);
    }
  };

  return (
    <div class="page management-page settings-page">
      <header class="page-header"><div><h1>{props.t("projectSettings")}</h1><p>{props.project.name}</p></div><button class="button primary" type="button" disabled={saving()} onClick={() => void save()}><Show when={saved()} fallback={<Save size={16} />}><Check size={16} /></Show>{saving() ? props.t("saving") : saved() ? props.t("saved") : props.t("save")}</button></header>
      <Show when={saveError()}><p class="form-error page-form-error" role="alert">{saveError()}</p></Show>
      <div class="settings-layout">
        <section class="settings-section">
          <div class="settings-section-heading"><h2>{props.t("projectInfo")}</h2></div>
          <div class="settings-form">
            <Field label={props.t("projectName")}><input value={draft.name} onInput={(event) => setDraft("name", event.currentTarget.value)} /></Field>
            <Field label={props.t("projectDescription")}><textarea rows="3" value={draft.description} onInput={(event) => setDraft("description", event.currentTarget.value)} /></Field>
            <Field label={props.t("storagePath")} hint={props.t("storagePathMoveHint")}><input value={draft.storagePath} readonly aria-readonly="true" /></Field>
          </div>
        </section>
        <section class="settings-section">
          <div class="settings-section-heading"><h2>{props.t("defaultModel")}</h2></div>
          <div class="settings-form control-grid">
            <Field label={props.t("provider")}><select value={draft.settings.defaultProviderId} onChange={(event) => { const providerId = event.currentTarget.value; const provider = props.providers.find((item) => item.id === providerId); setDraft("settings", { ...draft.settings, defaultProviderId: providerId, defaultModel: provider?.models[0] ?? "" }); }}><For each={props.providers.filter((provider) => provider.enabled && provider.models.length > 0)}>{(provider) => <option value={provider.id}>{provider.name}</option>}</For></select></Field>
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
            <Toggle
              checked={draft.settings.flatOutput}
              onChange={(value) => setDraft("settings", "flatOutput", value)}
              label={props.t("flatOutput")}
              description={props.t("flatOutputHint")}
            />
          </div>
        </section>
        <section class="settings-section danger-section">
          <div class="settings-section-heading"><h2>{props.t("dangerZone")}</h2></div>
          <div class="danger-row"><div><strong>{props.t("clearHistory")}</strong><small>{props.project.name}</small></div><button class="button danger" type="button" onClick={() => window.confirm(props.t("confirmClearHistory")) && props.onClearHistory()}><Trash2 size={16} />{props.t("clearHistory")}</button></div>
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
  onDeleteSelected: (assets: Array<{ runId: string; outputId: string }>) => Promise<string[]>;
  onDownloadSelected: (assets: GeneratedAsset[]) => Promise<number>;
}) {
  const allAssets = createMemo(() =>
    props.history
      .filter((record) => record.projectId === props.project.id && record.status === "completed")
      .flatMap((record) => record.assets.map((asset) => ({ asset, record })))
      .sort((a, b) => new Date(b.record.createdAt).getTime() - new Date(a.record.createdAt).getTime())
  );

  const [copiedId, setCopiedId] = createSignal<string | null>(null);
  const [viewMode, setViewMode] = createSignal<"grid" | "list">("grid");
  const [copyError, setCopyError] = createSignal("");
  const [selectionMode, setSelectionMode] = createSignal(false);
  const [selectedAssetIds, setSelectedAssetIds] = createSignal<Set<string>>(new Set());
  const [deleteConfirmOpen, setDeleteConfirmOpen] = createSignal(false);
  const [batchBusy, setBatchBusy] = createSignal<"delete" | "download" | null>(null);
  const [batchMessage, setBatchMessage] = createSignal("");
  const selectedResults = createMemo(() => allAssets().filter(({ asset }) => selectedAssetIds().has(asset.id)));

  createEffect(() => {
    props.project.id;
    setSelectionMode(false);
    setSelectedAssetIds(new Set<string>());
    setDeleteConfirmOpen(false);
    setBatchMessage("");
    setCopyError("");
  });

  const toggleSelectionMode = () => {
    setSelectionMode((enabled) => !enabled);
    setSelectedAssetIds(new Set<string>());
    setBatchMessage("");
  };

  const toggleAssetSelection = (assetId: string) => {
    setSelectedAssetIds((selected) => {
      const next = new Set(selected);
      if (next.has(assetId)) next.delete(assetId);
      else next.add(assetId);
      return next;
    });
  };

  const selectAllAssets = () => setSelectedAssetIds(new Set(allAssets().map(({ asset }) => asset.id)));

  const downloadSelected = async () => {
    if (selectedResults().length === 0) return;
    setBatchBusy("download");
    setBatchMessage("");
    try {
      const exported = await props.onDownloadSelected(selectedResults().map(({ asset }) => asset));
      if (exported > 0) {
        setBatchMessage(props.t("batchExportComplete").replace("{count}", String(exported)));
      }
    } catch (error) {
      setBatchMessage(formatError(error));
    } finally {
      setBatchBusy(null);
    }
  };

  const deleteSelected = async () => {
    if (selectedResults().length === 0) return;
    setBatchBusy("delete");
    setBatchMessage("");
    try {
      const deletedIds = await props.onDeleteSelected(selectedResults().map(({ asset, record }) => ({
        runId: record.id,
        outputId: asset.id,
      })));
      const deleted = new Set(deletedIds);
      setSelectedAssetIds((selected) => new Set([...selected].filter((id) => !deleted.has(id))));
      setDeleteConfirmOpen(false);
      if (deleted.size > 0) {
        setBatchMessage(props.t("batchDeleteComplete").replace("{count}", String(deleted.size)));
      }
      if (deleted.size > 0 && selectedAssetIds().size === 0) setSelectionMode(false);
    } catch (error) {
      setBatchMessage(formatError(error));
    } finally {
      setBatchBusy(null);
    }
  };

  const copyImage = async (asset: GeneratedAsset, assetUrl: string) => {
    setCopyError("");
    try {
      const img = new Image();
      img.crossOrigin = "anonymous";
      await new Promise<void>((resolve, reject) => {
        img.onload = () => resolve();
        img.onerror = () => reject(new Error(props.t("copyImageFailed")));
        img.src = assetUrl;
      });
      const canvas = document.createElement("canvas");
      canvas.width = img.naturalWidth || 512;
      canvas.height = img.naturalHeight || 512;
      canvas.getContext("2d")!.drawImage(img, 0, 0);
      const blob = await new Promise<Blob>((resolve, reject) => canvas.toBlob((blob) => {
        if (!blob) { reject(new Error("canvas toBlob failed")); return; }
        resolve(blob);
      }, "image/png"));
      await navigator.clipboard.write([new ClipboardItem({ "image/png": blob })]);
      setCopiedId(asset.id);
      window.setTimeout(() => setCopiedId(null), 1800);
    } catch (error) {
      setCopyError(formatError(error));
      props.onError?.(error, "results.copy_image");
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
          <button class={`button ${selectionMode() ? "primary" : "secondary"}`} type="button" onClick={toggleSelectionMode}>
            <Show when={selectionMode()} fallback={<ListChecks size={16} />}><X size={16} /></Show>
            {props.t(selectionMode() ? "exitSelection" : "multiSelect")}
          </button>
          <button class="button secondary" type="button" onClick={props.onOpenFolder}>
            <FolderOpen size={16} />
            {props.t("openProjectFolder")}
          </button>
        </div>
      </header>
      <Show when={copyError()}><p class="form-error page-form-error" role="alert">{copyError()}</p></Show>
      <Show when={batchMessage()}><p class="results-batch-message" role="status">{batchMessage()}</p></Show>

      <Show when={selectionMode()}>
        <div class="results-selection-toolbar">
          <div>
            <strong>{props.t("selectedImagesCount").replace("{count}", String(selectedAssetIds().size))}</strong>
            <span>{props.t("resultCount").replace("{count}", String(allAssets().length))}</span>
          </div>
          <div>
            <button class="button ghost compact" type="button" onClick={selectAllAssets} disabled={selectedAssetIds().size === allAssets().length}><Check size={15} />{props.t("selectAll")}</button>
            <button class="button ghost compact" type="button" onClick={() => setSelectedAssetIds(new Set<string>())} disabled={selectedAssetIds().size === 0}><X size={15} />{props.t("clearSelection")}</button>
            <button class="button secondary compact" type="button" onClick={() => void downloadSelected()} disabled={selectedAssetIds().size === 0 || batchBusy() !== null}><Download size={15} />{props.t("downloadSelected")}</button>
            <button class="button danger compact" type="button" onClick={() => setDeleteConfirmOpen(true)} disabled={selectedAssetIds().size === 0 || batchBusy() !== null}><Trash2 size={15} />{props.t("deleteSelected")}</button>
          </div>
        </div>
      </Show>

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
                  <article class={`result-list-row ${selectedAssetIds().has(asset.id) ? "is-selected" : ""} ${selectionMode() ? "is-selectable" : ""}`}>
                    <Show when={selectionMode()}>
                      <label class="result-selection-check">
                        <input type="checkbox" aria-label={props.t("selectResult")} checked={selectedAssetIds().has(asset.id)} onChange={() => toggleAssetSelection(asset.id)} />
                        <span><Check size={13} /></span>
                      </label>
                    </Show>
                    <div class="result-list-thumb"><img src={asset.url} alt={asset.prompt} loading="lazy" /></div>
                    <div class="result-list-copy">
                      <strong title={asset.prompt}>{asset.prompt || record.prompt}</strong>
                      <span>{record.model} · {asset.width} x {asset.height} · {asset.format.toUpperCase()}</span>
                    </div>
                    <time class="result-list-date" dateTime={asset.createdAt}>{new Date(asset.createdAt).toLocaleDateString()}</time>
                    <Show when={!selectionMode()}>{assetActions(asset)}</Show>
                  </article>
                )}
              </For>
            </div>
          )}
        >
          <div class="results-grid-full">
            <For each={allAssets()}>
              {({ asset, record }) => (
                <article class={`result-gallery-card ${selectedAssetIds().has(asset.id) ? "is-selected" : ""} ${selectionMode() ? "is-selectable" : ""}`}>
                  <Show when={selectionMode()}>
                    <label class="result-selection-check result-selection-overlay">
                      <input type="checkbox" aria-label={props.t("selectResult")} checked={selectedAssetIds().has(asset.id)} onChange={() => toggleAssetSelection(asset.id)} />
                      <span><Check size={13} /></span>
                    </label>
                  </Show>
                  <div class="result-gallery-thumb">
                    <img src={asset.url} alt={asset.prompt} loading="lazy" />
                    <span>{asset.width} x {asset.height}</span>
                  </div>
                  <div class="result-gallery-body">
                    <div class="result-gallery-copy">
                      <strong title={asset.prompt}>{asset.prompt || record.prompt}</strong>
                      <div><span>{record.model}</span><time dateTime={asset.createdAt}>{new Date(asset.createdAt).toLocaleDateString()}</time></div>
                    </div>
                    <Show when={!selectionMode()}>{assetActions(asset)}</Show>
                  </div>
                </article>
              )}
            </For>
          </div>
        </Show>
      </Show>
      <Modal
        open={deleteConfirmOpen()}
        title={props.t("batchDeleteTitle")}
        subtitle={props.t("batchDeleteDescription").replace("{count}", String(selectedAssetIds().size))}
        onClose={() => !batchBusy() && setDeleteConfirmOpen(false)}
        footer={(
          <>
            <button class="button secondary" type="button" disabled={batchBusy() !== null} onClick={() => setDeleteConfirmOpen(false)}>{props.t("cancel")}</button>
            <button class="button danger" type="button" disabled={batchBusy() !== null} onClick={() => void deleteSelected()}><Trash2 size={16} />{props.t("confirmDeleteSelected")}</button>
          </>
        )}
      >
        <div class="batch-delete-summary">
          <Images size={20} />
          <span>{props.t("batchDeleteLocalHint")}</span>
        </div>
      </Modal>
    </div>
  );
}
