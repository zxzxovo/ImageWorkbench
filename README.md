# ImageWorkbench

ImageWorkbench is a local-first desktop workbench for generating and editing images with OpenAI, xAI Grok, Google Gemini, and OpenAI Images-compatible providers.

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
bun run test
bun run test:e2e
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
bun run tauri build --debug --no-bundle
```

Live provider tests are opt-in and require locally configured credentials. Normal tests use mock HTTP servers.
