import { isValidElement, type KeyboardEvent, type ReactElement, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import FIXTURE from "../../../crates/symphony-api/fixtures/task-timeline.json";
import GUARDS from "../../symphony-web-tests/tests/browser/timeline/revision-guards.json";
import { whenShown } from "./feed";
import type { Shown } from "./parts";
import { Failure, NO_ANSWER as POST_UNANSWERED } from "./request";
import { CASES } from "./testing";
import {
  drawnClass,
  isWide,
  measured,
  NO_ANSWER,
  NO_TIMELINE,
  NOT_A_TIMELINE,
  putStyle,
  readAgain,
  readFailed,
  type Stops,
  settle,
  shownOf,
  stopFor,
  stopsOf,
  TimelineDrawn,
  TimelineFailed,
  TimelineHost,
  TimelineView,
  watchCrossings,
} from "./timeline";
import { readTaskTokens, totalWords } from "./timeline-cost";
import { BAND_SHARE } from "./timeline-cut";
import {
  failed,
  lanes,
  nextRead,
  readTimeline,
  stops,
  type View,
  WIDTH_RULES,
} from "./timeline-layout";
import {
  type Heard,
  handoff,
  keyView,
  OverviewStrip,
  quietCount,
  sayAgain,
  ZoomBar,
} from "./timeline-navigate";
import { quietStretches, SKIPPING, type Skipping } from "./timeline-quiet";
import { LEAST_SHOWN, leastShown, wholeOf } from "./timeline-zoom";
import { NOT_IN_TIME } from "./trouble";
import type { TaskTimeline, TaskTokens, TimelineMessage } from "./wire";

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
  return { active, focus: vi.fn(), say: vi.fn(), blur: vi.fn(), key: vi.fn() };
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
function press(element: ReactElement<Props> | undefined, event?: unknown): void {
  const onClick = element?.props.onClick;
  if (typeof onClick !== "function") throw new Error("nothing to press");
  onClick(event);
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

/** The timeline's part before its page's store has anything of it. */
const NOTHING: Shown<unknown> = {
  value: undefined,
  stale: false,
  error: null,
  retrying: false,
  missing: false,
  answered: false,
};
/** The part answered with `value`, the server's JSON as it came. */
const answered = (value: unknown): Shown<unknown> => ({ ...NOTHING, value, answered: true });
/** The fixture as the server sends it: JSON the chunk reads as it draws. */
const sent = () => JSON.parse(JSON.stringify(FIXTURE)) as unknown;
const noRead = () => Promise.resolve();

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
    // The builds and the tests, each a lane of its own.
    for (const lane of ["stage", "executor", "reviewer", "builds", "tests"]) {
      expect(drawn).toContain(`data-lane="${lane}"`);
    }
    // Each runs lane's name says its counts.
    expect(drawn).toContain("<b>Builds</b><small>1 run · 0 red</small>");
    expect(drawn).toContain("<b>Tests</b><small>4 runs · 0 red</small>");
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
    // The keyboard stops at the first part, its words read aloud; the rest by the arrow keys. Each
    // of the five runs' rows in the list takes a tap's focus, in no tab order.
    expect(count(drawn, 'tabindex="0"')).toBe(1);
    expect(count(drawn, 'tabindex="-1"')).toBe(stops(lanes(pictures())) - 1 + 5);
    expect(count(drawn, 'class="tl-row tl-sub" style="--c:var(--like)" title="')).toBe(4);
    expect(drawn).toContain(
      'id="tl-stop-0" tabindex="0" role="img" aria-label="ready 41 m, from 09:02 to 09:43"',
    );
    // A run's part in its lane and its row in the list say its command, its wait and its run
    // time, apart: the row under its bar too, which the style sheet shows while it has the focus.
    const verify =
      "Test, just verify · waited 6 m · ran 24 m · green · at 9ac5149 by quiet-wren on studio";
    expect(drawn).toContain(`title="${verify}" tabindex="-1"><span class="tl-at">10:52</span>`);
    expect(drawn).toContain(`<span class="tl-facts" aria-hidden="true">${verify}</span></li>`);
    expect(count(drawn, 'class="tl-facts"')).toBe(5);
    expect(drawn).toContain(`title="${verify}" id="tl-stop-`);
    expect(drawn).toContain(`role="img" aria-label="${verify}"`);
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

  it("tells a custom workflow's phases and states apart from the machine's time, and none without", () => {
    const made = pictures().created;
    const place = (hours: number, phase: string, state: string, status: string) => ({
      phase,
      phase_label: phase === "build" ? "Build" : "Review",
      state,
      state_label: state === "working" ? "Working" : "Awaiting review",
      status,
      from: made + hours * 3600,
      by: "owner",
    });
    const held = {
      ...pictures(),
      places: [
        place(0, "build", "working", "in_progress"),
        place(5, "review", "awaiting", "review"),
      ],
    };
    const drawn = html(held);
    expect(drawn).toContain('<p class="tl-totals">Phases: Build 5 h · Review 1 h 18 m</p>');
    expect(drawn).toContain(
      '<p class="tl-totals">States: Working 5 h · Awaiting review 1 h 18 m</p>',
    );
    expect(drawn).toContain(">Machine 1 h 33 m, queued 26 m<");
    expect(drawn).toContain('data-lane="phase"');
    expect(html(pictures())).not.toContain("Phases:");
    expect(html(pictures())).not.toContain('data-lane="phase"');
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
    // A verdict starts past its bar as drawn, and shows where its letters fit.
    expect(drawn).toContain(
      '<span class="tl-after tl-fit tl-from-400" aria-hidden="true" style="--c:var(--dislike);left:54.47%;max-width:18.42%;padding-left:max(6px, calc(20px - 8.95%))">changes</span>',
    );
    // A run's words inside its bar, at the widths where it is wide enough.
    expect(drawn).toMatch(
      /class="tl-seg" style="--c:var\(--like\);--row:0;left:[0-9.]+%;width:[0-9.]+%" title="Test, just verify · waited 6 m · ran 24 m[^"]*"[^>]*><span class="tl-fit tl-from-[0-9]+">verify · 24 m<\/span><\/span>/,
    );
    expect(drawn).not.toContain("tl-under");
    expect(drawn).toContain("max-width:calc(");
    expect(drawn).toContain(
      '<li><span class="tl-key tl-key-work"></span>worked or ran</li><li><span class="tl-key tl-key-wait"></span>queued or waiting</li><li><span class="tl-key tl-key-running"></span>running now</li><li class="tl-wide"><span class="tl-key tl-g tl-g-met"></span>gate met</li>',
    );
    expect(
      drawn.endsWith(
        '</ul></div></div><p class="tl-note">Role and address history unavailable.</p>',
      ),
    ).toBe(true);
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
    for (const part of ["achine", 'data-lane="builds"', 'data-lane="tests"', "tl-running"]) {
      expect(drawn).not.toContain(part);
    }
    expect(drawn).not.toContain("running now");
    // No row in the list takes a tap's focus or says more under its bar: only a run's does.
    expect(count(drawn, 'tabindex="-1"')).toBe(stops(lanes({ ...pictures(), runs: [] })) - 1);
    expect(drawn).not.toContain("tl-facts");
  });

  it("a task of builds alone draws a builds lane and no tests lane, and the other way round", () => {
    const runs = pictures().runs;
    const builds = html({ ...pictures(), runs: runs.filter((run) => run.phase === "build") });
    expect(builds).toContain('data-lane="builds"');
    expect(builds).not.toContain('data-lane="tests"');
    const tests = html({ ...pictures(), runs: runs.filter((run) => run.phase !== "build") });
    expect(tests).toContain('data-lane="tests"');
    expect(tests).not.toContain('data-lane="builds"');
  });

  it("runs that overlap take a row each, and a lane with no message has no line", () => {
    // The fixture's verify, run again a minute apart from 10:52, each going on until the last has
    // begun: each takes a row of its own.
    const verify = pictures().runs[1];
    if (verify === undefined) throw new Error("the fixture has no verify");
    const again = Array.from({ length: 7 }, (_, at) => ({
      ...verify,
      queued: verify.queued + at * 60,
      started: verify.queued + at * 60 + 1,
      ended: verify.queued + 600,
    }));
    const drawn = html({
      ...pictures(),
      runs: [pictures().runs[0], ...again] as (typeof verify)[],
    });
    expect(drawn).toContain('<div class="tl-lane" data-lane="tests" style="--rows:7">');
    expect(drawn).toContain('<div class="tl-lane" data-lane="builds" style="--rows:1">');
    expect(drawn).not.toContain("data-rail");
    expect(drawn).not.toContain("tl-rail");
    expect(drawn).not.toContain("tl-notkept");
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
      '<button class="dx-button tl-more" data-style="outline" data-size="default" type="button" aria-expanded="false">Show all 14 steps</button>',
    );
    // The time no run is kept of is a row at its end in the list, and hatched over the runs
    // lanes, in place of a sentence over them.
    expect(clipped).not.toContain("no longer kept");
    expect(clipped).toContain(
      '<li class="tl-row tl-sub tl-event tl-kept" style="--c:var(--muted-foreground)" title="Builds and tests before 11-11 10:25 are not kept"><span class="tl-at">10:25</span><span class="tl-mark" aria-hidden="true"></span><span class="tl-label">Builds and tests kept from here on</span></li>',
    );
    expect(
      count(
        clipped,
        '<span class="tl-notkept" aria-hidden="true" title="Builds and tests before 11-11 10:25 are not kept" style="left:0.53%;width:21.84%"><span class="tl-fit',
      ),
    ).toBe(2);
    expect(clipped).toContain(">not kept before 11-11 10:25</span>");
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

  it("a run's row in the list takes a tap's focus and holds all of the run; a stage's neither", () => {
    const at = spiedStops();
    const tree = TimelineDrawn({
      timeline: pictures(),
      all: false,
      clipped: false,
      detail: "",
      stops: at,
      toggle: () => {},
    });
    const rows = elements(tree, (one) => one.type === "li" && one.props.title !== undefined);
    const focusable = rows.filter((row) => row.props.tabIndex === -1);
    // The five runs, of thirteen rows; no gate change and no stage.
    expect([rows.length, focusable.length]).toEqual([13, 5]);
    expect(focusable.every((row) => row.props.className === "tl-row tl-sub")).toBe(true);
    expect(rows.filter((row) => row.props.tabIndex !== undefined)).toEqual(focusable);
    // Each holds its words, its title's, the last thing in it; no other row holds any.
    const facts = (row: ReactElement<Props>) =>
      elements(row.props.children, (one) => one.props.className === "tl-facts");
    expect(focusable.map((row) => facts(row)[0]?.props.children)).toEqual(
      focusable.map((row) => row.props.title),
    );
    expect(focusable[0]?.props.title).toBe(
      "Build, just lint-rs · waited 3 m · ran 4 m · green · at 9ac5149 by quiet-wren on studio",
    );
    expect(rows.filter((row) => facts(row).length > 0)).toEqual(focusable);
    // A row's focus is the style sheet's: it moves no stop and tells no detail.
    expect(focusable.every((row) => row.props.onFocus === undefined)).toBe(true);
  });
});

describe("the timeline's cost", () => {
  /** The wire's sample tokens as the pictures' task's own: its executor's first work, and its
   * first review's reviewer. */
  function tokens(): TaskTokens {
    const read = readTaskTokens(CASES.samples.taskTokens) as TaskTokens;
    const { slug, stages } = pictures();
    const rows = read.rows.map((row, at) => ({
      ...row,
      slug,
      first: (stages[at + 1]?.from ?? 0) + 60,
      last: (stages[at + 1]?.from ?? 0) + 600,
    }));
    return { ...read, rows };
  }
  const drawnWith = (spent: TaskTokens | null | undefined, at = spiedStops()) =>
    TimelineDrawn({
      timeline: pictures(),
      tokens: spent,
      all: false,
      clipped: false,
      detail: "",
      stops: at,
      toggle: () => {},
    });

  it("follows the task's time on the first line, and says the rest as its words", () => {
    const words = totalWords(tokens());
    const drawn = renderToStaticMarkup(drawnWith(tokens()));
    expect(drawn).toContain(
      '<p class="tl-totals"><span>6 h 18 m so far <span class="tl-cost" role="img" tabindex="0" ' +
        `title="${words}" aria-label="${words}">$3.00+</span><span class="tl-rest tl-wide"> · 2 ` +
        "review rounds · 1 bounce</span></span>" +
        // A phone's second line: the rounds and bounces the first has no room for, then the
        // machine's time; wide, the machine's follows the rounds on the one line.
        '<span class="tl-machine tl-narrow">2 review rounds · 1 bounce · machine 1 h 33 m</span>' +
        '<span class="tl-machine tl-wide"> · machine 1 h 33 m, queued 26 m</span></p>',
    );
    const noRuns = renderToStaticMarkup(
      TimelineDrawn({
        timeline: { ...pictures(), runs: [] },
        tokens: tokens(),
        all: false,
        clipped: false,
        detail: "",
        stops: spiedStops(),
        toggle: () => {},
      }),
    );
    expect(noRuns).toContain(
      '<span class="tl-machine tl-narrow">2 review rounds · 1 bounce</span></p>',
    );
    expect(words).toContain("estimated at API list prices read 2026-11-16, not money spent");
    expect(drawn).not.toContain("estimated</span>");
    // Its words under the totals, which the style sheet shows a phone while it has the focus.
    expect(drawn).toContain(`<p class="tl-note tl-cost-said" aria-hidden="true">${words}</p>`);
    // The total is a tab stop beside the lanes' one.
    expect(count(drawn, 'tabindex="0"')).toBe(2);
  });

  it("says its words as the detail while it has the focus, which leave with it", () => {
    const at = spiedStops();
    const [cost] = elements(drawnWith(tokens(), at), (one) => one.props.className === "tl-cost");
    const props = cost?.props as { onFocus(): void; onBlur(): void };
    props.onFocus();
    expect(at.say).toHaveBeenCalledWith(totalWords(tokens()));
    expect(at.focus).not.toHaveBeenCalled();
    props.onBlur();
    expect(at.blur).toHaveBeenCalledTimes(1);
  });

  it("follows each stage's time in its row and its lane, the row taking a tap's focus", () => {
    const tree = drawnWith(tokens());
    const drawn = renderToStaticMarkup(tree);
    expect(drawn).toContain('<span class="tl-took">1 h 52 m $2.00+</span>');
    expect(drawn).toContain('<span class="tl-took">52 m $1.00</span>');
    expect(drawn).toContain('<span class="tl-took">41 m</span>');
    expect(drawn).toContain(">in progress 1 h 52 m $2.00+</span>");
    expect(drawn).toContain(">1 h 52 m $2.00+</span>");
    // The two stages that carry a cost take a tap's focus as the five runs do, and say their
    // words under their bar; no other stage does.
    const rows = elements(tree, (one) => one.type === "li" && one.props.title !== undefined);
    const focusable = rows.filter((row) => row.props.tabIndex === -1);
    expect(focusable.map((row) => row.props.className)).toEqual([
      "tl-row",
      "tl-row tl-sub",
      "tl-row tl-sub",
      "tl-row",
      "tl-row tl-sub",
      "tl-row tl-sub",
      "tl-row tl-sub",
    ]);
    expect(focusable[0]?.props.title).toBe(
      "In progress · quiet-wren: from 09:43 to 11:35, moved there by quiet-wren: " +
        "executor $2.00+, 09:44–09:53, in 4 calls; " +
        "estimated at API list prices read 2026-11-16, not money spent",
    );
    expect(count(drawn, 'class="tl-facts"')).toBe(7);
    expect(drawn).toContain(
      `<span class="tl-facts" aria-hidden="true">${focusable[0]?.props.title}</span></li>`,
    );
  });

  it("is nowhere for a task with no tokens, or whose tokens are not read: drawn as it was", () => {
    const plain = renderToStaticMarkup(drawnWith(undefined));
    const none = readTaskTokens(CASES.samples.emptyTaskTokens) as TaskTokens;
    expect(renderToStaticMarkup(drawnWith(none))).toBe(plain);
    expect(renderToStaticMarkup(drawnWith(null))).toBe(plain);
    expect(plain).toContain("<span>6 h 18 m so far · 2 review rounds · 1 bounce</span>");
    expect(plain).not.toContain("tl-cost");
    expect(plain).not.toContain("$");
  });

  it("the host reads the tokens it is handed, and draws no cost of what it cannot read", () => {
    const part = answered(JSON.parse(JSON.stringify(FIXTURE)));
    const host = (handed: unknown) => {
      react.effects.length = 0;
      react.layouts.length = 0;
      return renderToStaticMarkup(
        <TimelineHost slug="a/b" part={part} tokens={handed} read={async () => {}} />,
      );
    };
    expect(host(tokens())).toContain('class="tl-cost"');
    expect(host(undefined)).not.toContain("tl-cost");
    expect(host(null)).not.toContain("tl-cost");
    expect(host({ total: "none" })).not.toContain("tl-cost");
  });
});

describe("the timeline's host", () => {
  it("before its first answer draws nothing, and the page's placeholder shows", () => {
    const drawn = renderToStaticMarkup(
      <TimelineHost slug="tools/new" part={NOTHING} read={noRead} />,
    );
    expect(drawn).not.toContain("tl-drawn");
    expect(drawn).not.toContain("Reading");
    expect(drawn).toBe("");
  });

  it("puts its style in the page, and asks for nothing by itself before its answer", async () => {
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
    const read = vi.fn(noRead);
    renderToStaticMarkup(<TimelineHost slug="tools/new" part={NOTHING} read={read} />);
    expect([react.layouts.length, react.effects.length]).toEqual([1, 1]);
    react.layouts[0]?.();
    expect(appended.map((element) => element.id)).toEqual(["timeline-style"]);
    // The page's store reads the part; the host only asks again once a timeline says how often.
    const leave = react.effects[0]?.();
    await Promise.resolve();
    expect(read).not.toHaveBeenCalled();
    leave?.();
  });

  it("asks for its part again each `every` seconds of an open task, until it leaves", async () => {
    vi.useFakeTimers();
    aPage();
    const read = vi.fn(noRead);
    const open = { ...(sent() as object), every: 45, closed: null };
    renderToStaticMarkup(<TimelineHost slug="tools/x" part={answered(open)} read={read} />);
    // The host's effect, before its view's.
    const leave = react.effects[0]?.();
    await vi.advanceTimersByTimeAsync(44_999);
    expect(read).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(read).toHaveBeenCalledTimes(1);
    leave?.();
    await vi.advanceTimersByTimeAsync(450_000);
    expect(read).toHaveBeenCalledTimes(1);
  });

  it("draws what its part shows: a failure with its Try again, else the timeline", () => {
    const failed = { ...NOTHING, error: new TypeError("Board snapshot unavailable. ") };
    expect(renderToStaticMarkup(<TimelineHost slug="tools/x" part={failed} read={noRead} />)).toBe(
      renderToStaticMarkup(
        <TimelineFailed error="Board snapshot unavailable. " retry={() => {}} />,
      ),
    );
    expect(
      renderToStaticMarkup(<TimelineHost slug="tools/x" part={answered(sent())} read={noRead} />),
    ).toBe(html(pictures()));
    // A later read that fails leaves the timeline shown.
    expect(
      renderToStaticMarkup(
        <TimelineHost
          slug="tools/x"
          part={{ ...answered(sent()), error: new TypeError("Failed to fetch") }}
          read={noRead}
        />,
      ),
    ).toBe(html(pictures()));
    expect(
      renderToStaticMarkup(
        <TimelineHost slug="tools/x" part={answered({ now: "soon" })} read={noRead} />,
      ),
    ).toBe(renderToStaticMarkup(<TimelineFailed error={NOT_A_TIMELINE} retry={() => {}} />));
  });

  it("a failed read with nothing shown says why in the box, with its Try again", () => {
    const retry = vi.fn();
    expect(renderToStaticMarkup(<TimelineFailed error="Failed to fetch" retry={retry} />)).toBe(
      '<div class="tl-drawn tl-failed"><p class="tl-note">The timeline could not be read: Failed to fetch. Trying again… <button class="dx-button" data-style="outline" data-size="default" type="button">Try again</button></p></div>',
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
    // The style sheet is put before the draw is measured, which it lays out.
    // Selection adds one layout effect for the phone row, plus attempt lifetime and thread-read effects;
    // a chosen message brought into view, and the zoomable lanes' wheel, one effect each.
    expect([react.layouts.length, react.effects.length]).toEqual([3, 5]);
    expect(react.layouts[0]).toBe(putStyle);
    // No element was drawn here: nothing to measure, nothing said.
    react.layouts[1]?.();
    expect(onDrawn).not.toHaveBeenCalled();
    const stop = react.effects[2]?.();
    expect(typeof stop).toBe("function");
    stop?.();
  });
});

describe("the timeline's reads", () => {
  it("says a failure in the server's or the browser's own words, a slow one in its own", () => {
    expect(
      readFailed(Failure.refused(503, "Not ready (the server answered 503)", "Not ready.")),
    ).toBe("Not ready.");
    expect(readFailed(new Failure("uncertain", NOT_IN_TIME))).toBe(NO_ANSWER);
    expect(NO_ANSWER).toBe("the server did not answer in time");
    // A post's words are no read's: said as they came.
    expect(readFailed(new Failure("uncertain", POST_UNANSWERED))).toBe(POST_UNANSWERED);
    expect(readFailed(new TypeError("Failed to fetch"))).toBe("Failed to fetch");
    expect(readFailed("offline")).toBe("offline");
  });

  it("shows nothing before an answer, why a read failed with none, and the answer once there", () => {
    expect(shownOf(NOTHING, null, 0)).toBeNull();
    // The server has no timeline for the task: said, and nothing tries it again.
    const none = shownOf({ ...NOTHING, answered: true, missing: true }, null, 0);
    expect(none).toEqual({ error: NO_TIMELINE, final: true });
    expect(nextRead(none)).toBeNull();
    expect(failed(NO_TIMELINE, false)).toBe(
      "The timeline could not be read: the board has no timeline for this task.",
    );
    expect(failed("", false)).toBe("The timeline could not be read.");
    expect(shownOf({ ...NOTHING, error: new TypeError("Failed to fetch") }, null, 0)).toEqual({
      error: "Failed to fetch",
    });
    expect(shownOf({ ...NOTHING, error: new Failure("uncertain", NOT_IN_TIME) }, null, 0)).toEqual({
      error: NO_ANSWER,
    });
    const value = sent();
    expect(shownOf(answered(value), null, 0)).toEqual({ timeline: pictures() });
    // A failure after a timeline leaves it shown.
    expect(shownOf({ ...answered(value), error: new Error("gone") }, null, 0)).toEqual({
      timeline: pictures(),
    });
    expect(shownOf(answered({ now: "soon" }), null, 0)).toEqual({ error: NOT_A_TIMELINE });
  });

  it("moves the answer kept on by this browser's clock since it was first drawn", () => {
    const value = sent();
    const timeline = pictures();
    const seen = { value, at: 5_000 };
    expect(shownOf(answered(value), seen, 5_000)).toEqual({ timeline });
    expect(shownOf(answered(value), seen, 50_000)).toEqual({
      timeline: { ...timeline, now: timeline.now + 45 },
    });
    // Moved from the clock of the answer, not of the last read.
    expect(shownOf(answered(value), seen, 65_000)).toEqual({
      timeline: { ...timeline, now: timeline.now + 60 },
    });
    // A clock before the answer's, or the clock of another answer, moves nothing.
    expect(shownOf(answered(value), seen, 0)).toEqual({ timeline });
    expect(shownOf(answered(sent()), seen, 50_000)).toEqual({ timeline });
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

  it("reads again each `every` seconds while open, and not while the page is hidden", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_000_000);
    const page = aPage();
    const read = vi.fn(noRead);
    const drawn = vi.fn();
    const reading = new AbortController();
    const done = readAgain(() => 45, read, drawn, reading.signal);
    await vi.advanceTimersByTimeAsync(44_999);
    expect(read).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(read).toHaveBeenCalledTimes(1);
    expect(drawn.mock.calls).toEqual([[1_045_000]]);
    // Hidden, the part is not read until the page shows again, then at once.
    page.hidden = true;
    await vi.advanceTimersByTimeAsync(10 * 45_000);
    expect(read).toHaveBeenCalledTimes(1);
    page.hidden = false;
    page.dispatchEvent(new Event("visibilitychange"));
    await flush();
    expect(read).toHaveBeenCalledTimes(2);
    // Left while it waits for its next read: it ends at once and reads nothing more.
    reading.abort();
    await done;
    await vi.advanceTimersByTimeAsync(120_000);
    expect(read).toHaveBeenCalledTimes(2);
  });

  it("reads nothing again of a closed task, and says no clock of a read its page left", async () => {
    vi.useFakeTimers();
    aPage();
    const read = vi.fn(noRead);
    const drawn = vi.fn();
    await readAgain(() => null, read, drawn, new AbortController().signal);
    await vi.advanceTimersByTimeAsync(3_600_000);
    expect(read).not.toHaveBeenCalled();
    // Left while a read is on its way: its answer moves no clock.
    const leaving = new AbortController();
    const out = readAgain(
      () => 1,
      () => {
        leaving.abort();
        return Promise.resolve();
      },
      drawn,
      leaving.signal,
    );
    await vi.advanceTimersByTimeAsync(1_000);
    await out;
    expect(drawn).not.toHaveBeenCalled();
    // A page already left reads nothing; one left while hidden reads nothing either.
    const gone = new AbortController();
    gone.abort();
    await readAgain(() => 1, read, drawn, gone.signal);
    expect(read).not.toHaveBeenCalled();
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
    // Its text is the sheet's, which Vitest empties, then the width steps, in the one element put
    // in the page in the layout effect, so they hold from the timeline's first paint.
    expect(head[0]?.textContent).toBe(WIDTH_RULES);
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

describe("the timeline zoomed", () => {
  /** The pictures' hours start at 09:00 on their clock: `minutes` after it. */
  const START = 1_794_380_400;
  const nine = (minutes: number) => START + minutes * 60;
  const NOON: View = { from: nine(180), to: nine(240) };
  const WHOLE: View = { from: START, to: nine(380) };

  /** The timeline drawn wide with `view`, set through `set`, or with no zoom at all. */
  function draw(
    timeline: TaskTimeline,
    zoom?: { view: View | null; set?: (v: View | null) => void },
  ) {
    return renderToStaticMarkup(
      <TimelineDrawn
        timeline={timeline}
        all={false}
        clipped={false}
        detail=""
        stops={spiedStops()}
        toggle={() => {}}
        zoom={zoom === undefined ? undefined : { view: zoom.view, set: zoom.set ?? (() => {}) }}
      />,
    );
  }

  it("a task too short to zoom into draws as it did, with no toolbar", () => {
    // Made half a minute after its hour and closed two minutes later: its hours are under five
    // minutes long.
    const brief: TaskTimeline = {
      ...pictures(),
      created: START + 30,
      now: START + 150,
      closed: START + 150,
      stages: [
        { status: "in_progress", from: START + 30, to: START + 150, by: "quiet-wren" },
        { status: "done", from: START + 150, to: null, by: "quiet-wren" },
      ],
      claims: [],
      rounds: [],
      runs: [],
      gates: [],
    };
    const before = draw(brief);
    expect(draw(brief, { view: null })).toBe(before);
    expect(draw(brief, { view: { from: START + 30, to: START + 90 } })).toBe(before);
    expect(before).not.toContain("tl-zoom");
    expect(before).toContain('<div class="tl-lanes" role="group" aria-label="Lanes">');
  });

  it("opens on the whole task: a toolbar line above the lanes, and no overview", () => {
    const drawn = draw(pictures(), { view: null });
    // Whole task and − cannot act there: not drawn, their places kept, + alone where it is when
    // zoomed.
    expect(drawn).toContain(
      '<div class="tl-zoom"><span class="tl-zoom-group" role="group" aria-label="Zoom"><button class="dx-button tl-idle" data-style="outline" data-size="sm" type="button" disabled="">Whole task</button><button class="dx-button tl-idle" data-style="outline" data-size="sm" type="button" aria-label="Zoom out" title="Zoom out" disabled="">−</button><button class="dx-button" data-style="outline" data-size="sm" type="button" aria-label="Zoom in" title="Zoom in">+</button></span><span class="tl-range">11-11 09:00 → 11-11 15:20 · 6 h 20 m</span><span class="tl-said" aria-live="polite"></span></div>',
    );
    expect(drawn).toContain(
      '<div class="tl-lanes tl-zoomable" id="tl-lanes" role="group" aria-label="Lanes">',
    );
    expect(drawn).not.toContain("tl-overview");
    expect(drawn).not.toContain("tl-out");
    // The toolbar is above the lanes, in the box's body.
    expect(drawn.indexOf("tl-zoom")).toBeLessThan(drawn.indexOf("tl-lanes"));
    // A stage's bar says its instants, which a double click zooms to; no other bar does.
    expect(count(drawn, "data-from=")).toBe(6);
    expect(drawn).toContain(`data-from="${pictures().created}" data-to="${nine(43)}"`);
    // Unzoomed lanes, or a task too short, say none.
    expect(draw(pictures())).not.toContain("data-from=");
  });

  it("zoomed, says the hours shown of the whole, draws the overview, and hides what is outside", () => {
    const drawn = draw(pictures(), { view: NOON });
    expect(drawn).toContain(
      '<span class="tl-range">11-11 12:00 → 11-11 13:00 · 1 h of 6 h 20 m</span>',
    );
    expect(drawn).toContain('type="button">Whole task</button>');
    expect(drawn).not.toContain("disabled");
    expect(drawn).not.toContain("tl-idle");
    expect(drawn).toContain('<div class="tl-overview">');
    expect(drawn).toContain(
      'class="tl-ov-window" role="scrollbar" tabindex="0" aria-label="Hours shown" aria-controls="tl-lanes" aria-orientation="horizontal" aria-valuemin="0" aria-valuemax="100" aria-valuenow="47" aria-valuetext="11-11 12:00 → 11-11 13:00 · 1 h of 6 h 20 m" style="left:47.37%;width:15.79%"',
    );
    // The overview sits between the toolbar and the lanes.
    expect(drawn.indexOf("tl-zoom")).toBeLessThan(drawn.indexOf("tl-overview"));
    expect(drawn.indexOf("tl-overview")).toBeLessThan(drawn.indexOf("tl-lanes"));
    // Every stop is still drawn, the ones outside the view unseen.
    const total = stops(lanes(pictures()));
    expect(count(drawn, 'id="tl-stop-')).toBe(total);
    expect(drawn).toContain('class="tl-seg tl-out"');
    expect(drawn).toContain('id="tl-stop-0"');
    expect(drawn).toContain("tl-g tl-g-missing tl-out");
    // The hours marked are the view's, every ten minutes.
    expect(drawn).toContain('<span class="tl-tick" style="left:0.00%">12:00</span>');
    expect(drawn).toContain('<span class="tl-tick" style="left:16.67%">12:10</span>');
    // Now is past the view: not marked.
    expect(drawn).not.toContain("tl-now");
  });

  it("at a zoom's least, + cannot zoom further and is not drawn, its place kept", () => {
    const least = draw(pictures(), { view: { from: nine(180), to: nine(180) + LEAST_SHOWN } });
    expect(least).toContain(
      '<button class="dx-button tl-idle" data-style="outline" data-size="sm" type="button" aria-label="Zoom in" title="Zoom in" disabled="">+</button>',
    );
    expect(count(least, "tl-idle")).toBe(1);
    expect(least).toContain("· 5 m of 6 h 20 m");
    expect([
      leastShown(null, WHOLE),
      leastShown(NOON, WHOLE),
      leastShown({ from: START, to: START + LEAST_SHOWN }, WHOLE),
    ]).toEqual([false, false, true]);
  });

  it("each of the toolbar's buttons sets the view it says", () => {
    const set = vi.fn();
    const buttons = (view: View | null) =>
      elements(ZoomBar({ timeline: pictures(), nav: { whole: WHOLE, view, set } }), (one) =>
        ["Whole task", "−", "+"].includes(one.props.children as string),
      );
    const [whole, out, inward] = buttons(NOON);
    press(whole, pressed(false).event);
    press(out, pressed(false).event);
    press(inward, pressed(false).event);
    // Each a press, its range said.
    expect(set.mock.calls).toEqual([
      [null, true],
      [{ from: nine(150), to: nine(270) }, true],
      [{ from: nine(195), to: nine(225) }, true],
    ]);
    // From the whole task, + zooms to its middle half.
    set.mockClear();
    press(buttons(null)[2], pressed(false).event);
    expect(set.mock.calls).toEqual([[{ from: nine(95), to: nine(285) }, true]]);
  });

  it("says a range again where it is the one said before it", () => {
    const first = sayAgain(undefined, "11-11 12:00 → 11-11 13:00 · 1 h of 6 h 20 m");
    const again = sayAgain(first, first.text);
    expect([first, again]).toEqual([
      { text: first.text, times: 1 },
      { text: first.text, times: 2 },
    ]);
    expect(sayAgain(again, "11-11 09:00 → 11-11 15:20 · 6 h 20 m").times).toBe(3);
    /** What the readout's live region holds once `said` is said. */
    const region = (said?: Heard) =>
      elements(
        ZoomBar({ timeline: pictures(), nav: { whole: WHOLE, view: NOON, set: vi.fn(), said } }),
        (one) => one.props.className === "tl-said",
      )[0]?.props.children as ReactElement<Props> | undefined;
    // Nothing said yet: the region is empty.
    expect(region()).toBeFalsy();
    // Each range said is drawn anew, the same words under a key of its own, so a reader hears it.
    const [once, twice] = [region(first), region(again)];
    expect([once?.key, once?.props.children]).toEqual(["1", first.text]);
    expect([twice?.key, twice?.props.children]).toEqual(["2", first.text]);
  });

  /** A press's event on a toolbar button with the focus or not, and the buttons it focused. */
  function pressed(focused: boolean) {
    const focus: string[] = [];
    const button: { ownerDocument: { activeElement: unknown }; parentElement: unknown } = {
      ownerDocument: { activeElement: null },
      parentElement: {
        querySelector: (selector: string) => ({ focus: () => focus.push(selector) }),
      },
    };
    if (focused) button.ownerDocument.activeElement = button;
    return { event: { currentTarget: button }, focus };
  }

  it("hands the focus on from a button that hides itself to the one that now acts", () => {
    const set = vi.fn();
    const buttons = (view: View | null) =>
      elements(ZoomBar({ timeline: pictures(), nav: { whole: WHOLE, view, set } }), (one) =>
        ["Whole task", "−", "+"].includes(one.props.children as string),
      );
    /** The buttons the press of the `at`th button with the focus, on `view`, focused. */
    const focused = (view: View, at: number) => {
      const click = pressed(true);
      press(buttons(view)[at], click.event);
      return click.focus;
    };
    const ZOOM_IN = ['[aria-label="Zoom in"]'];
    // Whole task always ends on the whole task: to +.
    expect(focused(NOON, 0)).toEqual(ZOOM_IN);
    // − to the whole task: to +; short of it, it stays.
    expect(focused({ from: nine(90), to: nine(290) }, 1)).toEqual(ZOOM_IN);
    expect(focused(NOON, 1)).toEqual([]);
    // + to the least: to −; short of it, it stays.
    expect(focused({ from: nine(180), to: nine(188) }, 2)).toEqual(['[aria-label="Zoom out"]']);
    expect(focused(NOON, 2)).toEqual([]);
    // A press with the focus elsewhere (a mouse's, where it takes none) moves no focus.
    const elsewhere = pressed(false);
    press(buttons(NOON)[0], elsewhere.event);
    expect(elsewhere.focus).toEqual([]);
    expect(set).toHaveBeenCalledTimes(6);
    expect([
      handoff("whole", null, WHOLE),
      handoff("out", null, WHOLE),
      handoff("out", NOON, WHOLE),
      handoff("in", { from: START, to: START + LEAST_SHOWN }, WHOLE),
      handoff("in", NOON, WHOLE),
    ]).toEqual(["in", "in", null, "out", null]);
  });

  it("+ and − zoom and 0 shows the whole task; a modifier's key is the browser's", () => {
    expect(keyView("+", false, NOON, WHOLE)).toEqual({ view: { from: nine(195), to: nine(225) } });
    expect(keyView("=", false, NOON, WHOLE)).toEqual(keyView("+", false, NOON, WHOLE));
    expect(keyView("-", false, NOON, WHOLE)).toEqual({ view: { from: nine(150), to: nine(270) } });
    expect(keyView("_", false, NOON, WHOLE)).toEqual(keyView("-", false, NOON, WHOLE));
    expect(keyView("−", false, NOON, WHOLE)).toEqual(keyView("-", false, NOON, WHOLE));
    expect(keyView("0", false, NOON, WHOLE)).toEqual({ view: null });
    expect(keyView("-", false, null, WHOLE)).toEqual({ view: null });
    expect(keyView("+", true, NOON, WHOLE)).toBeNull();
    expect(keyView("0", true, NOON, WHOLE)).toBeNull();
    expect(keyView("ArrowRight", false, NOON, WHOLE)).toBeNull();
    expect(keyView("a", false, NOON, WHOLE)).toBeNull();
  });

  it("the overview draws the whole task's stages and runs, the outside dimmed", () => {
    const strip = renderToStaticMarkup(
      <OverviewStrip
        timeline={pictures()}
        view={NOON}
        nav={{ whole: WHOLE, view: NOON, set: () => {} }}
      />,
    );
    expect(count(strip, 'class="tl-ov-stage"')).toBe(6);
    expect(count(strip, 'class="tl-ov-run"')).toBe(5);
    expect(strip).toContain(
      '<span class="tl-ov-dim" aria-hidden="true" style="left:0;width:47.37%"></span><span class="tl-ov-dim" aria-hidden="true" style="left:63.16%;right:0"></span>',
    );
    expect(strip).toContain(
      '<span class="tl-ov-edge tl-ov-start" data-hold="start" aria-hidden="true"></span><span class="tl-ov-edge tl-ov-end" data-hold="end" aria-hidden="true"></span>',
    );
    expect(wholeOf(pictures())).toEqual(WHOLE);
  });

  it("a message chosen outside the hours shown has no arrow nor address drawn; its tick is unseen", () => {
    const message = (id: number, at: number): TimelineMessage => ({
      id,
      at,
      sender: "quiet-wren",
      recipient: "@orchestrator",
      kind: "msg",
      from: { state: "known", candidates: [{ role: "executor", by: "quiet-wren", source: "t" }] },
      to: {
        state: "known",
        candidates: [{ role: "orchestrator", by: "glacial-merlin", source: "t" }],
      },
      after_close: false,
    });
    const messages = [message(7, nine(100)), message(8, nine(200))];
    const timeline: TaskTimeline = {
      ...pictures(),
      history: { sources_complete: true, sources: [], roles: [], messages },
    };
    const drawWith = (selected: number) =>
      renderToStaticMarkup(
        <TimelineDrawn
          timeline={timeline}
          all={false}
          clipped={false}
          detail=""
          stops={spiedStops()}
          toggle={() => {}}
          selection={{ messages, selected, choose: () => {} }}
          zoom={{ view: NOON, set: () => {} }}
        />,
      );
    const outside = drawWith(7);
    expect(outside).toContain('class="tl-message-mark tl-out" data-message="7"');
    expect(outside).toContain('class="tl-message-mark" data-message="8"');
    expect(outside).not.toContain("tl-address-said");
    expect(outside).not.toContain("tl-address-track");
    const inside = drawWith(8);
    expect(inside).toContain("tl-address-said");
    expect(inside).toContain("tl-address-track");
  });
});

describe("the quiet stretches skipped", () => {
  /** The card's quiet stretch's ends, as the server answers them. */
  const QUIET_FROM = 1795093413.1;
  const QUIET_TO = 1795132641.4;

  /** The card's task, quiet from 14:03 on 11-19 to 00:57 the next day. */
  function guards(): TaskTimeline {
    const read = readTimeline(JSON.parse(JSON.stringify(GUARDS)));
    if (read === null) throw new Error("the card's task is no timeline");
    return read;
  }

  /** `timeline` drawn wide on the whole task, its quiet stretches skipped as `skipping`; with no
   * way to change what is skipped where `skip` is false. */
  function draw(timeline: TaskTimeline, skipping: Skipping = SKIPPING, skip = true) {
    return renderToStaticMarkup(
      <TimelineDrawn
        timeline={timeline}
        all={false}
        clipped={false}
        detail=""
        stops={spiedStops()}
        toggle={() => {}}
        zoom={{
          view: null,
          set: () => {},
          quiet: quietStretches(timeline),
          skipping,
          skip: skip ? () => {} : undefined,
        }}
      />,
    );
  }

  it("draws a band that says its length, its day after it and why, the switch on with its count", () => {
    const drawn = draw(guards());
    expect(drawn).toContain(
      '<button class="dx-button tl-skip" data-style="ghost" data-size="sm" type="button" aria-pressed="true"><span class="tl-skip-box" aria-hidden="true"></span>Skip quiet stretches<span class="tl-skip-count">1 · 10 h 54 m</span></button>',
    );
    expect(drawn).toContain(
      '<span class="tl-range">11-19 01:00 → 11-20 04:34 · 1 d 3 h · 10 h 54 m skipped</span>',
    );
    const title = "10 h 54 m quiet, 11-19 14:03 → 11-20 00:57, task done, nothing recorded";
    expect(count(drawn, 'class="tl-break"')).toBe(1);
    expect(drawn).toContain(
      `<button type="button" class="tl-break" data-start="${QUIET_FROM}" data-end="${QUIET_TO}" title="${title}" aria-label="Show on the true clock: ${title}" style="left:75.68%;width:3.30%">`,
    );
    expect(count(drawn, '<svg class="tl-edge')).toBe(2);
    expect(drawn).toContain("quiet · task done, nothing recorded");
    expect(drawn).toContain(
      '<span class="tl-break-length tl-fit" style="left:77.33%">+10 h 54 m<span class="tl-break-day tl-fit">11-20</span></span>',
    );
    // The legend says what a band is, wide alone.
    expect(drawn).toContain(
      '<li class="tl-wide"><span class="tl-key tl-key-quiet"></span>quiet stretch skipped: nothing recorded</li>',
    );
    // The bands are over the tracks, after the hours' lines.
    expect(drawn.indexOf('class="tl-hours"')).toBeLessThan(drawn.indexOf('class="tl-breaks"'));
    // The phone's list says the stretch as a row of its own.
    expect(drawn).toContain(
      '<li class="tl-row tl-quiet" title="10 h 54 m quiet, 14:03 → 11-20 00:57: task done, nothing recorded"><b>10 h 54 m quiet, 14:03 → 11-20 00:57</b><small>Task done, nothing recorded</small></li>',
    );
  });

  it("marks each band on the overview, dashed at its share", () => {
    const timeline = guards();
    const whole = wholeOf(timeline);
    const cut = { skipped: quietStretches(timeline), share: BAND_SHARE };
    const view = { from: whole.from, to: whole.from + 3600 };
    const strip = renderToStaticMarkup(
      <OverviewStrip timeline={timeline} view={view} nav={{ whole, view, cut, set: () => {} }} />,
    );
    expect(strip).toContain(
      '<span class="tl-ov-break" aria-hidden="true" style="left:75.68%;width:3.30%"></span>',
    );
    const plain = renderToStaticMarkup(
      <OverviewStrip timeline={timeline} view={view} nav={{ whole, view, set: () => {} }} />,
    );
    expect(plain).not.toContain("tl-ov-break");
  });

  it("switched off, draws the true clock, the switch still there to turn it on", () => {
    const drawn = draw(guards(), { on: false, opened: [] });
    expect(drawn).toContain(
      'type="button" aria-pressed="false"><span class="tl-skip-box" aria-hidden="true"></span>Skip quiet stretches',
    );
    expect(drawn).not.toContain('class="tl-break"');
    expect(drawn).not.toContain("tl-key-quiet");
    expect(drawn).toContain('<span class="tl-range">11-19 01:00 → 11-20 04:34 · 1 d 3 h</span>');
    // Its row stays in the phone's list: the list is the true clock always.
    expect(drawn).toContain('class="tl-row tl-quiet"');
  });

  it("draws no band and no switch for a task with no quiet stretch, nor where neither can act", () => {
    const none = draw(pictures());
    expect(none).not.toContain("tl-skip");
    expect(none).not.toContain("tl-break");
    expect(none).not.toContain("tl-quiet");
    expect(none).not.toContain("skipped");
    const fixed = draw(guards(), SKIPPING, false);
    expect(fixed).not.toContain("tl-skip");
    expect(fixed).not.toContain("tl-break");
    // The row says what happened whatever the lanes draw.
    expect(fixed).toContain("tl-quiet");
  });

  it("the switch's press says what it does: every stretch on the true clock, or all skipped", () => {
    const press_ = vi.fn();
    const timeline = guards();
    const [button] = elements(
      ZoomBar({
        timeline,
        nav: { whole: wholeOf(timeline), view: null, set: () => {} },
        quiet: { count: 1, total: 39228, on: true, press: press_ },
      }),
      (one) => one.props.className === "tl-skip",
    );
    expect(button?.props["aria-pressed"]).toBe(true);
    press(button);
    expect(press_).toHaveBeenCalledTimes(1);
    expect([quietCount(1, 39228), quietCount(3, 90_000)]).toEqual(["1 · 10 h 54 m", "3 · 1 d 1 h"]);
  });
});
