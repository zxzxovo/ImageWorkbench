import { fireEvent, render, screen } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import type { HistoryRecord, Project } from "../types";
import { ResultsPage } from "./ManagementPages";

const project: Project = {
  id: "project-1",
  name: "Project",
  description: "",
  storagePath: "C:/Project",
  color: "#336699",
  createdAt: "2026-07-17T00:00:00.000Z",
  updatedAt: "2026-07-17T00:00:00.000Z",
  descriptions: [],
  presets: [],
  settings: {
    useCommonDescriptions: false,
    saveMetadata: true,
    saveRawResponse: false,
    autoOpenFolder: false,
    namingPattern: "{date}_{model}_{index}",
    defaultProviderId: "provider-1",
    defaultModel: "image-model",
    flatOutput: false,
    defaultStream: null,
  },
};

const record: HistoryRecord = {
  id: "task-1",
  projectId: project.id,
  providerId: "provider-1",
  providerName: "Provider",
  model: "image-model",
  prompt: "A compact product scene",
  composedPrompt: "A compact product scene",
  mode: "generate",
  status: "completed",
  progress: 100,
  count: 1,
  createdAt: "2026-07-17T01:00:00.000Z",
  favorite: false,
  responseParts: [],
  assets: [{
    id: "asset-1",
    taskId: "task-1",
    url: "data:image/png;base64,iVBORw0KGgo=",
    filePath: "assets/output.png",
    width: 1024,
    height: 768,
    format: "png",
    prompt: "A compact product scene",
    createdAt: "2026-07-17T01:00:00.000Z",
  }],
};

describe("ResultsPage", () => {
  it("switches between bounded card and list views", async () => {
    const { container } = render(() => (
      <ResultsPage
        project={project}
        providers={[]}
        history={[record]}
        t={(key) => key}
        onReveal={vi.fn()}
        onOpenFolder={vi.fn()}
        onDownload={vi.fn()}
      />
    ));

    expect(container.querySelector(".results-grid-full")).not.toBeNull();
    expect(container.querySelector(".result-gallery-thumb img")).not.toBeNull();
    await fireEvent.click(screen.getByRole("button", { name: "listView" }));
    expect(container.querySelector(".results-grid-full")).toBeNull();
    expect(container.querySelector(".results-list-full")).not.toBeNull();
    expect(container.querySelector(".result-list-thumb img")).not.toBeNull();
  });
});
