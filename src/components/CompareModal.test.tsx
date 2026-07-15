import { describe, it, expect } from "vitest";
import { render } from "@solidjs/testing-library";
import CompareModal from "./CompareModal";
import type { HistoryRecord } from "../types";

describe("CompareModal", () => {
  const mockT = (key: string) => key;

  const mockRecords: HistoryRecord[] = [
    {
      id: "1",
      projectId: "project-1",
      providerId: "provider-1",
      providerName: "OpenAI",
      model: "dall-e-3",
      prompt: "A sunset over mountains",
      composedPrompt: "A sunset over mountains",
      mode: "generate",
      status: "completed",
      progress: 100,
      count: 1,
      createdAt: "2024-01-01T00:00:00Z",
      assets: [
        {
          id: "asset-1",
          taskId: "1",
          url: "https://example.com/image1.png",
          filePath: "/path/to/image1.png",
          width: 1024,
          height: 1024,
          format: "png",
          prompt: "A sunset over mountains",
          createdAt: "2024-01-01T00:00:00Z",
        },
      ],
      responseParts: [],
      favorite: false,
      draftSnapshot: {
        size: "1024x1024",
        quality: "standard",
        style: "vivid",
      },
      usage: {
        inputTokens: 10,
        outputTokens: 0,
        thoughtTokens: 0,
        cachedTokens: 0,
        totalTokens: 10,
        generatedImages: 1,
        costUsd: 0.04,
      },
      durationMs: 5000,
    },
    {
      id: "2",
      projectId: "project-1",
      providerId: "provider-1",
      providerName: "OpenAI",
      model: "dall-e-2",
      prompt: "A forest path",
      composedPrompt: "A forest path",
      mode: "generate",
      status: "completed",
      progress: 100,
      count: 1,
      createdAt: "2024-01-02T00:00:00Z",
      assets: [],
      responseParts: [],
      favorite: true,
    },
  ];

  it("renders comparison view with multiple records", () => {
    const { container } = render(() => (
      <CompareModal records={mockRecords} t={mockT} onClose={() => {}} />
    ));

    const compareCards = container.querySelectorAll(".compare-card");
    expect(compareCards.length).toBe(2);
  });

  it("displays record details correctly", () => {
    const { getByText } = render(() => (
      <CompareModal records={[mockRecords[0]]} t={mockT} onClose={() => {}} />
    ));

    expect(getByText("A sunset over mountains")).toBeDefined();
    expect(getByText("OpenAI")).toBeDefined();
  });

  it("does not render when records array is empty", () => {
    const { container } = render(() => (
      <CompareModal records={[]} t={mockT} onClose={() => {}} />
    ));

    const modal = container.querySelector(".modal-overlay");
    expect(modal).toBeNull();
  });
});
