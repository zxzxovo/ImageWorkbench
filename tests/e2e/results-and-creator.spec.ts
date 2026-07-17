import { expect, test } from "@playwright/test";

test("creator keeps one prompt editor and removes redundant shell controls", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto("/");
  await expect(page.locator(".prompt-editor-panel")).toBeVisible();

  expect(await page.locator(".creator-layout textarea.prompt-input").count()).toBe(1);
  expect(await page.locator(".prompt-section").count()).toBe(0);
  expect(await page.locator(".provider-summary").count()).toBe(0);
  expect(await page.locator(".project-menu").count()).toBe(0);
  expect(await page.locator(".project-identity").evaluate((element) => element.tagName)).toBe("DIV");
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(1280);
});

for (const viewport of [{ width: 1024, height: 640 }, { width: 1280, height: 800 }]) {
  test(`results views constrain images at ${viewport.width}x${viewport.height}`, async ({ page }) => {
    await page.setViewportSize(viewport);
    await page.goto("/");
    await page.getByRole("button", { name: /All results|全部结果/ }).click();
    await expect(page.locator(".results-grid-full")).toBeVisible();

    const gridMetrics = await page.locator(".result-gallery-thumb").evaluateAll((elements) =>
      elements.map((element) => {
        const rect = element.getBoundingClientRect();
        return { height: rect.height, right: rect.right };
      }),
    );
    expect(gridMetrics.length).toBeGreaterThan(0);
    expect(gridMetrics.every((metric) => metric.height <= 220 && metric.right <= viewport.width + 1)).toBe(true);
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(viewport.width + 1);

    await page.getByRole("button", { name: /List view|列表视图/ }).click();
    await expect(page.locator(".results-list-full")).toBeVisible();
    expect(await page.locator(".result-gallery-card").count()).toBe(0);

    const listMetrics = await page.locator(".result-list-thumb").evaluateAll((elements) =>
      elements.map((element) => {
        const rect = element.getBoundingClientRect();
        return { width: rect.width, height: rect.height, right: rect.right };
      }),
    );
    expect(listMetrics.length).toBe(gridMetrics.length);
    expect(listMetrics.every((metric) => metric.width <= 76 && metric.height <= 76 && metric.right <= viewport.width + 1)).toBe(true);
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(viewport.width + 1);
  });
}
