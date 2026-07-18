export type UpdateCheckResult = { version: string; install: () => Promise<void> } | null;

/** Check for a signed update and install only after an explicit user confirmation. */
export async function checkForUpdate(): Promise<UpdateCheckResult> {
  if (!('__TAURI_INTERNALS__' in window)) return null;
  try {
    const [{ check }, { relaunch }] = await Promise.all([
      import('@tauri-apps/plugin-updater'),
      import('@tauri-apps/plugin-process'),
    ]);
    const update = await check();
    if (!update) return null;
    return {
      version: update.version,
      install: async () => {
        await update.downloadAndInstall();
        await relaunch();
      },
    };
  } catch {
    // Update checks are best effort; a network outage must never block startup.
    return null;
  }
}
