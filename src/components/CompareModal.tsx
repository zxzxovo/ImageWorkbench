import { For, Show } from "solid-js";
import { Image as ImageIcon } from "lucide-solid";
import type { TranslationKey } from "../lib/i18n";
import { getModelLabel } from "../lib/models";
import type { HistoryRecord } from "../types";
import { Modal } from "./common";

interface CompareModalProps {
  records: HistoryRecord[];
  t: (key: TranslationKey) => string;
  onClose: () => void;
}

export default function CompareModal(props: CompareModalProps) {
  return (
    <Modal
      open={props.records.length > 0}
      title={props.t("compareRecords")}
      subtitle={props.t("compareSubtitle")}
      onClose={props.onClose}
      size="wide"
      footer={
        <button class="button secondary" type="button" onClick={props.onClose}>
          {props.t("close")}
        </button>
      }
    >
      <div class="compare-grid" style={{ "grid-template-columns": `repeat(${props.records.length}, 1fr)` }}>
        <For each={props.records}>
          {(record) => (
            <article class="compare-card">
              <div class="compare-preview">
                <Show when={record.assets[0]} fallback={<div class="compare-no-image"><ImageIcon size={32} /></div>}>
                  <img src={record.assets[0]?.url} alt="" />
                  <Show when={record.assets.length > 1}>
                    <span class="compare-badge">+{record.assets.length - 1}</span>
                  </Show>
                </Show>
              </div>

              <div class="compare-section">
                <h3>{props.t("prompt")}</h3>
                <p class="compare-prompt">{record.prompt}</p>
              </div>

              <div class="compare-section">
                <h3>{props.t("model")}</h3>
                <div class="compare-params">
                  <div>
                    <span>{props.t("provider")}</span>
                    <strong>{record.providerName}</strong>
                  </div>
                  <div>
                    <span>{props.t("model")}</span>
                    <strong>{getModelLabel(record.model)}</strong>
                  </div>
                  <div>
                    <span>{props.t("mode")}</span>
                    <strong>{record.mode}</strong>
                  </div>
                  <Show when={record.draftSnapshot?.size}>
                    <div>
                      <span>{props.t("size")}</span>
                      <strong>{record.draftSnapshot?.size}</strong>
                    </div>
                  </Show>
                  <Show when={record.draftSnapshot?.quality}>
                    <div>
                      <span>{props.t("quality")}</span>
                      <strong>{record.draftSnapshot?.quality}</strong>
                    </div>
                  </Show>
                  <Show when={record.draftSnapshot?.style}>
                    <div>
                      <span>{props.t("styleOption")}</span>
                      <strong>{record.draftSnapshot?.style}</strong>
                    </div>
                  </Show>
                </div>
              </div>

              <div class="compare-section">
                <h3>{props.t("results")}</h3>
                <div class="compare-stats">
                  <div>
                    <span>{props.t("statusLabel")}</span>
                    <strong class={`status-chip status-${record.status}`}>
                      {props.t((`status${record.status[0].toUpperCase()}${record.status.slice(1)}`) as TranslationKey)}
                    </strong>
                  </div>
                  <div>
                    <span>{props.t("imageCount")}</span>
                    <strong>{record.assets.length} {props.t("imageUnit")}</strong>
                  </div>
                  <Show when={record.usage}>
                    <div>
                      <span>{props.t("totalTokens")}</span>
                      <strong>{record.usage?.totalTokens ?? 0}</strong>
                    </div>
                    <Show when={record.usage?.costUsd !== undefined}>
                      <div>
                        <span>{props.t("cost")}</span>
                        <strong>${record.usage?.costUsd?.toFixed(4)}</strong>
                      </div>
                    </Show>
                  </Show>
                  <Show when={record.durationMs}>
                    <div>
                      <span>{props.t("duration")}</span>
                      <strong>{(record.durationMs! / 1000).toFixed(1)}s</strong>
                    </div>
                  </Show>
                  <div>
                    <span>{props.t("date")}</span>
                    <strong>{new Date(record.createdAt).toLocaleString()}</strong>
                  </div>
                </div>
              </div>
            </article>
          )}
        </For>
      </div>
    </Modal>
  );
}
