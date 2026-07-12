import { describe, expect, it } from "vitest";
import type { ProviderKind, ProviderProfile } from "../types";
import {
  getModelCapabilities,
  getModelsForProvider,
  parseCapabilityOverridesJson,
} from "./models";

function provider(
  kind: ProviderKind,
  models: string[],
  capabilityOverridesJson = "{}",
): ProviderProfile {
  return {
    id: `provider-${kind}`,
    name: kind,
    kind,
    baseUrl: "https://example.com/v1",
    apiKey: "",
    apiMode: kind === "custom" ? "openai-compatible" : "native",
    enabled: true,
    models,
    capabilityOverridesJson,
  };
}

describe("model capability resolution", () => {
  it("resolves common version aliases and prefixes without changing the requested model ID", () => {
    const profile = provider("openai", ["openai/gpt-image-1.5-2026-06-01"]);
    const capability = getModelCapabilities(profile, profile.models[0]);

    expect(capability.id).toBe("openai/gpt-image-1.5-2026-06-01");
    expect(capability.label).toBe("GPT Image 1.5");
    expect(capability.modes).toEqual(["generate", "edit", "mask"]);
    expect(capability.maxReferences).toBe(16);
  });

  it("uses conservative defaults for an unknown model without an override", () => {
    const profile = provider("custom", ["private-image-v9"]);
    const capability = getModelCapabilities(profile, "private-image-v9");

    expect(capability).toMatchObject({
      id: "private-image-v9",
      label: "private-image-v9",
      providerKind: "custom",
      modes: ["generate"],
      aspectRatios: ["auto"],
      sizes: ["auto"],
      outputFormats: ["png"],
      maxReferences: 0,
      maxCount: 1,
      supportsStreaming: false,
      supportsBatch: false,
      supportsCustomSize: false,
    });
  });

  it("maps a provider registry override into dynamic UI capabilities", () => {
    const profile = provider("custom", ["private-image-v9"], JSON.stringify({
      "private-image-v9": {
        display_name: "Private Image V9",
        api_surfaces: ["responses_tool", "interactions"],
        operations: ["generate", "edit", "variation", "conversation_continue", "video_reference_to_image"],
        max_input_images: 5,
        output_count: { min: 1, max: 7 },
        sizes: ["auto", "1024x1024"],
        resolutions: ["1K", "2K"],
        custom_size: {
          multiple_of: 8,
          min_aspect_ratio: 0.5,
          max_aspect_ratio: 2,
          max_edge: 2048,
          max_pixels: 3_000_000,
        },
        aspect_ratios: ["1:1", "16:9"],
        qualities: ["draft", "final"],
        formats: ["image/png", "image/jpeg"],
        response_formats: ["url", "base64_json"],
        backgrounds: ["auto", "transparent"],
        styles: ["natural"],
        moderation_modes: ["auto"],
        features: {
          streaming: true,
          background: true,
          batch: true,
          mask: true,
          file_outputs: true,
          interleaved_text: true,
          thinking: true,
          google_search: true,
          image_search: true,
          video_input: true,
        },
      },
    }));
    const capability = getModelCapabilities(profile, "private-image-v9");

    expect(capability.label).toBe("Private Image V9");
    expect(capability.modes).toEqual(["generate", "edit", "mask", "variation", "video"]);
    expect(capability.maxReferences).toBe(5);
    expect(capability.maxCount).toBe(7);
    expect(capability.sizes).toEqual(["auto", "1024x1024", "1K", "2K"]);
    expect(capability.customSizeRule).toEqual({
      multipleOf: 8,
      minAspectRatio: 0.5,
      maxAspectRatio: 2,
      maxEdge: 2048,
      maxPixels: 3_000_000,
    });
    expect(capability.aspectRatios).toEqual(["1:1", "16:9"]);
    expect(capability.qualityOptions).toEqual(["draft", "final"]);
    expect(capability.outputFormats).toEqual(["png", "jpeg"]);
    expect(capability.responseFormats).toEqual(["url", "b64_json"]);
    expect(capability.backgrounds).toEqual(["auto", "transparent"]);
    expect(capability).toMatchObject({
      supportsCustomSize: true,
      supportsStreaming: true,
      supportsBackground: true,
      supportsBatch: true,
      supportsMask: true,
      supportsRemoteFiles: true,
      supportsTextOutput: true,
      supportsOutputModalities: true,
      supportsThinking: true,
      supportsWebSearch: true,
      supportsImageSearch: true,
      supportsResponsesApi: true,
      supportsRemoteStore: true,
      supportsConversation: true,
    });
  });

  it("patches a known model without discarding its other known capabilities", () => {
    const profile = provider("openai", ["gpt-image-1.5"], JSON.stringify({
      "gpt-image-1.5": {
        display_name: "Gateway GPT Image",
        output_count: { min: 1, max: 2 },
        features: { streaming: false },
      },
    }));
    const capability = getModelCapabilities(profile, "gpt-image-1.5");

    expect(capability.label).toBe("Gateway GPT Image");
    expect(capability.maxCount).toBe(2);
    expect(capability.supportsStreaming).toBe(false);
    expect(capability.supportsMask).toBe(true);
    expect(getModelsForProvider(profile)).toEqual([{ id: "gpt-image-1.5", label: "Gateway GPT Image" }]);
  });
});

describe("capability override validation", () => {
  it("requires an object whose values are model patch objects", () => {
    expect(() => parseCapabilityOverridesJson("[]")).toThrow();
    expect(() => parseCapabilityOverridesJson('{"model": []}')).toThrow();
    expect(() => parseCapabilityOverridesJson('{"model": {"operations": ["generate"]}}')).not.toThrow();
  });
});
