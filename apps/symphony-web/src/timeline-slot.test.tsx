import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { memoryStorage, until } from "./testing";
import type { HostProps } from "./timeline";
import {
  boxStyle,
  heights,
  kept,
  key,
  LEAST,
  NARROW,
  placeholders,
  RETRY,
  SAID,
  SLOT,
  TimelineSlot,
  WIDE,
} from "./timeline-slot";

// React's effects stood in for, as no DOM runs here: each effect a hook sets is kept to be run by
// its test.
const react = vi.hoisted(() => ({ effects: [] as (() => undefined | (() => void))[] }));

vi.mock("react", async (original) => ({
  ...(await original<typeof import("react")>()),
  useEffect: (effect: () => undefined | (() => void)) => {
    react.effects.push(effect);
  },
}));

/**
 * The text of the style sheet `name`, beside this file. Vitest empties every CSS import, `?raw`
 * among them, while its `css` option processes none, so the sheet is read through Node's own
 * `fs`, which this package takes no types of.
 */
function sheet(name: string): string {
  const { process } = globalThis as unknown as {
    process: {
      getBuiltinModule(id: "node:fs"): { readFileSync(path: URL, encoding: "utf8"): string };
    };
  };
  return process.getBuiltinModule("node:fs").readFileSync(new URL(name, import.meta.url), "utf8");
}

/** The page: its document with the failure notice `notice` when one is shown, its window's
 * shell, and this browser's storage. */
function aPage(shell: Record<string, unknown> = {}, notice: { remove: () => void } | null = null) {
  vi.stubGlobal("document", {
    getElementById: (id: string) => (id === "timeline-error" ? notice : null),
  });
  vi.stubGlobal("window", shell);
  const storage = memoryStorage();
  vi.stubGlobal("localStorage", storage);
  return storage;
}

/** A chunk's host that draws its task and what its part holds, and says it drew 212.4 px on a
 * phone. */
function DrawnHost({ slug, part, tokens, onDrawn }: HostProps) {
  onDrawn?.(false, 212.4);
  return (
    <i
      data-slug={slug}
      data-part={JSON.stringify(part.value ?? null)}
      data-tokens={tokens === undefined ? undefined : JSON.stringify(tokens)}
    />
  );
}

/** The slot's module afresh, its chunk not imported yet: one per test of the loader. */
async function aSlot() {
  vi.resetModules();
  return await import("./timeline-slot");
}

beforeEach(() => {
  react.effects.length = 0;
});

afterEach(() => {
  vi.useRealTimers();
  vi.doUnmock("./timeline");
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("the timeline's box", () => {
  it("is at most 240 px tall on a phone, and as tall as its whole timeline wide", () => {
    expect(NARROW).toBe(240);
    // On a phone nothing kept, nor a height kept by a build whose box was taller, reserves more;
    // wide the whole timeline's height is kept and reserved, however tall.
    expect(heights("460,480")).toEqual([240, 480]);
    expect(kept("460,480", true, 2000)).toBe("240,2000");
    expect(kept("460,480", false, 2000)).toBe("240,480");
    expect(heights("240,2000")).toEqual([240, 2000]);
    // A task never drawn wide reserves a typical whole timeline, taller than a phone's box.
    expect(WIDE).toBeGreaterThan(NARROW);
  });

  it("a kept height is read back at least the least, and on a phone at most its default", () => {
    expect(heights(null)).toEqual([NARROW, WIDE]);
    expect(heights("")).toEqual([NARROW, WIDE]);
    expect(heights("200,150")).toEqual([200, 150]);
    expect(heights(" 200 , 150 ")).toEqual([200, 150]);
    expect(heights("9999,10")).toEqual([NARROW, LEAST]);
    expect(heights("10,9999")).toEqual([LEAST, 9999]);
    expect(heights("tall,150")).toEqual([NARROW, 150]);
    expect(heights("200,-5")).toEqual([200, WIDE]);
    expect(heights("200")).toEqual([200, WIDE]);
    expect(heights(`${NARROW},${WIDE}`)).toEqual([NARROW, WIDE]);
  });

  it("reads a height as Rust reads a u32: its digits, a plus before them, no more than its most", () => {
    expect(heights("+200,+300")).toEqual([200, 300]);
    expect(heights("200.5,1e3")).toEqual([NARROW, WIDE]);
    expect(heights("0x10,-0")).toEqual([NARROW, WIDE]);
    expect(heights("200,4294967295")).toEqual([200, 4_294_967_295]);
    expect(heights("200,4294967296")).toEqual([200, WIDE]);
    // Every space Rust trims, a no-break and an ideographic one among them; three parts read two.
    expect(heights(" 200　,\t150\n,7")).toEqual([200, 150]);
    expect(heights(",")).toEqual([NARROW, WIDE]);
  });

  it("a draw keeps its own width's height, and the other as it was", () => {
    expect(kept(null, false, 212.4)).toBe(`213,${WIDE}`);
    expect(kept(null, true, 212.4)).toBe(`${NARROW},213`);
    expect(kept("213,150", true, 180)).toBe("213,180");
    expect(kept("213,150", true, 612)).toBe("213,612");
    expect(kept("213,150", true, 12)).toBe(`213,${LEAST}`);
    expect(kept("213,150", false, 2000)).toBe(`${NARROW},150`);
    expect(kept("213,150", false, 12)).toBe(`${LEAST},150`);
    expect(kept("213,150", false, -5)).toBe(`${LEAST},150`);
    expect(key("tools/a")).toBe("timeline-height:tools/a");
  });

  it("a draw of no height, or one past a u32, keeps the least or the most", () => {
    expect(kept(null, true, Number.NaN)).toBe(`${NARROW},${LEAST}`);
    expect(kept(null, true, 1e12)).toBe(`${NARROW},4294967295`);
    expect(kept(null, true, Number.POSITIVE_INFINITY)).toBe(`${NARROW},4294967295`);
    // A whole height is kept as it is, a part of a pixel up to the next.
    expect(kept(null, true, 300)).toBe(`${NARROW},300`);
    expect(kept(null, true, 300.0001)).toBe(`${NARROW},301`);
  });

  it("a taller box draws more placeholder rows, never none nor more than nine", () => {
    expect(placeholders(0)).toBe(1);
    expect(placeholders(LEAST)).toBe(1);
    expect(placeholders(NARROW)).toBe(3);
    expect(placeholders(WIDE)).toBe(9);
    expect(placeholders(408)).toBe(6);
    expect(placeholders(5000)).toBe(9);
    // A row more for each 46 px past the first 90.
    expect([181, 182, 136, 135].map(placeholders)).toEqual([1, 2, 1, 1]);
    expect(placeholders(-50)).toBe(1);
  });

  it("is reserved as tall as the timeline will be, with its placeholder rows", () => {
    aPage();
    const html = renderToStaticMarkup(<TimelineSlot slug="tools/new" scope="" />);
    expect(html).toContain('<h2 id="timeline-title">Timeline</h2>');
    expect(html).toContain(
      `id="${SLOT}" data-slug="tools/new" style="--tl-narrow:${NARROW}px;--tl-wide:${WIDE}px"`,
    );
    expect(html.split('class="tl-sk-row"').length - 1).toBe(placeholders(WIDE));
    // It says it reads, to assistive technology, inside the box's own height: no one-line note the
    // timeline then replaces.
    expect(html).toContain(">Reading the timeline…</span>");
    expect(html).not.toContain('role="alert"');
  });

  it("draws the Dioxus box's markup, the heights this browser kept for the task, and no timeline before its chunk", () => {
    const storage = aPage();
    storage.setItem(key("tools/new"), "200,500");
    const html = renderToStaticMarkup(<TimelineSlot slug="tools/new" scope="" />);
    const row = (at: number) =>
      `<span class="tl-sk-row" aria-hidden="true"><span class="tl-sk-line tl-sk-at"></span><span class="tl-sk-line tl-sk-label" style="width:${30 + ((at * 17) % 40)}%"></span><span class="tl-sk-line tl-sk-bar" style="width:${20 + ((at * 29) % 50)}%"></span></span>`;
    expect(html).toBe(
      `<section class="tl" id="timeline" aria-labelledby="timeline-title"><h2 id="timeline-title">Timeline</h2><div class="tl-box card" id="timeline-slot" data-slug="tools/new" style="--tl-narrow:200px;--tl-wide:500px"><div class="tl-skeleton"><span class="tl-sk-said">Reading the timeline…</span><span class="tl-sk-line tl-sk-head" aria-hidden="true"></span>${Array.from({ length: placeholders(500) }, (_, at) => row(at)).join("")}</div></div><div class="tl-message-place"></div></section>`,
    );
    expect(placeholders(500)).toBe(8);
  });

  it("waits below the screen behind its sentinel, where the browser says when it nears", () => {
    aPage();
    vi.stubGlobal("IntersectionObserver", class {});
    const html = renderToStaticMarkup(<TimelineSlot slug="tools/new" scope="" />);
    expect(html).toContain(
      '<section class="tl" id="timeline" aria-labelledby="timeline-title"><div class="viewport-sentinel" aria-hidden="true"></div><h2 id="timeline-title">Timeline</h2>',
    );
    // Not near the screen yet: no chunk is asked for.
    expect(react.effects).toHaveLength(1);
    expect(react.effects[0]?.()).toBeUndefined();
  });

  it("keeps a task's heights in this browser for its next visit", async () => {
    const storage = aPage();
    const slot = await aSlot();
    expect(slot.reserved("tools/a")).toEqual([NARROW, WIDE]);
    slot.keep("tools/a", false, 212.4);
    expect(storage.getItem("timeline-height:tools/a")).toBe(`213,${WIDE}`);
    slot.keep("tools/a", true, 612);
    expect(slot.reserved("tools/a")).toEqual([213, 612]);
    expect(slot.reserved("tools/b")).toEqual([NARROW, WIDE]);
  });

  it("writes its two heights for its style sheet to take the one its width draws", () => {
    expect(boxStyle(200, 500)).toEqual({ "--tl-narrow": "200px", "--tl-wide": "500px" });
  });

  it("the box's style holds its height per width, and takes down the placeholder once drawn", () => {
    const style = sheet("./timeline-slot.css");
    expect(style).toContain(".tl-box {\n  height: var(--tl-narrow);\n");
    // Wide, the box holds its height until the timeline is drawn, then grows or shrinks to it.
    expect(style).toContain(
      "@media (min-width: 900px) {\n  .tl-box {\n    height: var(--tl-wide);\n  }\n  .tl-box:has(.tl-drawn) {\n    height: auto;\n  }\n",
    );
    expect(style).toContain(".tl-box:has(.tl-drawn) > .tl-skeleton {\n  display: none;\n}");
    expect(style).toContain(".tl-box:has(.tl-all) {\n  height: auto;\n}");
    // A failed read never cuts off its Try again: the box grows past its height, never under.
    expect(style).toContain(
      ".tl-box:has(.tl-failed) {\n  height: auto;\n  min-height: var(--tl-narrow);\n}",
    );
    expect(style).toContain("  .tl-box:has(.tl-failed) {\n    min-height: var(--tl-wide);\n  }\n}");
    // On a phone, a box that holds every step grows with a row a tap focused, or with the words
    // of the total's cost, never under its height; one that clips keeps it for a row.
    expect(style).toContain(
      "@media (max-width: 899.98px) {\n  .tl-box:has(.tl-drawn:not(.tl-clipped) .tl-row:focus, .tl-cost:focus) {\n    height: auto;\n    min-height: var(--tl-narrow);\n  }\n}",
    );
    // The timeline is laid out where the shell put its root, which the box still holds.
    expect(style).toContain('.tl-box > [id="timeline-root"]:not([hidden]) {');
    for (const written of ["#", "rgb(", "rgba(", "hsl("]) expect(style).not.toContain(written);
  });
});

describe("the timeline's chunk", () => {
  it("is imported once for every box waiting, each told when it comes", async () => {
    const notice = { remove: vi.fn() };
    aPage({}, notice);
    const chunk = vi.fn(() => ({ TimelineHost: DrawnHost }));
    vi.doMock("./timeline", chunk);
    const TimelineHost = DrawnHost;
    const slot = await aSlot();
    const first = vi.fn();
    const second = vi.fn();
    slot.whenImported(first);
    slot.whenImported(second);
    expect(first).not.toHaveBeenCalled();
    expect(slot.loadedHost()).toBeNull();
    await slot.importTimeline();
    // One import for both.
    expect(chunk).toHaveBeenCalledTimes(1);
    expect(first).toHaveBeenCalledTimes(1);
    expect(first).toHaveBeenCalledWith(TimelineHost);
    expect(second).toHaveBeenCalledWith(TimelineHost);
    expect(slot.loadedHost()).toBe(TimelineHost);
    // A notice a failed import left is taken down.
    expect(notice.remove).toHaveBeenCalledTimes(1);
    // A box that asks once it has come is told at once, and nothing is imported again.
    const later = vi.fn();
    slot.whenImported(later)();
    expect(later).toHaveBeenCalledWith(TimelineHost);
    await slot.importTimeline();
    expect(chunk).toHaveBeenCalledTimes(1);
  });

  it("a box that left is not told", async () => {
    aPage();
    const slot = await aSlot();
    const left = vi.fn();
    const staying = vi.fn();
    const leave = slot.whenImported(left);
    slot.whenImported(staying);
    leave();
    await until(() => expect(staying).toHaveBeenCalledTimes(1));
    expect(left).not.toHaveBeenCalled();
  });

  it("a chunk that does not come is said with its retry, which imports it at once", async () => {
    const alarm = vi.fn();
    const gone = vi.fn(async () => false);
    aPage({ reviewShell: { gone, alarm } });
    const failure = vi.spyOn(console, "error").mockImplementation(() => {});
    vi.doMock("./timeline", () => {
      throw new Error("The chunk is gone");
    });
    const slot = await aSlot();
    const told = vi.fn();
    slot.whenImported(told);
    await until(() => expect(alarm).toHaveBeenCalledTimes(1));
    expect(gone).toHaveBeenCalledTimes(1);
    expect(alarm).toHaveBeenCalledWith("timeline-error", SAID, RETRY, expect.any(Function));
    expect(failure).toHaveBeenCalledWith(SAID, expect.any(Error));
    expect(told).not.toHaveBeenCalled();
    expect(slot.loadedHost()).toBeNull();
    // The retry asks again.
    vi.doUnmock("./timeline");
    const retry = alarm.mock.calls[0]?.[3] as () => void;
    retry();
    await until(() => expect(told).toHaveBeenCalledTimes(1));
  });

  it("a chunk that does not come says nothing where the page's build is gone", async () => {
    const alarm = vi.fn();
    const gone = vi.fn(async () => true);
    aPage({ reviewShell: { gone, alarm } });
    const failure = vi.spyOn(console, "error").mockImplementation(() => {});
    vi.doMock("./timeline", () => {
      throw new Error("The chunk is gone");
    });
    const slot = await aSlot();
    slot.whenImported(vi.fn());
    await slot.importTimeline();
    expect(gone).toHaveBeenCalledTimes(1);
    // The page loads again onto its new build: no notice.
    expect(alarm).not.toHaveBeenCalled();
    expect(failure).not.toHaveBeenCalled();
  });

  it("is not fetched for a box that left before its import began, and the next box asks again", async () => {
    aPage();
    const chunk = vi.fn(() => ({ TimelineHost: DrawnHost }));
    vi.doMock("./timeline", chunk);
    const slot = await aSlot();
    const left = vi.fn();
    slot.whenImported(left)();
    await slot.importTimeline();
    // The page stopped before the import began: nothing fetched, nobody told.
    expect(chunk).not.toHaveBeenCalled();
    expect(left).not.toHaveBeenCalled();
    expect(slot.loadedHost()).toBeNull();
    // The next box asks in its turn, and is told.
    const next = vi.fn();
    slot.whenImported(next);
    await slot.importTimeline();
    expect(chunk).toHaveBeenCalledTimes(1);
    expect(next).toHaveBeenCalledWith(DrawnHost);
  });

  it("a chunk that does not come says nothing once its box left while the shell said whether the build is gone", async () => {
    let answer: (gone: boolean) => void = () => {};
    const gone = vi.fn(() => new Promise<boolean>((said) => (answer = said)));
    const alarm = vi.fn();
    aPage({ reviewShell: { gone, alarm } });
    const failure = vi.spyOn(console, "error").mockImplementation(() => {});
    vi.doMock("./timeline", () => {
      throw new Error("The chunk is gone");
    });
    const slot = await aSlot();
    const leave = slot.whenImported(vi.fn());
    const attempt = slot.importTimeline();
    await until(() => expect(gone).toHaveBeenCalledTimes(1));
    leave();
    answer(false);
    await attempt;
    expect(alarm).not.toHaveBeenCalled();
    expect(failure).not.toHaveBeenCalled();
  });

  it("a chunk that does not come in a page with no shell is said on the console alone", async () => {
    aPage();
    const failure = vi.spyOn(console, "error").mockImplementation(() => {});
    vi.doMock("./timeline", () => {
      throw new Error("The chunk is gone");
    });
    const slot = await aSlot();
    slot.whenImported(vi.fn());
    await until(() => expect(failure).toHaveBeenCalledTimes(1));
  });

  it("asks for no chunk while its part has nothing to draw", async () => {
    aPage();
    const chunk = vi.fn(() => ({ TimelineHost: DrawnHost }));
    vi.doMock("./timeline", chunk);
    const slot = await aSlot();
    // The box nears the screen with no answer and no failure yet: its placeholder alone.
    const html = renderToStaticMarkup(<slot.TimelineSlot slug="tools/new" scope="" />);
    expect(html).not.toContain("timeline-root");
    expect(react.effects).toHaveLength(1);
    expect(react.effects[0]?.()).toBeUndefined();
    await slot.importTimeline();
    expect(chunk).not.toHaveBeenCalled();
  });

  it("gives the timeline its part of the page's store, drawn in the root the box's style lays out, its heights kept", async () => {
    const storage = aPage();
    const asked: string[] = [];
    vi.stubGlobal("fetch", async (url: string) => {
      asked.push(url);
      return {
        status: 200,
        ok: true,
        headers: { get: () => null },
        json: async () => (url.includes("task-tokens") ? { total: 3 } : { now: 7 }),
      } as unknown as Response;
    });
    vi.doMock("./timeline", () => ({ TimelineHost: DrawnHost }));
    const slot = await aSlot();
    // The store and the part the slot's own module names, not this file's copies.
    const { Parts, PartsContext } = await import("./parts");
    const { timelinePart, tokensPart } = await import("./task-live");
    const mount = new AbortController();
    const parts = new Parts(mount.signal);
    const part = timelinePart("", "tools/new");
    parts.open(part);
    await until(() => expect(parts.shown(part).answered).toBe(true));
    expect(asked).toEqual(["/api/task-timeline?slug=tools%2Fnew"]);
    // The box asks for the task's tokens beside its timeline: before they come the timeline is
    // handed none, and draws without.
    const spent = tokensPart("", "tools/new");
    const inPage = () =>
      renderToStaticMarkup(
        <PartsContext.Provider value={parts}>
          <slot.TimelineSlot slug="tools/new" scope="" />
        </PartsContext.Provider>,
      );
    // Before the chunk: the placeholder, the answer already held.
    expect(inPage()).not.toContain("timeline-root");
    const leave = react.effects.at(-1)?.();
    await until(() => expect(slot.loadedHost()).not.toBeNull());
    leave?.();
    // Drawn again, the timeline is in its root, and its height is kept for the next visit.
    expect(inPage()).toContain(
      '</div><div id="timeline-root"><i data-slug="tools/new" data-part="{&quot;now&quot;:7}"></i></div></div><div class="tl-message-place"></div></section>',
    );
    expect(storage.getItem(key("tools/new"))).toBe(`213,${WIDE}`);
    // With the chunk come, the box asks for nothing more.
    expect(react.effects.at(-1)?.()).toBeUndefined();
    expect(asked).toHaveLength(1);
    // The tokens, once their part answered, are handed on as the server sent them.
    parts.open(spent);
    await until(() => expect(parts.shown(spent).answered).toBe(true));
    expect(asked).toEqual([
      "/api/task-timeline?slug=tools%2Fnew",
      "/api/task-tokens?slug=tools%2Fnew",
    ]);
    expect(inPage()).toContain(
      'data-part="{&quot;now&quot;:7}" data-tokens="{&quot;total&quot;:3}"></i>',
    );
    mount.abort();
  });
});
