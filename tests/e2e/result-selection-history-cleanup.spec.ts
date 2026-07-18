import { expect, test } from "@playwright/test";

test("results support asset-level multi-selection and deletion", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto("/");
  await page.getByRole("button", { name: /All results|全部结果/ }).click();

  await page.locator(".results-header-actions button:has(svg.lucide-list-checks)").click();
  const checkboxes = page.getByRole("checkbox", { name: /Select result|选择结果/ });
  await expect(checkboxes).toHaveCount(3);
  await page.locator(".result-selection-check").first().click();
  await expect(page.locator(".result-gallery-card.is-selected")).toHaveCount(1);

  await page.getByRole("button", { name: /Delete selected|删除所选/ }).click();
  await page.getByRole("button", { name: /Delete images|确认删除/ }).click();
  await expect(page.locator(".result-gallery-card")).toHaveCount(2);
});

test("history removes controls that had no useful action", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto("/");
  await page.getByRole("button", { name: /History|历史记录/ }).click();

  await expect(page.locator(".filter-bar svg.lucide-list-filter")).toHaveCount(0);
  await expect(page.locator(".history-table .table-actions svg.lucide-heart")).toHaveCount(0);
});
