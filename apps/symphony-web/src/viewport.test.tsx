import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import { memoryStorage } from "./testing";
import {
  drawnHeight,
  drawnStyle,
  keepHeight,
  linesFor,
  NearViewport,
  onScreen,
  recallHeight,
  Skeleton,
  waitsNear,
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

  it("is mounted at once when the address names it or an anchor it holds", () => {
    vi.stubGlobal("location", { hash: "#part" });
    for (const named of [{ id: "part" }, { anchors: ["part"] }]) {
      const markup = renderToStaticMarkup(
        <NearViewport {...named}>
          <b>content</b>
        </NearViewport>,
      );
      expect(markup).toContain("<b>content</b>");
      expect(markup).toContain("min-height:0px");
    }
  });
});
