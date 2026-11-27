// The wide timeline's zoom and movement, drawn and handled: the toolbar line above the lanes
// (Whole task, − and +, the range shown, and where the task has quiet stretches the switch that
// skips them), the overview strip while zoomed, and what the lanes do with a wheel, a pointer, a
// touch and a key. The arithmetic is timeline-zoom.ts's, through the cut scale where quiet
// stretches are skipped (`Navigation.cut`); this module only reads where the pointer is on the
// tracks, as a fraction of them, and hands the view it computes to `set`. A chunk of
// timeline.tsx's: it imports nothing of the task page.
//
// - ⌘/Ctrl + wheel, a trackpad's pinch with it, zooms about the pointer; Shift + wheel and a
//   sideways wheel move while zoomed; a plain wheel scrolls the page, which the lanes never trap.
// - A mouse dragged across the tracks zooms to the hours dragged, a band drawn while it is held;
//   the click its release makes runs nothing. A double click on a stage's bar zooms to the stage.
// - On a touch screen two fingers pinch, and one swiped sideways moves while zoomed; the page
//   still scrolls up and down under a finger (`touch-action: pan-y` in timeline.css).
// - While the lanes or the overview's window have the focus, + and − zoom and 0 shows the whole
//   task.
// - The range shown is said to assistive technology after a press or a key alone, never at each
//   step of a wheel, a drag or a pinch, and said again where it is the one said last
//   (`Navigation.said`).

import {
  type CSSProperties,
  type KeyboardEvent,
  type MouseEvent,
  type PointerEvent,
  type RefObject,
  useEffect,
  useRef,
  useState,
} from "react";
import { flushSync } from "react-dom";
import { Button } from "./button";
import type { Cut } from "./timeline-cut";
import { took, type View } from "./timeline-layout";
import {
  dragged,
  type Hold,
  held,
  leastShown,
  type Next,
  overview,
  type Point,
  pinched,
  readout,
  sideways,
  swiped,
  type Track,
  wheeled,
  windowKey,
  zoomed,
  zoomTo,
} from "./timeline-zoom";
import type { TaskTimeline } from "./wire";
import { fixed } from "./words";

/** The id of the lanes, which the overview's window says it moves. */
export const LANES_ID = "tl-lanes";

/** How far, in pixels, a mouse moves across the tracks before it drags rather than clicks. */
const DRAG_PX = 4;

/** The range last said to assistive technology, and how many have been said: the readout draws
 * each anew (`ZoomBar`), so a range equal to the one before it is said again. */
export interface Heard {
  text: string;
  times: number;
}

/** `text` said after `last`: one more said, so it is said again where it is `last`'s. */
export function sayAgain(last: Heard | undefined, text: string): Heard {
  return { text, times: (last?.times ?? 0) + 1 };
}

/** The view the lanes show and how to change it: the whole hours, the view (none for the whole
 * task), the quiet stretches skipped (`cut`), and `set`, which takes the next view (none for the
 * whole task) and, for a press or a key (`say`), says its range to assistive technology, where
 * `said` holds it. */
export interface Navigation {
  whole: View;
  view: View | null;
  cut?: Cut | null | undefined;
  said?: Heard | undefined;
  set: (view: View | null, say?: boolean) => void;
}

/** A share in percent as a style writes it, to two places: "12.5%". */
export const percent = (value: number) => `${fixed(value, 2)}%`;
/** A mark's place and its tone, as a style. */
const marked = (tone: string, left: number, width?: number) =>
  ({
    "--c": `var(--${tone})`,
    left: percent(left),
    ...(width === undefined ? {} : { width: percent(width) }),
  }) as CSSProperties;

/**
 * What a key does to the view while the lanes or the overview's window have the focus: + (or =)
 * zooms in, − zooms out, 0 shows the whole task; none for any other key, or one with a modifier,
 * which is the browser's (⌘ + zooms the page).
 */
export function keyView(
  key: string,
  modified: boolean,
  view: View | null,
  whole: View,
  cut?: Cut | null,
): { view: View | null } | null {
  if (modified) return null;
  switch (key) {
    case "+":
    case "=":
      return { view: zoomed(view, whole, true, cut) };
    case "-":
    case "_":
    case "−":
      return { view: zoomed(view, whole, false, cut) };
    case "0":
      return { view: null };
    default:
      return null;
  }
}

/** Sets the view `next` names, the event then the timeline's alone; whether it named one. */
function taken(event: { preventDefault(): void }, next: Next, set: (view: View | null) => void) {
  if (next === null) return false;
  event.preventDefault();
  set(next.view);
  return true;
}

/** A key's zoom, its range said. */
function zoomKey(event: KeyboardEvent<HTMLElement>, nav: Navigation): boolean {
  const modified = event.ctrlKey || event.metaKey || event.altKey;
  return taken(event, keyView(event.key, modified, nav.view, nav.whole, nav.cut), (view) =>
    nav.set(view, true),
  );
}

/** A button of the toolbar's that cannot act: not drawn, its place kept (`.tl-idle`), and
 * disabled, so no key or press reaches it. */
const idle = (cannot: boolean) => ({ disabled: cannot, className: cannot ? "tl-idle" : undefined });

/** The toolbar's buttons, by what they do: Whole task, − and +. */
export type ZoomButton = "whole" | "out" | "in";

/**
 * Where the focus goes once `pressed` has set the view `next`: none where it can still act there;
 * else to the button that now acts, + once the whole task shows (Whole task, or − pressed until
 * it), − at the least. A button that hides itself with the focus would leave it to the page.
 */
export function handoff(
  pressed: ZoomButton,
  next: View | null,
  whole: View,
  cut?: Cut | null,
): "in" | "out" | null {
  if (pressed === "in") return leastShown(next, whole, cut) ? "out" : null;
  return next === null ? "in" : null;
}

/** The labels the focus finds the buttons it moves to by. */
const LABELS = { in: "Zoom in", out: "Zoom out" } as const;

/** The switch that skips the task's quiet stretches: how many there are and how long they last
 * together, whether it is on, and what pressing it does. */
export interface QuietSwitch {
  count: number;
  total: number;
  on: boolean;
  press: () => void;
}

/** The switch's words after its label: how many quiet stretches, and how long together
 * ("1 · 10 h 54 m"). */
export function quietCount(count: number, total: number): string {
  return `${count} · ${took(total)}`;
}

/** The toolbar line: Whole task, − and +, each drawn only where it can act, its place kept so that
 * nothing on the line moves under the pointer (on the whole task + alone, which the readout says
 * is the whole), the range shown, and where the task has quiet stretches (`quiet`) the switch that
 * skips them. */
export function ZoomBar({
  timeline,
  nav,
  quiet,
}: {
  timeline: TaskTimeline;
  nav: Navigation;
  quiet?: QuietSwitch | undefined;
}) {
  const { view, whole, set, cut } = nav;
  /** A press of `button`, which sets `next`, the focus handed on where it would hide: once the
   * view is drawn, as the button it goes to may be hidden until then (+ at the least, − on the
   * whole task). */
  const press = (button: ZoomButton, next: () => View | null) => {
    return (event: MouseEvent<HTMLButtonElement>) => {
      const view = next();
      const pressed = event.currentTarget;
      const to =
        pressed.ownerDocument.activeElement === pressed ? handoff(button, view, whole, cut) : null;
      if (to === null) {
        set(view, true);
        return;
      }
      flushSync(() => set(view, true));
      pressed.parentElement?.querySelector<HTMLElement>(`[aria-label="${LABELS[to]}"]`)?.focus();
    };
  };
  return (
    <div className="tl-zoom">
      {/* biome-ignore lint/a11y/useSemanticElements: a group of the view's buttons, no form's fieldset */}
      <span className="tl-zoom-group" role="group" aria-label="Zoom">
        <Button
          type="button"
          size="sm"
          {...idle(view === null)}
          onClick={press("whole", () => null)}
        >
          Whole task
        </Button>
        <Button
          type="button"
          size="sm"
          aria-label="Zoom out"
          title="Zoom out"
          {...idle(view === null)}
          onClick={press("out", () => zoomed(view, whole, false, cut))}
        >
          −
        </Button>
        <Button
          type="button"
          size="sm"
          aria-label="Zoom in"
          title="Zoom in"
          {...idle(leastShown(view, whole, cut))}
          onClick={press("in", () => zoomed(view, whole, true, cut))}
        >
          +
        </Button>
      </span>
      <span className="tl-range">{readout(timeline, view, cut)}</span>
      <span className="tl-said" aria-live="polite">
        {nav.said && <span key={nav.said.times}>{nav.said.text}</span>}
      </span>
      {quiet && (
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="tl-skip"
          aria-pressed={quiet.on}
          onClick={quiet.press}
        >
          <span className="tl-skip-box" aria-hidden />
          Skip quiet stretches
          <span className="tl-skip-count">{quietCount(quiet.count, quiet.total)}</span>
        </Button>
      )}
    </div>
  );
}

/** A drag of the overview's window as it began: what it holds, where the pointer was, the view
 * then, and how wide the strip is. */
interface WindowDrag {
  hold: Hold;
  x: number;
  start: View;
  width: number;
}

/**
 * The overview strip while zoomed: the whole task's stages and runs, the hours outside the window
 * dimmed, and the window shown, which a drag moves and a drag of an edge resizes; with the focus,
 * the arrow keys move it, Home and End take it to an end, and + − 0 zoom as in the lanes.
 */
export function OverviewStrip({
  timeline,
  view,
  nav,
}: {
  timeline: TaskTimeline;
  view: View;
  nav: Navigation;
}) {
  const strip = overview(timeline, view, nav.cut);
  const drag = useRef<WindowDrag | null>(null);
  const { left, width } = strip.window;
  const down = (event: PointerEvent<HTMLElement>) => {
    if (event.pointerType === "mouse" && event.button !== 0) return;
    const box = event.currentTarget.parentElement?.getBoundingClientRect();
    if (!box || !(box.width > 0)) return;
    const edge = (event.target as HTMLElement).dataset.hold;
    const hold: Hold = edge === "start" || edge === "end" ? edge : "move";
    event.currentTarget.setPointerCapture?.(event.pointerId);
    drag.current = { hold, x: event.clientX, start: view, width: box.width };
  };
  const move = (event: PointerEvent<HTMLElement>) => {
    const at = drag.current;
    if (at === null) return;
    nav.set(held(at.start, nav.whole, at.hold, (event.clientX - at.x) / at.width, nav.cut));
  };
  const up = () => {
    drag.current = null;
  };
  const key = (event: KeyboardEvent<HTMLElement>) => {
    if (!zoomKey(event, nav)) {
      taken(event, windowKey(event.key, view, nav.whole, nav.cut), (next) => nav.set(next, true));
    }
  };
  return (
    <div className="tl-overview">
      {strip.stages.map((mark, at) => (
        <span
          // biome-ignore lint/suspicious/noArrayIndexKey: a stage's mark has no identity but its place
          key={at}
          className="tl-ov-stage"
          aria-hidden
          style={marked(mark.tone, mark.left, mark.width)}
        />
      ))}
      {strip.runs.map((mark, at) => (
        <span
          // biome-ignore lint/suspicious/noArrayIndexKey: a run's mark has no identity but its place
          key={at}
          className="tl-ov-run"
          aria-hidden
          style={marked(mark.tone, mark.left)}
        />
      ))}
      {strip.bands.map((band) => (
        <span
          key={band.left}
          className="tl-ov-break"
          aria-hidden
          style={{ left: percent(band.left), width: percent(band.width) }}
        />
      ))}
      <span className="tl-ov-dim" aria-hidden style={{ left: 0, width: percent(left) }} />
      <span className="tl-ov-dim" aria-hidden style={{ left: percent(left + width), right: 0 }} />
      <span
        className="tl-ov-window"
        role="scrollbar"
        tabIndex={0}
        aria-label="Hours shown"
        aria-controls={LANES_ID}
        aria-orientation="horizontal"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(left)}
        aria-valuetext={readout(timeline, view, nav.cut)}
        style={{ left: percent(left), width: percent(width) }}
        onPointerDown={down}
        onPointerMove={move}
        onPointerUp={up}
        onPointerCancel={up}
        onKeyDown={key}
      >
        <span className="tl-ov-edge tl-ov-start" data-hold="start" aria-hidden />
        <span className="tl-ov-edge tl-ov-end" data-hold="end" aria-hidden />
      </span>
    </div>
  );
}

/** A mouse's press on the tracks: its pointer, the fraction of the hours shown it began at, its x,
 * and whether it has moved far enough to be a drag. */
interface Press {
  id: number;
  from: number;
  x: number;
  dragging: boolean;
}

/** A touch on the lanes: where each finger began, the view then, and, for one finger, whether it
 * is swiping sideways (null until it has moved far enough to tell). */
interface Touches {
  start: Map<number, { x: number; y: number }>;
  now: Map<number, { x: number; y: number }>;
  view: View | null;
  swiping: boolean | null;
}

/** Where the tracks are in `lanes`: the hours' layer, which is drawn from the tracks' own start
 * and width; none where it is not drawn. */
function trackOf(lanes: HTMLElement | null): Track | null {
  const box = lanes?.querySelector(".tl-hours")?.getBoundingClientRect();
  return box && box.width > 0 ? { left: box.left, width: box.width } : null;
}

/** The fraction of the hours shown at `x` in `lanes`; none where the tracks are not drawn. */
function fractionAt(lanes: HTMLElement | null, x: number): number | null {
  const track = trackOf(lanes);
  return track ? (x - track.left) / track.width : null;
}

/** The lanes' handlers of a mouse, a touch, a key and a double click. */
export interface LanesHandlers {
  onPointerDown(event: PointerEvent<HTMLDivElement>): void;
  onPointerMove(event: PointerEvent<HTMLDivElement>): void;
  onPointerUp(event: PointerEvent<HTMLDivElement>): void;
  onPointerCancel(event: PointerEvent<HTMLDivElement>): void;
  onClickCapture(event: MouseEvent<HTMLDivElement>): void;
  onKeyDown(event: KeyboardEvent<HTMLDivElement>): void;
  onDoubleClick(event: MouseEvent<HTMLDivElement>): void;
}

/**
 * What the lanes do with a wheel, a mouse, a touch and a key, for `nav`: their handlers, and the
 * band a drag across the hours draws, from and to as fractions of the hours shown.
 */
export function useNavigation(
  lanes: RefObject<HTMLDivElement | null>,
  nav: Navigation,
): { band: [number, number] | null; handlers: LanesHandlers } {
  const latest = useRef(nav);
  latest.current = nav;
  // The view the next gesture moves on from: the one drawn, or one set since that is not drawn yet.
  const shown = useRef(nav.view);
  shown.current = nav.view;
  const press = useRef<Press | null>(null);
  const touches = useRef<Touches | null>(null);
  const swallow = useRef(false);
  const [band, setBand] = useState<[number, number] | null>(null);
  // The wheel's own listener, as React's is passive and cannot keep the page from scrolling.
  useEffect(() => {
    const element = lanes.current;
    if (!element) return;
    const wheel = (event: WheelEvent) => {
      const at = latest.current;
      const track = trackOf(element);
      if (track === null) return;
      const next = wheeled(
        shown.current,
        at.whole,
        track,
        {
          x: event.clientX,
          deltaX: event.deltaX,
          deltaY: event.deltaY,
          mode: event.deltaMode,
          zoom: event.ctrlKey || event.metaKey,
          shift: event.shiftKey,
        },
        at.cut,
      );
      // Each wheel event moves on from the last, though the page has not drawn it yet.
      taken(event, next, (view) => {
        shown.current = view;
        at.set(view);
      });
    };
    element.addEventListener("wheel", wheel, { passive: false });
    return () => element.removeEventListener("wheel", wheel);
  }, [lanes]);
  const set = (next: View | null, say?: boolean) => {
    shown.current = next;
    latest.current.set(next, say);
  };
  const touched = (event: PointerEvent<HTMLDivElement>) => {
    const at = latest.current;
    const held = touches.current;
    if (!at || !held?.start.has(event.pointerId)) return;
    held.now.set(event.pointerId, { x: event.clientX, y: event.clientY });
    const track = trackOf(lanes.current);
    if (track === null) return;
    const fingers = [...held.start.keys()].slice(0, 2);
    const of = (map: Map<number, Point>) => fingers.map((finger) => map.get(finger) as Point);
    if (fingers.length === 2) {
      const next = pinched(
        held.view,
        at.whole,
        track,
        of(held.start) as [Point, Point],
        of(held.now) as [Point, Point],
        at.cut,
      );
      if (next !== null) set(next.view);
      return;
    }
    const start = held.start.get(event.pointerId) as Point;
    held.swiping ??= sideways(event.clientX - start.x, event.clientY - start.y);
    if (!held.swiping) return;
    const next = swiped(held.view, at.whole, track, start.x, event.clientX, at.cut);
    if (next !== null) set(next.view);
  };
  const untouched = (id: number) => {
    const held = touches.current;
    if (!held) return;
    held.start.delete(id);
    held.now.delete(id);
    // The fingers left go on from where the view is now.
    for (const [finger, at] of held.now) held.start.set(finger, at);
    held.view = shown.current;
    held.swiping = null;
    if (held.start.size === 0) touches.current = null;
  };
  const handlers: LanesHandlers = {
    onPointerDown(event: PointerEvent<HTMLDivElement>) {
      if (event.pointerType === "touch") {
        const held = touches.current ?? {
          start: new Map(),
          now: new Map(),
          view: null,
          swiping: null,
        };
        const at = { x: event.clientX, y: event.clientY };
        for (const [finger, now] of held.now) held.start.set(finger, now);
        held.start.set(event.pointerId, at);
        held.now.set(event.pointerId, at);
        held.view = shown.current;
        held.swiping = null;
        touches.current = held;
        return;
      }
      swallow.current = false;
      press.current = null;
      setBand(null);
      if (event.button !== 0) return;
      const from = fractionAt(lanes.current, event.clientX);
      if (from === null || from < 0 || from > 1) return;
      press.current = { id: event.pointerId, from, x: event.clientX, dragging: false };
    },
    onPointerMove(event: PointerEvent<HTMLDivElement>) {
      if (event.pointerType === "touch") {
        touched(event);
        return;
      }
      const held = press.current;
      if (held === null || held.id !== event.pointerId) return;
      // A press let go outside the lanes before it dragged (the pointer is captured only once it
      // drags) ends with the button found up.
      if ((event.buttons & 1) === 0) {
        press.current = null;
        setBand(null);
        return;
      }
      if (!held.dragging) {
        if (Math.abs(event.clientX - held.x) < DRAG_PX) return;
        held.dragging = true;
        lanes.current?.setPointerCapture?.(event.pointerId);
      }
      const to = fractionAt(lanes.current, event.clientX);
      if (to !== null) setBand([held.from, Math.min(Math.max(to, 0), 1)]);
    },
    onPointerUp(event: PointerEvent<HTMLDivElement>) {
      if (event.pointerType === "touch") {
        untouched(event.pointerId);
        return;
      }
      const held = press.current;
      press.current = null;
      if (held === null || !held.dragging) return;
      setBand(null);
      swallow.current = true;
      const to = fractionAt(lanes.current, event.clientX);
      const at = latest.current;
      if (to === null || !at) return;
      set(dragged(shown.current, at.whole, held.from, to, at.cut));
    },
    onPointerCancel(event: PointerEvent<HTMLDivElement>) {
      if (event.pointerType === "touch") {
        untouched(event.pointerId);
        return;
      }
      press.current = null;
      setBand(null);
    },
    // The click a drag's release makes is the drag's: it picks no tick and focuses no part.
    onClickCapture(event: MouseEvent<HTMLDivElement>) {
      if (!swallow.current) return;
      swallow.current = false;
      event.preventDefault();
      event.stopPropagation();
    },
    onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
      zoomKey(event, { ...latest.current, view: shown.current, set });
    },
    // A double click on a stage's bar zooms to the stage, its instants on it.
    onDoubleClick(event: MouseEvent<HTMLDivElement>) {
      const bar = (event.target as HTMLElement).closest<HTMLElement>("[data-from][data-to]");
      const at = latest.current;
      if (!bar || !at) return;
      const from = Number(bar.dataset.from);
      const to = Number(bar.dataset.to);
      if (Number.isFinite(from) && Number.isFinite(to)) set(zoomTo(at.whole, from, to, at.cut));
    },
  };
  return { band, handlers };
}
