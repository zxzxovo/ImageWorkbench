import { expect, test } from "@playwright/test";

test("project actions menu and theme toggle are usable", async ({ page }) => {
  await page.goto("/");

  await page.getByRole("button", { name: /Atlas Campaign - 项目操作/ }).click();
  await expect(page.getByText("修改项目信息", { exact: true })).toBeVisible();
  await expect(page.getByText("完整复制", { exact: true })).toBeVisible();
  await expect(page.getByText("仅复制配置", { exact: true })).toBeVisible();
  await expect(page.getByText("删除项目", { exact: true })).toBeVisible();

  await page.getByText("修改项目信息", { exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "修改项目信息" });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole("textbox").first()).toHaveValue("Atlas Campaign");
  await dialog.getByRole("button", { name: "Close" }).click();

  await page.getByRole("button", { name: "切换到深色模式" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await expect.poll(async () => page.evaluate(() => {
    const saved = localStorage.getItem("imageworkbench.workspace.v1");
    return saved ? JSON.parse(saved).theme : null;
  })).toBe("dark");
});
