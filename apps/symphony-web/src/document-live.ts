// A document's and a folder's page, live, as the Dioxus client's docs_live.rs ran them. A document
// keeps its outline and the bodies of the sections it drew on this device (`cache-document.ts`), and
// an entrypoint's folder page its folder and its intro's, pinned: opened again, the page draws them
// at once (`fromDevice`), marked as possibly out of date, then reads the outline naming its copy's
// tag. A `304` confirms it, at the feed's cursor its header names, and the outline's validators
// tell which bodies are still current, so only the others are read again
// (docs/engineering/analysis/client-cache.md, "A return visit, step by step"). The page then follows the
// document's feed, and reads the bodies of the sections that come near the screen.
//
// What a page holds is a model (`DocumentLive`, `FolderLive`) its loops change and its component
// draws, told of each change. Every loop ends with the signal it is given, checked after each wait.

import {
  answered,
  drawn,
  FRESH,
  type Freshness,
  failed,
  forget,
  type Item,
  keepLater,
  left,
  lost,
  opened,
  readKept,
  visitLater,
  waits,
} from "./cache";
import {
  Bodies,
  documentItem,
  FOLDER,
  type KeptBody,
  OUTLINE,
  readAhead,
  SECTIONS,
  split,
} from "./cache-document";
import {
  type Body,
  bodies,
  cardSlug,
  change,
  type DocumentState,
  documentState,
  drawBodies,
  held as heldSections,
  put,
  refresh,
  settle,
} from "./document";
import {
  readAnchors,
  readChange,
  readFolder,
  readKeptBodies,
  readOutline,
  readSections,
} from "./document-read";
import { back, type Heard, Inbox, openFeed } from "./feed";
import type { FrameRefresh } from "./frame";
import { flushReload, pause, Retry } from "./live";
import { scoped } from "./project";
import { Failure, getDocument, getIf, getJson, said } from "./request";
import { field, SERVER_NOTICE } from "./shared";
import { Backoff, type Ended } from "./state";
// biome-ignore lint/style/noRestrictedImports: no-timed-waits baseline: flushHeld looks every 400 ms whether a held section is free (whenFree and the page's changes are the events), loadSections looks every 50 ms for sections to read and for the feed to catch up and tries a failed read again after 1 s, and followFolder reads its folder again each FOLDER_MS, where a feed event of the folder is the wait
import { retryWait, sleep } from "./timing";
import type {
  DocumentCard,
  DocumentChange,
  DocumentOutline,
  Folder,
  Fragment,
  Section,
} from "./wire";
import { FEED_PATH } from "./wire";

/** The document `path` as the API names it, in its document's `path` and its feed's changes. */
export function named(scope: string, path: string): string {
  return scope === "" ? path : `@${scope}/${path}`;
}

/** The API's `route` for the document `path` of the checkout `scope`. */
export function api(scope: string, route: string, path: string): string {
  return `${scoped(scope, route)}?path=${encodeURIComponent(path)}`;
}

/** Section keys as a query's value: a JSON list, in order. */
export function keysQuery(keys: ReadonlySet<string>): string {
  return encodeURIComponent(JSON.stringify([...keys].sort()));
}

/**
 * The bodies of the drawn `sections`, as the server sent them, by key: each section with, for a
 * card, its card from `cards` (by slug) and its thread from `threads`, as this device keeps a body
 * (`KeptBody`). A section without its blocks has no body.
 */
export function sentBodies(
  sections: readonly Section[],
  cards: ReadonlyMap<string, DocumentCard>,
  threads: Readonly<Record<string, Fragment | null>>,
): Map<string, Body> {
  const sent = new Map<string, Body>();
  for (const section of sections) {
    if (section.blocks.length === 0) continue;
    const body: Body = { section };
    const slug = cardSlug(section.key);
    if (slug !== null) {
      const card = cards.get(slug);
      if (card !== undefined) body.card = card;
      const thread = Object.hasOwn(threads, slug) ? threads[slug] : null;
      if (thread != null) body.thread = thread;
    }
    sent.set(section.key, body);
  }
  return sent;
}

const sameSet = (a: ReadonlySet<string>, b: ReadonlySet<string>): boolean =>
  a.size === b.size && [...a].every((key) => b.has(key));

/** The section key of each element `selector` finds in the page, by its closest section. */
function sectionsOf(selector: string): Set<string> {
  const keys = new Set<string>();
  for (const found of document.querySelectorAll(selector)) {
    const key = found.closest("[data-doc-key]")?.getAttribute("data-doc-key");
    if (key != null) keys.add(key);
  }
  return keys;
}

/**
 * The sections the owner is engaged in (a focused field; an open form, excerpt, gallery or
 * details), and those holding only a closed draft. None where there is no page.
 */
export function busy(): [Set<string>, Set<string>] {
  if (typeof document === "undefined") return [new Set(), new Set()];
  const engaged = sectionsOf(
    "[data-busy=true]:not([data-draft=true]), .section-prose details[open], .history details[open]",
  );
  const active = document.activeElement;
  if (
    active?.matches("input, textarea, select, button[aria-haspopup=listbox], [role=option]") ===
    true
  ) {
    const key = active.closest("[data-doc-key]")?.getAttribute("data-doc-key");
    if (key != null) engaged.add(key);
  }
  return [engaged, sectionsOf("[data-draft=true]")];
}

/** The outline the server last answered, or this device kept, with its tag. */
export interface Copy {
  tag: string;
  outline: DocumentOutline;
}

/**
 * The server's outline at `url`, read naming the tag of `copy`'s: a `304` is that copy again, at
 * the feed's cursor its answer names; a new outline replaces the copy, and this device keeps it in
 * `item`. A `304` that names no cursor opens no feed: the copy goes, and the outline is read again
 * whole. Null when the server has no such document. Once `signal` ends nothing is kept, read again
 * or changed: a read it ends before its answer fails, its fetch given up, and one whose answer was
 * in is null; its caller, which checks `signal` first, draws nothing either way.
 */
export async function readOutlineAt(
  url: string,
  copy: { current: Copy | null },
  item: Item | null,
  signal: AbortSignal,
): Promise<DocumentOutline | null> {
  for (;;) {
    const read = await getDocument(url, copy.current?.tag ?? "", signal);
    if (signal.aborted) return null;
    if (read === null) {
      copy.current = null;
      return null;
    }
    const { answer, cursor } = read;
    if (answer.same) {
      const held = copy.current;
      if (held === null) throw new Failure("uncertain", "The server answered 304 to a plain read");
      if (cursor === null) {
        copy.current = null;
        continue;
      }
      return { ...held.outline, document: { ...held.outline.document, version: cursor } };
    }
    const outline = readOutline(answer.value);
    if (outline === null) throw new Failure("uncertain", "The server's outline could not be read");
    if (item !== null && answer.etag !== "") keepLater(item, OUTLINE, answer.etag, answer.value);
    copy.current = { tag: answer.etag, outline };
    return outline;
  }
}

/** Something a page draws from, which tells its component each time it changed. */
class Model {
  private listeners = new Set<() => void>();
  private version = 0;

  /** For `useSyncExternalStore`: its listener is told of each change. */
  readonly subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  /** For `useSyncExternalStore`: a count that changes with each change. */
  readonly snapshot = (): number => this.version;

  /** The page shows something new. */
  changed(): void {
    this.version += 1;
    for (const listener of this.listeners) listener();
  }
}

/** What a document page reads and follows, and what it shows of it. */
export class DocumentLive extends Model {
  /** The document, once drawn: from this device, or the server's. */
  state: DocumentState | null = null;
  /** Whether the feed is behind: not open yet, or lost. */
  stale = true;
  /** Why the last read failed. */
  trouble: string | null = null;
  /** The server has no document at this path (it answered 404). */
  missing = false;
  /** What the page shows from this device, and whether the server has answered it. */
  freshness: Freshness = FRESH;
  /** The ids each section holds, by key, so a link to one finds it before its body is read. */
  anchors: Record<string, string[]> = {};
  /** The sections that came near the screen, whose bodies the page reads. */
  wanted = new Set<string>();
  /** The sections whose bodies the page holds. */
  loaded = new Set<string>();
  /** How many feed events moved the review queue's cards: their postponed list is read again. */
  orders = 0;
  /** The feed's cursor the page holds the document at. */
  cursor = 0;
  /** How many outlines the server answered: a read of bodies made before the last is dropped. */
  epoch = 0;
  /** The cards the owner just saved in (`settle`). */
  readonly released = new Map<string, number>();
  /**
   * Which section bodies the page holds are current by the server's outline, and so kept on this
   * device and not read again.
   */
  readonly bodies = new Bodies();
  /**
   * Each body the page took, as the server sent it (or this device kept it), by key: what the
   * device is given of a body it keeps (`keepBodies`).
   */
  readonly sent = new Map<string, Body>();
  /** The reader's Retry presses. */
  readonly retry = new Retry();

  constructor(
    readonly path: string,
    readonly scope: string,
    /** Where the page keeps what it drew: its document's item, its folder's for an intro, or none. */
    readonly kept: Item | null,
    readonly frame: FrameRefresh,
  ) {
    super();
  }

  /** The document as the API names it. */
  get name(): string {
    return named(this.scope, this.path);
  }

  /** The section `key` came near the screen: its body is to be read. */
  want(key: string): void {
    if (this.wanted.has(key)) return;
    this.wanted = new Set(this.wanted).add(key);
    this.changed();
  }

  /** The sections the page waits for: wanted, and not yet loaded. */
  loading(): Set<string> {
    return new Set([...this.wanted].filter((key) => !this.loaded.has(key)));
  }

  /** Whether every section that came near the screen has its body. */
  ready(): boolean {
    return this.wanted.size > 0 && [...this.wanted].every((key) => this.loaded.has(key));
  }

  /**
   * The sections the feed is to name, whose bodies it then brings with their changes: those near
   * the screen and those whose bodies the page holds. On a return visit the second are the ones
   * drawn from this device, there before the outline's check answers, while the first name
   * themselves only once their slots have come near the screen, a frame or more later.
   */
  followed(): Set<string> {
    return new Set([...this.wanted, ...this.loaded]);
  }

  /** What holds the shown page now, against `latest` (`held`, document.ts). */
  held(latest: DocumentState["latest"]): Set<string> {
    const [engaged, drafts] = busy();
    return heldSections(engaged, drafts, this.released, this.loading(), latest);
  }

  /** `next` with all but `keys` shown; a card's own save that now shows ends its release. */
  show(next: DocumentState, keys: ReadonlySet<string> = this.held(next.latest)): DocumentState {
    const shown = refresh(next, keys);
    settle(this.released, shown.shown);
    return shown;
  }

  /**
   * Give this device the bodies the page holds as current, when one was taken or let go since it
   * was last given them (`Bodies.unkept`): each as the server sent it, under its validator.
   */
  keepBodies(): void {
    if (this.kept === null) return;
    const taken = this.bodies.unkept();
    if (taken === null) return;
    const kept: Record<string, KeptBody> = {};
    for (const [key, validator] of taken) {
      const body = this.sent.get(key);
      if (body !== undefined) put(kept, key, { validator, body });
    }
    keepLater(this.kept, SECTIONS, "", kept);
  }

  /**
   * One event of the document's feed: its cursor taken, its changes of this document applied, and
   * the frame and the postponed cards told what moved. `false` for an event the page cannot read,
   * which loses the stream.
   */
  heard(value: unknown): boolean {
    this.frame.heard(value);
    const version = field(value, "version");
    if (typeof version !== "number" || !Number.isSafeInteger(version) || version < 0) return false;
    this.cursor = version;
    const changes = field(value, "changes");
    if (!Array.isArray(changes)) return false;
    for (const one of changes) {
      const region = field(one, "region");
      if (region === "queue-item" || region === "queue-item-order") this.orders += 1;
      if (region !== "document") continue;
      const data = field(one, "data");
      const delta = readChange(data);
      if (delta === null) return false;
      if (delta.path !== this.name) continue;
      this.bodies.changed(delta);
      const anchors = readAnchors(field(data, "anchors"));
      if (anchors !== null) {
        const all = { ...this.anchors };
        for (const [key, ids] of Object.entries(anchors)) put(all, key, ids);
        this.anchors = all;
      }
      // A section the page holds whose text the change left as it was keeps its body; any other,
      // the change bringing no body, is read again once the page wants it.
      const sections = (delta.sections ?? []).map((section) => {
        if (section.blocks.length > 0) return section;
        const before = this.state?.latest.sections.find((old) => old.key === section.key);
        if (before !== undefined && before.hash === section.hash && before.blocks.length > 0) {
          return { ...section, blocks: before.blocks, comparison: before.comparison ?? null };
        }
        if (this.loaded.has(section.key)) {
          this.loaded = new Set(this.loaded);
          this.loaded.delete(section.key);
        }
        return section;
      });
      if (this.state !== null) {
        const next = change(this.state, { ...delta, sections });
        this.state = this.show({ ...next, latest: { ...next.latest, version } });
      }
    }
    this.changed();
    return true;
  }
}

/**
 * Draw what this device kept of the document while the page shows nothing yet: its outline and the
 * bodies current by it, marked as drawn from the device; or the first part a page read ahead of
 * this first visit, its bodies with it. A document page counts one more visit first. The kept
 * outline, with its tag, for the first read to name.
 */
export async function fromDevice(page: DocumentLive, signal: AbortSignal): Promise<Copy | null> {
  const item = page.kept;
  if (item === null) return null;
  if (item.id === documentItem(page.name, false).id) visitLater(item);
  const outline = await readKept(item, OUTLINE, readOutline);
  // An entrypoint's intro may have moved to another document since.
  if (signal.aborted || outline === null) return null;
  if (outline.value.document.path !== page.name || page.state !== null) return null;
  let value = outline.value;
  let held: Record<string, KeptBody> = {};
  let sent = new Map<string, Body>();
  let saved: number | null = null;
  const ahead = readAhead(value);
  if (ahead) {
    const first = value.document;
    sent = sentBodies(
      first.sections,
      new Map(first.cards.map((card) => [card.slug, card])),
      first.threads,
    );
    [value, held] = split(value);
    saved = outline.saved;
  } else {
    const sections = await readKept(item, SECTIONS, readKeptBodies);
    if (signal.aborted || page.state !== null) return null;
    if (sections !== null) {
      held = sections.value;
      sent = new Map(Object.entries(sections.value).map(([key, kept]) => [key, kept.body]));
      saved = sections.saved;
    }
  }
  const taken = page.bodies.drawn(value, held);
  if (ahead && taken.length > 0) page.bodies.drawnAhead();
  const drawnKeys = new Set(taken.map((body) => body.section.key));
  for (const [key, body] of sent) if (drawnKeys.has(key)) page.sent.set(key, body);
  let freshness = drawn(page.freshness, OUTLINE, outline.saved);
  if (saved !== null && drawnKeys.size > 0) freshness = drawn(freshness, SECTIONS, saved);
  page.freshness = freshness;
  page.anchors = value.anchors;
  page.loaded = drawnKeys;
  page.state = documentState(drawBodies(value.document, taken));
  page.changed();
  return { tag: outline.etag, outline: value };
}

/** The server answered the document's outline: the page shows it, with the bodies still current. */
export function outlined(page: DocumentLive, outline: DocumentOutline): void {
  page.missing = false;
  page.epoch += 1;
  page.cursor = outline.document.version;
  page.anchors = outline.anchors;
  // The bodies still current by the server's outline stay, and are read no more.
  const current = page.bodies.answered(outline, page.wanted);
  const before = page.state ?? documentState(outline.document);
  const latest = drawBodies(outline.document, bodies(before.latest, current));
  page.loaded = current;
  page.state = page.show({ ...before, latest, removed: false });
  page.keepBodies();
  let freshness = answered(page.freshness, OUTLINE);
  if (page.bodies.confirmed()) freshness = answered(freshness, SECTIONS);
  page.freshness = freshness;
  page.changed();
}

/**
 * The server has no such document: what this device drew of it goes, and a document's own item is
 * forgotten (an entrypoint's stays, its folder still there).
 */
export function vanished(page: DocumentLive): void {
  page.missing = true;
  page.trouble = null;
  if (waits(page.freshness, OUTLINE)) {
    page.state = null;
    page.freshness = left(page.freshness, [OUTLINE, SECTIONS]);
    if (page.kept !== null && !page.kept.pinned) void forget(page.kept);
  }
  page.changed();
}

/**
 * Read the document's outline, follow its feed until the stream is lost, and start again after a
 * pause, until `signal` ends; with it, the loops that read the sections near the screen and show
 * what the owner's work held back once it is free.
 */
export async function documentLive(page: DocumentLive, signal: AbortSignal): Promise<void> {
  void flushHeld(page, signal);
  void loadSections(page, signal);
  const backoff = new Backoff();
  const url = `${api(page.scope, "/api/document", page.path)}&view=outline`;
  const copy = { current: await fromDevice(page, signal) };
  while (!signal.aborted) {
    await back(signal);
    if (signal.aborted) return;
    let ended: Ended = { lost: 0 };
    try {
      const outline = await readOutlineAt(url, copy, page.kept, signal);
      if (signal.aborted) return;
      if (outline === null) {
        vanished(page);
      } else {
        outlined(page, outline);
        ended = await followDocument(page, signal);
        if (signal.aborted) return;
        page.freshness = lost(page.freshness);
      }
    } catch (error) {
      if (signal.aborted) return;
      page.trouble = said(error);
      page.freshness = failed(page.freshness, OUTLINE);
    }
    page.stale = true;
    page.changed();
    await pause((pressed) => retryWait(backoff, ended, pressed), page.retry, signal);
  }
}

/**
 * Show what was held back once the form, excerpt or field that held it is free, the feed having
 * nothing further to say; no read is needed to do it. A reload the reader's work held back goes
 * then too.
 */
async function flushHeld(page: DocumentLive, signal: AbortSignal): Promise<void> {
  let previous = new Set<string>();
  while (!signal.aborted) {
    await sleep(400, signal);
    if (signal.aborted) return;
    flushReload();
    const current = page.state;
    if (current === null || current.shown === current.latest) continue;
    const keys = page.held(current.latest);
    if (keys.size > 0 && sameSet(keys, previous)) continue;
    previous = keys;
    const next = page.show(current, keys);
    if (next !== page.state) {
      page.state = next;
      page.changed();
    }
  }
}

/** How long a read of bodies waits for the feed to reach the cursor it was read at, in ms. */
const CATCH_UP_MS = 2_000;

/**
 * Read the bodies of the sections the page wants and does not hold, once the server's outline has
 * answered: a page drawn from this device does not know yet which of its bodies are current, nor
 * the feed's cursor. A read waits for the feed to catch up to its cursor before it is shown, so an
 * older body never overwrites a later edit the feed already applied.
 */
export async function loadSections(page: DocumentLive, signal: AbortSignal): Promise<void> {
  while (!signal.aborted) {
    await sleep(50, signal);
    if (signal.aborted) return;
    if (page.state === null || page.epoch === 0) continue;
    const missing = page.loading();
    if (missing.size === 0) continue;
    const url = `${api(page.scope, "/api/section", page.path)}&format=blocks&keys=${keysQuery(missing)}`;
    const epoch = page.epoch;
    try {
      const sent = await getJson(url, signal);
      if (signal.aborted) return;
      const parts = readSections(sent);
      if (parts === null) throw new Failure("uncertain", "The server's sections could not be read");
      const deadline = Date.now() + CATCH_UP_MS;
      while (page.cursor < parts.version && Date.now() < deadline && page.epoch === epoch) {
        await sleep(50, signal);
        if (signal.aborted) return;
      }
      if (page.cursor < parts.version || page.epoch !== epoch) continue;
      if (page.state !== null && page.state.latest.version > parts.version) continue;
      took(page, missing, parts.change);
    } catch (error) {
      if (signal.aborted) return;
      page.trouble = said(error);
      page.changed();
      await sleep(1_000, signal);
    }
  }
}

/** The page read the bodies of the sections `asked`: `read` holds them, as a change. */
export function took(page: DocumentLive, asked: ReadonlySet<string>, read: DocumentChange): void {
  const cards = new Map<string, DocumentCard>();
  for (const card of read.cards ?? []) if (card.value != null) cards.set(card.key, card.value);
  for (const [key, body] of sentBodies(read.sections ?? [], cards, read.threads ?? {})) {
    page.sent.set(key, body);
  }
  page.bodies.read(asked, read.sections ?? []);
  page.loaded = new Set([...page.loaded, ...asked]);
  if (page.state !== null) {
    page.state = page.show(change(page.state, read));
    page.keepBodies();
  }
  if (waits(page.freshness, SECTIONS) && page.bodies.confirmed()) {
    page.freshness = answered(page.freshness, SECTIONS);
  }
  page.trouble = null;
  page.changed();
}

/** How long the feed may say nothing before it is taken as lost, in ms. */
const QUIET_MS = 12_500;

/**
 * Follow the document's feed from the page's cursor, for the sections it wants and holds
 * (`followed`; opened again once it wants one the stream does not name), until the stream is lost
 * or the page goes away. How it ended. A return visit opens it once: the sections near the screen
 * are those drawn from this device, which it names from the start.
 *
 * Only its first open tells the frame the stream opened, which reads the asks, the chat and the
 * metadata again: one the page opened again itself names the cursor of the last event it heard,
 * from which the server sends every event after it (or refuses, a loss), so it missed nothing.
 */
async function followDocument(page: DocumentLive, signal: AbortSignal): Promise<Ended> {
  let openedAt: number | null = null;
  subscribe: for (;;) {
    const subscribed = page.followed();
    const url = `${scoped(page.scope, FEED_PATH)}?since=${page.cursor}&document=${encodeURIComponent(page.name)}&sections=${keysQuery(subscribed)}`;
    const inbox = new Inbox<Heard>();
    const close = openFeed(url, (heard) => inbox.push(heard));
    if (close === null) break;
    let lastWord = Date.now();
    try {
      for (;;) {
        let said = await inbox.next(200, signal);
        if (signal.aborted) return "away";
        if (said === null) {
          if ([...page.wanted].some((key) => !subscribed.has(key))) continue subscribe;
          if (Date.now() - lastWord < QUIET_MS) continue;
          said = { kind: "lost" };
        }
        lastWord = Date.now();
        if (said.kind === "open") {
          if (openedAt === null) page.frame.connected();
          openedAt ??= lastWord;
          page.stale = false;
          page.trouble = null;
          page.freshness = opened(page.freshness);
          page.changed();
        } else if (said.kind === "server") {
          SERVER_NOTICE.set(said.message);
        } else if (said.kind === "lost") {
          break subscribe;
        } else if (said.kind === "away") {
          return "away";
        } else if (said.kind === "delta" && !page.heard(said.value)) {
          break subscribe;
        }
      }
    } finally {
      close();
    }
  }
  return { lost: openedAt === null ? 0 : Date.now() - openedAt };
}

/** What a folder page reads, and what it shows of it. */
export class FolderLive extends Model {
  folder: Folder | null = null;
  /** Why the last read failed. */
  error: string | null = null;
  /** The server has no folder at this path. */
  missing = false;
  freshness: Freshness = FRESH;
  /** The reader's Retry presses. */
  readonly retry = new Retry();

  constructor(
    readonly path: string,
    readonly scope: string,
    /** An entrypoint's own page, kept on this device, pinned; none for any other folder. */
    readonly kept: Item | null,
  ) {
    super();
  }
}

/** How often a folder page reads its folder, in ms: it follows no feed. */
const FOLDER_MS = 5_000;

/**
 * Draw the folder this device kept, then read the server's every few seconds, naming the tag of
 * the copy the page holds, until `signal` ends: the page draws again only for a change.
 */
export async function followFolder(page: FolderLive, signal: AbortSignal): Promise<void> {
  let copy: { tag: string; folder: Folder } | null = null;
  const item = page.kept;
  if (item !== null) {
    visitLater(item);
    const held = await readKept(item, FOLDER, readFolder);
    if (signal.aborted) return;
    if (held !== null && page.folder === null) {
      page.folder = held.value;
      page.freshness = drawn(page.freshness, FOLDER, held.saved);
      copy = { tag: held.etag, folder: held.value };
      page.changed();
    }
  }
  const url = `${api(page.scope, "/api/folder", page.path)}&view=outline`;
  while (!signal.aborted) {
    const before = [page.folder, page.missing, page.error, JSON.stringify(page.freshness)];
    try {
      const answer = await getIf(url, copy?.tag ?? "", signal);
      if (signal.aborted) return;
      if (answer.same) {
        if (copy === null)
          throw new Failure("uncertain", "The server answered 304 to a plain read");
      } else {
        const folder = readFolder(answer.value);
        if (folder === null)
          throw new Failure("uncertain", "The server's folder could not be read");
        if (item !== null && answer.etag !== "") keepLater(item, FOLDER, answer.etag, answer.value);
        copy = { tag: answer.etag, folder };
      }
      page.folder = copy.folder;
      page.missing = false;
      page.error = null;
      // The folder follows no feed: the server's answer is all it waits for.
      page.freshness = opened(answered(page.freshness, FOLDER));
    } catch (error) {
      if (signal.aborted) return;
      if (error instanceof Failure && error.kind === "missing") {
        // A folder gone while away shows nothing this device drew of it.
        if (waits(page.freshness, FOLDER)) {
          page.folder = null;
          page.freshness = left(page.freshness, [FOLDER]);
        }
        copy = null;
        page.missing = true;
        page.error = null;
      } else {
        page.error = said(error);
        page.freshness = lost(failed(page.freshness, FOLDER));
      }
    }
    const after = [page.folder, page.missing, page.error, JSON.stringify(page.freshness)];
    if (after.some((one, at) => one !== before[at])) page.changed();
    // The baseline's plain wait (its import says why): a folder follows no feed.
    await pause((pressed) => sleep(FOLDER_MS, pressed), page.retry, signal);
  }
}
