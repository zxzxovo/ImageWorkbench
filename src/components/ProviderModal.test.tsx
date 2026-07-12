import { fireEvent, render, screen, waitFor } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import type { ProviderProfile } from "../types";
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
});
