// The board, live: read from the server's JSON API, then kept current by its feed, one event at a
// time. A lost feed (it errs, falls silent, or sends what the client cannot read) is never trusted
// to resume: the page closes it, shows the board as stale, and starts again from a fresh board. A
// page that draws the whole board, home's and the queue's, is board-page.tsx's `BoardPage`, over
// this loop.

import { useEffect, useLayoutEffect, useRef } from "react";
import {
  answered,
  BOARD,
  type Freshness,
  failed,
  type Item,
  keepLater,
  lost,
  opened,
} from "./cache";
import { back, type Heard, Inbox, openFeed } from "./feed";
import type { FrameRefresh } from "./frame";
import { taskSlug } from "./links";
import { scoped } from "./project";
import { getIf, said } from "./request";
import { engaged, whenFree } from "./route";
import { field, list, SERVER_NOTICE, STALE, useStore } from "./shared";
import { Backoff, type Ended, reloadFor } from "./state";
import { loadForTab, saveForTab } from "./storage";
import { retryWait } from "./timing";
import { BOARD_PATH, FEED_PATH } from "./wire";
import { deviceNotice, savedAt } from "./words";

/** What a page says while its feed reconnects. */
export const FEED_NOTICE = "Reconnecting: the board may be out of date";
/** The performance mark set once the board is first drawn (the phone test times it). */
const READY_MARK = "board-ready";
/** Where the tab keeps the build it last loaded the page again for (`reloadFor`). */
const RELOADED_FOR = "symphony-ui-reloaded-for";

/** What the page reads of the board: where its feed starts, how often it pings, and its build. */
export interface PageBoard {
  version: number;
  ping_ms: number;
  build: string;
}

/** The board as this client reads it, or none for a value without what it needs. */
export function readPageBoard(value: unknown): PageBoard | null {
  const version = field(value, "version");
  const ping = field(value, "ping_ms");
  const build = field(value, "build");
  return typeof version === "number" && typeof ping === "number" && typeof build === "string"
    ? { version, ping_ms: ping, build }
    : null;
}

let deferredBuild: string | null = null;
/** Whether a reload held back waits for the reader to be done (`whenFree`). */
let watching = false;

/**
 * Load the page again when the server serves another client build than this page's own (the
 * shell's module script), once per build; while the reader is in the middle of something, once
 * they are not (`flushReload`, at each event `whenFree` hears that can end it). Without the tab's
 * storage to remember it in, never.
 */
export function reloadIfRedeployed(served: string): void {
  if (engaged()) {
    deferredBuild = served;
    void watchHeld();
    return;
  }
  const own =
    document.querySelector("script[src^='/assets/symphony-shell-']")?.getAttribute("src") ?? null;
  const key = RELOADED_FOR;
  if (reloadFor(own, served, loadForTab(key)) && saveForTab(key, served)) location.reload();
}

/** A feed event's `client` changes, each naming the build the server serves now: load it. */
export function followClient(delta: unknown): void {
  for (const change of list(field(delta, "changes"))) {
    const build = field(field(change, "data"), "build");
    if (field(change, "region") === "client" && typeof build === "string") {
      reloadIfRedeployed(build);
    }
  }
}

/** A reload the reader's work held back, once they are done with it. */
export function flushReload(): void {
  if (!engaged() && deferredBuild !== null) {
    const build = deferredBuild;
    deferredBuild = null;
    reloadIfRedeployed(build);
  }
}

/** The held reload tried at each event that can end the reader's work, until it is gone: one
 * watch at a time, however many reloads were held meanwhile. */
async function watchHeld(): Promise<void> {
  if (watching) return;
  watching = true;
  try {
    while (deferredBuild !== null) {
      await whenFree();
      flushReload();
    }
  } finally {
    watching = false;
  }
}

/** Mark that the board is drawn (`board-ready`), as a page that follows it does once it is read. */
export function markBoardReady(): void {
  performance.mark(READY_MARK);
}

/**
 * How many times the reader pressed Retry (`current`, which a press counts up), and the waits a
 * press ends at once.
 */
export class Retry {
  private count = 0;
  private readonly presses = new Set<() => void>();

  get current(): number {
    return this.count;
  }

  /** A press: the count moves on, and every wait `pause` began for it ends. */
  set current(count: number) {
    this.count = count;
    for (const pressed of [...this.presses]) pressed();
  }

  /** `pressed` called at each press from now on, until the function it gives is called. */
  heard(pressed: () => void): () => void {
    this.presses.add(pressed);
    return () => {
      this.presses.delete(pressed);
    };
  }
}

/** Waits `wait` (a retry's, `retryWait`, or a baseline site's own) before a read is tried again,
 * or less: until the reader presses Retry, or `signal` ends, either of which ends the signal it
 * gives `wait`. */
export function pause(
  wait: (signal: AbortSignal) => Promise<void>,
  retry: Retry,
  signal: AbortSignal,
): Promise<void> {
  if (signal.aborted) return Promise.resolve();
  const pressed = new AbortController();
  const end = () => pressed.abort();
  const stop = retry.heard(end);
  signal.addEventListener("abort", end, { signal: pressed.signal });
  return wait(pressed.signal).finally(() => {
    stop();
    pressed.abort();
  });
}

/** What the page under the board shows of it, and hears of its feed. */
export interface Views {
  /** Changes what the page shows from this device. */
  freshen(change: (f: Freshness) => Freshness): void;
  /**
   * The board was read: each view reads its own parts again. The feed is followed once what it
   * gives (a promise, for a page that reads before it follows) has settled.
   */
  read(): unknown;
  /** One event of the feed, not yet applied; what it gives is `false` when the page cannot read
   * it, which loses the stream. */
  heard(delta: unknown): unknown;
  frame: FrameRefresh;
}

/**
 * Follow the feed from `since` until its stream is lost: when it errs, when it says nothing for
 * `quiet` ms, or when it sends an event this client cannot read (the board read next has its
 * change); or until the page goes away. How the stream ended.
 */
async function follow<B extends PageBoard>(
  board: { current: B | null },
  setStale: (stale: boolean) => void,
  since: number,
  quiet: number,
  scope: string,
  views: Views,
  signal: AbortSignal,
): Promise<Ended> {
  const inbox = new Inbox<Heard>();
  const close = openFeed(scoped(scope, `${FEED_PATH}?since=${since}&document=`), (heard) =>
    inbox.push(heard),
  );
  if (!close) return { lost: 0 };
  let openedAt: number | null = null;
  try {
    for (;;) {
      const said = (await inbox.next(quiet, signal)) ?? { kind: "lost" };
      if (signal.aborted) return "away";
      if (said.kind === "open") {
        views.frame.connected();
        openedAt = Date.now();
        setStale(false);
        views.freshen(opened);
        continue;
      }
      if (said.kind === "ping") continue;
      if (said.kind === "server") {
        SERVER_NOTICE.set(said.message);
        continue;
      }
      if (said.kind === "lost") break;
      if (said.kind === "away") return "away";
      const version = field(said.value, "version");
      if (typeof version !== "number") break;
      const shown = board.current;
      if (shown && version <= shown.version) continue;
      views.frame.heard(said.value);
      const read = views.heard(said.value);
      if (shown) shown.version = version;
      followClient(said.value);
      if (read === false) break;
    }
  } finally {
    close();
  }
  return { lost: openedAt === null ? 0 : Date.now() - openedAt };
}

/** What the board's loop changes on the page that runs it. */
export interface BoardRun<B extends PageBoard = PageBoard> {
  board: { current: B | null };
  show(read: B): void;
  setStale(stale: boolean): void;
  setTrouble(said: string): void;
  /** How many times the reader pressed Retry, each press ending the pause between two reads. */
  retry: Retry;
  views: Views;
  /** The board as the page reads it (`readPageBoard` by default). */
  read?: (value: unknown) => B | null;
}

/** The loop's waits: until the page is back, and between one try and the next. */
export interface BoardWaits {
  back(signal: AbortSignal): Promise<void>;
  pause(
    wait: (signal: AbortSignal) => Promise<void>,
    retry: Retry,
    signal: AbortSignal,
  ): Promise<void>;
}

/**
 * Read the board, follow its feed until the stream is lost, and start again after a pause, until
 * `signal` ends. After each wait it does nothing more once `signal` has ended: no read, no copy
 * kept on the device, nothing drawn.
 */
export async function keepBoard<B extends PageBoard>(
  scope: string,
  item: Item | null,
  run: BoardRun<B>,
  signal: AbortSignal,
  waits: BoardWaits = { back, pause },
): Promise<void> {
  const backoff = new Backoff();
  while (!signal.aborted) {
    await waits.back(signal);
    if (signal.aborted) return;
    let ended: Ended = { lost: 0 };
    try {
      const reader = run.read ?? (readPageBoard as (value: unknown) => B | null);
      const read = await readBoard(scope, item, signal, reader);
      if (signal.aborted) return;
      reloadIfRedeployed(read.build);
      run.show(read);
      run.views.freshen((f) => answered(f, BOARD));
      await run.views.read();
      if (signal.aborted) return;
      ended = await follow(
        run.board,
        run.setStale,
        read.version,
        Math.floor((read.ping_ms * 5) / 2),
        scope,
        run.views,
        signal,
      );
      if (signal.aborted) return;
      run.views.freshen(lost);
    } catch (error) {
      if (signal.aborted) return;
      run.views.freshen((f) => failed(f, BOARD));
      run.setTrouble(said(error));
    }
    run.setStale(true);
    await waits.pause((pressed) => retryWait(backoff, ended, pressed), run.retry, signal);
  }
}

/**
 * The board, read fresh: its version starts the feed. Kept as `item`'s, when the page keeps it and
 * `signal` has not ended while it was read.
 */
async function readBoard<B extends PageBoard>(
  scope: string,
  item: Item | null,
  signal: AbortSignal,
  reader: (value: unknown) => B | null,
): Promise<B> {
  const answer = await getIf(scoped(scope, BOARD_PATH), "", signal);
  if (answer.same) throw new Error("The server answered 304 to a plain read");
  const read = reader(answer.value);
  if (!read) throw new Error("The board's answer has no version, ping or build");
  if (item && !signal.aborted) keepLater(item, BOARD, answer.etag, answer.value);
  return read;
}

const REMOVED_DOCUMENT_NOTICE = "This document was removed; your draft is kept.";

export interface PageNoticesProps {
  reconnecting: boolean;
  reconnect?: string;
  error?: string;
  documentRemoved?: boolean;
  /** A page drawn from this device's cache: when it was kept, and whether the server is away. */
  device?: [number, boolean] | null;
  /** The page ends in a composer, which on a phone the stack draws under rather than over. */
  composer?: boolean;
}

/**
 * One stack for the feed's, the reads' and the server's messages, out of the page's flow and
 * floating over the window's foot (`board.css`). On a phone, a page that ends in a composer (a
 * task's, its message; a document's, its question: `composer`) draws it in its flow instead, in
 * its reserve's room under that composer, as a band fixed over a phone's page covers whatever
 * scrolls through it. The cost: a phone reader mid-page there sees no notice before the page's
 * end, and what says the page is stale where acting matters is STALE's "Checking…" on Send, Ask
 * and the page's actions. Every other page (the board, home, the queue, the agents, a folder,
 * whose end is its entries) keeps it floating on a phone too: no composer is under it there, and
 * it is their one sign that the page is out of date.
 * The stack and its reserve share one block, so an in-flow stack's place is its reserve's in any
 * parent, a grid's or a flex box's too. The reserve cannot shrink during this mount, so clearing
 * a message at the page's end cannot clamp the reader's scroll position. A page drawn from this
 * device says so in place of reconnecting.
 * Either sets STALE while shown, and only the notices that set it clear it.
 */
export function PageNotices({
  reconnecting,
  reconnect = FEED_NOTICE,
  error = "",
  documentRemoved = false,
  device = null,
  composer = false,
}: PageNoticesProps) {
  const server = useStore(SERVER_NOTICE);
  const notices = useRef<HTMLDivElement>(null);
  const reserve = useRef<HTMLDivElement>(null);
  useEffect(() => noticeGeometry(notices.current, reserve.current), []);
  const deviceSays = device
    ? deviceNotice(savedAt(new Date(device[0]), new Date()), device[1])
    : null;
  const checking = reconnecting || deviceSays !== null;
  const held = useRef(false);
  useLayoutEffect(() => {
    if (held.current !== checking) {
      held.current = checking;
      STALE.set(checking);
    }
  }, [checking]);
  useEffect(
    () => () => {
      if (held.current) STALE.set(false);
    },
    [],
  );
  const visible = checking || error !== "" || server !== "" || documentRemoved;
  const waiting = deviceSays ?? reconnect;
  return (
    <div className={composer ? "page-notice-end under-composer" : "page-notice-end"}>
      <div className="stale page-notices notice-stack" hidden={!visible} ref={notices}>
        {deviceSays !== null ? (
          <p className="pill wait device-notice" role="status">
            {deviceSays}
          </p>
        ) : (
          reconnecting && (
            <p className="pill wait" role="status">
              {reconnect}
            </p>
          )
        )}
        {error !== "" && (
          <p className="pill wait" role="alert">
            {error}
          </p>
        )}
        {documentRemoved && (
          <p className="pill wait document-removed" role="alert">
            {REMOVED_DOCUMENT_NOTICE}
          </p>
        )}
        {server !== "" && (
          <p className="pill wait server-notice" role="status">
            <NoticeText text={server} linked />
          </p>
        )}
      </div>
      <div
        className="feed-notice-space page-notice-reserve notice-stack"
        aria-hidden="true"
        ref={reserve}
      >
        <p className="pill">{waiting}</p>
        {error !== "" && <p className="pill">{error}</p>}
        {documentRemoved && <p className="pill">{REMOVED_DOCUMENT_NOTICE}</p>}
        {server !== "" && (
          <p className="pill">
            <NoticeText text={server} linked={false} />
          </p>
        )}
      </div>
    </div>
  );
}

/** A part of the server's notice: its words, or a task's page it names (`taskSlug`). */
export type NoticePart = string | { path: string; slug: string };

/** What may follow a path in a sentence and is no part of it. */
const TRAILING = /[.,;:)]+$/;

/**
 * The server's notice `text` in its parts: each whitespace-separated word that is a task's page
 * (`/symphony/task/SLUG`, the page of a failed schema update's task) apart, the punctuation after
 * it left in the words, and the words between kept whole.
 */
export function noticeParts(text: string): NoticePart[] {
  const parts: NoticePart[] = [];
  let words = "";
  for (const token of text.split(/(\s+)/)) {
    const trail = TRAILING.exec(token)?.[0] ?? "";
    const path = token.slice(0, token.length - trail.length);
    const slug = /\s/.test(token) ? null : taskSlug(path);
    if (slug === null) {
      words += token;
      continue;
    }
    if (words !== "") parts.push(words);
    parts.push({ path, slug });
    words = trail;
  }
  if (words !== "") parts.push(words);
  return parts;
}

/**
 * The server's notice, each task's page it names drawn as its slug: a link to that very path when
 * `linked`, which is the server's whole path and never takes the project's base, as the server
 * names a task under the project whose board holds it; plain text in the reserve, which only
 * holds the notice's room and is hidden from a reader. Words naming a task are one span, as a pill
 * lays its children out apart; words naming none are the text alone.
 */
function NoticeText({ text, linked }: { text: string; linked: boolean }) {
  const parts = noticeParts(text);
  if (parts.every((part) => typeof part === "string")) return text;
  return (
    <span>
      {parts.map((part, at) =>
        typeof part === "string" ? (
          part
        ) : (
          // biome-ignore lint/suspicious/noArrayIndexKey: a notice may name one task twice
          <NoticeTask key={at} task={part} linked={linked} />
        ),
      )}
    </span>
  );
}

/** A task's page the server's notice names: its slug, linked to that path when `linked`. */
function NoticeTask({ task, linked }: { task: { path: string; slug: string }; linked: boolean }) {
  return linked ? <a href={task.path}>{task.slug}</a> : <span>{task.slug}</span>;
}

/**
 * The page's measurement of its notice stack: its reserve never shrinks during the mount, and the
 * toasts sit above the stack as it is drawn (`--notice-toast-bottom`), rather than guessing how
 * many lines it takes, while it floats over the window; a stack in the page's flow (under a
 * phone's composer) leaves them their own place above the dock. What it gives undoes it.
 */
export function noticeGeometry(
  notice: HTMLElement | null,
  reserve: HTMLElement | null,
): (() => void) | undefined {
  if (!notice || !reserve || typeof ResizeObserver === "undefined") return;
  const root = document.documentElement;
  let maximum = 0;
  const sync = () => {
    const height = reserve.getBoundingClientRect().height;
    if (height > maximum) {
      maximum = height;
      reserve.style.setProperty("min-height", `${height}px`);
    }
    const box = notice.getBoundingClientRect();
    const floats = box.height > 0 && getComputedStyle(notice).position === "fixed";
    const bottom = floats ? innerHeight - box.top + 8 : 66;
    root.style.setProperty("--notice-toast-bottom", `${bottom}px`);
  };
  const observer = new ResizeObserver(sync);
  observer.observe(notice);
  observer.observe(reserve);
  addEventListener("resize", sync);
  sync();
  return () => {
    observer.disconnect();
    removeEventListener("resize", sync);
    root.style.removeProperty("--notice-toast-bottom");
  };
}
