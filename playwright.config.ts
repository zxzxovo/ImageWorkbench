import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/e2e",
  timeout: 30_000,
  fullyParallel: true,
  reporter: "line",
  use: {
    baseURL: "http://127.0.0.1:1422",
    browserName: "chromium",
    screenshot: "only-on-failure",
  },
  webServer: {
    command: "bun run dev -- --host 127.0.0.1 --port 1422",
    url: "http://127.0.0.1:1422",
    reuseExistingServer: true,
    timeout: 30_000,
  },
});
