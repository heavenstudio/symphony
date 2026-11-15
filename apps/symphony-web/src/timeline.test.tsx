import { isValidElement, type KeyboardEvent, type ReactElement, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import FIXTURE from "../../../crates/symphony-api/fixtures/task-timeline.json";
import { whenShown } from "./feed";
import { Failure, NO_ANSWER as SLOW } from "./request";
import {
  afterRead,
  drawnClass,
  follow,
  type Held,
  isWide,
  measured,
  NO_ANSWER,
  NOT_A_TIMELINE,
  NOTHING_HELD,
  putStyle,
  readFailed,
  readOnce,
  type Stops,
  settle,
  stopFor,
  stopsOf,
  TimelineDrawn,
  TimelineFailed,
  TimelineHost,
  TimelineView,
  timelineUrl,
  watchCrossings,
} from "./timeline";
import { lanes, readTimeline, stops } from "./timeline-layout";
import type { TaskTimeline } from "./wire";

// React's effects stood in for, as no DOM runs here: each effect a hook sets is kept to be run by
// its test, and a state a test gives (`states`, in the order the hooks ask) stands for what an
// effect would have set.
const react = vi.hoisted(() => ({
  effects: [] as (() => undefined | (() => void))[],
  layouts: [] as (() => undefined | (() => void))[],
  states: [] as unknown[],
}));

vi.mock("react", async (original) => {
  const actual = await original<typeof import("react")>();
  return {
    ...actual,
    useEffect: (effect: () => undefined | (() => void)) => {
      react.effects.push(effect);
    },
    useLayoutEffect: (effect: () => undefined | (() => void)) => {
      react.layouts.push(effect);
    },
    useState: (initial: unknown) => {
      const given = react.states.length > 0;
      const state = actual.useState(initial);
      return given ? [react.states.shift(), state[1]] : state;
    },
  };
});

/** The approved pictures' task, as the server answers it. */
function pictures(): TaskTimeline {
  const read = readTimeline(JSON.parse(JSON.stringify(FIXTURE)));
  if (read === null) throw new Error("the fixture is no timeline");
  return read;
}

/** The timeline's view as a page draws it, before any measure. */
const html = (timeline: TaskTimeline) => renderToStaticMarkup(<TimelineView timeline={timeline} />);

const count = (text: string, part: string) => text.split(part).length - 1;

/** What the stops do, each a spy. */
function spiedStops(active = 0): Stops {
  return { active, focus: vi.fn(), blur: vi.fn(), key: vi.fn() };
}

/** Every element of `node`'s tree that `test` holds for, drawing each component without hooks. */
function elements(
  node: ReactNode,
  test: (element: ReactElement<Props>) => boolean,
): ReactElement<Props>[] {
  if (Array.isArray(node)) return node.flatMap((each) => elements(each, test));
  if (!isValidElement<Props>(node)) return [];
  const found = test(node) ? [node] : [];
  const drawn =
    typeof node.type === "function" && node.type.name !== "Button"
      ? (node.type as (props: Props) => ReactNode)(node.props)
      : node.props.children;
  return [...found, ...elements(drawn, test)];
}

type Props = Record<string, unknown> & { children?: ReactNode };

/** Press `element` as a click would: its handler, which it must have. */
function press(element: ReactElement<Props> | undefined): void {
  const onClick = element?.props.onClick;
  if (typeof onClick !== "function") throw new Error("nothing to press");
  onClick();
}

/** The page's document: shown or hidden, its events, no element of the project's. */
function aPage(hidden = false) {
  const events = new EventTarget();
  const page = {
    hidden,
    getElementById: (): null => null,
    addEventListener: events.addEventListener.bind(events),
    removeEventListener: events.removeEventListener.bind(events),
    dispatchEvent: events.dispatchEvent.bind(events),
  };
  vi.stubGlobal("document", page);
  vi.stubGlobal("location", { pathname: "/task/tools/x" });
  return page;
}

/** One request the stubbed server holds, and how it is answered or refused. */
interface Asked {
  url: string;
  init: RequestInit;
  answer: (response: Response) => void;
  refuse: (error: unknown) => void;
}

function aServer(): Asked[] {
  const asked: Asked[] = [];
  vi.stubGlobal(
    "fetch",
    (url: string, init: RequestInit) =>
      new Promise<Response>((answer, refuse) => asked.push({ url, init, answer, refuse })),
  );
  return asked;
}

/** A response of `status` whose body reads as `body` (or fails with it, an error), tagged `tag`. */
function reply(status: number, body: unknown, tag?: string): Response {
  return {
    status,
    ok: status >= 200 && status < 300,
    headers: new Headers(tag === undefined ? {} : { ETag: tag }),
    json: () => (body instanceof Error ? Promise.reject(body) : Promise.resolve(body)),
  } as unknown as Response;
}

const flush = () => vi.advanceTimersByTimeAsync(0);

beforeEach(() => {
  react.effects.length = 0;
  react.layouts.length = 0;
  react.states.length = 0;
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("the timeline drawn", () => {
  it("the host render draws the list, the lanes with their words, and what the bars mean", () => {
    const drawn = html(pictures());
    expect(drawn).toContain("6 h 18 m so far · 2 review rounds · 1 bounce");
    expect(drawn).toContain(">Machine 1 h 33 m, queued 26 m<");
    expect(drawn).toContain("> · machine 1 h 33 m, queued 26 m<");
    expect(count(drawn, '<li class="tl-row')).toBe(13);
    expect(count(drawn, 'class="tl-row tl-sub tl-event"')).toBe(2);
    for (const lane of ["stage", "executor", "reviewer", "runs"]) {
      expect(drawn).toContain(`data-lane="${lane}"`);
    }
    expect(count(drawn, 'class="tl-lane tl-gate"')).toBe(4);
    // What runs now: striped in the list and in its lane, and the legend says so.
    expect(count(drawn, "tl-work tl-running")).toBe(1);
    expect(count(drawn, "tl-seg tl-running")).toBe(1);
    expect(drawn).toContain("running now");
    expect(drawn).toContain(">now 15:20<");
    expect(drawn).toContain(">changes<");
    expect(drawn).toContain(">approved<");
    expect(drawn).toContain('class="tl-g tl-g-waived"');
    expect(drawn).toContain("--c:var(--question)");
    // The keyboard stops at the first part, its words read aloud; the rest by the arrow keys.
    expect(count(drawn, 'tabindex="0"')).toBe(1);
    expect(count(drawn, 'tabindex="-1"')).toBe(stops(lanes(pictures())) - 1);
    expect(drawn).toContain(
      'id="tl-stop-0" tabindex="0" role="img" aria-label="ready 41 m, from 09:02 to 09:43"',
    );
    expect(drawn).toContain('aria-label="nightly met from 09:02: ');
    // A round's wait is drawn, not stopped at: its reviewer's time after it says all of it.
    expect(drawn).toContain(
      'title="round 1: waited 18 m for a reviewer, then lucid-heron took 34 m, changes" aria-hidden="true"',
    );
    // The list is a list of steps; the lanes a group; the gate states in the legend by their own
    // marks, the ones the timeline holds.
    expect(drawn).toContain('<ol class="tl-list" aria-label="Steps">');
    expect(drawn).toContain('role="group" aria-label="Lanes"');
    // In the approved picture's order: met, missing, waived.
    const legend = drawn
      .split(">gate ")
      .slice(1)
      .map((after) => after.split("<")[0]);
    expect(legend).toEqual(["met", "missing", "waived"]);
    // No overflow is measured before the page lays it out: no "Show all". No part of the lanes
    // has the focus: no line for its detail.
    expect(drawn).not.toContain("Show all");
    expect(drawn).not.toContain("tl-detail");
    expect(drawn).toContain('id="tl-body"');
  });

  it("draws each element and attribute as the Dioxus view, in its order", () => {
    const drawn = html(pictures());
    expect(
      drawn.startsWith('<div class="tl-drawn"><div class="tl-head"><p class="tl-totals">'),
    ).toBe(true);
    // A row: its time, its mark, its label, its length and its bars, the tone its own.
    expect(drawn).toContain(
      '<li class="tl-row" style="--c:var(--auto)" title="Ready, unclaimed: from 09:02 to 09:43, moved there by glacial-merlin"><span class="tl-at">09:02</span><span class="tl-mark" aria-hidden="true"></span><span class="tl-label">Ready, unclaimed</span><span class="tl-took">41 m</span><span class="tl-bars"><span class="tl-work" aria-hidden="true" style="width:26.36%"></span></span></li>',
    );
    // A review round: its wait for a reviewer before its work, and its verdict.
    expect(drawn).toMatch(
      /<span class="tl-wait" aria-hidden="true" style="--c:var\(--question\);width:11\.57%"><\/span><span class="tl-work" aria-hidden="true" style="width:21\.86%"><\/span><span class="tl-word" style="--c:var\(--dislike\)">changes<\/span>/,
    );
    // The lanes' hours, the axis and now, the gates under their head.
    expect(drawn).toContain(
      '<div class="tl-hours" aria-hidden="true"><span class="tl-hour" style="left:0.00%"></span>',
    );
    expect(drawn).toContain('<span class="tl-now" style="left:100.00%"></span></div>');
    expect(drawn).toContain(
      '<div class="tl-lane tl-axis" aria-hidden="true"><span class="tl-name"></span><span class="tl-track"><span class="tl-tick" style="left:0.00%">09:00</span>',
    );
    expect(drawn).toContain(
      '<span class="tl-tick tl-nowtag" style="left:100.00%">now 15:20</span>',
    );
    expect(drawn).toContain(
      '<div class="tl-lane" data-lane="executor" style="--rows:1"><span class="tl-name"><b>Executor</b><small>quiet-wren</small></span>',
    );
    expect(drawn).toContain(
      '<div class="tl-lane" data-lane="stage" style="--rows:1"><span class="tl-name"><b>Stage</b></span>',
    );
    expect(drawn).toContain(
      '<div class="tl-lane tl-gates-head"><span class="tl-name"><b>Gates</b></span></div><div class="tl-lane tl-gate" data-gate="certificate"><span class="tl-name">certificate</span>',
    );
    expect(drawn).toContain(
      'class="tl-after tl-fit" aria-hidden="true" style="--c:var(--dislike);left:',
    );
    expect(drawn).toContain('class="tl-under tl-fit tl-from-');
    expect(drawn).toContain("max-width:calc(");
    expect(drawn).toContain(
      '<li><span class="tl-key tl-key-work"></span>worked or ran</li><li><span class="tl-key tl-key-wait"></span>queued or waiting</li><li><span class="tl-key tl-key-running"></span>running now</li><li class="tl-wide"><span class="tl-key tl-g tl-g-met"></span>gate met</li>',
    );
    expect(drawn.endsWith("</ul></div></div>")).toBe(true);
  });

  it("a task with nothing but its making draws one stage and no other lane", () => {
    const now = 1_794_403_200;
    const timeline = {
      slug: "tools/new",
      now,
      zone: [{ from: now - 120, seconds: 7200 }],
      created: now - 120,
      bounces: 0,
      stages: [{ status: "ready", from: now - 120 }],
      claims: [],
      rounds: [],
      runs: [],
      gates: [],
    } as TaskTimeline;
    const drawn = html(timeline);
    expect(drawn).toContain("2 m so far · 0 review rounds · 0 bounces");
    expect(drawn).not.toContain("gate met");
    expect(drawn).not.toContain("achine");
    expect(drawn).not.toContain("running now");
    expect(drawn).not.toContain("tl-gates-head");
  });

  it("with no run kept there is no machine time, no runs lane and nothing running", () => {
    const drawn = html({ ...pictures(), runs: [] });
    for (const part of ["achine", 'data-lane="runs"', "tl-running", "running now"]) {
      expect(drawn).not.toContain(part);
    }
  });

  it("offers all of a clipped list, the latest again once shown, and the focused part's detail", () => {
    const toggle = vi.fn();
    const at = spiedStops(3);
    const props = { timeline: { ...pictures(), runs_from: pictures().created + 83 * 60 }, toggle };
    const clipped = renderToStaticMarkup(
      <TimelineDrawn {...props} all={false} clipped detail="" stops={at} />,
    );
    expect(clipped.startsWith('<div class="tl-drawn tl-clipped">')).toBe(true);
    expect(clipped).toContain(
      '<button class="dx-button tl-more" data-style="outline" data-size="default" type="button" aria-expanded="false">Show all 13 steps</button>',
    );
    expect(clipped).toContain(
      '<p class="tl-note">Builds and tests before 11-11 10:25 are no longer kept.</p>',
    );
    expect(clipped).toContain('id="tl-stop-3" tabindex="0"');
    expect(clipped).toContain('id="tl-stop-0" tabindex="-1"');
    const all = renderToStaticMarkup(
      <TimelineDrawn {...props} all clipped={false} detail="review 52 m, from 11:35" stops={at} />,
    );
    expect(all.startsWith('<div class="tl-drawn tl-all">')).toBe(true);
    expect(all).toContain('aria-expanded="true">Show the latest</button>');
    expect(all).toContain(
      '<p class="tl-detail tl-wide" aria-hidden="true">review 52 m, from 11:35</p>',
    );
    const [button] = elements(
      TimelineDrawn({ ...props, all: true, clipped: false, detail: "", stops: at }),
      (element) => element.props.className === "tl-more",
    );
    press(button);
    expect(toggle).toHaveBeenCalledTimes(1);
  });

  it("each part of the lanes the keyboard stops at says its focus, its blur and its keys", () => {
    const at = spiedStops();
    const tree = TimelineDrawn({
      timeline: pictures(),
      all: false,
      clipped: false,
      detail: "",
      stops: at,
      toggle: () => {},
    });
    const stopped = (id: string) => {
      const [element] = elements(
        tree,
        (one) => one.props.id === id || (one.props.stop as { id?: string } | undefined)?.id === id,
      );
      return (element?.props.stop ?? element?.props) as Record<string, (event?: unknown) => void>;
    };
    const first = stopped("tl-stop-0");
    first.onFocus?.();
    expect(at.focus).toHaveBeenCalledWith(0, "ready 41 m, from 09:02 to 09:43");
    first.onBlur?.();
    expect(at.blur).toHaveBeenCalledTimes(1);
    const event = { key: "ArrowRight" };
    first.onKeyDown?.(event);
    const count = stops(lanes(pictures()));
    expect(at.key).toHaveBeenCalledWith(event, 0, count);
    // A gate's state is a stop as the steps are, the last of them.
    const gate = stopped(`tl-stop-${count - 1}`);
    gate.onFocus?.();
    expect((at.focus as ReturnType<typeof vi.fn>).mock.lastCall?.[1]).toMatch(/^review met from /);
  });
});

describe("the timeline's host", () => {
  it("before its first answer draws nothing, and the page's placeholder shows", () => {
    const drawn = renderToStaticMarkup(<TimelineHost slug="tools/new" revision={1} />);
    expect(drawn).not.toContain("tl-drawn");
    expect(drawn).not.toContain("Reading");
    expect(drawn).toBe("");
  });

  it("puts its style in the page, reads when it mounts and gives the read up when it leaves", () => {
    const page = aPage();
    const appended: { id: string; textContent: string }[] = [];
    Object.assign(page, {
      getElementById: (id: string) => appended.find((element) => element.id === id) ?? null,
      createElement: () => ({ id: "", textContent: "" }),
      // A page with no theme of its own: the style goes last, as `insertBefore(…, null)` puts it.
      head: {
        querySelector: () => null,
        insertBefore: (element: { id: string; textContent: string }, before: null) =>
          before === null && appended.push(element),
      },
    });
    const asked = aServer();
    renderToStaticMarkup(<TimelineHost slug="tools/new" revision={1} />);
    expect([react.layouts.length, react.effects.length]).toEqual([1, 1]);
    react.layouts[0]?.();
    expect(appended.map((element) => element.id)).toEqual(["timeline-style"]);
    const leave = react.effects[0]?.();
    expect(asked.map((one) => one.url)).toEqual(["/api/task-timeline?slug=tools%2Fnew"]);
    expect(asked[0]?.init.signal?.aborted).toBe(false);
    leave?.();
    expect(asked[0]?.init.signal?.aborted).toBe(true);
  });

  it("draws what its reads show: a failure with its Try again, else the timeline", () => {
    react.states.push({ error: "Board snapshot unavailable. " });
    expect(renderToStaticMarkup(<TimelineHost slug="tools/x" revision={1} />)).toBe(
      renderToStaticMarkup(
        <TimelineFailed error="Board snapshot unavailable. " retry={() => {}} />,
      ),
    );
    react.states.push({ timeline: pictures() });
    expect(renderToStaticMarkup(<TimelineHost slug="tools/x" revision={1} />)).toBe(
      html(pictures()),
    );
  });

  it("a failed read with nothing shown says why in the box, with its Try again", () => {
    const retry = vi.fn();
    expect(renderToStaticMarkup(<TimelineFailed error="Failed to fetch" retry={retry} />)).toBe(
      '<div class="tl-drawn tl-failed"><p class="tl-note">The timeline could not be read: Failed to fetch. <button class="dx-button" data-style="outline" data-size="default" type="button">Try again</button></p></div>',
    );
    const [button] = elements(
      TimelineFailed({ error: "", retry }),
      (element) => element.props.children === "Try again",
    );
    press(button);
    expect(retry).toHaveBeenCalledTimes(1);
  });

  it("the view watches each crossing of 900 px and measures each draw", () => {
    const page = new EventTarget();
    vi.stubGlobal("addEventListener", page.addEventListener.bind(page));
    vi.stubGlobal("removeEventListener", page.removeEventListener.bind(page));
    vi.stubGlobal("innerWidth", 1200);
    const onDrawn = vi.fn();
    renderToStaticMarkup(<TimelineView timeline={pictures()} onDrawn={onDrawn} />);
    expect([react.layouts.length, react.effects.length]).toEqual([1, 1]);
    // No element was drawn here: nothing to measure, nothing said.
    react.layouts[0]?.();
    expect(onDrawn).not.toHaveBeenCalled();
    const stop = react.effects[0]?.();
    expect(typeof stop).toBe("function");
    stop?.();
  });
});

describe("the timeline's reads", () => {
  it("asks for the task's timeline under the checkout its route names", () => {
    aPage();
    expect(timelineUrl("tools/x y")).toBe("/api/task-timeline?slug=tools%2Fx%20y");
    vi.stubGlobal("location", { pathname: "/@feature-x/task/tools/x" });
    expect(timelineUrl("tools/x")).toBe("/@feature-x/api/task-timeline?slug=tools%2Fx");
  });

  it("asks with the last answer's validator, and reads a 304 as the one kept", async () => {
    aPage();
    const asked = aServer();
    const signal = new AbortController().signal;
    const first = readOnce("tools/x", null, signal);
    expect(asked[0]?.init.headers).toBeUndefined();
    // Past the browser's own cache, as every read the page checks.
    expect(asked[0]?.init.cache).toBe("no-store");
    asked[0]?.answer(reply(200, FIXTURE, '"v1"'));
    expect(await first).toEqual({ fresh: pictures(), tag: '"v1"' });
    const again = readOnce("tools/x", '"v1"', signal);
    expect(asked[1]?.init.headers).toEqual({ "If-None-Match": '"v1"' });
    asked[1]?.answer(reply(304, null));
    expect(await again).toEqual({ same: true });
    // An answer with no validator is kept without one.
    const untagged = readOnce("tools/x", null, signal);
    asked[2]?.answer(reply(200, FIXTURE));
    expect(await untagged).toEqual({ fresh: pictures(), tag: null });
  });

  it("says the server's own words of a refusal, its status where it gave none", async () => {
    aPage();
    const asked = aServer();
    const signal = new AbortController().signal;
    const cases: [Response, string][] = [
      [reply(503, { error: "Board snapshot unavailable." }), "Board snapshot unavailable."],
      [reply(500, { error: 7 }), "the server answered 500"],
      [reply(502, new SyntaxError("Unexpected token <")), "the server answered 502"],
      [reply(200, new SyntaxError("Unexpected end of JSON input")), "Unexpected end of JSON input"],
      [reply(200, { now: "soon" }), NOT_A_TIMELINE],
    ];
    for (const [response, error] of cases) {
      const read = readOnce("tools/x", null, signal);
      asked.at(-1)?.answer(response);
      expect(await read).toEqual({ error });
    }
    const refused = readOnce("tools/x", null, signal);
    asked.at(-1)?.refuse(new TypeError("Failed to fetch"));
    expect(await refused).toEqual({ error: "Failed to fetch" });
    const thrown = readOnce("tools/x", null, signal);
    asked.at(-1)?.refuse("offline");
    expect(await thrown).toEqual({ error: "offline" });
  });

  it("says a failure in the server's or the browser's own words, a slow one in its own", () => {
    expect(
      readFailed(Failure.refused(503, "Not ready (the server answered 503)", "Not ready.")),
    ).toBe("Not ready.");
    expect(readFailed(new Failure("uncertain", SLOW))).toBe(NO_ANSWER);
    expect(readFailed(new TypeError("Failed to fetch"))).toBe("Failed to fetch");
    expect(readFailed("offline")).toBe("offline");
  });

  it("gives up a read with no answer in fifteen seconds, and one its page left", async () => {
    vi.useFakeTimers();
    aPage();
    const asked = aServer();
    const slow = readOnce("tools/x", null, new AbortController().signal);
    await vi.advanceTimersByTimeAsync(14_999);
    expect(asked[0]?.init.signal?.aborted).toBe(false);
    await vi.advanceTimersByTimeAsync(1);
    expect(await slow).toEqual({ error: NO_ANSWER });
    expect(asked[0]?.init.signal?.aborted).toBe(true);
    const leaving = new AbortController();
    const left = readOnce("tools/x", null, leaving.signal);
    leaving.abort();
    expect(asked[1]?.init.signal?.aborted).toBe(true);
    asked[1]?.refuse(new DOMException("The operation was aborted.", "AbortError"));
    expect(await left).toEqual({ error: "The operation was aborted." });
    // A page already left gives its read up before it is sent.
    const gone = new AbortController();
    gone.abort();
    void readOnce("tools/x", null, gone.signal);
    expect(asked[2]?.init.signal?.aborted).toBe(true);
  });

  it("shows a new answer, moves the one kept on by this browser's clock, and keeps it on a failure", () => {
    const timeline = pictures();
    const fresh = afterRead(NOTHING_HELD, { fresh: timeline, tag: '"v1"' }, 5_000);
    expect(fresh).toEqual({
      shown: { timeline },
      kept: { tag: '"v1"', at: [timeline.now, 5_000] },
    });
    const same = afterRead(fresh, { same: true }, 50_000);
    expect(same.shown).toEqual({ timeline: { ...timeline, now: timeline.now + 45 } });
    expect(same.kept).toEqual(fresh.kept);
    // Moved from the clock of the answer, not of the last 304.
    expect(afterRead(same, { same: true }, 65_000).shown).toEqual({
      timeline: { ...timeline, now: timeline.now + 60 },
    });
    // A failure after a timeline leaves it shown; with none shown it is said.
    expect(afterRead(fresh, { error: "Failed to fetch" }, 9_000)).toBe(fresh);
    const failed = afterRead(NOTHING_HELD, { error: "Failed to fetch" }, 9_000);
    expect(failed).toEqual({ ...NOTHING_HELD, shown: { error: "Failed to fetch" } });
    // A 304 with nothing shown, or a failure shown, changes nothing.
    expect(afterRead(NOTHING_HELD, { same: true }, 9_000)).toBe(NOTHING_HELD);
    expect(afterRead(failed, { same: true }, 9_000)).toBe(failed);
    // A later failure says its own reason where no timeline is shown.
    expect(afterRead(failed, { error: "gone" }, 9_000).shown).toEqual({ error: "gone" });
  });

  it("waits for a hidden page to show, or for its signal to end", async () => {
    vi.stubGlobal("document", undefined);
    await whenShown(new AbortController().signal);
    const page = aPage(true);
    let shown = false;
    const waiting = whenShown(new AbortController().signal).then(() => {
      shown = true;
    });
    page.dispatchEvent(new Event("visibilitychange"));
    await Promise.resolve();
    expect(shown).toBe(false);
    page.hidden = false;
    page.dispatchEvent(new Event("visibilitychange"));
    await waiting;
    expect(shown).toBe(true);
    page.hidden = true;
    const ending = new AbortController();
    const ended = whenShown(ending.signal);
    ending.abort();
    await ended;
    const already = new AbortController();
    already.abort();
    await whenShown(already.signal);
    page.hidden = false;
    await whenShown(new AbortController().signal);
  });

  it("reads each `every` seconds while open, with the validator, and not while the page is hidden", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_000_000);
    const page = aPage();
    const asked = aServer();
    const held = { current: NOTHING_HELD };
    const drawn = vi.fn();
    const reading = new AbortController();
    const done = follow("tools/x", reading.signal, held, drawn);
    await flush();
    expect(asked.map((one) => one.url)).toEqual(["/api/task-timeline?slug=tools%2Fx"]);
    asked[0]?.answer(reply(200, { ...FIXTURE, every: 45 }, '"v1"'));
    await flush();
    expect(drawn).toHaveBeenLastCalledWith({ timeline: { ...pictures(), every: 45 } });
    expect(held.current.kept).toEqual({ tag: '"v1"', at: [FIXTURE.now, 1_000_000] });
    await vi.advanceTimersByTimeAsync(44_999);
    expect(asked).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(asked).toHaveLength(2);
    expect(asked[1]?.init.headers).toEqual({ "If-None-Match": '"v1"' });
    asked[1]?.answer(reply(304, null));
    await flush();
    expect(drawn).toHaveBeenCalledTimes(2);
    expect(drawn.mock.lastCall?.[0].timeline.now).toBe(FIXTURE.now + 45);
    // Hidden, the page is not read until it shows again, then at once.
    page.hidden = true;
    await vi.advanceTimersByTimeAsync(10 * 45_000);
    expect(asked).toHaveLength(2);
    page.hidden = false;
    page.dispatchEvent(new Event("visibilitychange"));
    await flush();
    expect(asked).toHaveLength(3);
    // Left while a read is on its way: it is given up, and its answer draws nothing.
    reading.abort();
    expect(asked[2]?.init.signal?.aborted).toBe(true);
    asked[2]?.answer(reply(200, FIXTURE));
    await done;
    expect(drawn).toHaveBeenCalledTimes(2);
  });

  it("reads a closed task once, and a failed read again a minute after", async () => {
    vi.useFakeTimers();
    aPage();
    const asked = aServer();
    const drawn = vi.fn();
    const closed = follow(
      "tools/x",
      new AbortController().signal,
      { current: NOTHING_HELD },
      drawn,
    );
    await flush();
    asked[0]?.answer(reply(200, { ...FIXTURE, closed: FIXTURE.now }));
    await closed;
    await vi.advanceTimersByTimeAsync(3_600_000);
    expect(asked).toHaveLength(1);
    const reading = new AbortController();
    const held: { current: Held } = { current: NOTHING_HELD };
    const failing = follow("tools/y", reading.signal, held, drawn);
    await flush();
    asked[1]?.refuse(new TypeError("Failed to fetch"));
    await flush();
    expect(drawn).toHaveBeenLastCalledWith({ error: "Failed to fetch" });
    await vi.advanceTimersByTimeAsync(59_999);
    expect(asked).toHaveLength(2);
    await vi.advanceTimersByTimeAsync(1);
    expect(asked).toHaveLength(3);
    asked[2]?.answer(reply(200, FIXTURE, '"v2"'));
    await flush();
    expect(drawn).toHaveBeenLastCalledWith({ timeline: pictures() });
    // Left while it waits for its next read: it ends at once and reads nothing more.
    reading.abort();
    await failing;
    await vi.advanceTimersByTimeAsync(120_000);
    expect(asked).toHaveLength(3);
  });
});

describe("the timeline measured", () => {
  const box = (height: number) => ({ getBoundingClientRect: () => ({ height }) });
  /** The steps' part `held` tall in a box `slot` tall, its list and lanes as tall as drawn. */
  const steps = (held: number, slot: number, list: number | null, lanesHeight: number | null) => ({
    clientHeight: held,
    getBoundingClientRect: () => ({ height: held }),
    querySelector: (selector: string) => {
      const height = selector === ".tl-list" ? list : selector === ".tl-lanes" ? lanesHeight : null;
      return height === null ? null : box(height);
    },
    closest: (selector: string) => (selector === "#timeline-slot" ? box(slot) : null),
  });

  it("a phone keeps the box's height less the room the list lacked, and says it clips", () => {
    const keep = vi.fn();
    const body = steps(160, 240, 500, 900);
    expect(settle(body, box(240), false, false, keep)).toBe(true);
    expect(keep).toHaveBeenCalledWith(false, 240 - 160 + 500);
    // A list that fits, to a pixel, is not clipped.
    expect(settle(steps(160, 240, 161, 0), box(240), false, false, keep)).toBe(false);
    expect(settle(steps(160, 240, 161.5, 0), box(240), false, false, keep)).toBe(true);
  });

  it("wide keeps the whole box, and showing all keeps nothing", () => {
    const keep = vi.fn();
    expect(settle(steps(480, 520, 900, 480), box(520), true, false, keep)).toBe(false);
    expect(keep).toHaveBeenCalledWith(true, 520);
    keep.mockClear();
    expect(settle(steps(160, 240, 500, 0), box(240), false, true, keep)).toBe(true);
    expect(keep).not.toHaveBeenCalled();
    // Nothing drawn measures as nothing; no box, nothing at all.
    expect(settle(steps(160, 240, null, null), box(240), false, false, keep)).toBe(false);
    expect(keep).toHaveBeenCalledWith(false, 80);
    expect(settle(null, box(240), false, false, keep)).toBe(false);
    expect(settle(steps(1, 1, 1, 1), null, false, false, keep)).toBe(false);
  });

  it("sets the clipped state only where a measure at the box's own height changed it", () => {
    vi.stubGlobal("innerWidth", 600);
    const keep = vi.fn();
    const over = steps(160, 240, 500, 0);
    expect(measured(over, false, false, keep)).toBe(true);
    expect(measured(over, false, true, keep)).toBeNull();
    expect(measured(steps(160, 240, 100, 0), false, true, keep)).toBe(false);
    // Showing all says nothing of the box's own height.
    expect(measured(over, true, false, keep)).toBeNull();
    expect(measured(null, false, false, keep)).toBeNull();
    expect(measured(null, false, true, keep)).toBe(false);
    // Wide the lanes are measured, in their box.
    vi.stubGlobal("innerWidth", 900);
    keep.mockClear();
    expect(measured(steps(480, 520, 900, 480), false, false, keep)).toBeNull();
    expect(keep).toHaveBeenCalledWith(true, 520);
  });

  it("is wide from 900 px, and hears each crossing of it, either way, until stopped", () => {
    const page = new EventTarget();
    vi.stubGlobal("addEventListener", page.addEventListener.bind(page));
    vi.stubGlobal("removeEventListener", page.removeEventListener.bind(page));
    vi.stubGlobal("innerWidth", 899);
    expect(isWide()).toBe(false);
    const crossed = vi.fn();
    const stop = watchCrossings(crossed);
    const resize = (width: number) => {
      vi.stubGlobal("innerWidth", width);
      page.dispatchEvent(new Event("resize"));
    };
    resize(600);
    expect(crossed).not.toHaveBeenCalled();
    resize(900);
    expect(isWide()).toBe(true);
    expect(crossed).toHaveBeenCalledTimes(1);
    resize(1400);
    expect(crossed).toHaveBeenCalledTimes(1);
    resize(899.5);
    expect(crossed).toHaveBeenCalledTimes(2);
    stop();
    resize(1000);
    expect(crossed).toHaveBeenCalledTimes(2);
  });

  it("puts its style in the page once, before the project's theme", () => {
    type Made = { tag: string; id: string; textContent: unknown };
    const theme: Made = { tag: "style", id: "", textContent: "theme" };
    const head: Made[] = [theme];
    vi.stubGlobal("document", {
      getElementById: (id: string) => head.find((element) => element.id === id) ?? null,
      createElement: (tag: string): Made => ({ tag, id: "", textContent: null }),
      head: {
        querySelector: (selector: string) => (selector === "[data-symphony-theme]" ? theme : null),
        insertBefore: (element: Made, before: Made | null) =>
          head.splice(before === null ? head.length : head.indexOf(before), 0, element),
      },
    });
    putStyle();
    putStyle();
    expect(head).toHaveLength(2);
    expect(head[0]).toMatchObject({ tag: "style", id: "timeline-style" });
    expect(head[1]).toBe(theme);
    // Its text is the sheet's, which Vitest empties.
    expect(typeof head[0]?.textContent).toBe("string");
  });

  it("draws a clipped list from its end, all of it once asked, and neither else", () => {
    expect(drawnClass(false, false)).toBe("tl-drawn");
    expect(drawnClass(false, true)).toBe("tl-drawn tl-clipped");
    expect(drawnClass(true, false)).toBe("tl-drawn tl-all");
    expect(drawnClass(true, true)).toBe("tl-drawn tl-all");
  });
});

describe("the lanes' keyboard", () => {
  it("walks the parts by the arrow keys, Home and End, and stops at their ends", () => {
    const walked = ["ArrowRight", "ArrowDown", "ArrowLeft", "ArrowUp", "Home", "End", "Tab", "a"];
    expect(walked.map((key) => stopFor(key, 3, 10))).toEqual([4, 4, 2, 2, 0, 9, null, null]);
    expect(stopFor("ArrowRight", 9, 10)).toBe(9);
    expect(stopFor("ArrowLeft", 0, 10)).toBe(0);
    // No parts: every key that walks stays at the first.
    expect(["ArrowRight", "End", "Home"].map((key) => stopFor(key, 0, 0))).toEqual([0, 0, 0]);
  });

  it("a part that takes the focus is the stop and its words the detail, which leave with it", () => {
    vi.stubGlobal("document", { getElementById: () => null });
    const choose = vi.fn();
    const tell = vi.fn();
    const at = stopsOf(2, choose, tell);
    expect(at.active).toBe(2);
    at.focus(5, "review 52 m");
    expect(choose).toHaveBeenCalledWith(5);
    expect(tell).toHaveBeenCalledWith("review 52 m");
    at.blur();
    expect(tell).toHaveBeenLastCalledWith("");
  });

  it("a key that walks moves the stop and the focus to the part it names, and no other does", () => {
    const focused = vi.fn();
    const asked: string[] = [];
    vi.stubGlobal("document", {
      getElementById: (id: string) => {
        asked.push(id);
        return id === "tl-stop-4" ? { focus: focused } : null;
      },
    });
    const choose = vi.fn();
    const at = stopsOf(3, choose, vi.fn());
    const press = (key: string) => {
      const event = { key, preventDefault: vi.fn() };
      at.key(event as unknown as KeyboardEvent<HTMLElement>, 3, 10);
      return event.preventDefault.mock.calls.length;
    };
    expect(press("ArrowRight")).toBe(1);
    expect(choose).toHaveBeenLastCalledWith(4);
    expect(focused).toHaveBeenCalledTimes(1);
    expect(press("Enter")).toBe(0);
    expect(choose).toHaveBeenCalledTimes(1);
    // A part not in the page takes no focus, and the stop still moves.
    expect(press("End")).toBe(1);
    expect(choose).toHaveBeenLastCalledWith(9);
    expect(asked).toEqual(["tl-stop-4", "tl-stop-9"]);
  });
});
