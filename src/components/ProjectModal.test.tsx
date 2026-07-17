import { fireEvent, render, screen } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import type { ProviderProfile } from "../types";
import ProjectModal from "./ProjectModal";

const provider: ProviderProfile = {
  id: "provider-test",
  name: "Provider",
  kind: "openai",
  baseUrl: "https://api.openai.com/v1",
  apiKey: "",
  hasStoredSecret: false,
  apiMode: "native",
  enabled: true,
  models: ["gpt-image-1"],
  customHeaders: [],
  capabilityOverridesJson: "{}",
};

describe("ProjectModal", () => {
  it("validates and normalizes a manually entered ARGB color", async () => {
    const onCreate = vi.fn();
    render(() => (
      <ProjectModal
        open
        providers={[provider]}
        t={(key) => key}
        onClose={vi.fn()}
        onCreate={onCreate}
      />
    ));

    const colorValue = screen.getByLabelText("colorValue") as HTMLInputElement;
    const createButton = screen.getByRole("button", { name: "createProjectAction" });
    await fireEvent.input(screen.getAllByRole("textbox")[0], { target: { value: "Color project" } });
    await fireEvent.input(colorValue, { target: { value: "not-a-color" } });
    expect(screen.getByRole("alert").textContent).toBe("invalidColor");
    expect((createButton as HTMLButtonElement).disabled).toBe(true);

    await fireEvent.input(colorValue, { target: { value: "argb(128, 51, 102, 153)" } });
    expect(screen.queryByRole("alert")).toBeNull();
    expect((screen.getByLabelText("chooseColor") as HTMLInputElement).value).toBe("#336699");
    expect((createButton as HTMLButtonElement).disabled).toBe(false);

    await fireEvent.click(createButton);
    expect(onCreate).toHaveBeenCalledTimes(1);
    expect(onCreate.mock.calls[0][0].color).toBe("rgba(51, 102, 153, 0.502)");
  });

  it("keeps presets, picker, and text value synchronized", async () => {
    render(() => (
      <ProjectModal
        open
        providers={[provider]}
        t={(key) => key}
        onClose={vi.fn()}
        onCreate={vi.fn()}
      />
    ));

    await fireEvent.click(screen.getByRole("button", { name: "#4e6e9c" }));
    expect((screen.getByLabelText("colorValue") as HTMLInputElement).value).toBe("#4e6e9c");

    await fireEvent.input(screen.getByLabelText("chooseColor"), { target: { value: "#a55b43" } });
    expect((screen.getByLabelText("colorValue") as HTMLInputElement).value).toBe("#a55b43");
  });

  it("keeps the modal open and shows backend project creation failures", async () => {
    render(() => (
      <ProjectModal
        open
        providers={[provider]}
        t={(key) => key}
        onClose={vi.fn()}
        onCreate={vi.fn(async () => Promise.reject({ code: "storage", message: "project database is read-only" }))}
      />
    ));

    await fireEvent.input(screen.getAllByRole("textbox")[0], { target: { value: "Broken project" } });
    await fireEvent.click(screen.getByRole("button", { name: "createProjectAction" }));

    expect((await screen.findByRole("alert")).textContent).toContain("[storage] project database is read-only");
    expect(screen.getByRole("dialog")).not.toBeNull();
  });
});
