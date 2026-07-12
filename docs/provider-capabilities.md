# Provider capability snapshot

Reviewed on 2026-07-12. Runtime model discovery is merged with the versioned JSON catalogs in `src-tauri/resources/capabilities`; model-list APIs are not treated as complete capability descriptions.

## OpenAI

- Image API: generation, edit, mask edit, reference-image composition, and DALL-E 2 variations.
- Responses API image tool: conversational generation/editing, file references, partial-image streaming, background responses, and Batch through `/v1/responses` when the selected model supports it.
- GPT Image options: size, quality, output format, compression, moderation, and model-dependent backgrounds.
- DALL-E options remain model-specific. DALL-E 3 accepts one output per request and supports `vivid`/`natural`; DALL-E 2 owns the variations endpoint.

Sources: [image guide](https://developers.openai.com/api/docs/guides/image-generation), [generation reference](https://developers.openai.com/api/reference/resources/images/methods/generate), [edit reference](https://developers.openai.com/api/reference/resources/images/methods/edit), [variation reference](https://developers.openai.com/api/reference/resources/images/methods/create_variation).

Known documentation mismatch: the guide documents `gpt-image-2` editing and arbitrary dimensions while some generated API-reference enums lag behind. The catalog follows the guide, forbids transparent backgrounds for `gpt-image-2`, and allows user overrides for newly discovered models.

## xAI

- `POST /v1/images/generations`: 1-10 outputs, fixed aspect-ratio enum, 1K/2K resolution, URL or Base64 response.
- `POST /v1/images/edits`: one to three ordered references using URL, Base64, or Files API IDs.
- Optional Files persistence, file/public-URL TTL, usage cost ticks, and provider Batch.
- No mask, incremental image streaming, arbitrary dimensions, or independent quality field. Quality is expressed through model selection.

Sources: [Imagine overview](https://docs.x.ai/developers/model-capabilities/imagine), [image REST reference](https://docs.x.ai/developers/rest-api-reference/inference/images), [model discovery](https://docs.x.ai/developers/rest-api-reference/inference/models).

## Google Gemini

- Native models: Gemini 3.1 Flash Image, Gemini 3.1 Flash Lite Image, Gemini 3 Pro Image, and Gemini 2.5 Flash Image.
- Interactions and generateContent surfaces support generation, editing, and conversational continuation.
- Model-dependent reference-image budgets, 1K/2K/4K or 512 output, interleaved text/images, thinking, Web/Image Search, and Gemini 3.1 Flash video-reference-to-image.
- Interactions supports streaming/background execution; provider Batch uses generateContent.
- Gemini has no reliable image-count field, so ImageWorkbench creates repeated jobs for multiple requested outputs.

Sources: [image generation](https://ai.google.dev/gemini-api/docs/image-generation), [Interactions API](https://ai.google.dev/api/interactions-api), [generateContent](https://ai.google.dev/api/generate-content), [Batch](https://ai.google.dev/gemini-api/docs/batch-api), [models](https://ai.google.dev/api/models).

Imagen is intentionally excluded because its documented shutdown date is 2026-08-17. Where Google pages disagree about Lite 512 output or PNG/JPEG enums, the stable catalog exposes the conservative option and permits an explicit user capability override.
