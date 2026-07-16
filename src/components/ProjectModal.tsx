import { createEffect } from "solid-js";
import { createStore } from "solid-js/store";
import { FolderOpen, Save } from "lucide-solid";
import { api } from "../lib/api";
import type { TranslationKey } from "../lib/i18n";
import type { Project, ProviderProfile } from "../types";
import { Field, Modal } from "./common";

interface ProjectModalProps {
  open: boolean;
  providers: ProviderProfile[];
  t: (key: TranslationKey) => string;
  onClose: () => void;
  onCreate: (project: Project) => void;
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

  createEffect(() => {
    if (props.open) setDraft({ name: "", description: "", storagePath: defaultStoragePath, color: colors[0] });
  });

  const browse = async () => {
    const path = await api.chooseDirectory(draft.storagePath);
    if (path) setDraft("storagePath", path);
  };

  const create = () => {
    if (!draft.name.trim() || !draft.storagePath.trim()) return;
    const provider = props.providers.find((item) => item.enabled) ?? props.providers[0];
    const now = new Date().toISOString();
    props.onCreate({
      id: crypto.randomUUID(),
      name: draft.name.trim(),
      description: draft.description.trim(),
      storagePath: draft.storagePath.trim(),
      color: draft.color,
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
  };

  const footer = (
    <>
      <button class="button secondary" type="button" onClick={props.onClose}>{props.t("cancel")}</button>
      <button class="button primary" type="button" disabled={!draft.name.trim() || !draft.storagePath.trim()} onClick={create}><Save size={16} />{props.t("createProjectAction")}</button>
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
          <div class="color-swatches">
            {colors.map((color) => (
              <button
                type="button"
                class={draft.color === color ? "is-selected" : ""}
                style={{ "background-color": color }}
                aria-label={color}
                onClick={() => setDraft("color", color)}
              />
            ))}
          </div>
        </Field>
      </div>
    </Modal>
  );
}
