// A task's timeline drawn in its page's box (timeline-slot.tsx), a chunk the box imports once it
// nears the screen and the page has settled, so a task page's first visit carries none of it, nor
// its style sheet, which it puts in the page itself. It imports nothing of the box: it says each
// draw's height through `onDrawn`, which the box keeps for the task's next visit.
//
// It reads `/api/task-timeline` when it mounts, again when the task changes (its `revision`) or
// Try again is pressed, and each `every` seconds while the task is open and the page shown, asking
// through `getIf` (request.ts) with its box's signal and the validator of the last answer, so an
// unchanged timeline is a `304`. On a phone the box
// keeps the height the page gave it: the latest steps that fit show, a read that adds a step moves
// nothing outside it, and "Show all" lets it grow. Wide (from 900 px, where the lanes replace the
// list) it grows to hold every lane, with no "Show all".

import {
  type CSSProperties,
  type JSX,
  type KeyboardEvent,
  type Ref,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { Button } from "./button";
import { sleep } from "./feed";
import { scoped } from "./project";
import { Failure, getIf, NO_ANSWER as SLOW, said } from "./request";
import { currentRoute } from "./route";
import STYLE from "./timeline.css?inline";
import {
  type Bar,
  type Fit,
  failed,
  fitClass,
  keptNote,
  type Lanes,
  LONGEST_WAIT,
  lanes as laneLayout,
  legendStates,
  lookClass,
  nextRead,
  type Row,
  readTimeline,
  roomCss,
  rows as rowLayout,
  type Seg,
  type Shown,
  showAll,
  stops,
  totals,
} from "./timeline-layout";
import { TASK_TIMELINE_PATH, type TaskTimeline } from "./wire";
import { fixed } from "./words";

/** The id of the page's box the timeline is drawn in (timeline-slot.tsx's, not imported here). */
const SLOT = "timeline-slot";
/** The id of the part of the box the steps are drawn in, which the box clips to its height. */
const BODY = "tl-body";
/** The id of the timeline's style element, put in the page once. */
const STYLE_ID = "timeline-style";

/** What a read says when the server gave no answer in time. */
export const NO_ANSWER = "no answer from the server";
/** What a read says of an answer that is no timeline this client can read. */
export const NOT_A_TIMELINE = "the server's answer is not a timeline";

/** Put the timeline's style sheet in the page once: no first visit carries it. */
export function putStyle(): void {
  if (document.getElementById(STYLE_ID)) return;
  const element = document.createElement("style");
  element.id = STYLE_ID;
  element.textContent = STYLE;
  // Before the project's theme, as the shell links a kind's sheets: the theme's rules win.
  document.head.insertBefore(element, document.head.querySelector("[data-symphony-theme]"));
}

/** Whether the page is wide: from 900 px, where the style sheet draws the lanes, not the list. */
export function isWide(): boolean {
  return innerWidth >= 900;
}

// The reads.

/** What the reads left: the last answer's validator, and the server's clock at it beside this
 * browser's (a `304` moves the one by the other). */
export interface Kept {
  tag: string | null;
  at: [number, number];
}

/** What the host holds across its reads: what it shows and what the reads left. */
export interface Held {
  shown: Shown;
  kept: Kept;
}

export const NOTHING_HELD: Held = { shown: null, kept: { tag: null, at: [0, 0] } };

/** A read of the timeline: a new answer and its validator, the one kept (`304`), or why it failed. */
export type Read = { fresh: TaskTimeline; tag: string | null } | { same: true } | { error: string };

/** The address of `slug`'s timeline under the checkout its route names, as `getIf` asks it (under
 * the page's project). */
export function timelineUrl(slug: string): string {
  return scoped(currentRoute().scope, `${TASK_TIMELINE_PATH}?slug=${encodeURIComponent(slug)}`);
}

/**
 * A failed read in the timeline's own sentence's words, as the Dioxus timeline said them: the
 * server's of a refusal (its status where it gave none), the browser's of a fetch or a body that
 * failed (its message, never its stack), or that no answer came in time.
 */
export function readFailed(failure: unknown): string {
  if (!(failure instanceof Failure)) return said(failure);
  return failure.text === SLOW ? NO_ANSWER : failure.own;
}

/**
 * `slug`'s timeline, asked through `getIf` with `tag`, the last answer's validator, when there is
 * one: a read that has no answer in ANSWER_MS fails, and one `signal` ends is given up on the wire.
 */
export async function readOnce(
  slug: string,
  tag: string | null,
  signal: AbortSignal,
): Promise<Read> {
  try {
    const answer = await getIf(timelineUrl(slug), tag ?? "", signal);
    if (answer.same) return { same: true };
    const timeline = readTimeline(answer.value);
    if (timeline === null) return { error: NOT_A_TIMELINE };
    return { fresh: timeline, tag: answer.etag === "" ? null : answer.etag };
  } catch (failure) {
    return { error: readFailed(failure) };
  }
}

/**
 * What `held` becomes after `read`, at `clock` (this browser's, in ms): a new answer is shown and
 * kept; the one kept is shown again, its `now` moved on by this browser's clock since; a failure
 * is said only where no timeline is shown, which a failed read leaves as it is.
 */
export function afterRead(held: Held, read: Read, clock: number): Held {
  if ("fresh" in read) {
    return {
      shown: { timeline: read.fresh },
      kept: { tag: read.tag, at: [read.fresh.now, clock] },
    };
  }
  const shown = held.shown;
  const showing = shown !== null && "timeline" in shown;
  if ("same" in read) {
    if (!showing) return held;
    const [now, then] = held.kept.at;
    return {
      ...held,
      shown: { timeline: { ...shown.timeline, now: now + (clock - then) / 1000 } },
    };
  }
  return showing ? held : { ...held, shown: { error: read.error } };
}

/** Once the page is shown: at once if it is, else at the first change of visibility that shows
 * it, or once `signal` ends. */
export function whenShown(signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    if (typeof document === "undefined" || !document.hidden || signal.aborted) return resolve();
    const heard = () => {
      if (!document.hidden) done();
    };
    const done = () => {
      document.removeEventListener("visibilitychange", heard);
      signal.removeEventListener("abort", done);
      resolve();
    };
    document.addEventListener("visibilitychange", heard);
    signal.addEventListener("abort", done);
  });
}

/**
 * Read `slug`'s timeline into `held` and say what it shows (`drawn`), and again after `nextRead`'s
 * seconds while the task is open (a minute after a failed read), waiting while the page is hidden
 * and reading once it shows again, until `signal` ends.
 */
export async function follow(
  slug: string,
  signal: AbortSignal,
  held: { current: Held },
  drawn: (shown: Shown) => void,
): Promise<void> {
  while (!signal.aborted) {
    const read = await readOnce(slug, held.current.kept.tag, signal);
    if (signal.aborted) return;
    held.current = afterRead(held.current, read, Date.now());
    drawn(held.current.shown);
    const every = nextRead(held.current.shown);
    if (every === null) return;
    await sleep(Math.min(every * 1000, LONGEST_WAIT), signal);
    await whenShown(signal);
  }
}

// The measuring.

/** What a draw is measured by: an element's box. */
interface Boxed {
  getBoundingClientRect(): { height: number };
}

/** The part of the box the steps are drawn in. */
interface Steps extends Boxed {
  clientHeight: number;
  querySelector(selector: string): Boxed | null;
}

/**
 * After a draw: whether the steps overflow the box (and so "Show all" is offered, on a phone), and
 * unless the reader asked to see `all`, the height the timeline took, kept (`keep`) for the task's
 * next visit so its box is reserved as tall. The steps are measured by what is drawn of them (the
 * list on a phone, the lanes `wide`), never by the part that holds them, which is at least as tall
 * as the box leaves it. Wide the box grows to hold every lane, so they never overflow, and the
 * height kept is the whole timeline's.
 */
export function settle(
  body: Steps | null,
  slot: Boxed | null,
  wide: boolean,
  all: boolean,
  keep: (wide: boolean, natural: number) => void,
): boolean {
  if (body === null || slot === null) return false;
  const drawn = body.querySelector(wide ? ".tl-lanes" : ".tl-list")?.getBoundingClientRect().height;
  const steps = drawn ?? 0;
  const held = body.clientHeight;
  if (!all) {
    const height = slot.getBoundingClientRect().height;
    keep(wide, wide ? height : height - held + steps);
  }
  return steps > held + 1;
}

/** The part of the box the steps are drawn in, as it is found in the page's box. */
interface Placed extends Steps {
  closest(selector: string): Boxed | null;
}

/**
 * After a draw of `steps` in the page's box: whether the steps are clipped now, to be set, or none
 * where it stays as it is. Showing `all`, the box has grown to hold every step and says nothing of
 * whether its own height clips them, so "Show the latest" stays the same button, with the focus
 * still on it, until the box is measured again at its own height.
 */
export function measured(
  steps: Placed | null,
  all: boolean,
  clipped: boolean,
  keep: (wide: boolean, natural: number) => void,
): boolean | null {
  const over = settle(steps, steps?.closest(`#${SLOT}`) ?? null, isWide(), all, keep);
  return !all && clipped !== over ? over : null;
}

/** Call `crossed` each time the page crosses the width the lanes replace the list at, either way,
 * so a draw measured on one side is measured again on the other; what it returns stops it. */
export function watchCrossings(crossed: () => void): () => void {
  let was = isWide();
  const resized = () => {
    if (isWide() !== was) {
      was = !was;
      crossed();
    }
  };
  addEventListener("resize", resized);
  return () => removeEventListener("resize", resized);
}

/** The lanes' part the keyboard goes to from part `index` of `count` on `key`, or none for a key
 * that walks nothing. */
export function stopFor(key: string, index: number, count: number): number | null {
  const last = Math.max(count - 1, 0);
  switch (key) {
    case "ArrowRight":
    case "ArrowDown":
      return Math.min(index + 1, last);
    case "ArrowLeft":
    case "ArrowUp":
      return Math.max(index - 1, 0);
    case "Home":
      return 0;
    case "End":
      return last;
    default:
      return null;
  }
}

/** The drawn timeline's classes: showing all of it, clipped (drawn from its end on a phone, its
 * latest steps in view whatever room the box's foot leaves them), or neither. */
export function drawnClass(all: boolean, clipped: boolean): string {
  if (all) return "tl-drawn tl-all";
  return clipped ? "tl-drawn tl-clipped" : "tl-drawn";
}

// The views.

/** A style of custom properties and others, written as the Dioxus page wrote it. */
const style = (properties: Record<string, string>) => properties as CSSProperties;
const percent = (value: number) => `${fixed(value, 2)}%`;

/** What a part of the lanes the keyboard stops at does: its focus, its blur, its keys. */
export interface Stops {
  active: number;
  focus(index: number, title: string): void;
  blur(): void;
  key(event: KeyboardEvent<HTMLElement>, index: number, count: number): void;
}

/**
 * The lanes' stops with the part `active` the keyboard stops at: a part that takes the focus is
 * the stop (`choose`) and its words the detail (`tell`), which leaves with the focus; an arrow key,
 * Home or End moves the focus to the part it names, and no key else does anything.
 */
export function stopsOf(
  active: number,
  choose: (index: number) => void,
  tell: (detail: string) => void,
): Stops {
  return {
    active,
    focus(index, title) {
      choose(index);
      tell(title);
    },
    blur() {
      tell("");
    },
    key(event, index, count) {
      const to = stopFor(event.key, index, count);
      if (to === null) return;
      event.preventDefault();
      choose(to);
      document.getElementById(`tl-stop-${to}`)?.focus();
    },
  };
}

/**
 * The attributes and handlers of the lanes' part `index` of `count`: one stop of one arrow-key
 * group (the first, until another is chosen), its words for assistive technology, and its whole
 * detail shown in the legend's place under the lanes while it has the focus.
 */
function stopOf(index: number, count: number, title: string, at: Stops) {
  return {
    id: `tl-stop-${index}`,
    tabIndex: at.active === index ? 0 : -1,
    role: "img",
    "aria-label": title,
    onFocus: () => at.focus(index, title),
    onBlur: () => at.blur(),
    onKeyDown: (event: KeyboardEvent<HTMLElement>) => at.key(event, index, count),
  };
}

function FitView({ fit }: { fit: Fit }) {
  return <span className={fitClass(fit)}>{fit.text}</span>;
}

function BarView({ bar, word }: { bar: Bar; word: [string, string] | null }) {
  return (
    <span className="tl-bars">
      {bar.wait > 0 && (
        <span
          className="tl-wait"
          aria-hidden="true"
          style={style({ "--c": `var(--${bar.waitTone})`, width: percent(bar.wait) })}
        />
      )}
      {bar.work > 0 && (
        <span
          className={bar.running ? "tl-work tl-running" : "tl-work"}
          aria-hidden="true"
          style={style({ width: percent(bar.work) })}
        />
      )}
      {word && (
        <span className="tl-word" style={style({ "--c": `var(--${word[1]})` })}>
          {word[0]}
        </span>
      )}
    </span>
  );
}

function RowView({ row }: { row: Row }) {
  const kind = !row.sub ? "tl-row" : row.event ? "tl-row tl-sub tl-event" : "tl-row tl-sub";
  return (
    <>
      {row.day !== null && <li className="tl-day">{row.day}</li>}
      <li className={kind} style={style({ "--c": `var(--${row.tone})` })} title={row.title}>
        <span className="tl-at">{row.at}</span>
        <span className="tl-mark" aria-hidden="true" />
        <span className="tl-label">{row.label}</span>
        {row.took !== null && <span className="tl-took">{row.took}</span>}
        {row.bar !== null && <BarView bar={row.bar} word={row.word} />}
      </li>
    </>
  );
}

function SegView({ seg, stop }: { seg: Seg; stop: object }) {
  const right = seg.left + seg.width;
  const placed = style({
    "--c": `var(--${seg.tone})`,
    "--row": String(seg.row),
    left: percent(seg.left),
    width: percent(seg.width),
  });
  return (
    <>
      <span className={lookClass(seg.look)} style={placed} title={seg.title} {...stop}>
        {seg.text.map((fit, at) => (
          // biome-ignore lint/suspicious/noArrayIndexKey: a segment's words have no identity but their place
          <FitView key={at} fit={fit} />
        ))}
      </span>
      {seg.after && (
        <span
          className={`tl-after ${fitClass(seg.after[0])}`}
          aria-hidden="true"
          style={style({
            "--c": `var(--${seg.after[1]})`,
            left: percent(right),
            maxWidth: roomCss(seg.after[2]),
          })}
        >
          {seg.after[0].text}
        </span>
      )}
      {seg.under && (
        <span
          className={`tl-under ${fitClass(seg.under[0])}`}
          aria-hidden="true"
          style={style({
            "--row": String(seg.row),
            left: percent(seg.left),
            maxWidth: roomCss(seg.under[1]),
          })}
        >
          {seg.under[0].text}
        </span>
      )}
    </>
  );
}

function LanesView({ lanes, at }: { lanes: Lanes; at: Stops }) {
  const count = stops(lanes);
  let index = 0;
  const next = () => index++;
  const laneRows = lanes.lanes.map((lane) => (
    <div
      key={lane.key}
      className="tl-lane"
      data-lane={lane.key}
      style={style({ "--rows": String(lane.rows) })}
    >
      <span className="tl-name">
        <b>{lane.name}</b>
        {lane.who !== "" && <small>{lane.who}</small>}
      </span>
      <span className="tl-track">
        {lane.segs.map((seg, place) => (
          <SegView
            // biome-ignore lint/suspicious/noArrayIndexKey: a lane's segments have no identity but their place
            key={place}
            seg={seg}
            stop={seg.stop ? stopOf(next(), count, seg.title, at) : { "aria-hidden": "true" }}
          />
        ))}
      </span>
    </div>
  ));
  const gateRows = lanes.gates.map((gate, place) => (
    // biome-ignore lint/suspicious/noArrayIndexKey: a gate's row has no identity but its place
    <div key={place} className="tl-lane tl-gate" data-gate={gate.name}>
      <span className="tl-name">{gate.name}</span>
      <span className="tl-track">
        {gate.spans.map((span, spanAt) => (
          <span
            // biome-ignore lint/suspicious/noArrayIndexKey: a gate's states have no identity but their place
            key={spanAt}
            className={`tl-g tl-g-${span.state}`}
            style={style({ left: percent(span.left), width: percent(span.width) })}
            title={span.title}
            {...stopOf(next(), count, span.title, at)}
          >
            {span.word && <FitView fit={span.word} />}
          </span>
        ))}
      </span>
    </div>
  ));
  return (
    // biome-ignore lint/a11y/useSemanticElements: a group of the lanes' parts, as the Dioxus page draws it
    <div className="tl-lanes" role="group" aria-label="Lanes">
      <div className="tl-hours" aria-hidden="true">
        {lanes.ticks.map((tick, at) => (
          // biome-ignore lint/suspicious/noArrayIndexKey: an hour's line has no identity but its place
          <span key={at} className="tl-hour" style={style({ left: percent(tick.left) })} />
        ))}
        {lanes.now && <span className="tl-now" style={style({ left: percent(lanes.now.left) })} />}
      </div>
      <div className="tl-lane tl-axis" aria-hidden="true">
        <span className="tl-name" />
        <span className="tl-track">
          {lanes.ticks.map((tick, at) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: an hour's label has no identity but its place
            <span key={at} className="tl-tick" style={style({ left: percent(tick.left) })}>
              {tick.label}
            </span>
          ))}
          {lanes.now && (
            <span className="tl-tick tl-nowtag" style={style({ left: percent(lanes.now.left) })}>
              {lanes.now.label}
            </span>
          )}
        </span>
      </div>
      {laneRows}
      {lanes.gates.length > 0 && (
        <>
          <div className="tl-lane tl-gates-head">
            <span className="tl-name">
              <b>Gates</b>
            </span>
          </div>
          {gateRows}
        </>
      )}
    </div>
  );
}

export interface DrawnProps {
  timeline: TaskTimeline;
  /** The reader asked to see all of it (a phone's "Show all"). */
  all: boolean;
  /** Its steps overflow the box. */
  clipped: boolean;
  /** The words of the lanes' part with the focus, shown in the legend's place; none when empty. */
  detail: string;
  /** The part the keyboard stops at, and what each does. */
  stops: Stops;
  /** Shows all of it, or the latest again. */
  toggle: () => void;
  /** The part of the box the steps are drawn in. */
  body?: Ref<HTMLDivElement> | undefined;
}

/**
 * The timeline drawn in its box: its totals, the steps (the phone's list and the wide lanes, the
 * style sheet shows one and keeps the list for assistive technology), what the bars mean, and on a
 * phone "Show all" when the steps overflow the box.
 */
export function TimelineDrawn({
  timeline,
  all,
  clipped,
  detail,
  stops: at,
  toggle,
  body,
}: DrawnProps) {
  const [first, machine] = totals(timeline);
  const note = keptNote(timeline);
  const steps = rowLayout(timeline);
  const drawn = laneLayout(timeline);
  const running = timeline.runs.some((run) => run.result === "running");
  return (
    <div className={drawnClass(all, clipped)}>
      <div className="tl-head">
        <p className="tl-totals">
          <span>{first}</span>
          {machine !== null && (
            <>
              <span className="tl-machine tl-narrow">{`Machine ${machine}`}</span>
              <span className="tl-machine tl-wide">{` · machine ${machine}`}</span>
            </>
          )}
        </p>
        {note !== null && <p className="tl-note">{note}</p>}
      </div>
      <div className="tl-body" id={BODY} ref={body}>
        <ol className="tl-list" aria-label="Steps">
          {steps.map((row, place) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: a step has no identity but its place
            <RowView key={place} row={row} />
          ))}
        </ol>
        <LanesView lanes={drawn} at={at} />
      </div>
      <div className="tl-foot">
        <ul className="tl-legend">
          <li>
            <span className="tl-key tl-key-work" />
            worked or ran
          </li>
          <li>
            <span className="tl-key tl-key-wait" />
            queued or waiting
          </li>
          {running && (
            <li>
              <span className="tl-key tl-key-running" />
              running now
            </li>
          )}
          {legendStates(drawn).map((state) => (
            <li key={state} className="tl-wide">
              <span className={`tl-key tl-g tl-g-${state}`} />
              {`gate ${state}`}
            </li>
          ))}
        </ul>
        {/* Only while a part of the lanes has the focus, in the legend's place: a line of its own
            would grow the box and move what is under it. */}
        {detail !== "" && (
          <p className="tl-detail tl-wide" aria-hidden="true">
            {detail}
          </p>
        )}
        {/* A phone's alone: wide the style sheet hides it, the box holding every lane. */}
        {(clipped || all) && (
          <Button className="tl-more" type="button" aria-expanded={all} onClick={toggle}>
            {all ? "Show the latest" : showAll(steps.length)}
          </Button>
        )}
      </div>
    </div>
  );
}

export interface ViewProps {
  timeline: TaskTimeline;
  /** Told each draw's height at its width (wide or not), unless the reader asked to see all. */
  onDrawn?: ((wide: boolean, natural: number) => void) | undefined;
}

/**
 * The timeline drawn and measured: after each draw, each crossing of 900 px and each "Show all"
 * or "Show the latest", whether its steps overflow the box, and the height it took.
 */
export function TimelineView({ timeline, onDrawn }: ViewProps): JSX.Element {
  const [all, setAll] = useState(false);
  const [clipped, setClipped] = useState(false);
  const [detail, setDetail] = useState("");
  const [active, setActive] = useState(0);
  const [crossings, setCrossings] = useState(0);
  const body = useRef<HTMLDivElement>(null);
  const said = useRef(onDrawn);
  said.current = onDrawn;
  useEffect(() => watchCrossings(() => setCrossings((count) => count + 1)), []);
  // biome-ignore lint/correctness/useExhaustiveDependencies: measured again for each answer drawn and each crossing of 900 px
  useLayoutEffect(() => {
    const over = measured(body.current, all, clipped, (wide, natural) =>
      said.current?.(wide, natural),
    );
    if (over !== null) setClipped(over);
  }, [timeline, crossings, all, clipped]);
  return (
    <TimelineDrawn
      timeline={timeline}
      all={all}
      clipped={clipped}
      detail={detail}
      stops={stopsOf(active, setActive, setDetail)}
      toggle={() => setAll((shown) => !shown)}
      body={body}
    />
  );
}

export interface HostProps {
  slug: string;
  /** The task's `updated`: a change reads the timeline again at once. */
  revision: number;
  /** Told each draw's natural height at its width, for the task's next visit. */
  onDrawn?: (wide: boolean, natural: number) => void;
}

/**
 * The timeline of `slug`: read when it mounts, again whenever the task changes (`revision`) or
 * Try again is pressed, and each `every` seconds while the task is open and the page shown. A read
 * that fails after one that did not leaves the one shown. Before its first answer it draws
 * nothing: the box's placeholder shows.
 */
export function TimelineHost({ slug, revision, onDrawn }: HostProps): JSX.Element | null {
  const [shown, setShown] = useState<Shown>(null);
  const [tries, setTries] = useState(0);
  const held = useRef<Held>(NOTHING_HELD);
  useLayoutEffect(putStyle, []);
  // biome-ignore lint/correctness/useExhaustiveDependencies: each revision and each Try again starts the reads afresh
  useEffect(() => {
    const reading = new AbortController();
    void follow(slug, reading.signal, held, setShown);
    return () => reading.abort();
  }, [slug, revision, tries]);
  if (shown === null) return null;
  if ("error" in shown) {
    return <TimelineFailed error={shown.error} retry={() => setTries((count) => count + 1)} />;
  }
  return <TimelineView timeline={shown.timeline} onDrawn={onDrawn} />;
}

/**
 * A read that failed with no timeline shown, said in the box's place quietly with its Try again
 * (`retry`): the page's own errors are its notice stack's, drawn once outside its flow.
 */
export function TimelineFailed({ error, retry }: { error: string; retry: () => void }) {
  return (
    <div className="tl-drawn tl-failed">
      <p className="tl-note">
        {`${failed(error)} `}
        <Button type="button" onClick={retry}>
          Try again
        </Button>
      </p>
    </div>
  );
}
