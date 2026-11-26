import type { ReactElement, ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import FIXTURE from "../../../crates/symphony-api/fixtures/task-timeline.json";
import { readTimeline, type View } from "./timeline-layout";
import {
  type LanesHandlers,
  type Navigation,
  OverviewStrip,
  useNavigation,
} from "./timeline-navigate";
import type { TaskTimeline } from "./wire";

// The hooks as plain values, so a component or a hook runs as a function, with no renderer: a
// ref is an object, a state a cell the test reads, and an effect is kept for the test to run.
const react = vi.hoisted(() => ({
  effects: [] as (() => undefined | (() => void))[],
  states: [] as { value: unknown }[],
}));
vi.mock("react", async (original) => {
  const actual = await original<typeof import("react")>();
  return {
    ...actual,
    useEffect: (effect: () => undefined | (() => void)) => {
      react.effects.push(effect);
    },
    useRef: (initial: unknown) => ({ current: initial }),
    useState: (initial: unknown) => {
      const cell = { value: initial };
      react.states.push(cell);
      return [initial, (next: unknown) => (cell.value = next)];
    },
  };
});

beforeEach(() => {
  react.effects.length = 0;
  react.states.length = 0;
});

/** Ten hours, from 0. */
const WHOLE: View = { from: 0, to: 36_000 };
/** The view from `from` to `to` hours. */
const hours = (from: number, to: number): View => ({ from: from * 3600, to: to * 3600 });

/** The lanes as the handlers read them: their tracks 1000 px wide from 200 px across the screen
 * (none drawn where `width` is 0), the wheel's listener and each pointer captured. */
function lanes(width = 1000) {
  const listeners = new Map<string, { listener: (event: unknown) => void; options: unknown }>();
  const captured: number[] = [];
  const element = {
    querySelector: (selector: string) =>
      selector === ".tl-hours" ? { getBoundingClientRect: () => ({ left: 200, width }) } : null,
    addEventListener: (type: string, listener: (event: unknown) => void, options: unknown) =>
      listeners.set(type, { listener, options }),
    removeEventListener: (type: string, listener: unknown) => {
      if (listeners.get(type)?.listener === listener) listeners.delete(type);
    },
    setPointerCapture: (id: number) => captured.push(id),
  };
  return { ref: { current: element as unknown as HTMLDivElement }, listeners, captured };
}

/** A navigation over the ten hours showing `view`, each view it is given kept. */
function navigation(view: View | null): Navigation & { given: (View | null)[] } {
  const given: (View | null)[] = [];
  return { whole: WHOLE, view, set: (next) => given.push(next), given };
}

/** The lanes' handlers for `nav` over `width` px of tracks, and how to read what they did. */
function navigated(nav: Navigation, width = 1000) {
  const at = lanes(width);
  // biome-ignore lint/correctness/useHookAtTopLevel: its hooks are this file's plain values
  const { handlers } = useNavigation(at.ref, nav);
  const band = () => react.states[0]?.value;
  return { ...at, handlers, band };
}

type Pointer = Parameters<LanesHandlers["onPointerDown"]>[0];

/** A pointer's event at `x` (`y`), the left button's, a mouse's unless `kind` says otherwise. */
const pointer = (x: number, more: Record<string, unknown> = {}) =>
  ({
    pointerType: "mouse",
    button: 0,
    pointerId: 1,
    clientX: x,
    clientY: 100,
    ...more,
  }) as unknown as Pointer;

/** An event that says whether it was kept from the page. */
function kept<T extends object>(event: T) {
  const said = { prevented: false, stopped: false };
  return {
    event: {
      ...event,
      preventDefault: () => (said.prevented = true),
      stopPropagation: () => (said.stopped = true),
    } as never,
    said,
  };
}

describe("the lanes' wheel", () => {
  /** A wheel's event at `x` by `deltaY`, nothing held, with what `more` changes. */
  const wheel = (x: number, deltaY: number, more: Record<string, unknown> = {}) =>
    kept({
      clientX: x,
      deltaX: 0,
      deltaY,
      deltaMode: 0,
      ctrlKey: false,
      metaKey: false,
      shiftKey: false,
      ...more,
    });

  it("is a listener of its own that may keep the page from scrolling, gone with the lanes", () => {
    const at = navigated(navigation(null));
    const leave = react.effects[0]?.();
    expect(at.listeners.get("wheel")?.options).toEqual({ passive: false });
    leave?.();
    expect(at.listeners.has("wheel")).toBe(false);
  });

  it("zooms about the pointer with ⌘ or Ctrl, each turn on from the last", () => {
    const nav = navigation(null);
    const at = navigated(nav);
    react.effects[0]?.();
    const listener = at.listeners.get("wheel")?.listener;
    // At the middle, up a notch with Ctrl, then with ⌘: half, then a quarter.
    const first = wheel(700, -300, { ctrlKey: true });
    listener?.(first.event);
    listener?.(wheel(700, -300, { metaKey: true }).event);
    expect(nav.given).toEqual([hours(2.5, 7.5), hours(3.75, 6.25)]);
    expect(first.said.prevented).toBe(true);
  });

  it("moves a zoomed view by Shift + wheel, and leaves a plain wheel to the page", () => {
    const nav = navigation(hours(2, 4));
    const at = navigated(nav);
    react.effects[0]?.();
    const listener = at.listeners.get("wheel")?.listener;
    const plain = wheel(700, 250);
    listener?.(plain.event);
    expect([nav.given, plain.said.prevented]).toEqual([[], false]);
    listener?.(wheel(700, 250, { shiftKey: true }).event);
    expect(nav.given).toEqual([hours(2.5, 4.5)]);
  });

  it("does nothing where the tracks are not drawn, or with no lanes", () => {
    const nav = navigation(null);
    const at = navigated(nav, 0);
    react.effects[0]?.();
    at.listeners.get("wheel")?.listener(wheel(700, -300, { ctrlKey: true }).event);
    expect(nav.given).toEqual([]);
    react.effects.length = 0;
    useNavigation({ current: null }, nav);
    expect(react.effects[0]?.()).toBeUndefined();
  });
});

describe("a mouse on the lanes", () => {
  it("dragged across the tracks draws its band, zooms to the hours dragged, its click the drag's", () => {
    const nav = navigation(null);
    const at = navigated(nav);
    at.handlers.onPointerDown(pointer(450));
    // Under 4 px it is still a click: no band, the pointer not captured.
    at.handlers.onPointerMove(pointer(453));
    expect([at.band(), at.captured]).toEqual([null, []]);
    at.handlers.onPointerMove(pointer(700));
    expect([at.band(), at.captured]).toEqual([[0.25, 0.5], [1]]);
    // Past the tracks' end, the band stops at it.
    at.handlers.onPointerMove(pointer(1400));
    expect(at.band()).toEqual([0.25, 1]);
    at.handlers.onPointerUp(pointer(700));
    expect([at.band(), nav.given]).toEqual([null, [hours(2.5, 5)]]);
    // The click its release makes picks nothing; the next click is a click.
    const click = kept({});
    at.handlers.onClickCapture(click.event);
    expect(click.said).toEqual({ prevented: true, stopped: true });
    const next = kept({});
    at.handlers.onClickCapture(next.event);
    expect(next.said).toEqual({ prevented: false, stopped: false });
  });

  it("pressed without moving is a click, which the lanes' parts take", () => {
    const nav = navigation(null);
    const at = navigated(nav);
    at.handlers.onPointerDown(pointer(450));
    at.handlers.onPointerUp(pointer(451));
    const click = kept({});
    at.handlers.onClickCapture(click.event);
    expect([nav.given, click.said.prevented]).toEqual([[], false]);
  });

  it("drags nothing with another button, from outside the tracks, or for another pointer", () => {
    const nav = navigation(null);
    const at = navigated(nav);
    for (const press of [pointer(450, { button: 2 }), pointer(100), pointer(1300)]) {
      at.handlers.onPointerDown(press);
      at.handlers.onPointerMove(pointer(700));
      at.handlers.onPointerUp(pointer(700));
    }
    at.handlers.onPointerDown(pointer(450));
    at.handlers.onPointerMove(pointer(700, { pointerId: 2 }));
    expect([at.band(), nav.given]).toEqual([null, []]);
  });

  it("cancelled mid-drag takes its band away and zooms nowhere", () => {
    const nav = navigation(null);
    const at = navigated(nav);
    at.handlers.onPointerDown(pointer(450));
    at.handlers.onPointerMove(pointer(700));
    at.handlers.onPointerCancel(pointer(700));
    at.handlers.onPointerUp(pointer(700));
    expect([at.band(), nav.given]).toEqual([null, []]);
  });

  it("double clicked on a stage zooms to it, and elsewhere does nothing", () => {
    const nav = navigation(null);
    const at = navigated(nav);
    /** A double click on a part with `data` on it, or on none. */
    const on = (data: Record<string, string> | null) =>
      ({ target: { closest: () => (data ? { dataset: data } : null) } }) as never;
    at.handlers.onDoubleClick(on({ from: "3600", to: "9000" }));
    at.handlers.onDoubleClick(on(null));
    at.handlers.onDoubleClick(on({ from: "soon", to: "9000" }));
    expect(nav.given).toEqual([hours(1, 2.5)]);
  });
});

describe("the lanes' keys", () => {
  it("zoom with + and −, show the whole task with 0, and leave the rest to the page", () => {
    const nav = navigation(hours(2, 6));
    const at = navigated(nav);
    const plus = kept({ key: "+" });
    at.handlers.onKeyDown(plus.event);
    at.handlers.onKeyDown(kept({ key: "0" }).event);
    const page = kept({ key: "+", metaKey: true });
    at.handlers.onKeyDown(page.event);
    at.handlers.onKeyDown(kept({ key: "ArrowRight" }).event);
    expect(nav.given).toEqual([hours(3, 5), null]);
    expect([plus.said.prevented, page.said.prevented]).toEqual([true, false]);
  });
});

describe("a touch screen's fingers on the lanes", () => {
  /** A finger `id` at `x`, `y`. */
  const finger = (id: number, x: number, y = 100) =>
    pointer(x, { pointerType: "touch", pointerId: id, clientY: y });

  it("pinch about themselves, and a finger left goes on from the view then", () => {
    const nav = navigation(null);
    const at = navigated(nav);
    at.handlers.onPointerDown(finger(1, 600));
    at.handlers.onPointerDown(finger(2, 800));
    // Spread from 200 px to 400 about the middle: half as long, about the middle.
    at.handlers.onPointerMove(finger(1, 500));
    at.handlers.onPointerMove(finger(2, 900));
    expect(nav.given.at(-1)).toEqual(hours(2.5, 7.5));
    // One lifted: the other swipes on from where it is and the view the pinch left, 250 px of
    // 1000 left, a quarter of the five hours later.
    at.handlers.onPointerUp(finger(2, 900));
    at.handlers.onPointerMove(finger(1, 250, 102));
    expect(nav.given.at(-1)).toEqual(hours(3.75, 8.75));
    at.handlers.onPointerUp(finger(1, 250));
    // All lifted, a move is no one's.
    const count = nav.given.length;
    at.handlers.onPointerMove(finger(1, 600));
    expect(nav.given.length).toBe(count);
  });

  it("swiped sideways move a zoomed view, and up or down leave the page to scroll", () => {
    const nav = navigation(hours(2, 4));
    const at = navigated(nav);
    at.handlers.onPointerDown(finger(1, 700));
    // Too little to tell; then 250 px left: a quarter of the two hours later.
    at.handlers.onPointerMove(finger(1, 696));
    expect(nav.given).toEqual([]);
    at.handlers.onPointerMove(finger(1, 450, 110));
    expect(nav.given).toEqual([hours(2.5, 4.5)]);
    at.handlers.onPointerCancel(finger(1, 450));
    // Down the page: not the timeline's, however far sideways it goes after.
    at.handlers.onPointerDown(finger(3, 700));
    at.handlers.onPointerMove(finger(3, 702, 140));
    at.handlers.onPointerMove(finger(3, 400, 150));
    expect(nav.given).toEqual([hours(2.5, 4.5)]);
  });

  it("leave the whole task where it is, and do nothing where the tracks are not drawn", () => {
    const whole = navigation(null);
    const at = navigated(whole);
    at.handlers.onPointerDown(finger(1, 700));
    at.handlers.onPointerMove(finger(1, 400));
    expect(whole.given).toEqual([]);
    const zoomed = navigation(hours(2, 4));
    const blank = navigated(zoomed, 0);
    blank.handlers.onPointerDown(finger(1, 700));
    blank.handlers.onPointerMove(finger(1, 400));
    blank.handlers.onPointerDown(finger(2, 900));
    blank.handlers.onPointerMove(finger(2, 1000));
    expect(zoomed.given).toEqual([]);
  });
});

/** The approved pictures' task, its hours 09:00 to 15:20 on its clock. */
function pictures(): TaskTimeline {
  const read = readTimeline(JSON.parse(JSON.stringify(FIXTURE)));
  if (read === null) throw new Error("the fixture is no timeline");
  return read;
}
const START = 1_794_380_400;
const END = 1_794_403_200;

/** The element under `node` with `className`, by its props. */
function find(node: ReactNode, className: string): ReactElement<Record<string, unknown>> | null {
  if (node === null || typeof node !== "object") return null;
  if (Array.isArray(node)) {
    for (const child of node) {
      const found = find(child, className);
      if (found) return found;
    }
    return null;
  }
  const element = node as ReactElement<Record<string, unknown>>;
  if (element.props?.className === className) return element;
  return find(element.props?.children as ReactNode, className);
}

describe("the overview's window", () => {
  const view: View = { from: START + 3600, to: START + 7200 };

  function strip() {
    const nav = { whole: { from: START, to: END }, view, given: [] as (View | null)[] };
    const window = find(
      OverviewStrip({
        timeline: pictures(),
        view,
        nav: { ...nav, set: (next) => nav.given.push(next) },
      }),
      "tl-ov-window",
    );
    if (!window) throw new Error("no window");
    type Handler = (event: unknown) => void;
    return { nav, on: (name: string) => window.props[name] as Handler };
  }

  /** A press on the window at `x` on a strip 1000 px wide, on its part `hold` names. */
  const press = (x: number, hold?: string, more: Record<string, unknown> = {}) => ({
    pointerType: "mouse",
    button: 0,
    pointerId: 1,
    clientX: x,
    target: { dataset: hold ? { hold } : {} },
    currentTarget: {
      parentElement: { getBoundingClientRect: () => ({ width: 1000 }) },
      setPointerCapture: () => undefined,
    },
    ...more,
  });

  it("dragged moves the view by as much of the whole as of the strip", () => {
    const { nav, on } = strip();
    // 6 h 20 m on 1000 px: 100 px is 38 minutes.
    on("onPointerDown")(press(400));
    on("onPointerMove")({ clientX: 500 });
    on("onPointerUp")({});
    on("onPointerMove")({ clientX: 600 });
    expect(nav.given).toEqual([{ from: view.from + 2280, to: view.to + 2280 }]);
  });

  it("dragged by an edge resizes the view from the other", () => {
    const { nav, on } = strip();
    on("onPointerDown")(press(500, "end"));
    on("onPointerMove")({ clientX: 600 });
    on("onPointerCancel")({});
    on("onPointerDown")(press(400, "start"));
    on("onPointerMove")({ clientX: 300 });
    expect(nav.given).toEqual([
      { from: view.from, to: view.to + 2280 },
      { from: view.from - 2280, to: view.to },
    ]);
  });

  it("is dragged by no other button, and by nothing on a strip not drawn", () => {
    const { nav, on } = strip();
    on("onPointerDown")(press(400, undefined, { button: 1 }));
    on("onPointerMove")({ clientX: 500 });
    on("onPointerDown")(
      press(400, undefined, {
        currentTarget: { parentElement: { getBoundingClientRect: () => ({ width: 0 }) } },
      }),
    );
    on("onPointerMove")({ clientX: 500 });
    expect(nav.given).toEqual([]);
  });

  it("moves by its keys, zooms by + − 0, and leaves the rest to the page", () => {
    const { nav, on } = strip();
    const right = kept({ key: "ArrowRight" });
    on("onKeyDown")(right.event);
    on("onKeyDown")(kept({ key: "0" }).event);
    const other = kept({ key: "Enter" });
    on("onKeyDown")(other.event);
    expect(nav.given).toEqual([{ from: view.from + 360, to: view.to + 360 }, null]);
    expect([right.said.prevented, other.said.prevented]).toEqual([true, false]);
  });
});
