import { For, Show, createEffect, createSignal } from "solid-js";
import { Check, Clipboard, FolderOpen, RefreshCw, Trash2 } from "lucide-solid";
import { api, formatError, type DiagnosticReport } from "../lib/api";
import type { TranslationKey } from "../lib/i18n";
import { Modal } from "./common";

export interface AppErrorEntry {
  id: string;
  occurredAt: string;
  context: string;
  code: string;
  message: string;
  details?: unknown;
}

interface DiagnosticsModalProps {
  open: boolean;
  errors: AppErrorEntry[];
  t: (key: TranslationKey) => string;
  onClose: () => void;
  onClearErrors: () => void;
}

const SENSITIVE_FIELD = /(?:api.?key|authorization|password|secret|token|cookie)/i;

export function stringifyDiagnosticValue(value: unknown): string {
  if (value === undefined || value === null) return "";
  if (typeof value === "string") return value;
  const seen = new WeakSet<object>();
  try {
    return JSON.stringify(value, (key, item: unknown) => {
      if (key && SENSITIVE_FIELD.test(key)) return "[REDACTED]";
      if (item instanceof Error) {
        return { name: item.name, message: item.message };
      }
      if (item && typeof item === "object") {
        if (seen.has(item)) return "[Circular]";
        seen.add(item);
      }
      return item;
    }, 2);
  } catch {
    return String(value);
  }
}

export default function DiagnosticsModal(props: DiagnosticsModalProps) {
  const [report, setReport] = createSignal<DiagnosticReport>();
  const [loading, setLoading] = createSignal(false);
  const [localError, setLocalError] = createSignal("");
  const [copied, setCopied] = createSignal(false);

  const refresh = async () => {
    setLoading(true);
    setLocalError("");
    try {
      setReport(await api.diagnostics());
    } catch (error) {
      setLocalError(formatError(error));
    } finally {
      setLoading(false);
    }
  };

  createEffect(() => {
    if (props.open) void refresh();
  });

  const copyReport = async () => {
    try {
      await navigator.clipboard.writeText(stringifyDiagnosticValue({
        generatedAt: new Date().toISOString(),
        diagnostics: report(),
        frontendErrors: props.errors,
      }));
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1800);
    } catch (error) {
      setLocalError(formatError(error));
    }
  };

  const openLogs = async () => {
    try {
      await api.openDiagnosticsFolder();
    } catch (error) {
      setLocalError(formatError(error));
    }
  };

  return (
    <Modal
      open={props.open}
      title={props.t("diagnostics")}
      subtitle={props.t("diagnosticsSubtitle")}
      onClose={props.onClose}
      size="wide"
      footer={<button class="button secondary" type="button" onClick={props.onClose}>{props.t("close")}</button>}
    >
      <div class="diagnostics-shell">
        <div class="diagnostics-toolbar">
          <button class="button secondary" type="button" disabled={loading()} onClick={() => void refresh()}>
            <RefreshCw class={loading() ? "spin" : ""} size={15} />{props.t("refreshDiagnostics")}
          </button>
          <button class="button secondary" type="button" onClick={() => void copyReport()}>
            <Show when={copied()} fallback={<Clipboard size={15} />}><Check size={15} /></Show>
            {copied() ? props.t("diagnosticCopied") : props.t("copyDiagnosticReport")}
          </button>
          <button class="button secondary" type="button" onClick={() => void openLogs()}><FolderOpen size={15} />{props.t("openLogFolder")}</button>
        </div>

        <Show when={localError()}><p class="diagnostics-local-error" role="alert">{localError()}</p></Show>
        <Show when={!report() && loading()}><p class="diagnostics-loading"><RefreshCw class="spin" size={16} />{props.t("loadingDiagnostics")}</p></Show>

        <section class="diagnostics-section">
          <div class="diagnostics-section-heading"><h3>{props.t("recentErrors")}</h3><Show when={props.errors.length > 0}><button class="button ghost compact" type="button" onClick={props.onClearErrors}><Trash2 size={14} />{props.t("clear")}</button></Show></div>
          <Show when={props.errors.length > 0} fallback={<p class="diagnostics-empty">{props.t("noRecordedErrors")}</p>}>
            <div class="diagnostic-error-list">
              <For each={props.errors}>{(error) => (
                <article class="diagnostic-error-row">
                  <header><code>{error.code}</code><time dateTime={error.occurredAt}>{new Date(error.occurredAt).toLocaleString()}</time></header>
                  <strong>{error.message}</strong>
                  <small>{props.t("errorContext")}: {error.context}</small>
                  <Show when={stringifyDiagnosticValue(error.details)}>{(details) => <pre>{details()}</pre>}</Show>
                </article>
              )}</For>
            </div>
          </Show>
        </section>

        <Show when={report()}>{(current) => (
          <>
            <section class="diagnostics-section">
              <h3>{props.t("runtimeInfo")}</h3>
              <dl class="diagnostics-kv-grid">
                <div><dt>{props.t("appVersion")}</dt><dd>{current().appVersion}</dd></div>
                <div><dt>{props.t("platform")}</dt><dd>{current().os} / {current().architecture}</dd></div>
                <div class="span-2"><dt>{props.t("appDataDirectory")}</dt><dd><code>{current().appDataDirectory}</code></dd></div>
                <div class="span-2"><dt>{props.t("logDirectory")}</dt><dd><code>{current().logDirectory}</code></dd></div>
                <div class="span-2"><dt>{props.t("credentialStore")}</dt><dd><span class={`diagnostic-status status-${current().credentialStoreStatus}`}>{current().credentialStoreStatus === "ok" ? props.t("credentialStoreOk") : current().credentialStoreStatus === "error" ? props.t("credentialStoreError") : current().credentialStoreStatus}</span><small>{current().credentialStoreMessage}</small></dd></div>
              </dl>
            </section>

            <section class="diagnostics-section">
              <h3>{props.t("projectHealth")}</h3>
              <Show when={current().projects.length > 0} fallback={<p class="diagnostics-empty">{props.t("noDiagnosticProjects")}</p>}>
                <div class="diagnostic-project-list">
                  <For each={current().projects}>{(project) => (
                    <div class="diagnostic-project-row">
                      <span><strong>{project.name}</strong><code>{project.storagePath}</code></span>
                      <span class={`diagnostic-status ${project.databaseExists ? "status-ok" : "status-error"}`}>{project.databaseExists ? props.t("databaseAvailable") : props.t("databaseMissing")}</span>
                      <span class={`diagnostic-status ${project.isOpen ? "status-ok" : "status-error"}`}>{project.isOpen ? props.t("projectOpen") : props.t("projectClosed")}</span>
                    </div>
                  )}</For>
                </div>
              </Show>
            </section>

            <section class="diagnostics-section diagnostics-log-section">
              <h3>{props.t("backendLogs")}</h3>
              <Show when={current().logTail} fallback={<p class="diagnostics-empty">{props.t("noBackendLogs")}</p>}>
                <pre class="diagnostics-log-tail">{current().logTail}</pre>
              </Show>
            </section>
          </>
        )}</Show>
      </div>
    </Modal>
  );
}
