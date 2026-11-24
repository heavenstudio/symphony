// A task's gates as its page draws them (`GET /api/task-gates`, docs/workflow/TASKS.md#gates): every stamp
// or check the task needs to land, each met, missing, waived, held or unknown, with who, when and
// why. This is what the section knows without a page: the room it holds before its answer
// (`Room`, kept per task in this browser), what it says of an answer, the words it links, the times
// it tells, and the guards of its reads. The section itself is gates.tsx, which a page imports as
// `./gates` (a `gates.ts` beside it would be what that names).
//
// `seenOf`, `countOf` and `waitingGates` carry crates/symphony-api's own (gates.rs: `TaskGates::seen`,
// `count`, `waiting`), and `unsaidCommit`, `signed` and `waits` crates/symphony-blocks' (gates.rs:
// `Gate::unsaid_commit`, `Gate::signed`, `GateState::waits`); the rest carries the Dioxus client's
// (its `task_gates.rs`, gone with its task page), whose unit tests gates-model.test.ts and
// gates.test.tsx port, and which the browser tests hold.

import { writer } from "./project";
import { field } from "./shared";
import { NOT_IN_TIME } from "./trouble";
import type { Gate, GateState, GatesSeen, MachineWork, Task, TaskGates, WorkState } from "./wire";

/** The words the section says. */
export const WORDS = {
  title: "Gates",
  reading: "Reading the gates…",
  slow: "The gates took too long to answer and were given up on. The rest of the page still works.",
  retry: "Retry",
  /** The retry while a read is out after the last failed. */
  retrying: "Retrying…",
  waitsOn: "waits on ",
  /** Before the last name the pill says, and before each other but the first. */
  and: " and ",
  comma: ", ",
  /** Between the parts of a gate's who and when. */
  dot: " · ",
  /** Before a gate's why, after its detail. */
  because: ": ",
  /** What a read the server answered with something this client cannot read fails with. */
  unreadable: "The server's gates could not be read",
} as const;

/** What the section has of the task's gates. */
export type Gates =
  /** Not answered yet. */
  | { kind: "reading" }
  /** The answer; no gates for a task nothing gates, or one the board does not have. */
  | { kind: "read"; answer: TaskGates }
  /** The server did not answer in time. */
  | { kind: "slow" }
  /** Why the read failed. */
  | { kind: "failed"; why: string };

export const READING: Gates = { kind: "reading" };

/** The answer for a task nothing gates. */
export function noGates(): TaskGates {
  return { slug: "", branch: "", gates: [] };
}

/** What a read brought: the answer, `null` for the server's 404 (a task it has no gates for), or why it failed. */
export type GatesAnswer = { found: TaskGates | null } | { failed: string };

/** The section's state from a read. */
export function gatesOf(answer: GatesAnswer): Gates {
  if ("found" in answer) return { kind: "read", answer: answer.found ?? noGates() };
  return answer.failed === NOT_IN_TIME ? { kind: "slow" } : { kind: "failed", why: answer.failed };
}

/**
 * The room the section holds before its gates are read: what the last answer was (how many gates,
 * how many the task waited on, which a phone puts on a line of its own, and how many had no who
 * and when, a phone's third line), and the height this browser drew it at for a window this wide,
 * `[width, height]` in px.
 */
export interface Room {
  seen: GatesSeen;
  drawn: [number, number] | null;
}

/** The room for a task nothing says anything of yet: four gates, one waited on. */
export const DEFAULT_ROOM: Room = { seen: { count: 4, waiting: 1, unsigned: 0 }, drawn: null };

const sameSeen = (a: GatesSeen, b: GatesSeen) =>
  a.count === b.count && a.waiting === b.waiting && a.unsigned === b.unsigned;

export const Room = {
  /**
   * The room from what the server last answered for the task (`seen`, with the page) and what this
   * browser kept (`kept`): the server's, with the browser's height while the two agree; else the
   * browser's; else DEFAULT_ROOM.
   */
  of(seen: GatesSeen | null, kept: Room | null): Room {
    if (seen !== null && kept !== null && sameSeen(kept.seen, seen)) return kept;
    if (seen !== null) return { seen, drawn: null };
    return kept ?? DEFAULT_ROOM;
  },

  /** The height to hold in a window `width` px wide: the one drawn at that width, if any. */
  held(room: Room, width: number | null): number | null {
    return room.drawn !== null && room.drawn[0] === width ? room.drawn[1] : null;
  },
};

/**
 * Where this browser keeps the room each task's gates took, newest first, as lines of
 * `COUNT WAITING UNSIGNED WIDTH HEIGHT SLUG` (a width of 0 drew nothing).
 */
export const KEPT = "gates-room";
/** How many tasks' rooms a browser keeps. */
export const KEPT_MAX = 64;

/**
 * Rust's `str::lines`: `text` cut at each `\n`, a `\r` before it dropped with it, and no line after
 * a last `\n`.
 */
function lines(text: string): string[] {
  return (text.match(/[^\n]*\n|[^\n]+$/g) ?? []).map((line) =>
    line.endsWith("\n") ? line.slice(0, -1).replace(/\r$/, "") : line,
  );
}

/** Rust's `str::splitn(n, ' ')`: at most `n` parts, the last holding the rest, its spaces too. */
function splitn(text: string, n: number): string[] {
  const parts: string[] = [];
  let rest = text;
  for (let space = rest.indexOf(" "); parts.length < n - 1 && space >= 0; ) {
    parts.push(rest.slice(0, space));
    rest = rest.slice(space + 1);
    space = rest.indexOf(" ");
  }
  parts.push(rest);
  return parts;
}

/** The slug a kept line names: what follows its fifth space; none for a line with fewer. */
const slugOf = (line: string) => splitn(line, 6)[5];

/** A number as Rust's `u32` reads one: digits, a `+` before them allowed, within its range. */
function u32(text: string): number | null {
  if (!/^\+?[0-9]+$/.test(text)) return null;
  const number = Number(text);
  return number <= 0xffff_ffff ? number : null;
}

/** The room `kept` (as KEPT holds it) gives the task `slug`. */
export function recall(kept: string, slug: string): Room | null {
  for (const line of lines(kept)) {
    const parts = splitn(line, 6);
    const numbers = parts
      .slice(0, 5)
      .map(u32)
      .filter((number) => number !== null);
    if (numbers.length < 5 || parts[5] !== slug) continue;
    const [count = 0, waiting = 0, unsigned = 0, width = 0, height = 0] = numbers;
    return { seen: { count, waiting, unsigned }, drawn: width > 0 ? [width, height] : null };
  }
  return null;
}

/**
 * The room `kept` gives the last task drawn in a window `width` px wide: a project's tasks mostly
 * wait on the same gates, so a task never opened at that width takes it; none when no task was
 * drawn as wide.
 */
export function latest(kept: string, width: number | null): Room | null {
  for (const line of lines(kept)) {
    const slug = slugOf(line);
    const room = slug === undefined ? null : recall(line, slug);
    if (room?.drawn && room.drawn[0] === width) return room;
  }
  return null;
}

/** `kept` with `room` as the task `slug`'s, first, its line before and the oldest past KEPT_MAX dropped. */
export function keep(kept: string, slug: string, room: Room): string {
  const { count, waiting, unsigned } = room.seen;
  const [width, height] = room.drawn ?? [0, 0];
  const line = `${count} ${waiting} ${unsigned} ${width} ${height} ${slug}`;
  return [line, ...lines(kept).filter((old) => slugOf(old) !== slug)].slice(0, KEPT_MAX).join("\n");
}

const utf8 = new TextEncoder();
const U64 = (1n << 64n) - 1n;

/** The 64-bit FNV-1a hash of `text`'s UTF-8 bytes. */
export function fnv1a(text: string): bigint {
  let hash = 0xcbf2_9ce4_8422_2325n;
  for (const byte of utf8.encode(text)) hash = ((hash ^ BigInt(byte)) * 0x0100_0000_01b3n) & U64;
  return hash;
}

/** A run's state as Rust's `{:?}` writes it. */
const DEBUG_STATE: Record<WorkState, string> = { queued: "Queued", running: "Running" };

/**
 * What of `work` the task `slug` runs, as one number: it changes when one of the task's builds or
 * tests starts, moves on or ends (its check may change with it), and not with each tick of the
 * machines' observation nor with the order the machines list them in. Each run is hashed (FNV-1a)
 * and the hashes summed as Rust's `u64` sums them, so no list is kept or sorted; none is 0. The
 * sum's low 53 bits are the number, which a JavaScript number holds exactly.
 */
export function workOf(work: MachineWork, slug: string): number {
  let sum = 0n;
  for (const operation of [...work.machines.flatMap((one) => one.operations), ...work.unassigned]) {
    if (operation.task !== slug) continue;
    const state = DEBUG_STATE[operation.state] ?? operation.state;
    const run = `${operation.operation_id ?? ""} ${operation.checkout} ${operation.recipe} ${state}`;
    sum = (sum + fnv1a(run)) & U64;
  }
  return Number(BigInt.asUintN(53, sum));
}

/** As serde reads an `Option`: absent and `null` are none. */
const present = <T>(value: T | null | undefined): value is T =>
  value !== undefined && value !== null;

/** Whether the task waits on a gate in `state`: missing or held. */
export function waits(state: GateState): boolean {
  return state === "missing" || state === "held";
}

/** The gates in `state`. */
export function countOf(answer: TaskGates, state: GateState): number {
  return answer.gates.filter((gate) => gate.state === state).length;
}

/** The gates the task waits on: missing or held. */
export function waitingGates(answer: TaskGates): Gate[] {
  return answer.gates.filter((gate) => waits(gate.state));
}

/**
 * The gate's commit, its first seven characters, when its words (its detail and why) do not
 * already say it: what its who and when add.
 */
export function unsaidCommit(gate: Gate): string | null {
  if (!present(gate.commit) || gate.commit === "") return null;
  const short = [...gate.commit].slice(0, 7).join("");
  const said = (text: string) => text.includes(short);
  return !said(gate.detail) && !(present(gate.why) && said(gate.why)) ? short : null;
}

/**
 * Whether a page has a who and when to draw for the gate: who, when, or a commit its words do not
 * say. The Symphony's review gate before any verdict has none.
 */
export function signed(gate: Gate): boolean {
  return (present(gate.by) && gate.by !== "") || present(gate.at) || unsaidCommit(gate) !== null;
}

/** How many gates there are, how many the task waits on and how many say no one and no time. */
export function seenOf(answer: TaskGates): GatesSeen {
  return {
    count: answer.gates.length,
    waiting: waitingGates(answer).length,
    unsigned: answer.gates.filter((gate) => !signed(gate)).length,
  };
}

/** What of a task says what it waits on: its status, who holds it and the tasks it depends on. */
export type Waiter = Pick<Task, "status" | "roles" | "depends">;

/** What the pill says the task waits on: the class its colour takes, and the names it says. */
export interface Waits {
  tone: string;
  names: string[];
}

/** The statuses a task's executor works it in. */
const WORKED = ["concept", "design", "in_progress"];

/** The healths of a holder that is gone: its role stands, but nobody works it. */
const GONE = ["lost", "left"];

/**
 * What `task` waits on, by its status, as the pill says it (`waits on certificate`, `waits on
 * executor quiet-wren and owner`, `waits on a, b and c`); none when it waits on nothing.
 *
 * In review, the gates of `answer` missing or held, in their order, coloured as the first. Else
 * who its status waits on, coloured as that status: in concept, design or progress, its executor,
 * said `lost` or `left` when it is gone and coloured as missing when every one is, since nobody
 * works it then; blocked, every task it depends on (the page has no word of theirs); not started
 * (backlog, ready), none. Then each gate held, a card waiting on the owner, coloured as held when
 * it is all. A done or cancelled task waits on nothing, whatever its gates say.
 */
export function waitsOn(answer: TaskGates, task: Waiter): Waits | null {
  const { status } = task;
  if (status === "done" || status === "cancelled") return null;
  if (status === "review") {
    const waiting = waitingGates(answer);
    const first = waiting[0];
    return first === undefined
      ? null
      : { tone: `gate-${first.state}`, names: waiting.map((gate) => gate.name) };
  }
  const executors = WORKED.includes(status)
    ? task.roles.filter((role) => role.role === "executor")
    : [];
  const own =
    status === "blocked"
      ? task.depends
      : executors.map(({ handle, health }) =>
          GONE.includes(health) ? `executor ${handle} ${health}` : `executor ${handle}`,
        );
  const held = answer.gates.filter((gate) => gate.state === "held").map((gate) => gate.name);
  if (own.length + held.length === 0) return null;
  const gone = executors.length > 0 && executors.every(({ health }) => GONE.includes(health));
  const tone = gone ? "gate-missing" : own.length > 0 ? `st-${status}` : "gate-held";
  return { tone, names: [...own, ...held] };
}

/** The states the counts say, what no longer stands in the way first. */
const COUNTED: GateState[] = ["met", "waived", "unknown", "held", "missing"];

/**
 * How many gates are in each state, what no longer stands in the way first: `2 met · 1 waived ·
 * 1 missing`. States with none are left out.
 */
export function counts(answer: TaskGates): string {
  return COUNTED.map((state): [GateState, number] => [state, countOf(answer, state)])
    .filter(([, count]) => count > 0)
    .map(([state, count]) => `${count} ${state}`)
    .join(WORDS.dot);
}

/**
 * Who, when, and the commit when the gate's words do not already name it, each a part of its own:
 * `[lucid-heron, 14:05]`, `[studio, 11-10 02:14, 4be91c2]`; none for a gate that is not `signed`.
 * `today` is now in `localMinutes`.
 */
export function byline(gate: Gate, today: number): string[] {
  const parts: string[] = [];
  if (present(gate.by) && gate.by !== "") parts.push(writer(gate.by).text);
  if (present(gate.at)) parts.push(when(localMinutes(gate.at), today));
  const commit = unsaidCommit(gate);
  if (commit !== null) parts.push(commit);
  return parts;
}

/** A part of a text: where it starts and where it ends, in the text's UTF-16 code units. */
export type Range = [number, number];

/** The length of `text` in UTF-8 bytes, which is what Rust's `str::len` counts. */
function bytes(text: string): number {
  return utf8.encode(text).length;
}

/** Rust's `char::is_alphanumeric`: a letter (Alphabetic) or a number (Nd, Nl, No). */
const ALPHANUMERIC = /^[\p{Alphabetic}\p{N}]$/u;

/** The character `text` has just before `at`, a pair of surrogates whole; none at its start. */
function before(text: string, at: number): string | undefined {
  return [...text.slice(0, at)].pop();
}

/** The character `text` has at `place`, a pair of surrogates whole; none at its end. */
function after(text: string, place: number): string | undefined {
  const code = text.codePointAt(place);
  return code === undefined ? undefined : String.fromCodePoint(code);
}

/** Whether `c` ends a word: none, or a character that is neither alphanumeric nor `-`, `_`, `/`. */
const edge = (c: string | undefined) =>
  c === undefined || !(ALPHANUMERIC.test(c) || "-_/".includes(c));

/** `text` with its ASCII capitals made small: every character stays where it was. */
const asciiLower = (text: string) => text.replace(/[A-Z]/g, (capital) => capital.toLowerCase());

/** Where `detail` says `words` as a word of its own, running on over what `more` takes. */
function word(detail: string, words: string, more: (c: string) => boolean): Range | null {
  const step = Math.max(words.length, 1);
  for (let start = detail.indexOf(words); start >= 0; start = detail.indexOf(words, start + step)) {
    let end = start + words.length;
    for (let next = after(detail, end); next !== undefined && more(next); ) {
      end += next.length;
      next = after(detail, end);
    }
    if (edge(before(detail, start)) && edge(after(detail, end))) return [start, end];
  }
  return null;
}

/** How many ASCII digits `text` has from `start` on. */
function digits(text: string, start: number): number {
  let end = start;
  while (/[0-9]/.test(text[end] ?? "")) end += 1;
  return end - start;
}

/** Where `detail` names a review round: `round 2` or `round 2 of 2`, in any case. */
function round(detail: string): Range | null {
  const said = "round ";
  // ASCII case leaves every character where it was, so a place in one is a place in the other.
  const lower = asciiLower(detail);
  for (
    let start = lower.indexOf(said);
    start >= 0;
    start = lower.indexOf(said, start + said.length)
  ) {
    const number = digits(detail, start + said.length);
    const letter = before(detail, start);
    if (number === 0 || (letter !== undefined && ALPHANUMERIC.test(letter))) continue;
    let end = start + said.length + number;
    if (detail.startsWith(" of ", end)) {
      const total = digits(detail, end + " of ".length);
      if (total > 0) end += " of ".length + total;
    }
    return [start, end];
  }
  return null;
}

/** A name has a digit or a joiner in it; a plain word (`task`, `docs`, `runs`) is the route's. */
function named(run: string): boolean {
  const characters = [...run];
  return characters.length >= 4 && characters.some((c) => /[0-9]/.test(c) || "-_./".includes(c));
}

/** `link` up to its query or fragment. */
const bare = (link: string) => link.replace(/[?#][\s\S]*$/, "");

/** The path of `link`: itself for a path of the app, else what follows a web address's host. */
function pathOf(link: string): string {
  if (link.startsWith("/")) return bare(link);
  const scheme = link.indexOf("://");
  const rest = scheme < 0 ? "" : link.slice(scheme + "://".length);
  const slash = rest.indexOf("/");
  return bare(slash < 0 ? "" : rest.slice(slash));
}

/** The longest run of `path`'s own segments that `detail` says as a name, the first of the longest. */
function longestRun(detail: string, path: string): Range | null {
  // Each segment's place in the path; a run of them is the path from one's start to another's end,
  // so none is built or kept.
  const segments: Range[] = [];
  let place = 0;
  for (const segment of path.split("/")) {
    if (segment !== "") segments.push([place, place + segment.length]);
    place += segment.length + 1;
  }
  let longest: [number, Range] | null = null;
  for (let first = 0; first < segments.length; first += 1) {
    const [start] = segments[first] as Range;
    for (const [, end] of segments.slice(first)) {
      const run = path.slice(start, end);
      const size = bytes(run);
      if ((longest !== null && longest[0] >= size) || !named(run)) continue;
      const found = word(detail, run, () => false);
      if (found !== null) longest = [size, found];
    }
  }
  return longest === null ? null : longest[1];
}

/** Where `detail` says the gate's `commit`, its first seven characters and the hex digits after. */
function commitSaid(detail: string, commit: string | null | undefined): Range | null {
  if (!present(commit)) return null;
  const short = [...commit].slice(0, 7).join("");
  return bytes(short) >= 4 ? word(detail, short, (c) => /^[0-9a-fA-F]$/.test(c)) : null;
}

/**
 * The words of `detail` a link to `link` stands on: the link itself where the detail writes it out
 * (with or without its query), else the longest run of the link's own path the detail says, a name
 * rather than a route's word (`tools/slot-pool-by-tree` for `/task/tools/slot-pool-by-tree`, never
 * `task`), else the gate's `commit` as the detail says it, else the review round it names
 * (`round 2 of 2`), else all of it. Of the path's runs the longest the detail says wins, the first
 * of them where two are as long.
 */
export function evidence(detail: string, link: string, commit?: string | null): Range {
  for (const written of [link, bare(link)]) {
    const found = written === "" ? null : word(detail, written, () => false);
    if (found !== null) return found;
  }
  return (
    longestRun(detail, pathOf(link)) ??
    commitSaid(detail, commit) ??
    round(detail) ?? [0, detail.length]
  );
}

/** Words of a text in order, each `[code, text]`: code for what a pair of backticks encloses. */
export type Words = [boolean, string][];

/**
 * The ranges of `text` cut at its backticks: `[true, code]` for what a pair of them encloses,
 * `[false, words]` for the rest; a backtick with no partner is kept as written.
 */
function spans(text: string): [boolean, Range][] {
  const found: [boolean, Range][] = [];
  let start = 0;
  for (let open = text.indexOf("`"); open >= 0; open = text.indexOf("`", start)) {
    const close = text.indexOf("`", open + 1);
    if (close < 0) break;
    if (open > start) found.push([false, [start, open]]);
    found.push([true, [open + 1, close]]);
    start = close + 1;
  }
  if (start < text.length) found.push([false, [start, text.length]]);
  return found;
}

/**
 * `text` cut at its backticks as `codeSpans` does, in three: before `linked`, inside it and after
 * it (all before it when there is none).
 */
export function pieces(text: string, linked: Range | null): [Words, Words, Words] {
  const [first, last] = linked ?? [text.length, text.length];
  const parts: [Words, Words, Words] = [[], [], []];
  for (const [code, [start, end]] of spans(text)) {
    const cuts: Range[] = [
      [0, Math.min(first, end)],
      [Math.max(first, start), Math.min(last, end)],
      [Math.max(last, start), end],
    ];
    cuts.forEach(([from, to], part) => {
      const begin = Math.max(from, start);
      if (begin < to) parts[part]?.push([code, text.slice(begin, to)]);
    });
  }
  return parts;
}

/**
 * `text` cut at its backticks: `[true, code]` for what a pair of them encloses, `[false, words]`
 * for the rest. A backtick with no partner is kept as written.
 */
export function codeSpans(text: string): Words {
  return spans(text).map(([code, [start, end]]) => [code, text.slice(start, end)]);
}

/**
 * The minute `seconds` since the epoch falls in on the reader's clock, counted from 1970-01-01
 * 00:00 there: the browser's time zone at that moment.
 */
export function localMinutes(seconds: number): number {
  const utc = Math.floor(seconds / 60);
  // Rust's `as i64` of the offset: its whole minutes, and none for a date the browser cannot tell.
  const offset = new Date(seconds * 1000).getTimezoneOffset();
  return utc - (Number.isFinite(offset) ? Math.trunc(offset) : 0);
}

/** `n` written in at least `width` digits, as Rust's `{:0width$}` writes it, a minus counting. */
function padded(n: number, width: number): string {
  return n < 0 ? `-${String(-n).padStart(width - 1, "0")}` : String(n).padStart(width, "0");
}

/**
 * A time as short as `now` lets it be told, both in `localMinutes`: `14:05` on the same day,
 * `11-10 02:14` in the same year, `2025-11-10` before.
 */
export function when(at: number, now: number): string {
  const day = Math.floor(at / 1440);
  const minute = at - day * 1440;
  const clock = `${padded(Math.floor(minute / 60), 2)}:${padded(minute % 60, 2)}`;
  if (day === Math.floor(now / 1440)) return clock;
  const [year, month, date] = civil(day);
  return year === civil(Math.floor(now / 1440))[0]
    ? `${padded(month, 2)}-${padded(date, 2)} ${clock}`
    : `${padded(year, 4)}-${padded(month, 2)}-${padded(date, 2)}`;
}

/**
 * The date of the proleptic Gregorian calendar `days` after 1970-01-01 (Howard Hinnant's
 * `civil_from_days`): its year, month and day.
 */
export function civil(days: number): [number, number, number] {
  const shifted = days + 719_468;
  const era = Math.floor(shifted / 146_097);
  const dayOfEra = shifted - era * 146_097;
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
  const date = dayOfYear - Math.floor((153 * monthFromMarch + 2) / 5) + 1;
  const month = monthFromMarch < 10 ? monthFromMarch + 3 : monthFromMarch - 9;
  const year = yearOfEra + era * 400 + (month <= 2 ? 1 : 0);
  return [year, month, date];
}

/** Rust's `str::trim`: `text` without the White_Space characters at either end. */
export const trimmed = (text: string) => text.replace(/^\p{White_Space}+|\p{White_Space}+$/gu, "");

/** What a read that failed says, from the reason the transport gave. */
export function failed(why: string): string {
  const reason = trimmed(why).replace(/\.+$/, "");
  return reason === ""
    ? "The gates could not be read. The rest of the page still works."
    : `The gates could not be read: ${reason}. The rest of the page still works.`;
}

const STATES: GateState[] = ["missing", "held", "waived", "met", "unknown"];

/** A gate's state as serde reads it: a word this client does not know yet is unknown. */
export function readState(value: unknown): GateState | null {
  if (typeof value !== "string") return null;
  return STATES.includes(value as GateState) ? (value as GateState) : "unknown";
}

const isOptionalText = (value: unknown) => !present(value) || typeof value === "string";

/**
 * A gate as the section reads it, or none for one serde would refuse in a field the section
 * draws: its name and state, its who, when, commit, why and link (each absent, `null` or of its
 * type) and its detail (absent is empty, as serde's default). Its past, the timeline's, is the
 * server's to send.
 */
export function readGate(value: unknown): Gate | null {
  const state = readState(field(value, "state"));
  // Serde's default stands only for a field left out: a `null` detail is no string.
  const said = field(value, "detail");
  const detail = said === undefined ? "" : said;
  const at = field(value, "at");
  return typeof field(value, "name") === "string" &&
    state !== null &&
    typeof detail === "string" &&
    (!present(at) || typeof at === "number") &&
    ["by", "commit", "why", "link"].every((key) => isOptionalText(field(value, key)))
    ? { ...(value as Gate), state, detail }
    : null;
}

/**
 * The answer of `/api/task-gates` as the section reads it, or none: its branch, which the page
 * matches moved branches against, and its gates, each as `readGate` reads it (none listed is none,
 * as serde's default). Its slug and tip, which the section does not draw, are the server's.
 */
export function readTaskGates(value: unknown): TaskGates | null {
  const listed = field(value, "gates");
  const gates = listed === undefined ? [] : listed;
  if (typeof field(value, "branch") !== "string" || !Array.isArray(gates)) return null;
  const read = gates.map(readGate);
  return read.every((gate) => gate !== null) ? { ...(value as TaskGates), gates: read } : null;
}

/** A count as serde reads a `usize`: a whole number, not below zero. */
const isCount = (value: unknown): value is number =>
  typeof value === "number" && Number.isSafeInteger(value) && value >= 0;

/**
 * What the server says of a task's last answer (`TaskPage.gates`), or none for what serde would
 * refuse: its count, and how many it waited on and had no who and when (none said is 0, as
 * serde's default).
 */
export function readGatesSeen(value: unknown): GatesSeen | null {
  const [count, waiting = 0, unsigned = 0] = ["count", "waiting", "unsigned"].map((key) =>
    field(value, key),
  );
  return isCount(count) && isCount(waiting) && isCount(unsigned)
    ? { count, waiting, unsigned }
    : null;
}
