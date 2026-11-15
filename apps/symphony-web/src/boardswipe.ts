// One column at most for a phone's swipe across the task board. board.css (`max-width: 600px`)
// snaps the board's scroller to each column, `scroll-snap-stop: always`, but a long or fast
// flick's momentum still carries Chrome past the column next to where the finger came down. Once
// such a scroll ends more than one column from where it began, the board goes on to the column
// next to that one, in the swipe's direction. A mouse's or a key's scroll lands where it lands, as
// does every scroll of a board that does not snap to one column at a time (a desk, a narrow
// window). A press the browser takes to scroll ends in `pointercancel`; one that scrolled nothing,
// a tap, ends in `pointerup` and is forgotten there, so the next scroll to end, a key's, a focus
// move's or a script's (Back or Forward's, `boardplace.ts`), lands where it lands too.

import { type RefObject, useEffect } from "react";
import { useMount } from "./mount";

/** Of the columns whose left edges are `lefts`, the one whose edge stands nearest `edge`; null
 * with none. */
export function nearestColumn(lefts: number[], edge: number): number | null {
  let nearest: number | null = null;
  let off = Number.POSITIVE_INFINITY;
  lefts.forEach((left, index) => {
    if (Math.abs(left - edge) < off) {
      nearest = index;
      off = Math.abs(left - edge);
    }
  });
  return nearest;
}

/** Where a swipe that began on the column `from` and ended on `landed` goes on to: the column next
 * to `from`, in its direction, when it passed more than one; null when it passed one at most. */
export function swipeTarget(from: number | null, landed: number | null): number | null {
  if (from === null || landed === null || Math.abs(landed - from) <= 1) return null;
  return from + Math.sign(landed - from);
}

/** What of the board's scroller is read and moved. */
export type Board = Pick<
  HTMLElement,
  "getBoundingClientRect" | "querySelectorAll" | "scrollBy" | "clientLeft"
>;

/** A board's swipes held to one column each: `down` for each press on it, `up` for each press let
 * go with no scroll, `ended` for each end of its scroll. `snaps` says whether the board snaps to
 * one column at a time. */
export class SwipeLimit {
  /** The column a finger's or a pen's press came down on, until its scroll ends or it is let go
   * having scrolled nothing. */
  private from: number | null = null;

  constructor(
    private readonly board: Board,
    private readonly snaps: () => boolean,
  ) {}

  /** Each column's left edge, and the edge of the board's scroller they snap to. */
  private columns(): { lefts: number[]; edge: number } {
    return {
      lefts: [...this.board.querySelectorAll(":scope > .pipeline > .column")].map(
        (column) => column.getBoundingClientRect().left,
      ),
      edge: this.board.getBoundingClientRect().left + this.board.clientLeft,
    };
  }

  /** A press on the board: a finger's or a pen's on a board that snaps begins a swipe from the
   * column it shows. */
  down = (event: { pointerType: string }): void => {
    if (event.pointerType === "mouse" || !this.snaps()) {
      this.from = null;
      return;
    }
    const { lefts, edge } = this.columns();
    this.from = nearestColumn(lefts, edge);
  };

  /** A press let go, not taken to scroll (a tap): no swipe began, and the scroll that ends next is
   * not its. */
  up = (): void => {
    this.from = null;
  };

  /** The board's scroll ended: a swipe that passed more than one column goes on to the one next to
   * where it began. */
  ended = (): void => {
    const from = this.from;
    this.from = null;
    if (from === null) return;
    const { lefts, edge } = this.columns();
    const target = swipeTarget(from, nearestColumn(lefts, edge));
    const left = target === null ? undefined : lefts[target];
    if (left !== undefined) this.board.scrollBy({ left: left - edge, behavior: "smooth" });
  };
}

/** Hold each swipe across the board's `scroller` to one column while it is `drawn`. */
export function useOneColumnASwipe(scroller: RefObject<HTMLElement | null>, drawn: boolean): void {
  const mount = useMount();
  useEffect(() => {
    const node = scroller.current;
    if (!drawn || node === null) return;
    const controller = new AbortController();
    const signal = AbortSignal.any([controller.signal, mount]);
    const limit = new SwipeLimit(node, () =>
      getComputedStyle(node).scrollSnapType.includes("mandatory"),
    );
    node.addEventListener("pointerdown", limit.down, { passive: true, signal });
    node.addEventListener("pointerup", limit.up, { passive: true, signal });
    node.addEventListener("scrollend", limit.ended, { signal });
    return () => controller.abort();
  }, [scroller, drawn, mount]);
}
