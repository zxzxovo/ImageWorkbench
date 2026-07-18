import { render, screen } from "@solidjs/testing-library";
import { describe, expect, it } from "vitest";
import HelpPage from "./HelpPage";

describe("HelpPage", () => {
  it("documents the quick start and every application section in Chinese", () => {
    const { container } = render(() => <HelpPage locale="zh-CN" />);

    expect(screen.getByRole("heading", { name: "帮助与使用指南" })).toBeTruthy();
    expect(screen.getByRole("heading", { name: "快速开始" })).toBeTruthy();
    expect(container.querySelectorAll(".help-section")).toHaveLength(9);
    expect(screen.getByRole("heading", { name: "供应商管理" })).toBeTruthy();
    expect(screen.getByRole("heading", { name: "诊断与错误排查" })).toBeTruthy();
  });

  it("switches the guide copy with the application locale", () => {
    render(() => <HelpPage locale="en-US" />);

    expect(screen.getByRole("heading", { name: "Help and user guide" })).toBeTruthy();
    expect(screen.getByRole("heading", { name: "All results" })).toBeTruthy();
  });
});
