import { render, screen } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import DiagnosticsModal, { stringifyDiagnosticValue } from "./DiagnosticsModal";

describe("DiagnosticsModal", () => {
  it("redacts sensitive diagnostic fields and handles circular details", () => {
    const details: Record<string, unknown> = { apiKey: "secret-value", nested: { token: "token-value" } };
    details.circular = details;

    const serialized = stringifyDiagnosticValue(details);

    expect(serialized).not.toContain("secret-value");
    expect(serialized).not.toContain("token-value");
    expect(serialized).toContain("[REDACTED]");
    expect(serialized).toContain("[Circular]");
  });

  it("shows runtime health and structured frontend errors", async () => {
    render(() => (
      <DiagnosticsModal
        open
        errors={[{
          id: "error-1",
          occurredAt: "2026-07-17T01:02:03.000Z",
          context: "provider.save",
          code: "keyring_unavailable",
          message: "Credential store failed",
          details: { osCode: 1312 },
        }]}
        t={(key) => key}
        onClose={vi.fn()}
        onClearErrors={vi.fn()}
      />
    ));

    expect((await screen.findByText("browser-demo")).textContent).toBe("browser-demo");
    expect(screen.getByText("keyring_unavailable").textContent).toBe("keyring_unavailable");
    expect(screen.getByText("Credential store failed").textContent).toBe("Credential store failed");
    expect(screen.getByText(/provider\.save/).textContent).toContain("provider.save");
  });
});
