// A task's timeline laid out, with no DOM: what it went through, read from `/api/task-timeline`
// (crates/symphony-api's `timeline`), as the phone's list and the wide lanes draw it.
//
// - On a phone, a list (`rows`): a row per step with its start, its label, how long it took and a
//   bar as long, its wait drawn lighter; the builds, tests and gate changes indented under the
//   stage they happened in, each build and test named by its command; the verdict after a review's
//   bar, and a striped bar for what runs now.
// - Wide, lanes over the same hours (`lanes`): the stage, the executor, the reviewer, the builds
//   and the tests, each a lane of its own (a row each for runs that overlap or meet, so no two are
//   drawn as one bar, thin past WORDED_ROWS), and a thin row per gate, its state drawn by a pattern
//   of its own as well as a colour, with a line at now.
//
// A run's words for a pointer resting on it and for its focus (`runTitle`) name its command, its
// wait to start and its run time, each apart.
//
// Where the task's tokens are read, each stage stretch's estimated cost follows its time, in its
// label, its words and its row (`Costs`, what timeline-cost.ts puts on each stage).
//
// What crates/symphony-api's `timeline` answers (a time's offset, how often to read again, where
// the timeline ends, a run's wait and run time, the words of a verdict, a result and a gate's
// state) is carried here in TypeScript; the rest is the layout the Dioxus client drew (its
// `task_timeline.rs`, gone with its task page), moved here with its tests case for case and held to
// it by the browser tests.

import { field } from "./shared";
import { type Carried, carriedFigure, carriedWords } from "./timeline-cost";
import { address, endpointLane, readHistory } from "./timeline-history";
import type {
  Claim,
  GateLane,
  GateSpan,
  GateState,
  Offset,
  PlaceSpan,
  Round,
  RoundVerdict,
  Run,
  RunResult,
  Stage,
  TaskTimeline,
} from "./wire";
import { TIMELINE_EVERY } from "./wire";
import { fixed, plural } from "./words";

/** How much of a row the longest step's bar takes on a phone, in percent: the rest is for the word
 * after it. */
export const BAR_SPAN = 72;

/**
 * A character of the lanes' small type, about, in pixels; then the padding a segment's words take
 * beside them, and the gap a label keeps from the next. A word the estimate lets through that is
 * wider still is cut short with an ellipsis at its room's end, never drawn over the next.
 */
export const CHAR_PX = 6;
export const PAD_PX = 12;
export const GAP_PX = 4;
/** The lanes' margin after the track's end (`--end`, 5.5rem), which the last label may take. */
export const END_PX = 88;

/**
 * The track widths a word's room is weighed at, in pixels, every 50 from 400 to 1600: a word shows
 * from the first of them its room holds it at (`tl-from-W`, `tl-to-W` in the style sheet), at
 * every width when 300 does, and never when 1600 does not.
 */
export const WIDTHS: readonly number[] = Array.from({ length: 25 }, (_, at) => 400 + at * 50);

/** How near, in percent of the hours drawn, a run may begin to the end of the one before it in a
 * row: nearer, the two would read as one bar (a segment is drawn 2 px wide at the least), so it
 * takes a row of its own. */
export const RUN_GAP = 0.5;

/** The most rows a lane of runs draws with each run's words under its bar. A lane that needs more
 * (a burst of runs on a timeline of days, where each is a sliver) draws its rows thin and no words
 * under them: each run's words are in its hover, its focus and its row in the list. */
export const WORDED_ROWS = 6;

/** The hours between the wide view's ticks: the first that draws no more than eight. */
const TICK_HOURS = [1, 2, 3, 4, 6, 8, 12, 24, 72, 168];

/** Statuses a task ends in: an instant on the timeline, with no time of its own. */
const CLOSED = ["done", "cancelled"];

/** The review gate's name (`symphony_api::gates::REVIEW`): its rounds say its changes. */
const REVIEW = "review";

/** The longest wait a browser's timer takes, in ms (an `i32`'s most, as the Dioxus client's). */
export const LONGEST_WAIT = 2_147_483_647;

// What crates/symphony-api's `timeline` answers, carried.

/**
 * Seconds to add to `at` for the server's local clock then: the offset of the last change of the
 * zone at or before it, the first one before them all; none without a zone
 * (`TaskTimeline::offset_at`).
 */
export function offsetAt(timeline: TaskTimeline, at: number): number {
  let found: Offset | undefined;
  for (const change of timeline.zone) {
    if (change.from > at) break;
    found = change;
  }
  return (found ?? timeline.zone[0])?.seconds ?? 0;
}

/** How often to read it again while the task is open, in seconds (`TaskTimeline::every_seconds`). */
export function everySeconds(timeline: TaskTimeline): number {
  const every = timeline.every ?? 0;
  return every === 0 ? TIMELINE_EVERY : every;
}

/** Where the timeline ends: its close, or now, or later where a run went on past it
 * (`TaskTimeline::end`). */
export function timelineEnd(timeline: TaskTimeline): number {
  const close = timeline.closed ?? timeline.now;
  const latest = timeline.runs.reduce(
    (end, run) => Math.max(end, run.ended ?? timeline.now),
    close,
  );
  return Math.max(latest, timeline.created);
}

/** How long a stage held its status, an open one up to `now` (`Stage::seconds`). */
export function stageSeconds(stage: Stage, now: number): number {
  return nonNegative((stage.to ?? now) - stage.from);
}

/** How long a run waited to start, up to `now` while it still waits (`Run::waited`). */
export function waited(run: Run, now: number): number {
  return nonNegative((run.started ?? run.ended ?? now) - run.queued);
}

/** How long a run ran, up to `now` while it runs (`Run::ran`). */
export function ran(run: Run, now: number): number {
  const started = run.started ?? null;
  return started === null ? 0 : nonNegative((run.ended ?? now) - started);
}

/** The machine's time on the task's runs (`TaskTimeline::machine_seconds`). */
export function machineSeconds(timeline: TaskTimeline): number {
  return timeline.runs.reduce((sum, run) => sum + ran(run, timeline.now), 0);
}

/** The runs' time waiting to start (`TaskTimeline::queued_seconds`). */
export function queuedSeconds(timeline: TaskTimeline): number {
  return timeline.runs.reduce((sum, run) => sum + waited(run, timeline.now), 0);
}

/** A verdict's word (`Verdict::as_str`): one this client does not know is "verdict". */
export function verdictWord(verdict: RoundVerdict): string {
  return verdict === "other" ? "verdict" : verdict;
}

/** A run's result's word (`RunResult::as_str`): one this client does not know is "ended". */
export function resultWord(result: RunResult): string {
  return result === "unknown" ? "ended" : result;
}

/** `x`, or 0 where it is below (or not a number), as Rust's `f64::max(x, 0.0)`. */
function nonNegative(x: number): number {
  return Number.isNaN(x) ? 0 : Math.max(x, 0);
}

/** Rust's `rem_euclid` of two numbers: never negative for a positive `by`. */
function remEuclid(x: number, by: number): number {
  const rest = x % by;
  return rest < 0 ? rest + Math.abs(by) : rest;
}

/** Rust's `f64::total_cmp` for the times a timeline holds: -0 before +0. */
function totalCmp(a: number, b: number): number {
  if (a < b) return -1;
  if (a > b) return 1;
  if (Object.is(a, b)) return 0;
  return Object.is(a, -0) ? -1 : 1;
}

/** Rust's `char::is_whitespace`, the Unicode White_Space property. */
const SPACE = /^\p{White_Space}+|\p{White_Space}+$/gu;

/** `text` without its leading and trailing whitespace, as Rust's `str::trim`. */
export function trimmed(text: string): string {
  return text.replace(SPACE, "");
}

/** The words of `text`, as Rust's `split_whitespace`. */
function words(text: string): string[] {
  return text.split(/\p{White_Space}+/u).filter((word) => word !== "");
}

// The words the timeline says.

/** How long `seconds` is, to the minute: "41 m", "1 h 52 m", "2 d 3 h". */
export function took(seconds: number): string {
  const minutes = Math.round(nonNegative(seconds) / 60);
  const [days, hours, rest] = [
    Math.floor(minutes / 1440),
    Math.floor(minutes / 60) % 24,
    minutes % 60,
  ];
  if (days === 0 && hours === 0) return `${rest} m`;
  if (days === 0) return rest === 0 ? `${hours} h` : `${hours} h ${rest} m`;
  return hours === 0 ? `${days} d` : `${days} d ${hours} h`;
}

/** How long a run's `seconds` are: to the second under a minute ("0 s", "47 s"), else as `took`
 * says, so a short build or test never reads "0 m". */
export function lasted(seconds: number): string {
  const whole = Math.round(nonNegative(seconds));
  return whole < 60 ? `${whole} s` : took(seconds);
}

/** Minutes since 1970 on the server's local clock, `offset` seconds ahead of `at`. */
function local(at: number, offset: number): number {
  return Math.floor((at + offset) / 60);
}

const twoDigits = (n: number) => String(n).padStart(2, "0");

/** `at` on the server's local clock: "09:02". */
export function clock(at: number, offset: number): string {
  const minutes = remEuclid(local(at, offset), 1440);
  return `${twoDigits(Math.floor(minutes / 60))}:${twoDigits(minutes % 60)}`;
}

/** The local day of `at`, month and day: "11-10". */
export function date(at: number, offset: number): string {
  const [, month, day] = civil(Math.floor(local(at, offset) / 1440));
  return `${twoDigits(month)}-${twoDigits(day)}`;
}

/** `at` on the server's clock as it read then, its offset that instant's (a change of summer time
 * between two steps reads each at its own). */
export function clockOf(timeline: TaskTimeline, at: number): string {
  return clock(at, offsetAt(timeline, at));
}

export function dateOf(timeline: TaskTimeline, at: number): string {
  return date(at, offsetAt(timeline, at));
}

/**
 * The date `days` after 1970-01-01 in the proleptic Gregorian calendar (Howard Hinnant's
 * `civil_from_days`), the inverse of `words.ts`'s `moment`: year, month and day. gates-model.ts
 * holds the same, kept apart: shared, the build makes it one more file of the page's first visit.
 */
export function civil(days: number): [number, number, number] {
  const z = days + 719_468;
  const era = Math.floor(z / 146_097);
  const dayOfEra = z - era * 146_097;
  const yearOfEra = Math.floor(
    (dayOfEra -
      Math.floor(dayOfEra / 1460) +
      Math.floor(dayOfEra / 36_524) -
      Math.floor(dayOfEra / 146_096)) /
      365,
  );
  const dayOfYear =
    dayOfEra - (365 * yearOfEra + Math.floor(yearOfEra / 4) - Math.floor(yearOfEra / 100));
  const monthFromMarch = Math.floor((5 * dayOfYear + 2) / 153);
  const day = dayOfYear - Math.floor((153 * monthFromMarch + 2) / 5) + 1;
  const month = monthFromMarch < 10 ? monthFromMarch + 3 : monthFromMarch - 9;
  const year = yearOfEra + era * 400 + (month <= 2 ? 1 : 0);
  return [year, month, day];
}

/** `word` with a capital first letter and spaces for underscores: "In progress". */
export function capitalised(word: string): string {
  const [first = "", ...rest] = word.replaceAll("_", " ");
  return first.toUpperCase() + rest.join("");
}

/** The theme token a board status is drawn in. */
export function statusTone(status: string): string {
  switch (status) {
    case "ready":
      return "auto";
    case "in_progress":
      return "primary";
    case "review":
      return "question";
    case "done":
      return "like";
    case "blocked":
      return "dislike";
    case "cancelled":
      return "resolved";
    case "concept":
    case "design":
      return "decision";
    default:
      return "muted-foreground";
  }
}

const RESULT_TONES: Record<RunResult, string> = {
  green: "like",
  red: "dislike",
  queued: "primary",
  running: "primary",
  unknown: "resolved",
};

const GATE_TONES: Record<GateState, string> = {
  met: "like",
  missing: "question",
  waived: "resolved",
  held: "dislike",
  unknown: "muted-foreground",
};

const VERDICT_TONES: Record<RoundVerdict, string> = {
  approved: "like",
  changes: "dislike",
  other: "muted-foreground",
};

export const resultTone = (result: RunResult) => RESULT_TONES[result];
export const gateTone = (state: GateState) => GATE_TONES[state];
export const verdictTone = (verdict: RoundVerdict) => VERDICT_TONES[verdict];

function closed(stage: Stage): boolean {
  return CLOSED.includes(stage.status);
}

/** `label`, then " · `by`" when someone is named. */
function withBy(label: string, by: string | null | undefined): string {
  return by === null || by === undefined || by === "" ? label : `${label} · ${by}`;
}

/** The bytes `char` takes in UTF-8. */
function utf8(char: string): number {
  const point = char.codePointAt(0) ?? 0;
  if (point < 0x80) return 1;
  if (point < 0x800) return 2;
  return point < 0x10000 ? 3 : 4;
}

/**
 * A commit as a label names it: its first seven characters, as Rust's `commit.get(..7)` cuts it:
 * its first seven bytes where they end a character, else the whole commit.
 */
export function shortCommit(commit: string): string {
  let bytes = 0;
  let at = 0;
  for (const char of commit) {
    if (bytes === 7) return commit.slice(0, at);
    bytes += utf8(char);
    at += char.length;
    if (bytes > 7) return commit;
  }
  return commit;
}

/** The first of the heading's totals in its two parts, between which the task's estimated cost
 * goes: how long the task has taken, then its rounds and bounces. */
export function totalParts(timeline: TaskTimeline): [string, string] {
  const length = took(timelineEnd(timeline) - timeline.created);
  const so = timeline.closed === undefined || timeline.closed === null ? "so far" : "in all";
  const rounds = timeline.rounds.length;
  return [
    `${length} ${so}`,
    ` · ${rounds} ${plural(rounds, "review round")} · ` +
      `${timeline.bounces} ${plural(timeline.bounces, "bounce")}`,
  ];
}

/** The heading's totals: how long the task has taken, its rounds and bounces; then, once a run is
 * kept for it, the machine's time on its runs and their wait to start. */
export function totals(timeline: TaskTimeline): [string, string | null] {
  const first = totalParts(timeline).join("");
  const machine =
    timeline.runs.length === 0
      ? null
      : `${took(machineSeconds(timeline))}, queued ${took(queuedSeconds(timeline))}`;
  return [first, machine];
}

/** What the timeline cannot show: the runs the recorder's rotated history let go, before
 * `runs_from`. */
export function keptNote(timeline: TaskTimeline): string | null {
  const from = timeline.runs_from ?? null;
  return from === null
    ? null
    : `Builds and tests before ${dateOf(timeline, from)} ${clockOf(timeline, from)} are no longer kept.`;
}

// The phone's list.

/** A row's bar: its wait, drawn lighter in `waitTone`, then its work, each a percentage of the row
 * as long as its share of the longest step's. */
export interface Bar {
  wait: number;
  waitTone: string;
  work: number;
  /** The work goes on now: striped. */
  running: boolean;
}

/** One row of the phone's list. */
export interface Row {
  /** When it began, server seconds. */
  start: number;
  /** The local day, "11-10", on the first row of each day of a timeline that spans more than one. */
  day: string | null;
  /** When it began on the server's local clock, "09:02". */
  at: string;
  /** A build, a test or a gate change, under the stage it happened in. */
  sub: boolean;
  /** A gate change: an instant, a diamond with no bar. */
  event: boolean;
  tone: string;
  label: string;
  /** How long it took; none for an instant. */
  took: string | null;
  /** A stage's estimated cost, said after its time; left out where it carries none. */
  cost?: string;
  /** Its bar; none for an instant. */
  bar: Bar | null;
  /** The word after the bar (a verdict, a run's result, "now") and its tone. */
  word: [string, string] | null;
  /** The whole of it, for a pointer resting on the row. */
  title: string;
  message?: number;
}

/** Who held the executor role while `stage` went on: the first claim over it. */
function claimer(timeline: TaskTimeline, stage: Stage): string | null {
  const end = stage.to ?? timeline.now;
  const claim = timeline.claims.find(
    (claim) => claim.from < end && (claim.to ?? timeline.now) > stage.from,
  );
  return claim?.by ?? null;
}

/** What the list says of the stages in progress before this one: whether there was one, and who
 * held it last. */
interface Worked {
  before: boolean;
  by: string | null;
}

type Share = (seconds: number) => number;

/** What the task's tokens put on each stage (`carried`, timeline-cost.ts), one for each of the
 * timeline's stages in their order, and the day their prices were read. */
export interface Costs {
  stages: Carried[];
  prices: string;
}

/** What the stage at `index` says of its cost: its figure after its time, or none, and what its
 * words add after its hours. */
function stageCost(timeline: TaskTimeline, costs: Costs | undefined, index: number) {
  const held = costs?.stages[index];
  const start = timeline.stages[index]?.from ?? timeline.created;
  // A call made another day than its stretch began says its day too.
  const when = (at: number) =>
    dateOf(timeline, at) === dateOf(timeline, start)
      ? clockOf(timeline, at)
      : `${dateOf(timeline, at)} ${clockOf(timeline, at)}`;
  const from = (other: number) => when(timeline.stages[other]?.from ?? start);
  return {
    figure: carriedFigure(held),
    words: carriedWords(held, timeline.slug, costs?.prices ?? "", when, from),
  };
}

/** `x` held between `least` and `most`, as Rust's `f64::clamp` (which panics where `least` is
 * over `most`: here `most` wins). */
function clamp(x: number, least: number, most: number): number {
  return Math.min(Math.max(x, least), most);
}

function stageRow(
  timeline: TaskTimeline,
  stage: Stage,
  worked: Worked,
  share: Share,
  cost: { figure: string | null; words: string },
): Row {
  const now = timeline.now;
  const end = stage.to ?? now;
  const ended = closed(stage);
  const tone = statusTone(stage.status);
  const number = stage.round ?? 0;
  const round = timeline.rounds.find((round) => number > 0 && round.round === number);
  const firstClaimed = timeline.first_claimed ?? null;
  let label: string;
  if (stage.status === "ready" && (firstClaimed === null || end <= firstClaimed)) {
    label = "Ready, unclaimed";
  } else if (stage.status === "in_progress") {
    const by = claimer(timeline, stage);
    label = worked.before
      ? withBy("In progress again", by !== null && worked.by !== by ? by : null)
      : withBy("In progress", by);
    worked.before = true;
    worked.by = by;
  } else if (stage.status === "review" && stage.approved === true) {
    label = "Approved, waiting to land";
  } else if (stage.status === "review" && number > 0) {
    label = withBy(`Review, round ${number}`, round?.by);
  } else {
    label = withBy(
      capitalised(stage.status),
      ended || stage.status === "blocked" ? stage.by : null,
    );
  }
  const seconds = stageSeconds(stage, now);
  let bar: Bar | null = null;
  if (!ended) {
    if (stage.approved === true) {
      bar = { wait: share(seconds), waitTone: tone, work: 0, running: false };
    } else if (round !== undefined) {
      const taken = clamp(round.taken ?? end, stage.from, end);
      bar = {
        wait: share(taken - stage.from),
        waitTone: tone,
        work: share(end - taken),
        running: false,
      };
    } else {
      bar = { wait: 0, waitTone: tone, work: share(seconds), running: false };
    }
  }
  const verdict = round?.verdict ?? null;
  let word: [string, string] | null = null;
  if (verdict !== null && stage.approved !== true) {
    word = [verdictWord(verdict), verdictTone(verdict)];
  } else if ((stage.to ?? null) === null && !ended) {
    word = ["now", "muted-foreground"];
  }
  const to = stage.to ?? null;
  const until = to === null ? " until now" : ` to ${clockOf(timeline, to)}`;
  const by = stage.by ?? null;
  const moved = by === null ? "" : `, moved there by ${by}`;
  return {
    start: stage.from,
    day: null,
    at: clockOf(timeline, stage.from),
    sub: false,
    event: false,
    tone,
    title: `${label}: from ${clockOf(timeline, stage.from)}${until}${moved}${cost.words}`,
    label,
    took: ended ? null : took(seconds),
    ...(cost.figure === null ? {} : { cost: cost.figure }),
    bar,
    word,
  };
}

/**
 * A run's label in the list: the executor's own as its phase and command, with its wait to start
 * where it waited a minute or more ("Test, just verify · queued 6 m", "Build, just lint-rs"); a
 * run nobody holds (a gate's, on the machine) as what it runs and the commit it ran at
 * ("Certificate verify · e19d3a0").
 */
export function runLabel(run: Run, wait: number): string {
  if (run.by === undefined || run.by === null) {
    const commit = run.commit ?? null;
    return withBy(capitalised(run.label), commit === null ? null : shortCommit(commit));
  }
  const label = `${capitalised(run.phase)}, ${run.label}`;
  return wait >= 60 ? `${label} · queued ${took(wait)}` : label;
}

/**
 * All of a run, for a pointer resting on it and for its focus: its phase and command, its wait to
 * start and its run time, each apart (one going on counts them so far), how it ended, and the
 * commit it ran at, who ran it and where: "Build, cargo install symphony-server · waited 0 s ·
 * ran 55 m · green · at 329c986 by glacial-ibis on studio".
 */
export function runTitle(run: Run, now: number): string {
  const open = run.result === "queued" || run.result === "running";
  const so = open ? " so far" : "";
  const wait = lasted(waited(run, now));
  const facts = [`${capitalised(run.phase)}, ${run.label}`];
  if ((run.started ?? null) === null) {
    facts.push(`waited ${wait}${so}`, open ? "not started yet" : "never started");
  } else {
    facts.push(`waited ${wait}`, `ran ${lasted(ran(run, now))}${so}`);
  }
  if (!open) facts.push(resultWord(run.result));
  const commit = run.commit ?? null;
  const where = (
    [
      ["at", commit === null ? null : shortCommit(commit)],
      ["by", run.by ?? null],
      ["on", run.host ?? null],
    ] as const
  )
    .filter(([, value]) => value !== null && value !== "")
    .map(([word, value]) => `${word} ${value}`);
  if (where.length > 0) facts.push(where.join(" "));
  return facts.join(" · ");
}

export function runRow(run: Run, timeline: TaskTimeline, share: Share): Row {
  const now = timeline.now;
  const [wait, running] = [waited(run, now), ran(run, now)];
  const tone = resultTone(run.result);
  // A build that went green says nothing more than that it was built.
  const word: [string, string] | null =
    run.phase !== "build" || run.result !== "green" ? [resultWord(run.result), tone] : null;
  const started = run.started ?? null;
  return {
    start: run.queued,
    day: null,
    at: clockOf(timeline, run.queued),
    sub: true,
    event: false,
    tone,
    label: runLabel(run, wait),
    took: lasted(started === null ? wait : running),
    bar: {
      wait: share(wait),
      waitTone: "primary",
      work: share(running),
      running: run.result === "running",
    },
    word,
    title: runTitle(run, now),
  };
}

/** What a gate change's detail says moved it, when it opens with a short reason before a colon:
 * "Main moved" of "Main moved: verify on the merge tree 4be91c2 running". */
export function reason(detail: string): string | null {
  const colon = detail.indexOf(": ");
  if (colon < 0) return null;
  const said = detail.slice(0, colon);
  return said !== "" && words(said).length <= 3 ? said : null;
}

/** Each change of a gate's state after its first, but the review gate's, whose rounds say it, and
 * a gate met by a run that went green then, whose row says it. */
export function gateChanges(timeline: TaskTimeline): Row[] {
  const greenAt = (at: number) =>
    timeline.runs.some(
      (run) =>
        run.result === "green" &&
        run.ended !== undefined &&
        run.ended !== null &&
        Math.abs(run.ended - at) <= 60,
    );
  const rows: Row[] = [];
  for (const lane of timeline.gates.filter((lane) => lane.name !== REVIEW)) {
    for (let at = 1; at < lane.states.length; at += 1) {
      const before = lane.states[at - 1] as GateSpan;
      const span = lane.states[at] as GateSpan;
      if (span.state === "met" && greenAt(span.from)) continue;
      const name = capitalised(lane.name);
      let label: string;
      if (span.state === "missing" && before.state === "met") {
        const why = reason(span.detail);
        label = why === null ? `${name} missing again` : `${why}: ${lane.name} again`;
      } else if (span.state === "waived" || span.state === "held") {
        label = withBy(`${name} ${span.state}`, span.by);
      } else {
        label = `${name} ${span.state}`;
      }
      rows.push({
        start: span.from,
        day: null,
        at: clockOf(timeline, span.from),
        sub: true,
        event: true,
        tone: gateTone(span.state),
        title: span.detail === "" ? label : span.detail,
        label,
        took: null,
        bar: null,
        word: null,
      });
    }
  }
  return rows;
}

// A custom workflow's places (docs/workflow/WORKFLOW.md, "The app").

/** What a stretch of places shares: its phase, its state, or both, its place. */
export type PlaceKey = "phase" | "state" | "place";

/** Places in a row that share one phase, one state or one place: one stretch, from the first's
 * move to the next stretch's (the task's close, or now, for the last). */
export interface PlaceStretch {
  id: string;
  label: string;
  /** The board status its first place projects to, which it is coloured by. */
  status: string;
  from: number;
  to: number;
  /** Who moved the task there, and by which edge (none for an activation's move). */
  by: string;
  edge: string | null;
}

function placeOf(place: PlaceSpan, key: PlaceKey): [string, string] {
  if (key === "phase") return [place.phase, place.phase_label];
  if (key === "state") return [place.state, place.state_label];
  return [`${place.phase}/${place.state}`, `${place.phase_label} · ${place.state_label}`];
}

/** The stretches of `timeline`'s places that share their `key`, oldest first: none for a task no
 * workflow held. Each is the clock's time there, never the machine's on runs. */
export function stretches(timeline: TaskTimeline, key: PlaceKey): PlaceStretch[] {
  const end = timeline.closed ?? timeline.now;
  const drawn: PlaceStretch[] = [];
  for (const place of timeline.places ?? []) {
    const [id, label] = placeOf(place, key);
    const last = drawn[drawn.length - 1];
    if (last?.id === id) continue;
    if (last !== undefined) last.to = place.from;
    drawn.push({
      id,
      label,
      status: place.status,
      from: place.from,
      to: end,
      by: place.by,
      edge: place.edge ?? null,
    });
  }
  return drawn;
}

/** The time `timeline`'s task spent at each phase or state (`key`), all its stretches there told,
 * in the order it first came to each: "Build 3 h · Review 52 m". None for a task no workflow held. */
export function placeTotals(timeline: TaskTimeline, key: "phase" | "state"): string | null {
  const spent = new Map<string, [string, number]>();
  for (const stretch of stretches(timeline, key)) {
    const [, seconds] = spent.get(stretch.id) ?? [stretch.label, 0];
    spent.set(stretch.id, [stretch.label, seconds + (stretch.to - stretch.from)]);
  }
  if (spent.size === 0) return null;
  return [...spent.values()].map(([label, seconds]) => `${label} ${took(seconds)}`).join(" · ");
}

/** All of a stretch, for a pointer resting on it: how long, from when to when, who moved the
 * task there and by which edge. */
function stretchTitle(stretch: PlaceStretch, length: string, clock: (at: number) => string) {
  const how = stretch.edge === null ? "" : ` (${stretch.edge.replaceAll("_", " ")})`;
  return (
    `${stretch.label} ${length}, from ${clock(stretch.from)} to ${clock(stretch.to)}, ` +
    `moved there by ${stretch.by}${how}`
  );
}

/** The phone's row of a place's stretch, under the stage it began in, its bar its share of the
 * longest step's: striped while the task is still there. */
export function placeRow(stretch: PlaceStretch, timeline: TaskTimeline, share: Share): Row {
  const seconds = stretch.to - stretch.from;
  const length = took(seconds);
  const open = timeline.closed === undefined || timeline.closed === null;
  return {
    start: stretch.from,
    day: null,
    at: clockOf(timeline, stretch.from),
    sub: true,
    event: false,
    tone: statusTone(stretch.status),
    label: stretch.label,
    took: length,
    bar: {
      wait: 0,
      waitTone: "primary",
      work: share(seconds),
      running: open && stretch.to === timeline.now,
    },
    word: null,
    title: stretchTitle(stretch, length, (at) => clockOf(timeline, at)),
  };
}

/**
 * The phone's list: each stage, and under it the builds, tests and gate changes that began while
 * it went on, by time. A stage left before a minute was out (one that would read "0 m") has no
 * row: what began in it goes under the stage before.
 */
export function rows(timeline: TaskTimeline, costs?: Costs): Row[] {
  const now = timeline.now;
  const places = stretches(timeline, "place");
  const longest = [
    ...timeline.stages.filter((stage) => !closed(stage)).map((stage) => stageSeconds(stage, now)),
    ...timeline.runs.map((run) => waited(run, now) + ran(run, now)),
    ...places.map((stretch) => stretch.to - stretch.from),
  ].reduce((most, seconds) => Math.max(most, seconds), 60);
  const share: Share = (seconds) => (nonNegative(seconds) / longest) * BAR_SPAN;
  const under = [
    ...timeline.runs.map((run) => runRow(run, timeline, share)),
    ...places.map((stretch) => placeRow(stretch, timeline, share)),
    ...gateChanges(timeline),
  ].sort((a, b) => totalCmp(a.start, b.start));
  let next = 0;
  const listed: Row[] = [];
  const worked: Worked = { before: false, by: null };
  timeline.stages.forEach((stage, index) => {
    const cost = stageCost(timeline, costs, index);
    const row = stageRow(timeline, stage, worked, share, cost);
    // A stage left before a minute was out has no row, unless it carries a cost, which no other
    // row would say.
    const fleeting =
      !closed(stage) &&
      (stage.to ?? null) !== null &&
      stageSeconds(stage, now) < 30 &&
      cost.figure === null;
    if (!fleeting) listed.push(row);
    const following = timeline.stages[index + 1]?.from;
    while (next < under.length) {
      const sub = under[next] as Row;
      if (following !== undefined && !(sub.start < following)) break;
      listed.push(sub);
      next += 1;
    }
  });
  listed.push(...under.slice(next));
  if (timeline.history) {
    const instant = (start: number, label: string, title = label): Row => ({
      start,
      label,
      title,
      day: null,
      at: clockOf(timeline, start),
      sub: true,
      event: true,
      tone: "muted-foreground",
      took: null,
      bar: null,
      word: null,
    });
    for (const role of timeline.history.roles.filter((r) => r.role === "orchestrator")) {
      const detail = `${role.by} holds orchestrator; source ${role.source}${role.uncertain ? "; uncertain boundaries" : ""}`;
      listed.push(instant(role.from, `Orchestrator: ${role.by}`, detail));
      if (role.to !== null)
        listed.push(instant(role.to, `Orchestrator released: ${role.by}`, detail));
    }
    for (const message of timeline.history.messages) {
      listed.push({
        ...instant(message.at, address(message), `Message #${message.id}: ${address(message)}`),
        message: message.id,
      });
    }
    listed.sort((a, b) => a.start - b.start || (a.message ?? 0) - (b.message ?? 0));
  }
  const earliest = Math.min(timeline.created, listed[0]?.start ?? timeline.created);
  if (dateOf(timeline, earliest) !== dateOf(timeline, new Scale(timeline).end)) {
    let last: string | null = null;
    for (const row of listed) {
      const day = dateOf(timeline, row.start);
      if (last !== day) {
        row.day = day;
        last = day;
      }
    }
  }
  return listed;
}

// The wide lanes.

/** How a lane's segment is drawn: worked or ran, queued or waiting (lighter, its edge dashed), or
 * running now (striped). */
export type Look = "solid" | "waiting" | "running";

export function lookClass(look: Look): string {
  return look === "solid" ? "tl-seg" : `tl-seg tl-${look}`;
}

/** Words drawn only at the track widths that hold them: from `from` pixels wide (always when
 * null) and below `to` (always when null). */
export interface Fit {
  text: string;
  from: number | null;
  to: number | null;
}

/** The classes that show a fit at the widths that hold it. */
export function fitClass(fit: Fit): string {
  let names = "tl-fit";
  if (fit.from !== null) names += ` tl-from-${fit.from}`;
  if (fit.to !== null) names += ` tl-to-${fit.to}`;
  return names;
}

/**
 * The first of WIDTHS at which `text`, with `pad` pixels beside it, fits in `room` percent of the
 * track: null at every width, undefined at none.
 */
export function widthFor(text: string, room: number, pad: number): number | null | undefined {
  if (text === "" || room <= 0) return undefined;
  const needed = (([...text].length * CHAR_PX + pad) * 100) / room;
  if (needed <= (WIDTHS[0] as number) - 100) return null;
  return WIDTHS.find((width) => width >= needed);
}

/** Room a label beside a segment has: `room` percent of the track before the next, and the lanes'
 * end margin past the track's end when nothing follows it (`last`). */
export interface Room {
  room: number;
  last: boolean;
}

/** A room's width, for the label's `max-width`: past it a label is cut short, never drawn over the
 * next. */
export function roomCss(room: Room): string {
  return room.last ? `calc(${fixed(room.room, 2)}% + var(--end))` : `${fixed(room.room, 2)}%`;
}

/** `text` as a label beside a segment, in `room`. */
export function label(text: string, room: Room): Fit | null {
  const pad = GAP_PX - (room.last ? END_PX : 0);
  if (room.last && [...text].length * CHAR_PX + pad <= 0) return { text, from: null, to: null };
  const from = widthFor(text, room.room, pad);
  return from === undefined ? null : { text, from, to: null };
}

/** The first of `texts`, longest first, that fits in a segment `room` percent wide: each drawn
 * only at the widths that hold it and no longer one. */
export function fittedFirst(texts: readonly string[], room: number): Fit[] {
  const fits: Fit[] = [];
  // The width from which a longer one is drawn: none yet.
  let to: number | null = null;
  for (const text of texts) {
    if (text === "" || fits.some((fit) => fit.text === text)) continue;
    const from = widthFor(text, room, PAD_PX);
    if (from === undefined) continue;
    if (from !== null && to !== null && from >= to) continue;
    fits.push({ text, from, to });
    if (from === null) break;
    to = from;
  }
  return fits;
}

/** `long` where it fits in a segment `room` percent wide, else `short` where that fits: each drawn
 * only at the widths that hold it. */
export function fitted(long: string, short: string, room: number): Fit[] {
  return fittedFirst([long, short], room);
}

/** A segment of a lane, placed in percent of the hours drawn. */
export interface Seg {
  left: number;
  width: number;
  tone: string;
  look: Look;
  /** Its words, at the widths that hold them. */
  text: Fit[];
  /** A word right after it (a review's verdict) and its tone, where there is room before the
   * next, and that room in percent: it is cut short there, never drawn over the next. */
  after: [Fit, string, Room] | null;
  /** A label under it (a run's), where there is room before the next in its row, and that room. */
  under: [Fit, Room] | null;
  /** The row of its lane it is drawn in (runs that overlap or meet take a row each). */
  row: number;
  /** All of it, for a pointer resting on it and for assistive technology. */
  title: string;
  /** Whether the keyboard stops at it: a part the rest of its step follows (a run's queue before
   * its run, a round's wait before its reviewer's time) is only drawn, the part after it saying
   * all of it. */
  stop: boolean;
}

/** A lane of the wide view: its key (its row's `data-lane`), its name, who worked in it, how many
 * rows its segments take, and its segments. */
export interface Lane {
  key: string;
  name: string;
  who: string;
  rows: number;
  /** Its rows are more than [`WORDED_ROWS`]: drawn thin, with no words under its runs. */
  dense: boolean;
  segs: Seg[];
}

/** A gate's state over the hours it held, placed in percent of the hours drawn. */
export interface GateBar {
  left: number;
  width: number;
  state: GateState;
  /** Its state's word, at the widths that hold it. */
  word: Fit | null;
  title: string;
}

/** A gate's thin row. */
export interface GateRow {
  name: string;
  spans: GateBar[];
}

/** An hour marked on the wide view's axis, in percent of the hours drawn. */
export interface Tick {
  left: number;
  label: string;
}

/** The wide view: the hours, a line at now while the task is open, the lanes and the gates' rows. */
export interface Lanes {
  ticks: Tick[];
  now: Tick | null;
  lanes: Lane[];
  gates: GateRow[];
}

/** How many parts of the lanes a keyboard stops at: each step's segment, then each gate's state. */
export function stops(lanes: Lanes): number {
  const segs = lanes.lanes.reduce(
    (count, lane) => count + lane.segs.filter((seg) => seg.stop).length,
    0,
  );
  return segs + lanes.gates.reduce((count, gate) => count + gate.spans.length, 0);
}

/** The hours drawn: from the local hour the task was made in to the timeline's end, now beside
 * them. */
export class Scale {
  readonly start: number;
  readonly span: number;
  readonly end: number;
  readonly now: number;

  constructor(readonly timeline: TaskTimeline) {
    const created = (timeline.history?.messages ?? []).reduce(
      (first, message) => Math.min(first, message.at),
      timeline.created,
    );
    const offset = offsetAt(timeline, created);
    this.start = created - remEuclid(created + offset, 3600);
    this.end = (timeline.history?.messages ?? []).reduce(
      (last, message) => Math.max(last, message.at),
      timelineEnd(timeline),
    );
    this.span = Math.max(this.end - this.start, 60);
    this.now = timeline.now;
  }

  at(at: number): number {
    return clamp(((at - this.start) / this.span) * 100, 0, 100);
  }

  width(from: number, to: number): number {
    return this.at(to) - this.at(from);
  }

  clock(at: number): string {
    return clockOf(this.timeline, at);
  }

  /**
   * The hours marked: the step's multiples from local midnight (midnight itself for a step of a
   * day or more), so a change of day is always marked, by its date; while the task is `open`,
   * none so near now that its label would meet now's.
   */
  ticks(open: boolean): Tick[] {
    const step =
      TICK_HOURS.map((hours) => hours * 3600).find((step) => this.span / step <= 8) ?? 168 * 3600;
    const every = Math.trunc(step / 60);
    const first = local(this.start, offsetAt(this.timeline, this.start));
    let tick = this.start + remEuclid(-first, Math.min(every, 1440)) * 60;
    const ticks: Tick[] = [];
    while (tick <= this.end) {
      const left = this.at(tick);
      if (!open || this.at(this.now) - left >= 4) {
        const offset = offsetAt(this.timeline, tick);
        const midnight = remEuclid(local(tick, offset), 1440) === 0;
        ticks.push({ left, label: midnight ? date(tick, offset) : clock(tick, offset) });
      }
      tick += step;
    }
    return ticks;
  }
}

/** The names in `names`, each once, in the order first met. */
function eachOnce(names: string[]): string {
  const seen: string[] = [];
  for (const name of names) {
    if (name !== "" && !seen.includes(name)) seen.push(name);
  }
  return seen.join(", ");
}

/** A run's name under its bar, its lane saying whether it is a build or a test: its command's first
 * word ("verify" for `just verify`, "lint-rs" for `just lint-rs`), or "running" while it runs;
 * then its result, unless it went green. */
export function short(run: Run): string {
  let name: string;
  if (run.result === "running") name = "running";
  else {
    const recipe = run.label.startsWith("just ") ? run.label.slice("just ".length) : run.label;
    name = words(recipe)[0] ?? recipe;
  }
  return run.result === "green" || run.result === "running"
    ? name
    : `${name} ${resultWord(run.result)}`;
}

/** The stage lane: each stretch the task held a status, the wait to land drawn as waiting; a close
 * is an instant, not drawn. */
function stageSegs(timeline: TaskTimeline, scale: Scale, costs?: Costs): Seg[] {
  const segs: Seg[] = [];
  for (const [index, stage] of timeline.stages.entries()) {
    if (closed(stage)) continue;
    const to = stage.to ?? scale.now;
    const width = scale.width(stage.from, to);
    if (!(width > 0)) continue;
    const approved = stage.approved === true;
    const word = approved ? "waiting to land" : stage.status.replaceAll("_", " ");
    const length = took(to - stage.from);
    const cost = stageCost(timeline, costs, index);
    const spent = cost.figure === null ? "" : ` ${cost.figure}`;
    segs.push({
      left: scale.at(stage.from),
      width,
      tone: statusTone(stage.status),
      look: approved ? "waiting" : "solid",
      // Its word, its time and its cost; else its time and its cost; else its time alone.
      text: fittedFirst([`${word} ${length}${spent}`, `${length}${spent}`, length], width),
      after: null,
      under: null,
      row: 0,
      title: `${word} ${length}${spent}, from ${scale.clock(stage.from)} to ${scale.clock(to)}${cost.words}`,
      stop: true,
    });
  }
  return segs;
}

/** A place lane: each stretch of the task's places that shares one phase, or one state (`key`). */
function placeSegs(timeline: TaskTimeline, scale: Scale, key: "phase" | "state"): Seg[] {
  return stretches(timeline, key).flatMap((stretch): Seg[] => {
    const width = scale.width(stretch.from, stretch.to);
    if (!(width > 0)) return [];
    const length = took(stretch.to - stretch.from);
    return [
      {
        left: scale.at(stretch.from),
        width,
        tone: statusTone(stretch.status),
        look: "solid",
        text: fitted(`${stretch.label} ${length}`, length, width),
        after: null,
        under: null,
        row: 0,
        title: stretchTitle(stretch, length, (at) => scale.clock(at)),
        stop: true,
      },
    ];
  });
}

/** The executor lane: each claim, the first saying so where one executor held them all, each
 * naming its executor where several did. */
function claimSegs(timeline: TaskTimeline, scale: Scale, one: boolean): Seg[] {
  return timeline.claims.map((claim: Claim, index) => {
    const to = claim.to ?? scale.now;
    const width = scale.width(claim.from, to);
    const length = took(to - claim.from);
    const word = one ? (index === 0 ? "claimed" : "") : claim.by;
    const long = word === "" ? length : `${word} ${length}`;
    return {
      left: scale.at(claim.from),
      width,
      tone: "primary",
      look: "solid",
      text: fitted(long, length, width),
      after: null,
      under: null,
      row: 0,
      title: `${claim.by} held it ${length}, from ${scale.clock(claim.from)} to ${scale.clock(to)}`,
      stop: true,
    };
  });
}

/** The reviewer lane: each round's wait for a reviewer, drawn as waiting, then the reviewer's
 * time on it, its verdict after it where there is room before the next round. */
function roundSegs(timeline: TaskTimeline, scale: Scale): Seg[] {
  const segs: Seg[] = [];
  timeline.rounds.forEach((round: Round, index) => {
    const to = round.to ?? scale.now;
    const takenAt = round.taken ?? null;
    const taken = takenAt === null ? null : clamp(takenAt, round.from, to);
    const next = timeline.rounds[index + 1];
    const room: Room = {
      room: (next === undefined ? 100 : scale.at(next.from)) - scale.at(to),
      last: next === undefined,
    };
    const verdict = round.verdict ?? null;
    const word = verdict === null ? null : label(verdictWord(verdict), room);
    const after: [Fit, string, Room] | null =
      verdict === null || word === null ? null : [word, verdictTone(verdict), room];
    const name = `round ${round.round}`;
    const by = round.by ?? "nobody yet";
    const wait = took((taken ?? to) - round.from);
    const said = verdict === null ? "" : `, ${verdictWord(verdict)}`;
    const title =
      taken === null
        ? `${name}: waited ${wait} for a reviewer${said}`
        : `${name}: waited ${wait} for a reviewer, then ${by} took ${took(to - taken)}${said}`;
    const waiting = scale.width(round.from, taken ?? to);
    if (waiting > 0 || taken === null) {
      segs.push({
        left: scale.at(round.from),
        width: waiting,
        tone: "question",
        look: "waiting",
        text: taken === null ? fitted(name, "", waiting) : [],
        after: taken === null ? after : null,
        under: null,
        row: 0,
        title,
        stop: taken === null,
      });
    }
    if (taken !== null) {
      const width = scale.width(taken, to);
      segs.push({
        left: scale.at(taken),
        width,
        tone: "question",
        look: "solid",
        text: fitted(name, "", width),
        after,
        under: null,
        row: 0,
        title,
        stop: true,
      });
    }
  });
  return segs;
}

/**
 * The rows of a lane of runs: each run in the first row where the run before it ended `gap`
 * seconds or more before its queue, else a row of its own, so runs that overlap or meet take a row
 * each, as many as they need, and no two are drawn as one bar. Each run's row, and how many rows
 * there are.
 */
export function runRows(runs: Run[], now: number, gap = 0): [number[], number] {
  const ends: number[] = [];
  const placed = runs.map((run) => {
    let row = ends.findIndex((free) => free + gap <= run.queued);
    if (row < 0) {
      ends.push(Number.NEGATIVE_INFINITY);
      row = ends.length - 1;
    }
    ends[row] = Math.max(ends[row] as number, run.ended ?? now);
    return row;
  });
  return [placed, Math.max(ends.length, 1)];
}

/** A lane of `runs`, the builds' or the tests': each run's queue, drawn as waiting with its name
 * under it where there is room before the next in its row, then its run in its result's tone,
 * striped while it runs. Its segments, how many rows they take, and whether they are more than
 * [`WORDED_ROWS`], each run's name then left to its hover, its focus and the list. */
function runSegs(runs: Run[], scale: Scale): [Seg[], number, boolean] {
  const now = scale.now;
  const [placed, count] = runRows(runs, now, (scale.span * RUN_GAP) / 100);
  const dense = count > WORDED_ROWS;
  const segs: Seg[] = [];
  runs.forEach((run, index) => {
    const row = placed[index] as number;
    const began = run.started ?? null;
    const started = began ?? run.ended ?? now;
    let next: Run | undefined;
    for (let later = index + 1; later < runs.length; later += 1) {
      if (placed[later] === row) {
        next = runs[later];
        break;
      }
    }
    const room: Room = {
      room: (next === undefined ? 100 : scale.at(next.queued)) - scale.at(run.queued),
      last: next === undefined,
    };
    const length = lasted(began === null ? waited(run, now) : ran(run, now));
    const title = runTitle(run, now);
    const under = dense ? null : label(`${short(run)} ${length}`, room);
    segs.push({
      left: scale.at(run.queued),
      width: scale.width(run.queued, started),
      tone: "primary",
      look: "waiting",
      text: [],
      after: null,
      under: under === null ? null : [under, room],
      row,
      title,
      stop: began === null,
    });
    if (began !== null) {
      segs.push({
        left: scale.at(began),
        width: scale.width(began, run.ended ?? now),
        tone: resultTone(run.result),
        look: run.result === "running" ? "running" : "solid",
        text: [],
        after: null,
        under: null,
        row,
        title,
        stop: true,
      });
    }
  });
  return [segs, count, dense];
}

/** Each gate's thin row: each state from when it was taken to the next, the last to the end, its
 * word where it fits. */
function gateRows(timeline: TaskTimeline, scale: Scale): GateRow[] {
  return timeline.gates.map((lane: GateLane) => ({
    name: lane.name,
    spans: lane.states.flatMap((span, index) => {
      const to = lane.states[index + 1]?.from ?? scale.end;
      const width = scale.width(span.from, to);
      if (!(width > 0)) return [];
      const from = widthFor(span.state, width, PAD_PX);
      const detail = span.detail === "" ? "" : `: ${span.detail}`;
      return [
        {
          left: scale.at(span.from),
          width,
          state: span.state,
          word: from === undefined ? null : { text: span.state, from, to: null },
          title: `${lane.name} ${span.state} from ${scale.clock(span.from)}${detail}`,
        },
      ];
    }),
  }));
}

/** The wide view of `timeline`: the stage lane always, the executor's, the reviewer's, the builds'
 * and the tests' (every run not a build's) once there is something to draw in them, and a row per
 * gate. */
export function lanes(timeline: TaskTimeline, costs?: Costs): Lanes {
  const scale = new Scale(timeline);
  const open = timeline.closed === undefined || timeline.closed === null;
  const executors = eachOnce(timeline.claims.map((claim) => claim.by));
  const reviewers = eachOnce(
    timeline.rounds.flatMap((round) =>
      round.by === undefined || round.by === null ? [] : [round.by],
    ),
  );
  const one = !executors.includes(", ");
  const drawn: Lane[] = [
    {
      key: "stage",
      name: "Stage",
      who: "",
      rows: 1,
      dense: false,
      segs: stageSegs(timeline, scale, costs),
    },
  ];
  if ((timeline.places ?? []).length > 0) {
    for (const [key, name] of [
      ["phase", "Phase"],
      ["state", "State"],
    ] as const) {
      drawn.push({
        key,
        name,
        who: "",
        rows: 1,
        dense: false,
        segs: placeSegs(timeline, scale, key),
      });
    }
  }
  if (timeline.history) {
    const roles = timeline.history.roles.filter((role) => role.role === "orchestrator");
    const segs: Seg[] = roles.map((role) => {
      // Uncertain boundaries are instants, never a corrected or invented duration.
      const width = role.uncertain
        ? 0
        : scale.width(role.from, role.to ?? timeline.closed ?? timeline.now);
      return {
        left: scale.at(role.from),
        width,
        tone: "resolved",
        look: "waiting",
        text: fitted(role.by, "", width),
        after: null,
        under: null,
        row: 0,
        title: `${role.by} held orchestrator from ${role.from} to ${role.to ?? "open"}; source ${role.source}${role.uncertain ? "; uncertain boundaries" : ""}; not continuous work or connection`,
        stop: true,
      };
    });
    drawn.push({
      key: "orchestrator",
      name: "Orchestrator",
      who: "recorded role holds",
      rows: 1,
      dense: false,
      segs,
    });
  }
  if (timeline.claims.length > 0) {
    drawn.push({
      key: "executor",
      name: "Executor",
      who: executors,
      rows: 1,
      dense: false,
      segs: claimSegs(timeline, scale, one),
    });
  }
  if (timeline.rounds.length > 0) {
    drawn.push({
      key: "reviewer",
      name: "Reviewer",
      who: reviewers,
      rows: 1,
      dense: false,
      segs: roundSegs(timeline, scale),
    });
  }
  if (timeline.history?.messages.length) {
    for (const [role, name] of [
      ["executor", "Executor"],
      ["reviewer", "Reviewer"],
    ] as const) {
      if (
        !drawn.some((lane) => lane.key === role) &&
        timeline.history.messages.some(
          (m) =>
            endpointLane(m.from, m.sender) === role || endpointLane(m.to, m.recipient) === role,
        )
      ) {
        drawn.push({
          key: role,
          name,
          who: "holder unknown",
          rows: 1,
          dense: false,
          segs: [],
        });
      }
    }
    drawn.push({
      key: "other",
      name: "Other / task messages",
      who: "outside, everyone or unknown",
      rows: 1,
      dense: false,
      segs: [],
    });
  }
  const builds = timeline.runs.filter((run) => run.phase === "build");
  const tests = timeline.runs.filter((run) => run.phase !== "build");
  for (const [key, name, runs] of [
    ["builds", "Builds", builds],
    ["tests", "Tests", tests],
  ] as const) {
    if (runs.length === 0) continue;
    const [segs, count, dense] = runSegs(runs, scale);
    const who = eachOnce(runs.map((run) => run.host ?? ""));
    drawn.push({ key, name, who, rows: count, dense, segs });
  }
  return {
    ticks: scale.ticks(open),
    now: open ? { left: scale.at(scale.now), label: `now ${scale.clock(scale.now)}` } : null,
    lanes: drawn,
    gates: gateRows(timeline, scale),
  };
}

/** The legend's gate states in the approved picture's order, those the lanes draw. */
export function legendStates(drawn: Lanes): GateState[] {
  const states: GateState[] = ["met", "missing", "waived", "held", "unknown"];
  return states.filter((state) =>
    drawn.gates.some((gate) => gate.spans.some((span) => span.state === state)),
  );
}

// The reads.

/** What a read that failed with nothing shown says of the next try, which the page's store makes
 * by itself (parts.ts), as the timeline's sentence says it. trouble.tsx's `TRYING` is the same
 * words, kept apart: naming it here would have the chunk every page loads export it. */
export const AGAIN = "Trying again…";

/** What a read that failed says, from the reason it failed for: its first line, the way the gates
 * and a page that did not load say theirs, and that it is tried again where it is (`again`). */
export function failed(why: string, again = true): string {
  const first = trimmed(why.split("\n")[0] ?? "").replace(/\.+$/, "");
  const said =
    first === "" ? "The timeline could not be read." : `The timeline could not be read: ${first}.`;
  return again ? `${said} ${AGAIN}` : said;
}

/** What the timeline's place shows: nothing yet, a timeline, or why its read failed. */
export type Shown = null | { timeline: TaskTimeline } | { error: string; final?: true };

/**
 * Seconds until the timeline asks to be read again after `shown`: its own `every` while its task
 * is open; none once the task is closed, before any read, or after a read that failed with nothing
 * shown, which the page's store tries again by itself.
 */
export function nextRead(shown: Shown): number | null {
  if (shown === null || "error" in shown) return null;
  const closed = shown.timeline.closed ?? null;
  return closed === null ? everySeconds(shown.timeline) : null;
}

/** What "Show all" says: the phone's list's steps it shows, counted. */
export function showAll(steps: number): string {
  return `Show all ${steps} ${plural(steps, "step")}`;
}

// The read's guard: what serde reads of `/api/task-timeline`.

const isText = (value: unknown): value is string => typeof value === "string";
const isNumber = (value: unknown): value is number => typeof value === "number";
/** As serde reads an `Option`: absent and `null` are none. */
const absent = (value: unknown): value is undefined | null => value === undefined || value === null;
const isCount = (value: unknown): value is number =>
  Number.isInteger(value) && (value as number) >= 0 && (value as number) <= 0xffff_ffff;

/** An `Option<f64>` field: none, a number, or `undefined` for neither (refused). */
function optionalNumber(value: unknown): number | null | undefined {
  if (absent(value)) return null;
  return isNumber(value) ? value : undefined;
}

/** An `Option<String>` field, as `optionalNumber`. */
function optionalText(value: unknown): string | null | undefined {
  if (absent(value)) return null;
  return isText(value) ? value : undefined;
}

/** A field serde fills with its default when absent: `fallback` then, the value when `guard` takes
 * it, `undefined` (refused) otherwise. */
function defaulted<T>(
  value: unknown,
  guard: (value: unknown) => value is T,
  fallback: T,
): T | undefined {
  if (value === undefined) return fallback;
  return guard(value) ? value : undefined;
}

/** A `#[serde(default)]` list of what `read` reads: empty when absent, refused (null) when it or
 * an item is not what it reads. */
function listOf<T>(value: unknown, read: (item: unknown) => T | null): T[] | null {
  if (value === undefined) return [];
  if (!Array.isArray(value)) return null;
  const items = value.map(read);
  return items.some((item) => item === null) ? null : (items as T[]);
}

/** A word of one of `known`, a word this client does not know reading as `other`, as serde's
 * `#[serde(other)]` reads it; refused (undefined) when it is not a word. */
function word<T extends string>(value: unknown, known: readonly T[], other: T): T | undefined {
  if (!isText(value)) return undefined;
  return (known as readonly string[]).includes(value) ? (value as T) : other;
}

const VERDICTS = ["approved", "changes"] as const;
const RESULTS = ["queued", "running", "green", "red"] as const;
const STATES = ["missing", "held", "waived", "met"] as const;

function readOffset(value: unknown): Offset | null {
  const [from, seconds] = [field(value, "from"), field(value, "seconds")];
  return isNumber(from) && Number.isSafeInteger(seconds)
    ? { from, seconds: seconds as number }
    : null;
}

function readStage(value: unknown): Stage | null {
  const [status, from] = [field(value, "status"), field(value, "from")];
  const to = optionalNumber(field(value, "to"));
  const by = optionalText(field(value, "by"));
  const round = defaulted(field(value, "round"), isCount, 0);
  const approved = defaulted(
    field(value, "approved"),
    (one): one is boolean => typeof one === "boolean",
    false,
  );
  if (!isText(status) || !isNumber(from)) return null;
  if (to === undefined || by === undefined || round === undefined || approved === undefined) {
    return null;
  }
  return { status, from, to, by, round, approved };
}

function readClaim(value: unknown): Claim | null {
  const [by, from] = [field(value, "by"), field(value, "from")];
  const to = optionalNumber(field(value, "to"));
  return isText(by) && isNumber(from) && to !== undefined ? { by, from, to } : null;
}

function readRound(value: unknown): Round | null {
  const [number, from] = [field(value, "round"), field(value, "from")];
  const taken = optionalNumber(field(value, "taken"));
  const to = optionalNumber(field(value, "to"));
  const by = optionalText(field(value, "by"));
  const said = field(value, "verdict");
  const verdict = absent(said) ? null : word<RoundVerdict>(said, VERDICTS, "other");
  if (!isCount(number) || !isNumber(from)) return null;
  if (taken === undefined || to === undefined || by === undefined || verdict === undefined) {
    return null;
  }
  return { round: number, from, taken, to, by, verdict };
}

function readRun(value: unknown): Run | null {
  const [phase, text, queued] = [
    field(value, "phase"),
    field(value, "label"),
    field(value, "queued"),
  ];
  const by = optionalText(field(value, "by"));
  const commit = optionalText(field(value, "commit"));
  const host = defaulted(field(value, "host"), isText, "");
  const started = optionalNumber(field(value, "started"));
  const ended = optionalNumber(field(value, "ended"));
  const result = word<RunResult>(field(value, "result"), RESULTS, "unknown");
  if (!isText(phase) || !isText(text) || !isNumber(queued)) return null;
  if (
    by === undefined ||
    commit === undefined ||
    host === undefined ||
    started === undefined ||
    ended === undefined ||
    result === undefined
  ) {
    return null;
  }
  return { phase, label: text, by, commit, host, queued, started, ended, result };
}

function readSpan(value: unknown): GateSpan | null {
  const state = word<GateState>(field(value, "state"), STATES, "unknown");
  const from = field(value, "from");
  const by = optionalText(field(value, "by"));
  const detail = defaulted(field(value, "detail"), isText, "");
  if (state === undefined || !isNumber(from) || by === undefined || detail === undefined) {
    return null;
  }
  return { state, from, by, detail };
}

function readLane(value: unknown): GateLane | null {
  const name = field(value, "name");
  const states = listOf(field(value, "states"), readSpan);
  return isText(name) && states !== null ? { name, states } : null;
}

/** A place a custom workflow put the task at, or none for one not in the shape this client
 * reads. */
function readPlace(value: unknown): PlaceSpan | null {
  const [phase, phase_label, state, state_label, status, by, from] = [
    field(value, "phase"),
    field(value, "phase_label"),
    field(value, "state"),
    field(value, "state_label"),
    field(value, "status"),
    field(value, "by"),
    field(value, "from"),
  ];
  const edge = optionalText(field(value, "edge"));
  if (
    !isText(phase) ||
    !isText(phase_label) ||
    !isText(state) ||
    !isText(state_label) ||
    !isText(status) ||
    !isText(by) ||
    !isNumber(from) ||
    edge === undefined
  ) {
    return null;
  }
  return { phase, phase_label, state, state_label, status, from, by, edge };
}

/**
 * `/api/task-timeline`'s answer as this client reads it, or none for one without what the
 * timeline draws. A field serde defaults (a list, a count, a flag, a word) takes its default when
 * absent, an `Option` is none when absent or null, and a word a newer server writes for a verdict,
 * a run's result or a gate's state reads as the catch-all (`#[serde(other)]`). The slug, which
 * nothing draws, is not checked. Places it cannot read are left out, never the timeline, as the
 * server leaves out a task's it cannot read.
 */
export function readTimeline(value: unknown): TaskTimeline | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const [now, created, slug] = [field(value, "now"), field(value, "created"), field(value, "slug")];
  const zone = listOf(field(value, "zone"), readOffset);
  const every = defaulted(field(value, "every"), isCount, 0);
  const runsFrom = optionalNumber(field(value, "runs_from"));
  const closedAt = optionalNumber(field(value, "closed"));
  const firstClaimed = optionalNumber(field(value, "first_claimed"));
  const bounces = defaulted(field(value, "bounces"), isCount, 0);
  const stages = listOf(field(value, "stages"), readStage);
  const claims = listOf(field(value, "claims"), readClaim);
  const rounds = listOf(field(value, "rounds"), readRound);
  const runs = listOf(field(value, "runs"), readRun);
  const gates = listOf(field(value, "gates"), readLane);
  const places = listOf(field(value, "places"), readPlace) ?? [];
  const rawHistory = field(value, "history");
  const history =
    rawHistory === undefined || rawHistory === null ? undefined : readHistory(rawHistory);
  if (history === null) return null;
  if (!isNumber(now) || !isNumber(created)) return null;
  if (
    zone === null ||
    every === undefined ||
    runsFrom === undefined ||
    closedAt === undefined ||
    firstClaimed === undefined ||
    bounces === undefined ||
    stages === null ||
    claims === null ||
    rounds === null ||
    runs === null ||
    gates === null
  ) {
    return null;
  }
  return {
    slug: isText(slug) ? slug : "",
    now,
    zone,
    every,
    runs_from: runsFrom,
    created,
    closed: closedAt,
    first_claimed: firstClaimed,
    bounces,
    stages,
    claims,
    rounds,
    runs,
    gates,
    ...(places.length === 0 ? {} : { places }),
    ...(history === undefined ? {} : { history }),
  };
}
