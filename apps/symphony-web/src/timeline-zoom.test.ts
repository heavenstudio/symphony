import { describe, expect, it } from "vitest";
import FIXTURE from "../../../crates/symphony-api/fixtures/task-timeline.json";
import GUARDS from "../../symphony-web-tests/tests/browser/timeline/revision-guards.json";
import { BAND_SHARE, Cuts } from "./timeline-cut";
import { readTimeline, type View } from "./timeline-layout";
import { quietStretches } from "./timeline-quiet";
import {
  clampView,
  dragged,
  fractionOn,
  gesture,
  held,
  LEAST_SHOWN,
  leastShown,
  moved,
  overview,
  pinched,
  readout,
  revealed,
  STEP,
  sideways,
  swiped,
  type Track,
  type Wheel,
  wheeled,
  wheelFactor,
  wheelPixels,
  wholeOf,
  windowKey,
  zoomable,
  zoomed,
  zoomTo,
} from "./timeline-zoom";
import type { TaskTimeline } from "./wire";

/** The quiet stretches of `timeline` skipped, as its page opens. */
const CUT_OF = (timeline: TaskTimeline) => ({
  skipped: quietStretches(timeline),
  share: BAND_SHARE,
});

/** The approved pictures' task, as the server answers it: made at 09:02 on its clock (+2 h), now
 * 15:20, so its hours are drawn from 09:00, 6 h 20 m. */
function pictures(): TaskTimeline {
  const read = readTimeline(JSON.parse(JSON.stringify(FIXTURE)));
  if (read === null) throw new Error("the fixture is no timeline");
  return read;
}

/** The pictures' hours: 09:00 to 15:20 on their clock. */
const START = 1_794_380_400;
const END = 1_794_403_200;
const WHOLE: View = { from: START, to: END };
const HOUR = 3600;

/** `minutes` after 09:00 on the pictures' clock. */
const at = (minutes: number) => START + minutes * 60;

/** A view from `from` to `to` minutes after 09:00. */
const minutes = (from: number, to: number): View => ({ from: at(from), to: at(to) });

/** A view's length in seconds. */
const length = (view: View | null) => (view === null ? null : view.to - view.from);

describe("the hours a zoom shows", () => {
  it("the whole hours are the lanes' own, from the hour the task was made in", () => {
    expect(wholeOf(pictures())).toEqual(WHOLE);
    // A task shorter than a minute is drawn over one, as the lanes draw it.
    const brief = { ...pictures(), created: START + 10, now: START + 20, stages: [], runs: [] };
    expect(wholeOf({ ...brief, gates: [], rounds: [], claims: [] })).toEqual({
      from: START,
      to: START + 60,
    });
  });

  it("a task no longer than a zoom's least is drawn whole: nothing to zoom into", () => {
    expect(zoomable(WHOLE)).toBe(true);
    expect(zoomable({ from: 0, to: LEAST_SHOWN })).toBe(false);
    expect(zoomable({ from: 0, to: LEAST_SHOWN + 1 })).toBe(true);
    expect(zoomable({ from: 0, to: 60 })).toBe(false);
  });

  it("a view stays inside the whole hours and at least five minutes long, else is the whole", () => {
    expect(LEAST_SHOWN).toBe(300);
    expect(clampView(null, WHOLE)).toBeNull();
    // Inside: as it is.
    expect(clampView(minutes(60, 120), WHOLE)).toEqual(minutes(60, 120));
    // All of the hours, or more: the whole task.
    expect(clampView(WHOLE, WHOLE)).toBeNull();
    expect(clampView(minutes(-60, 400), WHOLE)).toBeNull();
    // Narrower than five minutes: widened about its middle.
    expect(clampView(minutes(60, 61), WHOLE)).toEqual({ from: at(58), to: at(63) });
    expect(clampView({ from: at(60), to: at(60) }, WHOLE)).toEqual({
      from: at(60) - 150,
      to: at(60) + 150,
    });
    // Past an end: moved inside, as long.
    expect(clampView(minutes(-30, 30), WHOLE)).toEqual(minutes(0, 60));
    expect(clampView(minutes(370, 430), WHOLE)).toEqual({ from: END - HOUR, to: END });
    // Narrow at an end: widened, then moved inside.
    expect(clampView(minutes(0, 1), WHOLE)).toEqual(minutes(0, 5));
    // Hours too short to zoom into show whole, whatever the view.
    expect(clampView(minutes(0, 2), { from: START, to: START + LEAST_SHOWN })).toBeNull();
    // A view that is no number is the whole task.
    expect(clampView({ from: Number.NaN, to: at(60) }, WHOLE)).toBeNull();
    expect(clampView({ from: at(0), to: Number.POSITIVE_INFINITY }, WHOLE)).toBeNull();
  });

  it("a wheel's or a pinch's zoom keeps the instant under the pointer where it was", () => {
    const view = minutes(60, 180);
    const zoomedIn = gesture(view, WHOLE, 0.25, 0.25, 0.5);
    expect(zoomedIn).toEqual(minutes(75, 135));
    // The instant a quarter of the way along stays a quarter of the way along.
    const under = (shown: View, fraction: number) =>
      shown.from + fraction * (shown.to - shown.from);
    expect(under(zoomedIn as View, 0.25)).toBe(under(view, 0.25));
    // From the whole task, about its middle.
    expect(gesture(null, WHOLE, 0.5, 0.5, 0.5)).toEqual(minutes(95, 285));
    // Out to all of it or past: the whole task.
    expect(gesture(view, WHOLE, 0.5, 0.5, 4)).toBeNull();
    // In past the least: five minutes, about the pointer.
    expect(gesture(view, WHOLE, 0.5, 0.5, 0.001)).toEqual({
      from: at(120) - 150,
      to: at(120) + 150,
    });
    // Out at an end: kept inside.
    expect(gesture(minutes(0, 60), WHOLE, 0, 0, 2)).toEqual(minutes(0, 120));
  });

  it("a swipe moves the hours shown as far as the finger went, and stops at an end", () => {
    // The instant under a quarter of the way along goes to the middle: an earlier view.
    expect(gesture(minutes(60, 180), WHOLE, 0.25, 0.5, 1)).toEqual(minutes(30, 150));
    expect(gesture(minutes(60, 180), WHOLE, 1, 0, 1)).toEqual(minutes(180, 300));
    expect(gesture(minutes(60, 180), WHOLE, 0, 1, 1)).toEqual(minutes(0, 120));
    expect(gesture(minutes(300, 360), WHOLE, 1, 0, 1)).toEqual(minutes(320, 380));
    // The whole task moves nowhere.
    expect(gesture(null, WHOLE, 0.25, 0.75, 1)).toBeNull();
  });

  it("a press zooms by a step about the middle, never past the least or the whole", () => {
    expect(STEP).toBe(2);
    expect(zoomed(null, WHOLE, true)).toEqual(minutes(95, 285));
    expect(zoomed(minutes(95, 285), WHOLE, false)).toBeNull();
    expect(zoomed(minutes(60, 120), WHOLE, true)).toEqual(minutes(75, 105));
    expect(zoomed(minutes(60, 120), WHOLE, false)).toEqual(minutes(30, 150));
    // The whole task zooms out to itself.
    expect(zoomed(null, WHOLE, false)).toBeNull();
    // At the least, a press in leaves it there.
    expect(length(zoomed(minutes(60, 65), WHOLE, true))).toBe(LEAST_SHOWN);
    expect(zoomed(minutes(60, 65), WHOLE, true)).toEqual(minutes(60, 65));
  });

  it("a move goes by a part of the hours shown, and the whole task goes nowhere", () => {
    expect(moved(minutes(60, 120), WHOLE, 0.5)).toEqual(minutes(90, 150));
    expect(moved(minutes(60, 120), WHOLE, -0.1)).toEqual(minutes(54, 114));
    expect(moved(minutes(60, 120), WHOLE, -5)).toEqual(minutes(0, 60));
    expect(moved(minutes(60, 120), WHOLE, 50)).toEqual({ from: END - HOUR, to: END });
    expect(moved(null, WHOLE, 0.5)).toBeNull();
  });

  it("a drag across the hours zooms to them, either way, inside the hours shown", () => {
    expect(dragged(null, WHOLE, 0.25, 0.5)).toEqual(minutes(95, 190));
    expect(dragged(null, WHOLE, 0.5, 0.25)).toEqual(minutes(95, 190));
    expect(dragged(minutes(60, 120), WHOLE, 0.5, 1)).toEqual(minutes(90, 120));
    // Past the tracks' ends: to their ends.
    expect(dragged(minutes(60, 120), WHOLE, -1, 2)).toEqual(minutes(60, 120));
    // Narrower than the least: widened about its middle.
    expect(dragged(minutes(60, 120), WHOLE, 0.5, 0.51)).toEqual({
      from: at(90.3) - 150,
      to: at(90.3) + 150,
    });
    // All of the whole task: the whole task.
    expect(dragged(null, WHOLE, 0, 1)).toBeNull();
  });

  it("a double click on a stage zooms to the stage", () => {
    expect(zoomTo(WHOLE, at(29), at(141))).toEqual(minutes(29, 141));
    expect(zoomTo(WHOLE, at(29), at(30))).toEqual({ from: at(29.5) - 150, to: at(29.5) + 150 });
    expect(zoomTo(WHOLE, START, END)).toBeNull();
  });

  it("a part outside the hours shown comes into view, as long, its middle or its start theirs", () => {
    const view = minutes(60, 120);
    // In view, or partly: as it is.
    expect(revealed(view, WHOLE, at(70), at(80))).toBe(view);
    expect(revealed(view, WHOLE, at(30), at(60))).toBe(view);
    expect(revealed(view, WHOLE, at(120), at(200))).toBe(view);
    // Outside: centred on it.
    expect(revealed(view, WHOLE, at(200), at(220))).toEqual(minutes(180, 240));
    expect(revealed(view, WHOLE, at(150), at(150))).toEqual(minutes(120, 180));
    // Longer than the view: from its start.
    expect(revealed(view, WHOLE, at(200), at(300))).toEqual(minutes(200, 260));
    expect(revealed(view, WHOLE, at(0), at(59))).toEqual(minutes(0, 60));
    // Near an end: as far as the end.
    expect(revealed(view, WHOLE, at(375), at(380))).toEqual({ from: END - HOUR, to: END });
    expect(revealed(view, WHOLE, at(1), at(2))).toEqual(minutes(0, 60));
    // The whole task shows every part.
    expect(revealed(null, WHOLE, at(200), at(220))).toBeNull();
  });

  it("a drag of the overview's window moves it, and a drag of an edge resizes it", () => {
    // Each drag a part of the overview, the whole 380 minutes: a quarter is 95.
    const start = minutes(60, 120);
    expect(held(start, WHOLE, "move", 1 / 4)).toEqual(minutes(155, 215));
    expect(held(start, WHOLE, "move", -1 / 2)).toEqual(minutes(0, 60));
    expect(held(start, WHOLE, "start", -1 / 8)).toEqual(minutes(12.5, 120));
    expect(held(start, WHOLE, "start", -1)).toEqual(minutes(0, 120));
    // Never past the other edge: the least.
    expect(held(start, WHOLE, "start", 1 / 4)).toEqual(minutes(115, 120));
    expect(held(start, WHOLE, "end", 1 / 8)).toEqual(minutes(60, 167.5));
    expect(held(start, WHOLE, "end", -1 / 4)).toEqual(minutes(60, 65));
    expect(held(start, WHOLE, "end", 1)).toEqual({ from: at(60), to: END });
    // Out to all of it: the whole task.
    expect(held(minutes(0, 120), WHOLE, "end", 1)).toBeNull();
  });

  it("a wheel's travel counts a line and a page as pixels, and a notch zooms by a step", () => {
    expect(wheelPixels(100, 0)).toBe(100);
    expect(wheelPixels(3, 1)).toBe(48);
    expect(wheelPixels(1, 2)).toBe(800);
    expect(wheelPixels(-2, 0)).toBe(-2);
    expect(wheelFactor(0)).toBe(1);
    expect(wheelFactor(300)).toBe(2);
    expect(wheelFactor(-300)).toBe(0.5);
    // A trackpad's pinch, a few pixels at a time, zooms a little.
    expect(wheelFactor(-4)).toBeGreaterThan(0.99);
    expect(wheelFactor(-4)).toBeLessThan(1);
  });
});

/** Tracks 1000 px wide from 200 px across the screen. */
const TRACK: Track = { left: 200, width: 1000 };

/** A wheel's turn at `x` by `deltaY`, in pixels, nothing held, with what `more` changes. */
const turn = (x: number, deltaY: number, more: Partial<Wheel> = {}): Wheel => ({
  x,
  deltaX: 0,
  deltaY,
  mode: 0,
  zoom: false,
  shift: false,
  ...more,
});

describe("what a gesture on the lanes does", () => {
  it("reads a place across the screen as a fraction of the hours shown", () => {
    expect(fractionOn(TRACK, 200)).toBe(0);
    expect(fractionOn(TRACK, 450)).toBe(0.25);
    expect(fractionOn(TRACK, 1200)).toBe(1);
    expect(fractionOn(TRACK, 100)).toBe(-0.1);
  });

  it("zooms by a wheel with ⌘ or Ctrl about the pointer, a pointer past an end at that end", () => {
    const view = minutes(60, 180);
    // Up a notch at a quarter of the way along: half as long, its quarter where it was.
    expect(wheeled(view, WHOLE, TRACK, turn(450, -300, { zoom: true }))).toEqual({
      view: minutes(75, 135),
    });
    // Down a notch from the whole task: as long as it can be, the whole task.
    expect(wheeled(null, WHOLE, TRACK, turn(450, 300, { zoom: true }))).toEqual({ view: null });
    // From the whole task, a notch in at its start keeps its start.
    expect(wheeled(null, WHOLE, TRACK, turn(0, -300, { zoom: true }))).toEqual({
      view: minutes(0, 190),
    });
    // A line's travel counts as 16 px: 300 px in 18.75 lines.
    expect(wheeled(view, WHOLE, TRACK, turn(450, -18.75, { zoom: true, mode: 1 }))).toEqual({
      view: minutes(75, 135),
    });
  });

  it("moves a zoomed view by Shift + wheel or a sideways wheel, by its travel across the tracks", () => {
    const view = minutes(60, 180);
    // 250 px of 1000: a quarter of the two hours shown, later.
    expect(wheeled(view, WHOLE, TRACK, turn(450, 250, { shift: true }))).toEqual({
      view: minutes(90, 210),
    });
    // A browser that turns Shift + wheel sideways itself.
    expect(wheeled(view, WHOLE, TRACK, turn(450, 0, { shift: true, deltaX: -250 }))).toEqual({
      view: minutes(30, 150),
    });
    // A trackpad's sideways swipe, a little down too.
    expect(wheeled(view, WHOLE, TRACK, turn(450, 10, { deltaX: 250 }))).toEqual({
      view: minutes(90, 210),
    });
    // At the end already: the view stays, and the page does not move sideways either.
    const last = { from: END - HOUR, to: END };
    expect(wheeled(last, WHOLE, TRACK, turn(450, 250, { shift: true }))).toEqual({ view: last });
  });

  it("leaves a plain wheel, and any but a zoom on the whole task, to scroll the page", () => {
    const view = minutes(60, 180);
    expect(wheeled(view, WHOLE, TRACK, turn(450, 250))).toBeNull();
    // More down than sideways.
    expect(wheeled(view, WHOLE, TRACK, turn(450, 250, { deltaX: 100 }))).toBeNull();
    expect(wheeled(null, WHOLE, TRACK, turn(450, 250, { shift: true }))).toBeNull();
    expect(wheeled(null, WHOLE, TRACK, turn(450, 0, { deltaX: 250 }))).toBeNull();
    // Tracks not drawn: nothing to read the pointer against.
    expect(wheeled(view, WHOLE, { left: 0, width: 0 }, turn(450, -300, { zoom: true }))).toBeNull();
  });

  it("tells a finger's swipe sideways or not once it has gone 6 px", () => {
    expect(sideways(0, 0)).toBeNull();
    expect(sideways(5, -5)).toBeNull();
    expect(sideways(-6, 2)).toBe(true);
    expect(sideways(3, 6)).toBe(false);
    // As far either way: not sideways, so the page scrolls.
    expect(sideways(8, 8)).toBe(false);
  });

  it("moves a zoomed view by a swipe, what was under the finger under it still", () => {
    const view = minutes(60, 180);
    // 250 px to the left: a quarter of the hours shown later.
    expect(swiped(view, WHOLE, TRACK, 700, 450)).toEqual({ view: minutes(90, 210) });
    expect(swiped(view, WHOLE, TRACK, 450, 700)).toEqual({ view: minutes(30, 150) });
    // The whole task does not move, and the page scrolls on.
    expect(swiped(null, WHOLE, TRACK, 700, 450)).toBeNull();
    expect(swiped(view, WHOLE, { left: 0, width: 0 }, 700, 450)).toBeNull();
  });

  it("pinches about the fingers, as many times shorter as they are further apart", () => {
    const view = minutes(60, 180);
    // 200 px apart about a quarter of the way along, spread to 400: half as long, about it.
    expect(
      pinched(
        view,
        WHOLE,
        TRACK,
        [
          { x: 350, y: 0 },
          { x: 550, y: 0 },
        ],
        [
          { x: 250, y: 0 },
          { x: 650, y: 0 },
        ],
      ),
    ).toEqual({ view: minutes(75, 135) });
    // About the middle, 120 min, closed to 100: twice as long about it.
    expect(
      pinched(
        view,
        WHOLE,
        TRACK,
        [
          { x: 600, y: 0 },
          { x: 800, y: 0 },
        ],
        [
          { x: 650, y: 0 },
          { x: 750, y: 0 },
        ],
      ),
    ).toEqual({ view: minutes(0, 240) });
    // Both fingers moved 250 px left, as far apart: a quarter later, as long.
    expect(
      pinched(
        view,
        WHOLE,
        TRACK,
        [
          { x: 600, y: 0 },
          { x: 800, y: 0 },
        ],
        [
          { x: 350, y: 0 },
          { x: 550, y: 0 },
        ],
      ),
    ).toEqual({ view: minutes(90, 210) });
    // The distance is the fingers' straight line, up and down too.
    expect(
      pinched(
        null,
        WHOLE,
        TRACK,
        [
          { x: 700, y: 0 },
          { x: 700, y: 200 },
        ],
        [
          { x: 700, y: -100 },
          { x: 700, y: 300 },
        ],
      ),
    ).toEqual({ view: minutes(95, 285) });
  });

  it("tells nothing of two fingers on one place, or of tracks not drawn", () => {
    const one = { x: 700, y: 50 };
    const two: [{ x: number; y: number }, { x: number; y: number }] = [
      { x: 600, y: 0 },
      { x: 800, y: 0 },
    ];
    expect(pinched(null, WHOLE, TRACK, [one, one], two)).toBeNull();
    expect(pinched(null, WHOLE, TRACK, two, [one, one])).toBeNull();
    expect(pinched(null, WHOLE, { left: 0, width: 0 }, two, two)).toBeNull();
  });

  it("moves the overview's window by its keys: a tenth, a page, or to an end", () => {
    const view = minutes(60, 180);
    expect(windowKey("ArrowRight", view, WHOLE)).toEqual({ view: minutes(72, 192) });
    expect(windowKey("ArrowLeft", view, WHOLE)).toEqual({ view: minutes(48, 168) });
    expect(windowKey("PageDown", view, WHOLE)).toEqual({ view: minutes(180, 300) });
    expect(windowKey("PageUp", view, WHOLE)).toEqual({ view: minutes(0, 120) });
    expect(windowKey("Home", view, WHOLE)).toEqual({ view: minutes(0, 120) });
    expect(windowKey("End", view, WHOLE)).toEqual({ view: minutes(260, 380) });
    // At the start, a step back stays.
    expect(windowKey("ArrowLeft", minutes(0, 120), WHOLE)).toEqual({ view: minutes(0, 120) });
    // Another key is the page's, an object's own names among them.
    for (const key of ["ArrowUp", "Enter", "a", "constructor", "toString"]) {
      expect(windowKey(key, view, WHOLE)).toBeNull();
    }
  });
});

describe("the toolbar's readout", () => {
  it("says the hours shown and their length, and zoomed, of how long the whole is", () => {
    const timeline = pictures();
    expect(readout(timeline, null)).toBe("11-11 09:00 → 11-11 15:20 · 6 h 20 m");
    expect(readout(timeline, minutes(125, 198))).toBe(
      "11-11 11:05 → 11-11 12:18 · 1 h 13 m of 6 h 20 m",
    );
    // To the minute, as every time on the page: the start's minute, the length rounded.
    expect(readout(timeline, { from: at(125) + 50, to: at(130) + 50 })).toBe(
      "11-11 11:05 → 11-11 11:10 · 5 m of 6 h 20 m",
    );
  });

  it("names each end's own day across midnight", () => {
    const timeline = pictures();
    const view = { from: at(14 * 60 + 30), to: at(15 * 60 + 30) };
    const later = { ...timeline, now: at(16 * 60) };
    expect(readout(later, view)).toBe("11-11 23:30 → 11-12 00:30 · 1 h of 16 h");
  });
});

describe("the overview", () => {
  it("draws the whole task's stages but its close, each run at its start, and the window", () => {
    const timeline = pictures();
    const strip = overview(timeline, minutes(95, 190));
    const span = END - START;
    const percent = (time: number) => ((time - START) / span) * 100;
    expect(strip.window.left).toBeCloseTo(25, 10);
    expect(strip.window.width).toBeCloseTo(25, 10);
    expect(strip.stages.map((mark) => mark.tone)).toEqual([
      "auto",
      "primary",
      "question",
      "primary",
      "question",
      "question",
    ]);
    const [first] = timeline.stages;
    expect(strip.stages[0]?.left).toBeCloseTo(percent(first?.from ?? 0), 10);
    // The open stage runs to now, the hours' end.
    const last = strip.stages.at(-1);
    expect((last?.left ?? 0) + (last?.width ?? 0)).toBeCloseTo(100, 10);
    expect(strip.runs.map((mark) => mark.tone)).toEqual([
      "like",
      "like",
      "like",
      "like",
      "primary",
    ]);
    expect(strip.runs[0]?.left).toBeCloseTo(percent(timeline.runs[0]?.started ?? 0), 10);
    // A run that never started is marked at its queue; a closed task's close is no stage.
    const queued = {
      ...timeline,
      runs: [{ ...(timeline.runs[0] as TaskTimeline["runs"][number]), started: null }],
      stages: [...timeline.stages, { status: "done", from: END, to: END }],
    };
    const marked = overview(queued, minutes(0, 60));
    expect(marked.runs[0]?.left).toBeCloseTo(percent(timeline.runs[0]?.queued ?? 0), 10);
    expect(marked.stages).toHaveLength(6);
  });
});

describe("a zoom across the quiet stretches skipped", () => {
  /** 11:00 to 14:00 skipped, a band a tenth of the track: 200 minutes of work, so the whole task
   * holds 222.2 minutes of work on a track, and a view of 50 minutes with no band in it holds 50,
   * the band then 5 minutes of the axis. */
  const CUT = { skipped: [{ from: at(120), to: at(300) }], share: 0.1 };
  const cuts = new Cuts(WHOLE, CUT);
  /** `view`'s ends in minutes after 09:00, to a hundredth. */
  const ends = (view: View | null) =>
    view === null ? null : [view.from, view.to].map((t) => Math.round((t - START) / 0.6) / 100);

  it("draws the whole task where all of it shows, the band and all, and nothing shorter", () => {
    expect(clampView(WHOLE, WHOLE, CUT)).toBeNull();
    expect(zoomed(WHOLE, WHOLE, false, CUT)).toBeNull();
    // A view in the work alone is the clock's.
    expect(clampView(minutes(0, 50), WHOLE, CUT)).toEqual(minutes(0, 50));
    // A view of two minutes of work across the band is widened to five minutes of the axis, the
    // least.
    const narrow = clampView(minutes(119, 301), WHOLE, CUT);
    expect(leastShown(narrow, WHOLE, CUT)).toBe(true);
    expect(cuts.span(narrow as View)).toBeCloseTo(LEAST_SHOWN, 6);
    expect(leastShown(minutes(119, 301), WHOLE, CUT)).toBe(true);
    expect(leastShown(minutes(0, 50), WHOLE, CUT)).toBe(false);
  });

  it("comes back to the whole task from a zoom in and out again, whatever its seconds' rounding", () => {
    const guards = readTimeline(JSON.parse(JSON.stringify(GUARDS)));
    if (guards === null) throw new Error("the card's task is no timeline");
    const whole = wholeOf(guards);
    for (const cut of [null, { skipped: [], share: 0.033 }, CUT_OF(guards)]) {
      let view: View | null = null;
      for (let step = 0; step < 6; step += 1) view = zoomed(view, whole, true, cut);
      for (let step = 0; step < 6; step += 1) view = zoomed(view, whole, false, cut);
      expect(view).toBeNull();
      expect(zoomed(zoomed(null, whole, true, cut), whole, false, cut)).toBeNull();
    }
  });

  it("keeps the instant under the pointer where it was, the band as wide", () => {
    const view = minutes(60, 360);
    for (const x of [0, 0.3, 0.4, 0.45, 0.7, 1]) {
      const under = cuts.time(view, x);
      const zoomedIn = gesture(view, WHOLE, x, x, 0.5, CUT) as View;
      expect(cuts.at(zoomedIn, under)).toBeCloseTo(x, 9);
      expect(cuts.span(zoomedIn)).toBeCloseTo(cuts.span(view) / 2, 6);
    }
    // A notch of the wheel with ⌘ about the pointer does as much.
    const x = 0.45;
    expect(wheeled(view, WHOLE, TRACK, turn(200 + 1000 * x, -300, { zoom: true }), CUT)).toEqual({
      view: gesture(view, WHOLE, x, x, 0.5, CUT),
    });
  });

  it("moves a view across the band by as much of the track as the band takes", () => {
    // A track on from 10:00–10:50: ten minutes of work, the band's five, then 35 minutes more.
    expect(ends(moved(minutes(60, 110), WHOLE, 1, CUT))).toEqual([110, 335]);
    expect(ends(moved(minutes(330, 380), WHOLE, -1, CUT))).toEqual([105, 330]);
    // An end stops it.
    expect(ends(moved(minutes(60, 110), WHOLE, 10, CUT))).toEqual([330, 380]);
    // Shift + wheel, a swipe and the overview's keys move it so too.
    expect(
      ends(
        wheeled(minutes(60, 110), WHOLE, TRACK, turn(0, 1000, { shift: true }), CUT)?.view ?? null,
      ),
    ).toEqual([110, 335]);
    expect(ends(swiped(minutes(60, 110), WHOLE, TRACK, 1200, 200, CUT)?.view ?? null)).toEqual([
      110, 335,
    ]);
    expect(ends(windowKey("PageDown", minutes(60, 110), WHOLE, CUT)?.view ?? null)).toEqual([
      110, 335,
    ]);
    expect(ends(windowKey("Home", minutes(330, 380), WHOLE, CUT)?.view ?? null)).toEqual([0, 50]);
    expect(ends(windowKey("End", minutes(0, 50), WHOLE, CUT)?.view ?? null)).toEqual([330, 380]);
  });

  it("pinches about the fingers across the band", () => {
    const view = minutes(60, 360);
    const pinch = pinched(
      view,
      WHOLE,
      TRACK,
      [
        { x: 500, y: 0 },
        { x: 700, y: 0 },
      ],
      [
        { x: 400, y: 0 },
        { x: 800, y: 0 },
      ],
      CUT,
    );
    expect(pinch).toEqual({ view: gesture(view, WHOLE, 0.4, 0.4, 0.5, CUT) });
  });

  it("zooms to the hours dragged across the band, and brings a part into view across it", () => {
    const x = (time: number) => cuts.at(WHOLE, time);
    expect(ends(dragged(null, WHOLE, x(at(320)), x(at(100)), CUT))).toEqual([100, 320]);
    // At the zoom of a 50-minute view, a part from 14:05 to 14:10 in the middle: 25 minutes of the
    // axis before it, 7.5 of them work after the band, the band's five, and 12.5 more.
    expect(ends(revealed(minutes(0, 50), WHOLE, at(305), at(310), CUT))).toEqual([107.5, 332.5]);
    expect(ends(zoomTo(WHOLE, at(100), at(320), CUT))).toEqual([100, 320]);
  });

  it("drags the overview's window across the band, each drag a part of the overview", () => {
    const x = (time: number) => cuts.at(WHOLE, time);
    const start = minutes(60, 110);
    // Moved, it keeps its zoom: 50 minutes of work, past the band.
    expect(ends(held(start, WHOLE, "move", x(at(300)) - x(at(60)), CUT))).toEqual([300, 350]);
    // An edge dragged over the band takes it in.
    expect(ends(held(start, WHOLE, "end", x(at(330)) - x(at(110)), CUT))).toEqual([60, 330]);
    expect(ends(held(start, WHOLE, "start", -1, CUT))).toEqual([0, 110]);
  });

  it("says how long is skipped in the hours shown, a band partly in them for its part", () => {
    const timeline = pictures();
    expect(readout(timeline, null, CUT)).toBe("11-11 09:00 → 11-11 15:20 · 6 h 20 m · 3 h skipped");
    expect(readout(timeline, minutes(110, 335), CUT)).toBe(
      "11-11 10:50 → 11-11 14:35 · 3 h 45 m of 6 h 20 m · 3 h skipped",
    );
    expect(readout(timeline, minutes(200, 335), CUT)).toBe(
      "11-11 12:20 → 11-11 14:35 · 2 h 15 m of 6 h 20 m · 1 h 40 m skipped",
    );
    // Less than half a minute of it is not said; nor is a band outside the view.
    expect(readout(timeline, { from: at(300) - 20, to: at(330) }, CUT)).toBe(
      "11-11 13:59 → 11-11 14:30 · 30 m of 6 h 20 m",
    );
    expect(readout(timeline, minutes(0, 50), CUT)).toBe(
      "11-11 09:00 → 11-11 09:50 · 50 m of 6 h 20 m",
    );
  });

  it("draws each band on the overview at its share, the work about it to scale", () => {
    const strip = overview(pictures(), minutes(0, 50), CUT);
    expect(strip.bands).toHaveLength(1);
    expect(strip.bands[0]?.left).toBeCloseTo(54, 9);
    expect(strip.bands[0]?.width).toBeCloseTo(10, 9);
    expect(strip.window.left).toBeCloseTo(0, 9);
    expect(strip.window.width).toBeCloseTo(22.5, 9);
    // Nothing skipped, no band.
    expect(overview(pictures(), minutes(0, 50)).bands).toEqual([]);
  });
});
