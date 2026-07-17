import { For, Match, Show, Switch, createEffect, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import { createStore } from "solid-js/store";
import {
  Bug,
  FolderOpen,
  FolderPlus,
  History,
  Images,
  Languages,
  Library,
  Menu,
  Settings,
  SlidersHorizontal,
  Sparkles,
  X,
} from "lucide-solid";
import appIcon from "./assets/app-icon.png";
import CreatorPage from "./components/CreatorPage";
import DiagnosticsModal, { type AppErrorEntry } from "./components/DiagnosticsModal";
import {
  DescriptionsPage,
  HistoryPage,
  PresetsPage,
  ProjectSettingsPage,
  ResultsPage,
} from "./components/ManagementPages";
import ProjectModal from "./components/ProjectModal";
import ProviderModal from "./components/ProviderModal";
import { IconButton, StatusDot } from "./components/common";
import { demoWorkspace, initialDraft, starterProviders } from "./data/demo";
import { api, normalizeError } from "./lib/api";
import { translate, type TranslationKey } from "./lib/i18n";
import { getModelCapabilities } from "./lib/models";
import { composeNegativePrompt, normalizeDraftForModel } from "./lib/prompt";
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
  results: Images,
};

const tabKeys: Record<WorkspaceTab, TranslationKey> = {
  create: "create",
  history: "history",
  descriptions: "descriptions",
  presets: "presets",
  "project-settings": "projectSettings",
  results: "allResults",
};

const CAPABILITY_REGISTRY_VERSION = "2026-07-12";

function stripExtendedLengthPrefix(p: string): string {
  // Rust's canonicalize() on Windows prefixes paths with \\?\ — strip it so
  // Win32 APIs and path joins work correctly with plain drive-letter paths.
  return p.replace(/^\\\\\?\\/, "");
}

function projectAssetPath(projectRoot: string, path: string): string {
  const root = stripExtendedLengthPrefix(projectRoot);
  const p = stripExtendedLengthPrefix(path);
  if (/^(?:[A-Za-z]:[\\/]|\/)/.test(p) || /^https?:\/\//i.test(p)) return p;
  return `${root.replace(/[\\/]+$/, "")}/${p.replace(/^[\\/]+/, "")}`;
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
  const [sidebarCollapsed, setSidebarCollapsed] = createSignal(false);
  const [hydrated, setHydrated] = createSignal(false);
  const [backendError, setBackendError] = createSignal("");
  const [diagnosticsOpen, setDiagnosticsOpen] = createSignal(false);
  const [errorEntries, setErrorEntries] = createSignal<AppErrorEntry[]>([]);
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
  const recordError = (error: unknown, context: string) => {
    const normalized = normalizeError(error);
    const entry: AppErrorEntry = {
      id: crypto.randomUUID(),
      occurredAt: new Date().toISOString(),
      context,
      code: normalized.code,
      message: normalized.message,
      details: normalized.details,
    };
    setBackendError(normalized.code === "error" ? normalized.message : `[${normalized.code}] ${normalized.message}`);
    setErrorEntries((items) => [entry, ...items].slice(0, 50));
  };
  const recordMessage = (message: string, context: string, code = "error") => recordError({ code, message }, context);
  const project = createMemo(() => projects().find((item) => item.id === activeProjectId()) ?? projects()[0]);
  const projectHistoryCount = (projectId: string) => history().filter((item) => item.projectId === projectId && item.status === "completed").reduce((total, item) => total + item.assets.length, 0);

  const setDraftProviderModel = (provider: ProviderProfile | undefined, preferredModel: string, overrides: Partial<typeof draft> = {}) => {
    const model = provider?.models.includes(preferredModel) ? preferredModel : provider?.models[0] ?? "";
    const capabilities = getModelCapabilities(provider, model);
    const candidate = { ...draft, ...overrides, providerId: provider?.id ?? "", model, references: [...(overrides.references ?? draft.references)] };
    const normalized = normalizeDraftForModel(candidate, capabilities);
    // Resolve stream default: project setting > provider setting > model capability
    const currentProject = project();
    const resolvedStream = currentProject?.settings.defaultStream
      ?? provider?.defaultStream
      ?? capabilities.supportsStreaming;
    setDraft({ ...normalized, stream: resolvedStream as boolean });
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
      recordError(error, "remote_tasks.poll");
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
      recordError(error, "queue.status");
    }
    try {
      const saved = await api.loadWorkspace();
      if (saved) {
        setLocale(saved.locale);
        setProviders(saved.providers);
        setHistory(saved.history);
        let availableProjects = saved.projects;
        if (!api.isDemo && saved.projects.length > 0) {
          const results = await Promise.allSettled(saved.projects.map(async (savedItem) => ({
            project: savedItem,
            details: await api.loadProjectDetails(savedItem.id),
          })));
          const loaded = results.flatMap((result) => result.status === "fulfilled" ? [result.value] : []);
          availableProjects = loaded.map(({ project: savedItem, details }) => ({
            ...savedItem,
            descriptions: details.descriptions,
            presets: details.presets,
          }));
          loaded.forEach(({ project: savedItem, details }) => {
            loadedProjectIds.add(savedItem.id);
            mergePortableHistory(details.history);
          });
          results.forEach((result, index) => {
            if (result.status === "rejected") {
              recordError(result.reason, `workspace.reopen:${saved.projects[index].name}`);
            }
          });
        }
        setProjects(availableProjects);
        const restoredActiveId = availableProjects.some((item) => item.id === saved.activeProjectId)
          ? saved.activeProjectId
          : availableProjects[0]?.id ?? "";
        setActiveProjectId(restoredActiveId);
        const savedProject = availableProjects.find((item) => item.id === restoredActiveId);
        if (savedProject) {
          const savedProvider = saved.providers.find((item) => item.id === savedProject.settings.defaultProviderId)
            ?? saved.providers.find((item) => item.enabled)
            ?? saved.providers[0];
          setDraftProviderModel(savedProvider, savedProject.settings.defaultModel);
        }
        if (restoredActiveId) void pollProjectRemoteTasks(restoredActiveId);
        if (!api.isDemo && availableProjects.length === 0) setProjectModalOpen(true);
      } else if (!api.isDemo) {
        setProjectModalOpen(true);
      }
      setHydrated(true);
    } catch (error) {
      recordError(error, "workspace.load");
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
      recordError(error, "workspace.save");
    });
  });

  const closeMenus = (event: KeyboardEvent) => {
    if (event.key !== "Escape") return;
    setProviderModalOpen(false);
    setProjectModalOpen(false);
    setDiagnosticsOpen(false);
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
      recordError(error, "generation.events");
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
    if (!api.isDemo && !loadedProjectIds.has(projectId)) {
      void api.loadProjectDetails(projectId).then((details) => {
        loadedProjectIds.add(projectId);
        setProjects((items) => items.map((item) => item.id === projectId ? { ...item, descriptions: details.descriptions, presets: details.presets } : item));
        mergePortableHistory(details.history);
        void pollProjectRemoteTasks(projectId);
      }).catch((error: unknown) => recordError(error, "project.load_details"));
      return;
    }
    void pollProjectRemoteTasks(projectId);
  };

  const upsertProvider = (provider: ProviderProfile) => {
    setProviders((items) => items.some((item) => item.id === provider.id)
      ? items.map((item) => item.id === provider.id ? provider : item)
      : [...items, provider]);
    setProjects((items) => items.map((item) => item.settings.defaultProviderId === provider.id && !provider.models.includes(item.settings.defaultModel)
      ? { ...item, settings: { ...item.settings, defaultModel: provider.models[0] ?? "" } }
      : item));
    if (draft.providerId === provider.id && !provider.models.includes(draft.model)) {
      setDraftProviderModel(provider, provider.models[0] ?? "");
    }
    if (!draft.providerId) {
      setDraftProviderModel(provider, provider.models[0] ?? "");
    }
  };

  const deleteProvider = async (providerId: string): Promise<boolean> => {
    try {
      if (!(await api.deleteProvider(providerId))) return false;
    } catch (error) {
      recordError(error, "provider.delete");
      return false;
    }
    const remaining = providers().filter((item) => item.id !== providerId);
    const fallback = remaining.find((item) => item.enabled) ?? remaining[0];
    const dependentProjects = projects().filter((item) => item.settings.defaultProviderId === providerId);
    setProviders(remaining);
    if (dependentProjects.length > 0) {
      if (fallback) {
        void Promise.all(dependentProjects.map((item) => api.remapProjectProvider(item.id, providerId, fallback.id)))
          .catch((error: unknown) => recordError(error, "project.remap_provider"));
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

  const createProject = async (nextProject: Project) => {
    try {
      await api.createProject(nextProject);
      loadedProjectIds.add(nextProject.id);
      setProjects((items) => [nextProject, ...items]);
      selectProject(nextProject.id);
      setProjectModalOpen(false);
    } catch (error) {
      recordError(error, "project.create");
      throw error;
    }
  };

  const openProject = async () => {
    try {
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
          flatOutput: false,
          defaultStream: null,
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
    } catch (error) {
      recordError(error, "project.open");
    }
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
    ]).catch((error: unknown) => recordError(error, "descriptions.save"));
    updateActiveProject((item) => ({ ...item, descriptions: nextDescriptions, updatedAt: new Date().toISOString() }));
  };

  const changePresets = (nextPresets: Project["presets"]) => {
    const currentProject = project();
    if (!currentProject) return;
    const deleted = currentProject.presets.filter((item) => !nextPresets.some((next) => next.id === item.id));
    void Promise.all([
      ...deleted.map((item) => api.deleteGenerationPreset(currentProject.id, item.id)),
      ...nextPresets.map((item) => api.upsertGenerationPreset(currentProject.id, item)),
    ]).catch((error: unknown) => recordError(error, "presets.save"));
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
      // Project-level flatOutput flows into every request; per-image draft.flatOutput can override.
      flatOutput: draft.flatOutput || currentProject.settings.flatOutput,
    };
    const contextSnapshot = replayContexts ?? (currentProject.settings.useCommonDescriptions
      ? currentProject.descriptions.filter((item) => item.enabled && (item.prefixContent.trim() || item.suffixContent.trim() || item.negativeContent.trim()))
      : []);
    const effectiveNegativePrompt = composeNegativePrompt(
      draft.negativePrompt,
      contextSnapshot,
      true,
    );
    const commandDraft = { ...requestDraft, negativePrompt: effectiveNegativePrompt };
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
        draft: commandDraft,
        manualNegativePrompt: draft.negativePrompt,
        composedPrompt: effectivePrompt,
        contextIds: contextSnapshot.map((item) => item.id),
        presetId: activePresetId(),
        contextSnapshot,
        presetSnapshot,
      });
      const assets = result.assets.map((asset) => ({ ...asset, taskId }));
      const durationMs = Math.round(performance.now() - startedAt);
      const { status: nextStatus, error: remoteError } = outcomeFromResult(result);
      const failureReason = result.failureReason ?? remoteError;
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
        error: failureReason,
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
        error: failureReason,
        draftSnapshot: requestDraft,
        contextSnapshot,
        presetSnapshot,
      }, ...items]);
      updateActiveProject((item) => ({ ...item, updatedAt: new Date().toISOString() }));
      if (currentProject.settings.autoOpenFolder) {
        void api.revealPath(currentProject.storagePath).catch((error: unknown) => recordError(error, "project.auto_reveal"));
      }
    } catch (error) {
      const normalized = normalizeError(error);
      const message = cancelledTaskIds.has(taskId)
        ? "Cancelled"
        : normalized.code === "error" ? normalized.message : `[${normalized.code}] ${normalized.message}`;
      if (!cancelledTaskIds.has(taskId)) recordError(error, "generation.execute");
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
      recordError(error, "queue.toggle");
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
      recordError(error, "task.cancel");
    });
  };

  const deleteHistoryRecord = async (recordId: string) => {
    const record = history().find((item) => item.id === recordId);
    if (!record) return;
    try {
      const result = await api.deleteHistory(record.projectId, recordId);
      if (result.deletedRuns > 0) setHistory((items) => items.filter((item) => item.id !== recordId));
      if (result.failures.length > 0) recordMessage(result.failures.map((item) => item.message).join("; "), "history.delete", "lifecycle_failure");
    } catch (error) {
      recordError(error, "history.delete");
    }
  };

  const deleteFailedHistory = async (projectId: string) => {
    const failed = history().filter((item) => item.projectId === projectId && item.status === "failed");
    for (const record of failed) {
      try {
        const result = await api.deleteHistory(record.projectId, record.id);
        if (result.deletedRuns > 0) setHistory((items) => items.filter((item) => item.id !== record.id));
      } catch (error) {
        recordError(error, "history.delete_failed");
        break;
      }
    }
  };

  const clearProjectHistory = async (projectId: string) => {
    try {
      const result = await api.clearHistory(projectId);
      if (result.deletedRuns > 0 || api.isDemo) setHistory((items) => items.filter((item) => item.projectId !== projectId));
      if (result.failures.length > 0) recordMessage(result.failures.map((item) => item.message).join("; "), "history.clear", "lifecycle_failure");
    } catch (error) {
      recordError(error, "history.clear");
    }
  };

  const restoreHistoryDraft = async (record: HistoryRecord) => {
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

  const rerun = async (record: HistoryRecord) => {
    try {
      await restoreHistoryDraft(record);
    } catch (error) {
      recordError(error, "history.restore");
    }
  };

  const continueEditing = async (record: HistoryRecord) => {
    try {
      await restoreHistoryDraft(record);
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
    } catch (error) {
      recordError(error, "history.continue");
    }
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

  const tabItems: WorkspaceTab[] = ["create", "results", "history", "descriptions", "presets", "project-settings"];

  return (
    <main class={`app-shell ${sidebarCollapsed() ? "sidebar-collapsed" : ""}`}>
      <aside class="sidebar">
        <div class="brand-row">
          <span class="brand-mark"><img src={appIcon} alt="" /></span>
          <Show when={!sidebarCollapsed()}><strong>{t("appName")}</strong></Show>
          <IconButton label="Menu" onClick={() => setSidebarCollapsed((value) => !value)}><Menu size={17} /></IconButton>
        </div>

        <Show when={!sidebarCollapsed()}>
          <div class="sidebar-section-label"><span>{t("projects")}</span></div>
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
      </aside>

      <section class="workspace-shell">
        <header class="topbar">
          <div class="project-identity">
            <span class="project-color" style={{ "background-color": project()?.color ?? "#6c727a" }} />
            <span><strong>{project()?.name}</strong><small>{project()?.storagePath}</small></span>
          </div>
          <div class="topbar-spacer" />
          <Show when={api.isDemo}><span class="demo-badge" title={t("localDemoHint")}><StatusDot status="busy" />{t("localDemo")}</span></Show>
          <Show when={backendError()}>
            <span class="backend-error-badge" onClick={() => setDiagnosticsOpen(true)}>
              <StatusDot status="offline" />
              <span class="backend-error-text">{backendError()}</span>
              <button
                type="button"
                class="backend-error-close"
                title={t("dismissError")}
                onClick={(e) => { e.stopPropagation(); setBackendError(""); }}
              ><X size={13} /></button>
            </span>
          </Show>
          <Show when={notice()}><span class="notice-badge">{notice()}</span></Show>
          <div class="language-switch" title={t("language")}><Languages size={15} /><button type="button" class={locale() === "zh-CN" ? "is-active" : ""} onClick={() => setLocale("zh-CN")}>中</button><button type="button" class={locale() === "en-US" ? "is-active" : ""} onClick={() => setLocale("en-US")}>EN</button></div>
          <IconButton label={t("diagnostics")} onClick={() => setDiagnosticsOpen(true)}><Bug size={17} /></IconButton>
          <IconButton label={t("manageProviders")} onClick={() => setProviderModalOpen(true)}><SlidersHorizontal size={17} /></IconButton>
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
                    onError={recordError}
                    onReveal={(path) => void api.revealPath(projectAssetPath(currentProject().storagePath, path)).catch((error: unknown) => {
                      recordError(error, "asset.reveal");
                    })}
                    onDownload={(asset) => {
                      if (!asset.filePath) {
                        recordMessage(t("noLocalFile"), "asset.export", "no_local_file");
                        return;
                      }
                      const sourcePath = projectAssetPath(currentProject().storagePath, asset.filePath);
                      const suggestedName = asset.filePath.split(/[\\/]/).at(-1) || `${asset.id}.${asset.format}`;
                      void api.exportAsset(sourcePath, suggestedName, asset.url).catch((error: unknown) => {
                        recordError(error, "asset.export");
                      });
                    }}
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
                    onDeleteFailed={() => void deleteFailedHistory(currentProject().id)}
                  />
                </Match>
                <Match when={activeTab() === "results"}>
                  <ResultsPage
                    project={currentProject()}
                    providers={providers()}
                    history={history()}
                    t={t}
                    onError={recordError}
                    onReveal={(path) => void api.revealPath(projectAssetPath(currentProject().storagePath, path)).catch((error: unknown) => recordError(error, "asset.reveal"))}
                    onOpenFolder={() => void api.revealPath(currentProject().storagePath).catch((error: unknown) => recordError(error, "project.reveal"))}
                    onDownload={(asset) => {
                      if (!asset.filePath) { recordMessage(t("noLocalFile"), "asset.export", "no_local_file"); return; }
                      const sourcePath = projectAssetPath(currentProject().storagePath, asset.filePath);
                      const suggestedName = asset.filePath.split(/[\\/]/).at(-1) || `${asset.id}.${asset.format}`;
                      void api.exportAsset(sourcePath, suggestedName, asset.url).catch((error: unknown) => recordError(error, "asset.export"));
                    }}
                  />
                </Match>
                <Match when={activeTab() === "descriptions"}>
                  <DescriptionsPage project={currentProject()} providers={providers()} t={t} onChange={changeDescriptions} />
                </Match>
                <Match when={activeTab() === "presets"}>
                  <PresetsPage project={currentProject()} providers={providers()} t={t} onChange={changePresets} onApply={applyPreset} />
                </Match>
                <Match when={activeTab() === "project-settings"}>
                  <ProjectSettingsPage project={currentProject()} providers={providers()} t={t} onError={recordError} onChange={updateProject} onClearHistory={() => void clearProjectHistory(currentProject().id)} />
                </Match>
              </Switch>
            )}
          </Show>
        </section>
      </section>

      <DiagnosticsModal open={diagnosticsOpen()} errors={errorEntries()} t={t} onClose={() => setDiagnosticsOpen(false)} onClearErrors={() => setErrorEntries([])} />
      <ProviderModal open={providerModalOpen()} providers={providers()} t={t} onClose={() => setProviderModalOpen(false)} onUpsert={upsertProvider} onDelete={deleteProvider} onError={recordError} />
      <ProjectModal open={projectModalOpen()} providers={providers()} t={t} onClose={() => setProjectModalOpen(false)} onCreate={createProject} onError={recordError} />
    </main>
  );
}
