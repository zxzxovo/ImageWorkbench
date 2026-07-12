import { expect, test } from "@playwright/test";

const viewports = [
  { name: "minimum", width: 1024, height: 640 },
  { name: "standard", width: 1280, height: 800 },
  { name: "wide", width: 1440, height: 900 },
];

for (const viewport of viewports) {
  test(`${viewport.name} desktop layout has no page overflow or panel overlap`, async ({ page }) => {
    await page.setViewportSize({ width: viewport.width, height: viewport.height });
    await page.goto("/");
    await expect(page.locator(".app-shell")).toBeVisible();

    const metrics = await page.evaluate(() => {
      const rect = (selector: string) => {
        const element = document.querySelector<HTMLElement>(selector);
        if (!element) return null;
        const value = element.getBoundingClientRect();
        return { left: value.left, right: value.right, top: value.top, bottom: value.bottom };
      };
      return {
        viewportWidth: document.documentElement.clientWidth,
        pageWidth: document.documentElement.scrollWidth,
        sidebar: rect(".sidebar"),
        workspace: rect(".workspace-shell"),
        header: rect(".page-header"),
        layout: rect(".creator-layout"),
        composer: rect(".composer-panel"),
        inspector: rect(".creator-side"),
      };
    });

    expect(metrics.pageWidth).toBeLessThanOrEqual(metrics.viewportWidth + 1);
    expect(metrics.sidebar).not.toBeNull();
    expect(metrics.workspace).not.toBeNull();
    expect(metrics.header).not.toBeNull();
    expect(metrics.layout).not.toBeNull();
    expect(metrics.composer).not.toBeNull();
    expect(metrics.inspector).not.toBeNull();

    if (metrics.sidebar && metrics.workspace) {
      expect(metrics.sidebar.right).toBeLessThanOrEqual(metrics.workspace.left + 1);
    }
    if (metrics.header && metrics.layout) {
      expect(metrics.header.bottom).toBeLessThanOrEqual(metrics.layout.top + 1);
    }
    if (metrics.composer && metrics.inspector) {
      const horizontalOverlap = Math.min(metrics.composer.right, metrics.inspector.right)
        - Math.max(metrics.composer.left, metrics.inspector.left);
      const verticalOverlap = Math.min(metrics.composer.bottom, metrics.inspector.bottom)
        - Math.max(metrics.composer.top, metrics.inspector.top);
      expect(Math.min(horizontalOverlap, verticalOverlap)).toBeLessThanOrEqual(1);
    }
  });
}
