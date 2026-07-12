import { For, Match, Show, Switch, createEffect, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import { createStore } from "solid-js/store";
import {
  Archive,
  ChevronDown,
  FolderOpen,
  FolderPlus,
  History,
  Languages,
  Library,
  Menu,
  Plus,
  Settings,
  SlidersHorizontal,
  Sparkles,
} from "lucide-solid";
import appIcon from "./assets/app-icon.png";
import CreatorPage from "./components/CreatorPage";
import {
  DescriptionsPage,
  HistoryPage,
  PresetsPage,
  ProjectSettingsPage,
} from "./components/ManagementPages";
import ProjectModal from "./components/ProjectModal";
import ProviderModal from "./components/ProviderModal";
import { IconButton, StatusDot } from "./components/common";
import { demoWorkspace, initialDraft, starterProviders } from "./data/demo";
import { api } from "./lib/api";
import { translate, type TranslationKey } from "./lib/i18n";
import { getModelCapabilities, getProviderAccent } from "./lib/models";
import { normalizeDraftForModel } from "./lib/prompt";
import type {
  GenerationPreset,
  GenerationTask,
  HistoryRecord,
  Locale,
  Project,
  ProviderProfile,
  WorkspaceSnapshot,
  WorkspaceTab,
} from "./types";

const tabIcons: Record<WorkspaceTab, typeof Sparkles> = {
  create: Sparkles,
  history: History,
  descriptions: Library,
  presets: SlidersHorizontal,
  "project-settings": Settings,
};

const tabKeys: Record<WorkspaceTab, TranslationKey> = {
  create: "create",
  history: "history",
  descriptions: "descriptions",
  presets: "presets",
  "project-settings": "projectSettings",
};

const CAPABILITY_REGISTRY_VERSION = "2026-07-12";

function projectAssetPath(projectRoot: string, path: string): string {
  if (/^(?:[A-Za-z]:[\\/]|\/)/.test(path) || /^https?:\/\//i.test(path)) return path;
  return `${projectRoot.replace(/[\\/]+$/, "")}/${path.replace(/^[\\/]+/, "")}`;
}

export default function App() {
  const initialWorkspace = api.isDemo
    ? demoWorkspace
    : { locale: "zh-CN" as Locale, projects: [], providers: starterProviders, history: [], activeProjectId: "" };
  const initialGenerationDraft = api.isDemo
    ? initialDraft
    : { ...initialDraft, providerId: starterProviders[0].id, model: starterProviders[0].models[0] };
  const initialProvider = initialWorkspace.providers.find((item) => item.id === initialGenerationDraft.providerId)
    ?? initialWorkspace.providers[0];
  const normalizedInitialDraft = normalizeDraftForModel(
    initialGenerationDraft,
    getModelCapabilities(initialProvider, initialGenerationDraft.model),
  );
  const [locale, setLocale] = createSignal<Locale>(initialWorkspace.locale);
  const [projects, setProjects] = createSignal<Project[]>(initialWorkspace.projects);
  const [providers, setProviders] = createSignal<ProviderProfile[]>(initialWorkspace.providers);
  const [history, setHistory] = createSignal<HistoryRecord[]>(initialWorkspace.history);
  const [tasks, setTasks] = createSignal<GenerationTask[]>([]);
  const [activeProjectId, setActiveProjectId] = createSignal(initialWorkspace.activeProjectId);
  const [activeTab, setActiveTab] = createSignal<WorkspaceTab>("create");
  const [providerModalOpen, setProviderModalOpen] = createSignal(false);
  const [projectModalOpen, setProjectModalOpen] = createSignal(false);
  const [projectMenuOpen, setProjectMenuOpen] = createSignal(false);
  const [sidebarCollapsed, setSidebarCollapsed] = createSignal(false);
  const [hydrated, setHydrated] = createSignal(false);
  const [backendError, setBackendError] = createSignal("");
  const [notice, setNotice] = createSignal("");
  const [queuePaused, setQueuePaused] = createSignal(false);
  const [queueControlBusy, setQueueControlBusy] = createSignal(false);
  const [activePresetId, setActivePresetId] = createSignal<string>();
  const [composedPromptOverride, setComposedPromptOverride] = createSignal<string>();
  const [contextSnapshotOverride, setContextSnapshotOverride] = createSignal<Project["descriptions"]>();
  const [presetSnapshotOverride, setPresetSnapshotOverride] = createSignal<GenerationPreset>();
  const cancelledTaskIds = new Set<string>();
  const localPendingTaskIds = new Set<string>();
  const loadedProjectIds = new Set<string>();
  const [draft, setDraft] = createStore({ ...normalizedInitialDraft, references: [...normalizedInitialDraft.references] });
  const t = (key: TranslationKey) => translate(locale(), key);
  const project = createMemo(() => projects().find((item) => item.id === activeProjectId()) ?? projects()[0]);
  const projectHistoryCount = (projectId: string) => history().filter((item) => item.projectId === projectId && item.status === "completed").reduce((total, item) => total + item.assets.length, 0);

  const setDraftProviderModel = (provider: ProviderProfile | undefined, preferredModel: string, overrides: Partial<typeof draft> = {}) => {
    const model = provider?.models.includes(preferredModel) ? preferredModel : provider?.models[0] ?? "";
    const candidate = { ...draft, ...overrides, providerId: provider?.id ?? "", model, references: [...(overrides.references ?? draft.references)] };
    setDraft({ ...normalizeDraftForModel(candidate, getModelCapabilities(provider, model)) });
  };

  const outcomeFromResult = (result: { responseParts: HistoryRecord["responseParts"] }): { status: GenerationTask["status"]; error?: string } => {
    const remoteJob = result.responseParts.find((part) => part.type === "remote_job");
    if (remoteJob?.type !== "remote_job") return { status: "completed" as const, error: undefined };
    if (remoteJob.status === "queued" || remoteJob.status === "running") return { status: remoteJob.status as "queued" | "running", error: undefined };
    if (["failed", "cancelled", "expired"].includes(remoteJob.status)) {
      return { status: "failed" as const, error: `Remote ${remoteJob.kind} ${remoteJob.status}` };
    }
    if (remoteJob.status === "succeeded") return { status: "completed" as const, error: undefined };
    return { status: "running" as const, error: undefined };
  };

  const remotePollsInFlight = new Set<string>();
  const pollProjectRemoteTasks = async (projectId: string) => {
    if (api.isDemo || !projectId || remotePollsInFlight.has(projectId)) return;
    remotePollsInFlight.add(projectId);
    try {
      const results = await api.pollRemoteTasks(projectId);
      for (const result of results) {
        if (!result.runId) continue;
        const { status, error } = outcomeFromResult(result);
        const applyResult = <T extends GenerationTask>(item: T): T => item.id !== result.runId ? item : ({
          ...item,
          status,
          progress: status === "completed" ? 100 : status === "queued" ? 12 : 55,
          assets: result.assets.map((asset) => ({ ...asset, taskId: item.id })),
          responseParts: result.responseParts,
          requestId: result.requestId,
          interactionId: result.interactionId,
          usage: result.usage,
          error,
        }) as T;
        setTasks((items) => items.map(applyResult));
        setHistory((items) => items.map(applyResult));
      }
    } catch (error) {
      setBackendError(error instanceof Error ? error.message : String(error));
    } finally {
      remotePollsInFlight.delete(projectId);
    }
  };

  const mergePortableHistory = (records: HistoryRecord[]) => {
    const withProviderNames = records.map((record) => ({
      ...record,
      providerName: providers().find((provider) => provider.id === record.providerId)?.name ?? record.providerName,
    }));
    setHistory((items) => [
      ...withProviderNames.filter((record) => !items.some((item) => item.id === record.id)),
      ...items.map((item) => {
        const restored = withProviderNames.find((record) => record.id === item.id);
        return restored ? { ...restored, favorite: item.favorite } : item;
      }),
    ]);
  };

  onMount(async () => {
    try {
      setQueuePaused(await api.queueStatus());
    } catch (error) {
      setBackendError(error instanceof Error ? error.message : String(error));
    }
    try {
      const saved = await api.loadWorkspace();
      if (saved) {
        setLocale(saved.locale);
        setProjects(saved.projects);
        setProviders(saved.providers);
        setHistory(saved.history);
        setActiveProjectId(saved.activeProjectId);
        const savedProject = saved.projects.find((item) => item.id === saved.activeProjectId);
        if (savedProject) {
          const savedProvider = saved.providers.find((item) => item.id === savedProject.settings.defaultProviderId)
            ?? saved.providers.find((item) => item.enabled)
            ?? saved.providers[0];
          setDraftProviderModel(savedProvider, savedProject.settings.defaultModel);
        }
        if (!api.isDemo && saved.projects.length > 0) {
          void Promise.all(saved.projects.map(async (savedItem) => ({ id: savedItem.id, details: await api.loadProjectDetails(savedItem.id) })))
            .then((loaded) => {
              loaded.forEach((entry) => loadedProjectIds.add(entry.id));
              setProjects((items) => items.map((item) => {
                const match = loaded.find((entry) => entry.id === item.id);
                return match ? { ...item, descriptions: match.details.descriptions, presets: match.details.presets } : item;
              }));
              loaded.forEach((entry) => mergePortableHistory(entry.details.history));
            })
            .catch((error: unknown) => setBackendError(error instanceof Error ? error.message : String(error)));
        }
        void pollProjectRemoteTasks(saved.activeProjectId);
        if (!api.isDemo && saved.projects.length === 0) setProjectModalOpen(true);
      } else if (!api.isDemo) {
        setProjectModalOpen(true);
      }
      setHydrated(true);
    } catch (error) {
      setBackendError(error instanceof Error ? error.message : String(error));
    }
  });

  createEffect(() => {
    if (!hydrated()) return;
    const snapshot: WorkspaceSnapshot = {
      locale: locale(),
      activeProjectId: activeProjectId(),
      projects: projects(),
      providers: providers(),
      history: history(),
    };
    void api.saveWorkspace(snapshot).catch((error: unknown) => {
      setBackendError(error instanceof Error ? error.message : String(error));
    });
  });

  const closeMenus = (event: KeyboardEvent) => {
    if (event.key !== "Escape") return;
    setProviderModalOpen(false);
    setProjectModalOpen(false);
    setProjectMenuOpen(false);
  };
  onMount(() => window.addEventListener("keydown", closeMenus));
  onCleanup(() => window.removeEventListener("keydown", closeMenus));

  let unlistenGenerationEvents: (() => void) | undefined;
  onMount(() => {
    void api.listenGenerationEvents(({ runId, event }) => {
      setTasks((items) => items.map((task) => {
        if (task.id !== runId) return task;
        const type = typeof event.type === "string" ? event.type : "";
        if (type === "started") {
          return { ...task, status: "running", progress: Math.max(task.progress, 10), requestId: typeof event.requestId === "string" ? event.requestId : task.requestId };
        }
        if (type === "partial_image" && event.image && typeof event.image === "object") {
          const image = event.image as Record<string, unknown>;
          const url = image.type === "url" && typeof image.url === "string"
            ? image.url
            : image.type === "base64" && typeof image.data === "string"
              ? image.data.startsWith("data:") ? image.data : `data:image/png;base64,${image.data}`
              : "";
          if (!url) return task;
          const index = typeof event.index === "number" ? event.index : task.partialImages?.length ?? 0;
          const partialImages = [...(task.partialImages ?? []).filter((item) => item.index !== index), { index, url }].sort((a, b) => a.index - b.index);
          return { ...task, partialImages, progress: Math.min(92, Math.max(task.progress + 8, 24)) };
        }
        if (type === "checkpoint") {
          return { ...task, progress: Math.min(94, task.progress + 5), interactionId: typeof event.eventId === "string" ? event.eventId : task.interactionId };
        }
        if (type === "progress") return { ...task, progress: Math.min(94, task.progress + 4) };
        if (type === "completed") return { ...task, progress: Math.max(task.progress, 96) };
        return task;
      }));
    }).then((unlisten) => { unlistenGenerationEvents = unlisten; }).catch((error: unknown) => {
      setBackendError(error instanceof Error ? error.message : String(error));
    });
  });
  onCleanup(() => unlistenGenerationEvents?.());
  const remotePollTimer = api.isDemo ? undefined : window.setInterval(() => {
    for (const item of projects()) void pollProjectRemoteTasks(item.id);
  }, 5000);
  onCleanup(() => { if (remotePollTimer !== undefined) window.clearInterval(remotePollTimer); });

  const selectProject = (projectId: string) => {
    setActiveProjectId(projectId);
    setActivePresetId(undefined);
    setComposedPromptOverride(undefined);
    setContextSnapshotOverride(undefined);
    setPresetSnapshotOverride(undefined);
    const next = projects().find((item) => item.id === projectId);
    if (next) {
      const nextProvider = providers().find((item) => item.id === next.settings.defaultProviderId)
        ?? providers().find((item) => item.enabled)
        ?? providers()[0];
      setDraftProviderModel(nextProvider, next.settings.defaultModel);
    }
    setProjectMenuOpen(false);
    if (!api.isDemo && !loadedProjectIds.has(projectId)) {
      void api.loadProjectDetails(projectId).then((details) => {
        loadedProjectIds.add(projectId);
        setProjects((items) => items.map((item) => item.id === projectId ? { ...item, descriptions: details.descriptions, presets: details.presets } : item));
        mergePortableHistory(details.history);
      }).catch((error: unknown) => setBackendError(error instanceof Error ? error.message : String(error)));
    }
    void pollProjectRemoteTasks(projectId);
  };

  const upsertProvider = (provider: ProviderProfile) => {
    setProviders((items) => items.some((item) => item.id === provider.id)
      ? items.map((item) => item.id === provider.id ? provider : item)
      : [...items, provider]);
    if (!draft.providerId) {
      setDraftProviderModel(provider, provider.models[0] ?? "");
    }
  };

  const deleteProvider = async (providerId: string): Promise<boolean> => {
    try {
      if (!(await api.deleteProvider(providerId))) return false;
    } catch (error) {
      setBackendError(error instanceof Error ? error.message : String(error));
      return false;
    }
    const remaining = providers().filter((item) => item.id !== providerId);
    const fallback = remaining.find((item) => item.enabled) ?? remaining[0];
    const dependentProjects = projects().filter((item) => item.settings.defaultProviderId === providerId);
    setProviders(remaining);
    if (dependentProjects.length > 0) {
      if (fallback) {
        void Promise.all(dependentProjects.map((item) => api.remapProjectProvider(item.id, providerId, fallback.id)))
          .catch((error: unknown) => setBackendError(error instanceof Error ? error.message : String(error)));
      }
      setProjects((items) => items.map((item) => item.settings.defaultProviderId === providerId ? {
        ...item,
        settings: { ...item.settings, defaultProviderId: fallback?.id ?? "", defaultModel: fallback?.models[0] ?? "" },
      } : item));
      setNotice(t("providerRemapped"));
      window.setTimeout(() => setNotice(""), 4200);
    }
    if (draft.providerId === providerId) {
      setDraftProviderModel(fallback, fallback?.models[0] ?? "");
    }
    return true;
  };

  const createProject = (nextProject: Project) => {
    loadedProjectIds.add(nextProject.id);
    setProjects((items) => [nextProject, ...items]);
    selectProject(nextProject.id);
    setProjectModalOpen(false);
  };

  const openProject = async () => {
    const summary = await api.openProject();
    if (!summary) return;
    const details = await api.loadProjectDetails(summary.id);
    loadedProjectIds.add(summary.id);
    mergePortableHistory(details.history);
    const mappedProvider = providers().find((item) => item.id === summary.defaultProviderProfileId);
    const provider = mappedProvider
      ?? providers().find((item) => item.enabled)
      ?? providers()[0];
    const defaultModel = summary.defaultModelId && provider?.models.includes(summary.defaultModelId)
      ? summary.defaultModelId
      : provider?.models[0] ?? "";
    if (summary.defaultProviderProfileId && !mappedProvider && provider) {
      await api.remapProjectProvider(summary.id, summary.defaultProviderProfileId, provider.id);
    }
    const openedProject: Project = {
      id: summary.id,
      name: summary.name,
      description: "",
      storagePath: summary.rootPath,
      color: "#4e6e9c",
      createdAt: summary.createdAt,
      updatedAt: summary.updatedAt,
      descriptions: details.descriptions,
      presets: details.presets,
      settings: {
        useCommonDescriptions: false,
        saveMetadata: true,
        saveRawResponse: false,
        autoOpenFolder: false,
        namingPattern: "{date}_{model}_{index}",
        defaultProviderId: provider?.id ?? "",
        defaultModel,
      },
    };
    setProjects((items) => items.some((item) => item.id === openedProject.id)
      ? items.map((item) => item.id === openedProject.id ? { ...item, ...openedProject } : item)
      : [openedProject, ...items]);
    if (summary.defaultProviderProfileId && !mappedProvider) {
      setNotice(t("providerRemapped"));
      window.setTimeout(() => setNotice(""), 4200);
    }
    selectProject(openedProject.id);
  };

  const updateProject = (nextProject: Project) => {
    setProjects((items) => items.map((item) => item.id === nextProject.id ? nextProject : item));
  };

  const updateActiveProject = (updater: (current: Project) => Project) => {
    const current = project();
    if (!current) return;
    updateProject(updater(current));
  };

  const changeDescriptions = (nextDescriptions: Project["descriptions"]) => {
    const currentProject = project();
    if (!currentProject) return;
    const deleted = currentProject.descriptions.filter((item) => !nextDescriptions.some((next) => next.id === item.id));
    void Promise.all([
      ...deleted.map((item) => api.deletePromptContext(currentProject.id, item.id)),
      ...nextDescriptions.map((item, index) => api.upsertPromptContext(currentProject.id, item, index)),
    ]).catch((error: unknown) => setBackendError(error instanceof Error ? error.message : String(error)));
    updateActiveProject((item) => ({ ...item, descriptions: nextDescriptions, updatedAt: new Date().toISOString() }));
  };

  const changePresets = (nextPresets: Project["presets"]) => {
    const currentProject = project();
    if (!currentProject) return;
    const deleted = currentProject.presets.filter((item) => !nextPresets.some((next) => next.id === item.id));
    void Promise.all([
      ...deleted.map((item) => api.deleteGenerationPreset(currentProject.id, item.id)),
      ...nextPresets.map((item) => api.upsertGenerationPreset(currentProject.id, item)),
    ]).catch((error: unknown) => setBackendError(error instanceof Error ? error.message : String(error)));
    updateActiveProject((item) => ({ ...item, presets: nextPresets, updatedAt: new Date().toISOString() }));
  };

  const generate = async (composedPrompt: string) => {
    const currentProject = project();
    const provider = providers().find((item) => item.id === draft.providerId);
    if (!currentProject || !provider) return;
    const effectivePrompt = composedPromptOverride() ?? composedPrompt;
    const replayContexts = contextSnapshotOverride();
    const replayPreset = presetSnapshotOverride();
    setComposedPromptOverride(undefined);
    setContextSnapshotOverride(undefined);
    setPresetSnapshotOverride(undefined);
    const requestDraft = {
      ...draft,
      size: draft.size === "custom" ? `${draft.customWidth}x${draft.customHeight}` : draft.size,
      references: [...draft.references],
    };
    const contextSnapshot = replayContexts ?? (currentProject.settings.useCommonDescriptions
      ? currentProject.descriptions.filter((item) => item.enabled && item.content.trim())
      : []);
    const presetSnapshot = replayPreset ?? currentProject.presets.find((item) => item.id === activePresetId());
    const taskId = crypto.randomUUID();
    const createdAt = new Date().toISOString();
    const task: GenerationTask = {
      id: taskId,
      projectId: currentProject.id,
      providerId: provider.id,
      providerName: provider.name,
      model: draft.model,
      prompt: draft.prompt,
      composedPrompt: effectivePrompt,
      mode: draft.mode,
      status: queuePaused() ? "queued" : "running",
      progress: queuePaused() ? 4 : 8,
      count: draft.count,
      createdAt,
      assets: [],
      responseParts: [],
    };
    localPendingTaskIds.add(taskId);
    setTasks((items) => [task, ...items]);
    const startedAt = performance.now();
    const progressTimer = window.setInterval(() => {
      setTasks((items) => items.map((item) => item.id === taskId && item.status === "running" ? { ...item, progress: Math.min(88, item.progress + 9) } : item));
    }, 260);

    try {
      const result = await api.generate({
        clientTaskId: taskId,
        projectId: currentProject.id,
        storagePath: currentProject.storagePath,
        provider,
        draft: requestDraft,
        composedPrompt: effectivePrompt,
        contextIds: contextSnapshot.map((item) => item.id),
        presetId: activePresetId(),
        contextSnapshot,
        presetSnapshot,
      });
      const assets = result.assets.map((asset) => ({ ...asset, taskId }));
      const durationMs = Math.round(performance.now() - startedAt);
      const { status: nextStatus, error: remoteError } = outcomeFromResult(result);
      setTasks((items) => items.map((item) => item.id === taskId ? {
        ...item,
        status: nextStatus,
        progress: nextStatus === "completed" ? 100 : Math.max(item.progress, nextStatus === "queued" ? 12 : 48),
        assets,
        durationMs,
        responseParts: result.responseParts,
        requestId: result.requestId,
        interactionId: result.interactionId,
        usage: result.usage,
        error: remoteError,
      } : item));
      setHistory((items) => [{
        ...task,
        status: nextStatus,
        progress: nextStatus === "completed" ? 100 : nextStatus === "queued" ? 12 : 48,
        assets,
        durationMs,
        favorite: false,
        responseParts: result.responseParts,
        requestId: result.requestId,
        interactionId: result.interactionId,
        usage: result.usage,
        error: remoteError,
        draftSnapshot: requestDraft,
        contextSnapshot,
        presetSnapshot,
      }, ...items]);
      updateActiveProject((item) => ({ ...item, updatedAt: new Date().toISOString() }));
      if (currentProject.settings.autoOpenFolder) void api.revealPath(currentProject.storagePath);
    } catch (error) {
      const message = cancelledTaskIds.has(taskId)
        ? "Cancelled"
        : error instanceof Error ? error.message : String(error);
      setTasks((items) => items.map((item) => item.id === taskId ? { ...item, status: "failed", error: message } : item));
      setHistory((items) => [{
        ...task,
        status: "failed",
        error: message,
        favorite: false,
        draftSnapshot: requestDraft,
        contextSnapshot,
        presetSnapshot,
      }, ...items]);
    } finally {
      window.clearInterval(progressTimer);
      cancelledTaskIds.delete(taskId);
      localPendingTaskIds.delete(taskId);
    }
  };

  const toggleQueue = async () => {
    if (queueControlBusy()) return;
    const previous = queuePaused();
    const target = !previous;
    setQueueControlBusy(true);
    setQueuePaused(target);
    try {
      const paused = target ? await api.pauseQueue() : await api.resumeQueue();
      setQueuePaused(paused);
      if (!paused) {
        setTasks((items) => items.map((item) => localPendingTaskIds.has(item.id) && item.status === "queued"
          ? { ...item, status: "running", progress: Math.max(item.progress, 8) }
          : item));
      }
    } catch (error) {
      setQueuePaused(previous);
      setBackendError(error instanceof Error ? error.message : String(error));
    } finally {
      setQueueControlBusy(false);
    }
  };

  const cancelTask = (taskId: string) => {
    cancelledTaskIds.add(taskId);
    localPendingTaskIds.delete(taskId);
    setTasks((items) => items.map((item) => item.id === taskId ? { ...item, status: "failed", error: "Cancelled", progress: item.progress } : item));
    const taskProjectId = tasks().find((item) => item.id === taskId)?.projectId ?? project()?.id;
    void api.cancelRun(taskId, taskProjectId).catch((error: unknown) => {
      setBackendError(error instanceof Error ? error.message : String(error));
    });
  };

  const deleteHistoryRecord = async (recordId: string) => {
    const record = history().find((item) => item.id === recordId);
    if (!record) return;
    try {
      const result = await api.deleteHistory(record.projectId, recordId);
      if (result.deletedRuns > 0) setHistory((items) => items.filter((item) => item.id !== recordId));
      if (result.failures.length > 0) setBackendError(result.failures.map((item) => item.message).join("; "));
    } catch (error) {
      setBackendError(error instanceof Error ? error.message : String(error));
    }
  };

  const clearProjectHistory = async (projectId: string) => {
    try {
      const result = await api.clearHistory(projectId);
      if (result.deletedRuns > 0 || api.isDemo) setHistory((items) => items.filter((item) => item.projectId !== projectId));
      if (result.failures.length > 0) setBackendError(result.failures.map((item) => item.message).join("; "));
    } catch (error) {
      setBackendError(error instanceof Error ? error.message : String(error));
    }
  };

  const rerun = async (record: HistoryRecord) => {
    if (record.capabilityRegistryVersion && record.capabilityRegistryVersion !== CAPABILITY_REGISTRY_VERSION) {
      setNotice(t("capabilityVersionChanged"));
      window.setTimeout(() => setNotice(""), 6000);
    }
    const rerunProvider = providers().find((item) => item.id === record.providerId);
    const capabilities = getModelCapabilities(rerunProvider, record.model);
    const stored = record.draftSnapshot ?? {};
    const storedSize = typeof stored.size === "string" ? stored.size : "auto";
    const customMatch = storedSize.match(/^(\d+)x(\d+)$/i);
    const references = await api.hydrateReferencePreviews(record.projectId, stored.references ?? []);
    const maskDataUrl = stored.maskDataUrl && !stored.maskDataUrl.startsWith("data:")
      ? await api.readProjectAssetDataUrl(record.projectId, stored.maskDataUrl)
      : stored.maskDataUrl;
    setDraftProviderModel(rerunProvider, record.model, {
      ...initialDraft,
      ...stored,
      providerId: record.providerId,
      model: record.model,
      mode: record.mode,
      prompt: record.prompt,
      count: record.count,
      size: customMatch && !capabilities.sizes.includes(storedSize) ? "custom" : storedSize,
      customWidth: customMatch ? Number(customMatch[1]) : stored.customWidth ?? initialDraft.customWidth,
      customHeight: customMatch ? Number(customMatch[2]) : stored.customHeight ?? initialDraft.customHeight,
      references,
      maskDataUrl,
    });
    setActivePresetId(record.presetSnapshot?.id);
    setComposedPromptOverride(record.composedPrompt);
    setContextSnapshotOverride(record.contextSnapshot ?? []);
    setPresetSnapshotOverride(record.presetSnapshot);
    setActiveTab("create");
  };

  const continueEditing = async (record: HistoryRecord) => {
    await rerun(record);
    if (!record.interactionId) return;
    const recordProvider = providers().find((item) => item.id === record.providerId);
    if (recordProvider?.kind === "gemini") {
      setDraft("useInteractionsApi", true);
      setDraft("previousInteractionId", record.interactionId);
    } else if (recordProvider?.kind === "openai" || recordProvider?.kind === "custom") {
      setDraft("useResponsesApi", true);
      setDraft("previousResponseId", record.interactionId);
    }
    setComposedPromptOverride(undefined);
    setContextSnapshotOverride(undefined);
    setPresetSnapshotOverride(undefined);
  };

  const applyPreset = (preset: GenerationPreset) => {
    const presetProvider = providers().find((item) => item.id === preset.providerId);
    setDraftProviderModel(presetProvider, preset.model, {
      mode: preset.mode,
      aspectRatio: preset.aspectRatio,
      size: preset.size,
      quality: preset.quality,
      outputFormat: preset.outputFormat,
      prompt: preset.promptTemplate ? `${preset.promptTemplate}${draft.prompt}` : draft.prompt,
    });
    setActivePresetId(preset.id);
    setComposedPromptOverride(undefined);
    setContextSnapshotOverride(undefined);
    setPresetSnapshotOverride(undefined);
    setActiveTab("create");
  };

  const tabItems: WorkspaceTab[] = ["create", "history", "descriptions", "presets", "project-settings"];

  return (
    <main class={`app-shell ${sidebarCollapsed() ? "sidebar-collapsed" : ""}`}>
      <aside class="sidebar">
        <div class="brand-row">
          <span class="brand-mark"><img src={appIcon} alt="" /></span>
          <Show when={!sidebarCollapsed()}><strong>{t("appName")}</strong></Show>
          <IconButton label="Menu" onClick={() => setSidebarCollapsed((value) => !value)}><Menu size={17} /></IconButton>
        </div>

        <Show when={!sidebarCollapsed()}>
          <div class="sidebar-section-label"><span>{t("projects")}</span><IconButton label={t("newProject")} onClick={() => setProjectModalOpen(true)}><Plus size={15} /></IconButton></div>
        </Show>
        <nav class="project-list">
          <For each={projects()}>
            {(item) => (
              <button type="button" class={`project-nav-item ${item.id === activeProjectId() ? "is-active" : ""}`} title={item.name} onClick={() => selectProject(item.id)}>
                <span class="project-color" style={{ "background-color": item.color }} />
                <Show when={!sidebarCollapsed()}><span><strong>{item.name}</strong><small>{projectHistoryCount(item.id)} {t("generated")}</small></span></Show>
              </button>
            )}
          </For>
        </nav>

        <div class="sidebar-actions">
          <button type="button" title={t("newProject")} aria-label={t("newProject")} onClick={() => setProjectModalOpen(true)}><FolderPlus size={17} /><Show when={!sidebarCollapsed()}><span>{t("newProject")}</span></Show></button>
          <button type="button" title={t("openProject")} aria-label={t("openProject")} onClick={openProject}><FolderOpen size={17} /><Show when={!sidebarCollapsed()}><span>{t("openProject")}</span></Show></button>
        </div>

        <div class="sidebar-spacer" />
        <Show when={!sidebarCollapsed()}>
          <div class="provider-summary">
            <div class="sidebar-section-label"><span>{t("providers")}</span><IconButton label={t("manageProviders")} onClick={() => setProviderModalOpen(true)}><Settings size={15} /></IconButton></div>
            <For each={providers().filter((item) => item.enabled).slice(0, 4)}>
              {(provider) => <button type="button" class="provider-summary-row" onClick={() => setProviderModalOpen(true)}><span style={{ color: getProviderAccent(provider.kind) }}><Archive size={15} /></span><span>{provider.name}</span><StatusDot status="online" /></button>}
            </For>
          </div>
        </Show>
      </aside>

      <section class="workspace-shell">
        <header class="topbar">
          <div class="project-switcher-wrap">
            <button class="project-switcher" type="button" onClick={() => setProjectMenuOpen((value) => !value)}>
              <span class="project-color" style={{ "background-color": project()?.color ?? "#6c727a" }} />
              <span><strong>{project()?.name}</strong><small>{project()?.storagePath}</small></span>
              <ChevronDown size={16} />
            </button>
            <Show when={projectMenuOpen()}>
              <div class="project-menu">
                <For each={projects()}>{(item) => <button type="button" class={item.id === activeProjectId() ? "is-active" : ""} onClick={() => selectProject(item.id)}><span class="project-color" style={{ "background-color": item.color }} /><span><strong>{item.name}</strong><small>{item.storagePath}</small></span></button>}</For>
                <div class="menu-separator" />
                <button type="button" onClick={() => setProjectModalOpen(true)}><FolderPlus size={16} /><span>{t("newProject")}</span></button>
                <button type="button" onClick={openProject}><FolderOpen size={16} /><span>{t("openProject")}</span></button>
              </div>
            </Show>
          </div>
          <div class="topbar-spacer" />
          <Show when={api.isDemo}><span class="demo-badge" title={t("localDemoHint")}><StatusDot status="busy" />{t("localDemo")}</span></Show>
          <Show when={backendError()}><span class="backend-error-badge" title={backendError()}><StatusDot status="offline" />{t("backendUnavailable")}</span></Show>
          <Show when={notice()}><span class="notice-badge">{notice()}</span></Show>
          <div class="language-switch" title={t("language")}><Languages size={15} /><button type="button" class={locale() === "zh-CN" ? "is-active" : ""} onClick={() => setLocale("zh-CN")}>中</button><button type="button" class={locale() === "en-US" ? "is-active" : ""} onClick={() => setLocale("en-US")}>EN</button></div>
          <IconButton label={t("manageProviders")} onClick={() => setProviderModalOpen(true)}><SlidersHorizontal size={17} /></IconButton>
          <IconButton label={t("settings")} active={activeTab() === "project-settings"} onClick={() => setActiveTab("project-settings")}><Settings size={17} /></IconButton>
        </header>

        <nav class="tabbar">
          <For each={tabItems}>
            {(tab) => {
              const TabIcon = tabIcons[tab];
              return <button type="button" class={activeTab() === tab ? "is-active" : ""} onClick={() => setActiveTab(tab)}><TabIcon size={15} />{t(tabKeys[tab])}</button>;
            }}
          </For>
        </nav>

        <section class="workspace-content">
          <Show when={project()}>
            {(currentProject) => (
              <Switch>
                <Match when={activeTab() === "create"}>
                  <CreatorPage
                    project={currentProject()}
                    providers={providers()}
                    draft={draft}
                    setDraft={setDraft}
                    tasks={tasks()}
                    history={history()}
                    queuePaused={queuePaused()}
                    queueControlBusy={queueControlBusy()}
                    t={t}
                    promptOverride={composedPromptOverride()}
                    onPromptOverrideClear={() => {
                      setComposedPromptOverride(undefined);
                      setContextSnapshotOverride(undefined);
                      setPresetSnapshotOverride(undefined);
                    }}
                    onGenerate={generate}
                    onCancelTask={cancelTask}
                    onToggleQueue={() => void toggleQueue()}
                    onManageProviders={() => setProviderModalOpen(true)}
                    onReveal={(path) => void api.revealPath(projectAssetPath(currentProject().storagePath, path))}
                  />
                </Match>
                <Match when={activeTab() === "history"}>
                  <HistoryPage
                    project={currentProject()}
                    providers={providers()}
                    history={history()}
                    t={t}
                    onRerun={rerun}
                    onContinue={(record) => void continueEditing(record)}
                    onToggleFavorite={(recordId) => setHistory((items) => items.map((item) => item.id === recordId ? { ...item, favorite: !item.favorite } : item))}
                    onDelete={(recordId) => void deleteHistoryRecord(recordId)}
                  />
                </Match>
                <Match when={activeTab() === "descriptions"}>
                  <DescriptionsPage project={currentProject()} providers={providers()} t={t} onChange={changeDescriptions} />
                </Match>
                <Match when={activeTab() === "presets"}>
                  <PresetsPage project={currentProject()} providers={providers()} t={t} onChange={changePresets} onApply={applyPreset} />
                </Match>
                <Match when={activeTab() === "project-settings"}>
                  <ProjectSettingsPage project={currentProject()} providers={providers()} t={t} onChange={updateProject} onClearHistory={() => void clearProjectHistory(currentProject().id)} />
                </Match>
              </Switch>
            )}
          </Show>
        </section>
      </section>

      <ProviderModal open={providerModalOpen()} providers={providers()} t={t} onClose={() => setProviderModalOpen(false)} onUpsert={upsertProvider} onDelete={deleteProvider} />
      <ProjectModal open={projectModalOpen()} providers={providers()} t={t} onClose={() => setProjectModalOpen(false)} onCreate={createProject} />
    </main>
  );
}
