import { describe, expect, it } from "vitest";
import type { CommonDescription, GenerationDraft, ModelCapabilities } from "../types";
import { composePrompt, isValidCustomSize, normalizeDraftForModel, validateGenerationDraft } from "./prompt";

const descriptions: CommonDescription[] = [
  { id: "1", title: "Era", content: "Post-millennial visual language", enabled: true, placement: "prefix", createdAt: "2026-01-01" },
  { id: "2", title: "Style", content: "Anime-inspired line work", enabled: true, placement: "suffix", createdAt: "2026-01-01" },
  { id: "3", title: "Off", content: "Do not include", enabled: false, placement: "suffix", createdAt: "2026-01-01" },
];

const capabilities: ModelCapabilities = {
  id: "test",
  label: "Test",
  providerKind: "custom",
  modes: ["generate"],
  aspectRatios: ["1:1"],
  sizes: ["1K"],
  qualityOptions: [],
  outputFormats: ["png"],
  responseFormats: [],
  backgrounds: ["auto"],
  maxReferences: 0,
  maxCount: 16,
  supportsMask: false,
  supportsCustomSize: false,
  supportsInputFidelity: false,
  supportsThinking: false,
  thinkingLevels: [],
  supportsWebSearch: false,
  supportsImageSearch: false,
  supportsStreaming: false,
  supportsBackground: false,
  supportsBatch: false,
  supportsSeed: false,
  supportsPartialImages: false,
  supportsTextOutput: false,
  supportsServiceTier: false,
  supportsResponsesApi: false,
  supportsRemoteFiles: false,
  supportsRemoteStore: false,
  supportsOutputModalities: false,
  moderationOptions: [],
  styleOptions: [],
  imageGenerationActions: [],
};

const draft: GenerationDraft = {
  providerId: "provider",
  model: "test",
  mode: "edit",
  prompt: "A quiet city at dawn",
  negativePrompt: "",
  aspectRatio: "16:9",
  size: "4K",
  customWidth: 1024,
  customHeight: 1024,
  quality: "high",
  count: 1,
  outputFormat: "jpeg",
  responseFormat: "b64_json",
  responseModel: "gpt-4.1",
  imageGenerationAction: "auto",
  useResponsesApi: false,
  moderation: "auto",
  style: "vivid",
  background: "transparent",
  compression: 90,
  references: [],
  seed: "",
  inputFidelity: "high",
  thinkingLevel: "high",
  webSearch: true,
  imageSearch: true,
  includeText: false,
  stream: true,
  backgroundTask: true,
  batch: true,
  partialImages: 2,
  serviceTier: "standard",
  temperature: 1,
  topP: 0.95,
  storeInteraction: true,
  previousResponseId: "interaction-1",
  useInteractionsApi: false,
  previousInteractionId: "",
  lastEventId: "",
  outputModalities: ["image"],
  remoteStore: false,
  storageFilename: "",
  persistRemoteFile: false,
  publicFileUrl: false,
  ttlSeconds: 3600,
  customJson: "{}",
  outputFilename: "",
  flatOutput: false,
};

describe("composePrompt", () => {
  it("merges enabled prefix and suffix descriptions in order", () => {
    expect(composePrompt("A portrait", descriptions, true)).toBe(
      "Post-millennial visual language\n\nA portrait\n\nAnime-inspired line work",
    );
  });

  it("returns only the user prompt when project context is disabled", () => {
    expect(composePrompt("  A portrait  ", descriptions, false)).toBe("A portrait");
  });
});

describe("draft helpers", () => {
  it("flags missing references for editing", () => {
    expect(validateGenerationDraft(draft, capabilities)).toContain("reference");
  });

  it("removes unsupported options when switching models", () => {
    const normalized = normalizeDraftForModel(draft, capabilities);
    expect(normalized.mode).toBe("generate");
    expect(normalized.aspectRatio).toBe("1:1");
    expect(normalized.webSearch).toBe(false);
    expect(normalized.batch).toBe(false);
  });

  it("allows a DALL-E 2 variation without a text prompt", () => {
    const variationCapabilities: ModelCapabilities = { ...capabilities, modes: ["variation"], maxReferences: 1 };
    const variationDraft: GenerationDraft = {
      ...draft,
      mode: "variation",
      prompt: "",
      references: [{
        id: "source",
        name: "source.png",
        url: "data:image/png;base64,AA==",
        mimeType: "image/png",
        sourceType: "base64",
        role: "source",
        width: 512,
        height: 512,
      }],
    };
    expect(validateGenerationDraft(variationDraft, variationCapabilities)).not.toContain("prompt");
  });

  it("uses model-specific custom size constraints", () => {
    expect(isValidCustomSize(1600, 800, {
      multipleOf: 8,
      minAspectRatio: 0.5,
      maxAspectRatio: 2,
      maxEdge: 2048,
      maxPixels: 3_000_000,
    })).toBe(true);
    expect(isValidCustomSize(1604, 800, {
      multipleOf: 8,
      minAspectRatio: 0.5,
      maxAspectRatio: 2,
      maxEdge: 2048,
      maxPixels: 3_000_000,
    })).toBe(false);
  });
});
