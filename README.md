# ImageWorkbench

ImageWorkbench is a local-first desktop workbench for generating and editing images with OpenAI, xAI Grok, Google Gemini, and OpenAI Images-compatible providers.

## Changelog

### v0.1.1
- **Fix** Error messages in the top bar now show real text instead of `[object Object]` — Tauri IPC errors are properly unwrapped
- **Fix** Provider test-connection and sync-models no longer fail instantly on first launch: the API key is passed directly instead of relying solely on a keyring round-trip
- **Fix** API key field now shows masked dots (`••••••••••••`) after saving, confirming the key was stored
- **Fix** History date filter button (Last 30 Days) now actually filters records; click again to reset
- **Fix** Sidebar collapse button is clearly visible after collapsing
- **Feat** Streaming API enabled by default for all models that support it, reducing timeout failures on slow generations
- **Cleanup** Removed redundant settings icon from top bar, `+` button from sidebar Projects header, and New / Open Project entries from the project dropdown

## Supported desktop platforms

The 0.2.x release line is supported on Windows 10/11 x86_64 and Linux x86_64
(Ubuntu 22.04/24.04, Debian 12/13, and current/previous Fedora releases) on
both Wayland and X11. Linux packages are published as AppImage, DEB, and RPM.
ARM64 builds are preview-only and are not covered by the release support
promise. macOS x86_64 and Apple Silicon builds remain supported.

Windows release artifacts are unsigned until a maintainer configures the
signing interface. Set `SIGNING_MODE=none|pfx|azure` in the release environment;
`none` is for internal/draft artifacts only, while public releases must use
`pfx` or `azure` and the corresponding Tauri signing secrets.

The updater checks a signed Tauri update manifest and always asks for user
confirmation before downloading and installing. AppImage updates are handled
in-app; DEB/RPM updates follow the system package manager.

Release automation treats updater signing as an explicit capability. When
`TAURI_SIGNING_PRIVATE_KEY` is configured, the workflow emits signed updater
artifacts and `latest.json`. When it is absent, the workflow still publishes
unsigned draft installers for internal testing and intentionally does not emit
`.sig` files; those drafts must not be promoted to a public auto-update
channel.

## Stack

- Rust 2024 and Tauri 2
- Bun, SolidJS, TypeScript, and Vite
- SQLite project databases and operating-system credential storage

## Development

```powershell
bun install --registry https://registry.npmmirror.com
bun run tauri dev
```

The browser-only UI can be previewed with `bun run dev`. It uses an in-memory demo backend when Tauri IPC is unavailable.

## Data model

Provider profiles and recent-project pointers live in the application data directory. API keys are stored in the system credential manager. Each portable project owns its records and assets:

```text
project-root/
  .imageworkbench/project.sqlite3
  assets/inputs/
  assets/outputs/YYYY-MM-DD/<run-id>/
  assets/previews/
```

Imported inputs are copied into the project. Provider responses, usage, request IDs, errors, and the exact effective prompt are recorded without storing credentials.

## Verification

```powershell
bun run build
bun run check:version
bun run test
bun run test:e2e
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
bun run tauri build
```

Live provider tests are opt-in and require locally configured credentials. Normal tests use mock HTTP servers.
