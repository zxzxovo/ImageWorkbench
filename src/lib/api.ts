import { invoke } from "@tauri-apps/api/core";
import type {
  CommonDescription,
  GenerateRequest,
  GenerationDraft,
  GenerationPreset,
  GenerationResult,
  GeneratedAsset,
  HistoryRecord,
  Project,
  ProjectSummary,
  ProviderProfile,
  ReferenceAsset,
  ResponsePart,
  UsageSummary,
  WorkspaceSnapshot,
} from "../types";
import { providerDefaultModels } from "./models";

const STORAGE_KEY = "imageworkbench.workspace.v1";
const sessionSecrets = new Map<string, string>();
const headerSessionSecrets = new Map<string, string>();
const cancelledRuns = new Set<string>();
let demoQueuePaused = false;
const demoQueueWaiters = new Set<() => void>();

function notifyDemoQueueWaiters(): void {
  for (const waiter of demoQueueWaiters) waiter();
  demoQueueWaiters.clear();
}

async function waitForDemoQueue(runId: string): Promise<void> {
  while (demoQueuePaused && !cancelledRuns.has(runId)) {
    await new Promise<void>((resolve) => demoQueueWaiters.add(resolve));
  }
  if (cancelledRuns.has(runId)) throw new Error("Cancelled");
}

function isSensitiveHeaderName(name: string): boolean {
  return /^(authorization|proxy-authorization|x-api-key|api-key|x-goog-api-key)$/i.test(name.trim());
}

function headerSecretKey(providerId: string, headerId: string): string {
  return `${providerId}:${headerId}`;
}

interface RawUsage {
  inputTokens?: number;
  input_tokens?: number;
  outputTokens?: number;
  output_tokens?: number;
  thoughtTokens?: number;
  thought_tokens?: number;
  cachedTokens?: number;
  cached_tokens?: number;
  totalTokens?: number;
  total_tokens?: number;
  imageTokens?: number;
  image_tokens?: number;
  costUsd?: number;
  cost_usd?: number;
  generatedImages?: number;
  generated_images?: number;
}

type RawResponsePart = Record<string, unknown> & { type?: string };

interface RawGenerationCommandResult {
  runId?: string;
  run_id?: string;
  assets?: Array<Record<string, unknown>>;
  responseParts?: RawResponsePart[];
  response_parts?: RawResponsePart[];
  usage?: RawUsage;
  requestId?: string;
  request_id?: string;
  interactionId?: string;
  interaction_id?: string;
  failureReason?: string;
  failure_reason?: string;
}

interface ImportedInputDto {
  relativePath: string;
  mimeType: string;
  sha256: string;
  sizeBytes: number;
  width?: number;
  height?: number;
}

export interface GenerationEventEnvelope {
  runId: string;
  event: Record<string, unknown>;
}

interface RawPromptContext {
  id: string;
  name: string;
  content: string;
  placement: "prepend" | "append";
  prefixContent?: string;
  suffixContent?: string;
  negativeContent?: string;
  sortOrder: number;
  enabled: boolean;
  createdAt: string;
  updatedAt: string;
}

interface LegacyCommonDescription extends Partial<CommonDescription> {
  id: string;
  title: string;
  content?: string;
  placement?: "prefix" | "suffix";
  enabled: boolean;
  createdAt: string;
}

export function normalizeCommonDescription(description: LegacyCommonDescription): CommonDescription {
  const hasStructuredParts = Boolean(
    description.prefixContent?.trim()
    || description.suffixContent?.trim()
    || description.negativeContent?.trim(),
  );
  const legacyContent = hasStructuredParts ? "" : description.content?.trim() ?? "";
  return {
    id: description.id,
    title: description.title,
    prefixContent: hasStructuredParts ? description.prefixContent ?? "" : description.placement === "prefix" ? legacyContent : "",
    suffixContent: hasStructuredParts ? description.suffixContent ?? "" : description.placement !== "prefix" ? legacyContent : "",
    negativeContent: hasStructuredParts ? description.negativeContent ?? "" : "",
    enabled: description.enabled,
    createdAt: description.createdAt,
  };
}

interface RawGenerationPreset {
  id: string;
  name: string;
  providerProfileId?: string;
  modelId?: string;
  operation?: string;
  parameters?: Record<string, unknown>;
  output?: Record<string, unknown> & { size?: Record<string, unknown> };
  createdAt: string;
  updatedAt: string;
}

interface RawRunRecord {
  id: string;
  request: {
    projectId: string;
    providerProfileId: string;
    modelId: string;
    operation: string;
    prompt: string;
    finalPrompt?: string;
    output?: { count?: number };
    inputs?: RawInputAsset[];
    mask?: RawInputAsset;
    metadata?: { frontendDraft?: Record<string, unknown>; continuationId?: string; composedPromptBase?: string };
  };
  status: string;
  rawPrompt: string;
  finalPrompt: string;
  providerRequestId?: string;
  startedAt?: string;
  finishedAt?: string;
  createdAt: string;
  contextSnapshot?: RawPromptContext[];
  presetSnapshot?: RawGenerationPreset;
  capabilityRegistryVersion?: string;
  redactedRequest?: unknown;
  redactedResponse?: unknown;
}

interface RawInputAsset {
  id: string;
  kind: "image" | "mask" | "video";
  source: {
    type: "local_path" | "url" | "base64" | "provider_file" | "generated_output";
    path?: string;
    url?: string;
    data?: string;
    fileId?: string;
    file_id?: string;
  };
  mimeType?: string;
  role?: string;
  label?: string;
  metadata?: Record<string, unknown>;
}

interface RawHistoryOutput {
  output: {
    id: string;
    kind: "image" | "text" | "thought" | "citation" | "search_suggestion";
    text?: string;
    localPath?: string;
    remoteUrl?: string;
    providerFileId?: string;
    mimeType?: string;
    sizeBytes?: number;
    metadata?: Record<string, unknown>;
    createdAt: string;
  };
  previewDataUrl?: string;
  width?: number;
  height?: number;
}

interface RawHistoryUsage {
  inputTokens?: number;
  outputTokens?: number;
  imageCount?: number;
  costMicros?: number;
  details?: Record<string, unknown>;
}

interface RawHistoryDetails {
  run: RawRunRecord;
  outputs: RawHistoryOutput[];
  usage: RawHistoryUsage[];
  errors: Array<{ error: { message: string; requestId?: string } }>;
}

interface RawProjectDetails {
  summary: ProjectSummary;
  contexts: RawPromptContext[];
  presets: RawGenerationPreset[];
  recentRecords: RawHistoryDetails[];
}

interface HistoryMutationResult {
  requestedRuns: number;
  deletedRuns: number;
  localAssetsDeleted: number;
  remoteFilesDeleted: number;
  remoteFilesRetained: number;
  failures: Array<{ code: string; message: string }>;
}

export interface AssetMutationResult {
  requestedAssets: number;
  deletedAssetIds: string[];
  localAssetsDeleted: number;
  remoteFilesDeleted: number;
  remoteFilesRetained: number;
  failures: Array<{ runId: string; outputId: string; code: string; message: string }>;
}

export interface BatchAssetExport {
  sourcePath: string;
  suggestedName: string;
  previewUrl: string;
}

export interface BatchExportResult {
  exported: number;
  exportedPaths: string[];
  failures: Array<{ sourcePath: string; message: string }>;
}

export interface NormalizedError {
  code: string;
  message: string;
  details?: unknown;
}

export interface DiagnosticProject {
  id: string;
  name: string;
  storagePath: string;
  databaseExists: boolean;
  isOpen: boolean;
}

export interface DiagnosticReport {
  appVersion: string;
  os: string;
  architecture: string;
  appDataDirectory: string;
  logDirectory: string;
  logFiles: string[];
  logTail: string;
  credentialStoreStatus: string;
  credentialStoreMessage: string;
  projects: DiagnosticProject[];
}

function mapRawPreset(preset: RawGenerationPreset): GenerationPreset {
  const parameters = preset.parameters ?? {};
  const output = preset.output ?? {};
  const sizeSpec = output.size ?? {};
  const size = sizeSpec.mode === "preset" && typeof sizeSpec.value === "string"
    ? sizeSpec.value
    : sizeSpec.mode === "custom" && typeof sizeSpec.width === "number" && typeof sizeSpec.height === "number"
      ? `${sizeSpec.width}x${sizeSpec.height}`
      : "auto";
  const mode = ({
    edit: "edit",
    variation: "variation",
    video_reference_to_image: "video",
  } as Record<string, GenerationPreset["mode"]>)[preset.operation ?? ""] ?? "generate";
  return {
    id: preset.id,
    name: preset.name,
    description: typeof parameters.description === "string" ? parameters.description : "",
    providerId: preset.providerProfileId ?? "",
    model: preset.modelId ?? "",
    mode,
    aspectRatio: typeof output.aspectRatio === "string" ? output.aspectRatio : "auto",
    size,
    quality: typeof output.quality === "string" ? output.quality : "auto",
    outputFormat: typeof parameters.outputFormat === "string" ? parameters.outputFormat : "png",
    promptTemplate: typeof parameters.promptTemplate === "string" ? parameters.promptTemplate : "",
    createdAt: preset.createdAt,
  };
}

function mapRawReference(input: RawInputAsset): ReferenceAsset | undefined {
  if (input.kind === "mask") return undefined;
  const source = input.source;
  const fileId = source.fileId ?? source.file_id;
  const sourceType = source.type === "provider_file" ? "file-id"
    : source.type === "url" ? "url"
      : source.type === "base64" ? "base64" : "local";
  const url = source.type === "provider_file" ? `provider-file:${fileId ?? ""}`
    : source.path ?? source.url ?? source.data ?? "";
  if (!url) return undefined;
  const role = (["object", "character", "style", "source", "video"].includes(input.role ?? "")
    ? input.role
    : input.kind === "video" ? "video" : "source") as ReferenceAsset["role"];
  return {
    id: input.id,
    name: input.label ?? url.split(/[\\/]/).at(-1) ?? "reference",
    url,
    mimeType: input.mimeType ?? (input.kind === "video" ? "video/mp4" : "image/png"),
    sourceType,
    fileId,
    role,
    width: typeof input.metadata?.width === "number" ? input.metadata.width : undefined,
    height: typeof input.metadata?.height === "number" ? input.metadata.height : undefined,
  };
}

function mapHistoryDetails(record: RawHistoryDetails): HistoryRecord {
  const run = record.run;
  const status = ["succeeded", "partially_succeeded"].includes(run.status)
    ? "completed" as const
    : ["queued", "paused"].includes(run.status) ? "queued" as const
      : run.status === "running" ? "running" as const : "failed" as const;
  const mode = ({
    edit: "edit",
    variation: "variation",
    video_reference_to_image: "video",
  } as Record<string, HistoryRecord["mode"]>)[run.request.operation] ?? "generate";
  const durationMs = run.startedAt && run.finishedAt
    ? Math.max(0, new Date(run.finishedAt).getTime() - new Date(run.startedAt).getTime())
    : undefined;
  const assets = record.outputs.filter((item) => item.output.kind === "image").map((item) => ({
    id: item.output.id,
    taskId: run.id,
    url: item.previewDataUrl ?? item.output.remoteUrl ?? "",
    filePath: item.output.localPath ?? item.output.remoteUrl ?? "",
    width: item.width ?? 0,
    height: item.height ?? 0,
    format: item.output.mimeType?.split("/").at(-1) ?? "png",
    prompt: run.finalPrompt || run.rawPrompt,
    createdAt: item.output.createdAt,
  }));
  const responseParts: ResponsePart[] = record.outputs.flatMap((item): ResponsePart[] => {
    const output = item.output;
    if (output.kind === "image") return [{
      id: output.id,
      type: "image",
      assetId: output.id,
      url: item.previewDataUrl ?? output.remoteUrl ?? "",
      mimeType: output.mimeType ?? "image/png",
      width: item.width ?? 0,
      height: item.height ?? 0,
      filePath: output.localPath ?? "",
    }];
    if (output.kind === "text") return [{ id: output.id, type: "text", text: output.text ?? "" }];
    if (output.kind === "thought") return [{ id: output.id, type: "thought", summary: output.text ?? "" }];
    if (output.kind === "citation") return [{
      id: output.id,
      type: "citation",
      title: typeof output.metadata?.title === "string" ? output.metadata.title : undefined,
      url: typeof output.metadata?.url === "string" ? output.metadata.url : output.remoteUrl,
      snippet: output.text,
    }];
    if (output.kind === "search_suggestion") return [{ id: output.id, type: "search_suggestions", html: output.text ?? "" }];
    return [];
  });
  for (const item of record.outputs) {
    if (!item.output.providerFileId) continue;
    responseParts.push({
      id: `${item.output.id}-remote`,
      type: "remote_file",
      name: item.output.providerFileId,
      uri: item.output.remoteUrl ?? item.output.providerFileId,
      mimeType: item.output.mimeType ?? "application/octet-stream",
      sizeBytes: item.output.sizeBytes,
    });
  }
  const usage = record.usage.reduce<UsageSummary>((total, item) => ({
    inputTokens: total.inputTokens + (item.inputTokens ?? 0),
    outputTokens: total.outputTokens + (item.outputTokens ?? 0),
    thoughtTokens: total.thoughtTokens + (pickNumber(item.details ?? {}, "thoughtTokens", "thought_tokens", "thoughtsTokenCount") ?? 0),
    cachedTokens: total.cachedTokens + (pickNumber(item.details ?? {}, "cachedTokens", "cached_tokens", "cachedContentTokenCount") ?? 0),
    totalTokens: total.totalTokens + (item.inputTokens ?? 0) + (item.outputTokens ?? 0),
    generatedImages: total.generatedImages + (item.imageCount ?? 0),
    costUsd: (total.costUsd ?? 0) + (item.costMicros ?? 0) / 1_000_000,
  }), { inputTokens: 0, outputTokens: 0, thoughtTokens: 0, cachedTokens: 0, totalTokens: 0, generatedImages: 0, costUsd: 0 });
  if (usage.generatedImages === 0) usage.generatedImages = assets.length;
  if (record.usage.length > 0) responseParts.push({ id: `${run.id}-usage`, type: "usage", usage });
  const requestId = run.providerRequestId ?? record.errors.find((item) => item.error.requestId)?.error.requestId;
  if (requestId) responseParts.push({ id: `${run.id}-request`, type: "request_meta", requestId });
  if (run.redactedResponse !== null && run.redactedResponse !== undefined) {
    const rawJson = typeof run.redactedResponse === "string"
      ? run.redactedResponse
      : JSON.stringify(run.redactedResponse, null, 2);
    responseParts.push({ id: `${run.id}-raw`, type: "raw_response", json: rawJson });
  }
  const frontendDraft = Object.fromEntries(
    Object.entries(run.request.metadata?.frontendDraft ?? {}).filter(([, value]) => value !== null && value !== undefined),
  );
  if (typeof frontendDraft.customJson === "string" && /\[(?:REDACTED|OMITTED)\b/.test(frontendDraft.customJson)) {
    frontendDraft.customJson = "{}";
  }
  const storedNegativePrompt = typeof frontendDraft.negativePrompt === "string" ? frontendDraft.negativePrompt.trim() : "";
  const legacyFinalPrompt = run.finalPrompt || run.request.finalPrompt || run.request.prompt;
  const legacyNegativeSuffix = storedNegativePrompt ? `\n\nAvoid: ${storedNegativePrompt}` : "";
  const replayComposedPrompt = run.request.metadata?.composedPromptBase
    || (legacyNegativeSuffix && legacyFinalPrompt.endsWith(legacyNegativeSuffix)
      ? legacyFinalPrompt.slice(0, -legacyNegativeSuffix.length)
      : legacyFinalPrompt);
  const references = (run.request.inputs ?? []).map(mapRawReference).filter((item): item is ReferenceAsset => Boolean(item));
  const maskPath = run.request.mask?.source.path;
  const xaiStorageFilename = typeof frontendDraft.xaiStorageFilename === "string" ? frontendDraft.xaiStorageFilename : "";
  const replayMode = ["generate", "edit", "mask", "variation", "video"].includes(String(frontendDraft.mode))
    ? frontendDraft.mode as HistoryRecord["mode"]
    : mode;
  const draftSnapshot = {
    ...frontendDraft,
    storageFilename: xaiStorageFilename,
    persistRemoteFile: Boolean(xaiStorageFilename),
    publicFileUrl: frontendDraft.xaiPublicUrl === true,
    ttlSeconds: typeof frontendDraft.xaiExpiresAfter === "number" ? frontendDraft.xaiExpiresAfter : 3600,
    references,
    maskDataUrl: maskPath,
  } as Partial<GenerationDraft>;
  return {
    id: run.id,
    projectId: run.request.projectId,
    providerId: run.request.providerProfileId,
    providerName: run.request.providerProfileId,
    model: run.request.modelId,
    prompt: run.rawPrompt || run.request.prompt,
    composedPrompt: replayComposedPrompt,
    mode: replayMode,
    status,
    progress: status === "completed" ? 100 : status === "queued" ? 10 : status === "running" ? 50 : 0,
    count: run.request.output?.count ?? 1,
    createdAt: run.createdAt,
    durationMs,
    error: record.errors[0]?.error.message ?? (status === "failed" ? run.status : undefined),
    assets,
    responseParts,
    requestId,
    interactionId: run.request.metadata?.continuationId,
    usage: record.usage.length > 0 ? usage : undefined,
    favorite: false,
    draftSnapshot,
    contextSnapshot: (run.contextSnapshot ?? []).map((context) => normalizeCommonDescription({
      id: context.id,
      title: context.name,
      content: context.content,
      enabled: context.enabled,
      placement: context.placement === "append" ? "suffix" : "prefix",
      prefixContent: context.prefixContent,
      suffixContent: context.suffixContent,
      negativeContent: context.negativeContent,
      createdAt: context.createdAt,
    })),
    presetSnapshot: run.presetSnapshot ? mapRawPreset(run.presetSnapshot) : undefined,
    capabilityRegistryVersion: run.capabilityRegistryVersion,
  };
}

function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export function normalizeError(error: unknown): NormalizedError {
  if (typeof error === "string") {
    try {
      const parsed = JSON.parse(error) as unknown;
      if (parsed && typeof parsed === "object") return normalizeError(parsed);
    } catch {
      // Plain string errors are already the most useful representation.
    }
    return { code: "error", message: error || "Unknown error" };
  }
  if (error instanceof Error) {
    const nested = normalizeError(error.message);
    return nested.code === "error"
      ? { ...nested, details: error.cause }
      : nested;
  }
  if (error && typeof error === "object") {
    const obj = error as Record<string, unknown>;
    if (obj.error !== undefined && obj.message === undefined && obj.code === undefined) {
      return normalizeError(obj.error);
    }
    const code = typeof obj.code === "string" && obj.code ? obj.code : "error";
    const message = typeof obj.message === "string" && obj.message
      ? obj.message
      : typeof obj.error === "string" && obj.error
        ? obj.error
        : `Error [${code}]`;
    return { code, message, details: obj.details };
  }
  return { code: "error", message: String(error ?? "Unknown error") };
}

export function formatError(error: unknown): string {
  const normalized = normalizeError(error);
  return normalized.code === "error"
    ? normalized.message
    : `[${normalized.code}] ${normalized.message}`;
}

async function invokeWithFallback<T>(
  command: string,
  args: Record<string, unknown>,
  fallback: () => T | Promise<T>,
): Promise<T> {
  if (!isTauriRuntime()) return fallback();
  return invoke<T>(command, args);
}

function sanitizeSnapshot(snapshot: WorkspaceSnapshot): WorkspaceSnapshot {
  return {
    ...snapshot,
    history: isTauriRuntime() ? [] : snapshot.history,
    providers: snapshot.providers.map((provider) => {
      if (!isTauriRuntime() && provider.apiKey) sessionSecrets.set(provider.id, provider.apiKey);
      return {
        ...provider,
        apiKey: "",
        hasStoredSecret: provider.hasStoredSecret || (!isTauriRuntime() && sessionSecrets.has(provider.id)),
        customHeaders: provider.customHeaders?.map((header) => {
          const secret = header.secret || isSensitiveHeaderName(header.name);
          const key = headerSecretKey(provider.id, header.id);
          if (!isTauriRuntime() && secret && header.value) headerSessionSecrets.set(key, header.value);
          return secret
            ? { ...header, secret: true, value: "", hasStoredValue: header.hasStoredValue || (!isTauriRuntime() && headerSessionSecrets.has(key)) }
            : header;
        }),
      };
    }),
  };
}

function sanitizeProviderForCommand(provider: ProviderProfile): ProviderProfile {
  return {
    ...provider,
    apiKey: "",
    customHeaders: provider.customHeaders?.map((header) => ({
      ...header,
      value: header.secret || isSensitiveHeaderName(header.name) ? "" : header.value,
    })),
  };
}

function draftForCommand(request: GenerateRequest): Record<string, unknown> {
  const draft = request.draft;
  const persistRemoteFile = draft.persistRemoteFile && draft.storageFilename.trim().length > 0;
  return {
    ...draft,
    responseFormat: ["dall-e-2", "dall-e-3"].includes(draft.model) || request.provider.kind === "xai"
      ? draft.responseFormat
      : undefined,
    style: draft.model === "dall-e-3" ? draft.style : undefined,
    moderation: draft.model.startsWith("gpt-image-") && draft.moderation ? draft.moderation : undefined,
    storeInteraction: draft.storeInteraction || draft.remoteStore,
    stream: draft.stream,
    useInteractionsApi: draft.useInteractionsApi,
    previousResponseId: draft.previousResponseId.trim() || undefined,
    previousInteractionId: draft.previousInteractionId.trim() || undefined,
    resumeInteractionId: draft.useInteractionsApi ? draft.previousInteractionId.trim() || undefined : undefined,
    lastEventId: draft.lastEventId.trim() || undefined,
    responseModel: draft.responseModel.trim() || undefined,
    imageGenerationAction: draft.useResponsesApi ? draft.imageGenerationAction : undefined,
    xaiStorageFilename: persistRemoteFile ? draft.storageFilename.trim() : undefined,
    xaiExpiresAfter: persistRemoteFile ? draft.ttlSeconds : undefined,
    xaiPublicUrl: persistRemoteFile ? draft.publicFileUrl : undefined,
    xaiPublicUrlExpiresAfter: persistRemoteFile && draft.publicFileUrl ? draft.ttlSeconds : undefined,
  };
}

function contextSnapshotsForCommand(contexts: CommonDescription[]): Array<Record<string, unknown>> {
  const now = new Date().toISOString();
  return contexts.map((context, index) => ({
    id: context.id,
    name: context.title,
    content: context.prefixContent || context.suffixContent,
    placement: context.prefixContent ? "prepend" : "append",
    prefixContent: context.prefixContent,
    suffixContent: context.suffixContent,
    negativeContent: context.negativeContent,
    sortOrder: index,
    enabled: context.enabled,
    createdAt: context.createdAt,
    updatedAt: now,
  }));
}

function presetSnapshotForCommand(preset: GenerationPreset | undefined): Record<string, unknown> | undefined {
  if (!preset) return undefined;
  const now = new Date().toISOString();
  return {
    id: preset.id,
    name: preset.name,
    providerProfileId: preset.providerId || undefined,
    modelId: preset.model || undefined,
    operation: preset.mode === "video" ? "video_reference_to_image" : preset.mode === "mask" ? "edit" : preset.mode,
    parameters: { description: preset.description, promptTemplate: preset.promptTemplate, outputFormat: preset.outputFormat },
    output: {
      count: 1,
      size: preset.size === "auto" ? { mode: "auto" } : { mode: "preset", value: preset.size },
      aspectRatio: preset.aspectRatio || undefined,
      quality: preset.quality === "auto" ? undefined : preset.quality,
    },
    createdAt: preset.createdAt,
    updatedAt: now,
  };
}

function pickString(source: Record<string, unknown>, ...keys: string[]): string | undefined {
  for (const key of keys) if (typeof source[key] === "string") return source[key] as string;
  return undefined;
}

function pickNumber(source: Record<string, unknown>, ...keys: string[]): number | undefined {
  for (const key of keys) if (typeof source[key] === "number") return source[key] as number;
  return undefined;
}

function normalizeAsset(source: Record<string, unknown>): GeneratedAsset {
  return {
    id: pickString(source, "id") ?? crypto.randomUUID(),
    taskId: pickString(source, "taskId", "task_id") ?? "",
    url: pickString(source, "url") ?? "",
    filePath: pickString(source, "filePath", "file_path") ?? "",
    width: pickNumber(source, "width") ?? 0,
    height: pickNumber(source, "height") ?? 0,
    format: pickString(source, "format") ?? "png",
    prompt: pickString(source, "prompt") ?? "",
    createdAt: pickString(source, "createdAt", "created_at") ?? new Date().toISOString(),
    selected: typeof source.selected === "boolean" ? source.selected : undefined,
  };
}

function normalizeUsage(raw: RawUsage | undefined, imageCount: number): UsageSummary {
  const inputTokens = raw?.inputTokens ?? raw?.input_tokens ?? 0;
  const outputTokens = raw?.outputTokens ?? raw?.output_tokens ?? 0;
  const totalTokens = raw?.totalTokens ?? raw?.total_tokens ?? inputTokens + outputTokens;
  return {
    inputTokens,
    outputTokens,
    thoughtTokens: raw?.thoughtTokens ?? raw?.thought_tokens ?? 0,
    cachedTokens: raw?.cachedTokens ?? raw?.cached_tokens ?? 0,
    totalTokens,
    generatedImages: raw?.generatedImages ?? raw?.generated_images ?? imageCount,
    imageTokens: raw?.imageTokens ?? raw?.image_tokens,
    costUsd: raw?.costUsd ?? raw?.cost_usd,
  };
}

function normalizeTauriResult(raw: RawGenerationCommandResult): GenerationResult {
  const assets = (raw.assets ?? []).map(normalizeAsset);
  const responseParts: ResponsePart[] = [];
  let hasUsagePart = false;
  let hasRequestMetaPart = false;

  for (const rawPart of raw.responseParts ?? raw.response_parts ?? []) {
    const type = rawPart.type;
    const id = pickString(rawPart, "id") ?? crypto.randomUUID();
    if (type === "image") {
      responseParts.push({
        id,
        type: "image",
        assetId: pickString(rawPart, "assetId", "asset_id") ?? "",
        url: pickString(rawPart, "url") ?? "",
        mimeType: pickString(rawPart, "mimeType", "mime_type") ?? "image/png",
        width: pickNumber(rawPart, "width") ?? 0,
        height: pickNumber(rawPart, "height") ?? 0,
        filePath: pickString(rawPart, "filePath", "file_path") ?? "",
      });
    } else if (type === "text") {
      responseParts.push({ id, type: "text", text: pickString(rawPart, "text") ?? "" });
    } else if (type === "thought") {
      responseParts.push({ id, type: "thought", summary: pickString(rawPart, "summary") ?? "", imageUrl: pickString(rawPart, "imageUrl", "image_url") });
    } else if (type === "citation") {
      responseParts.push({
        id, type: "citation", title: pickString(rawPart, "title"),
        url: pickString(rawPart, "url"), snippet: pickString(rawPart, "snippet"),
        startIndex: pickNumber(rawPart, "startIndex", "start_index"),
        endIndex: pickNumber(rawPart, "endIndex", "end_index"),
      });
    } else if (type === "search_suggestions") {
      responseParts.push({ id, type: "search_suggestions", html: pickString(rawPart, "html") ?? "" });
    } else if (type === "remote_file") {
      responseParts.push({
        id, type: "remote_file", name: pickString(rawPart, "name") ?? "remote-file",
        uri: pickString(rawPart, "uri") ?? "", mimeType: pickString(rawPart, "mimeType", "mime_type") ?? "application/octet-stream",
        sizeBytes: pickNumber(rawPart, "sizeBytes", "size_bytes"),
      });
    } else if (type === "remote_job") {
      responseParts.push({
        id, type: "remote_job", jobId: pickString(rawPart, "jobId", "job_id") ?? "",
        kind: pickString(rawPart, "kind") ?? "", status: pickString(rawPart, "status") ?? "",
        provider: pickString(rawPart, "provider") ?? "", model: pickString(rawPart, "model"),
      });
    } else if (type === "usage" && rawPart.usage && typeof rawPart.usage === "object") {
      responseParts.push({ id, type: "usage", usage: normalizeUsage(rawPart.usage as RawUsage, assets.length) });
      hasUsagePart = true;
    } else if (type === "request_meta") {
      responseParts.push({
        id, type: "request_meta", requestId: pickString(rawPart, "requestId", "request_id") ?? "",
        interactionId: pickString(rawPart, "interactionId", "interaction_id"),
        providerResponseId: pickString(rawPart, "providerResponseId", "provider_response_id"),
      });
      hasRequestMetaPart = true;
    }
  }

  const requestId = raw.requestId ?? raw.request_id ?? crypto.randomUUID();
  const interactionId = raw.interactionId ?? raw.interaction_id;
  const usage = normalizeUsage(raw.usage, assets.length);
  if (!hasUsagePart) responseParts.push({ id: crypto.randomUUID(), type: "usage", usage });
  if (!hasRequestMetaPart) responseParts.push({ id: crypto.randomUUID(), type: "request_meta", requestId, interactionId });
  return {
    runId: raw.runId ?? raw.run_id,
    requestId,
    interactionId,
    failureReason: raw.failureReason ?? raw.failure_reason,
    assets,
    responseParts,
    usage,
  };
}

async function restoreSecrets(snapshot: WorkspaceSnapshot): Promise<WorkspaceSnapshot> {
  const providers = await Promise.all(snapshot.providers.map(async (provider) => {
    const normalizedProvider = { ...provider, discoveredModels: provider.discoveredModels ?? [] };
    if (!isTauriRuntime()) return {
      ...normalizedProvider,
      apiKey: "",
      hasStoredSecret: provider.hasStoredSecret || sessionSecrets.has(provider.id),
      customHeaders: provider.customHeaders?.map((header) => ({
        ...header,
        value: "",
        hasStoredValue: header.hasStoredValue || headerSessionSecrets.has(headerSecretKey(provider.id, header.id)),
      })),
    };
    const hasStoredSecret = await invoke<boolean>("provider_secret_exists", { providerId: provider.id });
    const customHeaders = await Promise.all((provider.customHeaders ?? []).map(async (header) => {
      if (!header.secret && !isSensitiveHeaderName(header.name)) return header;
      const hasStoredValue = await invoke<boolean>("provider_header_secret_exists", { providerId: provider.id, headerId: header.id });
      return { ...header, secret: true, value: "", hasStoredValue };
    }));
    return { ...normalizedProvider, apiKey: "", hasStoredSecret, customHeaders };
  }));
  const history = snapshot.history.map((record) => ({
    ...record,
    assets: record.assets ?? [],
    responseParts: record.responseParts ?? [],
  }));
  const projects = snapshot.projects.map((project) => ({
    ...project,
    descriptions: (project.descriptions ?? []).map((description) => normalizeCommonDescription(description as unknown as LegacyCommonDescription)),
  }));
  return { ...snapshot, projects, providers, history };
}

function wait(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

function mimeTypeForPath(path: string): string {
  const extension = path.split(/[?#]/, 1)[0].split(".").at(-1)?.toLowerCase();
  return ({
    png: "image/png",
    jpg: "image/jpeg",
    jpeg: "image/jpeg",
    webp: "image/webp",
    heic: "image/heic",
    heif: "image/heif",
    gif: "image/gif",
    mp4: "video/mp4",
    webm: "video/webm",
    mov: "video/quicktime",
  } as Record<string, string>)[extension ?? ""] ?? "application/octet-stream";
}

function resolveCanvasSize(aspectRatio: string): [number, number] {
  const ratios: Record<string, [number, number]> = {
    "1:1": [720, 720],
    "2:3": [640, 960],
    "3:2": [960, 640],
    "3:4": [660, 880],
    "4:3": [880, 660],
    "9:16": [540, 960],
    "16:9": [960, 540],
    "21:9": [1050, 450],
  };
  return ratios[aspectRatio] ?? [880, 660];
}

function createDemoImage(prompt: string, index: number, aspectRatio: string): { url: string; width: number; height: number } {
  const [width, height] = resolveCanvasSize(aspectRatio);
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const context = canvas.getContext("2d");
  if (!context) return { url: "", width, height };

  const palettes = [
    ["#dfe8e3", "#315b52", "#e26b4d", "#f4c76a"],
    ["#e7e5df", "#33445b", "#c65b43", "#7a9d8c"],
    ["#e8edf2", "#3d5274", "#d49a45", "#ba5b61"],
    ["#e9e2d8", "#384a40", "#b85e43", "#6e8da7"],
  ][index % 4];

  context.fillStyle = palettes[0];
  context.fillRect(0, 0, width, height);
  context.fillStyle = palettes[1];
  context.fillRect(width * 0.08, height * 0.12, width * 0.54, height * 0.66);
  context.fillStyle = palettes[2];
  context.fillRect(width * 0.48, height * 0.28, width * 0.42, height * 0.54);
  context.fillStyle = palettes[3];
  context.fillRect(width * 0.17, height * 0.66, width * 0.62, height * 0.16);

  context.globalAlpha = 0.2;
  context.fillStyle = "#ffffff";
  for (let x = 0; x < width; x += 18) context.fillRect(x, 0, 1, height);
  context.globalAlpha = 1;

  const shortPrompt = prompt.replace(/\s+/g, " ").slice(0, 64);
  context.fillStyle = "#ffffff";
  context.font = `600 ${Math.max(18, Math.round(width / 34))}px "Segoe UI", sans-serif`;
  context.fillText(`IMAGE ${String(index + 1).padStart(2, "0")}`, width * 0.12, height * 0.2);
  context.font = `500 ${Math.max(14, Math.round(width / 52))}px "Segoe UI", sans-serif`;
  const words = shortPrompt.split(" ");
  const lines: string[] = [];
  let current = "";
  words.forEach((word) => {
    const next = `${current} ${word}`.trim();
    if (context.measureText(next).width > width * 0.38 && current) {
      lines.push(current);
      current = word;
    } else {
      current = next;
    }
  });
  if (current) lines.push(current);
  lines.slice(0, 4).forEach((line, lineIndex) => {
    context.fillText(line, width * 0.12, height * (0.27 + lineIndex * 0.05));
  });

  return { url: canvas.toDataURL("image/png"), width, height };
}

export const api = {
  isDemo: !isTauriRuntime(),

  async loadWorkspace(): Promise<WorkspaceSnapshot | null> {
    const snapshot = await invokeWithFallback<WorkspaceSnapshot | null>("workspace_load", {}, () => {
      const value = window.localStorage.getItem(STORAGE_KEY);
      if (!value) return null;
      try {
        return JSON.parse(value) as WorkspaceSnapshot;
      } catch {
        return null;
      }
    });
    return snapshot ? restoreSecrets(snapshot) : null;
  },

  async saveWorkspace(snapshot: WorkspaceSnapshot): Promise<void> {
    const sanitized = sanitizeSnapshot(snapshot);
    return invokeWithFallback("workspace_save", { snapshot: sanitized }, () => {
      window.localStorage.setItem(STORAGE_KEY, JSON.stringify(sanitized));
    });
  },

  async diagnostics(): Promise<DiagnosticReport> {
    if (!isTauriRuntime()) {
      return {
        appVersion: "browser-demo",
        os: navigator.platform || "browser",
        architecture: "browser",
        appDataDirectory: "localStorage",
        logDirectory: "Browser developer tools",
        logFiles: [],
        logTail: "",
        credentialStoreStatus: "demo",
        credentialStoreMessage: "Session-only browser storage",
        projects: [],
      };
    }
    return invoke<DiagnosticReport>("diagnostics_report");
  },

  async openDiagnosticsFolder(): Promise<void> {
    if (!isTauriRuntime()) return;
    await invoke("diagnostics_open_logs");
  },

  async storeProviderSecret(providerId: string, apiKey: string): Promise<void> {
    if (!apiKey.trim()) return;
    if (!isTauriRuntime()) {
      sessionSecrets.set(providerId, apiKey);
      return;
    }
    await invoke("provider_secret_set", { providerId, apiKey });
  },

  async storeProviderHeaderSecret(providerId: string, headerId: string, value: string): Promise<void> {
    if (!value) return;
    if (!isTauriRuntime()) {
      headerSessionSecrets.set(headerSecretKey(providerId, headerId), value);
      return;
    }
    await invoke("provider_header_secret_set", { providerId, headerId, value });
  },

  async deleteProvider(providerId: string): Promise<boolean> {
    if (!isTauriRuntime()) return true;
    return invoke<boolean>("provider_delete", { providerId });
  },

  async chooseDirectory(suggestedPath = ""): Promise<string | null> {
    if (!isTauriRuntime()) return suggestedPath || "ImageWorkbench";
    const { open } = await import("@tauri-apps/plugin-dialog");
    const selection = await open({ directory: true, multiple: false });
    return typeof selection === "string" ? selection : null;
  },

  async chooseReferenceFiles(projectId: string, includeVideo = false, pngOnly = false): Promise<Array<Omit<ReferenceAsset, "id" | "role">>> {
    if (!isTauriRuntime()) return [];
    const { open } = await import("@tauri-apps/plugin-dialog");
    const selection = await open({
      multiple: true,
      directory: false,
      filters: [{
        name: includeVideo ? "Reference media" : "Reference images",
        extensions: pngOnly
          ? ["png"]
          : includeVideo
          ? ["png", "jpg", "jpeg", "webp", "heic", "heif", "mp4", "webm", "mov"]
          : ["png", "jpg", "jpeg", "webp", "heic", "heif"],
      }],
    });
    const paths = Array.isArray(selection) ? selection : selection ? [selection] : [];
    const imported = await invoke<ImportedInputDto[]>("project_import_inputs", { projectId, paths });
    return Promise.all(imported.map(async (item, index) => ({
      name: paths[index]?.split(/[\\/]/).filter(Boolean).at(-1) ?? "reference",
      url: item.relativePath,
      mimeType: item.mimeType || mimeTypeForPath(paths[index] ?? item.relativePath),
      sourceType: "local" as const,
      width: item.width,
      height: item.height,
      previewUrl: await invoke<string | null>("reference_preview", { projectId, path: item.relativePath }).then((value) => value ?? undefined),
    })));
  },

  referencePreviewUrl(reference: ReferenceAsset): string {
    if (reference.sourceType === "file-id" || !reference.url) return "";
    if (isTauriRuntime() && reference.sourceType === "local") return reference.previewUrl ?? "";
    return reference.url;
  },

  async previewProjectAsset(projectId: string, path: string): Promise<string | undefined> {
    if (!path || !isTauriRuntime()) return path.startsWith("data:") ? path : undefined;
    return invoke<string | null>("reference_preview", { projectId, path }).then((value) => value ?? undefined);
  },

  async readProjectAssetDataUrl(projectId: string, path: string): Promise<string | undefined> {
    if (!path || !isTauriRuntime()) return path.startsWith("data:") ? path : undefined;
    return invoke<string | null>("project_asset_data_url", { projectId, path }).then((value) => value ?? undefined);
  },

  async hydrateReferencePreviews(projectId: string, references: ReferenceAsset[]): Promise<ReferenceAsset[]> {
    return Promise.all(references.map(async (reference) => reference.sourceType !== "local"
      ? reference
      : {
          ...reference,
          previewUrl: await this.previewProjectAsset(projectId, reference.url),
        }));
  },

  async openProject(): Promise<ProjectSummary | null> {
    const path = await this.chooseDirectory();
    if (!path) return null;
    if (isTauriRuntime()) return invoke<ProjectSummary>("project_open", { path });
    const now = new Date().toISOString();
    return {
      id: `browser-${crypto.randomUUID()}`,
      name: path.split(/[\\/]/).filter(Boolean).at(-1) ?? "Imported project",
      rootPath: path,
      createdAt: now,
      updatedAt: now,
      lastOpenedAt: now,
      defaultParameters: {},
    };
  },

  async createProject(project: Project): Promise<ProjectSummary> {
    if (!isTauriRuntime()) {
      return {
        id: project.id,
        name: project.name,
        rootPath: project.storagePath,
        createdAt: project.createdAt,
        updatedAt: project.updatedAt,
        lastOpenedAt: project.updatedAt,
        defaultProviderProfileId: project.settings.defaultProviderId || undefined,
        defaultModelId: project.settings.defaultModel || undefined,
        defaultParameters: {},
      };
    }
    return invoke<ProjectSummary>("project_create", {
      projectId: project.id,
      name: project.name,
      path: project.storagePath,
    });
  },

  async updateProject(project: Project): Promise<ProjectSummary> {
    if (!isTauriRuntime()) {
      return {
        id: project.id,
        name: project.name,
        rootPath: project.storagePath,
        createdAt: project.createdAt,
        updatedAt: project.updatedAt,
        lastOpenedAt: project.updatedAt,
        defaultProviderProfileId: project.settings.defaultProviderId || undefined,
        defaultModelId: project.settings.defaultModel || undefined,
        defaultParameters: {},
      };
    }
    return invoke<ProjectSummary>("project_update", { project });
  },

  async duplicateProject(
    sourceProjectId: string,
    projectId: string,
    name: string,
    path: string,
    mode: "full" | "configuration",
  ): Promise<ProjectSummary> {
    if (!isTauriRuntime()) {
      const now = new Date().toISOString();
      return {
        id: projectId,
        name,
        rootPath: path,
        createdAt: now,
        updatedAt: now,
        lastOpenedAt: now,
        defaultParameters: {},
      };
    }
    return invoke<ProjectSummary>("project_duplicate", {
      sourceProjectId,
      projectId,
      name,
      path,
      mode,
    });
  },

  async deleteProject(projectId: string, deleteFiles: boolean): Promise<{ removed: boolean; filesDeleted: boolean; fileError?: string }> {
    if (!isTauriRuntime()) return { removed: true, filesDeleted: deleteFiles };
    return invoke<{ removed: boolean; filesDeleted: boolean; fileError?: string }>("project_delete", { projectId, deleteFiles });
  },

  async moveProject(projectId: string, destination: string, deleteOriginal = false): Promise<ProjectSummary> {
    if (!isTauriRuntime()) throw new Error("Project moves require the desktop application");
    return invoke<ProjectSummary>("project_move", { projectId, destination, deleteOriginal });
  },

  async loadProjectDetails(projectId: string): Promise<{ descriptions: CommonDescription[]; presets: GenerationPreset[]; history: HistoryRecord[] }> {
    if (!isTauriRuntime()) return { descriptions: [], presets: [], history: [] };
    // Results and history are user-visible archives, not a recent activity
    // feed. Keep a generous server-side bound while retaining pagination in
    // the UI for very large projects.
    const details = await invoke<RawProjectDetails>("project_load_details", { projectId, recentRunLimit: 5000 });
    const contexts = details.contexts;
    const rawPresets = details.presets;
    const descriptions = [...contexts]
      .sort((left, right) => left.sortOrder - right.sortOrder)
      .map((context) => ({
        id: context.id,
        title: context.name,
        content: context.content,
        placement: context.placement === "append" ? "suffix" as const : "prefix" as const,
        prefixContent: context.prefixContent,
        suffixContent: context.suffixContent,
        negativeContent: context.negativeContent,
        enabled: context.enabled,
        createdAt: context.createdAt,
      }))
      .map((description) => normalizeCommonDescription(description));
    const presets = rawPresets.map(mapRawPreset);
    const history = details.recentRecords.map(mapHistoryDetails);
    return { descriptions, presets, history };
  },

  async upsertPromptContext(projectId: string, description: CommonDescription, sortOrder: number): Promise<void> {
    if (!isTauriRuntime()) return;
    const now = new Date().toISOString();
    await invoke("prompt_context_upsert", {
      projectId,
      context: {
        id: description.id,
        name: description.title,
        content: description.prefixContent || description.suffixContent,
        placement: description.prefixContent ? "prepend" : "append",
        prefixContent: description.prefixContent,
        suffixContent: description.suffixContent,
        negativeContent: description.negativeContent,
        sortOrder,
        enabled: description.enabled,
        createdAt: description.createdAt,
        updatedAt: now,
      },
    });
  },

  async deletePromptContext(projectId: string, contextId: string): Promise<boolean> {
    if (!isTauriRuntime()) return true;
    return invoke<boolean>("prompt_context_delete", { projectId, contextId });
  },

  async upsertGenerationPreset(projectId: string, preset: GenerationPreset): Promise<void> {
    if (!isTauriRuntime()) return;
    const now = new Date().toISOString();
    await invoke("generation_preset_upsert", {
      projectId,
      preset: {
        id: preset.id,
        name: preset.name,
        providerProfileId: preset.providerId || undefined,
        modelId: preset.model || undefined,
        operation: preset.mode === "video" ? "video_reference_to_image" : preset.mode === "mask" ? "edit" : preset.mode,
        parameters: { description: preset.description, promptTemplate: preset.promptTemplate, outputFormat: preset.outputFormat },
        output: {
          count: 1,
          size: preset.size === "auto" ? { mode: "auto" } : { mode: "preset", value: preset.size },
          aspectRatio: preset.aspectRatio || undefined,
          quality: preset.quality === "auto" ? undefined : preset.quality,
        },
        createdAt: preset.createdAt,
        updatedAt: now,
      },
    });
  },

  async deleteGenerationPreset(projectId: string, presetId: string): Promise<boolean> {
    if (!isTauriRuntime()) return true;
    return invoke<boolean>("generation_preset_delete", { projectId, presetId });
  },

  async revealPath(path: string): Promise<void> {
    if (!isTauriRuntime()) return;
    await invoke<void>("reveal_path", { path });
  },

  async exportAsset(sourcePath: string, suggestedName: string, previewUrl: string): Promise<boolean> {
    if (!isTauriRuntime()) {
      const link = document.createElement("a");
      link.href = previewUrl;
      link.download = suggestedName;
      document.body.append(link);
      link.click();
      link.remove();
      return true;
    }

    const extension = suggestedName.split(".").at(-1)?.toLowerCase();
    // Bring the main window to the foreground so the native save dialog is not
    // spawned behind it (a common cause of the picker appearing to do nothing on Windows).
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      await getCurrentWindow().setFocus();
    } catch {
      // Non-fatal: continue to open the dialog even if focusing fails.
    }
    const { save } = await import("@tauri-apps/plugin-dialog");
    const destinationPath = await save({
      defaultPath: suggestedName,
      filters: extension ? [{ name: "Image", extensions: [extension] }] : undefined,
    });
    if (!destinationPath) return false;
    await invoke<void>("export_asset", { sourcePath, destinationPath });
    return true;
  },

  async exportAssets(assets: BatchAssetExport[]): Promise<BatchExportResult> {
    if (assets.length === 0) return { exported: 0, exportedPaths: [], failures: [] };
    if (!isTauriRuntime()) {
      for (const asset of assets) {
        const link = document.createElement("a");
        link.href = asset.previewUrl;
        link.download = asset.suggestedName;
        document.body.append(link);
        link.click();
        link.remove();
      }
      return { exported: assets.length, exportedPaths: [], failures: [] };
    }
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      await getCurrentWindow().setFocus();
    } catch {
      // The directory picker can still open if focusing the window is unsupported.
    }
    const { open } = await import("@tauri-apps/plugin-dialog");
    const destinationDirectory = await open({ directory: true, multiple: false });
    if (typeof destinationDirectory !== "string") {
      return { exported: 0, exportedPaths: [], failures: [] };
    }
    return invoke<BatchExportResult>("export_assets", {
      destinationDirectory,
      assets: assets.map(({ sourcePath, suggestedName }) => ({ sourcePath, suggestedName })),
    });
  },

  async testProvider(provider: ProviderProfile): Promise<boolean> {
    const sanitizedProvider = sanitizeProviderForCommand(provider);
    return invokeWithFallback("provider_test", { provider: sanitizedProvider }, async () => {
      await wait(550);
      return Boolean(provider.baseUrl.trim() && (sessionSecrets.has(provider.id) || provider.hasStoredSecret));
    });
  },

  async syncModels(provider: ProviderProfile): Promise<string[]> {
    const sanitizedProvider = sanitizeProviderForCommand(provider);
    return invokeWithFallback("provider_sync_models", { provider: sanitizedProvider }, async () => {
      await wait(700);
      return providerDefaultModels[provider.kind];
    });
  },

  async queueStatus(): Promise<boolean> {
    if (!isTauriRuntime()) return demoQueuePaused;
    return invoke<boolean>("queue_status");
  },

  async pauseQueue(): Promise<boolean> {
    if (!isTauriRuntime()) {
      demoQueuePaused = true;
      return true;
    }
    return invoke<boolean>("queue_pause");
  },

  async resumeQueue(): Promise<boolean> {
    if (!isTauriRuntime()) {
      demoQueuePaused = false;
      notifyDemoQueueWaiters();
      return false;
    }
    return invoke<boolean>("queue_resume");
  },

  async cancelRun(runId: string, projectId?: string): Promise<boolean> {
    cancelledRuns.add(runId);
    if (!isTauriRuntime()) {
      notifyDemoQueueWaiters();
      return true;
    }
    return invoke<boolean>("run_cancel", { runId, projectId });
  },

  async deleteHistory(projectId: string, runId: string): Promise<HistoryMutationResult> {
    if (!isTauriRuntime()) return { requestedRuns: 1, deletedRuns: 1, localAssetsDeleted: 0, remoteFilesDeleted: 0, remoteFilesRetained: 0, failures: [] };
    return invoke<HistoryMutationResult>("history_delete", {
      projectId,
      runId,
      deleteLocalAssets: true,
      deleteRemoteFiles: true,
    });
  },

  async deleteResultAssets(
    projectId: string,
    assets: Array<{ runId: string; outputId: string }>,
  ): Promise<AssetMutationResult> {
    if (!isTauriRuntime()) {
      return {
        requestedAssets: assets.length,
        deletedAssetIds: assets.map((asset) => asset.outputId),
        localAssetsDeleted: assets.length,
        remoteFilesDeleted: 0,
        remoteFilesRetained: 0,
        failures: [],
      };
    }
    return invoke<AssetMutationResult>("results_delete_assets", { projectId, assets });
  },

  async clearHistory(projectId: string): Promise<HistoryMutationResult> {
    if (!isTauriRuntime()) return { requestedRuns: 0, deletedRuns: 0, localAssetsDeleted: 0, remoteFilesDeleted: 0, remoteFilesRetained: 0, failures: [] };
    return invoke<HistoryMutationResult>("history_clear", {
      projectId,
      deleteLocalAssets: true,
      deleteRemoteFiles: true,
    });
  },

  async remapProjectProvider(projectId: string, fromProviderId: string, toProviderId: string): Promise<void> {
    if (!isTauriRuntime() || !fromProviderId || !toProviderId || fromProviderId === toProviderId) return;
    await invoke("project_remap_provider", { projectId, fromProviderId, toProviderId });
  },

  async listenGenerationEvents(handler: (envelope: GenerationEventEnvelope) => void): Promise<() => void> {
    if (!isTauriRuntime()) return () => undefined;
    const { listen } = await import("@tauri-apps/api/event");
    return listen<GenerationEventEnvelope>("generation-event", (event) => handler(event.payload));
  },

  async pollRemoteTasks(projectId: string): Promise<GenerationResult[]> {
    if (!isTauriRuntime()) return [];
    const results = await invoke<RawGenerationCommandResult[]>("remote_tasks_poll", { projectId });
    return results.map(normalizeTauriResult);
  },

  async generate(request: GenerateRequest): Promise<GenerationResult> {
    if (isTauriRuntime()) {
      try {
        const result = await invoke<RawGenerationCommandResult>("generate_images", {
          request: {
            ...request,
            provider: sanitizeProviderForCommand(request.provider),
            draft: draftForCommand(request),
            manualNegativePrompt: request.manualNegativePrompt,
            contextSnapshot: contextSnapshotsForCommand(request.contextSnapshot),
            presetSnapshot: presetSnapshotForCommand(request.presetSnapshot),
          },
        });
        if (cancelledRuns.has(request.clientTaskId)) throw new Error("Cancelled");
        return normalizeTauriResult(result);
      } finally {
        cancelledRuns.delete(request.clientTaskId);
      }
    }
    return invokeWithFallback("generate_images", { request }, async () => {
      await waitForDemoQueue(request.clientTaskId);
      await wait(900);
      if (cancelledRuns.delete(request.clientTaskId)) throw new Error("Cancelled");
      const taskId = request.clientTaskId;
      const requestId = `demo_req_${crypto.randomUUID()}`;
      const interactionId = request.provider.kind === "gemini" && request.draft.storeInteraction
        ? `demo_interaction_${crypto.randomUUID()}`
        : undefined;
      const outputFormat = ["png", "jpeg", "webp"].includes(request.draft.outputFormat)
        ? request.draft.outputFormat
        : "png";
      const assets = Array.from({ length: request.draft.count }, (_, index) => {
        const demo = createDemoImage(request.composedPrompt, index, request.draft.aspectRatio);
        return {
          id: crypto.randomUUID(),
          taskId,
          url: demo.url,
          filePath: `${request.storagePath}\\${taskId}-${index + 1}.png`,
          width: demo.width,
          height: demo.height,
          format: outputFormat,
          prompt: request.composedPrompt,
          createdAt: new Date().toISOString(),
        };
      });
      const usage = {
        inputTokens: Math.max(18, Math.round(request.composedPrompt.length / 3.5)),
        outputTokens: request.draft.includeText ? 86 : 0,
        thoughtTokens: request.draft.thinkingLevel === "high" ? 640 : request.provider.kind === "gemini" ? 120 : 0,
        cachedTokens: 0,
        totalTokens: 0,
        generatedImages: assets.length,
      };
      usage.totalTokens = usage.inputTokens + usage.outputTokens + usage.thoughtTokens;

      const responseParts: GenerationResult["responseParts"] = assets.map((asset) => ({
        id: crypto.randomUUID(),
        type: "image",
        assetId: asset.id,
        url: asset.url,
        mimeType: `image/${asset.format}`,
        width: asset.width,
        height: asset.height,
        filePath: asset.filePath,
      }));
      if (request.provider.kind === "gemini") {
        responseParts.unshift({
          id: crypto.randomUUID(),
          type: "thought",
          summary: request.draft.thinkingLevel === "high"
            ? "Refined subject placement, typography-safe negative space, and material lighting before rendering."
            : "Checked composition and visual hierarchy before rendering.",
        });
      }
      if (request.draft.includeText) {
        responseParts.push({
          id: crypto.randomUUID(),
          type: "text",
          text: "The generated set preserves the requested composition while varying material balance and camera distance across outputs.",
        });
      }
      if (request.draft.webSearch || request.draft.imageSearch) {
        responseParts.push(
          {
            id: crypto.randomUUID(),
            type: "citation",
            title: "Google Search grounding source",
            url: "https://ai.google.dev/gemini-api/docs/google-search",
          },
          {
            id: crypto.randomUUID(),
            type: "search_suggestions",
            html: '<div><strong>Search suggestions</strong><ul><li><a href="https://www.google.com/search?q=editorial+product+photography">Editorial product photography</a></li><li><a href="https://www.google.com/search?q=modern+garden+design">Modern garden design</a></li></ul></div>',
          },
        );
      }
      responseParts.push(
        {
          id: crypto.randomUUID(),
          type: "remote_file",
          name: `${taskId}.json`,
          uri: `https://files.example.invalid/imageworkbench/${taskId}.json`,
          mimeType: "application/json",
          sizeBytes: 2840,
        },
        { id: crypto.randomUUID(), type: "usage", usage },
        { id: crypto.randomUUID(), type: "request_meta", requestId, interactionId },
      );
      return { requestId, interactionId, assets, responseParts, usage };
    });
  },

  async generateImages(request: GenerateRequest): Promise<GeneratedAsset[]> {
    return (await this.generate(request)).assets;
  },
};
