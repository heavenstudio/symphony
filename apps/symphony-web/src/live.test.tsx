import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import { BoardPage, drawKept, hearBoard } from "./board-page";
import { BOARD, boardItem, FRESH, type Freshness } from "./cache";
import {
  type BoardRun,
  FEED_NOTICE,
  flushReload,
  followClient,
  keepBoard,
  noticeGeometry,
  noticeParts,
  type PageBoard,
  PageNotices,
  pause,
  Retry,
  readPageBoard,
  reloadIfRedeployed,
} from "./live";
import { SERVER_NOTICE } from "./shared";
import { Backoff, NEVER_ANSWERED } from "./state";
import {
  asking,
  CASES,
  memoryStorage,
  observers,
  feeds as pageFeeds,
  reply,
  say,
  server,
  turn,
  until,
} from "./testing";
import { retryWait } from "./timing";
import { CACHE_FORMAT, FEED_EVENT, PING_EVENT } from "./wire";

afterEach(() => {
  vi.unstubAllGlobals();
  SERVER_NOTICE.set("");
});

describe("the board a page reads", () => {
  it("is where its feed starts, how often it pings, and its build", () => {
    expect(readPageBoard(CASES.samples.board)).toEqual({
      version: 7,
      ping_ms: 15_000,
      build: "/assets/symphony-shell-0123456789abcdef.js",
    });
  });

  it.each([
    ["nothing", null],
    ["no version", { ping_ms: 1, build: "" }],
    ["a ping of another type", { version: 1, ping_ms: "1", build: "" }],
    ["no build", { version: 1, ping_ms: 1 }],
  ])("is none for %s", (_, value) => {
    expect(readPageBoard(value)).toBeNull();
  });
});

describe("the page's notices", () => {
  const notices = (props: Parameters<typeof PageNotices>[0]) =>
    renderToStaticMarkup(<PageNotices {...props} />);

  it("are hidden while there is nothing to say, their reserve as tall as one", () => {
    const markup = notices({ reconnecting: false });
    expect(markup).toContain('<div class="stale page-notices notice-stack" hidden="">');
    expect(markup).toContain(`<p class="pill">${FEED_NOTICE}</p>`);
  });

  it("share one block with their reserve, the stack first, so its place is the reserve's", () => {
    const markup = notices({ reconnecting: true });
    expect(markup).toMatch(
      /^<div class="page-notice-end"><div class="stale page-notices notice-stack">.*<\/div><div class="feed-notice-space page-notice-reserve notice-stack" aria-hidden="true">.*<\/div><\/div>$/,
    );
  });

  it("are marked to go under the composer only on a page that ends in one", () => {
    expect(notices({ reconnecting: true, composer: true })).toMatch(
      /^<div class="page-notice-end under-composer"><div class="stale page-notices/,
    );
    expect(notices({ reconnecting: true, composer: false })).toMatch(
      /^<div class="page-notice-end"><div class="stale page-notices/,
    );
  });

  it("say the feed reconnects, a read's error and the server's notice", () => {
    SERVER_NOTICE.set("Restarting");
    const markup = notices({ reconnecting: true, error: "No answer" });
    expect(markup).not.toContain('hidden=""');
    expect(markup).toContain(`<p class="pill wait" role="status">${FEED_NOTICE}</p>`);
    expect(markup).toContain('<p class="pill wait" role="alert">No answer</p>');
    expect(markup).toContain('<p class="pill wait server-notice" role="status">Restarting</p>');
  });

  it("say a page is drawn from this device in place of reconnecting", () => {
    const markup = notices({ reconnecting: true, device: [Date.now(), true] });
    expect(markup).toContain('class="pill wait device-notice" role="status">Not reachable');
    expect(markup).not.toContain(`role="status">${FEED_NOTICE}`);
  });

  it("say a document was removed", () => {
    expect(notices({ reconnecting: false, documentRemoved: true })).toContain(
      "This document was removed.",
    );
  });

  it("link the task the server's notice names by its slug, to the very path it names", async () => {
    vi.resetModules();
    vi.stubGlobal("document", {
      getElementById: () => ({ textContent: '{"name":"","register":"","base":"/alpha"}' }),
    });
    const live = await import("./live");
    const shared = await import("./shared");
    shared.SERVER_NOTICE.set(
      "This board is at schema 24, ahead of this server's 23. Its update's task: /symphony/task/tests/release-red-update-alpha-24.",
    );
    const markup = renderToStaticMarkup(<live.PageNotices reconnecting={false} />);
    shared.SERVER_NOTICE.set("");
    expect(markup).toContain(
      '<p class="pill wait server-notice" role="status"><span>This board is at schema 24, ahead of this server&#x27;s 23. Its update&#x27;s task: <a href="/symphony/task/tests/release-red-update-alpha-24">tests/release-red-update-alpha-24</a>.</span></p>',
    );
    expect(markup, "never under the page's own project").not.toContain('href="/alpha/');
    // The reserve holds the same words at the same width, with nothing a reader could reach.
    expect(markup).toContain(
      '<p class="pill"><span>This board is at schema 24, ahead of this server&#x27;s 23. Its update&#x27;s task: <span>tests/release-red-update-alpha-24</span>.</span></p>',
    );
  });

  it("leave a notice naming no task as its words", () => {
    SERVER_NOTICE.set("Restarting: see /docs/RUNS.md and https://h/task/x");
    const markup = notices({ reconnecting: false });
    expect(markup).not.toContain("<a ");
    expect(markup).toContain("Restarting: see /docs/RUNS.md and https://h/task/x</p>");
  });
});

describe("the server's notice in its parts", () => {
  it("sets each task's page apart, the punctuation after it left in the words", () => {
    expect(noticeParts("Its task: /task/tools/x), then /symphony/task/tests/y; done")).toEqual([
      "Its task: ",
      { path: "/task/tools/x", slug: "tools/x" },
      "), then ",
      { path: "/symphony/task/tests/y", slug: "tests/y" },
      "; done",
    ]);
    expect(noticeParts("/task/a /task/b")).toEqual([
      { path: "/task/a", slug: "a" },
      " ",
      { path: "/task/b", slug: "b" },
    ]);
  });

  it("keeps words with no task's page whole, a path of another host or page among them", () => {
    for (const text of [
      "",
      "Restarting",
      "see /docs/TASKS.md, then //evil.example/task/x and task/x.",
      "a task: /task/.",
    ]) {
      expect(noticeParts(text), JSON.stringify(text)).toEqual(text === "" ? [] : [text]);
    }
  });
});

describe("a page that follows the board", () => {
  it("opens on the board's skeleton, its feed live until it is lost", () => {
    const markup = renderToStaticMarkup(
      <BoardPage scope="" kept>
        {() => <p>page</p>}
      </BoardPage>,
    );
    expect(markup).toContain('<main data-ready="loading" data-feed="live">');
    expect(markup).toContain("Loading the board…");
    expect(markup).not.toContain("<p>page</p>");
  });
});

describe("a feed event a board page hears", () => {
  /** A board a page draws whole: its version and a count the events change. */
  type Counted = PageBoard & { waiting: number };
  const counted = (version: number, waiting: number): Counted => ({
    version,
    ping_ms: 15_000,
    build: "/b.js",
    waiting,
  });
  /** Each event a number of cards: the board after it, or none for any other event. */
  const apply = (board: Counted, delta: unknown): Counted | null =>
    typeof delta === "number" ? { ...board, waiting: delta } : null;

  /** What a page did on hearing `delta`: what it said, what it showed, how often it read again. */
  const hearing = (
    delta: unknown,
    board: Counted | null,
    given: { apply?: typeof apply; moves?: (delta: unknown) => boolean },
  ) => {
    const shown: Counted[] = [];
    let refreshed = 0;
    const said = hearBoard(
      delta,
      { current: board },
      (next) => shown.push(next),
      given.apply,
      given.moves,
      () => {
        refreshed += 1;
      },
    );
    return { said, shown, refreshed };
  };

  it("shows the board after it where the page draws the board", () => {
    expect(hearing(4, counted(2, 3), { apply })).toEqual({
      said: true,
      shown: [counted(2, 4)],
      refreshed: 0,
    });
  });

  it("loses the stream on an event the page cannot read, showing and reading nothing", () => {
    const moves = vi.fn(() => true);
    expect(hearing("unread", counted(2, 3), { apply, moves })).toEqual({
      said: false,
      shown: [],
      refreshed: 0,
    });
    expect(moves).not.toHaveBeenCalled();
  });

  it("applies nothing before the page has a board, and hears the event all the same", () => {
    expect(hearing("unread", null, { apply })).toEqual({ said: true, shown: [], refreshed: 0 });
  });

  it("reads the page under it again only for an event that moves it", () => {
    const moves = (delta: unknown) => delta === 5;
    expect(hearing(5, counted(2, 3), { apply, moves })).toEqual({
      said: true,
      shown: [counted(2, 5)],
      refreshed: 1,
    });
    expect(hearing(6, counted(2, 3), { apply, moves }).refreshed).toBe(0);
  });

  it("is the queue's with no apply: nothing shown, every event heard", () => {
    expect(hearing("any", counted(2, 3), { moves: () => true })).toEqual({
      said: true,
      shown: [],
      refreshed: 1,
    });
    expect(hearing("any", counted(2, 3), {})).toEqual({ said: true, shown: [], refreshed: 0 });
  });
});

describe("the board a page draws from this device", () => {
  const kept = { value: CASES.samples.board, etag: '"b1"', saved: 1_000 };
  /** What a page did with what the store kept for its board: what it showed, and said of it. */
  const drawing = async (stored: unknown, drew: PageBoard | null = null, gone = false) => {
    const store = { read: vi.fn(async () => stored), forget: vi.fn(async () => {}) };
    vi.stubGlobal("symphonyCache", store);
    const page = new AbortController();
    if (gone) page.abort();
    const shown: PageBoard[] = [];
    let freshness: Freshness = FRESH;
    await drawKept(
      boardItem(""),
      readPageBoard,
      { current: drew },
      (board) => shown.push(board),
      (change) => {
        freshness = change(freshness);
      },
      page.signal,
    );
    return { store, shown, freshness };
  };

  it("is drawn while the board is read, saying when it was kept", async () => {
    const { store, shown, freshness } = await drawing(kept);
    expect(store.read).toHaveBeenCalledWith(CACHE_FORMAT, "", "board:", BOARD);
    expect(shown).toEqual([readPageBoard(CASES.samples.board)]);
    expect(freshness).toEqual({ ...FRESH, saved: 1_000, unanswered: [BOARD] });
  });

  it("is never drawn over the board the page drew first", async () => {
    const { shown, freshness } = await drawing(kept, readPageBoard(CASES.samples.board));
    expect(shown).toEqual([]);
    expect(freshness).toEqual(FRESH);
  });

  it("is never drawn once the page is gone", async () => {
    const { shown, freshness } = await drawing(kept, null, true);
    expect(shown).toEqual([]);
    expect(freshness).toEqual(FRESH);
  });

  it("is forgotten, and nothing drawn, where the page cannot read it", async () => {
    const { store, shown, freshness } = await drawing({ ...kept, value: { version: "seven" } });
    expect(shown).toEqual([]);
    expect(freshness).toEqual(FRESH);
    expect(store.forget).toHaveBeenCalledWith(CACHE_FORMAT, "", "board:");
  });

  it("is nothing where the device kept none", async () => {
    const { store, shown, freshness } = await drawing(null);
    expect(shown).toEqual([]);
    expect(freshness).toEqual(FRESH);
    expect(store.forget).not.toHaveBeenCalled();
  });
});

describe("a page loaded again for the build the server serves", () => {
  /**
   * The page: its shell's script, whether the reader is in the middle of something (`reader`),
   * its tab, and the changes to it a held reload is watched for (`changed`, its focus moving).
   */
  const page = (busy = false) => {
    const reload = vi.fn();
    const reader = { busy };
    const { watched, changed } = observers();
    vi.stubGlobal("sessionStorage", memoryStorage());
    vi.stubGlobal("location", { reload });
    const doc = Object.assign(new EventTarget(), {
      getElementById: () => null,
      documentElement: {},
      activeElement: null as { matches: (selector: string) => boolean } | null,
      querySelector: (selector: string) =>
        selector.startsWith("script")
          ? { getAttribute: () => "/assets/symphony-shell-a.js" }
          : reader.busy
            ? {}
            : null,
    });
    vi.stubGlobal("document", doc);
    return { reload, reader, doc, watched, changed };
  };

  it("loads once for another build, never for its own", () => {
    const { reload } = page();
    reloadIfRedeployed("/assets/symphony-shell-a.js");
    expect(reload).not.toHaveBeenCalled();
    reloadIfRedeployed("/assets/symphony-shell-b.js");
    reloadIfRedeployed("/assets/symphony-shell-b.js");
    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("loads for a feed event's client change naming another build, once", () => {
    const { reload } = page();
    const client = (build: unknown) => ({ changes: [{ region: "client", data: { build } }] });
    followClient(client("/assets/symphony-shell-a.js"));
    followClient({
      changes: [{ region: "queue", data: { build: "/assets/symphony-shell-d.js" } }],
    });
    followClient(client(7));
    followClient({ changes: "none" });
    expect(reload).not.toHaveBeenCalled();
    followClient({ changes: [{ region: "card" }, client("/assets/symphony-shell-d.js")] });
    followClient(client("/assets/symphony-shell-d.js"));
    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("waits while the reader is in the middle of something, and loads as they are done", async () => {
    const { reload, reader, watched, changed } = page(true);
    reloadIfRedeployed("/assets/symphony-shell-c.js");
    flushReload();
    expect(reload).not.toHaveBeenCalled();
    // A change that leaves them in the middle of it: still held, and still watched.
    changed({ attribute: "data-busy" });
    await turn();
    expect(reload).not.toHaveBeenCalled();
    expect(watched.size).toBe(1);
    // A change no held reload waits on wakes nothing.
    reader.busy = false;
    changed({ attribute: "class" });
    await turn();
    expect(reload).not.toHaveBeenCalled();
    changed({ attribute: "data-busy" });
    await turn();
    expect(reload).toHaveBeenCalledTimes(1);
    // Loaded, nothing is watched any more, and nothing is held to load again.
    expect(watched.size).toBe(0);
    flushReload();
    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("loads a held reload once the busy region goes, which changes no attribute", async () => {
    const { reload, reader, watched, changed } = page(true);
    reloadIfRedeployed("/assets/symphony-shell-g.js");
    expect(watched.size).toBe(1);
    // The busy region, an open details or the focused field taken out of the page with what
    // held it: an element removed, and nothing else.
    reader.busy = false;
    changed({ childList: true });
    await turn();
    expect(reload).toHaveBeenCalledTimes(1);
    expect(watched.size).toBe(0);
  });

  it("loads a held reload as the focus leaves the fields, not as it moves to another", async () => {
    const { reload, doc, watched } = page();
    const field = { matches: () => true };
    doc.activeElement = field;
    reloadIfRedeployed("/assets/symphony-shell-e.js");
    // Several held while it waits: one watch for them all.
    reloadIfRedeployed("/assets/symphony-shell-f.js");
    expect(watched.size).toBe(1);
    doc.dispatchEvent(Object.assign(new Event("focusout"), { relatedTarget: field }));
    await turn();
    expect(reload).not.toHaveBeenCalled();
    doc.activeElement = null;
    doc.dispatchEvent(new Event("focusout"));
    await turn();
    expect(reload).toHaveBeenCalledTimes(1);
    expect(watched.size).toBe(0);
  });
});

describe("a read's pause before it is tried again", () => {
  afterEach(() => vi.useRealTimers());

  /** Whether `waiting` ended within the turns its events take. */
  async function ended(waiting: Promise<void>): Promise<boolean> {
    let over = false;
    void waiting.then(() => {
      over = true;
    });
    for (let turns = 0; turns < 4; turns += 1) await Promise.resolve();
    return over;
  }

  /** A retry's first wait, 1 s. */
  const retrying = (signal: AbortSignal) => retryWait(new Backoff(), NEVER_ANSWERED, signal);

  it("ends when its wait is over", async () => {
    vi.useFakeTimers();
    const waiting = pause(retrying, new Retry(), new AbortController().signal);
    await vi.advanceTimersByTimeAsync(999);
    expect(await ended(waiting)).toBe(false);
    await vi.advanceTimersByTimeAsync(1);
    expect(await ended(waiting)).toBe(true);
  });

  it("ends at the reader's press of Retry, and leaves the presses after it alone", async () => {
    vi.useFakeTimers();
    const retry = new Retry();
    const waiting = pause(retrying, retry, new AbortController().signal);
    expect(await ended(waiting)).toBe(false);
    retry.current += 1;
    expect(await ended(waiting)).toBe(true);
    expect(retry.current).toBe(1);
    expect(vi.getTimerCount()).toBe(0);
    // A press with no pause waiting is only counted.
    retry.current += 1;
    expect(retry.current).toBe(2);
  });

  it("waits through the wait it is given, whose signal the reader's press ends", async () => {
    const asked: AbortSignal[] = [];
    const given = (signal: AbortSignal) => {
      asked.push(signal);
      return new Promise<void>((done) => signal.addEventListener("abort", () => done()));
    };
    const retry = new Retry();
    const page = new AbortController();
    const waiting = pause(given, retry, page.signal);
    expect(asked).toHaveLength(1);
    expect(asked[0]).not.toBe(page.signal);
    expect(await ended(waiting)).toBe(false);
    retry.current += 1;
    expect(await ended(waiting)).toBe(true);
    expect(asked[0]?.aborted).toBe(true);
    expect(page.signal.aborted).toBe(false);
  });

  it("ends once its signal ends, or at once when it had, leaving no timer", async () => {
    vi.useFakeTimers();
    const leaving = new AbortController();
    const waiting = pause(retrying, new Retry(), leaving.signal);
    leaving.abort();
    expect(await ended(waiting)).toBe(true);
    await pause(retrying, new Retry(), leaving.signal);
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe("the reader's presses of Retry", () => {
  it("are counted, each told to whoever hears them until they stop", () => {
    const retry = new Retry();
    const first = vi.fn();
    const second = vi.fn();
    const stopFirst = retry.heard(first);
    retry.heard(second);
    retry.current += 1;
    stopFirst();
    retry.current += 1;
    expect(retry.current).toBe(2);
    expect(first).toHaveBeenCalledOnce();
    expect(second).toHaveBeenCalledTimes(2);
  });
});

describe("the board's loop", () => {
  /**
   * A loop over the stubbed server, for the board item, on a page whose window and document hear
   * events and that has no WebSocket unless its test gives it one (`feeds`), so its feed cannot
   * open. `reads` is how the page reads the board and each event it hears (`heard`, read by
   * default). What it drew, kept and waited, and what its frame heard.
   */
  const loop = (
    waitBack: (page: AbortController) => Promise<void> = async () => {},
    reads: Pick<BoardRun, "read"> & { heard?: (delta: unknown) => boolean } = {},
  ) => {
    const asked = server();
    vi.stubGlobal("window", new EventTarget());
    vi.stubGlobal(
      "document",
      Object.assign(new EventTarget(), { getElementById: () => null, querySelector: () => null }),
    );
    vi.stubGlobal("sessionStorage", memoryStorage());
    const keep = vi.fn(async () => {});
    vi.stubGlobal("symphonyCache", { keep });
    const page = new AbortController();
    const did: unknown[] = [];
    const paused: ((signal: AbortSignal) => Promise<void>)[] = [];
    const frame = { refresh: vi.fn(), connected: vi.fn(), heard: vi.fn() };
    const board: { current: PageBoard | null } = { current: null };
    const done = keepBoard(
      "",
      boardItem(""),
      {
        board,
        show: (read) => {
          board.current = read;
          did.push(["show", read.version]);
        },
        setStale: (stale) => did.push(["stale", stale]),
        setTrouble: (why) => did.push(["trouble", why]),
        retry: new Retry(),
        views: {
          freshen: () => {},
          read: () => did.push(["read"]),
          heard: (delta) => {
            did.push(["heard", delta]);
            return reads.heard?.(delta) ?? true;
          },
          frame,
        },
        ...(reads.read ? { read: reads.read } : {}),
      },
      page.signal,
      {
        back: () => waitBack(page),
        // The reader leaves while the loop waits to try again.
        pause: async (wait) => {
          paused.push(wait);
          page.abort();
        },
      },
    );
    return { asked, keep, page, did, paused, done, frame };
  };

  afterEach(() => vi.unstubAllGlobals());

  it("reads the board, keeps it for the device, draws it, and waits to read it again", async () => {
    const { asked, keep, did, paused, done } = loop();
    await asking(asked, 1);
    expect(asked[0]?.url).toBe("/api/board");
    asked[0]?.answer(reply(200, CASES.samples.board, "b1"));
    await done;
    expect(keep).toHaveBeenCalledWith(
      CACHE_FORMAT,
      "",
      "board:",
      "board",
      "board",
      "b1",
      CASES.samples.board,
      undefined,
      false,
    );
    expect(did).toEqual([["show", 7], ["read"], ["stale", true]]);
    expect(paused).toHaveLength(1);
  });

  it("says a failed read and waits to try again, keeping nothing", async () => {
    const { asked, keep, did, paused, done } = loop();
    await asking(asked, 1);
    asked[0]?.answer(reply(200, { version: "seven" }, "b1"));
    await done;
    expect(keep).not.toHaveBeenCalled();
    expect(did).toEqual([
      ["trouble", "The board's answer has no version, ping or build"],
      ["stale", true],
    ]);
    expect(paused).toHaveLength(1);
  });

  it("reads nothing once the page is gone while it waits for the page to be back", async () => {
    // The shell stops the kind while the page is away, which ends that wait.
    const { asked, did, paused, done } = loop(async (page) => page.abort());
    await turn();
    expect(asked).toEqual([]);
    await done;
    expect(did).toEqual([]);
    expect(paused).toEqual([]);
  });

  it("keeps and draws nothing, its read given up, once the page is gone before the board answers", async () => {
    const { asked, keep, page, did, paused, done } = loop();
    await asking(asked, 1);
    expect(asked[0]?.signal?.aborted).toBe(false);
    // The shell stops the kind before the board answers.
    page.abort();
    expect(asked[0]?.signal?.aborted).toBe(true);
    asked[0]?.answer(reply(200, CASES.samples.board, "b1"));
    await done;
    expect(keep).not.toHaveBeenCalled();
    expect(did).toEqual([]);
    expect(paused).toEqual([]);
  });

  it("says nothing of a failed read once the page is gone before it fails", async () => {
    const { asked, page, did, paused, done } = loop();
    await asking(asked, 1);
    page.abort();
    asked[0]?.answer(reply(200, { version: "seven" }, "b1"));
    await done;
    expect(did).toEqual([]);
    expect(paused).toEqual([]);
  });

  /** The page's WebSocket, each stream it opens kept in order: where, and how often closed. */
  const feeds = () => pageFeeds("http://127.0.0.1/queue");

  it("draws and waits for nothing more once the page is gone while it follows the feed", async () => {
    const opened = feeds();
    const { asked, page, did, paused, done } = loop();
    await asking(asked, 1);
    asked[0]?.answer(reply(200, CASES.samples.board, "b1"));
    await asking(opened, 1);
    expect(opened[0]?.url).toBe("ws://127.0.0.1/api/feed?since=7&document=");
    opened[0]?.source.dispatchEvent(new Event("open"));
    await turn();
    expect(did).toEqual([["show", 7], ["read"], ["stale", false]]);
    // The shell stops the kind while the feed is open and quiet.
    page.abort();
    await done;
    expect(opened.map(({ closed }) => closed)).toEqual([1]);
    expect(did).toEqual([["show", 7], ["read"], ["stale", false]]);
    expect(paused).toEqual([]);
  });

  it("gives a silent feed up after two and a half pings, 12.5 s at the longest, and holds one that pings", async () => {
    // The longest a server lets its feed go without a ping (symphony_api::PING_MAX_MS).
    const longest = 5_000;
    const opened = feeds();
    const { asked, did, paused, done } = loop();
    await asking(asked, 1);
    asked[0]?.answer(reply(200, { ...(CASES.samples.board as object), ping_ms: longest }, "b1"));
    await asking(opened, 1);
    const source = opened[0]?.source;
    source?.dispatchEvent(new Event("open"));
    await turn();
    const live = [["show", 7], ["read"], ["stale", false]];
    expect(did).toEqual(live);
    // From here the clock is the test's: each ping starts the wait for the next thing said.
    vi.useFakeTimers();
    try {
      for (let pings = 0; pings < 3; pings += 1) {
        say(source, PING_EVENT, "");
        await vi.advanceTimersByTimeAsync(12_499);
        expect(did).toEqual(live);
        expect(opened.map(({ closed }) => closed)).toEqual([0]);
      }
      await vi.advanceTimersByTimeAsync(1);
      await done;
    } finally {
      vi.useRealTimers();
    }
    expect(opened.map(({ closed }) => closed)).toEqual([1]);
    expect(did).toEqual([...live, ["stale", true]]);
    expect(paused).toHaveLength(1);
  });

  /** The loop's board read and its feed open, then `events` said on it, one after another. */
  const following = async (
    run: ReturnType<typeof loop>,
    opened: ReturnType<typeof feeds>,
    ...events: unknown[]
  ) => {
    await asking(run.asked, 1);
    run.asked[0]?.answer(reply(200, CASES.samples.board, "b1"));
    await asking(opened, 1);
    const source = opened[0]?.source;
    source?.dispatchEvent(new Event("open"));
    for (const event of events) {
      say(source, FEED_EVENT, JSON.stringify(event));
    }
  };

  it("tells the frame each event the page read, and none it has seen", async () => {
    const opened = feeds();
    const run = loop();
    const delta = { version: 8, changes: [] };
    await following(run, opened, { version: 7, changes: [] }, delta);
    await until(() => expect(run.frame.heard).toHaveBeenCalledTimes(1));
    expect(run.frame.heard).toHaveBeenCalledWith(delta);
    run.page.abort();
    await run.done;
    expect(run.did).toEqual([["show", 7], ["read"], ["stale", false], ["heard", delta]]);
  });

  it("loses the feed on an event the page cannot read, hearing nothing after it", async () => {
    const opened = feeds();
    const run = loop(undefined, { heard: () => false });
    const delta = { version: 8, changes: [] };
    await following(run, opened, delta, { version: 9, changes: [] });
    await run.done;
    expect(opened.map(({ closed }) => closed)).toEqual([1]);
    expect(run.frame.heard.mock.calls).toEqual([[delta]]);
    expect(run.did).toEqual([
      ["show", 7],
      ["read"],
      ["stale", false],
      ["heard", delta],
      ["stale", true],
    ]);
    expect(run.paused).toHaveLength(1);
  });

  it("says a board its page cannot read is no board, keeping nothing", async () => {
    const { asked, keep, did, done } = loop(undefined, { read: () => null });
    await asking(asked, 1);
    asked[0]?.answer(reply(200, CASES.samples.board, "b1"));
    await done;
    expect(keep).not.toHaveBeenCalled();
    expect(did).toEqual([
      ["trouble", "The board's answer has no version, ping or build"],
      ["stale", true],
    ]);
  });

  it("draws the board as its page reads it", async () => {
    const read = (value: unknown) => ({ ...readPageBoard(value), version: 70 }) as PageBoard;
    const { asked, did, done } = loop(undefined, { read });
    await asking(asked, 1);
    asked[0]?.answer(reply(200, CASES.samples.board, "b1"));
    await done;
    expect(did[0]).toEqual(["show", 70]);
  });
});

describe("the notices' geometry", () => {
  it("is none without the boxes or a resize observer", () => {
    expect(noticeGeometry(null, null)).toBeUndefined();
  });

  const box = (height: number, top = 0) => ({
    getBoundingClientRect: () => ({ height, top }),
    style: { setProperty: vi.fn() },
  });

  /**
   * The page's globals as `noticeGeometry` reads them: its root's style, the stack placed by
   * `placed.position` (the sheet's `fixed` wide, `absolute` on a phone), a window 800 px tall, and
   * `resized()` running the observer's measurement again.
   */
  const page = (placed: { position: string }) => {
    const geometry = {
      root: { setProperty: vi.fn(), removeProperty: vi.fn() },
      observed: [] as unknown[],
      resized: () => {},
    };
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(sync: () => void) {
          geometry.resized = sync;
        }
        observe(element: unknown) {
          geometry.observed.push(element);
        }
        disconnect() {}
      },
    );
    vi.stubGlobal("document", { documentElement: { style: geometry.root } });
    vi.stubGlobal("getComputedStyle", () => ({ position: placed.position }));
    vi.stubGlobal("innerHeight", 800);
    vi.stubGlobal("addEventListener", vi.fn());
    vi.stubGlobal("removeEventListener", vi.fn());
    return geometry;
  };

  it("keeps the reserve as tall as it grew, and the toasts above the stack", () => {
    const notice = box(40, 700);
    const reserve = box(40);
    const geometry = page({ position: "fixed" });
    const undo = noticeGeometry(notice as never, reserve as never);
    expect(geometry.observed).toEqual([notice, reserve]);
    expect(reserve.style.setProperty).toHaveBeenCalledWith("min-height", "40px");
    expect(geometry.root.setProperty).toHaveBeenCalledWith("--notice-toast-bottom", "108px");
    reserve.getBoundingClientRect = () => ({ height: 20, top: 0 });
    notice.getBoundingClientRect = () => ({ height: 0, top: 0 });
    geometry.resized();
    expect(reserve.style.setProperty).toHaveBeenCalledTimes(1);
    expect(geometry.root.setProperty).toHaveBeenLastCalledWith("--notice-toast-bottom", "66px");
    undo?.();
    expect(geometry.root.removeProperty).toHaveBeenCalledWith("--notice-toast-bottom");
  });

  it("leaves the toasts their own place while the stack is in the page's flow", () => {
    const notice = box(40, 700);
    const reserve = box(60);
    const placed = { position: "absolute" };
    const geometry = page(placed);
    noticeGeometry(notice as never, reserve as never);
    expect(geometry.root.setProperty).toHaveBeenLastCalledWith("--notice-toast-bottom", "66px");
    expect(reserve.style.setProperty).toHaveBeenCalledWith("min-height", "60px");
    // The window widened past a phone's: the stack floats again, and the toasts go above it.
    placed.position = "fixed";
    geometry.resized();
    expect(geometry.root.setProperty).toHaveBeenLastCalledWith("--notice-toast-bottom", "108px");
    placed.position = "absolute";
    geometry.resized();
    expect(geometry.root.setProperty).toHaveBeenLastCalledWith("--notice-toast-bottom", "66px");
    expect(reserve.style.setProperty).toHaveBeenCalledTimes(1);
  });
});
