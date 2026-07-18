import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import type { ProviderProfile } from "../types";
import { api } from "../lib/api";
import ProviderModal from "./ProviderModal";

const provider: ProviderProfile = {
  id: "provider-test",
  name: "Custom gateway",
  kind: "custom",
  baseUrl: "https://example.com/v1",
  apiKey: "",
  hasStoredSecret: true,
  apiMode: "openai-compatible",
  enabled: true,
  models: ["private-image-model"],
  discoveredModels: [],
  customHeaders: [],
  capabilityOverridesJson: "{}",
};

describe("ProviderModal", () => {
  it("validates capability overrides before saving a provider", async () => {
    const onUpsert = vi.fn();
    const { container } = render(() => (
      <ProviderModal
        open
        providers={[provider]}
        t={(key) => key}
        onClose={vi.fn()}
        onUpsert={onUpsert}
        onDelete={vi.fn(async () => true)}
      />
    ));

    await fireEvent.click(screen.getByRole("button", { name: /advancedConnection/ }));
    expect(container.querySelector<HTMLInputElement>('input[type="number"][step="1000"]')?.value).toBe("300000");
    const editor = container.querySelector<HTMLTextAreaElement>("textarea[placeholder*='my-image-model']");
    expect(editor).not.toBeNull();

    await fireEvent.input(editor!, { target: { value: "[]" } });
    await fireEvent.click(screen.getByRole("button", { name: "save" }));
    expect((await screen.findByRole("alert")).textContent).toContain("invalidCapabilityOverrides");
    expect(onUpsert).not.toHaveBeenCalled();

    await fireEvent.input(editor!, {
      target: { value: '{"private-image-model":{"operations":["generate","edit"]}}' },
    });
    await fireEvent.click(screen.getByRole("button", { name: "save" }));
    await waitFor(() => expect(onUpsert).toHaveBeenCalledTimes(1));
  });

  it("keeps discovered models disabled until the user enables them", async () => {
    const onUpsert = vi.fn();
    const sync = vi.spyOn(api, "syncModels").mockResolvedValue(["private-image-model", "new-image-model"]);
    render(() => (
      <ProviderModal
        open
        providers={[provider]}
        t={(key) => key}
        onClose={vi.fn()}
        onUpsert={onUpsert}
        onDelete={vi.fn(async () => true)}
      />
    ));

    await fireEvent.click(screen.getByRole("button", { name: "syncModels" }));
    const newModel = await screen.findByRole("checkbox", { name: /new-image-model/ });
    expect((newModel as HTMLInputElement).checked).toBe(false);

    await fireEvent.click(newModel);
    await fireEvent.click(screen.getByRole("button", { name: "save" }));
    await waitFor(() => expect(onUpsert).toHaveBeenCalled());
    expect(onUpsert.mock.calls[0][0].models).toContain("new-image-model");
    expect(onUpsert.mock.calls[0][0].discoveredModels).toEqual(["private-image-model", "new-image-model"]);
    sync.mockRestore();
  });

  it("shows provider templates only after Add is selected", async () => {
    const { container } = render(() => (
      <ProviderModal
        open
        providers={[provider]}
        t={(key) => key}
        onClose={vi.fn()}
        onUpsert={vi.fn()}
        onDelete={vi.fn(async () => true)}
      />
    ));

    expect(container.querySelector(".provider-editor .template-grid")).toBeNull();
    expect(document.querySelector(".provider-template-picker")).toBeNull();

    await fireEvent.click(screen.getByRole("button", { name: "addProvider" }));
    expect(document.querySelectorAll(".provider-template-picker .template-button")).toHaveLength(4);

    await fireEvent.click(screen.getByRole("button", { name: "Google Gemini" }));
    await waitFor(() => expect(document.querySelector(".provider-template-picker[data-expanded]")).toBeNull());
    expect(screen.getByDisplayValue("Google Gemini - New")).toBeTruthy();
  });
});
