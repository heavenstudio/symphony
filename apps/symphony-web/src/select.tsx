// A select field, drawn as the Dioxus entry's (`components/select/deferred.rs`): the page draws its
// trigger, and opening it asks the overlay module, the Dioxus program's kind the shell loads on a
// first use, for its popup (`window.reviewOverlay`, shell.js). The request is data and one
// callback, which the popup answers with the value chosen, or nothing once it closes. The shell
// answers nothing for a field whose page it leaves.

import { useEffect, useRef, useState } from "react";
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
  label: string;
  value: string;
  /** Each option's value and text, in order. */
  options: [string, string][];
  onChange: (value: string) => void;
  disabled?: boolean;
}

/**
 * A select: its trigger shows the chosen option's text. A click, or the arrow keys on it, opens
 * its popup; another click or Escape closes it; a choice calls `onChange` and gives the trigger
 * its focus back.
 */
export function SelectField({
  label,
  value,
  options,
  onChange,
  disabled = false,
}: SelectFieldProps) {
  const [id] = useState(() => `symphony-web-field-${fields++}`);
  const [open, setOpen] = useState(false);
  const mount = useMount();
  const latest = useRef({ open, label, value, options, onChange });
  latest.current = { open, label, value, options, onChange };
  // The popup's answers, taken after its own event and blur handlers have run, while the field
  // is drawn; and how the trigger opens or closes it.
  const drawn = useRef(false);
  const [act] = useState(() => {
    const answer = (chosen: unknown) => {
      queueMicrotask(() => {
        if (!drawn.current || mount.aborted) return;
        if (typeof chosen === "string") latest.current.onChange(chosen);
        setOpen(false);
        latest.current.open = false;
        overlay({ kind: "close", owner: id });
        document.getElementById(id)?.focus();
      });
    };
    const activate = () => {
      if (latest.current.open) {
        answer(null);
        return;
      }
      const { label, value, options } = latest.current;
      latest.current.open = true;
      setOpen(true);
      overlay({ kind: "select", owner: id, id, label, value, options, callback: answer });
    };
    return { answer, activate };
  });
  useEffect(() => {
    drawn.current = true;
    const keys = (event: KeyboardEvent) => {
      const target = event.target;
      if (!(target instanceof Element) || target.id !== id || target.hasAttribute("disabled")) {
        return;
      }
      const action = keyAction(event.key, latest.current.open);
      if (action === null) return;
      event.preventDefault();
      if (action === "close") {
        event.stopPropagation();
        act.answer(null);
      } else if (action === "open") {
        act.activate();
      }
    };
    addEventListener("keydown", keys);
    return () => {
      removeEventListener("keydown", keys);
      drawn.current = false;
      overlay({ kind: "close", owner: id });
    };
  }, [id, act]);
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
        onClick={act.activate}
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
