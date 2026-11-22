// The client cache: what a page drew, kept on this device so a return visit draws at once and then
// asks the server only whether it changed (docs/engineering/analysis/client-cache.md). The store is the shell's
// (shell/cache.ts, `symphonyCache`), which every page kind shares; this names what
// is kept, under the wire's CACHE_FORMAT, and reads it back. Without the store (a browser with no
// IndexedDB) every read misses and nothing is kept.

import { project } from "./project";
import { CACHE_FORMAT } from "./wire";

/**
 * Which caps an item counts against (`CAPS` in shell/cache.ts); the board's reads and the reports
 * (`reportsItem`, reports.tsx) are outside both, held by the origin's bytes and their age alone.
 */
export type Kind = "board" | "task" | "document" | "reports";

/** What one visit opens, and eviction drops whole: its parts are the reads its page drew. */
export interface Item {
  kind: Kind;
  /** Unique in its project: the kind's name, then what it names. */
  id: string;
  /** A project's entrypoint page, kept while the project's cache has room. */
  pinned: boolean;
}

/** The board, the queue and home of the checkout `scope` names (`""`: the main checkout). */
export function boardItem(scope: string): Item {
  return { kind: "board", id: `board:${scope === "" ? "" : `@${scope}/`}`, pinned: false };
}

/** The task `slug`'s page of the checkout `scope` names. */
export function taskItem(scope: string, slug: string): Item {
  return { kind: "task", id: `task:${scope === "" ? "" : `@${scope}/`}${slug}`, pinned: false };
}

/** The parts of a task item: its page (`/api/task-page`), and its thread's newest page
 * (`/api/task-history` without `before`). */
export const PAGE = "page";
export const THREAD = "thread";

/** The parts of a board item: `/api/board`, as home and the queue drew it, and `/api/queue`. */
export const BOARD = "board";
export const QUEUE = "queue";
/** The task board's counts (`/api/task-board?counts=1`), and the machines' work that places it. */
export const COUNTS = "counts";
export const WORK = "work";

/** A task board column's first page. */
export function column(status: string): string {
  return `column:${status}`;
}

/**
 * Whether a read of the task board's listing is the default view's, the one kept: no search,
 * finished tasks of the last 24 hours, stages by the machines' activity.
 */
export function keptView(query: string, days: number, activity: number): boolean {
  return query === "" && days === 1 && activity === 1;
}

/**
 * The columns a task board with these counts may have kept: each status its listing names (its
 * counts' when it names none), the machines' two stages and the owner's sign-off, once each.
 */
export function columns(statuses: string[], counts: [string, number][]): string[] {
  const named = statuses.length === 0 ? counts.map(([status]) => status) : statuses;
  return [...new Set([...named, "build", "test", "signoff"])];
}

/** A kept read: its value, the tag the server answered it with, and when it was kept (ms). */
export interface Kept<T> {
  value: T;
  etag: string;
  saved: number;
}

interface Store {
  read(format: number, project: string, id: string, part: string): Promise<unknown>;
  keep(
    format: number,
    project: string,
    id: string,
    kind: string,
    part: string,
    etag: string,
    value: unknown,
    size: number | undefined,
    ahead: boolean,
  ): Promise<unknown>;
  visit(
    format: number,
    project: string,
    id: string,
    kind: string,
    pinned: boolean,
  ): Promise<unknown>;
  forget(format: number, project: string, id: string): Promise<unknown>;
}

function store(): Store | undefined {
  return (globalThis as { symphonyCache?: Store }).symphonyCache;
}

/** The page's project, as its other kept values name it. */
function projectId(): string {
  return (project().base ?? "").replace(/^\/+/, "");
}

/** What the store's call gave, or nothing: no store, a failure, or nothing kept. */
async function answer(call: (store: Store) => Promise<unknown>): Promise<unknown> {
  const found = store();
  if (!found) return null;
  try {
    return (await call(found)) ?? null;
  } catch {
    return null;
  }
}

/**
 * `part` of `item` as kept, read by `parse` (null for a value this client cannot read). A value it
 * cannot read is a miss, and its item is forgotten, so the page fetches it whole.
 */
export async function readKept<T>(
  item: Item,
  part: string,
  parse: (value: unknown) => T | null,
): Promise<Kept<T> | null> {
  const kept = (await answer((s) => s.read(CACHE_FORMAT, projectId(), item.id, part))) as {
    value?: unknown;
    etag?: unknown;
    saved?: unknown;
  } | null;
  if (kept === null || typeof kept.etag !== "string" || typeof kept.saved !== "number") return null;
  const value = parse(kept.value);
  if (value === null) {
    await forget(item);
    return null;
  }
  return { value, etag: kept.etag, saved: kept.saved };
}

/** Keep `value`, answered with `etag`, as `part` of a visited `item`, the page not waiting. */
export function keepLater(item: Item, part: string, etag: string, value: unknown): void {
  void answer((s) =>
    s.keep(CACHE_FORMAT, projectId(), item.id, item.kind, part, etag, value, undefined, false),
  );
}

/** The page of `item` was opened: one more visit, the page not waiting. */
export function visitLater(item: Item): void {
  void answer((s) => s.visit(CACHE_FORMAT, projectId(), item.id, item.kind, item.pinned));
}

/** Drop `item` and every part of it. */
export async function forget(item: Item): Promise<void> {
  await answer((s) => s.forget(CACHE_FORMAT, projectId(), item.id));
}

/**
 * Whether a page shows what this device kept, and so whether its actions wait. A page drawn from
 * the cache says so until its feed is open and the server has answered every kept read it drew;
 * the page asks again each read that fails until it answers. It says the server is not reachable
 * while a read it made fails and its feed is not open. Each change gives a new value.
 */
export interface Freshness {
  /** When the oldest copy drawn was kept, ms since the epoch, while one is shown. */
  readonly saved: number | null;
  /** The parts drawn from the cache the server has not answered yet. */
  readonly unanswered: readonly string[];
  /** The reads whose last try failed, by part. */
  readonly failing: readonly string[];
  readonly open: boolean;
}

export const FRESH: Freshness = { saved: null, unanswered: [], failing: [], open: false };

const settle = (f: Freshness): Freshness =>
  f.open && f.unanswered.length === 0 ? { ...f, saved: null } : f;

/** `part`, kept at `saved`, was drawn from the cache. */
export function drawn(f: Freshness, part: string, saved: number): Freshness {
  return {
    ...f,
    saved: f.saved === null ? saved : Math.min(f.saved, saved),
    unanswered: f.unanswered.includes(part) ? f.unanswered : [...f.unanswered, part],
  };
}

/** The server answered `part`, and the page shows its answer. */
export function answered(f: Freshness, part: string): Freshness {
  return settle({
    ...f,
    unanswered: f.unanswered.filter((one) => one !== part),
    failing: f.failing.filter((one) => one !== part),
  });
}

/** The read of `part` went unanswered. */
export function failed(f: Freshness, part: string): Freshness {
  return f.failing.includes(part) ? f : { ...f, failing: [...f.failing, part] };
}

/** The feed is open: the server answers, and the reads that failed are being asked again. */
export function opened(f: Freshness): Freshness {
  return settle({ ...f, open: true });
}

/** The feed was lost. */
export function lost(f: Freshness): Freshness {
  return f.open ? { ...f, open: false } : f;
}

/**
 * The page no longer shows `parts`, answered or not: it opened another of its routes (another
 * document), or a search replaced the board's columns. None is shown or asked for any more. With
 * nothing else drawn left unanswered, nothing it shows came from the device.
 */
export function left(f: Freshness, parts: readonly string[]): Freshness {
  const unanswered = f.unanswered.filter((one) => !parts.includes(one));
  return {
    ...f,
    saved: unanswered.length === 0 ? null : f.saved,
    unanswered,
    failing: f.failing.filter((one) => !parts.includes(one)),
  };
}

/** Whether `part` was drawn from the cache and the server has not answered it yet. */
export function waits(f: Freshness, part: string): boolean {
  return f.unanswered.includes(part);
}

/**
 * While the page shows a copy from this device: when the oldest was kept, and whether the server
 * is not reachable.
 */
export function shown(f: Freshness): [number, boolean] | null {
  return f.saved === null ? null : [f.saved, !f.open && f.failing.length > 0];
}
