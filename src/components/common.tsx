import { Show, type JSX, type ParentProps } from "solid-js";
import { X } from "lucide-solid";

export function IconButton(props: {
  label: string;
  onClick?: () => void;
  children: JSX.Element;
  active?: boolean;
  disabled?: boolean;
  class?: string;
  type?: "button" | "submit";
}) {
  return (
    <button
      type={props.type ?? "button"}
      class={`icon-button ${props.active ? "is-active" : ""} ${props.class ?? ""}`}
      aria-label={props.label}
      aria-pressed={props.active === undefined ? undefined : props.active}
      title={props.label}
      disabled={props.disabled}
      onClick={props.onClick}
    >
      {props.children}
    </button>
  );
}

export function Toggle(props: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  description?: string;
  disabled?: boolean;
}) {
  return (
    <label class={`toggle-row ${props.disabled ? "is-disabled" : ""}`}>
      <span class="toggle-copy">
        <span>{props.label}</span>
        <Show when={props.description}>
          <small>{props.description}</small>
        </Show>
      </span>
      <input
        type="checkbox"
        checked={props.checked}
        disabled={props.disabled}
        onInput={(event) => props.onChange(event.currentTarget.checked)}
      />
      <span class="toggle-track" aria-hidden="true"><span /></span>
    </label>
  );
}

export function Field(props: ParentProps<{
  label: string;
  hint?: string;
  required?: boolean;
  class?: string;
}>) {
  return (
    <label class={`field ${props.class ?? ""}`}>
      <span class="field-label">
        {props.label}
        <Show when={props.required}><b>*</b></Show>
        <Show when={props.hint}><small>{props.hint}</small></Show>
      </span>
      {props.children}
    </label>
  );
}

export function Modal(props: ParentProps<{
  open: boolean;
  title: string;
  subtitle?: string;
  onClose: () => void;
  size?: "medium" | "large" | "wide";
  footer?: JSX.Element;
}>) {
  const onOverlayClick: JSX.EventHandlerUnion<HTMLDivElement, MouseEvent> = (event) => {
    if (event.target === event.currentTarget) props.onClose();
  };

  return (
    <Show when={props.open}>
      <div class="modal-overlay" role="presentation" onMouseDown={onOverlayClick}>
        <section class={`modal-shell modal-${props.size ?? "medium"}`} role="dialog" aria-modal="true" aria-label={props.title}>
          <header class="modal-header">
            <div>
              <h2>{props.title}</h2>
              <Show when={props.subtitle}><p>{props.subtitle}</p></Show>
            </div>
            <IconButton label="Close" onClick={props.onClose}><X size={18} /></IconButton>
          </header>
          <div class="modal-body">{props.children}</div>
          <Show when={props.footer}>
            <footer class="modal-footer">{props.footer}</footer>
          </Show>
        </section>
      </div>
    </Show>
  );
}

export function EmptyState(props: {
  icon: JSX.Element;
  title: string;
  description?: string;
  action?: JSX.Element;
}) {
  return (
    <div class="empty-state">
      <span class="empty-icon">{props.icon}</span>
      <strong>{props.title}</strong>
      <Show when={props.description}><p>{props.description}</p></Show>
      <Show when={props.action}><div>{props.action}</div></Show>
    </div>
  );
}

export function StatusDot(props: { status: "online" | "offline" | "busy" | "idle" }) {
  return <span class={`status-dot status-${props.status}`} aria-hidden="true" />;
}
