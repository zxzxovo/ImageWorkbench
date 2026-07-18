import { fireEvent, render, screen } from "@solidjs/testing-library";
import { createStore } from "solid-js/store";
import { afterEach, describe, expect, it, vi } from "vitest";
import { demoProjects, demoWorkspace, initialDraft } from "../data/demo";
import { api } from "../lib/api";
import type { GenerationDraft } from "../types";
import CreatorPage from "./CreatorPage";

describe("CreatorPage", () => {
  afterEach(() => {
    api.isDemo = true;
    vi.restoreAllMocks();
  });

  it("formats structured reference import errors and reports them to diagnostics", async () => {
    const error = { code: "io", message: "Cannot import this reference" };
    const wasDemo = api.isDemo;
    api.isDemo = false;
    const chooseReferences = vi.spyOn(api, "chooseReferenceFiles").mockRejectedValue(error);
    const onError = vi.fn();

    function Harness() {
      const [draft, setDraft] = createStore<GenerationDraft>({ ...initialDraft, references: [] });
      return (
        <CreatorPage
          project={demoProjects[0]}
          providers={demoWorkspace.providers}
          draft={draft}
          setDraft={setDraft}
          tasks={[]}
          history={[]}
          queuePaused={false}
          queueControlBusy={false}
          t={(key) => key}
          onPromptOverrideClear={vi.fn()}
          onGenerate={vi.fn(async () => undefined)}
          onCancelTask={vi.fn()}
          onToggleQueue={vi.fn()}
          onManageProviders={vi.fn()}
          onError={onError}
          onReveal={vi.fn()}
          onDownload={vi.fn()}
        />
      );
    }

    render(() => <Harness />);
    await fireEvent.click(screen.getByRole("button", { name: "addLocalReference" }));

    expect((await screen.findByText("[io] Cannot import this reference")).textContent).not.toContain("[object Object]");
    expect(onError).toHaveBeenCalledWith(error, "reference.import");
    chooseReferences.mockRestore();
    api.isDemo = wasDemo;
  });

  it("shows and enforces the generate-mode reference warning", async () => {
    const onGenerate = vi.fn(async () => undefined);

    function Harness() {
      const [draft, setDraft] = createStore<GenerationDraft>({
        ...initialDraft,
        mode: "generate",
        references: [{
          id: "reference",
          name: "reference.png",
          url: "assets/inputs/reference.png",
          mimeType: "image/png",
          sourceType: "local",
          role: "source",
        }],
      });
      return (
        <CreatorPage
          project={demoProjects[0]}
          providers={demoWorkspace.providers}
          draft={draft}
          setDraft={setDraft}
          tasks={[]}
          history={[]}
          queuePaused={false}
          queueControlBusy={false}
          t={(key) => key}
          onPromptOverrideClear={vi.fn()}
          onGenerate={onGenerate}
          onCancelTask={vi.fn()}
          onToggleQueue={vi.fn()}
          onManageProviders={vi.fn()}
          onError={vi.fn()}
          onReveal={vi.fn()}
          onDownload={vi.fn()}
        />
      );
    }

    const { container } = render(() => <Harness />);
    expect(container.querySelectorAll(".creator-step")).toHaveLength(3);
    expect(screen.getAllByRole("button", { name: "generate" })).toHaveLength(1);
    expect(screen.getByRole("alert").textContent).toContain("generationReferenceValidation");

    await fireEvent.click(screen.getByRole("button", { name: "generate" }));
    expect(onGenerate).not.toHaveBeenCalled();

    await fireEvent.click(screen.getByRole("button", { name: "switchToEditMode" }));
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
