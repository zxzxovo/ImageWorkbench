import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { WorkspaceSnapshot } from "../types";
import { api } from "./api";

const snapshot: WorkspaceSnapshot = {
  locale: "en-US",
  activeProjectId: "project",
  projects: [],
  history: [],
  providers: [{
    id: "provider-secret-test",
    name: "Provider",
    kind: "custom",
    baseUrl: "https://example.com/v1",
    apiKey: "",
    hasStoredSecret: true,
    apiMode: "openai-compatible",
    enabled: true,
    models: ["image-model"],
  }],
};

const values = new Map<string, string>();
const memoryStorage: Storage = {
  get length() { return values.size; },
  clear: () => values.clear(),
  getItem: (key) => values.get(key) ?? null,
  key: (index) => [...values.keys()][index] ?? null,
  removeItem: (key) => { values.delete(key); },
  setItem: (key, value) => { values.set(key, value); },
};

beforeEach(() => {
  Object.defineProperty(window, "localStorage", { configurable: true, value: memoryStorage });
});

afterEach(() => memoryStorage.clear());

describe("browser workspace persistence", () => {
  it("keeps provider keys out of localStorage while preserving the session value", async () => {
    await api.storeProviderSecret("provider-secret-test", "plain-text-secret");
    await api.saveWorkspace(snapshot);

    const persisted = window.localStorage.getItem("imageworkbench.workspace.v1") ?? "";
    expect(persisted).not.toContain("plain-text-secret");
    expect(JSON.parse(persisted).providers[0].apiKey).toBe("");

    const restored = await api.loadWorkspace();
    expect(restored?.providers[0].apiKey).toBe("");
    expect(restored?.providers[0].hasStoredSecret).toBe(true);
    expect(await api.testProvider(restored!.providers[0])).toBe(true);
  });

  it("exports a generated preview through a browser download", async () => {
    const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);

    await expect(api.exportAsset("ignored", "result.png", "data:image/png;base64,AAAA")).resolves.toBe(true);

    expect(click).toHaveBeenCalledOnce();
    click.mockRestore();
  });
});
