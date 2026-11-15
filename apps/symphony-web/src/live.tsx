// The board, live: read from the server's JSON API, then kept current by its feed, one event at a
// time. A lost feed (it errs, falls silent, or sends what the client cannot read) is never trusted
// to resume: the page closes it, shows the board as stale, and starts again from a fresh board.

import {
  createContext,
  type ReactNode,
  useContext,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import {
  answered,
  BOARD,
  boardItem,
  drawn,
  FRESH,
  type Freshness,
  failed,
  type Item,
  keepLater,
  lost,
  opened,
  readKept,
  shown,
  visitLater,
} from "./cache";
import { back, type Heard, Inbox, openFeed, sleep } from "./feed";
import { type FrameRefresh, useFrame } from "./frame";
import { scoped } from "./project";
import { getIf, said } from "./request";
import { engaged } from "./route";
import { field, list, SERVER_NOTICE, STALE, useStore } from "./shared";
import { Backoff, type Ended, reloadFor } from "./state";
import { loadForTab, saveForTab } from "./storage";
import { failed as failedRead, ready, Trouble } from "./trouble";
import { Skeleton } from "./viewport";
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

/**
 * Load the page again when the server serves another client build than this page's own (the
 * shell's module script), once per build; while the reader is in the middle of something, once
 * they are not (`flushReload`). Without the tab's storage to remember it in, never.
 */
export function reloadIfRedeployed(served: string): void {
  if (engaged()) {
    deferredBuild = served;
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

/** Mark that the board is drawn (`board-ready`), as a page that follows it does once it is read. */
export function markBoardReady(): void {
  performance.mark(READY_MARK);
}

/**
 * The feed's events since a read began, for each read still out: an answer read meanwhile is drawn
 * with them replayed onto it. Only the events some read still needs are kept.
 */
export interface Journal {
  version: number;
  events: [number, unknown][];
  /** How many reads are out since each version. */
  readers: Map<number, number>;
}

/** A read's place in the journal, from its start until it is released, answered or not. */
export class ReadCursor {
  readonly since: number;
  private out = true;

  constructor(private readonly journal: Journal) {
    this.since = journal.version;
    journal.readers.set(this.since, (journal.readers.get(this.since) ?? 0) + 1);
  }

  /** The events heard since the read began, in order. */
  events(): unknown[] {
    return this.journal.events
      .filter(([version]) => version > this.since)
      .map(([, value]) => value);
  }

  /** The read is over: the events no read still out needs are dropped. */
  release(): void {
    if (!this.out) return;
    this.out = false;
    const { journal } = this;
    const readers = (journal.readers.get(this.since) ?? 1) - 1;
    if (readers > 0) journal.readers.set(this.since, readers);
    else journal.readers.delete(this.since);
    if (journal.readers.size === 0) {
      journal.events = [];
      return;
    }
    const oldest = Math.min(...journal.readers.keys());
    journal.events = journal.events.filter(([version]) => version > oldest);
  }
}

/** A live update about to be drawn keeps what the reader looks at in place (shell.js). */
export function holdPlace(rows: string): void {
  (window as { reviewHoldPlace?: (rows: string) => void }).reviewHoldPlace?.(rows);
}

/** Waits `ms` before a read is tried again, or less once the reader presses Retry. */
export async function pause(
  ms: number,
  retry: { current: number },
  signal: AbortSignal,
): Promise<void> {
  const asked = retry.current;
  let left = ms;
  while (left > 0 && retry.current === asked && !signal.aborted) {
    const step = Math.min(left, 200);
    await sleep(step, signal);
    left -= step;
  }
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

/** Changes what the page shows from this device; the views under the board read it. */
const FreshnessContext = createContext<(change: (f: Freshness) => Freshness) => void>(() => {});

export function useFreshen(): (change: (f: Freshness) => Freshness) => void {
  return useContext(FreshnessContext);
}

export interface BoardPageProps<B extends PageBoard = PageBoard> {
  scope: string;
  /** Whether the page draws the board this device kept, and keeps the one it reads. */
  kept: boolean;
  /** The page under the board, once the board is drawn: given how many times it was read, and it. */
  children: (refreshed: number, board: B) => ReactNode;
  /** Each feed event the page under the board hears, by whether it moves what it shows. */
  moves?: (delta: unknown) => boolean;
  /**
   * The board as a page that draws it reads it, its server's answer and its kept copy alike
   * (`readPageBoard` by default).
   */
  read?: (value: unknown) => B | null;
  /**
   * Each feed event applied to the board such a page draws: the board after it, or none for an
   * event it cannot read, which loses the feed.
   */
  apply?: (board: B, delta: unknown) => B | null;
}

/**
 * One feed event as a board page hears it: applied to the board it draws by `apply`, when the page
 * draws the whole board and has it, the board after it shown; then the page under it read again
 * (`refresh`) when `moves` says the event moves what it shows. `false` when `apply` cannot read the
 * event, which loses the stream, with nothing shown or read again.
 */
export function hearBoard<B extends PageBoard>(
  delta: unknown,
  board: { current: B | null },
  show: (board: B) => void,
  apply: ((board: B, delta: unknown) => B | null) | undefined,
  moves: ((delta: unknown) => boolean) | undefined,
  refresh: () => void,
): boolean {
  if (apply && board.current) {
    const next = apply(board.current, delta);
    if (next === null) return false;
    show(next);
  }
  if (moves?.(delta)) refresh();
  return true;
}

/**
 * The board this device kept for `item`, as the page reads it, drawn while the server's is read:
 * never over a board the page drew by then, nor once the page is gone. `freshen` says it is drawn
 * from this device, and since when.
 */
export async function drawKept<B extends PageBoard>(
  item: Item,
  read: (value: unknown) => B | null,
  board: { current: B | null },
  show: (board: B) => void,
  freshen: (change: (f: Freshness) => Freshness) => void,
  signal: AbortSignal,
): Promise<void> {
  const copy = await readKept(item, BOARD, read);
  if (copy && !board.current && !signal.aborted) {
    show(copy.value);
    freshen((f) => drawn(f, BOARD, copy.saved));
  }
}

/**
 * A page that follows the board: it draws what this device kept, reads the board, follows its
 * feed, and shows what it heard in `children`, under its notices.
 */
export function BoardPage<B extends PageBoard = PageBoard>({
  scope,
  kept,
  children,
  moves,
  read = readPageBoard as (value: unknown) => B | null,
  apply,
}: BoardPageProps<B>) {
  const frame = useFrame();
  const board = useRef<B | null>(null);
  const [drawnBoard, setDrawnBoard] = useState<B | null>(null);
  const [stale, setStale] = useState(false);
  const [trouble, setTrouble] = useState<string | null>(null);
  const [freshness, setFreshness] = useState<Freshness>(FRESH);
  const [refreshed, setRefreshed] = useState(0);
  const retry = useRef(0);
  const movesRef = useRef(moves);
  movesRef.current = moves;
  const applyRef = useRef(apply);
  applyRef.current = apply;
  const readRef = useRef(read);
  readRef.current = read;
  useEffect(() => {
    const controller = new AbortController();
    const signal = controller.signal;
    const item: Item | null = kept ? boardItem(scope) : null;
    const show = (read: B) => {
      board.current = read;
      setDrawnBoard(read);
    };
    if (item) {
      visitLater(item);
      void drawKept(item, readRef.current, board, show, setFreshness, signal);
    }
    const views: Views = {
      freshen: (change) => setFreshness(change),
      read: () => setRefreshed((count) => count + 1),
      heard: (delta) =>
        hearBoard(delta, board, show, applyRef.current, movesRef.current, () =>
          setRefreshed((count) => count + 1),
        ),
      frame,
    };
    const run = { board, show, setStale, setTrouble, retry, views, read: readRef.current };
    void keepBoard(scope, item, run, signal);
    const minutes = setInterval(flushReload, 60_000);
    return () => {
      controller.abort();
      clearInterval(minutes);
    };
  }, [scope, kept, frame]);
  const onPage = drawnBoard !== null;
  useEffect(() => {
    if (onPage) markBoardReady();
  }, [onPage]);
  const device = shown(freshness);
  return (
    <FreshnessContext.Provider value={setFreshness}>
      <main
        data-ready={onPage ? "board" : ready(false, trouble !== null)}
        data-feed={stale || device ? "stale" : "live"}
        data-cache={device ? "device" : undefined}
      >
        {drawnBoard !== null ? (
          children(refreshed, drawnBoard)
        ) : trouble !== null ? (
          <Trouble
            title="The board could not be loaded"
            text={`${failedRead(trouble)} It is also tried again on its own.`}
            retry={() => {
              retry.current += 1;
            }}
          />
        ) : (
          <Skeleton lines={8} label="Loading the board…" />
        )}
        <PageNotices reconnecting={stale && onPage} device={device} />
      </main>
    </FreshnessContext.Provider>
  );
}

/** What the board's loop changes on the page that runs it. */
export interface BoardRun<B extends PageBoard = PageBoard> {
  board: { current: B | null };
  show(read: B): void;
  setStale(stale: boolean): void;
  setTrouble(said: string): void;
  /** How many times the reader pressed Retry. */
  retry: { current: number };
  views: Views;
  /** The board as the page reads it (`readPageBoard` by default). */
  read?: (value: unknown) => B | null;
}

/** The loop's waits: until the page is back, and between one try and the next. */
export interface BoardWaits {
  back(signal: AbortSignal): Promise<void>;
  pause(ms: number, retry: { current: number }, signal: AbortSignal): Promise<void>;
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
    await waits.pause(backoff.wait(ended), run.retry, signal);
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

const REMOVED_DOCUMENT_NOTICE = "This document was removed. Your draft is kept on this device.";

export interface PageNoticesProps {
  reconnecting: boolean;
  reconnect?: string;
  error?: string;
  documentRemoved?: boolean;
  /** A page drawn from this device's cache: when it was kept, and whether the server is away. */
  device?: [number, boolean] | null;
}

/**
 * One out-of-flow stack for the feed's, the reads' and the server's messages. Its matching end
 * reserve cannot shrink during this mount, so clearing a message at the page's end cannot clamp
 * the reader's scroll position. A page drawn from this device says so in place of reconnecting.
 * Either sets STALE while shown, and only the notices that set it clear it.
 */
export function PageNotices({
  reconnecting,
  reconnect = FEED_NOTICE,
  error = "",
  documentRemoved = false,
  device = null,
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
    <>
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
            {server}
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
        {server !== "" && <p className="pill">{server}</p>}
      </div>
    </>
  );
}

/**
 * The page's measurement of its notice stack: its reserve never shrinks during the mount, and the
 * toasts sit above the stack as it is drawn (`--notice-toast-bottom`), rather than guessing how
 * many lines it takes. What it gives undoes it.
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
    const bottom = box.height > 0 ? innerHeight - box.top + 8 : 66;
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
