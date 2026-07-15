# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# Full desktop app (recommended during development)
npm run tauri dev        # start Tauri + Vite dev server
npm run tauri build      # production build

# Frontend only (no Rust backend, runs in demo/browser mode)
npm run dev              # Vite dev server at 127.0.0.1:1420

# Tests
npm test                 # vitest run (unit + component tests in src/**/*.test.{ts,tsx})
npm run test:watch       # vitest watch mode
npm run test:e2e         # Playwright end-to-end (requires running dev server on port 1422)

# Rust backend only
cd src-tauri && cargo test   # inline #[cfg(test)] modules
cd src-tauri && cargo build
```

Tauri's `beforeDevCommand`/`beforeBuildCommand` call `bun run dev`/`bun run build`, so **bun must be installed** for the full Tauri lifecycle. The `npm run` scripts work independently for frontend-only work.

## Architecture

**Tauri 2 desktop app: Rust backend + SolidJS frontend, with a type-safe IPC bridge.**

### Type-safe bridge (`bindings.ts`)

All Rust `#[tauri::command]` functions are registered through `tauri-specta`. On every debug build, specta auto-generates `src/bindings.ts` from the Rust types. **Never hand-edit `src/bindings.ts`** — changes belong in `src-tauri/src/commands.rs` and `bridge.rs`. The `bridge.rs` module contains the DTOs that cross the IPC boundary; `domain/types.rs` holds the pure Rust domain types kept separate from serialization concerns.

### Dual runtime mode

`src/lib/api.ts` detects `window.__TAURI_INTERNALS__` at runtime. In the Tauri desktop process it calls real Rust commands; in a plain browser it falls back to localStorage persistence and canvas-drawn placeholder images (`src/data/demo.ts`). This makes the entire UI runnable and testable without the Rust backend. Tests under `src/` always run in browser/demo mode (jsdom).

### Frontend state (`App.tsx`)

`App.tsx` is the single state hub: SolidJS signals + `createStore`. It manages projects, providers, generation tasks, and history. It persists a sanitized `WorkspaceSnapshot` on every state change, listens for `generation-event` Tauri events for live streaming progress, and polls remote async jobs every 5 s. The workspace snapshot deliberately excludes credentials and project payloads (enforced by a backend test).

### Backend structure

- **`app_state.rs`** — `AppState`: global SQLite store, per-project `ProjectStore` map, OS keyring handle, per-provider concurrency semaphores, cancellation tokens, pausable `QueueGate`.
- **`storage/`** — `GlobalStore` (app-level SQLite) + per-project SQLite files (each project lives in its own directory with a `.imageworkbench/project.sqlite3`; projects are portable/local-first). SQLx compile-time checked queries; migrations are embedded at compile time.
- **`providers/`** — `ProviderAdapter` trait implemented for OpenAI, xAI, Gemini, and a generic OpenAI-compatible adapter. Each adapter handles: connection test, model listing, generation (sync + streaming), batch submission, remote job polling/cancellation.
- **`runtime/executor.rs`** — drives the generation pipeline: prepare request → provider adapter → persist outputs/usage/errors → emit Tauri events.
- **`security/`** — API keys are stored in the OS keyring, never in the workspace snapshot. `redact_json` strips secrets before any persistence. Path traversal guards (`validated_project_asset_path`) use canonicalization + root-prefix checks on all asset reads/exports.

### Model capabilities registry

`src/types.ts` contains `CAPABILITY_REGISTRY` (versioned, currently `2026-07-12`) that drives all per-model UI options: generation modes (generate/edit/mask/variation/video), sizes, quality levels, thinking levels, streaming, batch, remote file storage, etc. When adding support for a new model or new provider feature, update the registry here and the corresponding provider adapter.
