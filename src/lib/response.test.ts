import { describe, expect, it } from "vitest";
import { formatBytes, sanitizeSearchSuggestionsHtml } from "./response";

describe("response rendering helpers", () => {
  it("removes executable markup from search suggestions", () => {
    const output = sanitizeSearchSuggestionsHtml('<div onclick="alert(1)"><script>alert(1)</script><a href="javascript:alert(2)">Unsafe</a><a href="https://example.com">Safe</a></div>');
    expect(output).not.toContain("<script");
    expect(output).not.toContain("onclick");
    expect(output).not.toContain("javascript:");
    expect(output).toContain("https://example.com");
  });

  it("formats remote file sizes", () => {
    expect(formatBytes(2840)).toBe("2.8 KB");
  });
});
