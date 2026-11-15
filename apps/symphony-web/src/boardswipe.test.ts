import { afterEach, describe, expect, it, vi } from "vitest";
import {
  type Board,
  nearestColumn,
  SwipeLimit,
  swipeTarget,
  useOneColumnASwipe,
} from "./boardswipe";

// The tests run without a DOM: the board here is a scroller 16 px from the window's edge, its
// columns 358 px wide, 12 apart, scrolled `left` px. The hook's effect runs as it is called, its
// cleanup kept for the test to run.

const effects = vi.hoisted(() => ({ cleanups: [] as (() => void)[] }));

vi.mock("react", async (original) => {
  const actual = await original<typeof import("react")>();
  return {
    ...actual,
    useEffect: (effect: () => undefined | (() => void)) => {
      const cleanup = effect();
      if (cleanup) effects.cleanups.push(cleanup);
    },
  };
});

vi.mock("./mount", () => ({ useMount: () => new AbortController().signal }));

afterEach(() => {
  vi.unstubAllGlobals();
});

const PITCH = 370;

function board(count = 6, snaps = true) {
  const state = { left: 0, snaps };
  const scrollBy = vi.fn();
  const fake = {
    clientLeft: 1,
    getBoundingClientRect: () => ({ left: 15 }),
    querySelectorAll: (selector: string) => {
      expect(selector).toBe(":scope > .pipeline > .column");
      return Array.from({ length: count }, (_, index) => ({
        getBoundingClientRect: () => ({ left: 16 + index * PITCH - state.left }),
      }));
    },
    scrollBy,
  } as unknown as Board;
  const limit = new SwipeLimit(fake, () => state.snaps);
  /** The board scrolled to show `column`, `off` px past its edge. */
  const at = (column: number, off = 0) => {
    state.left = column * PITCH + off;
  };
  return { limit, scrollBy, at, state };
}

describe("the column a board shows", () => {
  it("is the one whose left edge stands nearest the scroller's, the first of two as near", () => {
    expect(nearestColumn([16, 386, 756], 16)).toBe(0);
    expect(nearestColumn([-354, 16, 386], 16)).toBe(1);
    expect(nearestColumn([-200, 170], 16)).toBe(1);
    expect(nearestColumn([-169, 201], 16)).toBe(0);
    expect(nearestColumn([], 16)).toBeNull();
  });
});

describe("where a swipe goes on to", () => {
  it("is nowhere when it passed one column at most", () => {
    expect([swipeTarget(2, 2), swipeTarget(2, 3), swipeTarget(2, 1)]).toEqual([null, null, null]);
  });

  it("is the column next to where it began, in its direction, when it passed more", () => {
    expect([swipeTarget(1, 3), swipeTarget(0, 5), swipeTarget(4, 1)]).toEqual([2, 1, 3]);
  });

  it("is nowhere without the column it began on or the one it ended on", () => {
    expect([swipeTarget(null, 3), swipeTarget(1, null)]).toEqual([null, null]);
  });
});

describe("a swipe across the board", () => {
  it("that a finger's momentum carried two columns on goes on to the next column only", () => {
    const { limit, scrollBy, at } = board();
    at(1);
    limit.down({ pointerType: "touch" });
    at(3);
    limit.ended();
    expect(scrollBy).toHaveBeenCalledExactlyOnceWith({ left: -PITCH, behavior: "smooth" });
  });

  it("back, or a pen's, is held to one column the same way", () => {
    const { limit, scrollBy, at } = board();
    at(4);
    limit.down({ pointerType: "pen" });
    at(1);
    limit.ended();
    expect(scrollBy).toHaveBeenCalledExactlyOnceWith({ left: 2 * PITCH, behavior: "smooth" });
  });

  it("that passed one column, or none, stays where it ended", () => {
    const { limit, scrollBy, at } = board();
    at(2);
    limit.down({ pointerType: "touch" });
    at(3);
    limit.ended();
    limit.down({ pointerType: "touch" });
    limit.ended();
    expect(scrollBy).not.toHaveBeenCalled();
  });

  it("begins on the column nearest the scroller's edge when the finger came down mid-scroll", () => {
    const { limit, scrollBy, at } = board();
    at(1, 150);
    limit.down({ pointerType: "touch" });
    at(3);
    limit.ended();
    expect(scrollBy).toHaveBeenCalledExactlyOnceWith({ left: -PITCH, behavior: "smooth" });
  });

  it("is a mouse's, or one on a board that does not snap to each column, left where it ends", () => {
    const mouse = board();
    mouse.limit.down({ pointerType: "mouse" });
    mouse.at(3);
    mouse.limit.ended();
    expect(mouse.scrollBy).not.toHaveBeenCalled();
    const desk = board(6, false);
    desk.limit.down({ pointerType: "touch" });
    desk.at(3);
    desk.limit.ended();
    expect(desk.scrollBy).not.toHaveBeenCalled();
  });

  it("ends once: a scroll ending with no press since, a key's or its own going on, moves nothing", () => {
    const { limit, scrollBy, at } = board();
    limit.down({ pointerType: "touch" });
    at(3);
    limit.ended();
    expect(scrollBy).toHaveBeenCalledOnce();
    at(5);
    limit.ended();
    expect(scrollBy).toHaveBeenCalledOnce();
    // A mouse's press after a finger's forgets the finger's.
    limit.down({ pointerType: "touch" });
    limit.down({ pointerType: "mouse" });
    at(1);
    limit.ended();
    expect(scrollBy).toHaveBeenCalledOnce();
  });

  it("is no swipe after a tap: the scroll ending next, a key's, a focus move's or a script's, stays", () => {
    const { limit, scrollBy, at } = board();
    at(3);
    for (const pointerType of ["touch", "pen"]) {
      limit.down({ pointerType });
      limit.up();
      at(5);
      limit.ended();
      at(3);
      limit.ended();
    }
    expect(scrollBy).not.toHaveBeenCalled();
    // A finger's swipe after the tap is held as any other.
    limit.down({ pointerType: "touch" });
    at(1);
    limit.ended();
    expect(scrollBy).toHaveBeenCalledExactlyOnceWith({ left: PITCH, behavior: "smooth" });
  });

  it("on a board with no column shows none and moves nothing", () => {
    const { limit, scrollBy } = board(0);
    limit.down({ pointerType: "touch" });
    limit.ended();
    expect(scrollBy).not.toHaveBeenCalled();
  });
});

describe("the board's swipes held to one column", () => {
  it("listen on its scroller while it is drawn, asking how it snaps, until it goes", () => {
    const listeners: [string, (event: unknown) => void, AddEventListenerOptions][] = [];
    const scrollBy = vi.fn();
    const node = {
      clientLeft: 0,
      getBoundingClientRect: () => ({ left: 0 }),
      querySelectorAll: () =>
        [0, 1, 2].map((index) => ({ getBoundingClientRect: () => ({ left: index * 100 }) })),
      scrollBy,
      addEventListener: (
        name: string,
        listener: (event: unknown) => void,
        options: AddEventListenerOptions,
      ) => listeners.push([name, listener, options]),
    } as unknown as HTMLElement;
    const style = vi.fn(() => ({ scrollSnapType: "x mandatory" }));
    vi.stubGlobal("getComputedStyle", style);
    useOneColumnASwipe({ current: null }, true);
    useOneColumnASwipe({ current: node }, false);
    expect(listeners).toEqual([]);
    useOneColumnASwipe({ current: node }, true);
    expect(listeners.map(([name, , options]) => [name, options.passive])).toEqual([
      ["pointerdown", true],
      ["pointerup", true],
      ["scrollend", undefined],
    ]);
    const [down, up, ended] = listeners.map(([, listener]) => listener);
    // A tap on the board's first column, then a scroll ending two on: it stays.
    down?.({ pointerType: "touch" });
    expect(style).toHaveBeenCalledExactlyOnceWith(node);
    up?.({ pointerType: "touch" });
    node.getBoundingClientRect = () => ({ left: 200 }) as DOMRect;
    ended?.({});
    expect(scrollBy).not.toHaveBeenCalled();
    // A swipe from there back to the first column goes on to the second only.
    down?.({ pointerType: "touch" });
    node.getBoundingClientRect = () => ({ left: 0 }) as DOMRect;
    ended?.({});
    expect(scrollBy).toHaveBeenCalledExactlyOnceWith({ left: 100, behavior: "smooth" });
    const signal = listeners[0]?.[2].signal;
    expect(signal?.aborted).toBe(false);
    effects.cleanups.pop()?.();
    expect(signal?.aborted).toBe(true);
  });
});
