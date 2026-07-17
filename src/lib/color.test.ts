import { describe, expect, it } from "vitest";
import { parseProjectColor } from "./color";

describe("parseProjectColor", () => {
  it.each([
    ["#369", "#336699", 1],
    ["#336699", "#336699", 1],
    ["#3698", "rgba(51, 102, 153, 0.533)", 0.533],
    ["#33669980", "rgba(51, 102, 153, 0.502)", 0.502],
    ["rgb(51, 102, 153)", "#336699", 1],
    ["rgb(20% 40% 60% / 50%)", "rgba(51, 102, 153, 0.5)", 0.5],
    ["rgba(51, 102, 153, 0.25)", "rgba(51, 102, 153, 0.25)", 0.25],
    ["argb(128, 51, 102, 153)", "rgba(51, 102, 153, 0.502)", 0.502],
    ["0x80336699", "rgba(51, 102, 153, 0.502)", 0.502],
  ])("normalizes %s", (input, css, alpha) => {
    expect(parseProjectColor(input)).toEqual({ css, picker: "#336699", alpha });
  });

  it.each(["", "#12", "rgb(256, 0, 0)", "rgba(0, 0, 0, 2)", "argb(300, 0, 0, 0)", "blue"])(
    "rejects unsupported value %s",
    (input) => expect(parseProjectColor(input)).toBeNull(),
  );
});
