import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import { boardItem } from "./cache";
import {
  BoardPage,
  type BoardRun,
  FEED_NOTICE,
  flushReload,
  followClient,
  keepBoard,
  noticeGeometry,
  type PageBoard,
  PageNotices,
  readPageBoard,
  reloadIfRedeployed,
} from "./live";
import { SERVER_NOTICE } from "./shared";
import { asking, CASES, memoryStorage, reply, server } from "./testing";
import { CACHE_FORMAT, FEED_EVENT } from "./wire";

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

describe("a page loaded again for the build the server serves", () => {
  /** The page: its shell's script, whether the reader is in the middle of something, its tab. */
  const page = (engaged: boolean) => {
    const reload = vi.fn();
    vi.stubGlobal("sessionStorage", memoryStorage());
    vi.stubGlobal("location", { reload });
    vi.stubGlobal("document", {
      getElementById: () => null,
      activeElement: null,
      querySelector: (selector: string) =>
        selector.startsWith("script")
          ? { getAttribute: () => "/assets/symphony-shell-a.js" }
          : engaged
            ? {}
            : null,
    });
    return reload;
  };

  it("loads once for another build, never for its own", () => {
    const reload = page(false);
    reloadIfRedeployed("/assets/symphony-shell-a.js");
    expect(reload).not.toHaveBeenCalled();
    reloadIfRedeployed("/assets/symphony-shell-b.js");
    reloadIfRedeployed("/assets/symphony-shell-b.js");
    expect(reload).toHaveBeenCalledTimes(1);
  });

  it("loads for a feed event's client change naming another build, once", () => {
    const reload = page(false);
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

  it("waits while the reader is in the middle of something", () => {
    const reload = page(true);
    reloadIfRedeployed("/assets/symphony-shell-c.js");
    flushReload();
    expect(reload).not.toHaveBeenCalled();
    const after = page(false);
    flushReload();
    expect(after).toHaveBeenCalledTimes(1);
    flushReload();
    expect(after).toHaveBeenCalledTimes(1);
  });
});

describe("the board's loop", () => {
  /**
   * A loop over the stubbed server, for the board item, on a page whose window and document hear
   * events and that has no EventSource unless its test gives it one (`feeds`), so its feed cannot
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
    const paused: number[] = [];
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
        retry: { current: 0 },
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
        pause: async (ms) => {
          paused.push(ms);
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
    await new Promise((settled) => setTimeout(settled, 0));
    expect(asked).toEqual([]);
    await done;
    expect(did).toEqual([]);
    expect(paused).toEqual([]);
  });

  it("keeps and draws nothing once the page is gone before the board answers", async () => {
    const { asked, keep, page, did, paused, done } = loop();
    await asking(asked, 1);
    // The shell stops the kind before the board answers.
    page.abort();
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

  /** The page's EventSource, each stream it opens kept in order: where, and how often closed. */
  const feeds = () => {
    const opened: { url: string; closed: number; source: EventTarget }[] = [];
    vi.stubGlobal("location", { href: "http://127.0.0.1/queue" });
    vi.stubGlobal(
      "EventSource",
      class extends EventTarget {
        private readonly stream: (typeof opened)[number];
        constructor(url: string) {
          super();
          this.stream = { url, closed: 0, source: this };
          opened.push(this.stream);
        }
        close() {
          this.stream.closed += 1;
        }
      },
    );
    return opened;
  };

  it("draws and waits for nothing more once the page is gone while it follows the feed", async () => {
    const opened = feeds();
    const { asked, page, did, paused, done } = loop();
    await asking(asked, 1);
    asked[0]?.answer(reply(200, CASES.samples.board, "b1"));
    await asking(opened, 1);
    expect(opened[0]?.url).toBe("http://127.0.0.1/api/feed?since=7&document=");
    opened[0]?.source.dispatchEvent(new Event("open"));
    await new Promise((settled) => setTimeout(settled, 0));
    expect(did).toEqual([["show", 7], ["read"], ["stale", false]]);
    // The shell stops the kind while the feed is open and quiet.
    page.abort();
    await done;
    expect(opened.map(({ closed }) => closed)).toEqual([1]);
    expect(did).toEqual([["show", 7], ["read"], ["stale", false]]);
    expect(paused).toEqual([]);
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
      source?.dispatchEvent(new MessageEvent(FEED_EVENT, { data: JSON.stringify(event) }));
    }
  };

  it("tells the frame each event the page read, and none it has seen", async () => {
    const opened = feeds();
    const run = loop();
    const delta = { version: 8, changes: [] };
    await following(run, opened, { version: 7, changes: [] }, delta);
    await vi.waitFor(() => expect(run.frame.heard).toHaveBeenCalledTimes(1));
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

  it("keeps the reserve as tall as it grew, and the toasts above the stack", () => {
    const box = (height: number, top = 0) => ({
      getBoundingClientRect: () => ({ height, top }),
      style: { setProperty: vi.fn() },
    });
    const notice = box(40, 700);
    const reserve = box(40);
    const root = { setProperty: vi.fn(), removeProperty: vi.fn() };
    const observed: unknown[] = [];
    let resized = () => {};
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(sync: () => void) {
          resized = sync;
        }
        observe(element: unknown) {
          observed.push(element);
        }
        disconnect() {}
      },
    );
    vi.stubGlobal("document", { documentElement: { style: root } });
    vi.stubGlobal("innerHeight", 800);
    vi.stubGlobal("addEventListener", vi.fn());
    vi.stubGlobal("removeEventListener", vi.fn());
    const undo = noticeGeometry(notice as never, reserve as never);
    expect(observed).toEqual([notice, reserve]);
    expect(reserve.style.setProperty).toHaveBeenCalledWith("min-height", "40px");
    expect(root.setProperty).toHaveBeenCalledWith("--notice-toast-bottom", "108px");
    reserve.getBoundingClientRect = () => ({ height: 20, top: 0 });
    notice.getBoundingClientRect = () => ({ height: 0, top: 0 });
    resized();
    expect(reserve.style.setProperty).toHaveBeenCalledTimes(1);
    expect(root.setProperty).toHaveBeenLastCalledWith("--notice-toast-bottom", "66px");
    undo?.();
    expect(root.removeProperty).toHaveBeenCalledWith("--notice-toast-bottom");
  });
});
