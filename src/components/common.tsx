import { Show, createEffect, onCleanup, type JSX, type ParentProps } from "solid-js";
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
  closeLabel?: string;
}>) {
  let shell: HTMLElement | undefined;
  let previouslyFocused: HTMLElement | null = null;
  const titleId = `modal-title-${crypto.randomUUID()}`;
  const subtitleId = `modal-subtitle-${crypto.randomUUID()}`;
  const onOverlayClick: JSX.EventHandlerUnion<HTMLDivElement, MouseEvent> = (event) => {
    if (event.target === event.currentTarget) props.onClose();
  };

  const onKeyDown: JSX.EventHandlerUnion<HTMLElement, KeyboardEvent> = (event) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      props.onClose();
      return;
    }
    if (event.key !== "Tab" || !shell) return;
    const focusable = [...shell.querySelectorAll<HTMLElement>(
      'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
    )].filter((item) => !item.hidden && item.getAttribute("aria-hidden") !== "true");
    if (focusable.length === 0) {
      event.preventDefault();
      shell.focus();
      return;
    }
    const first = focusable[0];
    const last = focusable.at(-1)!;
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  createEffect(() => {
    if (!props.open) return;
    previouslyFocused = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    queueMicrotask(() => {
      const first = shell?.querySelector<HTMLElement>('[autofocus], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])');
      (first ?? shell)?.focus();
    });
    onCleanup(() => previouslyFocused?.focus());
  });

  return (
    <Show when={props.open}>
      <div class="modal-overlay" role="presentation" onMouseDown={onOverlayClick}>
        <section
          ref={shell}
          class={`modal-shell modal-${props.size ?? "medium"}`}
          role="dialog"
          aria-modal="true"
          aria-labelledby={titleId}
          aria-describedby={props.subtitle ? subtitleId : undefined}
          tabindex="-1"
          onKeyDown={onKeyDown}
        >
          <header class="modal-header">
            <div>
              <h2 id={titleId}>{props.title}</h2>
              <Show when={props.subtitle}><p id={subtitleId}>{props.subtitle}</p></Show>
            </div>
            <IconButton label={props.closeLabel ?? "Close"} onClick={props.onClose}><X size={18} /></IconButton>
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
