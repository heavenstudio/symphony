// A row of choices that scrolls across where its window is too narrow for it: the header's pages,
// the board's wait strip and its stage tabs on a phone. Three things hold for each. The sides it
// continues past are read from the row itself (`data-overflow`), so the sheet fades only an edge
// with something beyond it (`.scroll-row`, frame.css), and fades the row, not a colour over it, so
// the cue shows on whatever surface is behind. A choice that does not show whole in the row's
// window is marked cut (`data-cut`), and the sheet draws it as its empty shape, so no label is
// ever read in part: what fades at an edge is the shape of a choice, which says there are more.
// The current choice is brought whole into view, clear of a faded edge, when it changes and when
// the row's width does, and so is a choice that takes the focus: the browser's own scroll to a
// focused element stops where the row's snap moves it on from, which can cut it again. The marks
// are written on every row and read by the sheet on a phone alone: a desk's row wraps or fits,
// scrolls nowhere, and is drawn as it was.

import { useCallback, useLayoutEffect, useRef } from "react";

/** The sides a row continues past. */
export type Sides = "none" | "start" | "end" | "both";

/** The width of a faded edge, the sheet's `--fade` and the row's scroll padding (frame.css). */
export const FADE = 20;

/** The sides a row scrolled to `left`, `width` wide over `full` of content, continues past. A pixel
 * of rounding is no overflow. */
export function overflowSides(left: number, width: number, full: number): Sides {
  const start = left > 1;
  const end = full - width - left > 1;
  if (start) return end ? "both" : "start";
  return end ? "end" : "none";
}

/**
 * Where a row scrolled to `left` (`width` wide over `full`) goes so that the choice between
 * `from` and `to`, in the content's own measure, shows whole and clear of the fade at each side
 * the row continues past: where it is when the choice already does, else the choice at the row's
 * start, a fade's width in, which is where a snap rests it.
 */
export function revealed(
  left: number,
  width: number,
  full: number,
  from: number,
  to: number,
): number {
  const most = Math.max(0, full - width);
  const start = left > 1 ? left + FADE : left;
  const end = most - left > 1 ? left + width - FADE : left + width;
  if (from >= start && to <= end) return left;
  return Math.min(most, Math.max(0, from - FADE));
}

/** What of a row's choice is read and marked. */
export type Choice = Pick<Element, "getBoundingClientRect" | "toggleAttribute">;

/** What of a row is read and moved. */
export type Row = Pick<
  HTMLElement,
  "scrollLeft" | "clientWidth" | "scrollWidth" | "clientLeft" | "dataset" | "getBoundingClientRect"
> & {
  children: ArrayLike<Choice>;
  querySelector(selector: string): Pick<Element, "getBoundingClientRect"> | null;
};

/** The row's current choice: the page or the stage shown, or the filter pressed. */
export const CURRENT = '[aria-current="page"], [aria-pressed="true"]';

/** Where `row`'s window starts in the browser's. */
function edge(row: Row): number {
  return row.getBoundingClientRect().left + row.clientLeft;
}

/** Say on `row` the sides it continues past, and on each of its choices whether it is cut: not
 * whole in the row's window, half a pixel of rounding aside. */
export function mark(row: Row): void {
  row.dataset.overflow = overflowSides(row.scrollLeft, row.clientWidth, row.scrollWidth);
  const from = edge(row) - 0.5;
  const to = from + row.clientWidth + 1;
  for (const choice of Array.from(row.children)) {
    const box = choice.getBoundingClientRect();
    choice.toggleAttribute("data-cut", box.left < from || box.left + box.width > to);
  }
}

/** Bring `item`, `row`'s current choice unless another is given, whole into view; a row with
 * none stays where it is. */
export function reveal(
  row: Row,
  item: Pick<Element, "getBoundingClientRect"> | null = row.querySelector(CURRENT),
): void {
  if (item === null) return;
  const box = item.getBoundingClientRect();
  const from = box.left - edge(row) + row.scrollLeft;
  const next = revealed(row.scrollLeft, row.clientWidth, row.scrollWidth, from, from + box.width);
  if (next !== row.scrollLeft) row.scrollLeft = next;
}

/** Follow `row` until the function returned is called: it is marked again at each scroll, its
 * current choice revealed again when its width changes, as at first, and a choice that takes the
 * focus revealed as it does. */
export function followRow(row: HTMLElement): () => void {
  const again = () => mark(row);
  const focused = (event: FocusEvent) => {
    const choice = Array.from(row.children).find((one) => one.contains(event.target as Node));
    if (choice !== undefined) reveal(row, choice);
    again();
  };
  row.addEventListener("scroll", again, { passive: true });
  row.addEventListener("focusin", focused);
  const watch = new ResizeObserver(() => {
    reveal(row);
    again();
  });
  watch.observe(row);
  return () => {
    row.removeEventListener("scroll", again);
    row.removeEventListener("focusin", focused);
    watch.disconnect();
  };
}

/**
 * The ref of a scrolling row whose current choice is named by `current`: the choice is revealed
 * when that changes, and the row is marked after every draw, as a count drawn anew changes what
 * it holds.
 */
export function useScrollRow(
  current: string,
): (row: HTMLElement | null) => (() => void) | undefined {
  const held = useRef<HTMLElement | null>(null);
  const attach = useCallback((row: HTMLElement | null) => {
    held.current = row;
    return row === null ? undefined : followRow(row);
  }, []);
  useLayoutEffect(() => {
    void current;
    if (held.current?.isConnected) reveal(held.current);
  }, [current]);
  useLayoutEffect(() => {
    if (held.current?.isConnected) mark(held.current);
  });
  return attach;
}
