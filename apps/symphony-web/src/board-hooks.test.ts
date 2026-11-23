// The task board's hooks (board.tsx), and its toolbar, which keeps a state of its own, each called
// as React calls it while it draws, with no DOM: its states, refs and memos kept from one draw to
// the next by the order it asks for them, and after each draw every effect whose dependencies
// moved run, its last cleanup first, the layout effects before the others (`runtime`). A state set
// to a new value draws it again, as React would, until none is; a test draws it again itself once
// a promise it waits on has answered.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  BoardFilters,
  type BoardFiltersProps,
  type FlowRead,
  type LinkedView,
  type Side,
  useColumns,
  useFlow,
  useLaneSides,
  useLinkedPage,
  useLinkedTask,
  useOpenedFocus,
  usePhoneAndMinute,
} from "./board";
import { NAMES, PHONE_STAGES, STAGES } from "./boardwait";
import type { Flow } from "./flow";
import { aTask, elementOf, type Found, fire, turn, until } from "./testing";
import type { Task } from "./wire";
import { NO_WORK } from "./work";

const runtime = vi.hoisted(() => {
  type Cleanup = (() => void) | undefined;
  interface Slot {
    value?: unknown;
    deps?: unknown[] | undefined;
    cleanup?: Cleanup;
  }
  const state = {
    slots: [] as Slot[],
    at: 0,
    layout: [] as (() => void)[],
    passive: [] as (() => void)[],
    dirty: false,
  };
  /** The hook's next slot, and whether it is new. */
  const slot = (): [Slot, boolean] => {
    const at = state.at++;
    const fresh = state.slots[at] === undefined;
    state.slots[at] ??= {};
    return [state.slots[at], fresh];
  };
  const moved = (before: unknown[] | undefined, deps: unknown[] | undefined) =>
    before === undefined ||
    deps === undefined ||
    deps.length !== before.length ||
    deps.some((dep, at) => !Object.is(dep, before[at]));
  const memo = (make: () => unknown, deps?: unknown[]) => {
    const [held, fresh] = slot();
    if (fresh || moved(held.deps, deps)) {
      held.value = make();
      held.deps = deps;
    }
    return held.value;
  };
  const effect =
    (queue: "layout" | "passive") => (run: () => Cleanup | undefined, deps?: unknown[]) => {
      const [held, fresh] = slot();
      if (!fresh && !moved(held.deps, deps)) return;
      held.deps = deps;
      state[queue].push(() => {
        held.cleanup?.();
        held.cleanup = run() ?? undefined;
      });
    };
  return {
    state,
    useState: (first: unknown) => {
      const [held, fresh] = slot();
      if (fresh) held.value = typeof first === "function" ? first() : first;
      const set = (next: unknown) => {
        const value = typeof next === "function" ? next(held.value) : next;
        if (Object.is(value, held.value)) return;
        held.value = value;
        state.dirty = true;
      };
      return [held.value, set];
    },
    useRef: (current: unknown) => memo(() => ({ current }), []),
    useMemo: memo,
    useCallback: (made: unknown, deps?: unknown[]) => memo(() => made, deps),
    useEffect: effect("passive"),
    useLayoutEffect: effect("layout"),
    /** Every effect's cleanup, as the component is taken off the page. */
    unmount: () => {
      for (const held of state.slots) held?.cleanup?.();
    },
  };
});

vi.mock("react", async (original) => ({
  ...(await original<typeof import("react")>()),
  useState: runtime.useState,
  useRef: runtime.useRef,
  useMemo: runtime.useMemo,
  useCallback: runtime.useCallback,
  useEffect: runtime.useEffect,
  useLayoutEffect: runtime.useLayoutEffect,
}));

/** Each read of a linked task, held until its test answers it. */
const links = vi.hoisted(() => ({
  asked: [] as {
    scope: string;
    slug: string;
    signal: AbortSignal;
    answer: (task: Task | null) => void;
    fail: (error: unknown) => void;
  }[],
}));

vi.mock("./boardlink", async (original) => ({
  ...(await original<typeof import("./boardlink")>()),
  linkedTask: (scope: string, slug: string, signal: AbortSignal) =>
    new Promise<Task | null>((answer, fail) =>
      links.asked.push({ scope, slug, signal, answer, fail }),
    ),
}));

/** Each read of the workflow's flow, held until its test answers it; the chunk comes when `come`. */
const flows = vi.hoisted(() => ({
  come: true,
  imported: 0,
  read: [] as {
    scope: string;
    signal: AbortSignal;
    names: Map<string, string>;
    own: unknown;
    answer: (flow: Flow | null) => void;
  }[],
}));

/** From here on the flow's chunk comes, or does not (`come`): each import is the chunk's afresh,
 * as one that came once is kept and would not be asked for again. */
function flowComes(come: boolean) {
  flows.come = come;
  vi.doMock("./flow", () => {
    flows.imported += 1;
    if (!flows.come) throw new Error("no such chunk");
    return {
      readFlow: (scope: string, signal: AbortSignal, names: Map<string, string>, own: unknown) =>
        new Promise<Flow | null>((answer) =>
          flows.read.push({ scope, signal, names, own, answer }),
        ),
    };
  });
  vi.resetModules();
}

/**
 * `hook` drawn with `args`: what it gave at its last draw, a draw again (with other args, or the
 * same), the draws once what it waits on has answered, and its cleanups.
 */
function mount<Args extends unknown[], T>(hook: (...args: Args) => T, ...args: NoInfer<Args>) {
  const { state } = runtime;
  state.slots = [];
  let given = args;
  let current = undefined as T;
  const draw = (...next: Args | []) => {
    if (next.length > 0) given = next as Args;
    do {
      state.dirty = false;
      state.at = 0;
      current = hook(...given);
      const run = [...state.layout, ...state.passive];
      state.layout = [];
      state.passive = [];
      for (const effect of run) effect();
    } while (state.dirty);
    return current;
  };
  draw();
  return {
    get current() {
      return current;
    },
    draw,
    /** The hook drawn again once the promises it waits on have answered. */
    settle: async () => {
      await turn();
      return draw();
    },
    unmount: runtime.unmount,
  };
}

/** The page's window, with its listeners and the shell's word on whether the build is `gone`
 * (none: no shell). */
function aWindow(gone?: boolean) {
  const listeners = new Map<string, EventListener>();
  const page = {
    listeners,
    addEventListener: (name: string, listener: EventListener) => listeners.set(name, listener),
    removeEventListener: (name: string, listener: EventListener) => {
      if (listeners.get(name) === listener) listeners.delete(name);
    },
    ...(gone === undefined ? {} : { reviewShell: { gone: async () => gone } }),
  };
  vi.stubGlobal("window", page);
  return page;
}

beforeEach(() => {
  links.asked = [];
  flows.read = [];
  flows.imported = 0;
  flowComes(true);
  aWindow(false);
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("the task the address links", () => {
  const task = aTask("w/a");

  it("is none, and read from nowhere, while the address links no task", () => {
    const linked = mount(useLinkedTask, "", "", undefined);
    expect(linked.current.linked).toBeNull();
    expect(linked.current.error).toBe("");
    expect(links.asked).toEqual([]);
  });

  it("is the board's own when the board holds it, with nothing read", () => {
    const linked = mount(useLinkedTask, "", "w/a", task);
    expect(linked.current.linked).toBe(task);
    expect(links.asked).toEqual([]);
  });

  it("is read from the server under the project when the board does not hold it", async () => {
    const linked = mount(useLinkedTask, "/p", "w/a", undefined);
    expect(linked.current.linked).toBeNull();
    expect(links.asked.map(({ scope, slug }) => [scope, slug])).toEqual([["/p", "w/a"]]);
    links.asked[0]?.answer(task);
    expect((await linked.settle()).linked).toBe(task);
    expect(linked.current.error).toBe("");
  });

  it("says a task the server no longer has is gone from the board", async () => {
    const linked = mount(useLinkedTask, "", "w/a", undefined);
    links.asked[0]?.answer(null);
    expect((await linked.settle()).error).toBe("The linked task is no longer on this board.");
    expect(linked.current.linked).toBeNull();
  });

  it("says why a read failed, in its own words, and clears it while a retry reads again", async () => {
    const linked = mount(useLinkedTask, "", "w/a", undefined);
    links.asked[0]?.fail(new Error("The server did not answer."));
    expect((await linked.settle()).error).toBe("The server did not answer.");
    linked.current.retry();
    expect(linked.draw().error).toBe("");
    expect(links.asked.map(({ slug }) => slug)).toEqual(["w/a", "w/a"]);
    links.asked[1]?.answer(task);
    expect((await linked.settle()).linked).toBe(task);
  });

  it("says no error once the board holds the task, nor reads it again", async () => {
    const linked = mount(useLinkedTask, "", "w/a", undefined);
    links.asked[0]?.fail("lost");
    expect((await linked.settle()).error).toBe("lost");
    expect(linked.draw("", "w/a", task)).toMatchObject({ linked: task, error: "" });
    expect(links.asked).toHaveLength(1);
  });

  it("gives up the read the address left, and never links its task to another address", async () => {
    const linked = mount(useLinkedTask, "", "w/a", undefined);
    const [left] = links.asked;
    linked.draw("", "w/b", undefined);
    expect(left?.signal.aborted).toBe(true);
    left?.answer(task);
    expect((await linked.settle()).linked).toBeNull();
    // A task read for one address is not the link of the next.
    links.asked[1]?.answer(aTask("w/b"));
    expect((await linked.settle()).linked?.slug).toBe("w/b");
    expect(linked.draw("", "w/c", undefined).linked).toBeNull();
  });

  it("gives up the read when the board's own copy comes first, and says nothing of its failure", async () => {
    const linked = mount(useLinkedTask, "", "w/a", undefined);
    const [read] = links.asked;
    linked.draw("", "w/a", task);
    expect(read?.signal.aborted).toBe(true);
    read?.fail(new Error("aborted"));
    expect(await linked.settle()).toMatchObject({ linked: task, error: "" });
  });
});

describe("a custom workflow's flow", () => {
  const flow = { lanes: () => [["p1"], ["p1"]] } as unknown as Flow;
  /** `read` drawn again until `check` holds of what it gives. */
  const drawnUntil = (read: { draw: () => FlowRead }, check: (now: FlowRead) => void) =>
    until(() => check(read.draw()));
  /** Until the flow was asked `count` reads. */
  const reads = (count: number) => until(() => expect(flows.read).toHaveLength(count));
  /** A shell whose `gone` answers when the test says, counting the times it is asked. */
  function aSlowShell() {
    const shell = { asked: 0, answer: (_gone: boolean) => {} };
    const answered = new Promise<boolean>((answer) => {
      shell.answer = answer;
    });
    const gone = () => {
      shell.asked += 1;
      return answered;
    };
    vi.stubGlobal("window", { reviewShell: { gone } });
    return shell;
  }

  it("is none, its names forgotten and its chunk never imported, while no workflow is active", async () => {
    NAMES.set("p1", "Phase one");
    for (const none of [undefined, null, 0]) {
      const read = mount(useFlow, "", none);
      expect(await read.settle()).toMatchObject({ flow: null, failed: false });
    }
    expect(NAMES.size).toBe(0);
    expect(flows.imported).toBe(0);
  });

  it("is read from its chunk once a workflow is active, with the board's own lanes and its names", async () => {
    const read = mount(useFlow, "/p", 3);
    await reads(1);
    expect(flows.read.map(({ scope, names, own }) => [scope, names, own])).toEqual([
      ["/p", NAMES, [STAGES, PHONE_STAGES]],
    ]);
    flows.read[0]?.answer(flow);
    expect((await read.settle()).flow).toBe(flow);
  });

  it("is read again when its generation moves, and forgotten when none is active", async () => {
    const read = mount(useFlow, "", 3);
    await reads(1);
    flows.read[0]?.answer(flow);
    await read.settle();
    read.draw("", 4);
    expect(flows.read[0]?.signal.aborted).toBe(true);
    await reads(2);
    NAMES.set("p1", "Phase one");
    expect(read.draw("", undefined).flow).toBeNull();
    expect(NAMES.size).toBe(0);
    expect(flows.read[1]?.signal.aborted).toBe(true);
  });

  it("drops a read its generation left", async () => {
    const read = mount(useFlow, "", 3);
    await reads(1);
    read.draw("", undefined);
    flows.read[0]?.answer(flow);
    expect((await read.settle()).flow).toBeNull();
  });

  it("fails when its chunk does not come and the page's build is not gone, until a retry imports it", async () => {
    flowComes(false);
    const read = mount(useFlow, "", 3);
    await drawnUntil(read, (now) => expect(now.failed).toBe(true));
    flowComes(true);
    read.current.retry();
    expect(read.draw().failed).toBe(false);
    await reads(1);
    flows.read[0]?.answer(flow);
    expect((await read.settle()).flow).toBe(flow);
  });

  it("fails as well when no shell says whether the build is gone", async () => {
    aWindow();
    flowComes(false);
    await drawnUntil(mount(useFlow, "", 3), (now) => expect(now.failed).toBe(true));
  });

  it("does not fail when the build is gone, as the page loads again", async () => {
    const shell = aSlowShell();
    flowComes(false);
    const read = mount(useFlow, "", 3);
    await until(() => expect(shell.asked).toBe(1));
    shell.answer(true);
    expect((await read.settle()).failed).toBe(false);
  });

  it("fails for its generation alone: a new one, or one after none, imports the chunk again", async () => {
    flowComes(false);
    const read = mount(useFlow, "", 3);
    await drawnUntil(read, (now) => expect(now.failed).toBe(true));
    flowComes(true);
    expect(read.draw("", 4).failed).toBe(false);
    await reads(1);
    flowComes(false);
    read.draw("", 5);
    await drawnUntil(read, (now) => expect(now.failed).toBe(true));
    flowComes(true);
    expect(read.draw("", undefined).failed).toBe(false);
    expect(read.draw("", 6).failed).toBe(false);
    await reads(2);
  });

  it("keeps no failure its generation left while the shell answered", async () => {
    const shell = aSlowShell();
    flowComes(false);
    const read = mount(useFlow, "", 3);
    await until(() => expect(shell.asked).toBe(1));
    read.draw("", undefined);
    shell.answer(false);
    await read.settle();
    // Back at that generation, the chunk is imported again.
    flowComes(true);
    expect(read.draw("", 3).failed).toBe(false);
    await reads(1);
  });
});

describe("the phone and the minute", () => {
  it("is a phone at 760 px wide or less, read again at each resize", () => {
    vi.stubGlobal("innerWidth", 760.4);
    const page = aWindow();
    const shown = mount(usePhoneAndMinute);
    expect(shown.current[0]).toBe(true);
    vi.stubGlobal("innerWidth", 761);
    page.listeners.get("resize")?.(new Event("resize"));
    expect(shown.draw()[0]).toBe(false);
    vi.stubGlobal("innerWidth", 390);
    page.listeners.get("resize")?.(new Event("resize"));
    expect(shown.draw()[0]).toBe(true);
    shown.unmount();
  });

  it("moves the time at each minute, and no more once it is off the page", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-11-22T10:00:30Z"));
    vi.stubGlobal("innerWidth", 1440);
    const page = aWindow();
    const shown = mount(usePhoneAndMinute);
    const start = Date.parse("2026-11-22T10:00:30Z") / 1000;
    expect(shown.current).toEqual([false, start]);
    await vi.advanceTimersByTimeAsync(29_000);
    expect(shown.draw()[1]).toBe(start);
    await vi.advanceTimersByTimeAsync(1_000);
    expect(shown.draw()[1]).toBe(start + 30);
    await vi.advanceTimersByTimeAsync(60_000);
    expect(shown.draw()[1]).toBe(start + 90);
    shown.unmount();
    expect(page.listeners.has("resize")).toBe(false);
    await vi.advanceTimersByTimeAsync(60_000);
    expect(shown.draw()[1]).toBe(start + 90);
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe("the lanes' sides and place", () => {
  /** The lane scroller: 300 px wide from 100, its `count` lanes `width` px wide side by side from
   * `first`, scrolled to `scrollLeft`, with the observer of its width. */
  function aScroller(count: number, first: number, width = 200) {
    const box = (left: number, wide: number) => ({ left, right: left + wide }) as DOMRect;
    const lanes = Array.from({ length: count }, (_, at) => ({
      getBoundingClientRect: () => box(first + at * width - scroller.scrollLeft, width),
    }));
    const scroller = {
      scrollLeft: 0,
      observed: [] as unknown[],
      disconnected: 0,
      resized: () => {},
      getBoundingClientRect: () => box(100, 300),
      querySelectorAll: (selector: string) => (selector === ".column" ? lanes : []),
    };
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(resized: () => void) {
          scroller.resized = resized;
        }
        observe(target: unknown) {
          scroller.observed.push(target);
        }
        disconnect() {
          scroller.disconnected += 1;
        }
      },
    );
    return scroller;
  }
  const surfaceOf = (scroller: unknown) => ({ current: scroller as HTMLElement | null });
  const sides = (lanes: { side: (at: number) => Side }, count: number) =>
    Array.from({ length: count }, (_, at) => lanes.side(at));

  it("says which lanes the scroller cuts, a pixel's rounding no cut, once it shows them", () => {
    // In the scroller's 100 to 400: lanes from 99.5 to 250, to 400.5 and to 551.
    const scroller = aScroller(3, 99.5, 150.5);
    const lanes = mount(useLaneSides, surfaceOf(scroller), true);
    expect(sides(lanes.current, 3)).toEqual(["shown", "shown", "after"]);
    expect(scroller.observed).toEqual([scroller]);
    scroller.scrollLeft = 250;
    lanes.current.scrolled();
    expect(sides(lanes.draw(), 3)).toEqual(["before", "before", "shown"]);
  });

  it("reads them again when the scroller's width changes", () => {
    const scroller = aScroller(2, 100);
    const lanes = mount(useLaneSides, surfaceOf(scroller), true);
    expect(sides(lanes.current, 2)).toEqual(["shown", "after"]);
    scroller.getBoundingClientRect = () => ({ left: 100, right: 500 }) as DOMRect;
    scroller.resized();
    expect(sides(lanes.draw(), 2)).toEqual(["shown", "shown"]);
  });

  it("takes an unmeasured lane for a whole one", () => {
    const lanes = mount(useLaneSides, surfaceOf(null), true);
    expect(sides(lanes.current, 2)).toEqual(["shown", "shown"]);
    lanes.current.scrolled();
    lanes.current.remember();
    expect(sides(lanes.draw(), 2)).toEqual(["shown", "shown"]);
  });

  it("reads nothing, nor watches the width, while the scroller shows no lanes", () => {
    const scroller = aScroller(3, 300);
    const lanes = mount(useLaneSides, surfaceOf(scroller), false);
    lanes.current.scrolled();
    expect(sides(lanes.draw(), 3)).toEqual(["shown", "shown", "shown"]);
    expect(scroller.observed).toEqual([]);
  });

  it("stops watching the width once the lanes are hidden, or off the page", () => {
    const scroller = aScroller(3, 100);
    const surface = surfaceOf(scroller);
    const lanes = mount(useLaneSides, surface, true);
    lanes.draw(surface, false);
    expect(scroller.disconnected).toBe(1);
    lanes.draw(surface, true);
    expect(scroller.observed).toHaveLength(2);
    lanes.unmount();
    expect(scroller.disconnected).toBe(2);
  });

  it("stands where it stood among the lanes once it shows them again", () => {
    const scroller = aScroller(4, 100);
    const surface = surfaceOf(scroller);
    const lanes = mount(useLaneSides, surface, true);
    scroller.scrollLeft = 420;
    lanes.current.remember();
    // Another stage's list, scrolled elsewhere, then the lanes again.
    lanes.draw(surface, false);
    scroller.scrollLeft = 0;
    lanes.current.remember();
    lanes.current.scrolled();
    lanes.draw(surface, true);
    expect(scroller.scrollLeft).toBe(420);
    // A draw that still shows them moves nothing.
    scroller.scrollLeft = 30;
    lanes.draw(surface, true);
    expect(scroller.scrollLeft).toBe(30);
  });

  it("keeps the place a scroll leaves, and moves nothing at the first draw", () => {
    const scroller = aScroller(4, 100);
    scroller.scrollLeft = 75;
    const surface = surfaceOf(scroller);
    const lanes = mount(useLaneSides, surface, true);
    expect(scroller.scrollLeft).toBe(75);
    scroller.scrollLeft = 160;
    lanes.current.scrolled();
    lanes.draw(surface, false);
    scroller.scrollLeft = 0;
    lanes.draw(surface, true);
    expect(scroller.scrollLeft).toBe(160);
  });

  it("keeps the same remember while the lanes stay shown, as a column's memo needs", () => {
    const surface = surfaceOf(aScroller(1, 100));
    const lanes = mount(useLaneSides, surface, true);
    const { remember } = lanes.current;
    expect(lanes.draw(surface, true).remember).toBe(remember);
    expect(lanes.draw(surface, false).remember).not.toBe(remember);
  });
});

describe("the board's columns", () => {
  const ready = aTask("w/a");
  const review = aTask("w/b", { status: "review" });
  const slice = aTask("w/c", { parent: "w/a" });
  const slugs = (columns: Map<string, Task[]>) =>
    Object.fromEntries([...columns].map(([status, tasks]) => [status, tasks.map((t) => t.slug)]));

  it("puts each task shown in the stage it waits in, or all in one under a wait filter", () => {
    const columns = mount(useColumns, [review, ready, slice], "", "", NO_WORK, true);
    expect(slugs(columns.current)).toEqual({ review: ["w/b"], ready: ["w/a"] });
    const on = aTask("w/d", { waits_on: { kind: "check", who: "", why: "", since: 0 } });
    expect(slugs(columns.draw([review, on], "", "check", NO_WORK, true))).toEqual({
      waiting: ["w/d"],
    });
  });

  it("keeps a column's list while its tasks are the same, and the columns while nothing moved", () => {
    const given = [ready, review];
    const columns = mount(useColumns, given, "", "", NO_WORK, true);
    const before = columns.current;
    expect(columns.draw(given, "", "", NO_WORK, true)).toBe(before);
    const moved = columns.draw(
      [ready, aTask("w/b", { status: "review", title: "Moved" })],
      "",
      "",
      NO_WORK,
      true,
    );
    expect(moved).not.toBe(before);
    expect(moved.get("ready")).toBe(before.get("ready"));
    expect(moved.get("review")).not.toBe(before.get("review"));
  });
});

describe("the page of a linked task's stage", () => {
  /** The board's view: a desk's lanes, the address linking `w/z`, which stands in Ready. */
  const view = (fields: Partial<LinkedView> = {}): LinkedView => ({
    address: "row-w/z",
    destination: "ready",
    held: false,
    phone: false,
    opened: "",
    tab: "review",
    wait: "",
    search: "",
    days: 1,
    ...fields,
  });
  /** The board, `rows` tasks in Ready of the `total` it counts. */
  const board = (rows: number, total = 3, onLoad = vi.fn()) => ({
    tasks: Array.from({ length: rows }, (_, at) => aTask(`w/${at}`)),
    counts: [["ready", total]] as [string, number][],
    work: NO_WORK,
    connected: true,
    onLoad,
  });

  it("reads the stage's next page while the board does not hold the task, once for its rows", () => {
    const onLoad = vi.fn();
    const page = mount(useLinkedPage, view({ days: 7 }), board(1, 3, onLoad));
    expect(onLoad.mock.calls).toEqual([["ready", true, "", 7]]);
    page.draw(view({ days: 7 }), board(1, 3, onLoad));
    expect(onLoad).toHaveBeenCalledTimes(1);
    page.draw(view({ days: 7 }), board(2, 3, onLoad));
    expect(onLoad).toHaveBeenCalledTimes(2);
    // Another address asks again for the same rows.
    page.draw(view({ days: 7, address: "row-w/y" }), board(2, 3, onLoad));
    expect(onLoad).toHaveBeenCalledTimes(3);
  });

  it.each([
    ["the board holds the task", view({ held: true }), 1, 3],
    ["the task is not found yet", view({ destination: "" }), 1, 3],
    ["a wait filter's column is shown", view({ wait: "check" }), 1, 3],
    ["a search's column is shown", view({ search: "lamp" }), 1, 3],
    ["the stage has no row yet", view(), 0, 3],
    ["the stage has every row", view(), 3, 3],
    ["a desk shows another stage", view({ opened: "backlog" }), 1, 3],
    ["a phone shows another tab", view({ phone: true }), 1, 3],
    ["a phone opened another stage", view({ phone: true, tab: "ready", opened: "build" }), 1, 3],
  ] as [string, LinkedView, number, number][])("reads nothing when %s", (_, given, rows, total) => {
    const onLoad = vi.fn();
    mount(useLinkedPage, given, board(rows, total, onLoad));
    expect(onLoad).not.toHaveBeenCalled();
  });

  it.each([
    ["a desk opened it", view({ opened: "ready" })],
    ["a phone's tab is it", view({ phone: true, tab: "ready" })],
    ["a phone opened it", view({ phone: true, opened: "ready" })],
  ] as [string, LinkedView][])("reads it when %s", (_, given) => {
    const onLoad = vi.fn();
    mount(useLinkedPage, given, board(1, 3, onLoad));
    expect(onLoad).toHaveBeenCalledWith("ready", true, "", 1);
  });
});

describe("the focus once the stage shown changed", () => {
  /** The page: what holds the focus, and the control each selector finds, which says it took it. */
  function aPage(focusHeld = false) {
    const body = {};
    const focused: string[] = [];
    vi.stubGlobal("document", {
      body,
      activeElement: focusHeld ? {} : body,
      querySelector: (selector: string) =>
        selector === "none" ? null : { focus: () => focused.push(selector) },
    });
    return focused;
  }

  it("goes on a desk to Back to stages, and back to the control that opened the stage", () => {
    const focused = aPage();
    const focus = mount(useOpenedFocus, "", false);
    expect(focused).toEqual([]);
    focus.draw("backlog", false);
    focus.draw("cancelled", false);
    focus.draw("", false);
    expect(focused).toEqual([
      ".back-to-stages",
      ".back-to-stages",
      '[data-open-stage="cancelled"]',
    ]);
  });

  it("goes on a phone to the current tab", () => {
    const focused = aPage();
    const focus = mount(useOpenedFocus, "", true);
    focus.draw("build", true);
    focus.draw("", true);
    expect(focused).toEqual([
      '.stage-tabs button[aria-current="page"]',
      '.stage-tabs button[aria-current="page"]',
    ]);
  });

  it("moves nothing while the focus is held, or while the stage shown stays", () => {
    const held = aPage(true);
    mount(useOpenedFocus, "", false).draw("backlog", false);
    expect(held).toEqual([]);
    const focused = aPage();
    const focus = mount(useOpenedFocus, "backlog", false);
    focus.draw("backlog", true);
    expect(focused).toEqual([]);
  });
});

describe("the board's toolbar and filters", () => {
  const props = (fields: Partial<BoardFiltersProps> = {}): BoardFiltersProps => ({
    filters: { search: "", days: 1 },
    onFilters: vi.fn(),
    filter: vi.fn(),
    backlog: 4,
    backlogOpen: false,
    onBacklog: vi.fn(),
    ...fields,
  });
  const byClass = (name: string) => (element: Found) => element.props.className === name;
  const prop = (drawn: unknown, name: string, key: string) =>
    elementOf(drawn as Found, byClass(name))?.props[key];

  it("opens and closes its filters", () => {
    const filters = mount(BoardFilters, props());
    expect(prop(filters.current, "board-filters-toggle", "aria-expanded")).toBe(false);
    expect(prop(filters.current, "row filters", "data-open")).toBe(false);
    fire(filters.current, byClass("board-filters-toggle"), "onClick");
    expect(prop(filters.draw(), "row filters", "data-open")).toBe(true);
    expect(prop(filters.current, "board-filters-toggle", "aria-expanded")).toBe(true);
    fire(filters.current, byClass("board-filters-toggle"), "onClick");
    expect(prop(filters.draw(), "row filters", "data-open")).toBe(false);
  });

  it("reads the board under each key typed, in the window chosen", () => {
    const given = props({ filters: { search: "", days: 7 } });
    fire(mount(BoardFilters, given).current, byClass("dx-input"), "onInput", {
      currentTarget: { value: "lamp" },
    });
    expect(given.filter).toHaveBeenCalledWith("lamp", 7);
    expect(given.onFilters).toHaveBeenCalledWith({ search: "lamp", days: 7 });
  });

  it("reads the board in the window chosen, under the search typed", () => {
    const given = props({ filters: { search: "lamp", days: 1 } });
    const filters = mount(BoardFilters, given).current;
    expect(
      elementOf(filters, (element) => element.props.label === "Finished tasks")?.props.value,
    ).toBe("1");
    fire(filters, (element) => element.props.label === "Finished tasks", "onChange", "7");
    expect(given.filter).toHaveBeenCalledWith("lamp", 7);
    expect(given.onFilters).toHaveBeenCalledWith({ search: "lamp", days: 7 });
  });

  it("has the desk's Backlog with its count, opened or not, and none where it is none", () => {
    const given = props({ backlogOpen: true });
    const filters = mount(BoardFilters, given).current;
    expect(prop(filters, "board-backlog", "children")).toEqual(["Backlog ", 4]);
    expect(prop(filters, "board-backlog", "aria-expanded")).toBe(true);
    fire(filters, byClass("board-backlog"), "onClick");
    expect(given.onBacklog).toHaveBeenCalledTimes(1);
    expect(
      prop(mount(BoardFilters, props({ backlog: null })).current, "board-backlog", "children"),
    ).toBeUndefined();
  });
});
