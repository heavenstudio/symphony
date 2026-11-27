// A task's wide timeline zoomed and moved, with no DOM: the part of its hours the lanes show (a
// `View`; none is the whole task, which each visit opens on), kept inside the whole hours and never
// narrower than LEAST_SHOWN; how a press, a key, a wheel, a pinch, a swipe, a drag across the
// hours, a double click on a stage and a drag of the overview's window move it; what the
// toolbar's readout says of it; and the overview strip drawn while zoomed.
//
// Every move is a fraction of the track, as the pointer's place on it gives it, so the arithmetic
// needs no pixel. Where quiet stretches are skipped (`cut`), it goes through the cut scale
// (timeline-cut.ts): a band keeps its width at every zoom, so what is under the pointer stays there
// across a band, and LEAST_SHOWN is five minutes of work. A view stays where it is when a read
// brings a new step; the whole task grows with it. A task no longer than LEAST_SHOWN is drawn whole,
// with no toolbar.

import { type Cut, Cuts } from "./timeline-cut";
import {
  clamp,
  closed,
  resultTone,
  Scale,
  stampOf,
  statusTone,
  took,
  type View,
} from "./timeline-layout";
import type { TaskTimeline } from "./wire";

/** The least part of the hours a zoom shows, in seconds: five minutes. */
export const LEAST_SHOWN = 300;
/** How many times as long or as short a press, a key or a wheel's notch makes the hours shown. */
export const STEP = 2;
/** A wheel's travel, in pixels, that zooms by one STEP. */
const WHEEL_STEP_PX = 300;
/** Pixels a wheel's line and page travel stand for, where a browser counts in those. */
const LINE_PX = 16;
const PAGE_PX = 800;
/** How far, in pixels, a finger moves before its swipe is told sideways or not. */
const SWIPE_PX = 6;
/** How much of the hours shown an arrow key moves the overview's window by. */
const KEY_MOVE = 0.1;

/** The whole hours the lanes draw for `timeline`, as a view. */
export function wholeOf(timeline: TaskTimeline): View {
  const scale = new Scale(timeline);
  return { from: scale.start, to: scale.start + scale.span };
}

/** Whether the whole hours are longer than a zoom's least, so zooming can show less of them. */
export function zoomable(whole: View): boolean {
  return whole.to - whole.from > LEAST_SHOWN;
}

/**
 * The view one track long from `u` on `cuts`' axis where a track holds `track` seconds of work,
 * kept inside the whole hours and holding at least LEAST_SHOWN; none (the whole task) where it
 * holds the whole hours.
 */
function windowAt(cuts: Cuts, track: number, u: number): View | null {
  const held = Math.max(track, LEAST_SHOWN);
  // A hair short of the whole, as a zoom in and out again leaves it, is the whole.
  if (!(held < cuts.most * (1 - 1e-9))) return null;
  return cuts.window(held, clamp(u, 0, cuts.length(held) - held));
}

/**
 * `view` kept inside `whole` and showing at least LEAST_SHOWN of work, about its middle; none (the
 * whole task) where it holds all of `whole`, or where `whole` is too short to zoom into.
 */
export function clampView(view: View | null, whole: View, cut?: Cut | null): View | null {
  if (view === null || !Number.isFinite(view.from) || !Number.isFinite(view.to)) return null;
  const cuts = new Cuts(whole, cut);
  const track = cuts.span(view);
  const middle = track > 0 ? cuts.time(view, 0.5) : (view.from + view.to) / 2;
  const held = Math.max(track, LEAST_SHOWN);
  return windowAt(cuts, held, cuts.axis(held, middle) - held / 2);
}

/**
 * The view a gesture leaves: the hours shown `factor` times as long, and the instant that was at
 * `before` (a fraction of the track, 0 at its start) now at `after`. A wheel's or a pinch's zoom
 * keeps the instant under the pointer (`before` = `after`); a swipe, a sideways wheel or two
 * fingers moving together move it (`factor` 1); a press zooms about the middle.
 */
export function gesture(
  view: View | null,
  whole: View,
  before: number,
  after: number,
  factor: number,
  cut?: Cut | null,
): View | null {
  const cuts = new Cuts(whole, cut);
  const shown = view ?? whole;
  const at = cuts.time(shown, before);
  const track = clamp(cuts.span(shown) * factor, LEAST_SHOWN, cuts.most);
  return windowAt(cuts, track, cuts.axis(track, at) - after * track);
}

/** A press's or a key's zoom, about the middle of the hours shown: in, or out. */
export function zoomed(
  view: View | null,
  whole: View,
  inward: boolean,
  cut?: Cut | null,
): View | null {
  return gesture(view, whole, 0.5, 0.5, inward ? 1 / STEP : STEP, cut);
}

/** The view moved by `by` of the track, later where it is more than 0. */
export function moved(view: View | null, whole: View, by: number, cut?: Cut | null): View | null {
  return gesture(view, whole, by, 0, 1, cut);
}

/** The view a drag across the hours from `a` to `b` zooms to, each a fraction of the track. */
export function dragged(
  view: View | null,
  whole: View,
  a: number,
  b: number,
  cut?: Cut | null,
): View | null {
  const cuts = new Cuts(whole, cut);
  const shown = view ?? whole;
  const from = cuts.time(shown, clamp(Math.min(a, b), 0, 1));
  const to = cuts.time(shown, clamp(Math.max(a, b), 0, 1));
  return clampView({ from, to }, whole, cut);
}

/** The view that shows `from` to `to`, a stage a double click zoomed to. */
export function zoomTo(whole: View, from: number, to: number, cut?: Cut | null): View | null {
  return clampView({ from, to }, whole, cut);
}

/**
 * The view with `from` to `to` in it: as it is where any of them shows (the whole task always
 * does), else at the same zoom, its middle theirs, or its start, where its words are, for a part
 * longer than the view. A part the keyboard steps to, and a message chosen in the message list,
 * come into view so.
 */
export function revealed(
  view: View | null,
  whole: View,
  from: number,
  to: number,
  cut?: Cut | null,
): View | null {
  if (view === null || (to >= view.from && from <= view.to)) return view;
  const cuts = new Cuts(whole, cut);
  const track = cuts.span(view);
  const [a, b] = [cuts.axis(track, from), cuts.axis(track, to)];
  return windowAt(cuts, track, b - a > track ? a : (a + b - track) / 2);
}

/** Which of the overview's window a drag holds: all of it, which moves it, or an edge, which
 * resizes it. */
export type Hold = "move" | "start" | "end";

/**
 * The view a drag of the overview's window by `by` of the overview leaves, from `start`, the view
 * as the drag began: the part it holds where the pointer took it on the overview, the rest as it
 * was, and an edge never so near the other that less than a zoom's least shows.
 */
export function held(
  start: View,
  whole: View,
  hold: Hold,
  by: number,
  cut?: Cut | null,
): View | null {
  const cuts = new Cuts(whole, cut);
  const taken = (at: number) => cuts.time(whole, cuts.at(whole, at) + by);
  if (hold === "move") {
    const track = cuts.span(start);
    return windowAt(cuts, track, cuts.axis(track, taken(start.from)));
  }
  const least = LEAST_SHOWN;
  if (hold === "start") {
    const latest = cuts.instant(least, cuts.axis(least, start.to) - least);
    const from = clamp(taken(start.from), whole.from, latest);
    return clampView({ from, to: start.to }, whole, cut);
  }
  const earliest = cuts.instant(least, cuts.axis(least, start.from) + least);
  const to = clamp(taken(start.to), earliest, whole.to);
  return clampView({ from: start.from, to }, whole, cut);
}

/** Whether `view` is as narrow as a zoom goes: no more of the hours shows at a press of +. */
export function leastShown(view: View | null, whole: View, cut?: Cut | null): boolean {
  return view !== null && new Cuts(whole, cut).span(view) <= LEAST_SHOWN + 1e-6;
}

/** A wheel's travel in pixels: a line's and a page's as a browser that counts in those means them
 * (`mode` 1 and 2). */
export function wheelPixels(delta: number, mode: number): number {
  return delta * (mode === 1 ? LINE_PX : mode === 2 ? PAGE_PX : 1);
}

/** How many times as long a wheel's travel of `pixels` (down, more than 0, zooms out) makes the
 * hours shown: a notch's STEP, a trackpad's pinch a little at a time. */
export function wheelFactor(pixels: number): number {
  return STEP ** (pixels / WHEEL_STEP_PX);
}

/** The view a gesture leaves; none where the gesture is not the timeline's (a plain wheel scrolls
 * the page), or tells nothing yet. */
export type Next = { view: View | null } | null;

/** Where the tracks are across the screen, in pixels: the hours' layer's start and width. */
export interface Track {
  left: number;
  width: number;
}

/** A place on the screen, in pixels. */
export interface Point {
  x: number;
  y: number;
}

/** The fraction of the hours shown at `x`, 0 at the tracks' start and 1 at their end. */
export function fractionOn(track: Track, x: number): number {
  return (x - track.left) / track.width;
}

/** A wheel's turn over the lanes: where (`x`), its travel each way in its `mode`'s unit, and
 * whether ⌘ or Ctrl (`zoom`; a trackpad's pinch sends Ctrl) or Shift is held. */
export interface Wheel {
  x: number;
  deltaX: number;
  deltaY: number;
  mode: number;
  zoom: boolean;
  shift: boolean;
}

/**
 * What a wheel's turn does: with ⌘ or Ctrl, it zooms about the pointer; while zoomed, with Shift or
 * travelling more sideways than down, it moves by its travel across the tracks; else nothing, and
 * the page scrolls.
 */
export function wheeled(
  view: View | null,
  whole: View,
  track: Track,
  wheel: Wheel,
  cut?: Cut | null,
): Next {
  if (!(track.width > 0)) return null;
  const x = wheelPixels(wheel.deltaX, wheel.mode);
  const y = wheelPixels(wheel.deltaY, wheel.mode);
  if (wheel.zoom) {
    const pointer = clamp(fractionOn(track, wheel.x), 0, 1);
    return { view: gesture(view, whole, pointer, pointer, wheelFactor(y), cut) };
  }
  if (view === null || !(wheel.shift || Math.abs(x) > Math.abs(y))) return null;
  return { view: moved(view, whole, (x !== 0 ? x : y) / track.width, cut) };
}

/** Whether one finger's travel by `dx`, `dy` is a sideways swipe; none until it has gone
 * SWIPE_PX either way. */
export function sideways(dx: number, dy: number): boolean | null {
  if (Math.max(Math.abs(dx), Math.abs(dy)) < SWIPE_PX) return null;
  return Math.abs(dx) > Math.abs(dy);
}

function between([a, b]: [Point, Point]) {
  return { x: (a.x + b.x) / 2, distance: Math.hypot(a.x - b.x, a.y - b.y) };
}

/**
 * The view two fingers leave, from `view`, the view as they touched: the hours shown as many times
 * shorter as the fingers are now further apart than they began, and the instant that was between
 * them where they are now between them; none for fingers on one place.
 */
export function pinched(
  view: View | null,
  whole: View,
  track: Track,
  began: [Point, Point],
  now: [Point, Point],
  cut?: Cut | null,
): Next {
  const before = between(began);
  const after = between(now);
  if (!(track.width > 0 && before.distance > 0 && after.distance > 0)) return null;
  const from = fractionOn(track, before.x);
  const to = fractionOn(track, after.x);
  return { view: gesture(view, whole, from, to, before.distance / after.distance, cut) };
}

/** The view a finger swiped sideways from `from` to `to` leaves, from `view`, the view as it
 * touched: what was under it, under it still. Only a zoomed view moves. */
export function swiped(
  view: View | null,
  whole: View,
  track: Track,
  from: number,
  to: number,
  cut?: Cut | null,
): Next {
  if (view === null || !(track.width > 0)) return null;
  return { view: gesture(view, whole, fractionOn(track, from), fractionOn(track, to), 1, cut) };
}

/**
 * What a key does to the overview's window with the focus, past + − 0: the arrows move it a tenth
 * of the track, Page Up and Page Down a whole track, Home and End take it to an end; nothing for
 * another key.
 */
export function windowKey(key: string, view: View, whole: View, cut?: Cut | null): Next {
  const cuts = new Cuts(whole, cut);
  const track = cuts.span(view);
  const ends = cuts.length(track) / track;
  const by = (() => {
    switch (key) {
      case "ArrowLeft":
        return -KEY_MOVE;
      case "ArrowRight":
        return KEY_MOVE;
      case "PageUp":
        return -1;
      case "PageDown":
        return 1;
      case "Home":
        return -ends;
      case "End":
        return ends;
      default:
        return null;
    }
  })();
  return by === null ? null : { view: moved(view, whole, by, cut) };
}

/** The toolbar's readout: the hours shown, their length, and, zoomed, of how long the whole is
 * ("11-19 11:05 → 11-19 12:18 · 1 h 13 m of 1 d 3 h"); then how much of them a band skips, where
 * one is shown ("· 10 h 54 m skipped"), as the widths across it are not to scale. */
export function readout(timeline: TaskTimeline, view: View | null, cut?: Cut | null): string {
  const whole = wholeOf(timeline);
  const shown = view ?? whole;
  const of = view === null ? "" : ` of ${took(whole.to - whole.from)}`;
  const skipped = new Cuts(whole, cut).skippedIn(shown.from, shown.to);
  const said = skipped < 30 ? "" : ` · ${took(skipped)} skipped`;
  const [from, to] = [stampOf(timeline, shown.from), stampOf(timeline, shown.to)];
  return `${from} → ${to} · ${took(shown.to - shown.from)}${of}${said}`;
}

/** A mark of the overview, in percent of the whole hours. */
export interface Mark {
  left: number;
  width: number;
  tone: string;
}

/** The overview strip: the whole task's stages and runs (each at its start), the window shown and
 * each band, in percent of the whole hours as the lanes draw the whole task. */
export interface Overview {
  stages: Mark[];
  runs: Mark[];
  window: { left: number; width: number };
  bands: { left: number; width: number }[];
}

/** The overview of `timeline` with `view`'s window on it, its quiet stretches skipped as `cut`
 * skips them. */
export function overview(timeline: TaskTimeline, view: View, cut?: Cut | null): Overview {
  const scale = new Scale(timeline, null, cut);
  const stages = timeline.stages.flatMap((stage): Mark[] => {
    const to = stage.to ?? scale.now;
    if (closed(stage) || !scale.spans(stage.from, to)) return [];
    const { left, width } = scale.place(stage.from, to);
    return [{ left, width, tone: statusTone(stage.status) }];
  });
  const runs = timeline.runs.map((run) => ({
    left: scale.at(run.started ?? run.queued),
    width: 0,
    tone: resultTone(run.result),
  }));
  const { left, width } = scale.place(view.from, view.to);
  const bands = scale.cuts.skipped.map((skip) => {
    const { left, width } = scale.place(skip.from, skip.to);
    return { left, width };
  });
  return { stages, runs, window: { left, width }, bands };
}
