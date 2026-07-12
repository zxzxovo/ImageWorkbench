import { fireEvent, render, screen } from "@solidjs/testing-library";
import { beforeEach, describe, expect, it, vi } from "vitest";
import MaskCanvas from "./MaskCanvas";

const translate = (key: string) => key;

beforeEach(() => {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({
    clearRect: vi.fn(),
  } as unknown as CanvasRenderingContext2D);
  vi.spyOn(HTMLCanvasElement.prototype, "toDataURL").mockReturnValue("data:image/png;base64,bWFzaw==");
});

describe("MaskCanvas", () => {
  it("keeps the source image dimensions and exposes working zoom controls", async () => {
    const onChange = vi.fn();
    const { container } = render(() => (
      <MaskCanvas
        t={translate as never}
        sourceUrl="data:image/png;base64,c291cmNl"
        sourceWidth={1536}
        sourceHeight={1024}
        onChange={onChange}
      />
    ));

    const canvas = container.querySelector("canvas");
    expect(canvas?.width).toBe(1536);
    expect(canvas?.height).toBe(1024);
    expect(container.querySelector<HTMLElement>(".mask-stage")?.style.aspectRatio).toBe("1536 / 1024");
    expect(onChange).toHaveBeenCalledWith("");

    await fireEvent.click(screen.getByRole("button", { name: "zoomIn" }));
    expect(container.querySelector(".mask-zoom-value")?.textContent).toBe("110%");

    await fireEvent.click(screen.getByRole("button", { name: "panTool" }));
    expect(screen.getByRole("button", { name: "panTool" }).classList.contains("is-active")).toBe(true);
  });
});
