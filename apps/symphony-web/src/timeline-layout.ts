// A task's timeline laid out, with no DOM: what it went through, read from `/api/task-timeline`
// (crates/symphony-api's `timeline`), as the phone's list and the wide lanes draw it.
//
// - On a phone, a list (`rows`): a row per step with its start, its label, how long it took and a
//   bar as long, its wait drawn lighter; the builds, tests and gate changes indented under the
//   stage they happened in, each build and test named by its command; the verdict after a review's
//   bar, and a striped bar for what runs now.
// - Wide, lanes over the same hours (`lanes`): the stage, the executor, the reviewer, the builds
//   and the tests, each a lane of its own, and a thin row per gate, its state drawn by a pattern of
//   its own as well as a colour, with a line at now. A run takes the first row of its lane free
//   when it is queued, so only runs that overlap open another (`runRows`), each one bar at its
//   recorded times, its queue the lighter head of it, its distinguishing words (`runWords`) and its
//   length inside it once it is BAR_WORDS_PX wide; a runs lane's name says its counts, and where
//   the recorder keeps no runs (before `runs_from`) the lanes are hatched (`NotKept`). With
//   messages, each role's lane and the board's and others' carry a line at their foot (`rail`),
//   on which timeline.tsx draws each message as a tick at its sender's lane. Given a view (a part
//   of the hours, timeline-zoom.ts), every part is placed in it and its words fitted to it, while
//   whether a part is drawn at all is the whole hours' (`Scale`).
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
import { type Cut, Cuts } from "./timeline-cut";
import { address, endpointLane, readHistory } from "./timeline-history";
import { type Quiet, quietStretches } from "./timeline-quiet";
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
import { fixed, plural, twoDigits } from "./words";

/** How much of a row the longest step's bar takes on a phone, in percent: the rest is for the word
 * after it. */
export const BAR_SPAN = 72;

/** The padding a segment's words take beside them, and the gap a label keeps from the next; every
 * word of the lanes is weighed letter by letter (`textPx`). */
export const PAD_PX = 12;
export const GAP_PX = 4;
/** The lanes' margin after the track's end (`--end`, 5.5rem), which the last label may take. */
export const END_PX = 88;
/** The inset a label's words keep from its bar (`insetCss`). */
export const AFTER_PAD_PX = 6;
/** A segment's least drawn width, however short its time: its padding and its 1 px border on each
 * side (`.tl-seg`). A label after it, and the next segment's words, start past it as drawn. */
export const SEG_MIN_PX = PAD_PX + 2;

/** The lanes' small type, in pixels (`--fs-xs`, 0.75rem). */
const XS_PX = 12;

/**
 * Each character's advance in the lanes' small type from " " to "~", in thousandths of its size:
 * the widest of the faces the page's sans-serif names (`--font-sans`: -apple-system, which Safari
 * and the Mac app draw as the system's face, "Segoe UI", Helvetica and Arial) as each draws it at
 * 12 px, at either weight the lanes set words in (400 and 600), rounded up. Each face's figures,
 * and how they were measured, are timeline-layout.test.ts's, which holds this table to them.
 */
const ADVANCE: readonly number[] = [
  282, 342, 539, 658, 658, 999, 850, 331, 414, 414, 481, 708, 331, 481, 331, 444, 667, 576, 630,
  655, 673, 649, 668, 593, 676, 668, 334, 334, 708, 708, 708, 611, 1016, 723, 723, 733, 741, 667,
  611, 778, 770, 317, 581, 723, 611, 958, 791, 782, 667, 782, 723, 667, 652, 753, 706, 1005, 713,
  691, 671, 414, 437, 414, 708, 613, 500, 577, 639, 579, 639, 591, 393, 634, 618, 285, 285, 582,
  285, 917, 613, 612, 635, 635, 417, 557, 397, 613, 570, 823, 562, 580, 558, 414, 327, 414, 708,
];
/** A middle dot's advance, as ADVANCE weighs a character. */
const MIDDLE_DOT = 334;
/** The widest of the faces' tabular figures, which the system's face draws wider than its own "1",
 * "2", "3", "5" and "7" (`font-variant-numeric: tabular-nums`, the hours' and a band's words). */
const TABULAR = 658;

/** `char`'s advance in the lanes' small type, in thousandths of its size (`ADVANCE`), a digit's in
 * tabular figures where `tabular` says so: one with an accent weighs its letter, a middle dot as
 * MIDDLE_DOT, and any other the whole size, which no face draws wider. */
export function advance(char: string, tabular = false): number {
  const letter = char.normalize("NFD")[0] ?? char;
  if (letter === "·") return MIDDLE_DOT;
  const own = ADVANCE[letter.charCodeAt(0) - 32] ?? 1000;
  return tabular && letter >= "0" && letter <= "9" ? Math.max(own, TABULAR) : own;
}

/** How wide `text` is drawn in the lanes' small type, in pixels, its digits in tabular figures
 * where `tabular` says so: its characters' advances (`advance`) and half a pixel for the browser's
 * rounding of them, so never under it. A word the estimate lets through is drawn whole in each face
 * the stack names. */
export function textPx(text: string, tabular = false): number {
  let advances = 0;
  for (const char of text) advances += advance(char, tabular);
  return (advances * XS_PX) / 1000 + 0.5;
}

/** The tracks of the common page widths, in pixels: 1280, 1440, and 1600 or wider, where the track
 * stops growing. The timeline browser test measures each at its page width. */
export const TRACKS: readonly number[] = [935, 1095, 1158];

/**
 * The track widths a word's room is weighed at, in pixels, ascending: every 50 from 400 to 1600,
 * and TRACKS, so a word whose room holds it at a common page's track shows there rather than from
 * the next 50. A word shows from the first of them its room holds it at (`tl-from-W`, `tl-to-W`
 * in the style sheet: two container queries a width), at every width when 300 does, and never
 * when 1600 does not.
 */
export const WIDTHS: readonly number[] = Array.from({ length: 25 }, (_, at) => 400 + at * 50)
  .concat(TRACKS)
  .sort((a, b) => a - b);

/** How wide a run's bar is, in pixels, before its words go inside it, cut to fit; narrower, they
 * are in its hover, its focus and its row in the list. */
export const BAR_WORDS_PX = 30;
/** The room the not-kept hatch's words take beside them: their inset and their padding. */
const NOT_KEPT_PAD_PX = 22;

/** The seconds between the wide view's ticks: the first that draws no more than eight, from an
 * hour up; zoomed, from a minute up. */
const TICK_HOURS = [1, 2, 3, 4, 6, 8, 12, 24, 72, 168].map((hours) => hours * 3600);
const ZOOMED_TICKS = [1, 2, 5, 10, 15, 30].map((minutes) => minutes * 60).concat(TICK_HOURS);

/** Statuses a task ends in: an instant on the timeline, with no time of its own. */
const CLOSED = ["done", "cancelled"];

/** The lanes a message starts or ends at (timeline-history.ts `endpointLane`). */
const RAILS = ["orchestrator", "executor", "reviewer", "other"];

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

/** When the recorder's rotated history begins, `runs_from`, as a time says it: "11-19 11:09";
 * none where it keeps every run of the task. */
function keptFrom(timeline: TaskTimeline): string | null {
  const from = timeline.runs_from ?? null;
  return from === null ? null : `${dateOf(timeline, from)} ${clockOf(timeline, from)}`;
}

/** What the timeline cannot show: the runs the recorder's rotated history let go, before
 * `runs_from`, said by the hatch's hover wide and the kept row's on a phone. */
export function keptTitle(timeline: TaskTimeline): string | null {
  const from = keptFrom(timeline);
  return from === null ? null : `Builds and tests before ${from} are not kept`;
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
  /** The row at `runs_from`, from which the recorder keeps every build and test. */
  kept?: true;
  /** A quiet stretch's row: why it is quiet, said under its label ("Task done, nothing
   * recorded"). */
  quiet?: string;
}

/** The phone's row at `runs_from`, from which the builds and tests are kept: none where they all
 * are. */
export function keptRow(timeline: TaskTimeline): Row | null {
  const from = timeline.runs_from ?? null;
  const title = keptTitle(timeline);
  if (from === null || title === null) return null;
  return {
    start: from,
    day: null,
    at: clockOf(timeline, from),
    sub: true,
    event: true,
    tone: "muted-foreground",
    label: "Builds and tests kept from here on",
    took: null,
    bar: null,
    word: null,
    title,
    kept: true,
  };
}

/** Why a quiet stretch is quiet, as its band and its row say it: the status the task held through
 * it, and that nothing was recorded ("task done, nothing recorded"). */
export function quietWhy(quiet: Quiet): string {
  if (quiet.status === "") return "nothing recorded";
  const status = quiet.status.replaceAll("_", " ");
  return `${CLOSED.includes(quiet.status) ? `task ${status}` : status}, nothing recorded`;
}

/** A quiet stretch as its row and its band say it: its length, then its ends, the end's day where
 * it is another ("10 h 54 m quiet, 14:03 → 11-20 00:57"), from `day` where given. */
export function quietSaid(timeline: TaskTimeline, quiet: Quiet, day = false): string {
  const at = (time: number) => `${dateOf(timeline, time)} ${clockOf(timeline, time)}`;
  const ends = dateOf(timeline, quiet.from) === dateOf(timeline, quiet.to);
  const from = day ? at(quiet.from) : clockOf(timeline, quiet.from);
  const to = ends && !day ? clockOf(timeline, quiet.to) : at(quiet.to);
  return `${took(quiet.to - quiet.from)} quiet, ${from} → ${to}`;
}

/** The phone's row of a quiet stretch, at its start, in place of a scale: drawn whether or not the
 * lanes skip it. */
export function quietRow(timeline: TaskTimeline, quiet: Quiet): Row {
  const label = quietSaid(timeline, quiet);
  const why = quietWhy(quiet);
  return {
    start: quiet.from,
    day: null,
    at: clockOf(timeline, quiet.from),
    sub: false,
    event: true,
    tone: "muted-foreground",
    label,
    took: null,
    bar: null,
    word: null,
    title: `${label}: ${why}`,
    quiet: capitalised(why),
  };
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
  const kept = keptRow(timeline);
  // The kept row first, so a run queued at its instant goes after it (the sort is stable).
  const under = [
    ...(kept === null ? [] : [kept]),
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
    // A quiet stretch begins at the record before it: its row goes after the rows of that instant.
    for (const quiet of quietStretches(timeline)) listed.push(quietRow(timeline, quiet));
    const quietLast = (row: Row) => (row.quiet === undefined ? 0 : 1);
    listed.sort(
      (a, b) =>
        a.start - b.start || quietLast(a) - quietLast(b) || (a.message ?? 0) - (b.message ?? 0),
    );
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
  return fromWidth(((textPx(text) + pad) * 100) / room);
}

/** The first of WIDTHS at which a track is `needed` pixels wide or more: null where 300 is,
 * undefined where 1600 is not. */
function fromWidth(needed: number): number | null | undefined {
  if (needed <= (WIDTHS[0] as number) - 100) return null;
  return WIDTHS.find((width) => width >= needed);
}

/** `text` inside a run's bar `room` percent of the track wide: from the first of WIDTHS at which
 * the bar is BAR_WORDS_PX wide, cut to fit there; none where it never is. */
export function barFit(text: string, room: number): Fit[] {
  if (text === "" || !(room > 0)) return [];
  const from = fromWidth((BAR_WORDS_PX * 100) / room);
  return from === undefined ? [] : [{ text, from, to: null }];
}

/** Room a label beside a segment has: `room` percent of the track before the next, and the lanes'
 * end margin past the track's end when nothing follows it (`last`); `bar`, the segment's own width
 * in percent, which the label starts past as drawn. */
export interface Room {
  room: number;
  last: boolean;
  bar: number;
}

/** A room's width, for the label's `max-width`: past it a label is cut short, never drawn over the
 * next. */
export function roomCss(room: Room): string {
  return room.last ? `calc(${fixed(room.room, 2)}% + var(--end))` : `${fixed(room.room, 2)}%`;
}

/** A label's inset, for its `padding-left`: AFTER_PAD_PX past its bar as drawn, which is
 * SEG_MIN_PX wide where its time is narrower. */
export function insetCss(room: Room): string {
  return `max(${AFTER_PAD_PX}px, calc(${SEG_MIN_PX + AFTER_PAD_PX}px - ${fixed(room.bar, 2)}%))`;
}

/** The track widths below which a word is weighed as at every width (`fromWidth`). */
const NARROWEST = (WIDTHS[0] as number) - 100;

/** The least a segment's words keep from the segment before it drawn over its padding, as the
 * approved pictures draw a round's name past its few minutes' wait. */
const CLEAR_PX = 3;
/** How far past its own border and padding a segment's words would be reached at the start of a
 * segment that begins where the one before it does: that one's least width and CLEAR_PX. */
const REACH_PX = SEG_MIN_PX + CLEAR_PX - SEG_MIN_PX / 2;

/**
 * How far `placed` starts past the start of `prev`, the segment before it in its row, in percent
 * of the track, where `prev` drawn at its least width (SEG_MIN_PX) reaches its words, or within
 * CLEAR_PX of them, on a track as narrow as a word is weighed at: its words then start CLEAR_PX
 * past `prev` as drawn (`segIndentCss`), and are weighed with that inset (`fittedFirst`). Undefined
 * where `prev` never comes so near.
 */
export function past(prev: Placed | undefined, placed: Placed): number | undefined {
  if (prev === undefined || prev.out) return undefined;
  const before = placed.left - prev.left;
  // One that ends where it starts, the two placed apart to the last digit, does not overlap it.
  const apart = before >= prev.width - 1e-9;
  return apart && (before * NARROWEST) / 100 < REACH_PX ? before : undefined;
}

/** A segment's words' indent past its padding, for its `text-indent`, starting `before` percent
 * of the track past the segment before it (`past`): CLEAR_PX past that one as drawn, where it comes
 * nearer. In the track's own units (`cqw`, the track an inline-size container), and an indent
 * rather than a padding, which would draw a segment with no room for words wider than its time. */
export function segIndentCss(before: number): string {
  return `max(0px, calc(${REACH_PX}px - ${fixed(before, 2)}cqw))`;
}

/** A segment's inset, as its `inset` field, where its words start past the one before it. */
function insetOf(before: number | undefined): { inset?: number } {
  return before === undefined ? {} : { inset: before };
}

/**
 * `text` as a label beside a segment, in `room`: from the first of WIDTHS at which its words
 * (`textPx`), its inset (`insetCss`) and its gap from the next fit there, at every width when 300
 * holds them, and never when 1600 does not. Its inset narrows as the track widens, so where it fits
 * once, it fits on every wider track.
 */
export function label(text: string, room: Room): Fit | null {
  const wide = textPx(text) + GAP_PX;
  const fits = (track: number) => {
    const inset = Math.max(AFTER_PAD_PX, SEG_MIN_PX + AFTER_PAD_PX - (room.bar * track) / 100);
    return wide + inset <= (room.room * track) / 100 + (room.last ? END_PX : 0);
  };
  if (fits((WIDTHS[0] as number) - 100)) return { text, from: null, to: null };
  const from = WIDTHS.find(fits);
  return from === undefined ? null : { text, from, to: null };
}

/**
 * The first of `texts`, longest first, that fits in a segment `room` percent wide, inside its
 * padding and its border (SEG_MIN_PX) and past the segment `before` percent before it where that
 * one, drawn at its least width, reaches in (`past`): each drawn only at the widths that hold it and
 * no longer one.
 */
export function fittedFirst(texts: readonly string[], room: number, before?: number): Fit[] {
  const fits: Fit[] = [];
  // The width from which a longer one is drawn: none yet.
  let to: number | null = null;
  for (const text of texts) {
    if (text === "" || fits.some((fit) => fit.text === text)) continue;
    const from = segWidthFor(text, room, before);
    if (from === undefined) continue;
    if (from !== null && to !== null && from >= to) continue;
    fits.push({ text, from, to });
    if (from === null) break;
    to = from;
  }
  return fits;
}

/** `long` where it fits in a segment `room` percent wide, else `short` where that fits, past the
 * segment `before` percent before it (`fittedFirst`): each drawn only at the widths that hold it. */
export function fitted(long: string, short: string, room: number, before?: number): Fit[] {
  return fittedFirst([long, short], room, before);
}

/**
 * The first of WIDTHS at which `text` fits in a segment `room` percent wide (`widthFor`, inside
 * SEG_MIN_PX), and past the segment `before` percent before it: that one, drawn SEG_MIN_PX wide,
 * takes `REACH_PX - before%` more of it until the track draws `before` as wide, so the words need
 * `room% ≥ words` and `room% ≥ words + REACH_PX - before%`.
 */
function segWidthFor(text: string, room: number, before?: number): number | null | undefined {
  if (before === undefined) return widthFor(text, room, SEG_MIN_PX);
  if (text === "" || room <= 0) return undefined;
  const words = textPx(text) + SEG_MIN_PX;
  return fromWidth(Math.max((words * 100) / room, ((words + REACH_PX) * 100) / (room + before)));
}

/** A segment of a lane, placed in percent of the hours shown. */
export interface Seg extends Placed {
  tone: string;
  look: Look;
  /** Its words, at the widths that hold them. */
  text: Fit[];
  /** How far it starts past the start of the segment before it in its row, in percent, where that
   * one drawn at its least width reaches into it (`past`): its words start past it as drawn. */
  inset?: number;
  /** A word right after it (a review's verdict) and its tone, where there is room before the
   * next, and that room in percent: it is cut short there, never drawn over the next. */
  after: [Fit, string, Room] | null;
  /** The row of its lane it is drawn in (runs that overlap take a row each). */
  row: number;
  /** All of it, for a pointer resting on it and for assistive technology. */
  title: string;
  /** Whether the keyboard stops at it: a part the rest of its step follows (a run's queue before
   * its run, a round's wait before its reviewer's time) is only drawn, the part after it saying
   * all of it. */
  stop: boolean;
}

/** A lane of the wide view: its key (its row's `data-lane`), its name, who worked in it (a runs
 * lane: its counts), how many rows its segments take, and its segments. */
export interface Lane {
  key: string;
  name: string;
  who: string;
  rows: number;
  /** A line at its foot that messages are drawn on, as ticks at their sender's lane: each role's
   * lane and the board's and others', on a timeline with messages. */
  rail: boolean;
  segs: Seg[];
}

/** Where the recorder keeps no runs of the task: from its making to `runs_from`, in percent of
 * the hours shown, over the builds' and the tests' lanes; its words where they fit. */
export interface NotKept {
  left: number;
  width: number;
  label: Fit | null;
  title: string;
}

/** A gate's state over the hours it held, placed in percent of the hours shown. */
export interface GateBar extends Placed {
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

/** An hour marked on the wide view's axis, in percent of the hours shown; near a band's words on
 * the hours line, its label drawn only from the track width (`wide`, one of WIDTHS) that clears
 * them. */
export interface Tick {
  left: number;
  label: string;
  wide?: number;
}

/** A quiet stretch skipped: a band through every lane, placed in percent of the hours shown. */
export interface Break extends Placed {
  /** Its length on the hours line over it, "+10 h 54 m", at the widths that clear the lengths of
   * the bands before it. */
  length: Fit | null;
  /** The day it ends on, said right after its length where a day changed inside it, at the widths
   * its length is drawn at that clear the other bands' lengths. */
  day: Fit | null;
  /** Its words down the band, "quiet · task done, nothing recorded", where it is wide enough. */
  words: Fit | null;
  /** The stretch, its ends and why it is quiet, for a pointer resting on it and for assistive
   * technology. */
  title: string;
}

/** The wide view: the hours, a line at now while the task is open, the lanes, the gates' rows, the
 * time no run is kept of, where some is, and the quiet stretches skipped. */
export interface Lanes {
  ticks: Tick[];
  now: Tick | null;
  lanes: Lane[];
  gates: GateRow[];
  notKept: NotKept | null;
  breaks: Break[];
}

/** A band's least width, in pixels, for its words down it. */
const BAND_WORDS_PX = 14;
/** The room a band's length takes beside its letters, its padding either side. */
const BREAK_PAD_PX = 8;
/** How far after a band's length its day is said, in pixels. */
const DAY_AFTER_PX = 4;

/** A word on the hours line: its place, in percent of the track, and how far it reaches before
 * and after it, in pixels. */
interface Said {
  at: number;
  before: number;
  after: number;
}

/** The first of WIDTHS at which `a` and `b` stand apart, a gap between them: null at every width,
 * undefined at none. */
function apartAt(a: Said, b: Said): number | null | undefined {
  const [first, second] = a.at <= b.at ? [a, b] : [b, a];
  const over = first.after + second.before + GAP_PX;
  if (over <= 0) return null;
  const between = second.at - first.at;
  return between > 0 ? fromWidth((over * 100) / between) : undefined;
}

/** The first of WIDTHS at which `word` stands apart from each of `others`: null at every width,
 * undefined at none. */
function apartFrom(word: Said, others: readonly Said[]): number | null | undefined {
  let from: number | null = null;
  for (const other of others) {
    const at = apartAt(word, other);
    if (at === undefined) return undefined;
    if (at !== null && (from === null || at > from)) from = at;
  }
  return from;
}

/** A word centred on `at`, `width` pixels wide. */
const centred = (at: number, width: number): Said => ({ at, before: width / 2, after: width / 2 });

/** The later of two first widths: none where either is drawn at none. */
function later(
  a: number | null | undefined,
  b: number | null | undefined,
): number | null | undefined {
  if (a === undefined || b === undefined) return undefined;
  if (a === null) return b;
  return b === null ? a : Math.max(a, b);
}

/**
 * The bands of the quiet stretches `cut` skips that are in the hours shown, each with its length
 * over it on the hours line and, where a day changed inside it, that day said after the length;
 * and the hours' ticks, each label drawn from the width that clears those words, none that clears
 * them at no width. A band's length gives way to the lengths before it, and a day to every other
 * band's length.
 */
function breaksOf(
  timeline: TaskTimeline,
  scale: Scale,
  ticks: Tick[],
  cut?: SkippedCut | null,
): [Break[], Tick[]] {
  const shown = scale.cuts.skipped.filter((skip) => scale.shows(skip.from, skip.to));
  const lengths: Said[] = [];
  const days: (Said | null)[] = [];
  const breaks = shown.map((skip): Break => {
    const placed = scale.place(skip.from, skip.to);
    const quiet = cut?.skipped.find((stretch) => stretch.from === skip.from) ?? {
      ...skip,
      status: "",
    };
    const text = `+${took(skip.to - skip.from)}`;
    const half = (textPx(text, true) + BREAK_PAD_PX) / 2;
    const centre = placed.left + placed.width / 2;
    const length = centred(centre, 2 * half);
    const lengthFrom = apartFrom(length, lengths);
    lengths.push(length);
    const dayText = dateOf(timeline, skip.to);
    const changed = dateOf(timeline, skip.from) !== dayText && skip.to <= scale.from + scale.shown;
    days.push(
      changed
        ? {
            at: centre,
            before: -half,
            after: half + DAY_AFTER_PX + textPx(dayText, true),
          }
        : null,
    );
    const words = `quiet · ${quietWhy(quiet)}`;
    const wordsFrom =
      placed.width > 0 ? fromWidth((BAND_WORDS_PX * 100) / placed.width) : undefined;
    return {
      ...placed,
      length: lengthFrom === undefined ? null : { text, from: lengthFrom, to: null },
      day: changed ? { text: dayText, from: lengthFrom ?? null, to: null } : null,
      words: wordsFrom === undefined ? null : { text: words, from: wordsFrom, to: null },
      title: `${quietSaid(timeline, quiet, true)}, ${quietWhy(quiet)}`,
    };
  });
  // The days, now every length is known: each after its own length, so only where that is drawn.
  breaks.forEach((band, place) => {
    const day = days[place];
    if (band.day === null || day === null || day === undefined) return;
    const others = lengths.filter((_, other) => other !== place);
    const from = band.length === null ? undefined : later(band.length.from, apartFrom(day, others));
    band.day = from === undefined ? null : { ...band.day, from };
  });
  const words = [...lengths, ...days.filter((day): day is Said => day !== null)];
  const cleared = ticks.flatMap((tick): Tick[] => {
    const from = apartFrom(centred(tick.left, textPx(tick.label, true)), words);
    if (from === undefined) return [];
    return [from === null ? tick : { ...tick, wide: from }];
  });
  return [breaks, cleared];
}

/** The stretches a view skips as bands, each a quiet stretch, with each band's share of the
 * track. */
export interface SkippedCut extends Cut {
  skipped: readonly Quiet[];
}

/** How many parts of the lanes a keyboard stops at: each step's segment, then each gate's state. */
export function stops(lanes: Lanes): number {
  const segs = lanes.lanes.reduce(
    (count, lane) => count + lane.segs.filter((seg) => seg.stop).length,
    0,
  );
  return segs + lanes.gates.reduce((count, gate) => count + gate.spans.length, 0);
}

/** A part of the hours, in seconds: the window a zoomed view shows (timeline-zoom.ts). */
export interface View {
  from: number;
  to: number;
}

/** Where a part is drawn in the hours shown, in percent, clamped to them; its own instants; and
 * whether none of it is in the hours shown (`out`), where it is kept, unseen, for the keyboard. */
export interface Placed {
  left: number;
  width: number;
  from: number;
  to: number;
  out: boolean;
}

/**
 * The hours drawn: from the local hour the task was made in to the timeline's end, now beside
 * them; given a `view`, the part of them it shows (`from`, `to`), which everything is placed in;
 * given a `cut`, the quiet stretches skipped, each a band (timeline-cut.ts), the rest placed on the
 * cut scale. Whether a part is drawn at all (`spans`) is the whole hours', so a zoom never adds or
 * takes away the keyboard's stops.
 */
export class Scale {
  readonly start: number;
  readonly span: number;
  readonly end: number;
  readonly now: number;
  /** The hours shown: the whole of them, or the view's. */
  readonly from: number;
  readonly to: number;
  readonly shown: number;
  readonly zoomed: boolean;
  /** The whole hours cut where quiet stretches are skipped. */
  readonly cuts: Cuts;
  /** The seconds of work a track holds on the cut scale, and where the hours shown start on it. */
  private readonly track: number;
  private readonly origin: number;

  constructor(
    readonly timeline: TaskTimeline,
    view?: View | null,
    cut?: Cut | null,
  ) {
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
    this.zoomed = view !== undefined && view !== null && view.to > view.from;
    this.from = this.zoomed ? (view as View).from : this.start;
    this.to = this.zoomed ? (view as View).to : this.end;
    this.shown = this.zoomed ? this.to - this.from : this.span;
    this.cuts = new Cuts({ from: this.start, to: this.start + this.span }, cut);
    this.track = this.cuts.span({ from: this.from, to: this.from + this.shown });
    this.origin = this.cuts.axis(this.track, this.from);
  }

  at(at: number): number {
    // The plain clock where nothing is skipped, or where the hours shown hold no work.
    const x =
      this.cuts.skipped.length > 0 && this.track > 0
        ? (this.cuts.axis(this.track, at) - this.origin) / this.track
        : (at - this.from) / this.shown;
    return clamp(x * 100, 0, 100);
  }

  width(from: number, to: number): number {
    return this.at(to) - this.at(from);
  }

  /** Whether `from` to `to` takes any of the whole hours: a part drawn at every zoom. */
  spans(from: number, to: number): boolean {
    const at = (time: number) => clamp(((time - this.start) / this.span) * 100, 0, 100);
    return at(to) - at(from) > 0;
  }

  /** Whether any of `from` to `to` is in the hours shown. */
  shows(from: number, to: number): boolean {
    return to >= this.from && from <= this.from + this.shown;
  }

  /** `from` to `to` placed in the hours shown. */
  place(from: number, to: number): Placed {
    return {
      left: this.at(from),
      width: this.width(from, to),
      from,
      to,
      out: !this.shows(from, to),
    };
  }

  clock(at: number): string {
    return clockOf(this.timeline, at);
  }

  /**
   * The hours marked: the step's multiples from local midnight (midnight itself for a step of a
   * day or more), so a change of day is always marked, by its date, a band's said after it
   * (`breaks`); while the task is `open`, none so near now that its label would meet now's, and
   * none in a band. The whole hours are marked from the hour up; a zoom marks minutes too. The
   * step is the first that marks no more than eight in a track's worth of the hours drawn.
   */
  ticks(open: boolean): Tick[] {
    const track = this.track > 0 ? this.track : this.shown;
    const step =
      (this.zoomed ? ZOOMED_TICKS : TICK_HOURS).find((step) => track / step <= 8) ?? 168 * 3600;
    const every = Math.trunc(step / 60);
    const offset = offsetAt(this.timeline, this.from);
    const first = local(this.from, offset);
    // The local minute the hours shown start in, which a zoom's need not start on.
    let tick = first * 60 - offset + remEuclid(-first, Math.min(every, 1440)) * 60;
    if (tick < this.from) tick += step;
    const nowAt = this.now <= this.from + this.shown ? this.at(this.now) : Number.POSITIVE_INFINITY;
    const ticks: Tick[] = [];
    while (tick <= this.to) {
      const left = this.at(tick);
      const skipped = this.cuts.skipped.some((skip) => skip.from <= tick && tick <= skip.to);
      if (!skipped && (!open || nowAt - left >= 4)) {
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

/**
 * A run's distinguishing words, said inside its bar, its lane saying whether it is a build or a
 * test: its command without `just`, its flags or a `--`, the values and the test filter kept
 * ("test-rs symphony-board store" for `just test-rs -p symphony-board --test store`); the command
 * whole where nothing else is left.
 */
export function runWords(run: Run): string {
  const kept = words(run.label).filter((word) => word !== "just" && !word.startsWith("-"));
  return kept.length === 0 ? trimmed(run.label) : kept.join(" ");
}

/** The stage lane: each stretch the task held a status, the wait to land drawn as waiting; a close
 * is an instant, not drawn. */
function stageSegs(timeline: TaskTimeline, scale: Scale, costs?: Costs): Seg[] {
  const segs: Seg[] = [];
  for (const [index, stage] of timeline.stages.entries()) {
    if (closed(stage)) continue;
    const to = stage.to ?? scale.now;
    if (!scale.spans(stage.from, to)) continue;
    const placed = scale.place(stage.from, to);
    const width = placed.width;
    const before = past(segs[segs.length - 1], placed);
    const approved = stage.approved === true;
    const word = approved ? "waiting to land" : stage.status.replaceAll("_", " ");
    const length = took(to - stage.from);
    const cost = stageCost(timeline, costs, index);
    const spent = cost.figure === null ? "" : ` ${cost.figure}`;
    segs.push({
      ...placed,
      tone: statusTone(stage.status),
      look: approved ? "waiting" : "solid",
      // Its word, its time and its cost; else its time and its cost; else its time alone.
      text: fittedFirst([`${word} ${length}${spent}`, `${length}${spent}`, length], width, before),
      ...insetOf(before),
      after: null,
      row: 0,
      title: `${word} ${length}${spent}, from ${scale.clock(stage.from)} to ${scale.clock(to)}${cost.words}`,
      stop: true,
    });
  }
  return segs;
}

/** A place lane: each stretch of the task's places that shares one phase, or one state (`key`). */
function placeSegs(timeline: TaskTimeline, scale: Scale, key: "phase" | "state"): Seg[] {
  let prev: Placed | undefined;
  return stretches(timeline, key).flatMap((stretch): Seg[] => {
    if (!scale.spans(stretch.from, stretch.to)) return [];
    const placed = scale.place(stretch.from, stretch.to);
    const width = placed.width;
    const before = past(prev, placed);
    prev = placed;
    const length = took(stretch.to - stretch.from);
    return [
      {
        ...placed,
        tone: statusTone(stretch.status),
        look: "solid",
        text: fitted(`${stretch.label} ${length}`, length, width, before),
        ...insetOf(before),
        after: null,
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
  const places = timeline.claims.map((claim) => scale.place(claim.from, claim.to ?? scale.now));
  return timeline.claims.map((claim: Claim, index) => {
    const to = claim.to ?? scale.now;
    const placed = places[index] as Placed;
    const width = placed.width;
    const before = past(places[index - 1], placed);
    const length = took(to - claim.from);
    const word = one ? (index === 0 ? "claimed" : "") : claim.by;
    const long = word === "" ? length : `${word} ${length}`;
    return {
      ...placed,
      tone: "primary",
      look: "solid",
      text: fitted(long, length, width, before),
      ...insetOf(before),
      after: null,
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
    const waiting = scale.width(round.from, taken ?? to);
    const width = taken === null ? 0 : scale.width(taken, to);
    // A verdict's room runs to the next round, or to the band of a stretch skipped before it, which
    // would hide it; past the last round and no band, into the margin after the track.
    const band = scale.cuts.skipped.find((skip) => skip.from >= to);
    const ends = [...(next === undefined ? [] : [next.from]), ...(band ? [band.from] : [])];
    const room: Room = {
      room: (ends.length === 0 ? 100 : scale.at(Math.min(...ends))) - scale.at(to),
      last: ends.length === 0,
      bar: taken === null ? waiting : width,
    };
    const verdict = round.verdict ?? null;
    // Said after its round's end, and only where that end is in the hours shown.
    const word =
      verdict === null || !scale.shows(to, to) ? null : label(verdictWord(verdict), room);
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
    if (scale.spans(round.from, taken ?? to) || taken === null) {
      const placed = scale.place(round.from, taken ?? to);
      const before = past(segs[segs.length - 1], placed);
      segs.push({
        ...placed,
        tone: "question",
        look: "waiting",
        text: taken === null ? fitted(name, "", waiting, before) : [],
        ...insetOf(taken === null ? before : undefined),
        after: taken === null ? after : null,
        row: 0,
        title,
        stop: taken === null,
      });
    }
    if (taken !== null) {
      const placed = scale.place(taken, to);
      const before = past(segs[segs.length - 1], placed);
      segs.push({
        ...placed,
        tone: "question",
        look: "solid",
        text: fitted(name, "", width, before),
        ...insetOf(before),
        after,
        row: 0,
        title,
        stop: true,
      });
    }
  });
  return segs;
}

/**
 * The rows of a lane of runs: each run in the first row free when it is queued (the run before it
 * there ended by then), else a row of its own, so only runs that overlap take a row each, as many
 * as they need, and runs one after another share one. A run holds its row from its queue to its
 * end, one going on to `now`. Each run's row, and how many rows there are.
 */
export function runRows(runs: Run[], now: number): [number[], number] {
  const ends: number[] = [];
  const placed = runs.map((run) => {
    let row = ends.findIndex((free) => free <= run.queued);
    if (row < 0) {
      ends.push(Number.NEGATIVE_INFINITY);
      row = ends.length - 1;
    }
    ends[row] = Math.max(ends[row] as number, run.ended ?? now);
    return row;
  });
  return [placed, Math.max(ends.length, 1)];
}

/** A lane of `runs`, the builds' or the tests': each run one bar in its row, its queue drawn as
 * waiting at its head, then its run in its result's tone, striped while it runs; its words inside
 * the part the keyboard stops at, where that part is wide enough. Its segments, and how many rows
 * they take. */
function runSegs(runs: Run[], scale: Scale): [Seg[], number] {
  const now = scale.now;
  const [placed, count] = runRows(runs, now);
  const segs: Seg[] = [];
  runs.forEach((run, index) => {
    const row = placed[index] as number;
    const began = run.started ?? null;
    const started = began ?? run.ended ?? now;
    const length = lasted(began === null ? waited(run, now) : ran(run, now));
    const said = `${runWords(run)} · ${length}`;
    const title = runTitle(run, now);
    const queued = scale.place(run.queued, started);
    const queue = queued.width;
    segs.push({
      ...queued,
      tone: "primary",
      look: "waiting",
      text: began === null ? barFit(said, queue) : [],
      after: null,
      row,
      title,
      stop: began === null,
    });
    if (began !== null) {
      const placed = scale.place(began, run.ended ?? now);
      const width = placed.width;
      segs.push({
        ...placed,
        tone: resultTone(run.result),
        look: run.result === "running" ? "running" : "solid",
        text: barFit(said, width),
        after: null,
        row,
        title,
        stop: true,
      });
    }
  });
  return [segs, count];
}

/** A runs lane's counts, which its name says at any zoom: "42 runs · 12 red". */
export function runCounts(runs: Run[]): string {
  const red = runs.filter((run) => run.result === "red").length;
  return `${runs.length} ${plural(runs.length, "run")} · ${red} red`;
}

/** The time no run is kept of: from the task's making to `runs_from`, where the recorder's history
 * begins after it; none where it keeps them all, where the task has no runs lane, or where none of
 * it is in the hours shown. */
function notKept(timeline: TaskTimeline, scale: Scale): NotKept | null {
  const from = timeline.runs_from ?? null;
  const title = keptTitle(timeline);
  const said = keptFrom(timeline);
  if (from === null || title === null || said === null || timeline.runs.length === 0) return null;
  const width = scale.width(timeline.created, from);
  if (!(width > 0)) return null;
  const text = `not kept before ${said}`;
  const at = widthFor(text, width, NOT_KEPT_PAD_PX);
  return {
    left: scale.at(timeline.created),
    width,
    label: at === undefined ? null : { text, from: at, to: null },
    title,
  };
}

/** Each gate's thin row: each state from when it was taken to the next, the last to the end, its
 * word where it fits. */
function gateRows(timeline: TaskTimeline, scale: Scale): GateRow[] {
  return timeline.gates.map((lane: GateLane) => ({
    name: lane.name,
    spans: lane.states.flatMap((span, index) => {
      const to = lane.states[index + 1]?.from ?? scale.end;
      if (!scale.spans(span.from, to)) return [];
      const placed = scale.place(span.from, to);
      const from = widthFor(span.state, placed.width, PAD_PX);
      const detail = span.detail === "" ? "" : `: ${span.detail}`;
      return [
        {
          ...placed,
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
 * gate; over the whole hours, or the part of them `view` shows, the quiet stretches `cut` skips a
 * band each. */
export function lanes(
  timeline: TaskTimeline,
  costs?: Costs,
  view?: View | null,
  cut?: SkippedCut | null,
): Lanes {
  const scale = new Scale(timeline, view, cut);
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
      rail: false,
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
        rail: false,
        segs: placeSegs(timeline, scale, key),
      });
    }
  }
  if (timeline.history) {
    const roles = timeline.history.roles.filter((role) => role.role === "orchestrator");
    // Uncertain boundaries are instants, never a corrected or invented duration.
    const places = roles.map((role) =>
      scale.place(
        role.from,
        role.uncertain ? role.from : (role.to ?? timeline.closed ?? timeline.now),
      ),
    );
    const segs: Seg[] = roles.map((role, index) => {
      const placed = places[index] as Placed;
      const width = placed.width;
      const before = past(places[index - 1], placed);
      return {
        ...placed,
        tone: "resolved",
        look: "waiting",
        text: fitted(role.by, "", width, before),
        ...insetOf(before),
        after: null,
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
      rail: false,
      segs,
    });
  }
  if (timeline.claims.length > 0) {
    drawn.push({
      key: "executor",
      name: "Executor",
      who: executors,
      rows: 1,
      rail: false,
      segs: claimSegs(timeline, scale, one),
    });
  }
  if (timeline.rounds.length > 0) {
    drawn.push({
      key: "reviewer",
      name: "Reviewer",
      who: reviewers,
      rows: 1,
      rail: false,
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
          rail: false,
          segs: [],
        });
      }
    }
    drawn.push({ key: "other", name: "Board & others", who: "", rows: 1, rail: false, segs: [] });
    // Every lane a message can start or end at carries the line its ticks and arrows are drawn
    // on, whether or not one starts there.
    for (const lane of drawn) lane.rail = RAILS.includes(lane.key);
  }
  const builds = timeline.runs.filter((run) => run.phase === "build");
  const tests = timeline.runs.filter((run) => run.phase !== "build");
  for (const [key, name, runs] of [
    ["builds", "Builds", builds],
    ["tests", "Tests", tests],
  ] as const) {
    if (runs.length === 0) continue;
    const [segs, count] = runSegs(runs, scale);
    drawn.push({ key, name, who: runCounts(runs), rows: count, rail: false, segs });
  }
  const [breaks, ticks] = breaksOf(timeline, scale, scale.ticks(open), cut);
  return {
    ticks,
    now:
      open && scale.shows(scale.now, scale.now)
        ? { left: scale.at(scale.now), label: `now ${scale.clock(scale.now)}` }
        : null,
    lanes: drawn,
    gates: gateRows(timeline, scale),
    notKept: notKept(timeline, scale),
    breaks,
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
