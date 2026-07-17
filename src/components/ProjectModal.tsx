import { Show, createEffect, createMemo, createSignal } from "solid-js";
import { createStore } from "solid-js/store";
import { FolderOpen, LoaderCircle, Save } from "lucide-solid";
import { api, formatError } from "../lib/api";
import { parseProjectColor } from "../lib/color";
import type { TranslationKey } from "../lib/i18n";
import type { Project, ProviderProfile } from "../types";
import { Field, Modal } from "./common";

interface ProjectModalProps {
  open: boolean;
  providers: ProviderProfile[];
  t: (key: TranslationKey) => string;
  onClose: () => void;
  onCreate: (project: Project) => void | Promise<void>;
  onError?: (error: unknown, context: string) => void;
}

const colors = ["#2f7667", "#4e6e9c", "#a55b43", "#8a6a32", "#6b5b8d"];

export default function ProjectModal(props: ProjectModalProps) {
  const defaultStoragePath = api.isDemo ? "ImageWorkbench" : "";
  const [draft, setDraft] = createStore({
    name: "",
    description: "",
    storagePath: defaultStoragePath,
    color: colors[0],
  });
  const [colorInput, setColorInput] = createSignal(colors[0]);
  const [submitting, setSubmitting] = createSignal(false);
  const [formError, setFormError] = createSignal("");
  const parsedColor = createMemo(() => parseProjectColor(colorInput()));

  createEffect(() => {
    if (props.open) {
      setDraft({ name: "", description: "", storagePath: defaultStoragePath, color: colors[0] });
      setColorInput(colors[0]);
      setSubmitting(false);
      setFormError("");
    }
  });

  const updateColorInput = (value: string) => {
    setColorInput(value);
    const parsed = parseProjectColor(value);
    if (parsed) setDraft("color", parsed.css);
  };

  const selectColor = (value: string) => {
    setColorInput(value);
    setDraft("color", value);
  };

  const browse = async () => {
    setFormError("");
    try {
      const path = await api.chooseDirectory(draft.storagePath);
      if (path) setDraft("storagePath", path);
    } catch (error) {
      setFormError(formatError(error));
      props.onError?.(error, "project.choose_directory");
    }
  };

  const create = async () => {
    const color = parsedColor();
    if (!draft.name.trim() || !draft.storagePath.trim() || !color) return;
    const provider = props.providers.find((item) => item.enabled) ?? props.providers[0];
    const now = new Date().toISOString();
    setSubmitting(true);
    setFormError("");
    try {
      await props.onCreate({
        id: crypto.randomUUID(),
        name: draft.name.trim(),
        description: draft.description.trim(),
        storagePath: draft.storagePath.trim(),
        color: color.css,
        createdAt: now,
        updatedAt: now,
        descriptions: [],
        presets: [],
        settings: {
          useCommonDescriptions: false,
          saveMetadata: true,
          saveRawResponse: false,
          autoOpenFolder: false,
          namingPattern: "{date}_{model}_{index}",
          defaultProviderId: provider?.id ?? "",
          defaultModel: provider?.models[0] ?? "",
          flatOutput: false,
          defaultStream: null,
        },
      });
    } catch (error) {
      setFormError(formatError(error));
    } finally {
      setSubmitting(false);
    }
  };

  const footer = (
    <>
      <button class="button secondary" type="button" onClick={props.onClose}>{props.t("cancel")}</button>
      <button class="button primary" type="button" disabled={submitting() || !draft.name.trim() || !draft.storagePath.trim() || !parsedColor()} onClick={() => void create()}>
        <Show when={submitting()} fallback={<Save size={16} />}><LoaderCircle class="spin" size={16} /></Show>
        {props.t("createProjectAction")}
      </button>
    </>
  );

  return (
    <Modal
      open={props.open}
      title={props.t("createProjectTitle")}
      subtitle={props.t("createProjectSubtitle")}
      onClose={props.onClose}
      size="medium"
      footer={footer}
    >
      <div class="project-form">
        <Field label={props.t("projectName")} required>
          <input autofocus value={draft.name} onInput={(event) => setDraft("name", event.currentTarget.value)} />
        </Field>
        <Field label={props.t("projectDescription")}>
          <textarea rows="3" value={draft.description} onInput={(event) => setDraft("description", event.currentTarget.value)} />
        </Field>
        <Field label={props.t("storagePath")} required>
          <div class="input-action-group">
            <input value={draft.storagePath} spellcheck={false} onInput={(event) => setDraft("storagePath", event.currentTarget.value)} />
            <button type="button" class="button secondary icon-only" title={props.t("browse")} onClick={browse}><FolderOpen size={17} /></button>
          </div>
        </Field>
        <Field label={props.t("color")}>
          <div class="color-control-row">
            <div class="color-swatches">
              {colors.map((color) => (
                <button
                  type="button"
                  class={parsedColor()?.css === color ? "is-selected" : ""}
                  style={{ "background-color": color }}
                  aria-label={color}
                  onClick={() => selectColor(color)}
                />
              ))}
            </div>
            <input
              class="project-color-picker"
              type="color"
              value={parsedColor()?.picker ?? colors[0]}
              aria-label={props.t("chooseColor")}
              title={props.t("chooseColor")}
              onInput={(event) => selectColor(event.currentTarget.value)}
            />
          </div>
          <div class="color-value-row">
            <span class="color-value-preview" style={{ "background-color": parsedColor()?.css ?? "transparent" }} aria-hidden="true" />
            <input
              value={colorInput()}
              aria-label={props.t("colorValue")}
              aria-invalid={!parsedColor()}
              spellcheck={false}
              onInput={(event) => updateColorInput(event.currentTarget.value)}
              onBlur={() => {
                const parsed = parsedColor();
                if (parsed) setColorInput(parsed.css);
              }}
            />
          </div>
          <small class="field-hint color-format-hint">{props.t("colorFormatHint")}</small>
          <Show when={!parsedColor()}><span class="form-error" role="alert">{props.t("invalidColor")}</span></Show>
        </Field>
        <Show when={formError()}><p class="form-error" role="alert">{formError()}</p></Show>
      </div>
    </Modal>
  );
}
