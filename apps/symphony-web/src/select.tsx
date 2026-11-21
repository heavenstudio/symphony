// A select field, drawn as the chat's was (the copied components' deferred select): the
// page draws its trigger, and opening it asks the overlay module, this client's persistent kind
// `overlay`, which the shell loads on a first use or ahead once the page is idle, for its popup
// (`window.reviewOverlay`, shell.ts).
// The request is data and one callback, which the popup answers with the value chosen, or nothing
// once it closes. The shell answers nothing for a field whose page it leaves.

import { useEffect, useState } from "react";
import { useMount } from "./mount";

/** What the overlay module is asked: a select's popup, or to close the popup `owner` opened. */
export type OverlayRequest =
  | {
      kind: "select";
      owner: string;
      id: string;
      label: string;
      value: string;
      options: [string, string][];
      callback: (value: unknown) => void;
    }
  | { kind: "close"; owner: string };

/** Ask the overlay module, when the shell gives one. */
export function overlay(request: OverlayRequest): void {
  (window as { reviewOverlay?: (request: OverlayRequest) => void }).reviewOverlay?.(request);
}

/** What a key pressed on a field's trigger does: Escape closes its open popup; an arrow opens a
 * closed one and, open, scrolls nothing (`stay`); any other key goes on as it would (none). */
export function keyAction(key: string, open: boolean): "close" | "open" | "stay" | null {
  if (key === "Escape") return open ? "close" : null;
  if (key !== "ArrowDown" && key !== "ArrowUp") return null;
  return open ? "stay" : "open";
}

let fields = 0;

export interface SelectFieldProps {
  /** The trigger's id, which a label names; one of the page's own otherwise. */
  id?: string;
  label: string;
  value: string;
  /** Each option's value and text, in order. */
  options: [string, string][];
  onChange: (value: string) => void;
  disabled?: boolean;
}

/**
 * What a field's trigger does with its popup: open it, close it, and take its answer. The page's
 * field gives it what it draws (`field`) each time it is drawn, and starts it once drawn.
 */
export class FieldControl {
  /** Whether the popup is open. */
  open = false;
  /** What the field draws: its label, value and options, and what a choice calls. */
  field: Omit<SelectFieldProps, "disabled" | "id">;
  /** Whether the field is drawn (`start`), outside which an answer does nothing. */
  private drawn = false;

  constructor(
    readonly id: string,
    field: Omit<SelectFieldProps, "disabled" | "id">,
    private readonly mount: AbortSignal,
    private readonly shown: (open: boolean) => void,
  ) {
    this.field = field;
  }

  /** The popup's answer, the value chosen or nothing once it closes, taken after its own event and
   * blur handlers have run: the choice reaches the page, the popup closes and the trigger has its
   * focus back. An answer to a field no longer drawn, or of a page stopped, does nothing. */
  readonly answer = (chosen: unknown): void => {
    queueMicrotask(() => {
      if (!this.drawn || this.mount.aborted) return;
      if (typeof chosen === "string") this.field.onChange(chosen);
      this.shown(false);
      this.open = false;
      overlay({ kind: "close", owner: this.id });
      document.getElementById(this.id)?.focus();
    });
  };

  /** A click on the trigger: it opens the popup, or closes the one open. */
  readonly activate = (): void => {
    if (this.open) {
      this.answer(null);
      return;
    }
    const { label, value, options } = this.field;
    this.open = true;
    this.shown(true);
    overlay({
      kind: "select",
      owner: this.id,
      id: this.id,
      label,
      value,
      options,
      callback: this.answer,
    });
  };

  /** A key pressed on the page: on this field's trigger, enabled, it opens or closes the popup
   * (`keyAction`). */
  readonly key = (event: KeyboardEvent): void => {
    const target = event.target;
    if (!(target instanceof Element) || target.id !== this.id || target.hasAttribute("disabled")) {
      return;
    }
    const action = keyAction(event.key, this.open);
    if (action === null) return;
    event.preventDefault();
    if (action === "close") {
      event.stopPropagation();
      this.answer(null);
    } else if (action === "open") {
      this.activate();
    }
  };

  /** The field drawn: it hears its keys and takes answers until the cleanup returned, which closes
   * its popup. */
  start(): () => void {
    this.drawn = true;
    addEventListener("keydown", this.key);
    return () => {
      removeEventListener("keydown", this.key);
      this.drawn = false;
      overlay({ kind: "close", owner: this.id });
    };
  }
}

/**
 * A select: its trigger shows the chosen option's text. A click, or the arrow keys on it, opens
 * its popup; another click or Escape closes it; a choice calls `onChange` and gives the trigger
 * its focus back (`FieldControl`).
 */
export function SelectField({
  id: named,
  label,
  value,
  options,
  onChange,
  disabled = false,
}: SelectFieldProps) {
  const [open, setOpen] = useState(false);
  const mount = useMount();
  const [control] = useState(
    () =>
      new FieldControl(
        named ?? `symphony-web-field-${fields++}`,
        { label, value, options, onChange },
        mount,
        setOpen,
      ),
  );
  control.field = { label, value, options, onChange };
  useEffect(() => control.start(), [control]);
  const { id } = control;
  const text = options.find(([key]) => key === value)?.[1] ?? "";
  return (
    <div className="dx-select">
      <button
        id={id}
        type="button"
        role="combobox"
        className="dx-select-trigger"
        disabled={disabled}
        aria-label={label}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={`${id}-popup`}
        onClick={control.activate}
      >
        <span>{text}</span>
        <svg
          aria-hidden="true"
          width="20"
          height="20"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <path d="m6 9 6 6 6-6" />
        </svg>
      </button>
    </div>
  );
}
