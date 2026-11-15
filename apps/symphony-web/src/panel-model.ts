// The pure half of the panel (the persistent kind `panel`): its activity toasts, what the asks'
// news makes its conversation read again on, what a task save's reply says, the draft the frame and
// the panel share, and which pending cards the bar's steps go to.

import { absentOr, isFlag, isRecord, isText, listOf, maybe, shaped } from "./blocks-read";
import { isAsk } from "./document-read";
import { Failure } from "./request";
import { field, list } from "./shared";
import type { ActivityItem, Ask, Pending, PendingItem } from "./wire";

/** `a` against `b` by their code points, as Rust orders two strings (by their UTF-8 bytes). */
export function codePointOrder(a: string, b: string): number {
  const [x, y] = [[...a], [...b]];
  for (let at = 0; at < Math.min(x.length, y.length); at += 1) {
    const step = (x[at]?.codePointAt(0) ?? 0) - (y[at]?.codePointAt(0) ?? 0);
    if (step !== 0) return step;
  }
  return x.length - y.length;
}

const isActivity = shaped({
  items: listOf(
    shaped({
      id: isText,
      kind: isText,
      path: isText,
      slug: isText,
      ts: isText,
      text: isText,
      href: isText,
      answered: isFlag,
    }),
  ),
});

/** The activity items the frame left on the window (`reviewActivitySnapshot`), or none. */
export function readActivity(value: unknown): ActivityItem[] | null {
  return isActivity(value) ? (value as { items: ActivityItem[] }).items : null;
}

const isPending = shaped({
  items: listOf(shaped({ path: isText, slug: isText, href: isText, branch: maybe(isText) })),
  docs: listOf(isText),
});

/** The pending cards the frame left on the window (`reviewPendingSnapshot`), or none. */
export function readPending(value: unknown): Pending | null {
  return isPending(value) ? (value as Pending) : null;
}

/**
 * The activity ids `value` lists (this device's seen ones, the frame's baseline), each once and in
 * Rust's order, as the panel keeps them; none for anything but a list of ids.
 */
export function readIds(value: unknown): string[] | null {
  if (!listOf(isText)(value)) return null;
  return [...new Set(value as string[])].sort(codePointOrder);
}

/** What the chat draws of `/api/ask?format=blocks&id=`: the conversation, and whom chat goes to. */
export interface AskReply {
  ask: Ask;
  recipient: string | null;
}

/** `/api/ask`'s answer read, or none for one the chat cannot draw. */
export function readAskReply(value: unknown): AskReply | null {
  if (!isRecord(value) || !isAsk(value.ask) || !maybe(isText)(value.recipient)) return null;
  const recipient = typeof value.recipient === "string" ? value.recipient : null;
  return { ask: value.ask as Ask, recipient };
}

/** How long a batch of activity toasts stays up unless dismissed, in ms. */
export const TOAST_MS = 7_000;
/** The most activity toasts up at once; a newer one takes the oldest down. */
export const TOASTS = 3;

/**
 * The activity toasts on screen, oldest first, each with the batch it arrived in. The toasts that
 * arrive together are a batch and leave together, `TOAST_MS` after they arrived: a batch's expiry
 * takes down only its own toasts, never a later batch's.
 */
export interface Toasts {
  readonly shown: readonly (readonly [number, ActivityItem])[];
  /** The last batch's number: the batch an arrival makes is the next. */
  readonly batches: number;
}

export const NO_TOASTS: Toasts = { shown: [], batches: 0 };

/** `items` up as one new batch (`batches` names it, for its expiry), the newest `TOASTS` kept. */
export function arrive(toasts: Toasts, items: readonly ActivityItem[]): Toasts {
  const batch = toasts.batches + 1;
  const shown = [...toasts.shown, ...items.map((item) => [batch, item] as const)];
  return { shown: shown.slice(Math.max(0, shown.length - TOASTS)), batches: batch };
}

/** What is left of `batch` taken down, its time up. */
export function expire(toasts: Toasts, batch: number): Toasts {
  return { ...toasts, shown: toasts.shown.filter(([shown]) => shown !== batch) };
}

/** The toast for `id` taken down, as its dismiss button does. */
export function dismiss(toasts: Toasts, id: string): Toasts {
  return { ...toasts, shown: toasts.shown.filter(([, item]) => item.id !== id) };
}

/**
 * Whether what the page's feed heard of the asks (`moved`, as the frame says it on `review:asks`)
 * concerns the conversation `ask`: the stream opened again (`true`), the chat's recipient changed,
 * or the thread has a new message (a thread names its id and its document's path).
 */
export function concernsThread(moved: unknown, ask: number): boolean {
  if (moved === true) return true;
  if (typeof moved !== "object" || moved === null) return false;
  if (Object.hasOwn(moved, "recipient")) return true;
  return list(field(moved, "threads")).some((thread) => {
    const id = field(thread, "id");
    return (
      typeof id === "number" && typeof field(thread, "path") === "string" && Math.trunc(id) === ask
    );
  });
}

/** Rust's `char::is_whitespace`, the Unicode White_Space property, at either end. */
const ENDS = /^\p{White_Space}+|\p{White_Space}+$/gu;

/** `text` without its leading and trailing whitespace, as Rust's `str::trim` and the server do. */
export function trimmed(text: string): string {
  return text.replace(ENDS, "");
}

/** How many characters `text` holds once trimmed, as the server counts a task's title. */
export function titleLength(text: string): number {
  return [...trimmed(text)].length;
}

/** What a task save's reply says: saved under its slug, or still being checked, and why. */
export type Saved = { saved: string } | { checking: string };

/**
 * A task save's reply read: a slug confirms the task, a pending reply is still being checked. Only
 * a confirmed task clears a draft, so a reply that confirms none fails with an unknown outcome.
 */
export function saveOutcome(reply: unknown): Saved {
  if (
    !isRecord(reply) ||
    !maybe(isText)(reply.slug) ||
    !absentOr(isFlag)(reply.pending) ||
    !maybe(isText)(reply.error)
  ) {
    throw new Failure("uncertain", "The server's answer to the save could not be read");
  }
  if (reply.pending === true) {
    return {
      checking:
        typeof reply.error === "string" ? reply.error : "Checking whether the task was saved…",
    };
  }
  if (typeof reply.slug === "string" && trimmed(reply.slug) !== "") return { saved: reply.slug };
  throw new Failure("uncertain", "The server did not confirm a saved task");
}

/**
 * The `+` sheet's draft, which the frame and the panel share (`reviewQuickDraft` on the window,
 * `review-quick-draft` on the device): whether the sheet is open and on which tab, the task's and
 * the message's words, the conversation it continues (`ask`, of the checkout `ask_scope`), and a
 * task save's request while its outcome is unknown, which a send repeats.
 */
export interface Draft {
  open: boolean;
  task: boolean;
  title: string;
  text: string;
  slug: string;
  priority: string;
  ask: number;
  ask_scope: string;
  request: string;
  request_page: string;
  request_scope: string;
}

/** A draft's fields at their defaults, as a draft that leaves one out has it. */
const EMPTY: Draft = {
  open: false,
  task: false,
  title: "",
  text: "",
  slug: "",
  priority: "",
  ask: 0,
  ask_scope: "",
  request: "",
  request_page: "",
  request_scope: "",
};

/** The draft of a device that kept none: a task's priority is p2. */
export const NEW_DRAFT: Draft = { ...EMPTY, priority: "2" };

/**
 * The draft `value` holds: each field it names of its type and the rest at their defaults, or none
 * when a field is of another type (a conversation's id a whole number) or `value` is no object.
 */
export function readDraft(value: unknown): Draft | null {
  if (!isRecord(value)) return null;
  const draft: Record<string, unknown> = { ...EMPTY };
  for (const [name, empty] of Object.entries(EMPTY)) {
    const given = value[name];
    if (given === undefined) continue;
    const fits =
      typeof empty === "number" ? Number.isSafeInteger(given) : typeof given === typeof empty;
    if (!fits) return null;
    draft[name] = given;
  }
  return draft as unknown as Draft;
}

/** Whether two drafts hold the same, field for field. */
export function sameDraft(a: Draft, b: Draft): boolean {
  return (Object.keys(EMPTY) as (keyof Draft)[]).every((name) => a[name] === b[name]);
}

/** Whether an activity item is read: answered, or seen on this device. */
export function isRead(item: ActivityItem, seen: readonly string[] | null): boolean {
  return item.answered || (seen?.includes(item.id) ?? false);
}

/**
 * The pending cards the bar's `← prev` and `next →` go to from the reader's place on the page
 * `path` (`@scope/path` for another checkout's): on this page, the card the address names while it
 * is on the screen (`height` tall), else the last card scrolled above the top and the first below
 * it (`top` says where a card is, none for a card not drawn); past this page's, the cards of the
 * documents before and after it in the pending list's order.
 */
export function pendingTargets(
  path: string,
  pending: Pending,
  top: (slug: string) => number | null,
  anchor: string,
  height: number,
): [PendingItem | null, PendingItem | null] {
  // A document the list does not name comes before every one it does, as Rust orders a `None`.
  const rank = (of: string) => pending.docs.indexOf(of);
  const here = rank(path);
  const mine = pending.items.filter((item) => item.path === path);
  const at = mine.findIndex((item) => {
    if (item.slug !== anchor) return false;
    const y = top(item.slug);
    return y !== null && y > 0 && y < height;
  });
  const above = (item: PendingItem) => (top(item.slug) ?? Number.POSITIVE_INFINITY) < 40;
  const below = (item: PendingItem) => (top(item.slug) ?? Number.NEGATIVE_INFINITY) > 100;
  const prev =
    (at >= 0 ? mine[at - 1] : mine.findLast(above)) ??
    pending.items.findLast((item) => rank(item.path) < here) ??
    null;
  const next =
    (at >= 0 ? mine[at + 1] : mine.find(below)) ??
    pending.items.find((item) => rank(item.path) > here) ??
    null;
  return [prev, next];
}
