import { render, screen } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import type { GenerationTask, ResponsePart } from "../types";
import TaskDetailModal from "./TaskDetailModal";

function task(status: GenerationTask["status"], error?: string, responseParts: ResponsePart[] = []): GenerationTask {
  return {
    id: "run-1",
    projectId: "project-1",
    providerId: "provider-1",
    providerName: "OpenAI",
    model: "gpt-image-1",
    prompt: "Draw a lighthouse",
    composedPrompt: "Draw a lighthouse",
    mode: "generate",
    status,
    progress: status === "completed" ? 100 : 0,
    count: 1,
    createdAt: "2026-07-13T00:00:00.000Z",
    error,
    assets: [],
    responseParts,
  };
}

describe("TaskDetailModal", () => {
  it("shows the persisted failure reason", () => {
    render(() => (
      <TaskDetailModal
        task={task("failed", "Provider request timed out after 300 seconds")}
        t={(key) => key}
        onClose={vi.fn()}
      />
    ));

    expect(screen.getByText("failureReason")).toBeTruthy();
    expect(screen.getByText("Provider request timed out after 300 seconds")).toBeTruthy();
  });

  it("explicitly reports that a successful run has no failure", () => {
    render(() => (
      <TaskDetailModal task={task("completed")} t={(key) => key} onClose={vi.fn()} />
    ));

    expect(screen.getByText("noFailure")).toBeTruthy();
  });

  it("renders a raw_response part as a pre block", () => {
    const parts: ResponsePart[] = [{ id: "raw-1", type: "raw_response", json: '{"foo":"bar"}' }];
    render(() => (
      <TaskDetailModal task={task("completed", undefined, parts)} t={(key) => key} onClose={vi.fn()} />
    ));

    expect(screen.getByText('{"foo":"bar"}')).toBeTruthy();
  });
});
