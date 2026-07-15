export type Locale = "zh-CN" | "en-US";

export type ProviderKind = "openai" | "xai" | "gemini" | "custom";

export interface ProviderHeader {
  id: string;
  name: string;
  value: string;
  secret: boolean;
  hasStoredValue?: boolean;
}

export type WorkspaceTab =
  | "create"
  | "history"
  | "descriptions"
  | "presets"
  | "project-settings";

export type GenerationMode = "generate" | "edit" | "mask" | "variation" | "video";

export type TaskStatus = "queued" | "running" | "completed" | "failed";

export type ReferenceSourceType = "local" | "url" | "base64" | "file-id";

export interface ModelCapabilities {
  id: string;
  label: string;
  providerKind: ProviderKind;
  modes: GenerationMode[];
  aspectRatios: string[];
  sizes: string[];
  qualityOptions: string[];
  outputFormats: string[];
  responseFormats: string[];
  backgrounds: string[];
  maxReferences: number;
  maxCount: number;
  supportsMask: boolean;
  supportsCustomSize: boolean;
  customSizeRule?: {
    multipleOf: number;
    minAspectRatio: number;
    maxAspectRatio: number;
    maxEdge: number;
    maxPixels: number;
  };
  supportsInputFidelity: boolean;
  supportsThinking: boolean;
  thinkingLevels: string[];
  supportsWebSearch: boolean;
  supportsImageSearch: boolean;
  supportsStreaming: boolean;
  supportsBackground: boolean;
  supportsBatch: boolean;
  supportsSeed: boolean;
  supportsPartialImages: boolean;
  supportsTextOutput: boolean;
  supportsServiceTier: boolean;
  supportsConversation?: boolean;
  supportsResponsesApi: boolean;
  supportsRemoteFiles: boolean;
  supportsRemoteStore: boolean;
  supportsOutputModalities: boolean;
  moderationOptions: string[];
  styleOptions: string[];
  imageGenerationActions: string[];
}

export interface ProviderProfile {
  id: string;
  name: string;
  kind: ProviderKind;
  baseUrl: string;
  apiKey: string;
  hasStoredSecret?: boolean;
  apiMode: "native" | "openai-compatible";
  enabled: boolean;
  models: string[];
  apiVersion?: string;
  organization?: string;
  projectId?: string;
  customHeader?: string;
  lastSyncedAt?: string;
  timeoutMs?: number;
  proxyUrl?: string;
  authScheme?: "bearer" | "header" | "query";
  authHeaderName?: string;
  authPrefix?: string;
  authQueryName?: string;
  customHeaders?: ProviderHeader[];
  modelsPath?: string;
  compatibilityJson?: string;
  capabilityOverridesJson?: string;
}

export interface CommonDescription {
  id: string;
  title: string;
  content: string;
  enabled: boolean;
  placement: "prefix" | "suffix";
  createdAt: string;
}

export interface GenerationPreset {
  id: string;
  name: string;
  description: string;
  providerId: string;
  model: string;
  mode: GenerationMode;
  aspectRatio: string;
  size: string;
  quality: string;
  outputFormat: string;
  promptTemplate: string;
  createdAt: string;
}

export interface ProjectSettings {
  useCommonDescriptions: boolean;
  saveMetadata: boolean;
  saveRawResponse: boolean;
  autoOpenFolder: boolean;
  namingPattern: string;
  defaultProviderId: string;
  defaultModel: string;
}

export interface Project {
  id: string;
  name: string;
  description: string;
  storagePath: string;
  createdAt: string;
  updatedAt: string;
  color: string;
  descriptions: CommonDescription[];
  presets: GenerationPreset[];
  settings: ProjectSettings;
}

export interface ReferenceAsset {
  id: string;
  name: string;
  url: string;
  mimeType: string;
  sourceType: ReferenceSourceType;
  fileId?: string;
  previewUrl?: string;
  role: "object" | "character" | "style" | "source" | "video";
  width?: number;
  height?: number;
}

export interface GenerationDraft {
  providerId: string;
  model: string;
  mode: GenerationMode;
  prompt: string;
  negativePrompt: string;
  aspectRatio: string;
  size: string;
  customWidth: number;
  customHeight: number;
  quality: string;
  count: number;
  outputFormat: string;
  responseFormat: string;
  responseModel: string;
  imageGenerationAction: string;
  useResponsesApi: boolean;
  moderation: string;
  style: string;
  background: string;
  compression: number;
  references: ReferenceAsset[];
  maskDataUrl?: string;
  seed: string;
  inputFidelity: string;
  thinkingLevel: string;
  webSearch: boolean;
  imageSearch: boolean;
  includeText: boolean;
  stream: boolean;
  backgroundTask: boolean;
  batch: boolean;
  partialImages: number;
  serviceTier: string;
  temperature: number;
  topP: number;
  storeInteraction: boolean;
  previousResponseId: string;
  useInteractionsApi: boolean;
  previousInteractionId: string;
  lastEventId: string;
  outputModalities: string[];
  remoteStore: boolean;
  storageFilename: string;
  persistRemoteFile: boolean;
  publicFileUrl: boolean;
  ttlSeconds: number;
  customJson: string;
}

export interface ProjectSummary {
  id: string;
  name: string;
  rootPath: string;
  createdAt: string;
  updatedAt: string;
  lastOpenedAt: string;
  defaultProviderProfileId?: string;
  defaultModelId?: string;
  defaultParameters: Record<string, unknown>;
}

export interface GeneratedAsset {
  id: string;
  taskId: string;
  url: string;
  filePath: string;
  width: number;
  height: number;
  format: string;
  prompt: string;
  createdAt: string;
  selected?: boolean;
}

export interface UsageSummary {
  inputTokens: number;
  outputTokens: number;
  thoughtTokens: number;
  cachedTokens: number;
  totalTokens: number;
  generatedImages: number;
  imageTokens?: number;
  costUsd?: number;
}

export type ResponsePart =
  | {
      id: string;
      type: "image";
      assetId: string;
      url: string;
      mimeType: string;
      width: number;
      height: number;
      filePath: string;
    }
  | {
      id: string;
      type: "text";
      text: string;
    }
  | {
      id: string;
      type: "thought";
      summary: string;
      imageUrl?: string;
    }
  | {
      id: string;
      type: "citation";
      title?: string;
      url?: string;
      snippet?: string;
      startIndex?: number;
      endIndex?: number;
    }
  | {
      id: string;
      type: "search_suggestions";
      html: string;
    }
  | {
      id: string;
      type: "remote_file";
      name: string;
      uri: string;
      mimeType: string;
      sizeBytes?: number;
    }
  | {
      id: string;
      type: "remote_job";
      jobId: string;
      kind: string;
      status: string;
      provider: string;
      model?: string;
    }
  | {
      id: string;
      type: "usage";
      usage: UsageSummary;
    }
  | {
      id: string;
      type: "request_meta";
      requestId: string;
      interactionId?: string;
      providerResponseId?: string;
    }
  | {
      id: string;
      type: "raw_response";
      json: string;
    };

export interface GenerationResult {
  runId?: string;
  requestId: string;
  interactionId?: string;
  failureReason?: string;
  assets: GeneratedAsset[];
  responseParts: ResponsePart[];
  usage: UsageSummary;
}

export interface GenerationTask {
  id: string;
  projectId: string;
  providerId: string;
  providerName: string;
  model: string;
  prompt: string;
  composedPrompt: string;
  mode: GenerationMode;
  status: TaskStatus;
  progress: number;
  count: number;
  createdAt: string;
  durationMs?: number;
  error?: string;
  assets: GeneratedAsset[];
  responseParts: ResponsePart[];
  partialImages?: Array<{ index: number; url: string }>;
  requestId?: string;
  interactionId?: string;
  usage?: UsageSummary;
}

export interface HistoryRecord extends GenerationTask {
  favorite: boolean;
  draftSnapshot?: Partial<GenerationDraft>;
  contextSnapshot?: CommonDescription[];
  presetSnapshot?: GenerationPreset;
  capabilityRegistryVersion?: string;
}

export interface GenerateRequest {
  clientTaskId: string;
  projectId: string;
  storagePath: string;
  provider: ProviderProfile;
  draft: GenerationDraft;
  composedPrompt: string;
  contextIds: string[];
  presetId?: string;
  contextSnapshot: CommonDescription[];
  presetSnapshot?: GenerationPreset;
}

export interface WorkspaceSnapshot {
  locale: Locale;
  activeProjectId: string;
  projects: Project[];
  providers: ProviderProfile[];
  history: HistoryRecord[];
}
