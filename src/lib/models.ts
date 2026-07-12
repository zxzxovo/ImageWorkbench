import type { GenerationMode, ModelCapabilities, ProviderKind, ProviderProfile } from "../types";

const commonRatios = ["auto", "1:1", "2:3", "3:2", "3:4", "4:3", "4:5", "5:4", "9:16", "16:9", "21:9"];
const wideRatios = ["auto", "1:1", "1:4", "1:8", "2:3", "3:2", "3:4", "4:1", "4:3", "4:5", "5:4", "8:1", "9:16", "16:9", "21:9"];
const xaiRatios = ["auto", "1:1", "16:9", "9:16", "4:3", "3:4", "3:2", "2:3", "2:1", "1:2", "19.5:9", "9:19.5", "20:9", "9:20"];

const capabilityDefaults = {
  responseFormats: [] as string[],
  maxCount: 16,
  supportsMask: false,
  supportsCustomSize: false,
  supportsInputFidelity: false,
  supportsThinking: false,
  thinkingLevels: [] as string[],
  supportsWebSearch: false,
  supportsImageSearch: false,
  supportsStreaming: false,
  supportsBackground: false,
  supportsBatch: false,
  supportsSeed: false,
  supportsPartialImages: false,
  supportsTextOutput: false,
  supportsServiceTier: false,
  supportsConversation: false,
  supportsResponsesApi: false,
  supportsRemoteFiles: false,
  supportsRemoteStore: false,
  supportsOutputModalities: false,
  moderationOptions: [] as string[],
  styleOptions: [] as string[],
  imageGenerationActions: [] as string[],
};

function defineModel(
  model: Omit<ModelCapabilities, keyof typeof capabilityDefaults> & Partial<typeof capabilityDefaults>,
): ModelCapabilities {
  return { ...capabilityDefaults, ...model };
}

const registry: Record<string, ModelCapabilities> = {
  "gpt-image-2": defineModel({
    id: "gpt-image-2",
    label: "GPT Image 2",
    providerKind: "openai",
    modes: ["generate", "edit", "mask"],
    aspectRatios: ["auto", "1:1", "3:2", "2:3"],
    sizes: ["auto", "1024x1024", "1536x1024", "1024x1536"],
    qualityOptions: ["auto", "low", "medium", "high"],
    outputFormats: ["png", "jpeg", "webp"],
    backgrounds: ["auto", "opaque"],
    maxReferences: 16,
    maxCount: 10,
    supportsMask: true,
    supportsCustomSize: true,
    customSizeRule: {
      multipleOf: 16,
      minAspectRatio: 1 / 3,
      maxAspectRatio: 3,
      maxEdge: 3840,
      maxPixels: 8_294_400,
    },
    supportsStreaming: true,
    supportsBatch: true,
    supportsResponsesApi: true,
    supportsConversation: true,
    moderationOptions: ["auto", "low"],
    imageGenerationActions: ["auto", "generate", "edit"],
  }),
  "gpt-image-1.5": defineModel({
    id: "gpt-image-1.5",
    label: "GPT Image 1.5",
    providerKind: "openai",
    modes: ["generate", "edit", "mask"],
    aspectRatios: ["auto", "1:1", "3:2", "2:3"],
    sizes: ["auto", "1024x1024", "1536x1024", "1024x1536"],
    qualityOptions: ["auto", "low", "medium", "high"],
    outputFormats: ["png", "jpeg", "webp"],
    backgrounds: ["auto", "opaque", "transparent"],
    maxReferences: 16,
    maxCount: 10,
    supportsMask: true,
    supportsInputFidelity: true,
    supportsStreaming: true,
    supportsBackground: true,
    supportsBatch: true,
    supportsPartialImages: true,
    supportsResponsesApi: true,
    supportsConversation: true,
    moderationOptions: ["auto", "low"],
    imageGenerationActions: ["auto", "generate", "edit"],
  }),
  "gpt-image-1": defineModel({
    id: "gpt-image-1",
    label: "GPT Image 1",
    providerKind: "openai",
    modes: ["generate", "edit", "mask"],
    aspectRatios: ["auto", "1:1", "3:2", "2:3"],
    sizes: ["auto", "1024x1024", "1536x1024", "1024x1536"],
    qualityOptions: ["auto", "low", "medium", "high"],
    outputFormats: ["png", "jpeg", "webp"],
    backgrounds: ["auto", "opaque", "transparent"],
    maxReferences: 16,
    maxCount: 10,
    supportsMask: true,
    supportsInputFidelity: true,
    supportsStreaming: true,
    supportsBackground: true,
    supportsBatch: true,
    supportsPartialImages: true,
    supportsResponsesApi: true,
    supportsConversation: true,
    moderationOptions: ["auto", "low"],
    imageGenerationActions: ["auto", "generate", "edit"],
  }),
  "gpt-image-1-mini": defineModel({
    id: "gpt-image-1-mini",
    label: "GPT Image 1 Mini",
    providerKind: "openai",
    modes: ["generate", "edit", "mask"],
    aspectRatios: ["auto", "1:1", "3:2", "2:3"],
    sizes: ["auto", "1024x1024", "1536x1024", "1024x1536"],
    qualityOptions: ["auto", "low", "medium", "high"],
    outputFormats: ["png", "jpeg", "webp"],
    backgrounds: ["auto", "opaque", "transparent"],
    maxReferences: 16,
    maxCount: 10,
    supportsMask: true,
    supportsInputFidelity: true,
    supportsStreaming: true,
    supportsBackground: true,
    supportsBatch: true,
    supportsPartialImages: true,
    supportsResponsesApi: true,
    supportsConversation: true,
    moderationOptions: ["auto", "low"],
    imageGenerationActions: ["auto", "generate", "edit"],
  }),
  "dall-e-2": defineModel({
    id: "dall-e-2",
    label: "DALL-E 2",
    providerKind: "openai",
    modes: ["generate", "edit", "mask", "variation"],
    aspectRatios: ["1:1"],
    sizes: ["256x256", "512x512", "1024x1024"],
    qualityOptions: ["standard"],
    outputFormats: ["png"],
    responseFormats: ["url", "b64_json"],
    backgrounds: ["opaque"],
    maxReferences: 1,
    maxCount: 10,
    supportsMask: true,
  }),
  "dall-e-3": defineModel({
    id: "dall-e-3",
    label: "DALL-E 3",
    providerKind: "openai",
    modes: ["generate"],
    aspectRatios: ["1:1", "16:9", "9:16"],
    sizes: ["1024x1024", "1792x1024", "1024x1792"],
    qualityOptions: ["standard", "hd"],
    outputFormats: ["png"],
    responseFormats: ["url", "b64_json"],
    backgrounds: ["opaque"],
    maxReferences: 0,
    maxCount: 1,
    styleOptions: ["vivid", "natural"],
  }),
  "grok-imagine-image": defineModel({
    id: "grok-imagine-image",
    label: "Grok Imagine Image",
    providerKind: "xai",
    modes: ["generate", "edit"],
    aspectRatios: xaiRatios,
    sizes: ["1k", "2k"],
    qualityOptions: [],
    outputFormats: ["png"],
    responseFormats: ["url", "b64_json"],
    backgrounds: ["auto"],
    maxReferences: 3,
    maxCount: 10,
    supportsBatch: true,
    supportsRemoteFiles: true,
  }),
  "grok-imagine-image-quality": defineModel({
    id: "grok-imagine-image-quality",
    label: "Grok Imagine Image Quality",
    providerKind: "xai",
    modes: ["generate", "edit"],
    aspectRatios: xaiRatios,
    sizes: ["1k", "2k"],
    qualityOptions: [],
    outputFormats: ["png"],
    responseFormats: ["url", "b64_json"],
    backgrounds: ["auto"],
    maxReferences: 3,
    maxCount: 10,
    supportsBatch: true,
    supportsRemoteFiles: true,
  }),
  "gemini-3.1-flash-image": defineModel({
    id: "gemini-3.1-flash-image",
    label: "Gemini 3.1 Flash Image",
    providerKind: "gemini",
    modes: ["generate", "edit", "video"],
    aspectRatios: wideRatios,
    sizes: ["1K", "2K", "4K", "512"],
    qualityOptions: [],
    outputFormats: ["png"],
    backgrounds: ["auto"],
    maxReferences: 14,
    supportsThinking: true,
    thinkingLevels: ["minimal", "high"],
    supportsWebSearch: true,
    supportsImageSearch: true,
    supportsStreaming: true,
    supportsBackground: true,
    supportsBatch: true,
    supportsTextOutput: true,
    supportsConversation: true,
    supportsRemoteStore: true,
    supportsOutputModalities: true,
  }),
  "gemini-3.1-flash-lite-image": defineModel({
    id: "gemini-3.1-flash-lite-image",
    label: "Gemini 3.1 Flash Lite Image",
    providerKind: "gemini",
    modes: ["generate", "edit"],
    aspectRatios: commonRatios,
    sizes: ["1K"],
    qualityOptions: [],
    outputFormats: ["png"],
    backgrounds: ["auto"],
    maxReferences: 14,
    supportsThinking: true,
    thinkingLevels: ["minimal", "high"],
    supportsStreaming: true,
    supportsBackground: true,
    supportsBatch: true,
    supportsTextOutput: true,
    supportsConversation: true,
    supportsRemoteStore: true,
    supportsOutputModalities: true,
  }),
  "gemini-3-pro-image": defineModel({
    id: "gemini-3-pro-image",
    label: "Gemini 3 Pro Image",
    providerKind: "gemini",
    modes: ["generate", "edit"],
    aspectRatios: commonRatios,
    sizes: ["1K", "2K", "4K"],
    qualityOptions: [],
    outputFormats: ["png"],
    backgrounds: ["auto"],
    maxReferences: 14,
    supportsThinking: true,
    thinkingLevels: ["high"],
    supportsWebSearch: true,
    supportsImageSearch: true,
    supportsStreaming: true,
    supportsBackground: true,
    supportsBatch: true,
    supportsTextOutput: true,
    supportsConversation: true,
    supportsRemoteStore: true,
    supportsOutputModalities: true,
  }),
  "gemini-2.5-flash-image": defineModel({
    id: "gemini-2.5-flash-image",
    label: "Gemini 2.5 Flash Image",
    providerKind: "gemini",
    modes: ["generate", "edit"],
    aspectRatios: commonRatios,
    sizes: ["1K"],
    qualityOptions: [],
    outputFormats: ["png"],
    backgrounds: ["auto"],
    maxReferences: 3,
    supportsStreaming: true,
    supportsBatch: true,
    supportsTextOutput: true,
    supportsConversation: true,
    supportsRemoteStore: true,
    supportsOutputModalities: true,
  }),
};

const aliasPatterns: Record<string, string[]> = {
  "gpt-image-2": ["gpt-image-2-*"],
  "gpt-image-1.5": ["gpt-image-1.5-*"],
  "gpt-image-1-mini": ["gpt-image-1-mini-*"],
  "gpt-image-1": ["gpt-image-1-*"],
  "dall-e-2": ["dall-e-2-*"],
  "dall-e-3": ["dall-e-3-*"],
  "grok-imagine-image-quality": ["grok-imagine-image-quality-*"],
  "grok-imagine-image": ["grok-imagine-image-*"],
  "gemini-3.1-flash-image": ["gemini-3.1-flash-image-preview", "gemini-3.1-flash-image-*"],
  "gemini-3.1-flash-lite-image": ["gemini-3.1-flash-lite-image-preview", "gemini-3.1-flash-lite-image-*"],
  "gemini-3-pro-image": ["gemini-3-pro-image-preview", "gemini-3-pro-image-*"],
  "gemini-2.5-flash-image": ["gemini-2.5-flash-image-preview", "gemini-2.5-flash-image-*"],
};

type CapabilityRegistryPatch = Record<string, unknown>;
type CapabilityOverrideMap = Record<string, CapabilityRegistryPatch>;

const defaultCustomSizeRule = {
  multipleOf: 16,
  minAspectRatio: 1 / 3,
  maxAspectRatio: 3,
  maxEdge: 3840,
  maxPixels: 8_294_400,
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

export function parseCapabilityOverridesJson(source: string | undefined): CapabilityOverrideMap {
  const parsed: unknown = JSON.parse(source?.trim() || "{}");
  if (!isRecord(parsed)) throw new Error("capability overrides must be an object");
  for (const [modelId, patch] of Object.entries(parsed)) {
    if (!modelId.trim() || !isRecord(patch)) {
      throw new Error("each capability override must map a model ID to an object");
    }
  }
  return parsed as CapabilityOverrideMap;
}

function modelBasename(modelId: string): string {
  return modelId.trim().toLowerCase().split("/").filter(Boolean).at(-1) ?? modelId.trim().toLowerCase();
}

function wildcardMatch(pattern: string, value: string): boolean {
  const expression = pattern
    .replace(/[.+?^${}()|[\]\\]/g, "\\$&")
    .replaceAll("*", ".*");
  return new RegExp(`^${expression}$`, "i").test(value);
}

function resolveKnownModelId(modelId: string): string | undefined {
  const candidate = modelBasename(modelId);
  if (registry[candidate]) return candidate;
  return Object.keys(aliasPatterns)
    .sort((left, right) => right.length - left.length)
    .find((canonical) => aliasPatterns[canonical].some((pattern) => wildcardMatch(pattern, candidate)));
}

function cloneCapabilities(base: ModelCapabilities, modelId: string, providerKind: ProviderKind): ModelCapabilities {
  return {
    ...base,
    id: modelId,
    providerKind,
    modes: [...base.modes],
    aspectRatios: [...base.aspectRatios],
    sizes: [...base.sizes],
    qualityOptions: [...base.qualityOptions],
    outputFormats: [...base.outputFormats],
    responseFormats: [...base.responseFormats],
    backgrounds: [...base.backgrounds],
    thinkingLevels: [...base.thinkingLevels],
    moderationOptions: [...base.moderationOptions],
    styleOptions: [...base.styleOptions],
    imageGenerationActions: [...base.imageGenerationActions],
    customSizeRule: base.customSizeRule ? { ...base.customSizeRule } : undefined,
  };
}

function conservativeCapabilities(providerKind: ProviderKind, modelId: string): ModelCapabilities {
  return defineModel({
    id: modelId,
    label: modelId || "Image model",
    providerKind,
    modes: ["generate"],
    aspectRatios: ["auto"],
    sizes: ["auto"],
    qualityOptions: [],
    outputFormats: ["png"],
    backgrounds: ["auto"],
    maxReferences: 0,
    maxCount: 1,
  });
}

function stringArray(value: unknown): string[] | undefined {
  if (!Array.isArray(value)) return undefined;
  return [...new Set(value.filter((item): item is string => typeof item === "string").map((item) => item.trim()).filter(Boolean))];
}

function finiteNumber(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function positiveNumber(value: unknown): number | undefined {
  const number = finiteNumber(value);
  return number !== undefined && number > 0 ? number : undefined;
}

function booleanField(source: Record<string, unknown>, key: string): boolean | undefined {
  return typeof source[key] === "boolean" ? source[key] as boolean : undefined;
}

function normalizeFormat(value: string): string {
  const format = value.toLowerCase().replace(/^image\//, "");
  return format === "jpg" ? "jpeg" : format;
}

function normalizeResponseFormat(value: string): string {
  return value.toLowerCase() === "base64_json" ? "b64_json" : value.toLowerCase();
}

function mapOperations(operations: string[], supportsMask: boolean): GenerationMode[] {
  const modes: GenerationMode[] = [];
  if (operations.includes("generate") || operations.includes("conversation_continue")) modes.push("generate");
  if (operations.includes("edit")) modes.push("edit");
  if (supportsMask && operations.includes("edit")) modes.push("mask");
  if (operations.includes("variation")) modes.push("variation");
  if (operations.includes("video_reference_to_image")) modes.push("video");
  return modes.length > 0 ? modes : ["generate"];
}

function overrideForModel(
  provider: ProviderProfile | undefined,
  modelId: string,
  canonicalId: string | undefined,
): CapabilityRegistryPatch | undefined {
  let overrides: CapabilityOverrideMap;
  try {
    overrides = parseCapabilityOverridesJson(provider?.capabilityOverridesJson);
  } catch {
    return undefined;
  }
  const candidates = [modelId, modelBasename(modelId), canonicalId].filter((item): item is string => Boolean(item));
  for (const candidate of candidates) {
    const exactKey = Object.keys(overrides).find((key) => key.toLowerCase() === candidate.toLowerCase());
    if (exactKey) return overrides[exactKey];
  }
  return undefined;
}

function applyRegistryPatch(
  base: ModelCapabilities,
  patch: CapabilityRegistryPatch | undefined,
): ModelCapabilities {
  if (!patch) return base;
  const next = cloneCapabilities(base, base.id, base.providerKind);
  if (typeof patch.display_name === "string" && patch.display_name.trim()) next.label = patch.display_name.trim();

  const features = isRecord(patch.features) ? patch.features : {};
  const supportsMask = booleanField(features, "mask") ?? booleanField(patch, "supports_mask") ?? next.supportsMask;
  const operations = stringArray(patch.operations)?.map((item) => item.toLowerCase());
  if (operations) next.modes = mapOperations(operations, supportsMask);
  next.supportsMask = supportsMask;
  if (!supportsMask) next.modes = next.modes.filter((mode) => mode !== "mask");

  const maxReferences = finiteNumber(patch.max_input_images) ?? finiteNumber(patch.max_reference_images);
  if (maxReferences !== undefined) next.maxReferences = Math.max(0, Math.floor(maxReferences));
  const outputCount = isRecord(patch.output_count) ? finiteNumber(patch.output_count.max) : finiteNumber(patch.max_outputs);
  if (outputCount !== undefined) next.maxCount = Math.max(1, Math.floor(outputCount));

  const declaredSizes = stringArray(patch.sizes);
  const resolutions = stringArray(patch.resolutions);
  if (declaredSizes || resolutions) {
    next.sizes = [...new Set([...(declaredSizes ?? []), ...(resolutions ?? [])])];
    if (next.sizes.length === 0) next.sizes = ["auto"];
  } else if (isRecord(patch.sizes)) {
    const presets = stringArray(patch.sizes.presets) ?? [];
    next.sizes = booleanField(patch.sizes, "supports_auto") ? ["auto", ...presets] : presets;
    if (next.sizes.length === 0) next.sizes = ["auto"];
  }

  if (Object.hasOwn(patch, "custom_size")) {
    const customSize = patch.custom_size;
    next.supportsCustomSize = customSize === true || isRecord(customSize);
    next.customSizeRule = next.supportsCustomSize ? {
      multipleOf: isRecord(customSize) ? positiveNumber(customSize.multiple_of) ?? positiveNumber(customSize.width_multiple_of) ?? next.customSizeRule?.multipleOf ?? defaultCustomSizeRule.multipleOf : defaultCustomSizeRule.multipleOf,
      minAspectRatio: isRecord(customSize) ? positiveNumber(customSize.min_aspect_ratio) ?? next.customSizeRule?.minAspectRatio ?? defaultCustomSizeRule.minAspectRatio : defaultCustomSizeRule.minAspectRatio,
      maxAspectRatio: isRecord(customSize) ? positiveNumber(customSize.max_aspect_ratio) ?? next.customSizeRule?.maxAspectRatio ?? defaultCustomSizeRule.maxAspectRatio : defaultCustomSizeRule.maxAspectRatio,
      maxEdge: isRecord(customSize) ? positiveNumber(customSize.max_edge) ?? positiveNumber(customSize.max_width) ?? next.customSizeRule?.maxEdge ?? defaultCustomSizeRule.maxEdge : defaultCustomSizeRule.maxEdge,
      maxPixels: isRecord(customSize) ? positiveNumber(customSize.max_pixels) ?? positiveNumber(customSize.max_area) ?? next.customSizeRule?.maxPixels ?? defaultCustomSizeRule.maxPixels : defaultCustomSizeRule.maxPixels,
    } : undefined;
  } else if (isRecord(patch.sizes) && booleanField(patch.sizes, "allow_custom") !== undefined) {
    next.supportsCustomSize = Boolean(patch.sizes.allow_custom);
  }

  const aspectRatios = stringArray(patch.aspect_ratios);
  if (aspectRatios) next.aspectRatios = aspectRatios.length > 0 ? aspectRatios : ["auto"];
  const qualities = stringArray(patch.qualities);
  if (qualities) next.qualityOptions = qualities;
  const formats = stringArray(patch.formats);
  if (formats?.length) next.outputFormats = formats.map(normalizeFormat);
  const responseFormats = stringArray(patch.response_formats);
  if (responseFormats) next.responseFormats = responseFormats.map(normalizeResponseFormat);
  const backgrounds = stringArray(patch.backgrounds);
  if (backgrounds?.length) next.backgrounds = backgrounds.map((item) => item.toLowerCase());
  const styles = stringArray(patch.styles);
  if (styles) next.styleOptions = styles;
  const moderationModes = stringArray(patch.moderation_modes);
  if (moderationModes) next.moderationOptions = moderationModes;

  const streaming = booleanField(features, "streaming") ?? booleanField(patch, "supports_streaming");
  if (streaming !== undefined) {
    next.supportsStreaming = streaming;
    next.supportsPartialImages = streaming && next.providerKind === "openai";
  }
  const background = booleanField(features, "background");
  if (background !== undefined) next.supportsBackground = background;
  const batch = booleanField(features, "batch");
  if (batch !== undefined) next.supportsBatch = batch;
  const thinking = booleanField(features, "thinking");
  if (thinking !== undefined) {
    next.supportsThinking = thinking;
    next.thinkingLevels = thinking ? stringArray(patch.thinking_levels) ?? ["minimal", "high"] : [];
  }
  const webSearch = booleanField(features, "google_search");
  if (webSearch !== undefined) next.supportsWebSearch = webSearch;
  const imageSearch = booleanField(features, "image_search");
  if (imageSearch !== undefined) next.supportsImageSearch = imageSearch;
  const interleavedText = booleanField(features, "interleaved_text");
  if (interleavedText !== undefined) {
    next.supportsTextOutput = interleavedText;
    next.supportsOutputModalities = interleavedText;
  }
  const fileOutputs = booleanField(features, "file_outputs");
  if (fileOutputs !== undefined) next.supportsRemoteFiles = fileOutputs;
  const videoInput = booleanField(features, "video_input");
  if (videoInput === true && !next.modes.includes("video")) next.modes.push("video");
  if (videoInput === false) next.modes = next.modes.filter((mode) => mode !== "video");

  const apiSurfaces = stringArray(patch.api_surfaces)?.map((item) => item.toLowerCase());
  if (apiSurfaces) {
    next.supportsResponsesApi = apiSurfaces.includes("responses_tool");
    if (next.supportsResponsesApi) next.imageGenerationActions = ["auto", "generate", "edit"];
    if (apiSurfaces.includes("interactions")) next.supportsRemoteStore = true;
  }
  if (operations?.includes("conversation_continue")) next.supportsConversation = true;
  return next;
}

export function getModelCapabilities(provider: ProviderProfile | undefined, modelId: string): ModelCapabilities {
  const providerKind = provider?.kind ?? "custom";
  const canonicalId = resolveKnownModelId(modelId);
  const known = canonicalId ? registry[canonicalId] : undefined;
  const base = known && (known.providerKind === providerKind || providerKind === "custom" || !provider)
    ? cloneCapabilities(known, modelId, providerKind)
    : conservativeCapabilities(providerKind, modelId);
  return applyRegistryPatch(base, overrideForModel(provider, modelId, canonicalId));
}

export function getModelLabel(modelId: string): string {
  const canonicalId = resolveKnownModelId(modelId);
  return canonicalId ? registry[canonicalId].label : modelId;
}

export function getModelsForProvider(provider: ProviderProfile | undefined): Array<{ id: string; label: string }> {
  if (!provider) return [];
  return provider.models.map((id) => ({ id, label: getModelCapabilities(provider, id).label }));
}

export function getProviderAccent(kind: ProviderKind): string {
  return {
    openai: "#1f7a61",
    xai: "#202327",
    gemini: "#3972c8",
    custom: "#a46225",
  }[kind];
}

export const providerDefaultModels: Record<ProviderKind, string[]> = {
  openai: ["gpt-image-2", "gpt-image-1.5", "gpt-image-1", "gpt-image-1-mini", "dall-e-2", "dall-e-3"],
  xai: ["grok-imagine-image", "grok-imagine-image-quality"],
  gemini: ["gemini-3.1-flash-image", "gemini-3-pro-image", "gemini-3.1-flash-lite-image", "gemini-2.5-flash-image"],
  custom: ["custom-image-model"],
};
