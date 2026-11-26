// The board home draws (`/api/board`), read as Rust's serde reads it, and kept current by the
// feed's events one at a time: each changes the regions it names and leaves every other part of
// the board as it was, the same objects, so what draws them is not drawn again.

import { field } from "./shared";
import {
  BOARD_CARDS,
  type Board,
  type Card,
  type Change,
  type Counts,
  type Delta,
  type Picture,
  type Progress,
  type ProgressBar,
  type Region,
} from "./wire";

const isText = (value: unknown): value is string => typeof value === "string";
/** A field serde leaves out by default: missing, or of its type (`null` is neither). */
const optional = (value: unknown, is: (value: unknown) => boolean) =>
  value === undefined || is(value);
/** A field of an `Option`: missing, `null`, or of its type. */
const maybe = (value: unknown, is: (value: unknown) => boolean) =>
  value === null || optional(value, is);
const isBool = (value: unknown) => typeof value === "boolean";
/** A number as Rust reads one into a `u32`. */
const isCount = (value: unknown): value is number =>
  Number.isInteger(value) && (value as number) >= 0 && (value as number) <= 0xffff_ffff;
/** A number as Rust reads one into a `u64`, as far as a double holds it. */
const isVersion = (value: unknown): value is number =>
  Number.isSafeInteger(value) && (value as number) >= 0;
const every = (value: unknown, is: (value: unknown) => boolean) =>
  Array.isArray(value) && value.every(is);
const has = (value: unknown, keys: string[], is: (value: unknown) => boolean) =>
  keys.every((key) => is(field(value, key)));

function isPicture(value: unknown): value is Picture {
  return (
    isText(field(value, "caption")) &&
    maybe(field(value, "src"), isText) &&
    optional(field(value, "waiting"), isBool)
  );
}

function isCard(value: unknown): value is Card {
  return (
    has(value, ["slug", "doc", "doc_title", "title", "why"], isText) &&
    optional(field(value, "postponed"), isBool) &&
    optional(field(value, "summary"), isText) &&
    ["posted", "task"].every((key) => maybe(field(value, key), isText)) &&
    optional(field(value, "pictures"), (pictures) => every(pictures, isPicture))
  );
}

function readCounts(value: unknown): Counts | null {
  const tasks = field(value, "tasks");
  return has(value, ["waiting", "docs_waiting", "spot_check", "answered_today"], isCount) &&
    maybe(tasks, (tasks) => has(tasks, ["in_progress", "in_review", "agents_live"], isCount)) &&
    optional(field(value, "held"), isCount)
    ? { ...(value as Counts), tasks: (tasks as Counts["tasks"] | undefined) ?? null }
    : null;
}

function isBar(value: unknown): value is ProgressBar {
  return (
    has(value, ["key", "name", "page", "title"], isText) &&
    has(value, ["done", "waiting", "total"], isCount)
  );
}

function isRegion(value: unknown): value is Region {
  const legend = field(value, "legend");
  return (
    has(value, ["id", "title", "note"], isText) &&
    every(field(value, "bars"), isBar) &&
    every(legend, isText) &&
    (legend as unknown[]).length === 3
  );
}

function readProgress(value: unknown): Progress | null {
  const regions = field(value, "regions");
  return has(value, ["docs", "decisions_waiting"], isCount) &&
    optional(regions, (regions) => every(regions, isRegion))
    ? { ...(value as Progress), regions: (regions as Region[] | undefined) ?? [] }
    : null;
}

/**
 * The board as this client reads it, or none for a value without what home draws: the server's
 * answer and the copy this device kept alike, as Rust's serde refuses one. A field serde gives a
 * default when it is missing takes it here too.
 */
export function readBoard(value: unknown): Board | null {
  const next = field(value, "next");
  const counts = readCounts(field(value, "counts"));
  const progress = readProgress(field(value, "progress"));
  return isVersion(field(value, "version")) &&
    isText(field(value, "build")) &&
    isCount(field(value, "ping_ms")) &&
    every(next, isCard) &&
    counts !== null &&
    progress !== null
    ? { ...(value as Board), counts, progress }
    : null;
}

/**
 * The regions Rust reads but `task-asks`, which is the desktop app's: that one and any other are
 * ignored, with their data.
 */
const KNOWN = ["card", "queue", "counts", "progress", "client"];

/**
 * A change of the board as Rust reads one: its region named before its data, which a known region
 * must have and of its type. "other" for a region this client does not know; none for what Rust
 * refuses.
 */
function readChange(value: unknown): Change | "other" | null {
  const region = field(value, "region");
  if (typeof value !== "object" || value === null || !isText(region)) return null;
  const keys = Object.keys(value);
  if (keys.includes("data") && keys.indexOf("data") < keys.indexOf("region")) return null;
  if (!KNOWN.includes(region)) return "other";
  if (!keys.includes("data")) return null;
  const data = field(value, "data");
  switch (region) {
    case "card":
      return isCard(data) ? { region, data } : null;
    case "queue":
      return every(data, isCard) ? { region, data: data as Card[] } : null;
    case "counts": {
      const counts = readCounts(data);
      return counts && { region, data: counts };
    }
    case "progress": {
      const progress = readProgress(data);
      return progress && { region, data: progress };
    }
    default: {
      const build = field(data, "build");
      return isText(build) ? { region: "client", data: { build } } : null;
    }
  }
}

/**
 * One event of the feed as this client reads it, the changes of regions it does not know left
 * out; none for one Rust refuses, which loses the feed.
 */
export function readDelta(value: unknown): Delta | null {
  const version = field(value, "version");
  const changes = field(value, "changes");
  if (!isVersion(version) || !Array.isArray(changes)) return null;
  const read = changes.map(readChange);
  if (read.includes(null)) return null;
  return { version, changes: read.filter((change): change is Change => change !== "other") };
}

/** A region of the board an event moved: the card at an index of `next`, or a whole region. */
export type Moved = number | "queue" | "counts" | "progress";

/** What the owner is told when more cards wait on them. */
export interface Notice {
  title: string;
  body: string;
}

/** What an event did: the board after it, the regions it moved in the event's order, the notice. */
export interface Outcome {
  board: Board;
  moved: Moved[];
  /** Set when more cards wait on the owner after the event than before it. */
  notice: Notice | null;
}

/**
 * `delta` applied to `board`. An event at or before the board's version was already applied (the
 * feed replays from `since` on a reconnect), so it changes nothing.
 */
export function apply(board: Board, delta: Delta): Outcome {
  if (delta.version <= board.version) return { board, moved: [], notice: null };
  const next = { ...board, version: delta.version };
  const moved: Moved[] = [];
  for (const change of delta.changes) {
    switch (change.region) {
      case "card": {
        const card = change.data;
        const at = next.next.findIndex(
          (shown) => shown.slug === card.slug && shown.doc === card.doc,
        );
        if (at >= 0) {
          next.next = next.next.map((shown, index) => (index === at ? card : shown));
          moved.push(at);
        }
        break;
      }
      case "queue":
        next.next = change.data.slice(0, BOARD_CARDS);
        moved.push("queue");
        break;
      case "counts":
        next.counts = change.data;
        moved.push("counts");
        break;
      case "progress":
        next.progress = change.data;
        moved.push("progress");
        break;
      default:
        next.build = change.data.build;
    }
  }
  const count = next.counts.waiting;
  const notice =
    count > board.counts.waiting
      ? {
          title: `${count} ${count === 1 ? "card waits" : "cards wait"} on you`,
          body: next.next[0]?.title ?? "",
        }
      : null;
  return { board: next, moved, notice };
}

/**
 * A feed event read and applied to `board`, the owner told when more cards wait on them: the board
 * after it, or none for an event this client cannot read.
 */
export function heard(board: Board, delta: unknown): Board | null {
  const read = readDelta(delta);
  if (read === null) return null;
  const outcome = apply(board, read);
  if (outcome.notice) notify(outcome.notice);
  return outcome.board;
}

/**
 * Tell the owner of `notice` with a system notification, only once they have allowed them and
 * only while the tab is hidden: a page in front of them already shows it. One tag for every
 * notice, so two open tabs, or two events in a row, leave one notification, the latest. A
 * browser whose constructor refuses (Chrome on Android, which shows one only through a service
 * worker) shows none, and the event is applied all the same, as the Dioxus client ignored the
 * error.
 */
export function notify(notice: Notice): void {
  if (typeof Notification === "undefined" || Notification.permission !== "granted") return;
  if (!document.hidden) return;
  try {
    new Notification(notice.title, { body: notice.body, tag: "review-waiting" });
  } catch {
    // No notification here: the page shows the change once it is in front.
  }
}
