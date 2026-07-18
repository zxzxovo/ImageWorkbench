import { Show, createEffect, createMemo, createSignal } from "solid-js";
import { createStore } from "solid-js/store";
import { AlertTriangle, Copy, FolderOpen, LoaderCircle, Save, Trash2 } from "lucide-solid";
import { api, formatError } from "../lib/api";
import { parseProjectColor } from "../lib/color";
import type { TranslationKey } from "../lib/i18n";
import type { Project } from "../types";
import { Field, Modal, Toggle } from "./common";

export type ProjectAction = "edit" | "copy-full" | "copy-configuration" | "move" | "delete";

interface ProjectActionModalProps {
  project?: Project;
  action?: ProjectAction;
  t: (key: TranslationKey) => string;
  onClose: () => void;
  onEdit: (project: Project) => Promise<void>;
  onCopy: (project: Project, name: string, path: string, mode: "full" | "configuration") => Promise<void>;
  onMove?: (project: Project, path: string) => Promise<void>;
  onDelete: (project: Project, deleteFiles: boolean) => Promise<void>;
  onError: (error: unknown, context: string) => void;
}

const colors = ["#2f7667", "#4e6e9c", "#a55b43", "#8a6a32", "#6b5b8d"];

export default function ProjectActionModal(props: ProjectActionModalProps) {
  const [draft, setDraft] = createStore({ name: "", description: "", color: colors[0], path: "" });
  const [colorInput, setColorInput] = createSignal(colors[0]);
  const [deleteFiles, setDeleteFiles] = createSignal(false);
  const [submitting, setSubmitting] = createSignal(false);
  const [formError, setFormError] = createSignal("");
  const parsedColor = createMemo(() => parseProjectColor(colorInput()));
  const isCopy = createMemo(() => props.action === "copy-full" || props.action === "copy-configuration");

  createEffect(() => {
    const project = props.project;
    if (!project || !props.action) return;
    setDraft({
      name: props.action === "edit" ? project.name : `${project.name} ${props.t("copyNameSuffix")}`,
      description: project.description,
      color: project.color,
      path: "",
    });
    setColorInput(project.color);
    setDeleteFiles(false);
    setSubmitting(false);
    setFormError("");
  });

  const title = createMemo(() => props.action === "edit"
    ? props.t("editProject")
    : props.action === "move"
      ? props.t("moveProject")
    : props.action === "delete"
      ? props.t("deleteProject")
      : props.t("copyProject"));

  const subtitle = createMemo(() => props.action === "copy-full"
    ? props.t("copyFullProjectHint")
    : props.action === "copy-configuration"
      ? props.t("copyConfigurationHint")
      : props.action === "delete"
        ? props.t("deleteProjectHint")
        : props.action === "move"
          ? props.t("moveProjectHint")
        : props.t("editProjectHint"));

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
      const path = await api.chooseDirectory();
      if (path) setDraft("path", path);
    } catch (error) {
      setFormError(formatError(error));
      props.onError(error, "project.copy.choose_directory");
    }
  };

  const submit = async () => {
    const project = props.project;
    const action = props.action;
    if (!project || !action) return;
    setSubmitting(true);
    setFormError("");
    try {
      if (action === "edit") {
        const color = parsedColor();
        if (!draft.name.trim() || !color) return;
        await props.onEdit({
          ...project,
          name: draft.name.trim(),
          description: draft.description.trim(),
          color: color.css,
          updatedAt: new Date().toISOString(),
        });
      } else if (action === "delete") {
        await props.onDelete(project, deleteFiles());
      } else if (action === "move") {
        if (!draft.path.trim()) return;
        if (!props.onMove) return;
        await props.onMove(project, draft.path.trim());
      } else {
        if (!draft.name.trim() || !draft.path.trim()) return;
        await props.onCopy(
          project,
          draft.name.trim(),
          draft.path.trim(),
          action === "copy-full" ? "full" : "configuration",
        );
      }
      props.onClose();
    } catch (error) {
      setFormError(formatError(error));
    } finally {
      setSubmitting(false);
    }
  };

  const canSubmit = createMemo(() => props.action === "delete"
    || props.action === "edit" && Boolean(draft.name.trim() && parsedColor())
    || isCopy() && Boolean(draft.name.trim() && draft.path.trim())
    || props.action === "move" && Boolean(draft.path.trim()));

  const footer = (
    <>
      <button class="button secondary" type="button" disabled={submitting()} onClick={props.onClose}>{props.t("cancel")}</button>
      <button class={`button ${props.action === "delete" ? "danger" : "primary"}`} type="button" disabled={submitting() || !canSubmit()} onClick={() => void submit()}>
        <Show when={submitting()} fallback={props.action === "delete" ? <Trash2 size={16} /> : isCopy() ? <Copy size={16} /> : <Save size={16} />}>
          <LoaderCircle class="spin" size={16} />
        </Show>
        {props.action === "delete" ? props.t("confirmDeleteProject") : isCopy() ? props.t("copyProjectAction") : props.action === "move" ? props.t("moveProjectAction") : props.t("save")}
      </button>
    </>
  );

  return (
    <Modal open={Boolean(props.project && props.action)} title={title()} subtitle={subtitle()} onClose={props.onClose} footer={footer} size="medium">
      <Show when={props.action === "edit"}>
        <div class="project-form">
          <Field label={props.t("projectName")} required><input autofocus value={draft.name} onInput={(event) => setDraft("name", event.currentTarget.value)} /></Field>
          <Field label={props.t("projectDescription")}><textarea rows="3" value={draft.description} onInput={(event) => setDraft("description", event.currentTarget.value)} /></Field>
          <Field label={props.t("color")}>
            <div class="color-control-row">
              <div class="color-swatches">
                {colors.map((color) => <button type="button" class={parsedColor()?.css === color ? "is-selected" : ""} style={{ "background-color": color }} aria-label={color} onClick={() => selectColor(color)} />)}
              </div>
              <input class="project-color-picker" type="color" value={parsedColor()?.picker ?? colors[0]} aria-label={props.t("chooseColor")} title={props.t("chooseColor")} onInput={(event) => selectColor(event.currentTarget.value)} />
            </div>
            <div class="color-value-row">
              <span class="color-value-preview" style={{ "background-color": parsedColor()?.css ?? "transparent" }} aria-hidden="true" />
              <input value={colorInput()} aria-label={props.t("colorValue")} aria-invalid={!parsedColor()} spellcheck={false} onInput={(event) => updateColorInput(event.currentTarget.value)} />
            </div>
            <small class="field-hint color-format-hint">{props.t("colorFormatHint")}</small>
            <Show when={!parsedColor()}><span class="form-error" role="alert">{props.t("invalidColor")}</span></Show>
          </Field>
        </div>
      </Show>

      <Show when={isCopy()}>
        <div class="project-form">
          <div class="copy-mode-summary">
            <Copy size={18} />
            <span><strong>{props.t(props.action === "copy-full" ? "copyFullProject" : "copyConfigurationOnly")}</strong><small>{subtitle()}</small></span>
          </div>
          <Field label={props.t("projectName")} required><input autofocus value={draft.name} onInput={(event) => setDraft("name", event.currentTarget.value)} /></Field>
          <Field label={props.t("copyDestination")} required hint={props.t("emptyDirectoryRequired")}>
            <div class="input-action-group">
              <input value={draft.path} spellcheck={false} onInput={(event) => setDraft("path", event.currentTarget.value)} />
              <button type="button" class="button secondary icon-only" title={props.t("browse")} onClick={() => void browse()}><FolderOpen size={17} /></button>
            </div>
          </Field>
        </div>
      </Show>

      <Show when={props.action === "move"}>
        <Field label={props.t("storagePath")} required hint={props.t("emptyDirectoryRequired")}>
          <div class="input-action-group">
            <input autofocus value={draft.path} spellcheck={false} onInput={(event) => setDraft("path", event.currentTarget.value)} />
            <button type="button" class="button secondary icon-only" title={props.t("browse")} onClick={() => void browse()}><FolderOpen size={17} /></button>
          </div>
        </Field>
      </Show>

      <Show when={props.action === "delete"}>
        <div class="delete-project-content">
          <div class="destructive-notice"><AlertTriangle size={19} /><span><strong>{props.project?.name}</strong><small>{props.project?.storagePath}</small></span></div>
          <Toggle checked={deleteFiles()} onChange={setDeleteFiles} label={props.t("deleteProjectFiles")} description={props.t("deleteProjectFilesHint")} />
          <Show when={!deleteFiles()}><p class="field-hint">{props.t("removeProjectOnlyHint")}</p></Show>
        </div>
      </Show>
      <Show when={formError()}><p class="form-error" role="alert">{formError()}</p></Show>
    </Modal>
  );
}
