// A select's popup, as the overlay module draws it for a field of any page (select.tsx asks for it
// through the shell's `reviewOverlay`): a listbox that behaves as the pinned primitive's does
// (dioxus-primitives `select`), mounted afresh on each opening. The list takes the focus; the
// arrows (from the chosen option while none has the focus), Home and End move it among the options,
// round the list, and typing moves it to the option nearest what was typed (`typeahead.ts`); Enter
// or Space chooses the focused option, as a press released on the option it went down on does (a
// finger's or a pen's on its click, so the click lands on the option and nothing under it,
// `choosesOnClick`), and as a click does with no pointer released before it (`clickChooses`), an
// assistive technology's activation; Escape, or the focus leaving the list, closes it, and the
// focus moving among its options, as a screen reader moves it, does not. It is drawn beside its
// field's trigger (`popup-position.ts`), whose page gives the trigger its focus back, and drawn
// closed while its exit plays (overlay.css), which its host waits for before it lets it go
// (`onLeft`).
//
// What an assistive technology sends to activate an option, from the engines' own sources: Blink,
// for TalkBack or any other's default action, focuses the option, then sends a mouse's pointerdown,
// mousedown, pointerup and mouseup at its middle, then its click; WebKit on iOS, for VoiceOver,
// sends a touch (pointer events of a touch) where a page hears touches, as this one does, else a
// mouse's down and up, then the click; WebKit on the Mac a mouse's pointer events and the click;
// Gecko's activation, as a script's `click()`, is the click alone.

import { type KeyboardEvent, type PointerEvent, useEffect, useRef, useState } from "react";
import { anchor } from "./popup-position";
import { Debounce } from "./timing";
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

/** Where the arrows move from: the focused option, else the chosen one (`value`'s), else none. */
export function arrowsFrom(
  options: readonly [string, string][],
  value: string,
  focused: number | null,
): number | null {
  if (focused !== null) return focused;
  const chosen = options.findIndex(([key]) => key === value);
  return chosen < 0 ? null : chosen;
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

/** A pointer released on an option since the last press: whether that release chose by the
 * option's click (a finger's or a pen's tap, `choosesOnClick`). */
export interface Released {
  key: string;
  chooses: boolean;
}

/** Whether a click on the option `clicked` chooses it, after `released`, the pointer released since
 * the last press, if any: the click of a tap on that option does, the click after any other
 * release does not (a mouse's chose at its release; a touch that moved, or a press released on
 * another option, chose nothing), and a click with no release before it, as an assistive
 * technology's or a script's is, does while the popup is open. */
export function clickChooses(released: Released | null, clicked: string, open: boolean): boolean {
  if (released === null) return open;
  return released.chooses && released.key === clicked;
}

/** Whether the focus that left `from` went to `to` within it: moved among the list's options, or
 * to the list, it has not left the popup. */
export function staysWithin(from: Node | null, to: EventTarget | null): boolean {
  return from !== null && to instanceof Node && from.contains(to);
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
  /** Closed by its request, as its trigger closes it: drawn closed while it leaves. */
  closed?: boolean;
  /** Closed, its exit is over, or it had none to play: the host lets it go. */
  onLeft?: (() => void) | undefined;
}

export function SelectPopup({
  id,
  label,
  value,
  options,
  onChange,
  onClose,
  closed = false,
  onLeft,
}: SelectPopupProps) {
  const popup = `${id}-popup`;
  const [open, setOpen] = useState(true);
  const shut = closed || !open;
  const [focused, setFocused] = useState<number | null>(null);
  const list = useRef<HTMLDivElement>(null);
  const search = useRef({
    typed: "",
    resting: new Debounce(TYPEAHEAD_MS),
    keyboard: new Keyboard(),
  });
  // The option a pointer went down on, and where: only a release on that same option chooses.
  const down = useRef<{ key: string; at: [number, number] } | null>(null);
  // The pointer released since the last press, which the next click is judged by (`clickChooses`).
  const released = useRef<Released | null>(null);

  // Placed beside the trigger now and as the page or the viewport moves, until it closes.
  useEffect(() => anchor(id, popup), [id, popup]);
  useEffect(() => () => search.current.resting.cancel(), []);
  // The list holds the focus until an option takes it.
  useEffect(() => {
    if (shut) return;
    if (focused === null) list.current?.focus();
    else document.getElementById(`${popup}-${focused}`)?.focus();
  }, [shut, focused, popup]);
  // Closed with no exit playing (none drawn, or one its sheet cancelled), it is let go at once.
  useEffect(() => {
    if (shut && !list.current?.getAnimations?.().some(({ playState }) => playState === "running"))
      onLeft?.();
  }, [shut, onLeft]);

  const close = () => {
    if (shut) return;
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
    const from = arrowsFrom(options, value, focused);
    if (key === " " || key === "Enter") {
      if (!shut && focused !== null) {
        const option = options[focused];
        if (option) choose(option[0]);
      }
      event.preventDefault();
      event.stopPropagation();
    } else if (typesLetter(key)) {
      const kept = search.current;
      kept.typed += key;
      // What was typed is forgotten once the keys rest for TYPEAHEAD_MS: a later key starts again.
      void kept.resting.rest().then((rested) => {
        if (rested) kept.typed = "";
      });
      const match = bestMatch(
        kept.keyboard,
        kept.typed,
        options.map(([, text], index) => ({ index, text })),
      );
      if (match !== null) setFocused(match);
    } else if (key === "ArrowUp") moved(previousIndex(options.length, from));
    else if (key === "ArrowDown") moved(nextIndex(options.length, from));
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
    released.current = null;
  };
  const release = (event: PointerEvent<HTMLDivElement>, chosen: string) => {
    if (event.button !== 0) return;
    const from = down.current;
    down.current = null;
    const tap =
      from?.key === chosen &&
      tapped(from.at, [event.clientX, event.clientY], event.pointerType === "touch");
    const onClick = tap && choosesOnClick(event.pointerType);
    released.current = { key: chosen, chooses: onClick };
    if (tap && !onClick) choose(chosen);
  };
  // A click on `clicked`, judged by the pointer released before it, if any, which it spends.
  const clicked = (clicked: string) => {
    const before = released.current;
    released.current = null;
    if (clickChooses(before, clicked, !shut)) choose(clicked);
  };

  return (
    <div className="dx-select" data-symphony-control="select" data-state={shut ? "closed" : "open"}>
      <div
        ref={list}
        id={popup}
        role="listbox"
        tabIndex={!shut && focused === null ? 0 : -1}
        aria-multiselectable="false"
        data-state={shut ? "closed" : "open"}
        className="dx-select-list"
        style={HIDDEN}
        aria-label={label}
        onKeyDown={onKeyDown}
        onAnimationEnd={(event) => {
          if (shut && event.target === event.currentTarget) onLeft?.();
        }}
        onFocus={(event) => {
          // The list's own focus, from an option (a press on its edge): no option holds it now.
          if (event.target === event.currentTarget && focused !== null) setFocused(null);
        }}
        onBlur={(event) => {
          // The list's own blur: an option's comes here too, and is its own (below).
          if (
            event.target === event.currentTarget &&
            focused === null &&
            !staysWithin(event.currentTarget, event.relatedTarget)
          )
            close();
        }}
      >
        {options.map(([key, text], index) => (
          // biome-ignore lint/a11y/useKeyWithClickEvents: the click ends a finger's or a pen's tap, or is an assistive technology's activation; the keyboard chooses with Enter or Space on the list (`onKeyDown`).
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
            onFocus={() => {
              // Given the focus by another than the list's keys, a screen reader's move among them.
              if (focused !== index) setFocused(index);
            }}
            onBlur={(event) => {
              // Gone to another option, that one takes it (above); to the list, the list does.
              if (focused === index && !staysWithin(list.current, event.relatedTarget)) {
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
