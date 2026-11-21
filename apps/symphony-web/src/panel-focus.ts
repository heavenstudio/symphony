// Where the focus goes as the panel's sheets open and close. The header and the `+` button
// (frame.tsx) ask for the focus to move into a sheet they open (`reviewPanelFocus`, with the control
// that opened it, `reviewPanelOpener`): the sheet takes it once it shows, unless the reader has put
// it in the sheet already, and gives it back to that control when it closes, else to its header or
// page button. A send holds the focus in the sheet while it disables the sheet's fields, and gives it
// to the field the reader types in once it is over. Escape closes a sheet the focus is in, unless a
// control in it takes that Escape itself.

import { useEffect, useRef } from "react";
import { remember, snapshot } from "./shared";
// biome-ignore lint/style/noRestrictedImports: no-timed-waits baseline: focusFirst looks again every 20 ms for the element to focus, where its mount is the event
import { sleep } from "./timing";

/** The first element `selector` finds that is drawn (has a box), as an element focus can take. */
function shown(selector: string): HTMLElement | null {
  for (const element of document.querySelectorAll<HTMLElement>(selector)) {
    const box = element.getBoundingClientRect();
    if (box.width > 0 || box.height > 0) return element;
  }
  return null;
}

/**
 * Whether the focus is in the sheet `within` as the panel draws it, not in the shell's stand-in for
 * it while the panel loads (`#panel-loading`, shell/shell.ts), which carries the same id.
 */
function focusIn(within: string): boolean {
  const active = document.activeElement;
  return (
    typeof active?.closest === "function" &&
    active.closest(within) !== null &&
    active.closest("#panel-loading") === null
  );
}

/** Focus the first of `selectors` to show and take it now: whether one did. */
function focusShown(selectors: readonly string[]): boolean {
  for (const selector of selectors) {
    const element = shown(selector);
    element?.focus();
    if (element && document.activeElement === element) return true;
  }
  return false;
}

/**
 * Focus the first of `selectors` to show and take it, trying for a second while the panel draws.
 * The first try waits for the task that opened it to end: the shell takes its loading sheet down
 * once the panel is up (shell/shell.ts), and the focus would go with it. The tries end once the
 * focus is in the sheet `within`, wherever the reader put it: a sheet shown before its tries run is
 * one the reader can already press a tab or a field of, and a later try would take that away.
 */
export async function focusFirst(selectors: readonly string[], within?: string): Promise<void> {
  for (let tries = 0; tries < 50; tries += 1) {
    await sleep(tries === 0 ? 0 : 20);
    if (within !== undefined && focusIn(within)) return;
    if (focusShown(selectors)) return;
  }
}

/** The sheet each of the panel's sheets is drawn in. */
const SHEETS = { quick: "#quick-sheet", activity: "#activity-panel" } as const;

/** Move the focus into the sheet `which` when its opener asked for it: a sheet shown again on
 * another page leaves the focus where it is. */
export function takeFocus(which: "quick" | "activity", selectors: readonly string[]): void {
  if (snapshot("reviewPanelFocus") !== which) return;
  remember("reviewPanelFocus", null);
  void focusFirst(selectors, SHEETS[which]);
}

/** Give the focus back to the control that opened a sheet, else to `fallback` (its header or page
 * button), once that shows again: the `+` button is hidden while its sheet is open. */
export function returnFocus(fallback: string): void {
  const opener = snapshot("reviewPanelOpener");
  remember("reviewPanelOpener", null);
  const selectors: string[] = [];
  if (
    opener instanceof Element &&
    opener.isConnected &&
    opener.closest("#quick-sheet, #activity-panel") === null
  ) {
    // The opener, found again by a mark only it carries.
    opener.setAttribute("data-panel-opener", "");
    selectors.push("[data-panel-opener]");
  }
  selectors.push(fallback);
  void focusFirst(selectors).then(() =>
    document.querySelector("[data-panel-opener]")?.removeAttribute("data-panel-opener"),
  );
}

/**
 * Hold the focus in the sheet `within` while a send disables its fields and buttons: a disabled
 * control drops the focus to the page's body, where Escape closes nothing, so it goes to the first
 * shown element `holder` finds (a stop for Tab the send leaves enabled). The element holding it,
 * or none when the focus was not in the sheet.
 */
export function holdFocus(within: string, holder: string): Element | null {
  if (!focusIn(within)) return null;
  const held = shown(holder);
  held?.focus();
  return held !== null && document.activeElement === held ? held : null;
}

/** The send over and its fields enabled again: the focus `held` holds goes to the first of
 * `selectors` that shows and takes it (the field the reader types in), unless the reader moved it
 * meanwhile. */
export function releaseFocus(held: Element | null, selectors: readonly string[]): void {
  if (held !== null && document.activeElement === held) focusShown(selectors);
}

/** What an Escape key's event names: its key, whether a handler took it, and where it was pressed. */
export interface EscapeEvent {
  key: string;
  defaultPrevented: boolean;
  target: EventTarget | null;
}

/** Whether an Escape pressed in the sheet `within` closes it: not one a control there took (an
 * open list, menu or tooltip, or a handler that prevented its default). */
export function escapeCloses(event: EscapeEvent, within: string): boolean {
  if (event.key !== "Escape" || event.defaultPrevented) return false;
  const target = event.target as Element | null;
  if (typeof target?.closest !== "function") return false;
  return (
    target.closest(within) !== null &&
    target.closest("[role=listbox], [role=menu], [aria-expanded=true]") === null
  );
}

/** Close the sheet `within` on an Escape pressed in it (`escapeCloses`), heard on the window. */
export function useEscape(within: string, close: () => void): void {
  const latest = useRef(close);
  latest.current = close;
  useEffect(() => {
    const keyed = (event: KeyboardEvent) => {
      if (escapeCloses(event, within)) latest.current();
    };
    addEventListener("keydown", keyed);
    return () => removeEventListener("keydown", keyed);
  }, [within]);
}
