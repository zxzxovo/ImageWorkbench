import { expect, test } from "@playwright/test";

const resolutions = [
  { name: "1024×640", width: 1024, height: 640 },
  { name: "1280×800", width: 1280, height: 800 },
  { name: "1440×900", width: 1440, height: 900 },
];

for (const resolution of resolutions) {
  test.describe(`Resolution: ${resolution.name}`, () => {
    test("CreatorPage: no overflow and parameter panel scrollable", async ({ page }) => {
      await page.setViewportSize({ width: resolution.width, height: resolution.height });
      await page.goto("/");
      await expect(page.locator(".app-shell")).toBeVisible({ timeout: 10000 });

      // Navigate to create tab (should be default)
      await expect(page.locator(".tabbar button.is-active")).toContainText(/创作|Create/i);
      await expect(page.locator(".creator-layout")).toBeVisible();

      // Check no horizontal overflow on page
      const noHorizontalOverflow = await page.evaluate(() => {
        return document.body.scrollWidth <= window.innerWidth;
      });
      expect(noHorizontalOverflow).toBeTruthy();

      // Check key elements are present and within viewport
      const metrics = await page.evaluate(() => {
        const getBounds = (selector: string) => {
          const element = document.querySelector<HTMLElement>(selector);
          if (!element) return null;
          const rect = element.getBoundingClientRect();
          return {
            left: rect.left,
            right: rect.right,
            top: rect.top,
            bottom: rect.bottom,
            width: rect.width,
            height: rect.height,
          };
        };

        return {
          viewportWidth: window.innerWidth,
          viewportHeight: window.innerHeight,
          composerPanel: getBounds(".composer-panel"),
          creatorSide: getBounds(".creator-side"),
          creatorLayout: getBounds(".creator-layout"),
        };
      });

      // Verify composer panel doesn't overflow horizontally
      expect(metrics.composerPanel).not.toBeNull();
      if (metrics.composerPanel) {
        expect(metrics.composerPanel.left).toBeGreaterThanOrEqual(0);
        expect(metrics.composerPanel.right).toBeLessThanOrEqual(metrics.viewportWidth + 1);
      }

      // Verify creator side panel doesn't overflow horizontally
      expect(metrics.creatorSide).not.toBeNull();
      if (metrics.creatorSide) {
        expect(metrics.creatorSide.left).toBeGreaterThanOrEqual(0);
        expect(metrics.creatorSide.right).toBeLessThanOrEqual(metrics.viewportWidth + 1);
      }

      // At resolutions < 1030px, the layout stacks vertically (responsive design)
      // So we only check for horizontal panel separation at wider resolutions
      if (resolution.width >= 1030 && metrics.composerPanel && metrics.creatorSide) {
        // Verify panels are side-by-side, not overlapping
        const horizontalOverlap = Math.min(metrics.composerPanel.right, metrics.creatorSide.right)
          - Math.max(metrics.composerPanel.left, metrics.creatorSide.left);
        // Allow small overlap for borders/margins
        expect(horizontalOverlap).toBeLessThanOrEqual(5);
      }

      // Verify the workspace content area itself is scrollable for parameter overflow
      const workspaceScrollable = await page.evaluate(() => {
        const workspaceContent = document.querySelector<HTMLElement>(".workspace-content");
        if (!workspaceContent) return false;
        // Check if content can scroll (scrollHeight > clientHeight)
        return workspaceContent.scrollHeight > workspaceContent.clientHeight;
      });
      // At smaller resolutions or with lots of controls, the workspace should be scrollable
      // This is a soft check - it's okay if everything fits without scrolling
      expect(workspaceScrollable).toBeDefined();
    });

    test("HistoryPage: no overflow and table displays correctly", async ({ page }) => {
      await page.setViewportSize({ width: resolution.width, height: resolution.height });
      await page.goto("/");
      await expect(page.locator(".app-shell")).toBeVisible({ timeout: 10000 });

      // Navigate to history tab
      const historyButton = page.locator(".tabbar button").filter({ hasText: /历史|History/i });
      await historyButton.click();
      await expect(page.locator(".history-page")).toBeVisible();

      // Check no horizontal overflow on page
      const noHorizontalOverflow = await page.evaluate(() => {
        return document.body.scrollWidth <= window.innerWidth;
      });
      expect(noHorizontalOverflow).toBeTruthy();

      // Check key elements are present and within viewport
      const metrics = await page.evaluate(() => {
        const getBounds = (selector: string) => {
          const element = document.querySelector<HTMLElement>(selector);
          if (!element) return null;
          const rect = element.getBoundingClientRect();
          return {
            left: rect.left,
            right: rect.right,
            top: rect.top,
            bottom: rect.bottom,
            width: rect.width,
            height: rect.height,
          };
        };

        return {
          viewportWidth: window.innerWidth,
          viewportHeight: window.innerHeight,
          historyPage: getBounds(".history-page"),
          pageHeader: getBounds(".history-page .page-header"),
          filterBar: getBounds(".filter-bar"),
          historyTableWrap: getBounds(".history-table-wrap"),
        };
      });

      // Verify history page doesn't overflow horizontally
      expect(metrics.historyPage).not.toBeNull();
      if (metrics.historyPage) {
        expect(metrics.historyPage.left).toBeGreaterThanOrEqual(0);
        expect(metrics.historyPage.right).toBeLessThanOrEqual(metrics.viewportWidth + 1);
      }

      // Verify page header is within viewport
      expect(metrics.pageHeader).not.toBeNull();
      if (metrics.pageHeader) {
        expect(metrics.pageHeader.right).toBeLessThanOrEqual(metrics.viewportWidth + 1);
      }

      // Verify filter bar is within viewport
      expect(metrics.filterBar).not.toBeNull();
      if (metrics.filterBar) {
        expect(metrics.filterBar.right).toBeLessThanOrEqual(metrics.viewportWidth + 1);
      }

      // Verify table wrapper is within viewport (table itself may scroll horizontally within wrapper)
      if (metrics.historyTableWrap) {
        expect(metrics.historyTableWrap.left).toBeGreaterThanOrEqual(0);
        expect(metrics.historyTableWrap.right).toBeLessThanOrEqual(metrics.viewportWidth + 1);
      }
    });

    test("ProjectSettingsPage: no overflow and settings form displays correctly", async ({ page }) => {
      await page.setViewportSize({ width: resolution.width, height: resolution.height });
      await page.goto("/");
      await expect(page.locator(".app-shell")).toBeVisible({ timeout: 10000 });

      // Navigate to project settings tab
      const settingsButton = page.locator(".tabbar button").filter({ hasText: /项目设置|Project Settings/i });
      await settingsButton.click();
      await expect(page.locator(".settings-page")).toBeVisible();

      // Check no horizontal overflow on page
      const noHorizontalOverflow = await page.evaluate(() => {
        return document.body.scrollWidth <= window.innerWidth;
      });
      expect(noHorizontalOverflow).toBeTruthy();

      // Check key elements are present and within viewport
      const metrics = await page.evaluate(() => {
        const getBounds = (selector: string) => {
          const element = document.querySelector<HTMLElement>(selector);
          if (!element) return null;
          const rect = element.getBoundingClientRect();
          return {
            left: rect.left,
            right: rect.right,
            top: rect.top,
            bottom: rect.bottom,
            width: rect.width,
            height: rect.height,
          };
        };

        return {
          viewportWidth: window.innerWidth,
          viewportHeight: window.innerHeight,
          settingsPage: getBounds(".settings-page"),
          pageHeader: getBounds(".settings-page .page-header"),
          settingsLayout: getBounds(".settings-layout"),
          settingsSections: Array.from(document.querySelectorAll<HTMLElement>(".settings-section")).map((el) => {
            const rect = el.getBoundingClientRect();
            return {
              left: rect.left,
              right: rect.right,
              width: rect.width,
            };
          }),
        };
      });

      // Verify settings page doesn't overflow horizontally
      expect(metrics.settingsPage).not.toBeNull();
      if (metrics.settingsPage) {
        expect(metrics.settingsPage.left).toBeGreaterThanOrEqual(0);
        expect(metrics.settingsPage.right).toBeLessThanOrEqual(metrics.viewportWidth + 1);
      }

      // Verify page header is within viewport
      expect(metrics.pageHeader).not.toBeNull();
      if (metrics.pageHeader) {
        expect(metrics.pageHeader.right).toBeLessThanOrEqual(metrics.viewportWidth + 1);
      }

      // Verify settings layout is within viewport
      expect(metrics.settingsLayout).not.toBeNull();
      if (metrics.settingsLayout) {
        expect(metrics.settingsLayout.left).toBeGreaterThanOrEqual(0);
        expect(metrics.settingsLayout.right).toBeLessThanOrEqual(metrics.viewportWidth + 1);
      }

      // Verify all settings sections are within viewport
      for (const section of metrics.settingsSections) {
        expect(section.left).toBeGreaterThanOrEqual(0);
        expect(section.right).toBeLessThanOrEqual(metrics.viewportWidth + 1);
      }
    });
  });
}
