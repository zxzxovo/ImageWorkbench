import { For, Show } from "solid-js";
import {
  BrainCircuit,
  CloudCog,
  ExternalLink,
  FileJson2,
  FileText,
  Fingerprint,
  Image as ImageIcon,
  Link2,
  Search,
  Sigma,
} from "lucide-solid";
import type { TranslationKey } from "../lib/i18n";
import { formatBytes, sanitizeSearchSuggestionsHtml } from "../lib/response";
import type { GenerationTask, ResponsePart } from "../types";
import { Modal } from "./common";

interface TaskDetailModalProps {
  task: GenerationTask | null;
  t: (key: TranslationKey) => string;
  onClose: () => void;
}

function PartIcon(props: { part: ResponsePart }) {
  switch (props.part.type) {
    case "image": return <ImageIcon size={16} />;
    case "text": return <FileText size={16} />;
    case "thought": return <BrainCircuit size={16} />;
    case "citation": return <Link2 size={16} />;
    case "search_suggestions": return <Search size={16} />;
    case "remote_file": return <FileJson2 size={16} />;
    case "remote_job": return <CloudCog size={16} />;
    case "usage": return <Sigma size={16} />;
    case "request_meta": return <Fingerprint size={16} />;
  }
}

function partTitle(part: ResponsePart, t: (key: TranslationKey) => string): string {
  return {
    image: t("imageOutput"),
    text: t("textResponse"),
    thought: t("thinking"),
    citation: t("citations"),
    search_suggestions: t("searchSuggestions"),
    remote_file: t("remoteFile"),
    remote_job: t("remoteJob"),
    usage: t("usage"),
    request_meta: t("requestId"),
  }[part.type];
}

export default function TaskDetailModal(props: TaskDetailModalProps) {
  return (
    <Modal
      open={Boolean(props.task)}
      title={props.t("taskDetails")}
      subtitle={props.task ? `${props.task.providerName} · ${props.task.model}` : undefined}
      onClose={props.onClose}
      size="large"
      footer={<button class="button secondary" type="button" onClick={props.onClose}>{props.t("close")}</button>}
    >
      <Show when={props.task}>
        {(task) => (
          <div class="task-detail">
            <section class="task-detail-meta">
              <div><span>{props.t("requestId")}</span><code>{task().requestId ?? "-"}</code></div>
              <div><span>{props.t("interactionId")}</span><code>{task().interactionId ?? "-"}</code></div>
              <div><span>{props.t("statusLabel")}</span><strong class={`status-chip status-${task().status}`}>{task().status}</strong></div>
              <div><span>Date</span><strong>{new Date(task().createdAt).toLocaleString()}</strong></div>
            </section>

            <section class="task-detail-prompt">
              <span class="section-kicker">{props.t("prompt")}</span>
              <p>{task().composedPrompt}</p>
            </section>

            <section class="response-parts">
              <div class="response-parts-heading"><h3>{props.t("responseParts")}</h3><span>{task().responseParts.length}</span></div>
              <Show when={task().responseParts.length > 0} fallback={<p class="response-empty">{props.t("noResponseParts")}</p>}>
                <For each={task().responseParts}>
                  {(part) => (
                    <article class={`response-part response-${part.type}`}>
                      <header><span><PartIcon part={part} /></span><strong>{partTitle(part, props.t)}</strong></header>
                      <Show when={part.type === "image" && part}>
                        {(imagePart) => <div class="response-image"><img src={imagePart().url} alt="" /><div><code>{imagePart().mimeType}</code><span>{imagePart().width}×{imagePart().height}</span><small>{imagePart().filePath}</small></div></div>}
                      </Show>
                      <Show when={part.type === "text" && part}>{(textPart) => <p class="response-text">{textPart().text}</p>}</Show>
                      <Show when={part.type === "thought" && part}>{(thoughtPart) => <div class="thought-content"><p>{thoughtPart().summary}</p><Show when={thoughtPart().imageUrl}><img src={thoughtPart().imageUrl} alt="" /></Show></div>}</Show>
                      <Show when={part.type === "citation" && part}>{(citationPart) => <Show when={citationPart().url} fallback={<div class="citation-link citation-static"><span>{citationPart().title ?? props.t("citations")}</span><small>{citationPart().snippet ?? "-"}</small></div>}>{(url) => <a class="citation-link" href={url()} target="_blank" rel="noreferrer"><span>{citationPart().title ?? url()}</span><small>{citationPart().snippet ?? url()}</small><ExternalLink size={14} /></a>}</Show>}</Show>
                      <Show when={part.type === "search_suggestions" && part}>{(suggestionsPart) => <iframe class="suggestions-frame" title={props.t("searchSuggestions")} sandbox="" referrerpolicy="no-referrer" srcdoc={sanitizeSearchSuggestionsHtml(suggestionsPart().html)} />}</Show>
                      <Show when={part.type === "remote_file" && part}>{(filePart) => <div class="remote-file-row"><div><strong>{filePart().name}</strong><small>{filePart().mimeType} · {formatBytes(filePart().sizeBytes)}</small></div><code>{filePart().uri}</code></div>}</Show>
                      <Show when={part.type === "remote_job" && part}>{(jobPart) => <div class="remote-job-row"><div><span>{props.t("remoteJob")}</span><code>{jobPart().jobId}</code></div><div><span>Status</span><strong>{jobPart().status}</strong></div><div><span>{props.t("provider")}</span><strong>{jobPart().provider}</strong></div><div><span>{props.t("model")}</span><strong>{jobPart().model ?? "-"}</strong></div></div>}</Show>
                      <Show when={part.type === "usage" && part}>{(usagePart) => <div class="usage-grid"><div><span>{props.t("inputTokens")}</span><strong>{usagePart().usage.inputTokens}</strong></div><div><span>{props.t("outputTokens")}</span><strong>{usagePart().usage.outputTokens}</strong></div><div><span>{props.t("thoughtTokens")}</span><strong>{usagePart().usage.thoughtTokens}</strong></div><div><span>{props.t("cachedTokens")}</span><strong>{usagePart().usage.cachedTokens}</strong></div><div><span>{props.t("totalTokens")}</span><strong>{usagePart().usage.totalTokens}</strong></div><div><span>{props.t("imageCount")}</span><strong>{usagePart().usage.generatedImages}</strong></div><Show when={usagePart().usage.imageTokens !== undefined}><div><span>Image tokens</span><strong>{usagePart().usage.imageTokens}</strong></div></Show><Show when={usagePart().usage.costUsd !== undefined}><div><span>{props.t("cost")}</span><strong>${usagePart().usage.costUsd?.toFixed(4)}</strong></div></Show></div>}</Show>
                      <Show when={part.type === "request_meta" && part}>{(metaPart) => <div class="request-id-list"><div><span>{props.t("requestId")}</span><code>{metaPart().requestId}</code></div><Show when={metaPart().interactionId}><div><span>{props.t("interactionId")}</span><code>{metaPart().interactionId}</code></div></Show><Show when={metaPart().providerResponseId}><div><span>{props.t("providerResponseId")}</span><code>{metaPart().providerResponseId}</code></div></Show></div>}</Show>
                    </article>
                  )}
                </For>
              </Show>
            </section>
          </div>
        )}
      </Show>
    </Modal>
  );
}
