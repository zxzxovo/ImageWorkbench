import { expect, test } from "@playwright/test";

test("provider templates open from Add instead of occupying the editor", async ({ page }) => {
  await page.setViewportSize({ width: 1120, height: 760 });
  await page.goto("/");
  await page.locator(".topbar button:has(svg.lucide-sliders-horizontal)").click();

  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog.locator(".provider-editor .template-grid")).toHaveCount(0);
  await dialog.locator(".pane-heading button:has(svg.lucide-plus)").click();
  await expect(page.locator(".provider-template-picker")).toBeVisible();
  await expect(page.locator(".provider-template-picker .template-button")).toHaveCount(4);
  await page.getByRole("button", { name: "Google Gemini" }).click();
  await expect(page.locator(".provider-template-picker[data-expanded]")).toHaveCount(0);
  await expect(page.locator(".provider-editor input").first()).toHaveValue("Google Gemini - New");
});

test("Help opens from the top bar and documents every section", async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 640 });
  await page.goto("/");
  await page.locator(".topbar button:has(svg.lucide-circle-help)").click();

  await expect(page.locator(".help-page")).toBeVisible();
  await expect(page.locator(".help-section")).toHaveCount(9);
  await expect(page.locator(".help-toc a")).toHaveCount(10);
  expect(await page.evaluate(() => document.body.scrollWidth)).toBeLessThanOrEqual(1024);

  await page.locator(".language-switch button", { hasText: "EN" }).click();
  await expect(page.getByRole("heading", { name: "Help and user guide" })).toBeVisible();
});

test("project setting toggles use one consistent row structure", async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 640 });
  await page.goto("/");
  await page.getByRole("button", { name: /Project settings|项目设置/ }).click();

  const rows = page.locator(".settings-toggle-list > .toggle-row");
  await expect(rows).toHaveCount(5);
  await expect(rows.last().locator(".toggle-row")).toHaveCount(0);
  await expect(rows.last().locator(".toggle-copy small")).toBeVisible();
});
