// A select's popup, as the overlay module draws it for a field of any page (select.tsx asks for it
// through the shell's `reviewOverlay`): a listbox that behaves as the pinned primitive's does
// (dioxus-primitives `select`), mounted afresh on each opening. The list takes the focus; the
// arrows, Home and End move it among the options, round the list, and typing moves it to the
// option nearest what was typed (`typeahead.ts`); Enter or Space chooses the focused option, as a
// press released on the option it went down on does (a finger's or a pen's on its click, so the
// click lands on the option and nothing under it, `choosesOnClick`); Escape, or the focus leaving
// the list, closes it. It is drawn beside its field's trigger (`popup-position.ts`), whose page
// gives the trigger its focus back.

import { type KeyboardEvent, type PointerEvent, useEffect, useRef, useState } from "react";
import { anchor } from "./popup-position";
import { debounce } from "./timing";
import { bestMatch, Keyboard, TYPEAHEAD_MS } from "./typeahead";

/** The next option after `current` among `count`, round the list; the first with none. */
export function nextIndex(count: number, current: number | null): number | null {
  if (count === 0) return null;
  if (current === null) return 0;
  return current + 1 < count ? current + 1 : 0;
}

/** The option before `current` among `count`, round the list; the last with none. */
export function previousIndex(count: number, current: number | null): number | null {
  if (count === 0) return null;
  if (current === null || current === 0) return count - 1;
  return current - 1;
}

/** Whether a key types a letter (a search) rather than naming one (`Enter`, `ArrowDown`…). */
export function typesLetter(key: string): boolean {
  return [...key].length === 1;
}

/** Whether a pointer that went down at `down` and up at `up` on the same option chose it: a touch
 * that moved further than 5 pixels scrolled rather than tapped. */
export function tapped(down: [number, number], up: [number, number], touch: boolean) {
  if (!touch) return true;
  const [dx, dy] = [up[0] - down[0], up[1] - down[1]];
  return dx * dx + dy * dy <= 25;
}

/** Whether a tap of `pointerType` chooses on the option's own click rather than at its release:
 * a finger's or a pen's tap sends its click after the release, aimed where the option was, and a
 * popup closed at the release left it to land on whatever lay under the option (the Add task
 * form's Send, which sent the form). A mouse's click goes to no element once the popup that held
 * its press is gone. */
export function choosesOnClick(pointerType: string): boolean {
  return pointerType === "touch" || pointerType === "pen";
}

/** The list hidden until it is placed: measured before it is first painted. */
const HIDDEN = { position: "fixed", visibility: "hidden" } as const;

export interface SelectPopupProps {
  /** The trigger's id, beside which the popup is drawn, as `ID-popup`. */
  id: string;
  label: string;
  value: string;
  /** Each option's value and text, in order. */
  options: [string, string][];
  /** An option chosen. */
  onChange: (value: string) => void;
  /** The popup closed without a choice of its own: Escape, or the focus leaving it. */
  onClose: () => void;
}

export function SelectPopup({ id, label, value, options, onChange, onClose }: SelectPopupProps) {
  const popup = `${id}-popup`;
  const [open, setOpen] = useState(true);
  const [focused, setFocused] = useState<number | null>(null);
  const list = useRef<HTMLDivElement>(null);
  const search = useRef({ typed: "", resting: new AbortController(), keyboard: new Keyboard() });
  // The option a pointer went down on, and where: only a release on that same option chooses.
  const down = useRef<{ key: string; at: [number, number] } | null>(null);
  // The option a finger's or a pen's tap chose, waiting for the tap's own click (`choosesOnClick`).
  const tap = useRef<string | null>(null);

  // Placed beside the trigger now and as the page or the viewport moves, until it closes.
  useEffect(() => anchor(id, popup), [id, popup]);
  useEffect(() => () => search.current.resting.abort(), []);
  // The list holds the focus until an option takes it.
  useEffect(() => {
    if (!open) return;
    if (focused === null) list.current?.focus();
    else document.getElementById(`${popup}-${focused}`)?.focus();
  }, [open, focused, popup]);

  const close = () => {
    if (!open) return;
    setOpen(false);
    onClose();
  };
  const choose = (chosen: string) => {
    onChange(chosen);
    close();
  };
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const key = event.key;
    if (typesLetter(key)) search.current.keyboard.learn(event.code, key);
    const moved = (to: number | null) => {
      search.current.typed = "";
      event.preventDefault();
      event.stopPropagation();
      setFocused(to);
    };
    if (key === " " || key === "Enter") {
      if (open && focused !== null) {
        const option = options[focused];
        if (option) choose(option[0]);
      }
      event.preventDefault();
      event.stopPropagation();
    } else if (typesLetter(key)) {
      const kept = search.current;
      kept.resting.abort();
      kept.typed += key;
      // What was typed is forgotten once the keys rest for TYPEAHEAD_MS: a later key starts again.
      const resting = new AbortController();
      kept.resting = resting;
      void debounce(TYPEAHEAD_MS, resting.signal).then(() => {
        if (!resting.signal.aborted) kept.typed = "";
      });
      const match = bestMatch(
        kept.keyboard,
        kept.typed,
        options.map(([, text], index) => ({ index, text })),
      );
      if (match !== null) setFocused(match);
    } else if (key === "ArrowUp") moved(previousIndex(options.length, focused));
    else if (key === "ArrowDown") moved(nextIndex(options.length, focused));
    else if (key === "Home") moved(options.length > 0 ? 0 : null);
    else if (key === "End") moved(options.length > 0 ? options.length - 1 : null);
    else if (key === "Escape") {
      close();
      event.preventDefault();
      event.stopPropagation();
    }
  };
  const press = (event: PointerEvent<HTMLDivElement>, pressed: string) => {
    if (event.button !== 0) return;
    event.preventDefault();
    down.current = { key: pressed, at: [event.clientX, event.clientY] };
    tap.current = null;
  };
  const release = (event: PointerEvent<HTMLDivElement>, chosen: string) => {
    if (event.button !== 0) return;
    const from = down.current;
    down.current = null;
    if (from?.key !== chosen) return;
    if (tapped(from.at, [event.clientX, event.clientY], event.pointerType === "touch")) {
      if (choosesOnClick(event.pointerType)) tap.current = chosen;
      else choose(chosen);
    }
  };
  // The click of the tap that chose `clicked`; any other click, a script's among them, is nothing.
  const clicked = (clicked: string) => {
    if (tap.current !== clicked) return;
    tap.current = null;
    choose(clicked);
  };

  return (
    <div className="dx-select" data-symphony-control="select" data-state={open ? "open" : "closed"}>
      <div
        ref={list}
        id={popup}
        role="listbox"
        tabIndex={open && focused === null ? 0 : -1}
        aria-multiselectable="false"
        data-state={open ? "open" : "closed"}
        className="dx-select-list"
        style={HIDDEN}
        aria-label={label}
        onKeyDown={onKeyDown}
        onBlur={(event) => {
          // The list's own blur: an option's comes here too, and is its own (below).
          if (event.target === event.currentTarget && focused === null) close();
        }}
      >
        {options.map(([key, text], index) => (
          // biome-ignore lint/a11y/useKeyWithClickEvents: the click only ends a finger's or a pen's tap; the keyboard chooses with Enter or Space on the list (`onKeyDown`).
          <div
            key={key}
            role="option"
            id={`${popup}-${index}`}
            tabIndex={focused === index ? 0 : -1}
            aria-selected={key === value}
            aria-disabled="false"
            data-disabled="false"
            className="dx-select-option"
            onPointerDown={(event) => press(event, key)}
            onPointerUp={(event) => release(event, key)}
            onClick={() => clicked(key)}
            onPointerCancel={() => {
              down.current = null;
            }}
            onBlur={() => {
              if (focused === index) {
                setFocused(null);
                close();
              }
            }}
          >
            {text}
            {key === value && (
              // biome-ignore lint/a11y/noSvgWithoutTitle: the chosen option's check mark, drawn as the Dioxus icons draw it, says nothing its `aria-selected` does not.
              <svg
                xmlns="http://www.w3.org/2000/svg"
                width="1rem"
                height="1rem"
                viewBox="0 0 24 24"
                fill="none"
                stroke="var(--secondary-color-5)"
                strokeWidth="2"
                strokeLinecap="round"
                strokeLinejoin="round"
              >
                <path d="M20 6 9 17l-5-5" />
              </svg>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
