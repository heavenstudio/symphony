import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  BoardPage,
  FEED_NOTICE,
  flushReload,
  noticeGeometry,
  PageNotices,
  readPageBoard,
  reloadIfRedeployed,
} from "./live";
import { SERVER_NOTICE } from "./shared";
import { CASES, memoryStorage } from "./testing";

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
