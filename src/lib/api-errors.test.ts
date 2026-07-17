import { describe, expect, it } from "vitest";
import { formatError, normalizeCommonDescription, normalizeError } from "./api";

describe("API error normalization", () => {
  it("preserves structured Tauri error codes and details", () => {
    const error = { code: "keyring_unavailable", message: "Credential store failed", details: { osCode: 1312 } };
    expect(normalizeError(error)).toEqual(error);
    expect(formatError(error)).toBe("[keyring_unavailable] Credential store failed");
  });

  it("unwraps JSON and nested error payloads", () => {
    expect(normalizeError('{"code":"not_found","message":"project is not open"}')).toEqual({
      code: "not_found",
      message: "project is not open",
      details: undefined,
    });
    expect(formatError({ error: { code: "storage", message: "database is locked" } })).toBe("[storage] database is locked");
  });

  it("keeps plain Error messages readable", () => {
    expect(normalizeError(new Error("network failed"))).toMatchObject({ code: "error", message: "network failed" });
    expect(formatError(new Error("network failed"))).toBe("network failed");
  });
});

describe("normalizeCommonDescription", () => {
  it("maps legacy placement content without losing it", () => {
    expect(normalizeCommonDescription({
      id: "legacy",
      title: "Legacy",
      content: "legacy suffix",
      placement: "suffix",
      prefixContent: "",
      suffixContent: "",
      negativeContent: "",
      enabled: true,
      createdAt: "2026-01-01",
    })).toEqual({
      id: "legacy",
      title: "Legacy",
      prefixContent: "",
      suffixContent: "legacy suffix",
      negativeContent: "",
      enabled: true,
      createdAt: "2026-01-01",
    });
  });
});
