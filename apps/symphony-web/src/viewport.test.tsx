import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import { memoryStorage, observers as pageObservers } from "./testing";
import {
  AtOnce,
  drawnHeight,
  drawnStyle,
  keepHeight,
  linesFor,
  NearViewport,
  onScreen,
  recallHeight,
  SETTLE_MS,
  Settle,
  Skeleton,
  unsettled,
  useVisibility,
  waitsNear,
  watchSettle,
  watchUntilShown,
} from "./viewport";

describe("a placeholder's lines", () => {
  it("are one per line of text its height holds, one to six", () => {
    expect(linesFor(0)).toBe(1);
    expect(linesFor(52)).toBe(2);
    expect(linesFor(77)).toBe(2);
    expect(linesFor(1_000)).toBe(6);
  });
});

describe("the heights parts were drawn at", () => {
  it("are kept newest first, a part's old line replaced", () => {
    let kept = keepHeight("", "queue:you", 390, 180);
    kept = keepHeight(kept, "queue:spot", 390, 90);
    kept = keepHeight(kept, "queue:you", 390, 200);
    expect(kept).toBe("390 200 queue:you\n390 90 queue:spot");
  });

  it("are read back at the width they were drawn at", () => {
    const kept = keepHeight("", "queue:you", 390, 180);
    expect(recallHeight(kept, "queue:you", 390)).toBe(180);
    expect(recallHeight(kept, "queue:you", 1280)).toBeNull();
    expect(recallHeight(kept, "queue:spot", 390)).toBeNull();
  });

  it("give a part never drawn the height the last of its kind took", () => {
    const kept = keepHeight(keepHeight("", "doc a", 390, 40), "doc b", 390, 60);
    expect(recallHeight(kept, "doc c", 390)).toBe(60);
    expect(recallHeight(kept, "other c", 390)).toBeNull();
  });

  it("skip lines they cannot read", () => {
    expect(recallHeight("x 1 k\n390 y k\n390\n390 12 k", "k", 390)).toBe(12);
  });

  it("keep a hundred and twenty-eight parts at most", () => {
    let kept = "";
    for (let part = 0; part < 130; part += 1) kept = keepHeight(kept, `p${part}`, 1, part);
    expect(kept.split("\n")).toHaveLength(128);
    expect(recallHeight(kept, "p0", 1)).toBeNull();
    expect(recallHeight(kept, "p129", 1)).toBe(129);
  });
});

describe("a ghost's box", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("is as tall as its part was drawn in a window as wide, or as its rows make it", () => {
    vi.stubGlobal("localStorage", memoryStorage());
    vi.stubGlobal("innerWidth", 390.4);
    expect(drawnHeight("queue:you")).toBeNull();
    localStorage.setItem("drawn-heights", "390 180 queue:you");
    expect(drawnHeight("queue:you")).toBe(180);
    expect(drawnStyle(180)).toEqual({
      boxSizing: "border-box",
      height: "180px",
      overflow: "hidden",
    });
    expect(drawnStyle(null)).toBeUndefined();
  });
});

describe("a region on the screen", () => {
  const screen = [0, 800, 0, 400];

  it("shows when its box crosses the screen", () => {
    expect(onScreen([100, 200, 0, 50], screen)).toBe(true);
    expect(onScreen([-10, 1, 399, 500], screen)).toBe(true);
  });

  it("does not when it is out of the screen or has no size", () => {
    expect(onScreen([800, 900, 0, 50], screen)).toBe(false);
    expect(onScreen([-100, 0, 0, 50], screen)).toBe(false);
    expect(onScreen([100, 200, 400, 450], screen)).toBe(false);
    expect(onScreen([100, 100, 0, 50], screen)).toBe(false);
    expect(onScreen([], screen)).toBe(false);
  });
});

describe("what waits near the screen", () => {
  afterEach(() => vi.unstubAllGlobals());

  const box = (top: number, bottom: number, left = 0, right = 100) => ({
    getBoundingClientRect: () => ({ top, bottom, left, right }),
  });
  /** A part of the page, in `scroller` when it is in one, holding `waiting` regions. */
  const part = (scroller: unknown, ...waiting: unknown[]) =>
    ({ closest: () => scroller, querySelectorAll: () => waiting }) as unknown as Element;

  it("is a region waiting within the sentinel's reach of the window", () => {
    vi.stubGlobal("innerHeight", 800);
    vi.stubGlobal("innerWidth", 400);
    expect(waitsNear(part(null, box(1300, 1350)))).toBe(true);
    expect(waitsNear(part(null, box(1500, 1550)))).toBe(false);
    expect(waitsNear(part(null))).toBe(false);
  });

  it("is cut to the box of the scroller it sits in", () => {
    vi.stubGlobal("innerHeight", 800);
    vi.stubGlobal("innerWidth", 400);
    // The scroller ends 300 px down: its reach ends 600 px below that.
    expect(waitsNear(part(box(0, 300), box(1300, 1350)), ".board-scroll")).toBe(false);
    expect(waitsNear(part(box(0, 300), box(850, 900)), ".board-scroll")).toBe(true);
    // A column scrolled out of the scroller's sides never shows, however near it is.
    expect(waitsNear(part(box(0, 800, 0, 200), box(100, 200, 250, 300)), ".board-scroll")).toBe(
      false,
    );
    // Outside any scroller, the window's reach.
    expect(waitsNear(part(null, box(1300, 1350)), ".board-scroll")).toBe(true);
  });
});

describe("a paragraph waiting for its read", () => {
  it("is a bar per line, and its label for a reader of the page's text", () => {
    expect(renderToStaticMarkup(<Skeleton lines={2} label="Loading…" />)).toBe(
      '<div class="skeleton"><span class="status-text" role="status">Loading…</span>' +
        '<span class="bone" aria-hidden="true"></span><span class="bone" aria-hidden="true"></span></div>',
    );
    expect(renderToStaticMarkup(<Skeleton />).match(/class="bone"/g)).toHaveLength(3);
  });
});

describe("content near the screen", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("is its ghost in a box of its height until it comes near, with the anchors it holds", () => {
    const markup = renderToStaticMarkup(
      <NearViewport height={64} ghost={<i>ghost</i>} anchors={["a"]} className="row">
        <b>content</b>
      </NearViewport>,
    );
    expect(markup).toContain('class="viewport-slot row"');
    expect(markup).toContain("contain-intrinsic-size:auto 64px;min-height:64px");
    expect(markup).toContain('data-pending="true"><span id="a"></span><i>ghost</i>');
    expect(markup).not.toContain("content");
  });

  it("is a list's item where it stands in a list", () => {
    const markup = renderToStaticMarkup(
      <NearViewport id="tree-a" height={40} listItem>
        <b>content</b>
      </NearViewport>,
    );
    expect(markup).toMatch(/^<li id="tree-a" style="[^"]*" class="viewport-slot ">/);
  });

  it("is lines of text without a ghost", () => {
    const markup = renderToStaticMarkup(
      <NearViewport height={52}>
        <b>content</b>
      </NearViewport>,
    );
    expect(markup.match(/class="bone"/g)).toHaveLength(2);
  });

  it("is mounted at once under AtOnce, which leaves the latch elsewhere as it was", () => {
    observers();
    const markup = renderToStaticMarkup(
      <>
        <AtOnce.Provider value={true}>
          <NearViewport height={64}>
            <b>asked for</b>
          </NearViewport>
        </AtOnce.Provider>
        <NearViewport height={64}>
          <b>beside it</b>
        </NearViewport>
      </>,
    );
    expect(markup).toContain("<b>asked for</b>");
    expect(markup).not.toContain("beside it");
    expect(markup.match(/class="viewport-sentinel"/g)).toHaveLength(1);
  });

  it("is mounted at once when the address names it or an anchor it holds", () => {
    // A heading named by its text (`#The%20part`) names the anchor the renderer gives it.
    const cases: [string, { id?: string; anchors?: string[] }][] = [
      ["#part", { id: "part" }],
      ["#part", { anchors: ["part"] }],
      ["#The%20part", { id: "the-part" }],
      ["#The%20part", { anchors: ["the-part"] }],
    ];
    for (const [hash, named] of cases) {
      vi.stubGlobal("location", { hash });
      const markup = renderToStaticMarkup(
        <NearViewport {...named}>
          <b>content</b>
        </NearViewport>,
      );
      expect(markup, hash).toContain("<b>content</b>");
      expect(markup, hash).toContain("min-height:0px");
    }
  });

  it("waits as any part when the address names another", () => {
    vi.stubGlobal("location", { hash: "#The%20other" });
    const markup = renderToStaticMarkup(
      <NearViewport anchors={["the-part"]} height={64}>
        <b>content</b>
      </NearViewport>,
    );
    expect(markup).not.toContain("content");
  });

  it("waits as any part when the address names it but it lands no more, its anchors held", () => {
    vi.stubGlobal("location", { hash: "#part" });
    for (const named of [{ id: "part" }, { anchors: ["part"] }]) {
      const markup = renderToStaticMarkup(
        <NearViewport {...named} height={64} ghost={<i>ghost</i>} lands={false}>
          <b>content</b>
        </NearViewport>,
      );
      expect(markup).not.toContain("content");
      expect(markup).toContain("min-height:64px");
      expect(markup.match(/id="part"/g)).toHaveLength(1);
    }
  });

  it("is mounted at once when eager, near the screen or not", () => {
    const markup = renderToStaticMarkup(
      <NearViewport height={64} ghost={<i>ghost</i>} eager>
        <b>content</b>
      </NearViewport>,
    );
    expect(markup).toContain("<b>content</b>");
    expect(markup).toContain("min-height:0px");
    expect(markup).not.toContain("ghost");
    expect(markup).not.toContain("viewport-sentinel");
  });
});

/** An IntersectionObserver whose callback a test calls, and what each was asked to do. */
function observers() {
  const made: { said: (entries: { isIntersecting: boolean }[]) => void; log: string[] }[] = [];
  vi.stubGlobal(
    "IntersectionObserver",
    class {
      log: string[] = [];
      constructor(said: (entries: { isIntersecting: boolean }[]) => void) {
        made.push({ said, log: this.log });
      }
      observe(node: { id: string }) {
        this.log.push(`observe ${node.id}`);
      }
      disconnect() {
        this.log.push("disconnect");
      }
      takeRecords() {
        this.log.push("takeRecords");
        return [];
      }
    },
  );
  return made;
}

function Latched({ initial = false }: { initial?: boolean }) {
  const [shown] = useVisibility(initial);
  return <i>{shown ? "shown" : "waiting"}</i>;
}

describe("a part that keeps its box until it comes into view", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("waits where the browser can tell, unless it starts shown", () => {
    observers();
    expect(renderToStaticMarkup(<Latched />)).toBe("<i>waiting</i>");
    expect(renderToStaticMarkup(<Latched initial />)).toBe("<i>shown</i>");
  });

  it("shows at once under AtOnce, where the browser could tell", () => {
    observers();
    expect(
      renderToStaticMarkup(
        <AtOnce.Provider value={true}>
          <Latched />
        </AtOnce.Provider>,
      ),
    ).toBe("<i>shown</i>");
    expect(
      renderToStaticMarkup(
        <AtOnce.Provider value={false}>
          <Latched />
        </AtOnce.Provider>,
      ),
    ).toBe("<i>waiting</i>");
  });

  it("shows at once where nothing can tell it came into view", () => {
    expect(renderToStaticMarkup(<Latched />)).toBe("<i>shown</i>");
  });

  it("is shown once its element crosses into view, and its watch then stops", () => {
    const made = observers();
    const shown: string[] = [];
    const stop = watchUntilShown({ id: "row" } as unknown as Element, () => shown.push("row"));
    const [watched] = made;
    expect(watched?.log).toEqual(["observe row"]);
    watched?.said([{ isIntersecting: false }]);
    expect(shown).toEqual([]);
    watched?.said([{ isIntersecting: false }, { isIntersecting: true }]);
    expect(shown).toEqual(["row"]);
    stop();
    expect(watched?.log).toEqual(["observe row", "disconnect", "takeRecords"]);
  });
});

describe("a page's first screen", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("is drawn hidden under its cover until it settles", () => {
    const markup = renderToStaticMarkup(
      <Settle cover={<i>ghost</i>}>
        <b>page</b>
      </Settle>,
    );
    expect(markup).toMatch(
      /^<div class="settle" data-settle="web-\d+" data-settled="false" aria-busy="true"><div class="settle-cover"><i>ghost<\/i><\/div><b>page<\/b><\/div>$/,
    );
  });

  it("is drawn at once when its read failed, with no cover over it", () => {
    const markup = renderToStaticMarkup(
      <Settle cover={<i>ghost</i>} failed>
        <b>page</b>
      </Settle>,
    );
    expect(markup).toContain('data-settled="true" aria-busy="false"><b>page</b></div>');
    expect(markup).not.toContain("ghost");
  });

  /** A page's root holding `count` elements, each region of `waiting` still waiting for its read. */
  const root = (count: number, waiting: number[][]) =>
    ({
      childElementCount: count,
      getBoundingClientRect: () => ({ left: 0, right: 400 }),
      querySelectorAll: () =>
        waiting.map(([top = 0, bottom = 0, left = 0, right = 0]) => ({
          getBoundingClientRect: () => ({ top, bottom, left, right }),
        })),
    }) as unknown as Element;

  it("waits while only its cover is in, or a region on the screen waits for its read", () => {
    vi.stubGlobal("innerHeight", 800);
    vi.stubGlobal("innerWidth", 1000);
    expect(unsettled(root(1, []))).toBe(true);
    expect(unsettled(root(2, [[100, 200, 0, 300]]))).toBe(true);
    // Below the screen, or beside the page's own width, a region waits for no one yet.
    expect(unsettled(root(2, [[900, 1000, 0, 300]]))).toBe(false);
    expect(unsettled(root(2, [[100, 200, 500, 600]]))).toBe(false);
    expect(unsettled(root(2, []))).toBe(false);
  });
});

describe("the watch of a page's first screen", () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  /**
   * A page 800 px high whose root holds one region still waiting for its read, at `top` (moved
   * by the test), heard by its observers (`changed`, `resized`) and its window's listeners.
   */
  function page(top: number) {
    vi.useFakeTimers();
    vi.stubGlobal("innerHeight", 800);
    vi.stubGlobal("innerWidth", 1000);
    const seen = pageObservers();
    const view = new EventTarget();
    vi.stubGlobal("addEventListener", view.addEventListener.bind(view));
    const whole = {};
    vi.stubGlobal("document", { documentElement: whole });
    const region = { top, waits: true };
    const node = {
      childElementCount: 2,
      getBoundingClientRect: () => ({ left: 0, right: 400 }),
      querySelectorAll: () =>
        region.waits
          ? [
              {
                getBoundingClientRect: () => ({
                  top: region.top,
                  bottom: region.top + 100,
                  left: 0,
                  right: 300,
                }),
              },
            ]
          : [],
    } as unknown as Element;
    const settled = vi.fn();
    return { ...seen, view, whole, region, node, settled };
  }

  it("settles at once when nothing on its first screen waits", () => {
    const { node, settled, watched, sized } = page(900);
    watchSettle(node, settled);
    expect(settled).toHaveBeenCalledOnce();
    expect(watched.size + sized.size).toBe(0);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("settles as what draws above it moves its waiting region off the screen", () => {
    const { node, region, settled, sized, whole, resized } = page(100);
    watchSettle(node, settled);
    expect(settled).not.toHaveBeenCalled();
    expect([...sized.values()]).toEqual([[node, whole]]);
    // The gates and the timeline drawn above it on a phone: its own box neither changed nor moved
    // under it, the page's size did.
    region.top = 900;
    resized(whole);
    expect(settled).toHaveBeenCalledOnce();
    expect(sized.size).toBe(0);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("settles as its region is drawn, and looks again at a scroll or a resize", () => {
    const { node, region, settled, changed, view } = page(100);
    watchSettle(node, settled);
    changed({ attribute: "class" });
    view.dispatchEvent(new Event("scroll"));
    view.dispatchEvent(new Event("resize"));
    expect(settled).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(1);
    region.waits = false;
    changed({ attribute: "data-pending" });
    expect(settled).toHaveBeenCalledOnce();
    // Its deadline ended with it: a page settled by its event leaves no timer waiting.
    expect(vi.getTimerCount()).toBe(0);
    view.dispatchEvent(new Event("scroll"));
    expect(settled).toHaveBeenCalledOnce();
  });

  it("settles at its deadline when its region never comes, and only once", () => {
    const { node, settled, view, watched } = page(100);
    watchSettle(node, settled);
    vi.advanceTimersByTime(SETTLE_MS - 1);
    expect(settled).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(settled).toHaveBeenCalledOnce();
    expect(watched.size).toBe(0);
    view.dispatchEvent(new Event("scroll"));
    vi.advanceTimersByTime(SETTLE_MS);
    expect(settled).toHaveBeenCalledOnce();
  });

  it("stops unsettled, hearing nothing after", () => {
    const { node, region, settled, view, watched, sized } = page(100);
    const stop = watchSettle(node, settled);
    stop();
    expect(watched.size + sized.size).toBe(0);
    expect(vi.getTimerCount()).toBe(0);
    region.waits = false;
    view.dispatchEvent(new Event("scroll"));
    expect(settled).not.toHaveBeenCalled();
  });
});
