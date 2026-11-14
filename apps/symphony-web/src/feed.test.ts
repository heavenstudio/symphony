import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { type Heard, heard, Inbox, sleep } from "./feed";
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
  /** The EventSource the feed opened, as a fake that the test makes speak. */
  class FakeSource extends EventTarget {
    static opened: FakeSource[] = [];
    closed = false;
    constructor(readonly url: string) {
      super();
      FakeSource.opened.push(this);
    }
    close() {
      this.closed = true;
    }
  }
  const page = new EventTarget();
  const doc = new EventTarget();

  beforeEach(() => {
    FakeSource.opened = [];
    vi.resetModules();
    vi.stubGlobal("EventSource", FakeSource);
    vi.stubGlobal("window", page);
    vi.stubGlobal("location", { href: "http://127.0.0.1:1/queue" });
    vi.stubGlobal("document", Object.assign(doc, { getElementById: () => null }));
  });
  afterEach(() => vi.unstubAllGlobals());

  it("opens on the project's feed and hands on what it says", async () => {
    const { openFeed } = await import("./feed");
    const said: Heard[] = [];
    const close = openFeed("/api/feed?since=3", (one) => said.push(one));
    const [source] = FakeSource.opened;
    expect(source?.url).toBe("http://127.0.0.1:1/api/feed?since=3");
    source?.dispatchEvent(new Event("open"));
    source?.dispatchEvent(new MessageEvent(FEED_EVENT, { data: '{"version":4}' }));
    source?.dispatchEvent(new Event("error"));
    expect(said).toEqual([
      { kind: "open" },
      { kind: "delta", value: { version: 4 } },
      { kind: "lost" },
    ]);
    close?.();
    expect(source?.closed).toBe(true);
  });

  it("closes as the page goes away, and opens none until it is back", async () => {
    const { isAway, openFeed } = await import("./feed");
    const said: Heard[] = [];
    openFeed("/api/feed", (one) => said.push(one));
    page.dispatchEvent(new Event("pagehide"));
    expect(FakeSource.opened[0]?.closed).toBe(true);
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
});
