import { afterEach, describe, expect, it, vi } from "vitest";
import {
  CURRENT,
  FADE,
  followRow,
  mark,
  overflowSides,
  type Row,
  reveal,
  revealed,
  useScrollRow,
} from "./scrollrow";

// The tests run without a DOM: a row here is 300 px wide at the window's 10 px, its choices 100 px
// wide with no gap, scrolled `left` px. The hook's layout effects run as they are called.

vi.mock("react", async (original) => {
  const actual = await original<typeof import("react")>();
  return {
    ...actual,
    useCallback: (made: unknown) => made,
    useRef: (current: unknown) => ({ current }),
    useLayoutEffect: (effect: () => void) => effect(),
  };
});

afterEach(() => {
  vi.unstubAllGlobals();
});

/** A row of `count` choices scrolled to `left`, its current choice the one at `current` (none
 * when null). `cut` holds the choices marked cut, by their place. */
function row(count: number, left: number, current: number | null) {
  const cut = new Set<number>();
  const choice = (at: number) => ({
    getBoundingClientRect: () => ({ left: 10 + at * 100 - made.scrollLeft, width: 100 }) as DOMRect,
    toggleAttribute(name: string, on?: boolean) {
      expect(name).toBe("data-cut");
      if (on) cut.add(at);
      else cut.delete(at);
      return on === true;
    },
  });
  const made = {
    scrollLeft: left,
    clientWidth: 300,
    scrollWidth: count * 100,
    clientLeft: 0,
    dataset: {} as DOMStringMap,
    asked: "",
    cut,
    children: Array.from({ length: count }, (_, at) => choice(at)),
    getBoundingClientRect: () => ({ left: 10, width: 300 }) as DOMRect,
    querySelector(selector: string) {
      made.asked = selector;
      return current === null ? null : choice(current);
    },
  };
  return made;
}

describe("the sides a row continues past", () => {
  it.each([
    [0, 300, 300, "none"],
    [0, 300, 900, "end"],
    [600, 300, 900, "start"],
    [200, 300, 900, "both"],
    // A row with nothing in it, and one wider than what it holds.
    [0, 0, 0, "none"],
    [0, 300, 120, "none"],
  ])("at %i of a row %i wide over %i is %s", (left, width, full, sides) => {
    expect(overflowSides(left, width, full)).toBe(sides);
  });

  it("takes a pixel of rounding at either end for no overflow", () => {
    expect(overflowSides(1, 300, 900)).toBe("end");
    expect(overflowSides(599, 300, 900)).toBe("start");
    expect(overflowSides(0.6, 300, 301)).toBe("none");
    expect(overflowSides(1.5, 300, 900)).toBe("both");
  });
});

describe("where a row goes to show a choice", () => {
  it("keeps the fade the sheet draws", () => {
    expect(FADE).toBe(20);
  });

  it("stays where the choice already shows whole", () => {
    // The first, at the start, where no fade is.
    expect(revealed(0, 300, 900, 0, 100)).toBe(0);
    // One in the middle, clear of both fades.
    expect(revealed(200, 300, 900, 300, 400)).toBe(200);
    // The last, at the end, where no fade is.
    expect(revealed(600, 300, 900, 800, 900)).toBe(600);
  });

  it("puts a choice cut at the end at the row's start, clear of its fade", () => {
    expect(revealed(0, 300, 900, 250, 350)).toBe(230);
    // Whole, but under the end's fade.
    expect(revealed(0, 300, 900, 190, 290)).toBe(170);
  });

  it("brings back a choice left behind the start", () => {
    expect(revealed(400, 300, 900, 300, 400)).toBe(280);
    // Whole, but under the start's fade.
    expect(revealed(290, 300, 900, 300, 400)).toBe(280);
    // The first goes to the very start.
    expect(revealed(400, 300, 900, 0, 100)).toBe(0);
  });

  it("stops at the row's end for the last choices", () => {
    expect(revealed(0, 300, 900, 800, 900)).toBe(600);
    expect(revealed(0, 300, 900, 700, 800)).toBe(600);
  });

  it("starts a choice wider than the row at its start", () => {
    expect(revealed(0, 300, 900, 400, 800)).toBe(380);
  });

  it("never moves a row that holds all it has", () => {
    expect(revealed(0, 300, 300, 200, 300)).toBe(0);
    expect(revealed(0, 300, 120, 20, 120)).toBe(0);
  });
});

describe("marking a row", () => {
  it("says the sides the row continues past", () => {
    const one = row(9, 200, null);
    mark(one);
    expect(one.dataset.overflow).toBe("both");
    one.scrollLeft = 600;
    mark(one);
    expect(one.dataset.overflow).toBe("start");
  });

  it("marks cut every choice but those whole in the row's window, anew at each call", () => {
    const one = row(9, 0, null);
    mark(one);
    expect([...one.cut]).toEqual([3, 4, 5, 6, 7, 8]);
    one.scrollLeft = 280;
    mark(one);
    // Snapped to the fourth, a fade's width in: it and the fifth are whole, the sixth is cut.
    expect([...one.cut].sort()).toEqual([0, 1, 2, 5, 6, 7, 8]);
    one.scrollLeft = 600;
    mark(one);
    expect([...one.cut].sort()).toEqual([0, 1, 2, 3, 4, 5]);
  });

  it("takes half a pixel of rounding for no cut", () => {
    const one = row(3, 0.4, null);
    mark(one);
    expect([...one.cut]).toEqual([]);
    one.scrollLeft = 0.6;
    mark(one);
    expect([...one.cut]).toEqual([0]);
  });

  it("measures a bordered row's choices from inside its border", () => {
    const one = row(3, 0, null);
    one.clientLeft = 2;
    one.clientWidth = 296;
    // The window is the 296 px inside a 2 px border: the first choice starts under the border and
    // the third ends past the window, where an unbordered row cuts neither.
    mark(one);
    expect([...one.cut]).toEqual([0, 2]);
    const plain = row(3, 0, null);
    mark(plain);
    expect([...plain.cut]).toEqual([]);
  });

  it("says none of a row that holds all it has, and cuts nothing in it", () => {
    const one = row(3, 0, null);
    mark(one);
    expect(one.dataset.overflow).toBe("none");
    expect(one.cut.size).toBe(0);
  });
});

describe("revealing a row's current choice", () => {
  it("asks the row for the page shown or the filter pressed", () => {
    const one = row(9, 0, 4);
    reveal(one);
    expect(one.asked).toBe(CURRENT);
    expect(CURRENT).toContain('[aria-current="page"]');
    expect(CURRENT).toContain('[aria-pressed="true"]');
  });

  it("scrolls to a choice out of view, measured from the row's own edge", () => {
    const one = row(9, 0, 4);
    reveal(one);
    expect(one.scrollLeft).toBe(380);
    // A row with a border: the choices start past it.
    const bordered = row(9, 0, 4);
    bordered.clientLeft = 2;
    reveal(bordered);
    expect(bordered.scrollLeft).toBe(378);
  });

  it("leaves a row alone whose choice shows or which has none", () => {
    const shown = row(9, 0, 1);
    const set = vi.fn();
    Object.defineProperty(shown, "scrollLeft", { get: () => 0, set });
    reveal(shown);
    expect(set).not.toHaveBeenCalled();
    const none = row(9, 100, null);
    reveal(none);
    expect(none.scrollLeft).toBe(100);
  });
});

/** A row as an element: its listeners and the observer made for it. */
function element(count: number, current: number | null) {
  const listeners = new Map<string, (event?: unknown) => void>();
  const watched: { resized: () => void; observed: unknown[]; disconnected: boolean }[] = [];
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observed: unknown[] = [];
      disconnected = false;
      constructor(readonly resized: () => void) {
        watched.push(this);
      }
      observe(node: unknown) {
        this.observed.push(node);
      }
      disconnect() {
        this.disconnected = true;
      }
    },
  );
  const node = Object.assign(row(count, 0, current), {
    isConnected: true,
    addEventListener: (name: string, heard: (event?: unknown) => void) =>
      listeners.set(name, heard),
    removeEventListener: (name: string) => listeners.delete(name),
  });
  return { node, as: node as unknown as HTMLElement & Row, listeners, watched };
}

describe("following a row", () => {
  it("marks it at each scroll and reveals its choice when its width changes, until ended", () => {
    const { node, as, listeners, watched } = element(9, 4);
    const end = followRow(as);
    expect(watched[0]?.observed).toEqual([node]);
    // The observer's first call, as the row is first measured.
    watched[0]?.resized();
    expect(node.scrollLeft).toBe(380);
    expect(node.dataset.overflow).toBe("both");
    node.scrollLeft = 600;
    listeners.get("scroll")?.();
    expect(node.dataset.overflow).toBe("start");
    expect([...node.cut].sort()).toEqual([0, 1, 2, 3, 4, 5]);
    end();
    expect(listeners.size).toBe(0);
    expect(watched[0]?.disconnected).toBe(true);
  });
});

describe("a choice that takes the focus", () => {
  it("is revealed and the row marked, whichever part of it the focus lands on", () => {
    const { node, as, listeners } = element(9, 0);
    const end = followRow(as);
    const inside = {};
    const choices = node.children.map((choice, at) => ({
      ...choice,
      contains: (target: unknown) => target === choices[at] || (at === 7 && target === inside),
    }));
    node.children = choices;
    listeners.get("focusin")?.({ target: choices[2] });
    // The third starts 200 in: at the row's start, a fade's width in.
    expect(node.scrollLeft).toBe(180);
    expect(node.dataset.overflow).toBe("both");
    expect(node.cut.has(2)).toBe(false);
    listeners.get("focusin")?.({ target: inside });
    expect(node.scrollLeft).toBe(600);
    expect(node.dataset.overflow).toBe("start");
    // A focus that is none of the row's choices moves nothing.
    listeners.get("focusin")?.({ target: {} });
    listeners.get("focusin")?.({ target: null });
    expect(node.scrollLeft).toBe(600);
    end();
    expect(listeners.size).toBe(0);
  });
});

describe("a scrolling row's ref", () => {
  it("follows the row it is given until that row goes", () => {
    const attach = useScrollRow("review");
    expect(attach(null)).toBeUndefined();
    const { node, as, listeners, watched } = element(9, 6);
    const detach = attach(as);
    expect(listeners.has("scroll")).toBe(true);
    watched[0]?.resized();
    expect(node.scrollLeft).toBe(580);
    detach?.();
    expect(listeners.size).toBe(0);
    expect(watched[0]?.disconnected).toBe(true);
  });

  it("draws with no row yet without touching one", () => {
    expect(() => useScrollRow("")).not.toThrow();
  });
});
