// Where the reader is on the task board, kept through what moves it: the column a live update
// holds in place (the reader's, never merely the first in the page), the focus of a card a live
// update moves to another column (its link in the card's new row), and the board's place in its
// history entry, which Back draws the board at again. What draws the board is board.tsx's.

import { type RefObject, useLayoutEffect, useRef } from "react";
import { useMount } from "./mount";
import { holdPlace } from "./shared";

/** Every card of the board: what a live update holds in place when no column is the reader's. */
export const ROWS = "[data-slug]";

/** A column of the board's pipeline the scroller shows: its status and its left and right edges. */
export type Shown = [status: string, left: number, right: number];

/**
 * The column the reader is in, of those the board's scroller shows (`columns`): the one holding
 * the focus, else the one under the pointer, else the one nearest the scroller's middle (`middle`,
 * as the columns' edges are measured). Null when it shows none.
 */
export function readerColumn(
  focused: string | null,
  hovered: string | null,
  columns: Shown[],
  middle: number,
): string | null {
  const shows = (status: string | null) => columns.some(([one]) => one === status);
  if (focused !== null && shows(focused)) return focused;
  if (hovered !== null && shows(hovered)) return hovered;
  let nearest: Shown | null = null;
  const off = ([, left, right]: Shown) => Math.abs((left + right) / 2 - middle);
  for (const column of columns) {
    if (nearest === null || off(column) < off(nearest)) nearest = column;
  }
  return nearest?.[0] ?? null;
}

/** The status `node`'s column carries, if `node` is in one of the pipeline's columns. */
function columnOf(node: Element | null | undefined): string | null {
  return node?.closest?.(".board-scroll .column[data-status]")?.getAttribute("data-status") ?? null;
}

/** What of the board's scroller is read: its box and what is in it. */
type Surface = Pick<Element, "getBoundingClientRect" | "querySelector" | "querySelectorAll">;

/**
 * Whether the pointer's place counts: a mouse's, which is where the reader is. A touch screen's
 * pointer stays where it last tapped (`:hover` there), wherever the reader swiped since.
 */
function pointerCounts(): boolean {
  return typeof matchMedia === "function" && matchMedia("(hover: hover)").matches;
}

/** The column the reader is in (`readerColumn`) of those the board's `scroller` shows. */
function readerStatus(scroller: Surface): string | null {
  const box = scroller.getBoundingClientRect();
  const columns = [...scroller.querySelectorAll(".column[data-status]")]
    .map((column): Shown => {
      const edges = column.getBoundingClientRect();
      return [column.getAttribute("data-status") ?? "", edges.left, edges.right];
    })
    .filter(([, left, right]) => right > box.left && left < box.right);
  return readerColumn(
    columnOf(document.activeElement),
    pointerCounts() ? columnOf(scroller.querySelector(".column[data-status]:hover")) : null,
    columns,
    (box.left + box.right) / 2,
  );
}

/**
 * The first of what `selector` names in the board's `scroller` whose top is on it, and in the
 * window as well when `windowed`.
 */
function firstOnScreen(scroller: Surface, selector: string, windowed = false): Element | undefined {
  const box = scroller.getBoundingClientRect();
  const top = windowed ? Math.max(box.top, 0) : box.top;
  const bottom = windowed ? Math.min(box.bottom, innerHeight) : box.bottom;
  return [...scroller.querySelectorAll(selector)].find((node) => {
    const edges = node.getBoundingClientRect();
    return (
      edges.top >= top && edges.top < bottom && edges.right > box.left && edges.left < box.right
    );
  });
}

/** `value` quoted for a selector's attribute. */
function quoted(value: string): string {
  return `"${value.replace(/["\\]/g, "\\$&")}"`;
}

/**
 * The card a live update holds in place (the shell's `reviewHoldPlace`): the first of the column
 * the reader is in (`readerColumn`) whose top is on the screen, so a card filed or moved in
 * another column moves only that one, and one filed above the reader in theirs moves none on
 * their screen. Named alone, so the hold looks at no other card to find it. Every card when the
 * board has no scroller, or no card of the reader's column has its top on it.
 */
export function readerRows(): string {
  if (typeof document === "undefined") return ROWS;
  const scroller = document.querySelector(".board-scroll");
  if (scroller === null) return ROWS;
  const status = readerStatus(scroller);
  if (status === null) return ROWS;
  const column = `.board-scroll .column[data-status=${quoted(status)}]`;
  const slug = firstOnScreen(scroller, `${column} ${ROWS}`, true)?.getAttribute("data-slug");
  return typeof slug === "string" ? `${column} [data-slug=${quoted(slug)}]` : ROWS;
}

/** Every row of the board's columns, drawn or waiting for its card (`row-SLUG`). */
const ROW_IDS = '[id^="row-"]';

/** Where the reader's place on the board is kept by: a row on the screen, the column it stands
 * in, and how far below the scroller's top its top stands, in whole px. */
export interface Anchor {
  row: string;
  column: string;
  at: number;
}

/**
 * The row the reader's place on the board's `scroller` is kept by: the first of the reader's
 * column whose top is on the screen, else the first of any column's. The rows above it may be
 * drawn at another height when the board is drawn again (their cards, or their ghosts), so the
 * place is that row's, not a scroll offset.
 */
export function anchorOf(scroller: Surface): Anchor | null {
  const status = readerStatus(scroller);
  const row =
    (status === null
      ? undefined
      : firstOnScreen(scroller, `.column[data-status=${quoted(status)}] ${ROW_IDS}`)) ??
    firstOnScreen(scroller, ROW_IDS);
  const column = row?.closest(".column[data-status]")?.getAttribute("data-status");
  if (row === undefined || column === null || column === undefined) return null;
  const at = row.getBoundingClientRect().top - scroller.getBoundingClientRect().top;
  return { row: row.id.slice("row-".length), column, at: Math.round(at) };
}

/** The row of the task `slug` (`row-SLUG`), wherever it stands. */
export function rowOf(slug: string): string {
  return `[id=${quoted(`row-${slug}`)}]`;
}

/** The row of the task `slug` in the board's column `status` (`.column`, in its scroller), and in
 * no other: a task moved since has its row in another column, or none. What the shell's hold
 * (`reviewHoldPlace`) keeps in place once Back returned to that row. */
export function rowIn(status: string, slug: string): string {
  return `.board-scroll .column[data-status=${quoted(status)}] ${rowOf(slug)}`;
}

/** What in a card takes the focus, in the order Tab reaches it. */
const FOCUSABLE = "a[href], button, summary, [tabindex]";

/** The board's card that holds the focus, if one does. */
function focusedCard(): Element | null {
  if (typeof document === "undefined") return null;
  return document.activeElement?.closest?.(`.kanban ${ROWS}`) ?? null;
}

/** The slug of the board's card that holds the focus, if one does. */
export function focusedSlug(): string | null {
  return focusedCard()?.getAttribute("data-slug") ?? null;
}

/**
 * Which of the task `slug`'s card's links (`FOCUSABLE`) holds the focus, if the focus is in that
 * card: a row drawn while it is, in another column, is the row of that card's new place, and takes
 * the focus over from it (`takeFocus`).
 */
export function focusedLink(slug: string): number | null {
  const card = focusedCard();
  if (card === null || card.getAttribute("data-slug") !== slug) return null;
  return Math.max(
    [...card.querySelectorAll(FOCUSABLE)].indexOf(document.activeElement as Element),
    0,
  );
}

/**
 * Give the focus to link `link` of the task `slug`'s card in its row (`row-SLUG`), its title's when
 * it has fewer, without scrolling to it: the card held it before a live update moved it to another
 * column, which took it down with its old row. Not when the focus went anywhere since.
 */
export function takeFocus(slug: string, link: number): void {
  const active = document.activeElement;
  if (active !== null && active !== document.body) return;
  const card = document.getElementById(`row-${slug}`)?.querySelector(".taskcard");
  const target = card?.querySelectorAll(FOCUSABLE)[link] ?? card?.querySelector("a[href]");
  (target as HTMLElement | null | undefined)?.focus({ preventScroll: true });
}

/**
 * Where the board is scrolled: its scroller's left and top and the window's top, in whole px, and
 * the row on the screen the place is kept by and its column (`anchorOf`), when one is.
 */
export interface Place {
  left: number;
  top: number;
  page: number;
  row?: string;
  column?: string;
  at?: number;
}

/** The board's place a history entry's `state` keeps, if it keeps one. */
export function keptPlace(state: unknown): Place | null {
  const place = (state as { board?: unknown } | null)?.board as Record<string, unknown> | undefined;
  if (typeof place !== "object" || place === null) return null;
  const { left, top, page, row, column, at } = place;
  const whole = (value: unknown): value is number =>
    typeof value === "number" && Number.isFinite(value) && value >= 0;
  const named = (value: unknown): value is string => typeof value === "string" && value !== "";
  if (!whole(left) || !whole(top) || !whole(page)) return null;
  return named(row) && named(column) && whole(at)
    ? { left, top, page, row, column, at }
    : { left, top, page };
}

/** A history entry's `state` with the board's `place` in it, and whatever else it held. */
export function withPlace(state: unknown, place: Place): Record<string, unknown> {
  return { ...(typeof state === "object" && state !== null ? state : {}), board: place };
}

/** Whether two places are the same. */
export function samePlace(a: Place | null, b: Place | null): boolean {
  return (
    a !== null &&
    b !== null &&
    a.left === b.left &&
    a.top === b.top &&
    a.page === b.page &&
    a.row === b.row &&
    a.column === b.column &&
    a.at === b.at
  );
}

/** The scroller whose place is kept: the board's, its box, its rows, its left and top. */
export type Scroller = Surface & { scrollLeft: number; scrollTop: number };

/** How long the reader's scrolling rests before its place is kept, in ms: the history keeps a few
 * entries' writes a second at most. */
export const RESTS_MS = 250;
/** The frames a return to the kept place takes at most while the board's rows come in. */
export const RETURN_FRAMES = 120;
/** What the reader does that ends a return: their own scroll, touch, click or key. */
const GESTURES = ["wheel", "touchstart", "mousedown", "keydown"];

/**
 * The board's place in its history entry: kept as the reader scrolls, once they rest, and at once
 * before a click follows a link or the page goes away, and returned to when the board is drawn
 * from that entry again. Nothing it does goes on once `signal` ends: it never writes the entry of
 * the page the next kind shows.
 */
export class PlaceKeeper {
  private timer: ReturnType<typeof setTimeout> | undefined;
  private returning: AbortController | null = null;
  /** The board's path: an entry of any other is another page's, and never written. */
  private readonly path = location.pathname;

  constructor(
    private readonly scroller: Scroller,
    private readonly signal: AbortSignal,
  ) {}

  /** The board's place now. */
  now(): Place {
    const { scrollLeft, scrollTop } = this.scroller;
    const place = {
      left: Math.round(scrollLeft),
      top: Math.round(scrollTop),
      page: Math.round(scrollY),
    };
    const anchor = anchorOf(this.scroller);
    return anchor === null ? place : { ...place, ...anchor };
  }

  /** Whether the history's entry is still one of the board's. */
  private here(): boolean {
    return location.pathname === this.path;
  }

  /** Keep the place in the entry now: not while returning to it, nor once ended or moved off it. */
  readonly keep = (): void => {
    clearTimeout(this.timer);
    this.timer = undefined;
    if (this.returning !== null || this.signal.aborted || !this.here()) return;
    const place = this.now();
    if (!samePlace(keptPlace(history.state), place)) {
      history.replaceState(withPlace(history.state, place), "");
    }
  };

  /** Keep the place once the reader's scrolling rests for RESTS_MS. */
  readonly later = (): void => {
    clearTimeout(this.timer);
    if (!this.signal.aborted) this.timer = setTimeout(this.keep, RESTS_MS);
  };

  /** Back or Forward: the scrolling not yet kept is the entry left's, and lost; another entry of
   * the board, which goes on drawing it, has its own place to return to. */
  readonly moved = (): void => {
    clearTimeout(this.timer);
    this.timer = undefined;
    if (this.here()) this.back();
  };

  /**
   * Scroll the board sideways to `place`, if it is not there. Its scroller snaps a sideways scroll
   * to a column's start (board.css), and the browser snaps only to a column that shows in the
   * scroller's height: wider than a phone, down the board a short column has ended above the
   * screen, and a scroll to it would snap to another (a phone's columns stand as tall as the
   * longest, each showing at any depth). So it goes sideways at the columns' tops, where each
   * shows, and back down, in one go the screen never draws between.
   */
  private across(place: Place): void {
    const { scroller } = this;
    if (Math.round(scroller.scrollLeft) === place.left) return;
    const down = scroller.scrollTop;
    scroller.scrollTop = 0;
    scroller.scrollLeft = place.left;
    scroller.scrollTop = down;
  }

  /**
   * Scroll the board down to `place`: its row (`row`, if on the page) back where it stood on the
   * screen, else the scroller's top. Whether it is there.
   */
  private down(place: Place, row: Element | null): boolean {
    const { scroller } = this;
    if (row === null || place.at === undefined) {
      scroller.scrollTop = place.top;
      return Math.round(scroller.scrollTop) === place.top;
    }
    const at = place.at;
    const off = () => row.getBoundingClientRect().top - scroller.getBoundingClientRect().top - at;
    scroller.scrollTop += off();
    return Math.abs(off()) < 1;
  }

  /**
   * The row `place` is kept by, if it is drawn in its column on the board's scroller, and whether
   * to wait for it: while that column has not drawn its rows yet (the board's first draw holds
   * none: each column draws them once near the screen). A task moved or gone since has its row in
   * another column, one under the scroller among them, or none: the place is the scroller's
   * offset then, never that row's.
   */
  private kept(place: Place): [row: Element | null, waits: boolean] {
    if (place.row === undefined || place.column === undefined) return [null, false];
    const row = this.scroller.querySelector(rowIn(place.column, place.row));
    if (row !== null) return [row, false];
    const column = this.scroller.querySelector(`.column[data-status=${quoted(place.column)}]`);
    const drawing =
      column !== null &&
      column.querySelector(ROW_IDS) === null &&
      column.querySelector('[data-pending="true"]') !== null;
    return [null, drawing];
  }

  /**
   * Return to the place the entry keeps, if it keeps one: the scroller's at once and each frame
   * until it is there (its rows come in while it reads, the kept one among them), and the
   * window's once the shell's navigation is ready (it puts the window at its top before), for
   * RETURN_FRAMES at most; then the shell's hold keeps the row there while the rows near it draw
   * their cards. A row no longer in its column holds nothing: the scroller's own place is the
   * reader's. The reader's own gesture ends it where they took over.
   */
  back(): void {
    this.returning?.abort();
    const place = keptPlace(history.state);
    if (place === null || this.signal.aborted) return;
    const returning = new AbortController();
    this.returning = returning;
    const end = () => {
      returning.abort();
      if (this.returning === returning) this.returning = null;
    };
    this.signal.addEventListener("abort", end, { signal: returning.signal });
    for (const kind of GESTURES) {
      addEventListener(kind, end, { capture: true, passive: true, signal: returning.signal });
    }
    let frames = 0;
    const step = () => {
      if (returning.signal.aborted) return;
      this.across(place);
      // Until the kept row's column draws its rows, the place is the scroller's top; then the
      // row's, if the row is still in that column.
      const [row, waits] = this.kept(place);
      const down = this.down(place, row) && !waits;
      const ready = document.documentElement.dataset.navigation === "ready";
      if (ready && Math.round(scrollY) !== place.page) scrollTo(scrollX, place.page);
      const there =
        ready &&
        down &&
        Math.round(this.scroller.scrollLeft) === place.left &&
        Math.round(scrollY) === place.page;
      if (there) {
        end();
        if (row !== null && place.row !== undefined && place.column !== undefined) {
          holdPlace(rowIn(place.column, place.row));
        }
      } else if (++frames >= RETURN_FRAMES) end();
      else requestAnimationFrame(step);
    };
    step();
  }
}

/**
 * Keep the place of the board's scroller (`scroller`, while `drawn`) in the board's history entry,
 * and return to it when the board is first drawn from an entry that keeps one: Back to the board
 * draws it where the reader left it, sideways and down its columns.
 */
export function useKeptPlace(scroller: RefObject<HTMLElement | null>, drawn: boolean): void {
  const mount = useMount();
  const first = useRef(true);
  useLayoutEffect(() => {
    const node = scroller.current;
    if (!drawn || node === null) return;
    const controller = new AbortController();
    const signal = AbortSignal.any([controller.signal, mount]);
    const keeper = new PlaceKeeper(node, signal);
    if (first.current) keeper.back();
    first.current = false;
    node.addEventListener("scroll", keeper.later, { passive: true, signal });
    addEventListener("scroll", keeper.later, { passive: true, signal });
    // Before the shell follows a link the click names (its listener is the document's, bubbling).
    document.addEventListener("click", keeper.keep, { capture: true, signal });
    addEventListener("pagehide", keeper.keep, { signal });
    addEventListener("popstate", keeper.moved, { signal });
    return () => controller.abort();
  }, [scroller, drawn, mount]);
}
