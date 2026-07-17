import { For, Show, createEffect, createSignal } from "solid-js";
import { createStore } from "solid-js/store";
import { Bot, Boxes, Check, ChevronDown, Gem, KeyRound, Plus, RefreshCw, Save, Server, Trash2, Zap } from "lucide-solid";
import type { TranslationKey } from "../lib/i18n";
import { api, formatError } from "../lib/api";
import { getProviderAccent, parseCapabilityOverridesJson, providerDefaultModels } from "../lib/models";
import type { ProviderKind, ProviderProfile } from "../types";
import { Field, IconButton, Modal, StatusDot, Toggle } from "./common";

interface ProviderModalProps {
  open: boolean;
  providers: ProviderProfile[];
  t: (key: TranslationKey) => string;
  onClose: () => void;
  onUpsert: (provider: ProviderProfile) => void;
  onDelete: (providerId: string) => Promise<boolean>;
  onError?: (error: unknown, context: string) => void;
}

const templateData: Array<{
  kind: ProviderKind;
  name: string;
  baseUrl: string;
  apiMode: ProviderProfile["apiMode"];
  icon: typeof Bot;
}> = [
  { kind: "openai", name: "OpenAI", baseUrl: "https://api.openai.com/v1", apiMode: "native", icon: Bot },
  { kind: "xai", name: "xAI / Grok", baseUrl: "https://api.x.ai/v1", apiMode: "native", icon: Zap },
  { kind: "gemini", name: "Google Gemini", baseUrl: "https://generativelanguage.googleapis.com/v1beta", apiMode: "native", icon: Gem },
  { kind: "custom", name: "Custom", baseUrl: "https://api.example.com/v1", apiMode: "openai-compatible", icon: Boxes },
];

function emptyProvider(kind: ProviderKind): ProviderProfile {
  const template = templateData.find((item) => item.kind === kind) ?? templateData[3];
  return {
    id: crypto.randomUUID(),
    name: `${template.name} - New`,
    kind,
    baseUrl: template.baseUrl,
    apiKey: "",
    apiMode: template.apiMode,
    enabled: true,
    models: providerDefaultModels[kind],
    discoveredModels: [],
    apiVersion: kind === "gemini" ? "v1beta" : "",
    organization: "",
    projectId: "",
    customHeader: "",
    timeoutMs: 300_000,
    proxyUrl: "",
    authScheme: kind === "gemini" ? "header" : "bearer",
    authHeaderName: kind === "gemini" ? "x-goog-api-key" : "Authorization",
    authPrefix: kind === "gemini" ? "" : "Bearer",
    authQueryName: "key",
    customHeaders: [],
    modelsPath: "",
    compatibilityJson: "{}",
    capabilityOverridesJson: "{}",
  };
}

function normalizeProvider(provider: ProviderProfile): ProviderProfile {
  const defaults = emptyProvider(provider.kind);
  const discoveredModels = [...new Set(provider.discoveredModels ?? [])];
  return {
    ...defaults,
    ...provider,
    models: [...provider.models],
    discoveredModels,
    customHeaders: (provider.customHeaders ?? []).map((header) => ({
      ...header,
      id: header.id || crypto.randomUUID(),
      value: header.secret ? "" : header.value,
    })),
  };
}

function isSensitiveHeaderName(name: string): boolean {
  return /^(authorization|proxy-authorization|x-api-key|api-key|x-goog-api-key)$/i.test(name.trim());
}

export default function ProviderModal(props: ProviderModalProps) {
  const [selectedId, setSelectedId] = createSignal("");
  const [isNew, setIsNew] = createSignal(false);
  const [draft, setDraft] = createStore<ProviderProfile>(emptyProvider("openai"));
  const [testState, setTestState] = createSignal<"idle" | "testing" | "success" | "failure">("idle");
  const [syncing, setSyncing] = createSignal(false);
  const [advancedOpen, setAdvancedOpen] = createSignal(false);
  const [formError, setFormError] = createSignal("");
  // Track the raw API key the user has typed in this session so test/sync can
  // pass it directly to the backend without relying on a keyring round-trip.
  const [pendingApiKey, setPendingApiKey] = createSignal("");
  const modelOptions = () => [...new Set([
    ...providerDefaultModels[draft.kind],
    ...(draft.discoveredModels ?? []),
    ...draft.models,
  ])];

  const loadProvider = (provider: ProviderProfile) => {
    setIsNew(false);
    setSelectedId(provider.id);
    setDraft(normalizeProvider(provider));
    setTestState("idle");
    setAdvancedOpen(false);
    setFormError("");
    setPendingApiKey("");
  };

  let modalWasOpen = false;
  createEffect(() => {
    if (!props.open) {
      modalWasOpen = false;
      return;
    }
    if (modalWasOpen) return;
    modalWasOpen = true;
    const current = props.providers.find((item) => item.id === selectedId()) ?? props.providers[0];
    if (current) loadProvider(current);
    else createInstance("openai");
  });

  const createInstance = (kind: ProviderKind) => {
    const next = emptyProvider(kind);
    setIsNew(true);
    setSelectedId(next.id);
    setDraft(next);
    setTestState("idle");
    setAdvancedOpen(false);
    setFormError("");
    setPendingApiKey("");
  };

  const prepareDraft = async (): Promise<ProviderProfile> => {
    setFormError("");
    const compatibilityJson = draft.compatibilityJson?.trim() ?? "";
    if (compatibilityJson) {
      try {
        const parsed = JSON.parse(compatibilityJson);
        if (!parsed || Array.isArray(parsed) || typeof parsed !== "object") throw new Error("object required");
      } catch {
        setFormError(props.t("invalidJson"));
        throw new Error(props.t("invalidJson"));
      }
    }
    try {
      parseCapabilityOverridesJson(draft.capabilityOverridesJson);
    } catch {
      setFormError(props.t("invalidCapabilityOverrides"));
      throw new Error(props.t("invalidCapabilityOverrides"));
    }

    const apiKey = draft.apiKey.trim();
    if (apiKey) await api.storeProviderSecret(draft.id, apiKey);
    const customHeaders = await Promise.all((draft.customHeaders ?? []).map(async (header) => {
      const secret = header.secret || isSensitiveHeaderName(header.name);
      if (secret && header.value) await api.storeProviderHeaderSecret(draft.id, header.id, header.value);
      return {
        ...header,
        secret,
        value: secret ? "" : header.value,
        hasStoredValue: secret ? header.hasStoredValue || Boolean(header.value) : undefined,
      };
    }));
    const provider: ProviderProfile = {
      ...draft,
      apiKey: "",
      hasStoredSecret: draft.hasStoredSecret || Boolean(apiKey),
      customHeaders,
    };
    setDraft(provider);
    return provider;
  };

  const saveProvider = async () => {
    try {
      const provider = await prepareDraft();
      if (provider.enabled && provider.models.length === 0) {
        throw new Error(props.t("noModelsEnabled"));
      }
      props.onUpsert(provider);
      setSelectedId(provider.id);
      setIsNew(false);
    } catch (error) {
      setFormError(formatError(error));
      props.onError?.(error, "provider.save");
    }
  };

  const testConnection = async () => {
    setTestState("testing");
    setFormError("");
    try {
      const provider = await prepareDraft();
      // Pass the pending key directly so the backend doesn't need a keyring
      // round-trip, which can fail on some Windows configurations.
      const providerForTest = pendingApiKey() ? { ...provider, apiKey: pendingApiKey() } : provider;
      setTestState((await api.testProvider(providerForTest)) ? "success" : "failure");
    } catch (error) {
      setTestState("failure");
      setFormError(formatError(error));
      props.onError?.(error, "provider.test");
    }
  };

  const syncModels = async () => {
    setSyncing(true);
    setFormError("");
    try {
      const provider = await prepareDraft();
      const providerForSync = pendingApiKey() ? { ...provider, apiKey: pendingApiKey() } : provider;
      const models = await api.syncModels(providerForSync);
      setDraft("discoveredModels", [...new Set(models)]);
      setDraft("lastSyncedAt", new Date().toISOString());
    } catch (error) {
      setFormError(formatError(error));
      props.onError?.(error, "provider.sync_models");
    } finally {
      setSyncing(false);
    }
  };

  const updateHeader = (headerId: string, patch: Partial<NonNullable<ProviderProfile["customHeaders"]>[number]>) => {
    setDraft("customHeaders", (headers = []) => headers.map((header) => header.id === headerId ? { ...header, ...patch } : header));
  };

  const addHeader = () => {
    setDraft("customHeaders", (headers = []) => [...headers, {
      id: crypto.randomUUID(),
      name: "",
      value: "",
      secret: false,
    }]);
  };

  const footer = (
    <>
      <button class="button secondary" type="button" onClick={props.onClose}>{props.t("close")}</button>
      <button class="button primary" type="button" onClick={saveProvider}><Save size={16} />{props.t("save")}</button>
    </>
  );

  return (
    <Modal
      open={props.open}
      title={props.t("providerTitle")}
      subtitle={props.t("providerSubtitle")}
      onClose={props.onClose}
      size="wide"
      footer={footer}
    >
      <div class="provider-manager">
        <aside class="provider-list-pane">
          <div class="pane-heading">
            <span>{props.t("providers")}</span>
            <IconButton label={props.t("addProvider")} onClick={() => createInstance("openai")}><Plus size={16} /></IconButton>
          </div>
          <div class="provider-instance-list">
            <For each={props.providers}>
              {(provider) => (
                <button
                  type="button"
                  class={`provider-instance ${selectedId() === provider.id ? "is-selected" : ""}`}
                  onClick={() => loadProvider(provider)}
                >
                  <span class="provider-glyph" style={{ color: getProviderAccent(provider.kind) }}><Server size={17} /></span>
                  <span><strong>{provider.name}</strong><small>{provider.baseUrl.replace(/^https?:\/\//, "")}</small></span>
                  <StatusDot status={provider.enabled ? "online" : "idle"} />
                </button>
              )}
            </For>
          </div>
        </aside>

        <div class="provider-editor">
          <section class="provider-templates">
            <span class="section-kicker">{props.t("template")}</span>
            <div class="template-grid">
              <For each={templateData}>
                {(template) => {
                  const TemplateIcon = template.icon;
                  return (
                    <button
                      type="button"
                      class={`template-button ${draft.kind === template.kind ? "is-selected" : ""}`}
                      onClick={() => createInstance(template.kind)}
                    >
                      <span style={{ color: getProviderAccent(template.kind) }}><TemplateIcon size={19} /></span>
                      <strong>{template.name}</strong>
                    </button>
                  );
                }}
              </For>
            </div>
          </section>

          <div class="provider-form-grid">
            <Field label={props.t("providerName")} required>
              <input value={draft.name} onInput={(event) => setDraft("name", event.currentTarget.value)} />
            </Field>
            <Field label={props.t("apiMode")}>
              <select value={draft.apiMode} onChange={(event) => setDraft("apiMode", event.currentTarget.value as ProviderProfile["apiMode"])}>
                <option value="native">Native API</option>
                <option value="openai-compatible">OpenAI compatible</option>
              </select>
            </Field>
            <Field label={props.t("baseUrl")} required class="span-2">
              <input value={draft.baseUrl} spellcheck={false} onInput={(event) => setDraft("baseUrl", event.currentTarget.value)} />
            </Field>
            <Field label={props.t("apiKey")} required class="span-2">
              <input
                type="password"
                value={draft.hasStoredSecret && !draft.apiKey && !pendingApiKey() ? "••••••••••••" : draft.apiKey}
                autocomplete="off"
                spellcheck={false}
                placeholder={draft.hasStoredSecret ? props.t("credentialSaved") : ""}
                onFocus={(event) => {
                  if (draft.hasStoredSecret && !draft.apiKey && !pendingApiKey()) {
                    event.currentTarget.value = "";
                  }
                }}
                onInput={(event) => {
                  const value = event.currentTarget.value;
                  setDraft("apiKey", value);
                  setPendingApiKey(value);
                }}
              />
            </Field>
            <Show when={draft.kind === "gemini"}>
              <Field label={props.t("apiVersion")}>
                <select value={draft.apiVersion} onChange={(event) => setDraft("apiVersion", event.currentTarget.value)}>
                  <option value="v1beta">v1beta</option>
                  <option value="v1">v1</option>
                </select>
              </Field>
            </Show>
            <Show when={draft.kind === "openai"}>
              <Field label={props.t("organization")}>
                <input value={draft.organization ?? ""} onInput={(event) => setDraft("organization", event.currentTarget.value)} />
              </Field>
              <Field label={props.t("projectId")}>
                <input value={draft.projectId ?? ""} onInput={(event) => setDraft("projectId", event.currentTarget.value)} />
              </Field>
            </Show>
            <Show when={draft.kind === "custom"}>
              <Field label={props.t("customHeader")}>
                <input placeholder="X-API-Key" value={draft.customHeader ?? ""} onInput={(event) => setDraft("customHeader", event.currentTarget.value)} />
              </Field>
            </Show>
          </div>

          <section class={`provider-advanced ${advancedOpen() ? "is-open" : ""}`}>
            <button class="provider-advanced-toggle" type="button" onClick={() => setAdvancedOpen((value) => !value)}>
              <span><KeyRound size={15} />{props.t("advancedConnection")}</span>
              <ChevronDown size={16} />
            </button>
            <Show when={advancedOpen()}>
              <div class="provider-advanced-body">
                <div class="provider-form-grid">
                  <Field label={props.t("timeoutMs")}>
                    <input type="number" min="1000" step="1000" value={draft.timeoutMs ?? 300000} onInput={(event) => setDraft("timeoutMs", Number(event.currentTarget.value))} />
                  </Field>
                  <Field label={props.t("proxyUrl")}>
                    <input placeholder="http://127.0.0.1:7890" value={draft.proxyUrl ?? ""} spellcheck={false} onInput={(event) => setDraft("proxyUrl", event.currentTarget.value)} />
                  </Field>
                  <Field label={props.t("authScheme")}>
                    <select value={draft.authScheme ?? "bearer"} onChange={(event) => setDraft("authScheme", event.currentTarget.value as ProviderProfile["authScheme"])}>
                      <option value="bearer">{props.t("authBearer")}</option>
                      <option value="header">{props.t("authHeader")}</option>
                      <option value="query">{props.t("authQuery")}</option>
                    </select>
                  </Field>
                  <Show when={draft.authScheme === "header"}>
                    <Field label={props.t("authHeaderName")}>
                      <input value={draft.authHeaderName ?? ""} spellcheck={false} onInput={(event) => setDraft("authHeaderName", event.currentTarget.value)} />
                    </Field>
                    <Field label={props.t("authPrefix")}>
                      <input placeholder="Bearer" value={draft.authPrefix ?? ""} spellcheck={false} onInput={(event) => setDraft("authPrefix", event.currentTarget.value)} />
                    </Field>
                  </Show>
                  <Show when={draft.authScheme === "query"}>
                    <Field label={props.t("authQueryName")}>
                      <input placeholder="key" value={draft.authQueryName ?? ""} spellcheck={false} onInput={(event) => setDraft("authQueryName", event.currentTarget.value)} />
                    </Field>
                  </Show>
                  <Field label={props.t("modelsPath")}>
                    <input placeholder="/models" value={draft.modelsPath ?? ""} spellcheck={false} onInput={(event) => setDraft("modelsPath", event.currentTarget.value)} />
                  </Field>
                  <Field label={props.t("compatibilityJson")} class="span-2">
                    <textarea class="code-input" rows="3" value={draft.compatibilityJson ?? "{}"} spellcheck={false} onInput={(event) => setDraft("compatibilityJson", event.currentTarget.value)} />
                  </Field>
                  <Field label={props.t("capabilityOverrides")} class="span-2">
                    <small class="field-hint">{props.t("capabilityOverridesHint")}</small>
                    <textarea
                      class="code-input"
                      rows="6"
                      value={draft.capabilityOverridesJson ?? "{}"}
                      placeholder={'{\n  "my-image-model": {\n    "operations": ["generate", "edit"],\n    "resolutions": ["1K", "2K"]\n  }\n}'}
                      spellcheck={false}
                      onInput={(event) => setDraft("capabilityOverridesJson", event.currentTarget.value)}
                    />
                  </Field>
                </div>

                <div class="custom-header-editor">
                  <div class="section-row">
                    <div><span class="section-kicker">{props.t("customHeaders")}</span><small>{props.t("sensitiveHeaderHint")}</small></div>
                    <button class="button secondary compact" type="button" onClick={addHeader}><Plus size={14} />{props.t("addHeader")}</button>
                  </div>
                  <div class="custom-header-list">
                    <For each={draft.customHeaders ?? []}>
                      {(header) => {
                        const forcedSecret = () => isSensitiveHeaderName(header.name);
                        const secret = () => header.secret || forcedSecret();
                        return (
                          <div class="custom-header-row">
                            <input aria-label={props.t("headerName")} placeholder={props.t("headerName")} value={header.name} spellcheck={false} onInput={(event) => updateHeader(header.id, { name: event.currentTarget.value })} />
                            <input aria-label={props.t("headerValue")} type={secret() ? "password" : "text"} placeholder={header.hasStoredValue ? props.t("credentialSaved") : props.t("headerValue")} value={header.value} autocomplete="off" spellcheck={false} onInput={(event) => updateHeader(header.id, { value: event.currentTarget.value })} />
                            <label class="header-secret-check"><input type="checkbox" checked={secret()} disabled={forcedSecret()} onChange={(event) => updateHeader(header.id, { secret: event.currentTarget.checked })} /><span>{props.t("secretValue")}</span></label>
                            <IconButton label={props.t("delete")} onClick={() => setDraft("customHeaders", (headers = []) => headers.filter((item) => item.id !== header.id))}><Trash2 size={14} /></IconButton>
                          </div>
                        );
                      }}
                    </For>
                    <Show when={(draft.customHeaders?.length ?? 0) === 0}><p class="empty-inline">{props.t("noCustomHeaders")}</p></Show>
                  </div>
                </div>
              </div>
            </Show>
          </section>

          <Show when={formError()}><p class="form-error" role="alert">{formError()}</p></Show>
          <Toggle checked={draft.enabled} onChange={(value) => setDraft("enabled", value)} label={props.t("enabled")} />
          <div class="setting-row">
            <label class="setting-label">{props.t("defaultStream")}</label>
            <select
              class="setting-select"
              value={draft.defaultStream === null || draft.defaultStream === undefined ? "auto" : draft.defaultStream ? "on" : "off"}
              onChange={(event) => {
                const v = event.currentTarget.value;
                setDraft("defaultStream", v === "auto" ? null : v === "on");
              }}
            >
              <option value="auto">{props.t("streamAuto")}</option>
              <option value="on">{props.t("streamOn")}</option>
              <option value="off">{props.t("streamOff")}</option>
            </select>
          </div>

          <section class="model-sync-section">
            <div class="section-row">
              <div>
                <span class="section-kicker">{props.t("modelList")}</span>
                <Show when={draft.lastSyncedAt}><small>{props.t("syncedAt")} {new Date(draft.lastSyncedAt ?? "").toLocaleString()}</small></Show>
              </div>
              <div class="button-row">
                <button class="button secondary compact" type="button" disabled={testState() === "testing"} onClick={testConnection}>
                  <Show when={testState() === "success"} fallback={<Zap size={15} />}><Check size={15} /></Show>
                  {testState() === "success" ? props.t("connectionReady") : testState() === "failure" ? props.t("connectionFailed") : props.t("testConnection")}
                </button>
                <button class="button secondary compact" type="button" disabled={syncing()} onClick={syncModels}>
                  <RefreshCw size={15} class={syncing() ? "spin" : ""} />{props.t("syncModels")}
                </button>
              </div>
            </div>
            <p class="field-hint model-selection-hint">{props.t("modelSelectionHint")}</p>
            <div class="model-selection-list">
              <For each={modelOptions()}>{(model) => {
                const isPreset = () => providerDefaultModels[draft.kind].includes(model);
                const isDiscovered = () => (draft.discoveredModels ?? []).includes(model);
                return (
                  <label class={`model-selection-row ${draft.models.includes(model) ? "is-enabled" : ""}`}>
                    <input
                      type="checkbox"
                      checked={draft.models.includes(model)}
                      onChange={(event) => setDraft("models", (models) => event.currentTarget.checked
                        ? [...new Set([...models, model])]
                        : models.filter((item) => item !== model))}
                    />
                    <span>{model}</span>
                    <span class="model-origin">
                      <Show when={isPreset()}><small>{props.t("presetModel")}</small></Show>
                      <Show when={isDiscovered()}><small>{props.t("discoveredModel")}</small></Show>
                    </span>
                  </label>
                );
              }}</For>
            </div>
          </section>

          <Show when={!isNew()}>
            <button
              class="button danger-text compact provider-delete"
              type="button"
              onClick={async () => {
                if (!(await props.onDelete(draft.id))) return;
                const next = props.providers.find((item) => item.id !== draft.id);
                if (next) loadProvider(next);
              }}
            >
              <Trash2 size={15} />{props.t("delete")}
            </button>
          </Show>
        </div>
      </div>
    </Modal>
  );
}
