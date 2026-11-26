// A task's timeline drawn in its page's box (timeline-slot.tsx), a chunk the box imports once it
// nears the screen and the page has settled, so a task page's first visit carries none of it, nor
// its style sheet, which it puts in the page itself. It imports nothing of the box: it says each
// draw's height through `onDrawn`, which the box keeps for the task's next visit.
//
// Its answer is a part of the page (task-live.ts's `timelinePart`, in the page's store), asked as
// the page mounts, so it is there, or on its way, when this chunk comes: the page reads it again
// when the task changes and tries a failed read again by itself, and the timeline asks for it
// again each `every` seconds while the task is open and the page shown, an unchanged timeline a
// `304` that moves its clock on. On a phone the box
// keeps the height the page gave it: the latest steps that fit show, a read that adds a step moves
// nothing outside it, and "Show all" lets it grow. Wide (from 900 px, where the lanes replace the
// list) it grows to hold every lane, with no "Show all".
//
// The task's tokens are another part of the page (task-live.ts's `tokensPart`), which the box
// hands on as it came: once read, the task's estimated cost follows its total time and each stage
// stretch's its own (timeline-cost.ts); until then, and for a task with none, no cost is drawn.

import {
  type CSSProperties,
  type JSX,
  type KeyboardEvent,
  type Ref,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";
import { Button } from "./button";
import { whenShown } from "./feed";
import type { Shown as Answer } from "./parts";
import { Failure, said } from "./request";
import STYLE from "./timeline.css?inline";
import { carried, costFigure, readTaskTokens, tookTokens, totalWords } from "./timeline-cost";
import { address, addressAt, endpointLane, historyWords, messagesOf } from "./timeline-history";
import {
  type Bar,
  type Fit,
  failed,
  fitClass,
  insetCss,
  type Lanes,
  LONGEST_WAIT,
  lanes as laneLayout,
  legendStates,
  lookClass,
  machineSeconds,
  nextRead,
  placeTotals,
  type Row,
  readTimeline,
  roomCss,
  rows as rowLayout,
  Scale,
  type Seg,
  type Shown,
  showAll,
  stops,
  took,
  totalParts,
  totals,
} from "./timeline-layout";
import { MessagePanel, type MessageSelection, type ThreadAccess } from "./timeline-messages";
// biome-ignore lint/style/noRestrictedImports: no-timed-waits baseline: readAgain reads the timeline again each period while the task is open, where the feed's event of a change, or a clock tick for the times shown, is the wait
import { sleep } from "./timing";
import { NOT_IN_TIME } from "./trouble";
import type { TaskTimeline, TaskTokens, TimelineMessage } from "./wire";
import { fixed } from "./words";

/** The id of the page's box the timeline is drawn in (timeline-slot.tsx's, not imported here). */
const SLOT = "timeline-slot";
/** The id of the part of the box the steps are drawn in, which the box clips to its height. */
const BODY = "tl-body";
/** The id of the timeline's style element, put in the page once. */
const STYLE_ID = "timeline-style";

/** What a read says when the server gave no answer in time: the shared words, within a sentence. */
export const NO_ANSWER = "the server did not answer in time";
/** What a read says of an answer that is no timeline this client can read. */
export const NOT_A_TIMELINE = "the server's answer is not a timeline";
/** What a read says where the server has no timeline for the task: nothing tries it again. */
export const NO_TIMELINE = "the board has no timeline for this task";

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

/**
 * A failed read in the timeline's own sentence's words, as the Dioxus timeline said them: the
 * server's of a refusal (its status where it gave none), the browser's of a fetch or a body that
 * failed (its message, never its stack), or that no answer came in time.
 */
export function readFailed(failure: unknown): string {
  if (!(failure instanceof Failure)) return said(failure);
  return failure.text === NOT_IN_TIME ? NO_ANSWER : failure.own;
}

/** The answer the timeline last drew anew, and this browser's clock (ms) when it first did. */
export interface Seen {
  value: unknown;
  at: number;
}

/**
 * What the timeline's place shows of its `part` at `clock` (this browser's, in ms): the answer,
 * its `now` moved on by this browser's clock since it was first drawn (`seen`), as the server's
 * own is not read again while nothing changed; why its read failed, only where no timeline is
 * shown, or that the server has none (which nothing tries again); nothing before either.
 */
export function shownOf(part: Answer<unknown>, seen: Seen | null, clock: number): Shown {
  if (part.value === undefined) {
    if (part.missing) return { error: NO_TIMELINE, final: true };
    return part.error === null ? null : { error: readFailed(part.error) };
  }
  const timeline = readTimeline(part.value);
  if (timeline === null) return { error: NOT_A_TIMELINE };
  const since = seen !== null && seen.value === part.value ? (clock - seen.at) / 1000 : 0;
  return { timeline: since > 0 ? { ...timeline, now: timeline.now + since } : timeline };
}

/**
 * Read the timeline again (`read`) after each of `every`'s seconds while it gives some (the task
 * is open), waiting while the page is hidden and reading once it shows again, and say this
 * browser's clock after each read (`drawn`), until `signal` ends.
 */
export async function readAgain(
  every: () => number | null,
  read: () => Promise<unknown>,
  drawn: (clock: number) => void,
  signal: AbortSignal,
): Promise<void> {
  while (!signal.aborted) {
    const seconds = every();
    if (seconds === null) return;
    await sleep(Math.min(seconds * 1000, LONGEST_WAIT), signal);
    await whenShown(signal);
    if (signal.aborted) return;
    await read();
    if (signal.aborted) return;
    drawn(Date.now());
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
  /** Words shown as the detail for what has the focus outside the lanes' parts (the total's
   * cost), which leave with it. */
  say(detail: string): void;
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
    say: tell,
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

/** A build's or a test's row: a tap (or a script) puts the focus on it, in no tab order, the lanes'
 * parts being the keyboard's; while it has it, the style sheet shows all of the run under its bar. */
const isRun = (row: Row) => row.sub && !row.event;
/** A row a tap says all of: a run's, and a stage's that carries a cost, whose words say whose
 * calls it was and when they ran. */
const isTold = (row: Row) => isRun(row) || row.cost !== undefined;

function RowView({
  row,
  selection,
  wide,
}: {
  row: Row;
  selection?: MessageSelection | undefined;
  wide: boolean;
}) {
  const kind = !row.sub
    ? "tl-row"
    : row.message !== undefined
      ? "tl-row tl-sub tl-event tl-message"
      : row.kept
        ? "tl-row tl-sub tl-event tl-kept"
        : row.event
          ? "tl-row tl-sub tl-event"
          : "tl-row tl-sub";
  const told = isTold(row);
  return (
    <>
      {row.day !== null && <li className="tl-day">{row.day}</li>}
      <li
        className={kind}
        style={style({ "--c": `var(--${row.tone})` })}
        title={row.title}
        tabIndex={told ? -1 : undefined}
      >
        <span className="tl-at">{row.at}</span>
        <span className="tl-mark" aria-hidden="true" />
        {row.message !== undefined ? (
          <button
            type="button"
            className="tl-label tl-message-row"
            tabIndex={wide ? -1 : 0}
            data-message={row.message}
            aria-pressed={selection?.selected === row.message}
            onClick={() => selection?.choose(row.message as number)}
          >
            {row.label}
          </button>
        ) : (
          <span className="tl-label">{row.label}</span>
        )}
        {(row.took !== null || row.cost !== undefined) && (
          <span className="tl-took">{[row.took, row.cost].filter(Boolean).join(" ")}</span>
        )}
        {row.bar !== null && <BarView bar={row.bar} word={row.word} />}
        {told && (
          <span className="tl-facts" aria-hidden="true">
            {row.title}
          </span>
        )}
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
            paddingLeft: insetCss(seg.after[2]),
          })}
        >
          {seg.after[0].text}
        </span>
      )}
    </>
  );
}

/** The lanes of runs, which the time no run is kept of is hatched over. */
const RUNS = ["builds", "tests"];

/** A drawn box, as `getBoundingClientRect` gives it. */
interface Box {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** A selected message's marks in their layer: its tick's x; the y of its sender's line, where its
 * tick is, and of its recipient's; and the y of the hours line's foot, where its guide starts;
 * each from the layer's corner. */
interface ArrowPlace {
  left: number;
  from: number;
  to: number;
  top: number;
}

/** How far above a ring around a tick, in pixels, its guide stops: the ring's radius and a gap. */
const RING_CLEAR = 11;

/**
 * Where the selected message's marks are drawn in `layer`: through the middle of its tick (`mark`,
 * on its sender's line), to the middle of its recipient's line (`rail`), each as drawn, so the
 * arrow meets both marks wherever the lanes put them, and its guide down from the hours' foot
 * (`hours`). None while the tick or the line is not drawn.
 */
export function arrowPlace(
  layer: Box,
  mark: Box | undefined,
  rail: Box | undefined,
  hours: Box | undefined,
): ArrowPlace | null {
  if (!mark || !rail) return null;
  return {
    left: mark.left + mark.width / 2 - layer.left,
    from: mark.top + mark.height / 2 - layer.top,
    to: rail.top + rail.height / 2 - layer.top,
    top: hours === undefined ? 0 : hours.top + hours.height - layer.top,
  };
}

/**
 * The selected message's arrow, from its tick straight to a ring on its recipient's line at the
 * same instant, and a dashed guide from the hours line, where its address is said, down to it. A
 * message whose two ends are one lane draws no arrow: a ring around its tick, so an arrow always
 * joins two lanes.
 */
function AddressArrow({ selected, at }: { selected: TimelineMessage; at: number }) {
  const root = useRef<HTMLDivElement>(null);
  const [arrow, setArrow] = useState<ArrowPlace | null>(null);
  const from = endpointLane(selected.from, selected.sender);
  const to = endpointLane(selected.to, selected.recipient);
  // biome-ignore lint/correctness/useExhaustiveDependencies: the tick moves with its time's place on the hours (`at`), which the effect reads from the page
  useLayoutEffect(() => {
    const layer = root.current;
    const lanes = layer?.closest(".tl-lanes");
    if (!layer || !lanes) {
      setArrow(null);
      return;
    }
    const measure = () =>
      setArrow(
        arrowPlace(
          layer.getBoundingClientRect(),
          lanes
            .querySelector(`.tl-message-mark[data-message="${selected.id}"]`)
            ?.getBoundingClientRect(),
          lanes.querySelector(`[data-lane="${to}"] .tl-rail`)?.getBoundingClientRect(),
          lanes.querySelector(".tl-axis")?.getBoundingClientRect(),
        ),
      );
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(lanes);
    return () => observer.disconnect();
  }, [selected, at, to]);
  const same = from === to;
  const upper =
    arrow === null ? 0 : same ? arrow.from - RING_CLEAR : Math.min(arrow.from, arrow.to);
  return (
    <div className="tl-address-track" aria-hidden="true" ref={root}>
      {arrow && (
        <>
          <span
            className="tl-address-guide"
            style={{ left: arrow.left, top: arrow.top, height: Math.max(upper - arrow.top, 0) }}
          />
          {same ? (
            <span
              className="tl-address-ring tl-ring-around"
              data-at={from}
              style={{ left: arrow.left, top: arrow.from }}
            />
          ) : (
            <>
              <span
                className={`tl-address-arrow ${arrow.to < arrow.from ? "tl-arrow-up" : ""}`}
                data-from={from}
                data-to={to}
                style={{
                  left: arrow.left,
                  top: Math.min(arrow.from, arrow.to),
                  height: Math.abs(arrow.to - arrow.from),
                }}
              />
              <span className="tl-address-ring" style={{ left: arrow.left, top: arrow.to }} />
            </>
          )}
        </>
      )}
    </div>
  );
}

function LanesView({
  lanes,
  at,
  timeline,
  selection,
}: {
  lanes: Lanes;
  at: Stops;
  timeline: TaskTimeline;
  selection?: MessageSelection | undefined;
}) {
  const scale = new Scale(timeline);
  const selected = selection?.messages.find((m) => m.id === selection.selected);
  const count = stops(lanes);
  const notKept = lanes.notKept;
  let index = 0;
  const next = () => index++;
  const laneRows = lanes.lanes.map((lane) => (
    <div
      key={lane.key}
      className="tl-lane"
      data-lane={lane.key}
      data-rail={lane.rail ? "" : undefined}
      style={style({ "--rows": String(lane.rows) })}
    >
      <span className="tl-name">
        <b>{lane.name}</b>
        {lane.who !== "" && <small>{lane.who}</small>}
      </span>
      <span className="tl-track">
        {notKept && RUNS.includes(lane.key) && (
          <span
            className="tl-notkept"
            aria-hidden="true"
            title={notKept.title}
            style={style({ left: percent(notKept.left), width: percent(notKept.width) })}
          >
            {notKept.label && <FitView fit={notKept.label} />}
          </span>
        )}
        {lane.segs.map((seg, place) => (
          <SegView
            // biome-ignore lint/suspicious/noArrayIndexKey: a lane's segments have no identity but their place
            key={place}
            seg={seg}
            stop={seg.stop ? stopOf(next(), count, seg.title, at) : { "aria-hidden": "true" }}
          />
        ))}
        {lane.rail && <span className="tl-rail" aria-hidden="true" />}
        {lane.rail &&
          selection?.messages
            .filter((m) => endpointLane(m.from, m.sender) === lane.key)
            .map((m) => (
              <button
                key={m.id}
                type="button"
                className="tl-message-mark"
                data-message={m.id}
                style={{ left: percent(scale.at(m.at)) }}
                aria-label={`Message #${m.id}: ${address(m)}`}
                aria-pressed={selection.selected === m.id}
                onClick={() => selection.choose(m.id)}
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
          {selected && (
            // Its address on the hours line, over the hours' words, kept inside the track: it
            // moves left of its instant by as much of its width as its instant is of the hours.
            <span
              className="tl-address-said"
              style={style({
                left: percent(scale.at(selected.at)),
                transform: `translateX(-${percent(scale.at(selected.at))})`,
              })}
            >
              {addressAt(selected, scale.clock(selected.at))}
            </span>
          )}
        </span>
      </div>
      {laneRows}
      {selected && <AddressArrow selected={selected} at={scale.at(selected.at)} />}
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
  /** The task's tokens, once read; none before, or where the read gave none. */
  tokens?: TaskTokens | null | undefined;
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
  selection?: MessageSelection | undefined;
  /** The phone list stays readable by assistive technology wide, without invisible tab stops. */
  wide?: boolean;
}

/**
 * The timeline drawn in its box: its totals, the steps (the phone's list and the wide lanes, the
 * style sheet shows one and keeps the list for assistive technology), what the bars mean, and on a
 * phone "Show all" when the steps overflow the box.
 */
export function TimelineDrawn({
  timeline,
  tokens,
  all,
  clipped,
  detail,
  stops: at,
  toggle,
  body,
  selection,
  wide = false,
}: DrawnProps) {
  const [first, machine] = totals(timeline);
  const [length, rest] = totalParts(timeline);
  // A task that took no tokens says no cost anywhere, and draws as one whose tokens are not read.
  const spent = tokens && tookTokens(tokens) ? tokens : null;
  const costs = spent
    ? {
        stages: carried(timeline.stages, spent.rows, timeline.slug),
        prices: spent.cost.prices,
      }
    : undefined;
  const words = spent ? totalWords(spent) : "";
  // The clock's time at each phase and state, apart from the machine's on runs.
  const places = [
    ["Phases", placeTotals(timeline, "phase")],
    ["States", placeTotals(timeline, "state")],
  ] as const;
  const steps = rowLayout(timeline, costs);
  const drawn = laneLayout(timeline, costs);
  const running = timeline.runs.some((run) => run.result === "running");
  return (
    <div className={drawnClass(all, clipped)} data-selected={selection?.selected ?? undefined}>
      <div className="tl-head">
        <p className="tl-totals">
          {spent ? (
            <span>
              {`${length} `}
              {/* The task's estimated cost after its time; its words say the rest: in the legend's
                  place wide, under the totals on a phone, while it has the focus. */}
              <span
                className="tl-cost"
                role="img"
                // biome-ignore lint/a11y/noNoninteractiveTabindex: a part whose words its focus shows, by key or tap, as the lanes' parts are
                tabIndex={0}
                title={words}
                aria-label={words}
                onFocus={() => at.say(words)}
                onBlur={() => at.blur()}
              >
                {costFigure(spent.cost)}
              </span>
              <span className="tl-rest tl-wide">{rest}</span>
            </span>
          ) : (
            <span>{first}</span>
          )}
          {/* A phone's second line with a cost on the first, which leaves the rounds no room there:
              the rounds and bounces, then the machine's time. */}
          {spent && (
            <span className="tl-machine tl-narrow">
              {rest.slice(" · ".length)}
              {machine !== null && ` · machine ${took(machineSeconds(timeline))}`}
            </span>
          )}
          {machine !== null && (
            <>
              {!spent && <span className="tl-machine tl-narrow">{`Machine ${machine}`}</span>}
              <span className="tl-machine tl-wide">{` · machine ${machine}`}</span>
            </>
          )}
        </p>
        {spent && (
          <p className="tl-note tl-cost-said" aria-hidden="true">
            {words}
          </p>
        )}
        {places.map(
          ([name, spent]) =>
            spent !== null && <p key={name} className="tl-totals">{`${name}: ${spent}`}</p>,
        )}
      </div>
      <div className="tl-body" id={BODY} ref={body}>
        <ol className="tl-list" aria-label="Steps">
          {steps.map((row, place) => (
            <RowView
              key={row.message === undefined ? place : `message-${row.message}`}
              row={row}
              selection={selection}
              wide={wide}
            />
          ))}
        </ol>
        <LanesView lanes={drawn} at={at} timeline={timeline} selection={selection} />
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
  tokens?: TaskTokens | null | undefined;
  /** Told each draw's height at its width (wide or not), unless the reader asked to see all. */
  onDrawn?: ((wide: boolean, natural: number) => void) | undefined;
  thread?: ThreadAccess | undefined;
  details?: HTMLElement | null | undefined;
}

/**
 * The timeline drawn and measured: after each draw, each crossing of 900 px and each "Show all"
 * or "Show the latest", whether its steps overflow the box, and the height it took.
 */
export function TimelineView({
  timeline,
  tokens,
  onDrawn,
  thread,
  details,
}: ViewProps): JSX.Element {
  const [selected, setSelected] = useState<number | null>(null);
  const [loading, setLoading] = useState(false);
  const [messageError, setMessageError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const reads = useMemo(
    () => ({
      selected,
      attempt,
      cursors: new Set<string>(),
      failed: false,
      stopped: false,
    }),
    [selected, attempt],
  );
  const currentReads = useRef(reads);
  currentReads.current = reads;
  useEffect(() => {
    reads.stopped = false;
    return () => {
      reads.stopped = true;
    };
  }, [reads]);
  const messages = timeline.history ? messagesOf(timeline.history, thread?.entries) : [];
  const selection: MessageSelection = {
    messages,
    selected,
    choose: (id) => {
      setSelected(id);
      setMessageError(null);
    },
  };
  const projected = timeline.history
    ? { ...timeline, history: { ...timeline.history, messages } }
    : timeline;
  // Thread reads remain in the page's store. Read one existing cursor at a time, until the
  // selected entry is present; failed or unchanging cursors stop with an explicit retry.
  const entries = thread?.entries;
  const before = thread?.before;
  const pending = thread?.pending;
  // biome-ignore lint/correctness/useExhaustiveDependencies: callbacks change on parent draws; the attempt and thread cursor drive paging, pending joins a read already owned by the store
  useEffect(() => {
    if (!thread || selected === null || thread.entries.some((e) => e.message.id === selected)) {
      setLoading(false);
      return;
    }
    if (reads.failed) return;
    const newest = thread.entries.reduce((last, e) => Math.max(last, e.message.id), -Infinity);
    const latest = thread.pending || selected > newest || thread.error !== null;
    const cursor = latest ? "latest" : thread.before === null ? null : `before:${thread.before}`;
    if (cursor === null || reads.cursors.has(cursor)) return;
    reads.cursors.add(cursor);
    setLoading(true);
    void (latest ? thread.latest() : thread.earlier()).then((error) => {
      if (reads.stopped || currentReads.current !== reads) return;
      reads.failed = error !== null;
      setLoading(false);
      setMessageError(error);
    });
  }, [selected, entries, before, pending, reads]);
  const [all, setAll] = useState(false);
  const [clipped, setClipped] = useState(false);
  const [detail, setDetail] = useState("");
  const [active, setActive] = useState(0);
  const [crossings, setCrossings] = useState(0);
  const body = useRef<HTMLDivElement>(null);
  const said = useRef(onDrawn);
  said.current = onDrawn;
  // Before the first draw is measured, which the sheet lays out: an answer may be there as the
  // chunk comes.
  useLayoutEffect(putStyle, []);
  useEffect(() => watchCrossings(() => setCrossings((count) => count + 1)), []);
  // biome-ignore lint/correctness/useExhaustiveDependencies: measured again for each answer drawn and each crossing of 900 px
  useLayoutEffect(() => {
    const over = measured(body.current, all, clipped, (wide, natural) =>
      said.current?.(wide, natural),
    );
    if (over !== null) setClipped(over);
  }, [timeline, crossings, all, clipped]);
  // biome-ignore lint/correctness/useExhaustiveDependencies: history updates, width crossings, collapse and expansion reflow the selected row
  useLayoutEffect(() => {
    if (isWide()) {
      if (body.current?.querySelector(".tl-message-row:focus")) {
        document.getElementById("tl-message-choice")?.focus();
      }
      return;
    }
    if (selected === null) return;
    const box = body.current;
    const row = box?.querySelector(`.tl-message-row[data-message="${selected}"]`);
    if (box && row)
      box.scrollTop +=
        row.getBoundingClientRect().top - box.getBoundingClientRect().top - box.clientHeight / 2;
  }, [selected, all, clipped, timeline, entries, crossings]);
  const panel =
    timeline.history || messages.length > 0 ? (
      <MessagePanel
        timeline={timeline}
        selection={selection}
        thread={thread}
        loading={loading}
        error={messageError}
        retry={() => {
          setMessageError(null);
          setAttempt((n) => n + 1);
        }}
        earlier={() => {
          if (!thread) return;
          setLoading(true);
          void thread.earlier().then((error) => {
            setLoading(false);
            setMessageError(error);
          });
        }}
      />
    ) : (
      <p className="tl-note">{historyWords(undefined)}</p>
    );
  return (
    <>
      <TimelineDrawn
        timeline={projected}
        tokens={tokens}
        selection={selection}
        wide={typeof innerWidth !== "undefined" && isWide()}
        all={all}
        clipped={clipped}
        detail={detail}
        stops={stopsOf(active, setActive, setDetail)}
        toggle={() => setAll((shown) => !shown)}
        body={body}
      />
      {panel && details ? createPortal(panel, details) : panel}
    </>
  );
}

export interface HostProps {
  slug: string;
  thread?: ThreadAccess | undefined;
  details?: HTMLElement | null | undefined;
  /** The timeline's part as the page's store shows it: the server's answer as it came. */
  part: Answer<unknown>;
  /** The task's tokens as the server sent them (the page's `tokensPart`), once they came: an
   * answer this client cannot read draws no cost. */
  tokens?: unknown;
  /** Asks the page's store to read the part again. */
  read: () => Promise<unknown>;
  /** Told each draw's natural height at its width, for the task's next visit. */
  onDrawn?: (wide: boolean, natural: number) => void;
}

/**
 * The timeline of `slug`, drawn from its `part`: read again (`read`) each `every` seconds while
 * the task is open and the page shown, and when Try again is pressed. A read that fails after one
 * that did not leaves the one shown. Before its first answer it draws nothing: the box's
 * placeholder shows.
 */
export function TimelineHost({
  part,
  tokens,
  read,
  onDrawn,
  thread,
  details,
}: HostProps): JSX.Element | null {
  const seen = useRef<Seen | null>(null);
  const [clock, setClock] = useState(0);
  if (part.value !== undefined && seen.current?.value !== part.value) {
    seen.current = { value: part.value, at: Date.now() };
  }
  // biome-ignore lint/correctness/useExhaustiveDependencies: drawn again for a new answer, a failure or a moved clock, whatever else the part says
  const shown = useMemo(
    () => shownOf(part, seen.current, clock),
    [part.value, part.error, part.missing, clock],
  );
  const every = nextRead(shown);
  const spent = useMemo(() => (tokens == null ? null : readTaskTokens(tokens)), [tokens]);
  const asked = useRef(read);
  asked.current = read;
  useLayoutEffect(putStyle, []);
  // Each task starts its reads afresh: its host is mounted anew (timeline-slot.tsx keys it).
  useEffect(() => {
    const reading = new AbortController();
    void readAgain(
      () => every,
      () => asked.current(),
      setClock,
      reading.signal,
    );
    return () => reading.abort();
  }, [every]);
  if (shown === null) return null;
  if ("error" in shown) {
    return (
      <TimelineFailed
        error={shown.error}
        again={shown.final !== true}
        retry={() => void asked.current()}
      />
    );
  }
  return (
    <TimelineView
      timeline={shown.timeline}
      tokens={spent}
      onDrawn={onDrawn}
      thread={thread}
      details={details}
    />
  );
}

/**
 * A read that failed with no timeline shown, said in the box's place quietly with its Try again
 * (`retry`): the page's own errors are its notice stack's, drawn once outside its flow.
 */
export function TimelineFailed({
  error,
  again = true,
  retry,
}: {
  error: string;
  /** Whether the read is tried again by itself. */
  again?: boolean;
  retry: () => void;
}) {
  return (
    <div className="tl-drawn tl-failed">
      <p className="tl-note">
        {`${failed(error, again)} `}
        <Button type="button" onClick={retry}>
          Try again
        </Button>
      </p>
    </div>
  );
}
