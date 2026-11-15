import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { type Heard, heard, Inbox, message, sleep, socketAddress } from "./feed";
import { feeds, say } from "./testing";
import { FEED_EVENT, PING_EVENT, SERVER_EVENT } from "./wire";

describe("what one event of the stream says", () => {
  it("is open, a ping, a change or the server's notice", () => {
    expect(heard("open", undefined)).toEqual({ kind: "open" });
    expect(heard(PING_EVENT, "")).toEqual({ kind: "ping" });
    expect(heard(FEED_EVENT, '{"version":3}')).toEqual({ kind: "delta", value: { version: 3 } });
    expect(heard(SERVER_EVENT, '"Restarting"')).toEqual({ kind: "server", message: "Restarting" });
  });

  it.each([
    ["an error", "error", undefined],
    ["a change that does not parse", FEED_EVENT, "{"],
    ["a notice that is no text", SERVER_EVENT, "3"],
    ["a notice that does not parse", SERVER_EVENT, "x"],
  ])("is lost for %s", (_, type, data) => {
    expect(heard(type, data)).toEqual({ kind: "lost" });
  });
});

describe("what one message of the socket says", () => {
  it("is its event, named before its first line feed, with the data after it", () => {
    expect(message(`${FEED_EVENT}\n{"version":3}`)).toEqual({
      kind: "delta",
      value: { version: 3 },
    });
    expect(message(`${PING_EVENT}\n`)).toEqual({ kind: "ping" });
    expect(message(`${SERVER_EVENT}\n"Rebuilt"`)).toEqual({ kind: "server", message: "Rebuilt" });
    // The data is the rest, line feeds and all: JSON may span lines.
    expect(message(`${FEED_EVENT}\n{\n"version":\n4}`)).toEqual({
      kind: "delta",
      value: { version: 4 },
    });
  });

  it.each([
    ["no line feed", FEED_EVENT],
    ["an empty message", ""],
    ["an event of no name", '\n{"version":3}'],
    ["an event it does not know", 'later\n{"version":3}'],
    ["a change that does not parse", `${FEED_EVENT}\n{`],
    ["binary data", new ArrayBuffer(3)],
    ["no data", undefined],
  ])("is lost for %s", (_, data) => {
    expect(message(data)).toEqual({ kind: "lost" });
  });
});

describe("the socket's address", () => {
  it.each([
    ["http://127.0.0.1:8765/api/feed?since=3", "ws://127.0.0.1:8765/api/feed?since=3"],
    [
      "https://machine.tail.ts.net/@p/api/feed?since=3",
      "wss://machine.tail.ts.net/@p/api/feed?since=3",
    ],
    ["http://[::1]:9/api/feed?since=0&document=", "ws://[::1]:9/api/feed?since=0&document="],
  ])("of %s is %s", (page, socket) => {
    expect(socketAddress(new URL(page))).toBe(socket);
  });

  it("leaves the page's address as it was", () => {
    const page = new URL("http://127.0.0.1/api/feed");
    socketAddress(page);
    expect(page.protocol).toBe("http:");
  });
});

describe("the inbox", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("gives what was said before it was asked, in order", async () => {
    const inbox = new Inbox<number>();
    inbox.push(1);
    inbox.push(2);
    expect(await inbox.next(10)).toBe(1);
    expect(await inbox.next(10)).toBe(2);
  });

  it("wakes a wait with what is said next", async () => {
    const inbox = new Inbox<string>();
    const next = inbox.next(1_000);
    inbox.push("said");
    expect(await next).toBe("said");
    inbox.push("later");
    expect(await inbox.next(1_000)).toBe("later");
  });

  it("gives none when the wait ends first, and keeps what is said after", async () => {
    const inbox = new Inbox<string>();
    const next = inbox.next(1_000);
    await vi.advanceTimersByTimeAsync(1_000);
    expect(await next).toBeNull();
    inbox.push("after");
    expect(await inbox.next(1_000)).toBe("after");
  });

  it("gives none once its signal aborts, or at once when it had", async () => {
    const inbox = new Inbox<string>();
    const controller = new AbortController();
    const next = inbox.next(60_000, controller.signal);
    controller.abort();
    expect(await next).toBeNull();
    expect(await inbox.next(60_000, controller.signal)).toBeNull();
  });
});

describe("a sleep", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("ends when its time is up", async () => {
    let done = false;
    void sleep(500).then(() => {
      done = true;
    });
    await vi.advanceTimersByTimeAsync(499);
    expect(done).toBe(false);
    await vi.advanceTimersByTimeAsync(1);
    expect(done).toBe(true);
  });

  it("ends at once when its signal aborts, or had", async () => {
    const controller = new AbortController();
    const slept = sleep(60_000, controller.signal);
    controller.abort();
    await slept;
    await sleep(60_000, controller.signal);
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe("the feed's stream", () => {
  // A page of its own each test, as each imports the module afresh, with listeners of its own.
  let page: EventTarget;
  let doc: EventTarget;
  let opened: ReturnType<typeof feeds>;

  beforeEach(() => {
    vi.resetModules();
    page = new EventTarget();
    doc = new EventTarget();
    opened = feeds("http://127.0.0.1:1/queue");
    vi.stubGlobal("window", page);
    vi.stubGlobal("document", Object.assign(doc, { getElementById: () => null }));
  });
  afterEach(() => vi.unstubAllGlobals());

  it("opens a socket on the project's feed and hands on what it says", async () => {
    const { openFeed } = await import("./feed");
    const said: Heard[] = [];
    const close = openFeed("/api/feed?since=3", (one) => said.push(one));
    const [socket] = opened;
    expect(socket?.url).toBe("ws://127.0.0.1:1/api/feed?since=3");
    socket?.source.dispatchEvent(new Event("open"));
    say(socket?.source, SERVER_EVENT, '"Serving"');
    say(socket?.source, FEED_EVENT, '{"version":4}');
    say(socket?.source, PING_EVENT, "");
    expect(said).toEqual([
      { kind: "open" },
      { kind: "server", message: "Serving" },
      { kind: "delta", value: { version: 4 } },
      { kind: "ping" },
    ]);
    close?.();
    expect(socket?.closed).toBe(1);
    // A socket tells of its close after it is closed: the feed it was says nothing more.
    socket?.source.dispatchEvent(new Event("close"));
    say(socket?.source, FEED_EVENT, '{"version":5}');
    expect(said).toHaveLength(4);
  });

  it.each([
    ["fails", ["error", "close"]],
    ["is closed by the server", ["close"]],
  ])("is lost once when its socket %s", async (_, events) => {
    const { openFeed } = await import("./feed");
    const said: Heard[] = [];
    openFeed("/api/feed?since=3", (one) => said.push(one));
    const socket = opened[0]?.source;
    socket?.dispatchEvent(new Event("open"));
    for (const name of events) socket?.dispatchEvent(new Event(name));
    say(socket, FEED_EVENT, '{"version":4}');
    expect(said).toEqual([{ kind: "open" }, { kind: "lost" }]);
  });

  it("is lost at a message it cannot read, and hears nothing after it", async () => {
    const { openFeed } = await import("./feed");
    const said: Heard[] = [];
    openFeed("/api/feed?since=3", (one) => said.push(one));
    const socket = opened[0]?.source;
    socket?.dispatchEvent(new MessageEvent("message", { data: '{"version":4}' }));
    say(socket, FEED_EVENT, '{"version":5}');
    socket?.dispatchEvent(new Event("close"));
    expect(said).toEqual([{ kind: "lost" }]);
  });

  it("opens on the page's secure address as a secure socket", async () => {
    vi.stubGlobal("location", { href: "https://machine.tail.ts.net/queue" });
    const { openFeed } = await import("./feed");
    openFeed("/api/feed?since=0", () => {});
    expect(opened[0]?.url).toBe("wss://machine.tail.ts.net/api/feed?since=0");
  });

  it("is none when the browser cannot open the socket", async () => {
    vi.stubGlobal(
      "WebSocket",
      class {
        constructor() {
          throw new SyntaxError("refused");
        }
      },
    );
    const { openFeed } = await import("./feed");
    expect(openFeed("/api/feed?since=0", () => {})).toBeNull();
  });

  it("closes as the page goes away, and opens none until it is back", async () => {
    const { isAway, openFeed } = await import("./feed");
    const said: Heard[] = [];
    openFeed("/api/feed", (one) => said.push(one));
    page.dispatchEvent(new Event("pagehide"));
    expect(opened[0]?.closed).toBe(1);
    // Its close, told after, is no loss: the page went away.
    opened[0]?.source.dispatchEvent(new Event("close"));
    expect(said).toEqual([{ kind: "away" }]);
    expect(isAway()).toBe(true);
    expect(openFeed("/api/feed", () => {})).toBeNull();
    page.dispatchEvent(new Event("pageshow"));
    expect(isAway()).toBe(false);
    doc.dispatchEvent(new Event("freeze"));
    expect(isAway()).toBe(true);
    doc.dispatchEvent(new Event("resume"));
    expect(isAway()).toBe(false);
  });

  it("is not told of the page going away once it was closed", async () => {
    const { isAway, openFeed } = await import("./feed");
    const said: Heard[] = [];
    const close = openFeed("/api/feed", (one) => said.push(one));
    close?.();
    doc.dispatchEvent(new Event("freeze"));
    expect(said).toEqual([]);
    expect(opened[0]?.closed).toBe(1);
    expect(isAway()).toBe(true);
    doc.dispatchEvent(new Event("resume"));
  });

  it("is not told of the page going away once it was lost, though the page is away", async () => {
    const { isAway, openFeed } = await import("./feed");
    const said: Heard[] = [];
    openFeed("/api/feed", (one) => said.push(one));
    opened[0]?.source.dispatchEvent(new Event("error"));
    page.dispatchEvent(new Event("pagehide"));
    expect(said).toEqual([{ kind: "lost" }]);
    expect(isAway()).toBe(true);
    page.dispatchEvent(new Event("pageshow"));
  });
});
