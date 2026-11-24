// What a task page's controls do when they are used: the owner's priority and cancel, the
// composer's message and landing, a held message's pass, the brief's next part, and the page's own
// wiring. Each component
// is called as React would draw it, its hooks answered here (`runtime`): the states a test gives,
// a spy for each setter, its refs kept from one draw to the next as one mounted component's are,
// and its effects held for the test to run. The handlers it draws are then used as a click would.
/* biome-ignore-all lint/correctness/useHookAtTopLevel: the hooks mocked here answer for a component
 * called outside a render, and call React's own only while React renders. */

import { isValidElement, type ReactElement, type ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, type Mock, vi } from "vitest";
import type { Attach } from "./attach";
import { FRESH } from "./cache";
import { STALE } from "./shared";
import { load, save } from "./storage";
import type { LiveTask } from "./task-live";
import {
  Brief,
  Composer,
  HeldMessage,
  LiveTaskPage,
  NOTHING_READ,
  OwnerControls,
  TaskPage,
  TaskPageView,
} from "./task-page";
import { aRole, memoryStorage, reply, server, turn, until } from "./testing";
import type { HeldWords, TaskEntry, TaskOffer } from "./wire";
import { TASK_ACTION_ROUTE, TASK_PASS_ROUTE } from "./wire";
import { CHECKING } from "./words";

/** The hooks of the component drawn while `on`; React's own otherwise. */
const runtime = vi.hoisted(() => ({
  on: false,
  /** The values the next `useState` calls give, in order; past them, each gives its initial. */
  states: [] as unknown[],
  setters: [] as Mock[],
  /** The component's refs, by the order it asks for them, kept across its draws. */
  refs: [] as { current: unknown }[],
  ref: 0,
  effects: [] as (() => undefined | (() => void))[],
  reducers: [] as Mock[],
  signal: new AbortController().signal,
  frame: { refresh: () => {} } as unknown,
}));

vi.mock("react", async (original) => {
  const actual = await original<typeof import("react")>();
  return {
    ...actual,
    useState: (initial: unknown) => {
      if (!runtime.on) return actual.useState(initial);
      const given = runtime.states.length > 0;
      const value = given
        ? runtime.states.shift()
        : typeof initial === "function"
          ? (initial as () => unknown)()
          : initial;
      const set = vi.fn();
      runtime.setters.push(set);
      return [value, set];
    },
    useRef: (initial: unknown) => {
      if (!runtime.on) return actual.useRef(initial);
      const at = runtime.ref++;
      runtime.refs[at] ??= { current: initial };
      return runtime.refs[at];
    },
    useEffect: (effect: () => undefined | (() => void), deps?: unknown[]) => {
      if (!runtime.on) return actual.useEffect(effect, deps);
      runtime.effects.push(effect);
    },
    useMemo: (make: () => unknown, deps: unknown[]) =>
      runtime.on ? make() : actual.useMemo(make, deps),
    useReducer: (reducer: (state: number) => number, initial: number) => {
      if (!runtime.on) return actual.useReducer(reducer, initial);
      const dispatch = vi.fn();
      runtime.reducers.push(dispatch);
      return [initial, dispatch];
    },
    useSyncExternalStore: <T,>(
      subscribe: (listener: () => void) => () => void,
      get: () => T,
      server?: () => T,
    ) => (runtime.on ? get() : actual.useSyncExternalStore(subscribe, get, server)),
  };
});

vi.mock("./mount", async (original) => {
  const actual = await original<typeof import("./mount")>();
  return { ...actual, useMount: () => (runtime.on ? runtime.signal : actual.useMount()) };
});

vi.mock("./frame", async (original) => {
  const actual = await original<typeof import("./frame")>();
  return { ...actual, useFrame: () => (runtime.on ? runtime.frame : actual.useFrame()) };
});

/** The composer's attachments: none, until a test gives some. */
const attach = vi.hoisted(() => ({
  value: null as unknown,
}));

vi.mock("./attach", async (original) => {
  const actual = await original<typeof import("./attach")>();
  return { ...actual, useAttach: () => attach.value };
});

/** The live tasks the page made, each a spy of what the page asked of it. */
const made = vi.hoisted(() => ({
  tasks: [] as {
    scope: string;
    slug: string;
    host: Record<string, unknown>;
    signal: AbortSignal;
    start: Mock;
    moved: Mock;
  }[],
}));

vi.mock("./task-live", async (original) => {
  const actual = await original<typeof import("./task-live")>();
  class Recorded {
    start = vi.fn(async () => {});
    moved = vi.fn(async () => {});
    constructor(scope: string, slug: string, host: Record<string, unknown>, signal: AbortSignal) {
      made.tasks.push({ scope, slug, host, signal, start: this.start, moved: this.moved });
    }
  }
  return { ...actual, LiveTask: Recorded };
});

beforeEach(() => {
  // A page of the plain project, its route a task's.
  vi.stubGlobal("document", { getElementById: () => null });
  vi.stubGlobal("location", { pathname: "/task/w/a" });
  vi.stubGlobal("localStorage", memoryStorage());
  runtime.refs = [];
  runtime.signal = new AbortController().signal;
  made.tasks = [];
});

afterEach(() => {
  vi.unstubAllGlobals();
  STALE.set(false);
  runtime.on = false;
});

/** `component` drawn with `props`, its states the ones given: what it drew, its setters in the
 * order it asked for its states, and its effects. Its refs stay from the draw before. */
function draw<P>(component: (props: P) => ReactNode, props: P, states: unknown[] = []) {
  runtime.on = true;
  runtime.ref = 0;
  runtime.states = [...states];
  runtime.setters = [];
  runtime.effects = [];
  runtime.reducers = [];
  try {
    const node = component(props);
    return {
      node,
      set: [...runtime.setters],
      effects: [...runtime.effects],
      reducers: [...runtime.reducers],
    };
  } finally {
    runtime.on = false;
  }
}

/** Every element `node` writes itself (a component in it is not drawn) that `test` takes. */
function all(
  node: ReactNode,
  test: (element: ReactElement<Record<string, unknown>>) => boolean,
): ReactElement<Record<string, unknown>>[] {
  if (Array.isArray(node)) return node.flatMap((each) => all(each, test));
  if (!isValidElement<Record<string, unknown>>(node)) return [];
  const below = all(node.props.children as ReactNode, test);
  return test(node) ? [node, ...below] : below;
}

/** The click of the one button in `node` that says `words`. */
function button(node: ReactNode, words: string): () => void {
  const found = all(node, (each) => each.props.children === words && "onClick" in each.props);
  expect(found, words).toHaveLength(1);
  return found[0]?.props.onClick as () => void;
}

/** The words of every button in `node`. */
const buttons = (node: ReactNode) =>
  all(node, (each) => "onClick" in each.props && typeof each.props.children === "string").map(
    (each) => each.props.children,
  );

/** The props of the one element in `node` with the id `id`. */
function byId(node: ReactNode, id: string): Record<string, unknown> {
  const found = all(node, (each) => each.props.id === id);
  expect(found, id).toHaveLength(1);
  return found[0]?.props ?? {};
}

/** Until `response`'s body is read, then the turn after it: what awaited it has gone on. */
async function read(response: Response): Promise<void> {
  await until(() => expect(response.bodyUsed).toBe(true));
  await turn();
}

const calls = (spy: Mock | undefined) => spy?.mock.calls ?? [];

describe("the owner's controls used", () => {
  /** The controls of `w/a`, at priority 2 and in progress unless `fields` says otherwise. */
  const controls = (states: unknown[] = [], fields: { priority?: number; status?: string } = {}) =>
    draw(OwnerControls, { slug: "w/a", priority: 2, status: "in_progress", ...fields }, states);

  /** The setters, by their state's name. */
  const named = (set: Mock[]) => {
    const [selected, confirming, cancelled, busy, result] = set;
    return { selected, confirming, cancelled, busy, result };
  };

  it("saves a priority chosen, saying so", async () => {
    const asked = server();
    const drawn = controls();
    const set = named(drawn.set);
    (byId(drawn.node, "task-priority").onChange as (value: string) => void)("0");
    expect(asked.map((one) => [one.url, one.body])).toEqual([
      ["/api/task-priority", { slug: "w/a", priority: "0" }],
    ]);
    expect([calls(set.selected), calls(set.busy), calls(set.result)]).toEqual([
      [["0"]],
      [[true]],
      [[""]],
    ]);
    const answer = reply(200, {});
    asked[0]?.answer(answer);
    await read(answer);
    expect(calls(set.busy)).toEqual([[true], [false]]);
    expect(calls(set.result)).toEqual([[""], ["Priority saved"]]);
    expect(calls(set.selected)).toEqual([["0"]]);
  });

  it("goes back to the task's priority as it stands when the save fails", async () => {
    const asked = server();
    const drawn = controls();
    const set = named(drawn.set);
    (byId(drawn.node, "task-priority").onChange as (value: string) => void)("0");
    // The board moved the task to 3 while the save was out: the control shows that, not the 2.
    controls([], { priority: 3 });
    const answer = reply(409, { error: "The task moved" });
    asked[0]?.answer(answer);
    await read(answer);
    expect(calls(set.selected)).toEqual([["0"], ["3"]]);
    expect(calls(set.result)).toEqual([[""], ["The task moved"]]);
    expect(calls(set.busy)).toEqual([[true], [false]]);
  });

  it("asks one save at a time, and none while the page may be out of date", () => {
    const asked = server();
    const drawn = controls();
    const choose = byId(drawn.node, "task-priority").onChange as (value: string) => void;
    choose("0");
    choose("1");
    expect(asked).toHaveLength(1);
    runtime.refs = [];
    STALE.set(true);
    const stale = controls();
    (byId(stale.node, "task-priority").onChange as (value: string) => void)("1");
    expect(asked).toHaveLength(1);
    expect(stale.set.every((setter) => setter.mock.calls.length === 0)).toBe(true);
    expect(byId(stale.node, "task-priority").disabled).toBe(true);
    expect(buttons(stale.node)).toEqual([CHECKING]);
  });

  it("does nothing more once its page is gone", async () => {
    for (const status of [200, 500]) {
      const asked = server();
      const mount = new AbortController();
      runtime.signal = mount.signal;
      runtime.refs = [];
      const drawn = controls();
      const set = named(drawn.set);
      (byId(drawn.node, "task-priority").onChange as (value: string) => void)("0");
      mount.abort();
      const answer = reply(status, {});
      asked[0]?.answer(answer);
      await read(answer);
      expect([calls(set.busy), calls(set.result), calls(set.selected)], `${status}`).toEqual([
        [[true]],
        [[""]],
        [["0"]],
      ]);
    }
  });

  it("cancels only once confirmed, and is done with it", async () => {
    const asked = server();
    const first = controls();
    button(first.node, "Cancel task")();
    expect(calls(named(first.set).confirming)).toEqual([[true]]);
    expect(asked).toEqual([]);
    const confirming = controls(["2", true]);
    expect(buttons(confirming.node)).toEqual(["Confirm cancellation", "Keep task"]);
    const set = named(confirming.set);
    button(confirming.node, "Confirm cancellation")();
    expect(asked.map((one) => [one.url, one.body])).toEqual([
      ["/api/task-cancel", { slug: "w/a" }],
    ]);
    const answer = reply(200, {});
    asked[0]?.answer(answer);
    await read(answer);
    expect([calls(set.cancelled), calls(set.confirming), calls(set.result)]).toEqual([
      [[true]],
      [[false]],
      [[""], ["Task cancelled"]],
    ]);
    // Cancelled, the task offers no cancel again, and its confirmation closes.
    const done = controls(["2", true, true]);
    expect(buttons(done.node)).toEqual([]);
    done.effects[1]?.();
    expect(calls(named(done.set).confirming)).toEqual([[false]]);
  });

  it("keeps the task when the cancel is kept back or fails", async () => {
    const asked = server();
    const confirming = controls(["2", true]);
    button(confirming.node, "Keep task")();
    expect(calls(named(confirming.set).confirming)).toEqual([[false]]);
    button(confirming.node, "Confirm cancellation")();
    const answer = reply(409, { error: "The task is done" });
    asked[0]?.answer(answer);
    await read(answer);
    const set = named(confirming.set);
    expect(calls(set.cancelled)).toEqual([]);
    expect(calls(set.result)).toEqual([[""], ["The task is done"]]);
    expect(calls(set.busy)).toEqual([[true], [false]]);
  });

  it("asks no cancel while busy or out of date, and drops its answer once its page is gone", async () => {
    const asked = server();
    const confirming = controls(["2", true]);
    const confirm = button(confirming.node, "Confirm cancellation");
    confirm();
    confirm();
    expect(asked).toHaveLength(1);
    runtime.refs = [];
    STALE.set(true);
    const stale = controls(["2", true]);
    button(stale.node, CHECKING)();
    expect(asked).toHaveLength(1);
    STALE.set(false);
    for (const status of [200, 500]) {
      const mount = new AbortController();
      runtime.signal = mount.signal;
      runtime.refs = [];
      const gone = controls(["2", true]);
      const set = named(gone.set);
      button(gone.node, "Confirm cancellation")();
      mount.abort();
      const answer = reply(status, {});
      asked.at(-1)?.answer(answer);
      await read(answer);
      expect([calls(set.cancelled), calls(set.result)], `${status}`).toEqual([[], [[""]]]);
    }
  });

  it("follows the task's priority and its end", () => {
    const drawn = controls([], { priority: 4, status: "done" });
    drawn.effects[0]?.();
    drawn.effects[1]?.();
    const set = named(drawn.set);
    expect([calls(set.selected), calls(set.confirming)]).toEqual([[["4"]], [[false]]]);
    expect(buttons(drawn.node)).toEqual([]);
    const open = controls();
    open.effects[1]?.();
    expect(calls(named(open.set).confirming)).toEqual([]);
    expect(buttons(controls([], { status: "cancelled" }).node)).toEqual([]);
  });
});

describe("the composer used", () => {
  const offer: TaskOffer = {
    action: "approve",
    label: "Approve",
    branch: "w/a",
    payload: "abc123",
  };

  /** The attachments a composer is given: none unless `fields` says so, each call a spy. */
  const files = (fields: Partial<Attach> = {}): Attach => ({
    zone: "task-draft:w/a",
    files: [],
    pending: [],
    trouble: null,
    busy: false,
    empty: true,
    compose: vi.fn((text: string) => text),
    clear: vi.fn(),
    remove: vi.fn(),
    ...fields,
  });

  /** The composer of `w/a`, its states the ones given, with `with` as its attachments. */
  const composer = (
    states: unknown[] = [],
    given: Attach = files(),
    approval: TaskOffer | null = null,
  ) => {
    attach.value = given;
    const drawn = draw(
      Composer,
      { slug: "w/a", roles: [aRole("executor", "amber")], approval },
      states,
    );
    const [draft, recipient, busy, result] = drawn.set;
    return { ...drawn, draft, recipient, busy, result };
  };

  const submit = (node: ReactNode) => {
    const event = { preventDefault: vi.fn() };
    (byId(node, "task-send").onSubmit as (event: unknown) => void)(event);
    return event;
  };

  it("starts from the draft and the recipient kept on this device", () => {
    save("task-draft:w/a", "half a thought");
    save("task-to:w/a", "@reviewer");
    const drawn = composer();
    expect(byId(drawn.node, "task-message").value).toBe("half a thought");
    expect(byId(drawn.node, "task-recipient").value).toBe("@reviewer");
    expect(byId(drawn.node, "task-recipient").options).toEqual([
      ["", "everyone on it"],
      ["@executor", "@executor"],
      ["@reviewer", "@reviewer"],
      ["@orchestrator", "@orchestrator"],
      ["amber", "amber"],
    ]);
    const fresh = composer([], files());
    // Its effect keeps the draft as it is drawn.
    fresh.effects[0]?.();
    expect(load("task-draft:w/a")).toBe("half a thought");
  });

  it("sends the draft with its files to whom it names, then clears both", async () => {
    const asked = server();
    const given = files({ empty: false, compose: vi.fn((text: string) => `${text}\n[a.png]`) });
    const drawn = composer(["Ship it", "amber"], given);
    const event = submit(drawn.node);
    expect(event.preventDefault).toHaveBeenCalled();
    expect(asked.map((one) => [one.url, one.body])).toEqual([
      ["/api/task-message", { slug: "w/a", text: "Ship it\n[a.png]", to: "amber" }],
    ]);
    expect(calls(drawn.busy)).toEqual([[true]]);
    const answer = reply(200, {});
    asked[0]?.answer(answer);
    await read(answer);
    expect(given.clear).toHaveBeenCalledTimes(1);
    expect(calls(drawn.draft)).toEqual([[""]]);
    expect(load("task-draft:w/a")).toBe("");
    expect(calls(drawn.result)).toEqual([["Sent"]]);
    expect(calls(drawn.busy)).toEqual([[true], [false]]);
  });

  it("keeps a draft the owner went on typing while it was sent", async () => {
    const asked = server();
    const given = files();
    const drawn = composer(["Ship it"], given);
    submit(drawn.node);
    // The owner types on: the page draws the longer draft while the post is out.
    composer(["Ship it, then"], given);
    const answer = reply(200, {});
    asked[0]?.answer(answer);
    await read(answer);
    expect(given.clear).toHaveBeenCalledTimes(1);
    expect(calls(drawn.draft)).toEqual([]);
    expect(calls(drawn.result)).toEqual([[""]]);
  });

  it("keeps the draft and says why when the send fails", async () => {
    const asked = server();
    const given = files();
    const drawn = composer(["Ship it"], given);
    submit(drawn.node);
    const answer = reply(500, { error: "The board is busy" });
    asked[0]?.answer(answer);
    await read(answer);
    expect(given.clear).not.toHaveBeenCalled();
    expect(calls(drawn.draft)).toEqual([]);
    expect(calls(drawn.result)).toEqual([["The board is busy"]]);
    expect(calls(drawn.busy)).toEqual([[true], [false]]);
  });

  it("sends nothing empty, nothing while a file is on its way and nothing out of date", () => {
    const asked = server();
    for (const [draft, given] of [
      ["", files()],
      [" \n\t", files()],
      ["Ship it", files({ busy: true, empty: false })],
    ] as const) {
      runtime.refs = [];
      submit(composer([draft], given).node);
    }
    STALE.set(true);
    runtime.refs = [];
    const stale = composer(["Ship it"]);
    submit(stale.node);
    expect(asked).toEqual([]);
    expect(buttons(stale.node)).toEqual([]);
    expect(all(stale.node, (each) => each.props.type === "submit")[0]?.props.children).toBe(
      CHECKING,
    );
    // Files alone are a message.
    STALE.set(false);
    runtime.refs = [];
    submit(composer([""], files({ empty: false })).node);
    expect(asked).toHaveLength(1);
  });

  it("sends one message at a time", () => {
    const asked = server();
    const drawn = composer(["Ship it"]);
    submit(drawn.node);
    submit(drawn.node);
    expect(asked).toHaveLength(1);
  });

  it("does nothing more once its page is gone", async () => {
    for (const status of [200, 500]) {
      const asked = server();
      const mount = new AbortController();
      runtime.signal = mount.signal;
      runtime.refs = [];
      const given = files();
      const drawn = composer(["Ship it"], given);
      submit(drawn.node);
      mount.abort();
      const answer = reply(status, {});
      asked[0]?.answer(answer);
      await read(answer);
      expect(given.clear).not.toHaveBeenCalled();
      expect([calls(drawn.draft), calls(drawn.result), calls(drawn.busy)], `${status}`).toEqual([
        [],
        [],
        [[true]],
      ]);
    }
  });

  it("sends on ⌘ or Ctrl with Enter, and Enter alone types a line", () => {
    const asked = server();
    for (const [key, ctrlKey, metaKey, sends] of [
      ["Enter", true, false, true],
      ["Enter", false, true, true],
      ["Enter", false, false, false],
      ["a", true, false, false],
    ] as const) {
      runtime.refs = [];
      const drawn = composer(["Ship it"]);
      const event = { key, ctrlKey, metaKey, preventDefault: vi.fn() };
      (byId(drawn.node, "task-message").onKeyDown as (event: unknown) => void)(event);
      expect(event.preventDefault.mock.calls.length, `${key} ${ctrlKey} ${metaKey}`).toBe(
        sends ? 1 : 0,
      );
    }
    expect(asked).toHaveLength(2);
  });

  it("keeps each keystroke's draft and the recipient chosen, and lets a Sent go", () => {
    const typed = composer(["", "", false, "Sent"]);
    (byId(typed.node, "task-message").onChange as (event: unknown) => void)({
      target: { value: "Next" },
    });
    expect(load("task-draft:w/a")).toBe("Next");
    expect([calls(typed.draft), calls(typed.result)]).toEqual([[["Next"]], [[""]]]);
    const failed = composer(["", "", false, "The board is busy"]);
    (byId(failed.node, "task-message").onChange as (event: unknown) => void)({
      target: { value: "Next" },
    });
    expect(calls(failed.result)).toEqual([]);
    (byId(failed.node, "task-recipient").onChange as (value: string) => void)("@orchestrator");
    expect(load("task-to:w/a")).toBe("@orchestrator");
    expect(calls(failed.recipient)).toEqual([["@orchestrator"]]);
  });

  it("lands what the task offers and says what the server answered", async () => {
    const asked = server();
    const drawn = composer([], files(), offer);
    expect(
      all(drawn.node, (each) => each.props.className === "approval-branch")[0]?.props.children,
    ).toBe("w/a");
    button(drawn.node, "✓ Approve")();
    button(drawn.node, "✓ Approve")();
    expect(asked.map((one) => [one.url, one.body])).toEqual([
      [TASK_ACTION_ROUTE, { slug: "w/a", action: "approve", payload: "abc123" }],
    ]);
    const answer = reply(200, { said: "Landed w/a" });
    asked[0]?.answer(answer);
    await read(answer);
    expect(calls(drawn.result)).toEqual([["Landed w/a"]]);
    expect(calls(drawn.busy)).toEqual([[true], [false]]);
  });

  it("says nothing of a landing whose answer has no words, and why one failed", async () => {
    for (const [status, body, words] of [
      [200, { said: 3 }, ""],
      [409, { error: "Not approved" }, "Not approved"],
    ] as const) {
      const asked = server();
      runtime.refs = [];
      const drawn = composer([], files(), offer);
      button(drawn.node, "✓ Approve")();
      const answer = reply(status, body);
      asked[0]?.answer(answer);
      await read(answer);
      expect(calls(drawn.result), `${status}`).toEqual([[words]]);
    }
  });

  it("offers no landing without its payload, none out of date, and drops one its page left", async () => {
    expect(buttons(composer([], files(), { ...offer, payload: null }).node)).toEqual([]);
    const asked = server();
    STALE.set(true);
    const stale = composer([], files(), offer);
    button(stale.node, CHECKING)();
    expect(asked).toEqual([]);
    STALE.set(false);
    for (const status of [200, 500]) {
      const mount = new AbortController();
      runtime.signal = mount.signal;
      runtime.refs = [];
      const drawn = composer([], files(), offer);
      button(drawn.node, "✓ Approve")();
      mount.abort();
      const answer = reply(status, { said: "Landed" });
      asked.at(-1)?.answer(answer);
      await read(answer);
      expect([calls(drawn.result), calls(drawn.busy)], `${status}`).toEqual([[], [[true]]]);
    }
  });
});

describe("a held message passed on", () => {
  const words: HeldWords = { id: 4, author: "ana", text: "Cut", to: "@executor", passed_by: null };
  /** The board's line for `words`, on `w/a`. */
  const line: TaskEntry = {
    message: {
      id: 4,
      slug: "w/a",
      sender: "board",
      recipient: "owner",
      kind: "msg",
      text: "a message is held for people: #4",
      ts: 1,
      person: false,
    },
    stamp: "2026-11-24 10:00",
    blocks: [],
    held: words,
  };
  /** The held message drawn for one who may pass it on, its states the ones given. */
  const held = (states: unknown[] = []) =>
    draw(HeldMessage, { entry: line, held: words, passes: true }, states);
  /** The setters, by their state's name. */
  const named = (set: Mock[]) => {
    const [busy, told] = set;
    return { busy, told };
  };
  /** The props of the one Pass on control `node` draws, by what it says. */
  const control = (node: ReactNode, words: string) => {
    const found = all(node, (each) => each.props.children === words && "onClick" in each.props);
    expect(found, words).toHaveLength(1);
    return found[0]?.props ?? {};
  };

  it("posts its number once and stays pressed until the feed brings the line it became", async () => {
    const asked = server();
    const drawn = held();
    const set = named(drawn.set);
    button(drawn.node, "Pass on")();
    button(drawn.node, "Pass on")();
    expect(asked.map((one) => [one.url, one.body])).toEqual([
      [TASK_PASS_ROUTE, { slug: "w/a", id: 4 }],
    ]);
    expect(calls(set.busy)).toEqual([[true]]);
    const answer = reply(200, { ok: true, line: 9 });
    asked[0]?.answer(answer);
    await read(answer);
    expect([calls(set.busy), calls(set.told)]).toEqual([[[true]], [[""]]]);
    expect(control(held([true]).node, "Pass on").disabled).toBe(true);
  });

  it("says a refusal in the server's words, and may be pressed again", async () => {
    const asked = server();
    const drawn = held();
    const set = named(drawn.set);
    button(drawn.node, "Pass on")();
    const refusal = "Forbidden: ana may not direct agents on this board";
    const answer = reply(403, { error: refusal });
    asked[0]?.answer(answer);
    await read(answer);
    expect([calls(set.busy), calls(set.told)]).toEqual([[[true], [false]], [[refusal]]]);
    const told = held([false, refusal]);
    const status = all(told.node, (each) => each.props.role === "status");
    expect(status.map((each) => each.props.children)).toEqual([refusal]);
    button(told.node, "Pass on")();
    expect(asked).toHaveLength(2);
  });

  it("asks nothing out of date, and drops its answer once its page is gone", async () => {
    const asked = server();
    STALE.set(true);
    const stale = held();
    expect(control(stale.node, CHECKING).disabled).toBe(true);
    button(stale.node, CHECKING)();
    expect(asked).toEqual([]);
    expect(stale.set.every((setter) => setter.mock.calls.length === 0)).toBe(true);
    STALE.set(false);
    for (const status of [200, 500]) {
      const mount = new AbortController();
      runtime.signal = mount.signal;
      runtime.refs = [];
      const gone = held();
      const set = named(gone.set);
      button(gone.node, "Pass on")();
      mount.abort();
      const answer = reply(status, {});
      asked.at(-1)?.answer(answer);
      await read(answer);
      expect([calls(set.busy), calls(set.told)], `${status}`).toEqual([[[true]], []]);
    }
  });
});

describe("the brief read on", () => {
  const props = { slug: "w/a b", revision: "r1", blocks: [], omitted: 900, next: 4096 };

  /** The brief's read after `set`'s one call, from nothing read. */
  const briefAfter = (set: Mock | undefined) => {
    const update = set?.mock.calls[0]?.[0] as (before: typeof NOTHING_READ) => typeof NOTHING_READ;
    return update(NOTHING_READ);
  };

  it("asks for the part after the one shown, and adds it", async () => {
    const asked = server();
    const drawn = draw(Brief, props);
    const [readSet, busy] = drawn.set;
    button(drawn.node, "Show more of the brief")();
    expect(asked.map((one) => one.url)).toEqual(["/api/task-brief?slug=w%2Fa%20b&from=4096"]);
    const answer = reply(200, { blocks: [], next: 5000, omitted: 0, revision: "r1" });
    asked[0]?.answer(answer);
    await read(answer);
    expect(briefAfter(readSet)).toEqual({
      revision: "r1",
      blocks: [],
      next: 5000,
      omitted: 0,
      failed: "",
    });
    expect(calls(busy)).toEqual([[true], [false]]);
  });

  it("says a part it cannot read, or why the read failed", async () => {
    for (const [status, body, failed] of [
      [200, { next: 1 }, "The brief's part is not readable"],
      [500, { error: "down" }, "down (the server answered 500)"],
    ] as const) {
      const asked = server();
      const drawn = draw(Brief, props);
      button(drawn.node, "Show more of the brief")();
      const answer = reply(status, body);
      asked[0]?.answer(answer);
      await read(answer);
      expect(briefAfter(drawn.set[0]).failed, `${status}`).toBe(failed);
    }
  });

  it("asks nothing while it reads, and keeps nothing once its page is gone", async () => {
    const asked = server();
    const busy = draw(Brief, props, [NOTHING_READ, true]);
    button(busy.node, "Show more of the brief")();
    expect(asked).toEqual([]);
    const mount = new AbortController();
    runtime.signal = mount.signal;
    const drawn = draw(Brief, props);
    button(drawn.node, "Show more of the brief")();
    mount.abort();
    const answer = reply(200, { blocks: [], next: 5000, omitted: 0, revision: "r1" });
    asked[0]?.answer(answer);
    await read(answer);
    expect([calls(drawn.set[0]), calls(drawn.set[1])]).toEqual([[], [[true]]]);
  });
});

/** A live task as the page reads it: what `fields` say of it, each of its calls a spy. */
function aLive(fields: Record<string, unknown> = {}): LiveTask {
  const nothing = { value: undefined, stale: false, error: null, missing: false, answered: false };
  return {
    slug: "w/a",
    scope: "",
    page: () => null,
    missing: () => false,
    thread: () => nothing,
    error: () => "",
    parts: { shown: () => nothing },
    pagePart: { key: "page w/a" },
    history: vi.fn(async () => {}),
    children: vi.fn(async () => {}),
    again: vi.fn(),
    drawn: () => true,
    boardShown: true,
    trouble: null,
    retry: { current: 0 },
    ...fields,
  } as unknown as LiveTask;
}

describe("the page's wiring", () => {
  it("hands each of the task's asks to its live task", () => {
    const live = aLive({
      page: () => ({ task: { slug: "w/a" }, machine_work: {}, children: [] }),
    });
    const node = LiveTaskPage({ live, connected: true }) as ReactElement<{
      children: ReactElement<{ children: ReactElement }>;
    }>;
    // The page's store around the task's detail.
    const detail = node.props.children.props.children.props as Record<
      string,
      (...args: unknown[]) => void
    >;
    detail.onHistory?.(true);
    detail.onChildren?.(false);
    detail.onRetry?.();
    expect(live.history).toHaveBeenCalledWith(true);
    expect(live.children).toHaveBeenCalledWith(false);
    expect(live.again).toHaveBeenCalledTimes(1);
  });

  it("reads a missing task again when asked", () => {
    const live = aLive({ missing: () => true });
    const node = LiveTaskPage({ live, connected: true }) as ReactElement<{
      children: ReactElement<{ retry: () => void }>;
    }>;
    node.props.children.props.retry();
    expect(live.again).toHaveBeenCalledTimes(1);
  });

  it("marks the board drawn once, and tries a failed board again when asked", () => {
    const marked = vi.spyOn(performance, "mark");
    const shown = draw(TaskPageView, { live: aLive(), stale: false, freshness: FRESH });
    shown.effects[0]?.();
    expect(marked.mock.calls.map((call) => call[0])).toContain("board-ready");
    marked.mockClear();
    const live = aLive({
      drawn: () => false,
      boardShown: false,
      trouble: "The server answered 500",
    });
    const failed = draw(TaskPageView, { live, stale: false, freshness: FRESH });
    failed.effects[0]?.();
    expect(marked).not.toHaveBeenCalled();
    const troubles = all(failed.node, (each) => typeof each.props.retry === "function");
    expect(troubles).toHaveLength(1);
    const retry = troubles[0]?.props.retry as () => void;
    retry();
    retry();
    expect((live as unknown as { retry: { current: number } }).retry.current).toBe(2);
    marked.mockRestore();
  });

  it("keeps one live task for its checkout, moves it with the route and stops it as it goes", () => {
    vi.useFakeTimers();
    try {
      const drawn = draw(TaskPage, { scope: "other", slug: "w/a" });
      expect(made.tasks).toEqual([]);
      const leave = drawn.effects[0]?.();
      const [task] = made.tasks;
      expect([task?.scope, task?.slug, task?.signal.aborted]).toEqual(["other", "w/a", false]);
      expect(task?.start).toHaveBeenCalledWith(runtime.frame);
      expect(drawn.reducers[0]).toHaveBeenCalledTimes(1);
      // A checkout's page starts as fresh as its reads.
      expect(calls(drawn.set[1])).toEqual([[FRESH]]);
      // The route moves within the checkout: the same live task follows it.
      const moved = draw(TaskPage, { scope: "other", slug: "w/b" });
      moved.effects[1]?.();
      expect(task?.moved).toHaveBeenCalledWith("w/b");
      expect(made.tasks).toHaveLength(1);
      // No timer of its own: a held reload waits for the reader to be done, not a minute.
      expect(vi.getTimerCount()).toBe(0);
      leave?.();
      expect(task?.signal.aborted).toBe(true);
      expect(vi.getTimerCount()).toBe(0);
    } finally {
      vi.useRealTimers();
    }
  });
});
