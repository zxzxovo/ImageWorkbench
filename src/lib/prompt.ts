import type { CommonDescription, GenerationDraft, ModelCapabilities } from "../types";

export function composePrompt(
  prompt: string,
  descriptions: CommonDescription[],
  enabled: boolean,
): string {
  const cleanPrompt = prompt.trim();
  if (!enabled) return cleanPrompt;

  const prefixes = descriptions
    .filter((item) => item.enabled)
    .map((item) => item.prefixContent.trim())
    .filter(Boolean);
  const suffixes = descriptions
    .filter((item) => item.enabled)
    .map((item) => item.suffixContent.trim())
    .filter(Boolean);

  return [...prefixes, cleanPrompt, ...suffixes].filter(Boolean).join("\n\n");
}

export function composeNegativePrompt(
  manualNegativePrompt: string,
  descriptions: CommonDescription[],
  enabled: boolean,
): string {
  const projectNegativeParts = enabled
    ? descriptions
      .filter((item) => item.enabled)
      .map((item) => item.negativeContent.trim())
      .filter(Boolean)
    : [];
  return [...projectNegativeParts, manualNegativePrompt.trim()].filter(Boolean).join("\n");
}

export type DraftValidationError = "prompt" | "reference" | "reference-limit" | "count" | "variation-input" | "custom-size" | "mask" | "reference-dimensions" | "generation-input";

export function isValidCustomSize(
  width: number,
  height: number,
  rule: ModelCapabilities["customSizeRule"] = {
    multipleOf: 16,
    minAspectRatio: 1 / 3,
    maxAspectRatio: 3,
    maxEdge: 3840,
    maxPixels: 8_294_400,
  },
): boolean {
  if (!Number.isInteger(width) || !Number.isInteger(height) || width <= 0 || height <= 0) return false;
  if (width % rule.multipleOf !== 0 || height % rule.multipleOf !== 0) return false;
  if (width > rule.maxEdge || height > rule.maxEdge || width * height > rule.maxPixels) return false;
  const ratio = width / height;
  return ratio >= rule.minAspectRatio && ratio <= rule.maxAspectRatio;
}

export function validateGenerationDraft(
  draft: GenerationDraft,
  capabilities: ModelCapabilities,
): DraftValidationError[] {
  const errors: DraftValidationError[] = [];
  if (draft.mode !== "variation" && !draft.prompt.trim()) errors.push("prompt");
  const continuesConversation = Boolean(draft.previousResponseId.trim() || draft.previousInteractionId.trim());
  if (draft.mode === "generate" && !continuesConversation && (draft.references.length > 0 || Boolean(draft.maskDataUrl?.trim()))) {
    errors.push("generation-input");
  }
  if (["edit", "mask", "variation", "video"].includes(draft.mode) && draft.references.length === 0) {
    errors.push("reference");
  }
  if (draft.mode === "variation" && draft.references.some((item) => item.mimeType !== "image/png" || (item.width !== undefined && item.height !== undefined && item.width !== item.height))) {
    errors.push("variation-input");
  }
  if (draft.mode === "mask" && !draft.maskDataUrl) errors.push("mask");
  if (draft.mode === "mask" && draft.references.length > 0 && (!draft.references[0].width || !draft.references[0].height)) errors.push("reference-dimensions");
  if (draft.references.length > capabilities.maxReferences) errors.push("reference-limit");
  if (!Number.isInteger(draft.count) || draft.count < 1 || draft.count > capabilities.maxCount) errors.push("count");
  if (draft.size === "custom" && (!capabilities.supportsCustomSize || !isValidCustomSize(draft.customWidth, draft.customHeight, capabilities.customSizeRule))) {
    errors.push("custom-size");
  }
  return errors;
}

export function normalizeDraftForModel(
  draft: GenerationDraft,
  capabilities: ModelCapabilities,
): GenerationDraft {
  const mode = capabilities.modes.includes(draft.mode) ? draft.mode : capabilities.modes[0];
  return {
    ...draft,
    mode,
    aspectRatio: capabilities.aspectRatios.includes(draft.aspectRatio)
      ? draft.aspectRatio
      : capabilities.aspectRatios[0] ?? "auto",
    size: capabilities.sizes.includes(draft.size) ? draft.size : capabilities.sizes[0] ?? "auto",
    quality: capabilities.qualityOptions.includes(draft.quality)
      ? draft.quality
      : capabilities.qualityOptions[0] ?? "auto",
    outputFormat: capabilities.outputFormats.includes(draft.outputFormat)
      ? draft.outputFormat
      : capabilities.outputFormats[0] ?? "auto",
    responseFormat: capabilities.responseFormats.includes(draft.responseFormat)
      ? draft.responseFormat
      : capabilities.responseFormats[0] ?? "b64_json",
    background: capabilities.backgrounds.includes(draft.background)
      ? draft.background
      : capabilities.backgrounds[0] ?? "auto",
    references: draft.references.slice(0, capabilities.maxReferences),
    thinkingLevel: capabilities.thinkingLevels.includes(draft.thinkingLevel)
      ? draft.thinkingLevel
      : capabilities.thinkingLevels[0] ?? "minimal",
    webSearch: capabilities.supportsWebSearch && draft.webSearch,
    imageSearch: capabilities.supportsImageSearch && draft.imageSearch,
    stream: capabilities.supportsStreaming && draft.stream,
    backgroundTask: capabilities.supportsBackground && draft.backgroundTask,
    batch: capabilities.supportsBatch && draft.batch,
    partialImages: capabilities.supportsPartialImages ? draft.partialImages : 0,
    previousResponseId: capabilities.supportsConversation ? draft.previousResponseId : "",
    useResponsesApi: capabilities.supportsResponsesApi && draft.useResponsesApi,
    moderation: capabilities.moderationOptions.includes(draft.moderation)
      ? draft.moderation
      : capabilities.moderationOptions[0] ?? "auto",
    style: capabilities.styleOptions.includes(draft.style)
      ? draft.style
      : capabilities.styleOptions[0] ?? "vivid",
    imageGenerationAction: capabilities.imageGenerationActions.includes(draft.imageGenerationAction)
      ? draft.imageGenerationAction
      : capabilities.imageGenerationActions[0] ?? "auto",
    useInteractionsApi: Boolean(capabilities.supportsConversation) && draft.useInteractionsApi,
    previousInteractionId: capabilities.supportsConversation ? draft.previousInteractionId : "",
    lastEventId: capabilities.supportsConversation ? draft.lastEventId : "",
    outputModalities: capabilities.supportsOutputModalities ? draft.outputModalities : ["image"],
    remoteStore: capabilities.supportsRemoteStore && draft.remoteStore,
    storageFilename: capabilities.supportsRemoteFiles ? draft.storageFilename : "",
    persistRemoteFile: capabilities.supportsRemoteFiles && draft.persistRemoteFile,
    publicFileUrl: capabilities.supportsRemoteFiles && draft.publicFileUrl,
    ttlSeconds: capabilities.supportsRemoteFiles ? draft.ttlSeconds : 3600,
    outputFilename: draft.outputFilename ?? "",
    flatOutput: draft.flatOutput ?? false,
  };
}
