import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import FIXTURE from "../../../crates/symphony-api/fixtures/task-timeline.json";
import { carried } from "./timeline-cost";
import {
  BAR_SPAN,
  BAR_WORDS_PX,
  barFit,
  CHAR_PX,
  type Costs,
  capitalised,
  civil,
  clock,
  clockOf,
  date,
  dateOf,
  everySeconds,
  type Fit,
  failed,
  fitClass,
  fitted,
  fittedFirst,
  GAP_PX,
  gateChanges,
  gateTone,
  keptRow,
  keptTitle,
  type Lane,
  LONGEST_WAIT,
  label,
  lanes,
  lasted,
  legendStates,
  lookClass,
  machineSeconds,
  nextRead,
  offsetAt,
  PAD_PX,
  placeRow,
  placeTotals,
  queuedSeconds,
  type Row,
  ran,
  readTimeline,
  reason,
  resultTone,
  resultWord,
  roomCss,
  rows,
  runCounts,
  runLabel,
  runRow,
  runRows,
  runTitle,
  runWords,
  Scale,
  shortCommit,
  showAll,
  stageSeconds,
  statusTone,
  stops,
  stretches,
  timelineEnd,
  took,
  totalParts,
  totals,
  trimmed,
  verdictTone,
  verdictWord,
  WIDTHS,
  waited,
  widthFor,
} from "./timeline-layout";
import type { PlaceSpan, Run, RunResult, Stage, TaskTimeline, TokenRow } from "./wire";
import { TIMELINE_EVERY } from "./wire";
import { moment } from "./words";

/**
 * The text of the style sheet `name`, beside this file. Vitest empties every CSS import, `?raw`
 * among them, while its `css` option processes none, so the sheet is read through Node's own `fs`.
 */
function sheet(name: string): string {
  return readFileSync(new URL(name, import.meta.url), "utf8");
}

const CSS = sheet("./timeline.css");

/** The approved pictures' task: two review rounds, a build and its tests, one running now, a gate
 * met then missing again. */
function pictures(): TaskTimeline {
  const read = readTimeline(JSON.parse(JSON.stringify(FIXTURE)));
  if (read === null) throw new Error("the fixture is no timeline");
  return read;
}

/** `minutes` after the pictures' task was made (09:02 on their clock). */
const at = (minutes: number) => pictures().created + minutes * 60;

/** A timeline with nothing in it but what `fields` say, as Rust's `TaskTimeline::default()`. */
function aTimeline(fields: Partial<TaskTimeline> = {}): TaskTimeline {
  return {
    slug: "",
    now: 0,
    zone: [],
    created: 0,
    bounces: 0,
    stages: [],
    claims: [],
    rounds: [],
    runs: [],
    gates: [],
    ...fields,
  };
}

/** A stage as Rust's `Stage::default()` with `fields`. */
const aStage = (fields: Partial<Stage>): Stage => ({ status: "", from: 0, ...fields });

/** The words of `fits` drawn on a track `px` wide. */
function shown(fits: Fit[], px: number): string {
  return fits
    .filter((fit) => (fit.from === null || fit.from <= px) && (fit.to === null || px < fit.to))
    .map((fit) => fit.text)
    .join("|");
}

function aRun(
  phase: string,
  recipe: string,
  by: string | null,
  [queued, started, ended]: [number, number, number | null],
): Run {
  return {
    phase,
    label: recipe,
    by,
    commit: "0123456789abcdef",
    host: "studio",
    queued,
    started,
    ended,
    result: ended === null ? "running" : "green",
  };
}

/** Each row as (at, label, took, word, under its stage). */
const said = (listed: Row[]) =>
  listed.map((row) => [row.at, row.label, row.took ?? "", row.word?.[0] ?? "", row.sub]);

const close = (a: number, b: number) => Math.abs(a - b) < 1e-9;

describe("a task's timeline as the server answers it", () => {
  it("a run's wait and run time count up to now while it goes on", () => {
    const now = 100;
    const run = (queued: number, started: number | null, ended: number | null, result: RunResult) =>
      ({ phase: "test", label: "just test-rs", queued, started, ended, result }) as Run;
    const finished = run(10, 13, 40, "green");
    expect([waited(finished, now), ran(finished, now)]).toEqual([3, 27]);
    const running = run(50, 60, null, "running");
    expect([waited(running, now), ran(running, now)]).toEqual([10, 40]);
    const queued = run(90, null, null, "queued");
    expect([waited(queued, now), ran(queued, now)]).toEqual([10, 0]);
    // A recorder lost before its command started: it waited until it ended, and never ran.
    const lost = run(20, null, 25, "unknown");
    expect([waited(lost, now), ran(lost, now)]).toEqual([5, 0]);
    const timeline = aTimeline({ now, runs: [finished, running, queued, lost] });
    expect(machineSeconds(timeline)).toBe(67);
    expect(queuedSeconds(timeline)).toBe(28);
    // A run's times written out of order count as nothing, never below it.
    expect([waited(run(50, 40, 30, "green"), now), ran(run(50, 40, 30, "green"), now)]).toEqual([
      0, 0,
    ]);
  });

  it("a time takes the offset of the last change of the zone before it", () => {
    const timeline = aTimeline({
      zone: [
        { from: 100, seconds: 7200 },
        { from: 500, seconds: 3600 },
      ],
      every: 5,
    });
    // Before the first change (a time before the task), at it, between and after.
    expect([50, 100, 499, 500, 9e9].map((when) => offsetAt(timeline, when))).toEqual([
      7200, 7200, 7200, 3600, 3600,
    ]);
    expect(everySeconds(timeline)).toBe(5);
    // A change of summer time at the very second: before it the old offset, at it the new.
    expect([499.999, 500].map((when) => offsetAt(timeline, when))).toEqual([7200, 3600]);
    expect(offsetAt(aTimeline(), 3)).toBe(0);
  });

  it("a timeline ends at its close or now or a run that went on past them", () => {
    const timeline = aTimeline({ now: 100, created: 10 });
    expect(timelineEnd(timeline)).toBe(100);
    timeline.closed = 60;
    expect(timelineEnd(timeline)).toBe(60);
    timeline.runs = [{ ...aRun("test", "x", null, [50, 55, 70]) }];
    expect(timelineEnd(timeline)).toBe(70);
    timeline.runs.push(aRun("test", "x", null, [80, 81, null]));
    expect(timelineEnd(timeline)).toBe(100);
    // Never before its making, whatever its clock says.
    expect(timelineEnd(aTimeline({ now: 5, created: 10 }))).toBe(10);
  });

  it("words a newer server writes read as unknown and empty fields stay out", () => {
    const verdict = readTimeline({
      now: 5,
      created: 1,
      rounds: [{ round: 1, from: 1, verdict: "withdrawn" }],
      runs: [{ phase: "test", label: "x", queued: 1, result: "cancelled" }],
      gates: [{ name: "g", states: [{ state: "expired", from: 1 }] }],
    });
    expect(verdict?.rounds[0]?.verdict).toBe("other");
    expect(verdictWord("other")).toBe("verdict");
    expect(verdict?.runs[0]?.result).toBe("unknown");
    expect(resultWord("unknown")).toBe("ended");
    expect(verdict?.gates[0]?.states[0]?.state).toBe("unknown");
    for (const word of ["queued", "running", "green", "red"] as const) {
      expect(resultWord(word)).toBe(word);
    }
    const read = readTimeline({ slug: "tools/x", now: 5, created: 1 });
    expect(read).not.toBeNull();
    expect([offsetAt(read as TaskTimeline, 3), everySeconds(read as TaskTimeline)]).toEqual([
      0,
      TIMELINE_EVERY,
    ]);
    expect(read?.stages).toEqual([]);
    expect(read?.runs).toEqual([]);
    expect(read?.gates).toEqual([]);
    expect(stageSeconds(aStage({ status: "review", from: 2 }), 5)).toBe(3);
  });
});

describe("the read's guard", () => {
  it("reads the server's fixture whole, each field as serde reads it", () => {
    const timeline = pictures();
    expect(timeline.slug).toBe("tools/verify-stops-when-moved");
    expect(timeline.stages).toHaveLength(6);
    expect(timeline.stages[0]).toEqual({
      status: "ready",
      from: 1794380520,
      to: 1794382980,
      by: "glacial-merlin",
      round: 0,
      approved: false,
    });
    expect(timeline.stages[5]?.approved).toBe(true);
    expect(timeline.rounds[1]).toEqual({
      round: 2,
      from: 1794397020,
      taken: 1794397260,
      to: 1794398700,
      by: "lucid-heron",
      verdict: "approved",
    });
    expect(timeline.runs[4]).toMatchObject({ by: null, ended: null, result: "running" });
    expect(timeline.gates[1]?.states[0]).toEqual({
      state: "missing",
      from: 1794380520,
      by: null,
      detail: "Waits on tools/slot-pool-by-tree",
    });
    expect([timeline.runs_from, timeline.closed, timeline.first_claimed]).toEqual([
      null,
      null,
      1794382980,
    ]);
  });

  it("gives what serde defaults its default when it is absent, and an option none when null", () => {
    const read = readTimeline({
      now: 5,
      created: 1,
      closed: null,
      stages: [{ status: "ready", from: 1, to: null }],
      runs: [{ phase: "build", label: "x", queued: 1, result: "green" }],
      gates: [{ name: "g" }, { name: "h", states: [{ state: "met", from: 2 }] }],
      claims: [{ by: "a", from: 1 }],
    });
    expect(read).toMatchObject({ every: 0, bounces: 0, closed: null, zone: [], rounds: [] });
    expect(read?.stages[0]).toEqual({
      status: "ready",
      from: 1,
      to: null,
      by: null,
      round: 0,
      approved: false,
    });
    expect(read?.runs[0]).toMatchObject({ host: "", by: null, commit: null, started: null });
    expect(read?.gates[0]?.states).toEqual([]);
    expect(read?.gates[1]?.states[0]?.detail).toBe("");
    expect(read?.claims[0]).toEqual({ by: "a", from: 1, to: null });
    // A slug, which nothing draws, is not checked.
    expect(readTimeline({ now: 5, created: 1, slug: 7 })?.slug).toBe("");
  });

  it("refuses an answer without what the timeline draws, or one of another type", () => {
    const good = { now: 5, created: 1 };
    for (const value of [
      null,
      "timeline",
      [good],
      { created: 1 },
      { now: "5", created: 1 },
      { ...good, zone: null },
      { ...good, zone: [{ from: 1, seconds: 1.5 }] },
      { ...good, every: -1 },
      { ...good, every: 2.5 },
      { ...good, bounces: null },
      { ...good, closed: "soon" },
      { ...good, first_claimed: "x" },
      { ...good, runs_from: true },
      { ...good, stages: {} },
      { ...good, stages: [{ status: "ready" }] },
      { ...good, stages: [{ status: "ready", from: 1, round: -1 }] },
      { ...good, stages: [{ status: "ready", from: 1, approved: "yes" }] },
      { ...good, stages: [{ status: "ready", from: 1, by: 3 }] },
      { ...good, claims: [{ by: null, from: 1 }] },
      { ...good, claims: [{ by: "a", from: 1, to: "x" }] },
      { ...good, rounds: [{ round: 1, from: 1, verdict: 5 }] },
      { ...good, rounds: [{ round: "1", from: 1 }] },
      { ...good, rounds: [{ round: 1, from: 1, taken: "x" }] },
      { ...good, runs: [{ phase: "test", label: "x", queued: 1 }] },
      { ...good, runs: [{ phase: "test", label: "x", queued: 1, result: "green", host: null }] },
      { ...good, runs: [{ phase: "test", queued: 1, result: "green" }] },
      { ...good, runs: [{ phase: "test", label: "x", queued: 1, result: "green", ended: "x" }] },
      { ...good, gates: [{ states: [] }] },
      { ...good, gates: [{ name: "g", states: [{ state: 1, from: 1 }] }] },
      { ...good, gates: [{ name: "g", states: [{ state: "met", from: 1, detail: null }] }] },
    ]) {
      expect(readTimeline(value), JSON.stringify(value)).toBeNull();
    }
  });
});

describe("a task's timeline laid out", () => {
  it("a length reads to the minute in hours and days and never below nothing", () => {
    const cases: [number, string][] = [
      [-5, "0 m"],
      [0, "0 m"],
      [29, "0 m"],
      [31, "1 m"],
      [2460, "41 m"],
      [3580, "1 h"],
      [6720, "1 h 52 m"],
      [22_680, "6 h 18 m"],
      [86_400, "1 d"],
      [2 * 86_400 + 3 * 3600 + 20 * 60, "2 d 3 h"],
      // Half a minute is a minute, as Rust rounds a tie away from nothing; nothing at all is none.
      [30, "1 m"],
      [90, "2 m"],
      [Number.NaN, "0 m"],
    ];
    for (const [seconds, words] of cases) expect(took(seconds), `${seconds} s`).toBe(words);
  });

  it("a run's length reads to the second under a minute, else to the minute", () => {
    const cases: [number, string][] = [
      [-5, "0 s"],
      [0, "0 s"],
      [0.44, "0 s"],
      [0.5, "1 s"],
      [47, "47 s"],
      [59.49, "59 s"],
      // Half a second short of a minute rounds to it: a minute, never "60 s".
      [59.5, "1 m"],
      [60, "1 m"],
      [3292.14, "55 m"],
      [6720, "1 h 52 m"],
      [Number.NaN, "0 s"],
    ];
    for (const [seconds, words] of cases) expect(lasted(seconds), `${seconds} s`).toBe(words);
  });

  it("a time reads on the server's local clock and its day by the calendar", () => {
    const midnight = 20_768 * 86_400;
    expect(clock(midnight, 0)).toBe("00:00");
    expect(clock(midnight - 1, 0)).toBe("23:59");
    expect(clock(midnight, 7200)).toBe("02:00");
    expect(clock(midnight, -60)).toBe("23:59");
    expect(date(midnight - 1, 0)).toBe(date(midnight - 86_400, 0));
    expect(date(midnight, -60)).not.toBe(date(midnight, 0));
    expect(civil(0)).toEqual([1970, 1, 1]);
    for (const [year, month, day] of [
      [2026, 11, 11],
      [2024, 2, 29],
      [2000, 3, 1],
      [1969, 12, 31],
      [1900, 2, 28],
      [-1, 12, 31],
    ] as const) {
      const minutes = moment(year, month, day, 0, 0) as number;
      expect(civil(Math.floor(minutes / 1440))).toEqual([year, month, day]);
    }
    const timeline = pictures();
    expect(clockOf(timeline, timeline.created)).toBe("09:02");
    expect(dateOf(timeline, timeline.created)).toBe("11-11");
    // Before 1970 a time still reads on its own day.
    expect([clock(-60, 0), date(-60, 0)]).toEqual(["23:59", "12-31"]);
  });

  it("each time reads at the offset its own instant had across a change of summer time", () => {
    // The clock went back an hour at 12:00 on the pictures' clock (10:00 UTC).
    const change = at(178);
    const timeline = {
      ...pictures(),
      zone: [
        { from: pictures().created, seconds: 7200 },
        { from: change, seconds: 3600 },
      ],
    };
    expect(clockOf(timeline, timeline.created)).toBe("09:02");
    expect(clockOf(timeline, change - 60)).toBe("11:59");
    expect(clockOf(timeline, change)).toBe("11:00");
    // The second's own offset at the change's very second, the one before a second earlier.
    expect(clockOf(timeline, change - 1)).toBe("11:59");
    const listed = rows(timeline).map((row) => row.at);
    // The second round's changes came after the change.
    expect(listed.slice(0, 6)).toEqual(["09:02", "09:43", "10:28", "10:52", "11:35", "11:27"]);
    expect(lanes(timeline).now?.label).toBe("now 14:20");
  });

  it("the pictures' task heads its timeline with its totals", () => {
    expect(totals(pictures())).toEqual([
      "6 h 18 m so far · 2 review rounds · 1 bounce",
      "1 h 33 m, queued 26 m",
    ]);
    const closed = { ...pictures(), closed: pictures().now, rounds: [], bounces: 0, runs: [] };
    expect(totals(closed)).toEqual(["6 h 18 m in all · 0 review rounds · 0 bounces", null]);
    expect(keptTitle(pictures())).toBeNull();
    const rotated = { ...pictures(), runs_from: at(83) };
    expect(keptTitle(rotated)).toBe("Builds and tests before 11-11 10:25 are not kept");
    // One round, one bounce: each said once.
    const one = { ...pictures(), rounds: pictures().rounds.slice(0, 1) };
    expect(totals(one)[0]).toBe("6 h 18 m so far · 1 review round · 1 bounce");
  });

  it("the time no run is kept of is a row at its end in the list and a hatch over the runs lanes", () => {
    const rotated = { ...pictures(), runs_from: at(86) };
    // At the instant the build was queued: the row before it, under the stage it began in.
    const listed = rows(rotated);
    const kept = listed.findIndex((row) => row.kept);
    expect(listed[kept]).toEqual({
      start: at(86),
      day: null,
      at: "10:28",
      sub: true,
      event: true,
      tone: "muted-foreground",
      label: "Builds and tests kept from here on",
      took: null,
      bar: null,
      word: null,
      title: "Builds and tests before 11-11 10:28 are not kept",
      kept: true,
    });
    expect(listed[kept + 1]?.label).toBe("Build, just lint-rs · queued 3 m");
    expect(listed[kept - 1]?.label).toBe("In progress · quiet-wren");
    expect(rows(pictures()).some((row) => row.kept)).toBe(false);
    expect(keptRow(pictures())).toBeNull();
    // Wide: from the task's making to it, its words where they fit, over both runs lanes.
    const hatch = lanes(rotated).notKept;
    expect(hatch?.left).toBeCloseTo((2 / 380) * 100, 10);
    expect(hatch?.width).toBeCloseTo((86 / 380) * 100, 10);
    // 27 characters and their padding, 184 px, in 23% of the track: from 850 px.
    expect(hatch?.label).toEqual({ text: "not kept before 11-11 10:28", from: 850, to: null });
    expect(hatch?.title).toBe("Builds and tests before 11-11 10:28 are not kept");
    // Too narrow for its words: a hatch alone.
    expect(lanes({ ...pictures(), runs_from: at(4) }).notKept?.label).toBeNull();
    // None where every run is kept, where the task has no runs lane to hatch, or where the
    // recorder's history begins at the task's making.
    expect(lanes(pictures()).notKept).toBeNull();
    // With no run kept at all, no runs lane to hatch: the list's row still says since when.
    expect(lanes({ ...rotated, runs: [] }).notKept).toBeNull();
    expect(
      rows({ ...rotated, runs: [] })
        .filter((row) => row.kept)
        .map((row) => row.label),
    ).toEqual(["Builds and tests kept from here on"]);
    expect(lanes({ ...pictures(), runs_from: pictures().created }).notKept).toBeNull();
  });

  it("the phone list reads as the approved picture, each run and gate change under its stage", () => {
    const listed = rows(pictures());
    expect(said(listed)).toEqual([
      ["09:02", "Ready, unclaimed", "41 m", "", false],
      ["09:43", "In progress · quiet-wren", "1 h 52 m", "", false],
      // A build is named by its command; one that went green says no more than that it was built.
      ["10:28", "Build, just lint-rs · queued 3 m", "4 m", "", true],
      ["10:52", "Test, just verify · queued 6 m", "24 m", "green", true],
      ["11:35", "Review, round 1 · lucid-heron", "52 m", "changes", false],
      ["12:27", "In progress again", "1 h 10 m", "", false],
      ["13:00", "Test, just verify · queued 2 m", "23 m", "green", true],
      ["13:37", "Review, round 2 · lucid-heron", "28 m", "approved", false],
      ["14:05", "Approved, waiting to land", "1 h 15 m", "now", false],
      // The machine's own run, nobody's: what it runs and the commit it ran at.
      ["14:05", "Certificate verify · e19d3a0", "24 m", "green", true],
      ["14:12", "Dependency waived · cobalt-otter", "", "", true],
      // The certificate met at 14:34 is the green run's row: no row of its own. Missing again, it
      // says why.
      ["14:50", "Main moved: certificate again", "", "", true],
      ["14:52", "Certificate verify · 4be91c2", "18 m", "running", true],
    ]);
    // One day: no day marks.
    expect(listed.every((row) => row.day === null)).toBe(true);
    // The longest step, in progress for 1 h 52 m, fills the bar's span; the rest are as long as
    // their time.
    const bar = (index: number) => listed[index]?.bar as NonNullable<Row["bar"]>;
    expect(bar(1).work).toBe(BAR_SPAN);
    expect(close(bar(0).work, (41 / 112) * BAR_SPAN)).toBe(true);
    // A review's bar waits for its reviewer, lighter, then works.
    expect(close(bar(4).wait, (18 / 112) * BAR_SPAN)).toBe(true);
    expect(close(bar(4).work, (34 / 112) * BAR_SPAN)).toBe(true);
    // Waiting to land is all wait; what runs now is striped after its queue; a gate change is an
    // instant.
    expect([bar(8).work, bar(8).wait > 0]).toEqual([0, true]);
    expect(bar(12).running && !bar(9).running).toBe(true);
    expect(bar(12).waitTone).toBe("primary");
    expect(listed[10]?.event && listed[10]?.bar === null).toBe(true);
    expect(listed[11]?.title).toBe("Main moved: verify on the merge tree 4be91c2 running");
    expect(listed[12]?.tone).toBe("primary");
    expect(listed[3]?.tone).toBe("like");
    // A run's row says all of it: what it ran, its wait and its run time apart, and where.
    expect(listed[9]?.title).toBe(
      "Test, certificate verify · waited 5 m · ran 24 m · green · at e19d3a0 on studio",
    );
    expect(listed[2]?.title).toBe(
      "Build, just lint-rs · waited 3 m · ran 4 m · green · at 9ac5149 by quiet-wren on studio",
    );
    expect(listed[0]?.title).toBe(
      "Ready, unclaimed: from 09:02 to 09:43, moved there by glacial-merlin",
    );
    expect(listed[8]?.title).toBe(
      "Approved, waiting to land: from 14:05 until now, moved there by lucid-heron",
    );
  });

  it("a gate change says why only from a short reason, and shows a met no run said", () => {
    expect(reason("Main moved: verify on the merge tree 4be91c2 running")).toBe("Main moved");
    expect(reason("Verify on the merge tree e19d3a0 running")).toBeNull();
    expect(reason(": nothing")).toBeNull();
    // More than three words is no short reason.
    expect(reason("the branch moved on and on again: rerun")).toBeNull();
    // Words are told apart by any space, as Rust's.
    expect(reason("Main  moved\tagain: rerun")).toBe("Main  moved\tagain");
    // No run went green when the certificate was met: its row says so.
    const timeline = { ...pictures(), runs: [] };
    expect(gateChanges(timeline).map((row) => row.label)).toEqual([
      "Certificate met",
      "Main moved: certificate again",
      "Dependency waived · cobalt-otter",
    ]);
    // Missing again with no reason to give.
    const unsaid = pictures();
    (unsaid.gates[0]?.states[2] as { detail: string }).detail =
      "Verify on the merge tree 4be91c2 running";
    expect(gateChanges(unsaid)[0]?.label).toBe("Certificate missing again");
  });

  it("a gate change says each state it took, who held or waived it, and its detail", () => {
    const span = (state: GateState, from: number, by: string | null = null, detail = "") => ({
      state,
      from,
      by,
      detail,
    });
    type GateState = "met" | "missing" | "waived" | "held" | "unknown";
    const timeline = aTimeline({
      now: 600,
      gates: [
        {
          name: "owner_card",
          states: [
            span("missing", 0),
            span("held", 60, "owner"),
            span("unknown", 120),
            span("waived", 180),
            span("missing", 240),
            span("met", 300, null, "Met at last"),
          ],
        },
      ],
    });
    expect(gateChanges(timeline).map((row) => [row.label, row.tone, row.title])).toEqual([
      ["Owner card held · owner", "dislike", "Owner card held · owner"],
      ["Owner card unknown", "muted-foreground", "Owner card unknown"],
      ["Owner card waived", "resolved", "Owner card waived"],
      ["Owner card missing", "question", "Owner card missing"],
      ["Owner card met", "like", "Met at last"],
    ]);
  });

  it("a stage left within its first half minute draws no 0 m row", () => {
    const timeline = pictures();
    // Moved to review and back within twenty seconds, before the first claim.
    timeline.stages.splice(
      1,
      0,
      aStage({ status: "blocked", from: at(41), to: at(41) + 20, by: "cobalt-otter" }),
    );
    (timeline.stages[2] as Stage).from = at(41) + 20;
    const listed = rows(timeline);
    expect(listed.every((row) => row.took !== "0 m")).toBe(true);
    expect(listed.every((row) => !row.label.startsWith("Blocked"))).toBe(true);
    // The one stage going on now keeps its row however short.
    const now = pictures().now;
    const fresh = {
      ...pictures(),
      created: now - 10,
      stages: [aStage({ status: "ready", from: now - 10 })],
      rounds: [],
      runs: [],
      gates: [],
      claims: [],
      first_claimed: null,
    };
    expect(said(rows(fresh))).toEqual([["15:19", "Ready, unclaimed", "0 m", "now", false]]);
  });

  it("with no run kept there is no run row, no machine time and no runs lane", () => {
    const timeline = { ...pictures(), runs: [] };
    const listed = rows(timeline);
    expect(
      listed.every((row) => !row.label.startsWith("Build") && !row.label.startsWith("Test")),
    ).toBe(true);
    // Six stages and three gate changes.
    expect(listed).toHaveLength(9);
    expect(totals(timeline)[1]).toBeNull();
    expect(lanes(timeline).lanes.map((lane) => lane.key)).toEqual([
      "stage",
      "executor",
      "reviewer",
    ]);
  });

  it("a timeline over two days marks each day's first row", () => {
    // Made at 23:30 the day before, ready until 09:43.
    const made = pictures().created - (9 * 3600 + 32 * 60);
    const timeline = { ...pictures(), created: made };
    (timeline.stages[0] as Stage).from = made;
    const listed = rows(timeline);
    expect(listed.filter((row) => row.day !== null).map((row) => [row.at, row.day])).toEqual([
      ["23:30", "11-10"],
      ["09:43", "11-11"],
    ]);
    expect(listed[0]?.label).toBe("Ready, unclaimed");
    expect(listed[0]?.took).toBe("10 h 13 m");
  });

  it("a stage says who moved it only where its name says nothing else", () => {
    const created = pictures().created;
    const stage = (status: string, by: string) =>
      aStage({ status, from: created, to: created + 600, by });
    const timeline = {
      ...pictures(),
      first_claimed: created + 300,
      stages: [
        stage("ready", "glacial-merlin"),
        stage("blocked", "cobalt-otter"),
        { ...stage("done", "lucid-heron"), to: null },
      ],
      rounds: [],
      runs: [],
      gates: [],
      closed: created + 600,
    };
    const listed = rows(timeline);
    expect(said(listed)).toEqual([
      ["09:02", "Ready", "10 m", "", false],
      ["09:02", "Blocked · cobalt-otter", "10 m", "", false],
      ["09:02", "Done · lucid-heron", "", "", false],
    ]);
    expect([listed[1]?.tone, listed[2]?.tone]).toEqual(["dislike", "like"]);
    // A close is an instant.
    expect(listed[2]?.bar).toBeNull();
    // A closed task has no now.
    expect(lanes(timeline).now).toBeNull();
  });

  it("a stage of a status the list does not name says its own word, in progress again by whom", () => {
    const timeline = aTimeline({
      now: 7200,
      stages: [
        aStage({ status: "concept", from: 0, to: 600 }),
        aStage({ status: "in_progress", from: 600, to: 1200 }),
        aStage({ status: "review", from: 1200, to: 1800 }),
        aStage({ status: "in_progress", from: 1800, to: 2400 }),
        aStage({ status: "in_progress", from: 2400, to: 3000 }),
        aStage({ status: "odd_word", from: 3000 }),
      ],
      claims: [
        { by: "quiet-wren", from: 600, to: 1200 },
        { by: "teal-lynx", from: 2400, to: 3000 },
      ],
    });
    expect(rows(timeline).map((row) => [row.label, row.tone])).toEqual([
      ["Concept", "decision"],
      ["In progress · quiet-wren", "primary"],
      // A review with no round says its status alone.
      ["Review", "question"],
      // Nobody claimed it this time; the next claim was another executor's, who is named.
      ["In progress again", "primary"],
      ["In progress again · teal-lynx", "primary"],
      ["Odd word", "muted-foreground"],
    ]);
  });

  it("a review round's bar waits until its reviewer took it, and says its verdict", () => {
    const timeline = aTimeline({
      now: 3600,
      stages: [aStage({ status: "review", from: 0, round: 1 })],
      rounds: [{ round: 1, from: 0, taken: 600, by: "lucid-heron", verdict: "other" }],
    });
    const [row] = rows(timeline);
    expect(row?.word).toEqual(["verdict", "muted-foreground"]);
    expect(row?.bar?.wait).toBeCloseTo((600 / 3600) * BAR_SPAN);
    expect(row?.bar?.work).toBeCloseTo((3000 / 3600) * BAR_SPAN);
    // Taken before the stage began, its wait is none.
    const early = aTimeline({
      now: 3600,
      stages: [aStage({ status: "review", from: 600, round: 1 })],
      rounds: [{ round: 1, from: 0, taken: 0 }],
    });
    expect(rows(early)[0]?.bar?.wait).toBe(0);
    expect(rows(early)[0]?.label).toBe("Review, round 1");
  });

  it("rows that begin at one instant keep the order they were found in, as Rust's stable sort", () => {
    const met = (from: number) => ({
      name: "check",
      states: [
        { state: "missing" as const, from: -1, by: null, detail: "" },
        { state: "met" as const, from, by: null, detail: "" },
      ],
    });
    const at100 = aTimeline({
      now: 600,
      stages: [aStage({ status: "in_progress", from: 0 })],
      runs: [
        aRun("build", "just lint-rs", "a", [100, 100, 200]),
        aRun("test", "just verify", "a", [100, 110, 300]),
      ],
      gates: [met(100)],
    });
    expect(rows(at100).map((row) => row.label)).toEqual([
      "In progress",
      "Build, just lint-rs",
      "Test, just verify",
      "Check met",
    ]);
    // Rust's `total_cmp` puts -0 before 0, so a change at -0 goes before a run at 0.
    const zeros = {
      ...at100,
      runs: [aRun("build", "just lint-rs", "a", [0, 0, 200])],
      gates: [met(-0)],
    };
    expect(rows(zeros).map((row) => row.label)).toEqual([
      "In progress",
      "Check met",
      "Build, just lint-rs",
    ]);
  });

  it("a word is drawn only at the track widths its room holds it", () => {
    // Room enough at every width, at some, at none.
    expect(widthFor("41 m", 50, PAD_PX)).toBeNull();
    expect(widthFor("ready 41 m", 10, PAD_PX)).toBe(750);
    expect(widthFor("ready 41 m", 0.5, PAD_PX)).toBeUndefined();
    expect(widthFor("", 50, PAD_PX)).toBeUndefined();
    expect(widthFor("41 m", 0, PAD_PX)).toBeUndefined();
    // The long words where they fit, the short below that, nothing below the short's own.
    const fits = fitted("ready 41 m", "41 m", 10);
    expect(shown(fits, 1280)).toBe("ready 41 m");
    expect(shown(fits, 750)).toBe("ready 41 m");
    expect(shown(fits, 700)).toBe("41 m");
    expect(shown(fits, 400)).toBe("41 m");
    expect(shown(fits, 350)).toBe("");
    expect(fitClass(fits[1] as Fit)).toBe("tl-fit tl-from-400 tl-to-750");
    // A short word no narrower than the long one is never drawn beside it.
    expect(fitted("41 m", "41 m", 10)).toHaveLength(1);
    // The last label may run into the lanes' end margin.
    const last = { room: 1, last: true };
    expect(label("running 18 m", last)?.from).toBeNull();
    expect(roomCss(last)).toBe("calc(1.00% + var(--end))");
    const between = { room: 1, last: false };
    expect(label("running 18 m", between)).toBeNull();
    expect(roomCss(between)).toBe("1.00%");
  });

  it("a word's room is weighed by its characters, and a room's width rounds a tie to even", () => {
    // Weighed at exactly 300 px it shows at every width; a hair more, from 400.
    expect(widthFor("ab", ((2 * CHAR_PX + PAD_PX) * 100) / 300, PAD_PX)).toBeNull();
    expect(widthFor("ab", ((2 * CHAR_PX + PAD_PX) * 100) / 301, PAD_PX)).toBe(400);
    // An accented word is as wide as its letters, not its bytes.
    expect(widthFor("é".repeat(8), 10, PAD_PX)).toBe(widthFor("e".repeat(8), 10, PAD_PX));
    // A short word that fits where the long one never does shows wherever it fits.
    // "2 m" needs 3 × 6 + 12 = 30 px, 3 % of a 1000 px track exactly.
    expect(fitted("a very long word for one segment", "2 m", 3)).toEqual([
      { text: "2 m", from: 1000, to: null },
    ]);
    // A short word that fits only where the long one does is left out.
    expect(fitted("ab", "a", 0.5)).toEqual([]);
    // "abc" needs 30 px of 3.2 %, a 937.5 px track: from 950; "ab" 24 px, from 750 to there.
    expect(fitted("abc", "ab", 3.2)).toEqual([
      { text: "abc", from: 950, to: null },
      { text: "ab", from: 750, to: 950 },
    ]);
    // No short word: the long one alone, from where it fits (36 px of 10 %, 360 px) or at every
    // width (of 20 %, 180 px, below the narrowest track).
    expect(fitted("long", "", 10)).toEqual([{ text: "long", from: 400, to: null }]);
    expect(fitted("long", "", 20)).toEqual([{ text: "long", from: null, to: null }]);
    // Rust's `{:.2}`: a tie rounds to the even digit.
    expect(roomCss({ room: 0.125, last: false })).toBe("0.12%");
    expect(roomCss({ room: 0.375, last: true })).toBe("calc(0.38% + var(--end))");
    // The last label of none at all still shows: it takes the end margin.
    expect(label("", { room: 0, last: true })).toEqual({ text: "", from: null, to: null });
    // Twenty characters past the end margin need 120 + 4 − 88 = 36 px: 5 % of 720 px, from 750;
    // 2 % of 1800 px, wider than the widest track.
    expect(label("a".repeat(20), { room: 5, last: true })?.from).toBe(750);
    expect(label("a".repeat(20), { room: 2, last: true })).toBeNull();
    expect(fitClass({ text: "x", from: null, to: 500 })).toBe("tl-fit tl-to-500");
    expect(WIDTHS).toEqual(Array.from({ length: 25 }, (_, at) => 400 + 50 * at));
  });

  it("the wide lanes read as the approved picture at its width, and mark the hours and now", () => {
    const drawn = lanes(pictures());
    expect(drawn.ticks.map((tick) => tick.label)).toEqual([
      "09:00",
      "10:00",
      "11:00",
      "12:00",
      "13:00",
      "14:00",
      "15:00",
    ]);
    expect(Math.abs(drawn.ticks[0]?.left as number)).toBeLessThan(1e-9);
    expect(close(drawn.ticks[6]?.left as number, (360 / 380) * 100)).toBe(true);
    expect(drawn.now).toEqual({ left: 100, label: "now 15:20" });
    const lane = (key: string) => drawn.lanes.find((one) => one.key === key);
    // The words of a lane on a track `px` wide: the picture's is 950.
    const texts = (key: string, px: number) => lane(key)?.segs.map((seg) => shown(seg.text, px));
    expect(drawn.lanes.map((one) => [one.name, one.who, one.rows])).toEqual([
      ["Stage", "", 1],
      ["Executor", "quiet-wren", 1],
      ["Reviewer", "lucid-heron", 1],
      // The builds and the tests each a lane of their own, never one, its name saying its counts;
      // the tests one after another on one row.
      ["Builds", "1 run · 0 red", 1],
      ["Tests", "4 runs · 0 red", 1],
    ]);
    expect(texts("stage", 950)).toEqual([
      "ready 41 m",
      "in progress 1 h 52 m",
      "review 52 m",
      "in progress 1 h 10 m",
      "28 m",
      "waiting to land 1 h 15 m",
    ]);
    // At the narrowest the lanes are drawn (a 900 px page), the shorter words.
    expect(texts("stage", 600)).toEqual([
      "41 m",
      "in progress 1 h 52 m",
      "review 52 m",
      "1 h 10 m",
      "28 m",
      "1 h 15 m",
    ]);
    expect(lane("stage")?.segs[5]?.look).toBe("waiting");
    expect(texts("executor", 950)).toEqual(["claimed 1 h 52 m", "1 h 10 m"]);
    expect(texts("reviewer", 950)).toEqual(["", "round 1", "", "round 2"]);
    expect(texts("reviewer", 600)).toEqual(["", "", "", ""]);
    expect(
      lane("reviewer")
        ?.segs.filter((seg) => seg.after !== null)
        .map((seg) => [shown([seg.after?.[0] as Fit], 600), seg.after?.[1]]),
    ).toEqual([
      ["changes", "dislike"],
      ["approved", "like"],
    ]);
    // A run's words go inside the part of its bar the keyboard stops at, once that part is
    // BAR_WORDS_PX wide: its distinguishing words and its length. The four-minute build is 10 px on
    // the picture's track, too narrow; its queue, and every test's, says nothing.
    expect(texts("builds", 950)).toEqual(["", ""]);
    expect(texts("tests", 950)).toEqual([
      "",
      "verify · 24 m",
      "",
      "verify · 23 m",
      "",
      "certificate verify · 24 m",
      "",
      "certificate verify · 18 m",
    ]);
    // On the narrowest track the run going on for 18 minutes is 28 px, under BAR_WORDS_PX.
    expect(texts("tests", 600)).toEqual([
      "",
      "verify · 24 m",
      "",
      "verify · 23 m",
      "",
      "certificate verify · 24 m",
      "",
      "",
    ]);
    expect(lane("tests")?.segs.map((seg) => seg.row)).toEqual([0, 0, 0, 0, 0, 0, 0, 0]);
    const looks = lane("tests")?.segs.map((seg) => seg.look) ?? [];
    expect(looks.filter((look) => look === "running")).toHaveLength(1);
    expect(looks.at(-1)).toBe("running");
    expect(lane("builds")?.segs.map((seg) => seg.look)).toEqual(["waiting", "solid"]);
    // A run's part says its command, its wait and its run time apart: the one the keyboard stops
    // at, and its queue's, which a pointer rests on.
    expect(lane("builds")?.segs.map((seg) => [seg.title, seg.stop])).toEqual([
      [
        "Build, just lint-rs · waited 3 m · ran 4 m · green · at 9ac5149 by quiet-wren on studio",
        false,
      ],
      [
        "Build, just lint-rs · waited 3 m · ran 4 m · green · at 9ac5149 by quiet-wren on studio",
        true,
      ],
    ]);
    expect(lane("tests")?.segs.at(-1)?.title).toBe(
      "Test, certificate verify · waited 10 m · ran 18 m so far · at 4be91c2 on studio",
    );
    expect(drawn.gates.map((gate) => [gate.name, gate.spans.map((span) => span.state)])).toEqual([
      ["certificate", ["missing", "met", "missing"]],
      ["dependency", ["missing", "waived"]],
      ["nightly", ["met"]],
      // From the move to review: not reviewed yet, changes asked, approved.
      ["review", ["missing", "missing", "met"]],
    ]);
    // The nightly, met before the task, holds from the task's making to now, and says so.
    const nightly = drawn.gates[2]?.spans[0];
    expect(close(nightly?.left as number, (2 / 380) * 100)).toBe(true);
    expect(close((nightly?.left as number) + (nightly?.width as number), 100)).toBe(true);
    expect(nightly?.word?.text).toBe("met");
    expect(nightly?.title.startsWith("nightly met from 09:02: ")).toBe(true);
    // Each step a stop for the keyboard (a round's wait and a run's queue are their step's), and
    // each gate's state.
    const parts = (key: string) => {
      const segs = lane(key)?.segs ?? [];
      return [segs.length, segs.filter((seg) => seg.stop).length];
    };
    expect(parts("reviewer")).toEqual([4, 2]);
    expect(parts("builds")).toEqual([2, 1]);
    expect(parts("tests")).toEqual([8, 4]);
    expect(stops(drawn)).toBe(6 + 2 + 2 + 1 + 4 + 3 + 2 + 1 + 3);
    expect(legendStates(drawn)).toEqual(["met", "missing", "waived"]);
  });

  it("only runs that overlap take a row each, as many as they need, never one bar", () => {
    const runs = [
      aRun("test", "just lint-rs", "a", [0, 10, 100]),
      aRun("test", "just verify", "a", [20, 30, 60]),
      aRun("test", "just verify", "b", [40, 50, 90]),
      // A fourth over the three: a row of its own, never drawn over another's.
      aRun("test", "just verify", "c", [45, 50, 80]),
      // After the second's end: back in its row.
      aRun("test", "just verify", "d", [70, 75, 95]),
    ];
    expect(runRows(runs, 200)).toEqual([[0, 1, 2, 3, 1], 4]);
    expect(runRows(runs.slice(0, 1), 200)).toEqual([[0], 1]);
    expect(runRows([], 200)).toEqual([[], 1]);
    // One after another, however near: one row; queued at the instant the one before ended too.
    const apart = [
      aRun("test", "just lint-rs", "a", [0, 10, 20]),
      aRun("test", "just verify", "a", [20, 30, 60]),
      aRun("test", "just verify", "a", [60.5, 61, 62]),
    ];
    expect(runRows(apart, 200)).toEqual([[0, 0, 0], 1]);
    // Queued a second before the one before ended: they overlap.
    expect(runRows([apart[0] as Run, aRun("test", "x", "a", [19, 30, 60])], 200)).toEqual([
      [0, 1],
      2,
    ]);
    // One still running holds its row until now.
    const running = [
      aRun("test", "just verify", "a", [0, 10, null]),
      aRun("test", "just verify", "b", [150, 160, 170]),
    ];
    expect(runRows(running, 200)[0]).toEqual([0, 1]);
    expect(runRows(running, 100)[0]).toEqual([0, 0]);
    // A run queued long before it started holds its row from its queue: one that ran meanwhile
    // takes another.
    const waiting = [
      aRun("test", "just verify", "a", [0, 500, 600]),
      aRun("test", "just lint-rs", "a", [100, 101, 200]),
      aRun("test", "just lint-rs", "a", [600, 601, 700]),
    ];
    expect(runRows(waiting, 1000)).toEqual([[0, 1, 0], 2]);
    // Rows that free at once: each run its own while they overlap, the first freed taken after.
    const tied = [0, 1, 2, 3].map(() => aRun("test", "x", "a", [0, 1, 50]));
    expect(runRows(tied, 200)).toEqual([[0, 1, 2, 3], 4]);
    expect(runRows([...tied, aRun("test", "x", "a", [50, 51, 60])], 200)[0]).toEqual([
      0, 1, 2, 3, 0,
    ]);
  });

  it("a lane's runs are drawn one bar each in the first row free when queued", () => {
    const tests = [
      aRun("test", "just verify", "a", [0, 100, 4000]),
      aRun("test", "just test-rs -p symphony-board", "a", [1000, 1100, 3000]),
      aRun("test", "just test-rs -p symphony-api", "b", [2000, 2100, 3500]),
      aRun("test", "just test-rs -p symphony-web-tests", "c", [2500, 2600, 3800]),
      // Fifty seconds after the second's end: its row is free again.
      aRun("test", "just verify", "d", [3050, 3100, 5000]),
      // At the first's end: back in its row.
      aRun("test", "just verify", "d", [4000, 4000, 6000]),
    ];
    const build = aRun("build", "just lint-rs", "a", [0, 50, 6000]);
    const timeline = aTimeline({ now: 20_000, created: 0, runs: [build, ...tests] });
    const drawn = lanes(timeline);
    const lane = (key: string) => drawn.lanes.find((one) => one.key === key) as Lane;
    // The build overlaps every test, and has a lane and a row of its own.
    expect([lane("builds").rows, lane("builds").segs.map((seg) => seg.row)]).toEqual([1, [0, 0]]);
    expect(lane("tests").rows).toBe(4);
    expect(lane("tests").segs.map((seg) => seg.row)).toEqual([0, 0, 1, 1, 2, 2, 3, 3, 1, 1, 0, 0]);
    // Each run's queue is the head of its bar, its run after it, each its own part.
    const segs = lane("tests").segs;
    expect(segs.slice(0, 2).map((seg) => [seg.look, seg.left, seg.width])).toEqual([
      ["waiting", 0, 0.5],
      ["solid", 0.5, 19.5],
    ]);
    // The run with no wait has a queue of no width, only drawn.
    expect([segs[10]?.width, segs[10]?.stop, segs[11]?.stop]).toEqual([0, false, true]);
    expect(drawn.lanes.map((one) => one.rail)).toEqual([false, false, false]);
  });

  it("a burst of runs one after another is one row, its words inside each bar wide enough", () => {
    // A burst of tests a minute apart on a timeline of five days: each after the one before it,
    // so all on one row, every run still its own bar and a part the keyboard stops at.
    const days = 5 * 86_400;
    const burst = Array.from({ length: 40 }, (_, at) =>
      aRun("test", `just test-rs -p p${at}`, "a", [
        days - 3600 + at * 60,
        days - 3600 + at * 60 + 1,
        days - 3600 + at * 60 + 30,
      ]),
    );
    const long = aRun("test", "just test-rs -p symphony-board --test store", "a", [0, 0, 43_200]);
    const drawn = lanes(aTimeline({ now: days, created: 0, runs: [long, ...burst] }));
    const tests = drawn.lanes.find((one) => one.key === "tests") as Lane;
    expect(tests.rows).toBe(1);
    expect(tests.who).toBe("41 runs · 0 red");
    expect(tests.segs.filter((seg) => seg.stop).map((seg) => seg.title.split(" · ")[0])).toEqual([
      "Test, just test-rs -p symphony-board --test store",
      ...burst.map((_, at) => `Test, just test-rs -p p${at}`),
    ]);
    // Thirty seconds of five days is never 30 px: no words inside; the twelve hours are 10% of
    // the track, BAR_WORDS_PX from a 300 px track, so always.
    expect(tests.segs.slice(2).every((seg) => seg.text.length === 0)).toBe(true);
    expect(tests.segs[1]?.text).toEqual([
      { text: "test-rs symphony-board store · 12 h", from: null, to: null },
    ]);
  });

  it("a run's bar says its words from the width where it is BAR_WORDS_PX wide", () => {
    expect(BAR_WORDS_PX).toBe(30);
    // 10% of the track is 30 px from a 300 px track: always.
    expect(barFit("verify · 1 m", 10)).toEqual([{ text: "verify · 1 m", from: null, to: null }]);
    // 5% is 30 px from 600 px, 6% from 500; 7% from 450, the first of WIDTHS past 30 * 100 / 7.
    expect(barFit("verify · 1 m", 5)).toEqual([{ text: "verify · 1 m", from: 600, to: null }]);
    expect(barFit("verify · 1 m", 6)).toEqual([{ text: "verify · 1 m", from: 500, to: null }]);
    expect(barFit("verify · 1 m", 7)).toEqual([{ text: "verify · 1 m", from: 450, to: null }]);
    // 1.875% is 30 px at 1600 exactly; under it, at no width.
    expect(barFit("x", 1.875)).toEqual([{ text: "x", from: 1600, to: null }]);
    expect(barFit("x", 1.8)).toEqual([]);
    // Nothing to say, or no width: nothing.
    expect(barFit("", 50)).toEqual([]);
    expect(barFit("x", 0)).toEqual([]);
    expect(barFit("x", Number.NaN)).toEqual([]);
  });

  it("a run not started is its wait alone, its words and its wait inside it, a keyboard stop", () => {
    // One ended before it started (its recorder lost), 10 to 30 minutes in, and one queued still
    // at 50 minutes, on a timeline of 100 minutes.
    const lost: Run = {
      ...aRun("test", "just verify", "a", [600, 0, 1800]),
      started: null,
      result: "unknown",
    };
    const queued: Run = {
      ...aRun("test", "just test-rs -p web", "a", [3000, 0, null]),
      started: null,
      result: "queued",
    };
    const drawn = lanes(aTimeline({ created: 0, now: 6000, runs: [lost, queued] }));
    const tests = drawn.lanes.find((lane) => lane.key === "tests");
    expect(tests?.who).toBe("2 runs · 0 red");
    // Each a dashed wait from its queue to its end or now, on the one row the first frees: no
    // part of it ran, so the wait is the part the keyboard stops at, saying the wait's length.
    expect(
      tests?.segs.map((seg) => [seg.look, seg.row, seg.stop, seg.text.map((fit) => fit.text)]),
    ).toEqual([
      ["waiting", 0, true, ["verify · 20 m"]],
      ["waiting", 0, true, ["test-rs web · 50 m"]],
    ]);
    expect(tests?.segs.map((seg) => [seg.left, seg.width])).toEqual([
      [10, 20],
      [50, 50],
    ]);
    // A wait narrower than BAR_WORDS_PX at every width keeps its words to its hover and focus.
    const brief: Run = { ...queued, queued: 5990 };
    const narrow = lanes(aTimeline({ created: 0, now: 6000, runs: [brief] }));
    const only = narrow.lanes.find((lane) => lane.key === "tests")?.segs;
    expect(only?.map((seg) => [seg.stop, seg.text, seg.title])).toEqual([
      [true, [], runTitle(brief, 6000)],
    ]);
  });

  it("the owner's case reads as it ran: a 55-minute build, then another of 8, each named", () => {
    // perf/client-distribution-keeps-every-build, from the recorder's day files: a cold build of
    // the full install, a rebuild after a touch, a cold build of the pruned one, and a test and the
    // client's builds over it.
    const created = 1_794_485_013.498;
    const run = (
      phase: string,
      recipe: string,
      [queued, started, ended]: [number, number, number],
    ) => ({
      ...aRun(phase, recipe, "glacial-ibis", [queued, started, ended]),
      commit: "329c98640a491148c96e9da99ebdd2f2d0a3f8c8",
      host: "Stefanos-MacBook-Pro.local",
    });
    const runs = [
      run(
        "build",
        "cargo install symphony-server (cold-full)",
        [1_794_485_256.2, 1_794_485_256.64, 1_794_488_548.78],
      ),
      run(
        "build",
        "cargo install symphony-server (touch-full)",
        [1_794_488_602.28, 1_794_488_602.52, 1_794_489_109.33],
      ),
      run(
        "build",
        "cargo install symphony-server (cold-pruned)",
        [1_794_489_150.22, 1_794_489_150.74, 1_794_491_176.22],
      ),
      run(
        "test",
        "just test-rs -p symphony --lib -- cli::publish reload::build_tests",
        [1_794_489_874.0, 1_794_489_874.11, 1_794_490_072.58],
      ),
      run("build", "just client-update", [1_794_490_223.53, 1_794_490_223.7, 1_794_491_103.93]),
    ];
    const timeline = aTimeline({
      now: 1_794_863_486.18,
      zone: [{ from: created, seconds: 3600 }],
      created,
      closed: 1_794_494_770.6,
      stages: [aStage({ status: "in_progress", from: created, to: 1_794_494_770.6 })],
      runs,
    });
    const drawn = lanes(timeline);
    const lane = (key: string) => drawn.lanes.find((one) => one.key === key) as Lane;
    // The builds in a lane, the test in another, each a bar of its own: the pruned one's cold
    // build, queued 41 s after the touched rebuild ended, goes on in its row; the client's build,
    // queued while it ran, takes a row of its own.
    expect(
      lane("builds")
        .segs.filter((seg) => seg.stop)
        .map((seg) => [seg.row, seg.title]),
    ).toEqual([
      [
        0,
        "Build, cargo install symphony-server (cold-full) · waited 0 s · ran 55 m · green · at 329c986 by glacial-ibis on Stefanos-MacBook-Pro.local",
      ],
      [
        0,
        "Build, cargo install symphony-server (touch-full) · waited 0 s · ran 8 m · green · at 329c986 by glacial-ibis on Stefanos-MacBook-Pro.local",
      ],
      [
        0,
        "Build, cargo install symphony-server (cold-pruned) · waited 1 s · ran 34 m · green · at 329c986 by glacial-ibis on Stefanos-MacBook-Pro.local",
      ],
      [
        1,
        "Build, just client-update · waited 0 s · ran 15 m · green · at 329c986 by glacial-ibis on Stefanos-MacBook-Pro.local",
      ],
    ]);
    expect(
      lane("tests")
        .segs.filter((seg) => seg.stop)
        .map((seg) => seg.title),
    ).toEqual([
      "Test, just test-rs -p symphony --lib -- cli::publish reload::build_tests · waited 0 s · ran 3 m · green · at 329c986 by glacial-ibis on Stefanos-MacBook-Pro.local",
    ]);
    // On a phone each is a row named by its command, its run time beside it.
    expect(
      rows(timeline)
        .filter((row) => row.sub)
        .map((row) => [row.at, row.label, row.took]),
    ).toEqual([
      ["13:07", "Build, cargo install symphony-server (cold-full)", "55 m"],
      ["14:03", "Build, cargo install symphony-server (touch-full)", "8 m"],
      ["14:12", "Build, cargo install symphony-server (cold-pruned)", "34 m"],
      ["14:24", "Test, just test-rs -p symphony --lib -- cli::publish reload::build_tests", "3 m"],
      ["14:30", "Build, just client-update", "15 m"],
    ]);
  });

  it("a run's words inside its bar are its command's distinguishing words", () => {
    const words = (recipe: string) => runWords(aRun("test", recipe, "a", [0, 1, 60]));
    // No `just`, no flags; a flag's value and the test filter kept.
    expect(words("just test-rs -p symphony-board --test store")).toBe(
      "test-rs symphony-board store",
    );
    expect(words("just test-rs -p symphony-board --test cli -- beat")).toBe(
      "test-rs symphony-board cli beat",
    );
    expect(words("just lint-rs")).toBe("lint-rs");
    expect(words("cargo install symphony-server (cold-full)")).toBe(
      "cargo install symphony-server (cold-full)",
    );
    expect(words("cargo clippy --workspace --all-targets")).toBe("cargo clippy");
    expect(words("  certificate   verify ")).toBe("certificate verify");
    // Nothing left but flags or `just`: the command whole, as it ran.
    expect(words("just")).toBe("just");
    expect(words(" --help ")).toBe("--help");
    expect(words("")).toBe("");
  });

  it("a runs lane's name says how many ran and how many went red", () => {
    const run = (result: RunResult) => ({ ...aRun("test", "x", "a", [0, 1, 2]), result });
    expect(runCounts([run("green")])).toBe("1 run · 0 red");
    expect(runCounts([run("green"), run("red"), run("red"), run("running")])).toBe(
      "4 runs · 2 red",
    );
    expect(runCounts([])).toBe("0 runs · 0 red");
  });

  it("a run's row in the list says its result unless it was a green build", () => {
    const build = aRun("build", "just lint-rs", "a", [0, 1, 60]);
    const redBuild = { ...build, result: "red" as const };
    // A red build says so in the list too, by its command.
    expect(runRow(redBuild, pictures(), () => 1).word).toEqual(["red", "dislike"]);
    expect(runLabel(redBuild, 120)).toBe("Build, just lint-rs · queued 2 m");
    // A wait of less than a minute is not said in the label.
    expect(runLabel(redBuild, 59)).toBe("Build, just lint-rs");
    const gate = aRun("test", "certificate verify", null, [0, 1, 60]);
    expect(runLabel(gate, 600)).toBe("Certificate verify · 0123456");
    // A run nobody holds with no commit names what it runs alone.
    expect(runLabel({ ...gate, commit: null }, 0)).toBe("Certificate verify");
  });

  it("a run's row says all of it, and what waits to start counts its wait", () => {
    const timeline = { ...pictures(), now: 1000 };
    const waiting: Run = {
      phase: "test",
      label: "just verify",
      by: "a",
      commit: null,
      host: "",
      queued: 100,
      started: null,
      ended: null,
      result: "queued",
    };
    const row = runRow(waiting, timeline, (seconds) => seconds / 10);
    expect([row.label, row.took, row.title]).toEqual([
      "Test, just verify · queued 15 m",
      "15 m",
      "Test, just verify · waited 15 m so far · not started yet · by a",
    ]);
    expect(row.bar).toEqual({ wait: 90, waitTone: "primary", work: 0, running: false });
    expect(row.word).toEqual(["queued", "primary"]);
    // A short run's row says its seconds, never "0 m".
    const quick = runRow(aRun("test", "just test-rs", "a", [990, 991, 997]), timeline, () => 1);
    expect([quick.label, quick.took]).toEqual(["Test, just test-rs", "6 s"]);
  });

  it("a run's words say its command, its wait and its run time apart, and how it ended", () => {
    const run = aRun(
      "build",
      "cargo install symphony-server (cold-full)",
      "glacial-ibis",
      [100, 100.44, 3392.58],
    );
    expect(runTitle(run, 5000)).toBe(
      "Build, cargo install symphony-server (cold-full) · waited 0 s · ran 55 m · green · " +
        "at 0123456 by glacial-ibis on studio",
    );
    // Red, or ended with no word of how: said after its times.
    expect(runTitle({ ...run, result: "red" }, 5000)).toContain(" · ran 55 m · red · at ");
    expect(runTitle({ ...run, result: "unknown" }, 5000)).toContain(" · ran 55 m · ended · at ");
    // Going on: its run time so far, and no word of an end.
    const running = { ...run, ended: null, result: "running" as const };
    expect(runTitle(running, 1300.44)).toBe(
      "Build, cargo install symphony-server (cold-full) · waited 0 s · ran 20 m so far · " +
        "at 0123456 by glacial-ibis on studio",
    );
    // Waiting still, or ended before it started (its recorder lost).
    const queued = { ...run, started: null, ended: null, result: "queued" as const };
    expect(runTitle(queued, 145)).toBe(
      "Build, cargo install symphony-server (cold-full) · waited 45 s so far · not started yet · " +
        "at 0123456 by glacial-ibis on studio",
    );
    const lost = { ...run, started: null, ended: 400, result: "unknown" as const };
    expect(runTitle(lost, 5000)).toBe(
      "Build, cargo install symphony-server (cold-full) · waited 5 m · never started · ended · " +
        "at 0123456 by glacial-ibis on studio",
    );
    // Nobody's run, with no commit or machine said: what it ran and its times alone.
    const bare = { ...run, by: null, commit: null, host: "" };
    expect(runTitle(bare, 5000)).toBe(
      "Build, cargo install symphony-server (cold-full) · waited 0 s · ran 55 m · green",
    );
    expect(runTitle({ ...bare, host: "studio" }, 5000)).toMatch(/ · green · on studio$/);
  });

  it("a long timeline marks fewer hours, and a midnight by its date", () => {
    const made = pictures().now - 30 * 3600;
    const timeline = { ...pictures(), created: made };
    (timeline.stages[0] as Stage).from = made;
    expect(lanes(timeline).ticks.map((tick) => tick.label)).toEqual([
      "12:00",
      "16:00",
      "20:00",
      "11-11",
      "04:00",
      "08:00",
      "12:00",
    ]);
    // A step of days marks each midnight by its date.
    const longer = pictures().now - 40 * 86_400;
    const month = { ...timeline, created: longer };
    (month.stages[0] as Stage).from = longer;
    expect(lanes(month).ticks.map((tick) => tick.label)).toEqual([
      "10-03",
      "10-10",
      "10-17",
      "10-24",
      "10-31",
      "11-07",
    ]);
  });

  it("a week of short rounds prints no word over another", () => {
    const drawn = lanes(week());
    const reviewer = drawn.lanes.find((lane) => lane.key === "reviewer");
    for (const px of [600, 950, 1600]) {
      // No round's word drawn in its two minutes, and no verdict wider than its room.
      expect(reviewer?.segs.every((seg) => shown(seg.text, px) === "")).toBe(true);
      for (const seg of reviewer?.segs ?? []) {
        if (seg.after === null) continue;
        const [word, , room] = seg.after;
        const drawnWord = shown([word], px) !== "";
        const wide = [...word.text].length * CHAR_PX + GAP_PX;
        expect(!drawnWord || wide <= (room.room / 100) * px + 0.5, `${px}`).toBe(true);
      }
    }
    // Each stage's word only where it fits: two-minute reviews say nothing, six hours of work say
    // their length.
    const stage = drawn.lanes.find((lane) => lane.key === "stage");
    for (const seg of stage?.segs ?? []) {
      const words = shown(seg.text, 950);
      const wide = [...words].length * CHAR_PX + PAD_PX;
      expect(words === "" || wide <= (seg.width / 100) * 950 + 0.5).toBe(true);
    }
  });

  it("a task with nothing but its making lays out one stage and no other lane", () => {
    const now = 1_794_403_200;
    const timeline = aTimeline({
      slug: "tools/new",
      now,
      zone: [{ from: now - 120, seconds: 7200 }],
      created: now - 120,
      stages: [aStage({ status: "ready", from: now - 120 })],
    });
    expect(said(rows(timeline))).toEqual([["15:18", "Ready, unclaimed", "2 m", "now", false]]);
    const drawn = lanes(timeline);
    expect(drawn.lanes).toHaveLength(1);
    expect(drawn.gates).toEqual([]);
    expect(legendStates(drawn)).toEqual([]);
  });

  it("an empty timeline lays out nothing but its stage lane and its hour", () => {
    const empty = aTimeline({ now: 7200 * 10, created: 7200 * 10 });
    expect(rows(empty)).toEqual([]);
    expect(totals(empty)).toEqual(["0 m so far · 0 review rounds · 0 bounces", null]);
    const drawn = lanes(empty);
    expect(drawn.lanes).toEqual([
      { key: "stage", name: "Stage", who: "", rows: 1, rail: false, segs: [] },
    ]);
    // Nothing to draw before now: no hour marked, and now at its start (20:00 UTC, no zone).
    expect(drawn.ticks).toEqual([]);
    expect(drawn.now).toEqual({ left: 0, label: "now 20:00" });
    expect(stops(drawn)).toBe(0);
    // A closed one marks its hour, by its time as it is no midnight, and has no now.
    const closed = lanes({ ...empty, closed: empty.now });
    expect([closed.ticks, closed.now]).toEqual([[{ left: 0, label: "20:00" }], null]);
    // At midnight, by its date.
    const midnight = lanes(aTimeline({ now: 86_400, created: 86_400, closed: 86_400 }));
    expect(midnight.ticks).toEqual([{ left: 0, label: "01-02" }]);
  });

  it("an open task's lanes mark no hour so near now that its label would meet now's", () => {
    const timeline = pictures();
    const scale = new Scale(timeline);
    // An hour 4 % of the track before now is marked; nearer, it is not.
    expect(scale.ticks(true).at(-1)?.label).toBe("15:00");
    const later = new Scale({ ...timeline, now: timeline.now - 13 * 60 });
    expect(later.ticks(true).map((tick) => tick.label)).not.toContain("15:00");
    expect(later.ticks(false).map((tick) => tick.label)).toContain("15:00");
    expect([scale.at(scale.start - 60), scale.at(scale.end + 60)]).toEqual([0, 100]);
  });

  it("the executor lane names each executor where several held the task", () => {
    const timeline = {
      ...pictures(),
      claims: [
        { by: "quiet-wren", from: at(41), to: at(153) },
        { by: "teal-lynx", from: at(205), to: at(275) },
      ],
    };
    const executor = lanes(timeline).lanes.find((lane) => lane.key === "executor");
    expect(executor?.who).toBe("quiet-wren, teal-lynx");
    expect(executor?.segs.map((seg) => shown(seg.text, 1600))).toEqual([
      "quiet-wren 1 h 52 m",
      "teal-lynx 1 h 10 m",
    ]);
    expect(executor?.segs[1]?.title).toBe("teal-lynx held it 1 h 10 m, from 12:27 to 13:37");
  });

  it("a round nobody has taken waits in its lane, its words its own and its stop", () => {
    const timeline = aTimeline({
      now: 7200,
      created: 0,
      stages: [aStage({ status: "review", from: 0, round: 1 })],
      rounds: [{ round: 1, from: 0, verdict: "changes", to: 3600 }],
    });
    const reviewer = lanes(timeline).lanes.find((lane) => lane.key === "reviewer");
    expect(reviewer?.who).toBe("");
    expect(reviewer?.segs).toHaveLength(1);
    const [wait] = reviewer?.segs ?? [];
    expect([wait?.look, wait?.stop, shown(wait?.text ?? [], 1600)]).toEqual([
      "waiting",
      true,
      "round 1",
    ]);
    expect(wait?.title).toBe("round 1: waited 1 h for a reviewer, changes");
    expect(wait?.after?.[1]).toBe("dislike");
    // Taken at once, a round has no wait to draw: its reviewer's time alone.
    const taken = aTimeline({
      now: 7200,
      rounds: [{ round: 1, from: 0, taken: 0, to: 3600, by: "lucid-heron" }],
    });
    const segs = lanes(taken).lanes.find((lane) => lane.key === "reviewer")?.segs;
    expect(segs?.map((seg) => [seg.look, seg.after])).toEqual([["solid", null]]);
  });

  it("a gate's state that held no time is not drawn, and its title says its detail", () => {
    const timeline = aTimeline({
      now: 3600,
      gates: [
        {
          name: "check",
          states: [
            { state: "missing", from: 0, by: null, detail: "" },
            { state: "met", from: 0, by: null, detail: "" },
            { state: "held", from: 1800, by: "owner", detail: "Waits on the owner" },
          ],
        },
      ],
    });
    const [gate] = lanes(timeline).gates;
    expect(gate?.spans.map((span) => [span.state, span.title])).toEqual([
      ["met", "check met from 00:00"],
      ["held", "check held from 00:30: Waits on the owner"],
    ]);
  });

  it("the tones are the theme's tokens for each status, result, gate state and verdict", () => {
    expect(
      [
        "ready",
        "in_progress",
        "review",
        "done",
        "blocked",
        "cancelled",
        "concept",
        "design",
        "x",
      ].map(statusTone),
    ).toEqual([
      "auto",
      "primary",
      "question",
      "like",
      "dislike",
      "resolved",
      "decision",
      "decision",
      "muted-foreground",
    ]);
    expect((["green", "red", "queued", "running", "unknown"] as const).map(resultTone)).toEqual([
      "like",
      "dislike",
      "primary",
      "primary",
      "resolved",
    ]);
    expect((["met", "missing", "waived", "held", "unknown"] as const).map(gateTone)).toEqual([
      "like",
      "question",
      "resolved",
      "dislike",
      "muted-foreground",
    ]);
    expect((["approved", "changes", "other"] as const).map(verdictTone)).toEqual([
      "like",
      "dislike",
      "muted-foreground",
    ]);
    expect((["solid", "waiting", "running"] as const).map(lookClass)).toEqual([
      "tl-seg",
      "tl-seg tl-waiting",
      "tl-seg tl-running",
    ]);
  });

  it("a word reads with a capital and spaces, and a commit by its first seven bytes", () => {
    expect(capitalised("in_progress")).toBe("In progress");
    expect(capitalised("")).toBe("");
    expect(capitalised("ßeta")).toBe("SSeta");
    expect(shortCommit("0123456789abcdef")).toBe("0123456");
    expect(shortCommit("e19d3a0")).toBe("e19d3a0");
    expect(shortCommit("abc")).toBe("abc");
    // Seven bytes that end a character are cut there; seven that split one keep the whole.
    expect(shortCommit("abcdeéxyz")).toBe("abcdeé");
    expect(shortCommit("abcdefé")).toBe("abcdefé");
    expect(shortCommit("😀😀x")).toBe("😀😀x");
    expect(shortCommit("ab€€")).toBe("ab€€");
    expect(trimmed("\u0085 a b　")).toBe("a b");
  });

  it("the style sheet shows the list on a phone and the lanes from 900 px, in theme colours", () => {
    // As Rust's `split_once`: before the first switch, and all after it.
    const switches = CSS.indexOf("@media (min-width: 900px)");
    expect(switches, "one width switches the views").toBeGreaterThan(0);
    const phone = CSS.slice(0, switches);
    const wide = CSS.slice(switches + "@media (min-width: 900px)".length);
    expect(phone).toContain(".tl-lanes {\n  --name: 9rem;\n  --end: 5.5rem;\n  display: none;\n");
    // Wide, the list stays for assistive technology, out of sight but not display:none.
    expect(wide).toContain(
      "  .tl-list {\n    position: absolute;\n    width: 1px;\n    height: 1px;\n    overflow: hidden;\n    clip-path: inset(50%);\n",
    );
    expect(wide).not.toContain(".tl-list {\n    display: none;\n  }");
    expect(wide).toContain("  .tl-lanes {\n    display: block;\n  }");
    // A phone's text keeps clear of the + button's column.
    expect(phone).toContain(
      "@media (max-width: 899.98px) {\n  .tl-drawn {\n    padding-right: 40px;\n  }",
    );
    // A phone's clipped list is drawn from its end, the latest steps in view, and fades out at its
    // top, a row a tap focused growing upward into the totals' room; the lanes are never
    // clipped, and no fade is drawn wide.
    expect(phone).toContain("  .tl-clipped:has(.tl-row:focus) .tl-head {\n    display: none;\n  }");
    expect(phone).toContain(
      '  .tl-clipped .tl-body {\n    display: flex;\n    flex-direction: column;\n    justify-content: flex-end;\n  }\n  .tl-clipped .tl-list {\n    flex: none;\n  }\n  .tl-clipped .tl-body::after {\n    content: "";\n    position: absolute;\n    left: 0;\n    right: 0;\n    top: 0;\n    height: 28px;\n    background: linear-gradient(var(--card), transparent);\n    pointer-events: none;\n  }\n',
    );
    expect([...CSS.matchAll(/\.tl-clipped \.tl-body::after/g)]).toHaveLength(1);
    expect(wide).not.toContain("tl-clipped");
    expect(CSS).not.toContain("data-cut");
    // "Show all" is a phone's alone: wide the box holds every lane, and the style sheet hides the
    // button even where a draw at a phone's width offered it.
    expect(wide).toContain("  .tl-foot .tl-more {\n    display: none;\n  }");
    expect(CSS).not.toContain("tl-more-");
    // "Show all" sits on the legend's line, the legend wrapping beside it, and the lanes' detail
    // reserves no line of its own: it takes the legend's place while it shows.
    expect(phone).toContain(".tl-foot .tl-legend {\n  flex: 1 1 0;\n}");
    expect(CSS).not.toContain("min-height: 1.3em");
    expect(phone).toContain(".tl-detail {\n  flex: 1 1 0;\n");
    expect(CSS).not.toContain("flex-basis: 100%");
    expect(wide).toContain("  .tl-detail.tl-wide {\n    display: block;\n  }");
    expect(wide).toContain("  .tl-foot:has(.tl-detail) .tl-legend {\n    display: none;\n  }");
    expect(phone).not.toContain(":has(.tl-detail)");
    // A run's row a tap focused, and a stage's that carries a cost, says all of it under its bar,
    // across the row, and nothing while it has no focus: the foot keeps the legend on a phone. The
    // row with the focus is tinted, over no word of it, where an outline would cross its time and
    // its length.
    expect(phone).toContain(
      ".tl-row:focus {\n  outline: none;\n  border-radius: 6px;\n  background: color-mix(in srgb, var(--primary) 14%, transparent);\n}",
    );
    expect(phone).toContain(".tl-facts {\n  display: none;\n}");
    expect(phone).toContain(
      ".tl-row:focus .tl-facts {\n  grid-column: 1 / -1;\n  display: block;\n  margin-top: 4px;\n",
    );
    expect(wide).not.toContain("tl-facts");
    // The total's cost says its words under the totals on a phone while a tap gave it the focus,
    // and never wide, where they take the legend's place.
    expect(phone).toContain(".tl-head .tl-cost-said {\n  display: none;\n");
    expect(phone).toContain(
      "  .tl-totals:has(.tl-cost:focus) + .tl-cost-said {\n    display: block;\n  }\n" +
        // A box that clips its steps hides them while the words are said, and grows with them.
        "  .tl-clipped:has(.tl-cost:focus) .tl-list {\n    display: none;\n  }",
    );
    expect(wide).not.toContain("tl-cost");
    // The rounds and bounces follow the cost on the first line wide, and are a phone's second.
    expect(wide).toContain(
      "  .tl-machine.tl-wide,\n  .tl-rest.tl-wide {\n    display: inline;\n  }",
    );
    // The builds' lane and the tests' are drawn alike, a row each for runs that overlap, each run
    // a 14 px bar, a 1 px edge of the card's colour keeping two apart.
    expect(phone).toContain(
      '.tl-lane:is([data-lane="builds"], [data-lane="tests"]) .tl-track {\n  height: calc(var(--rows) * 20px + 16px);\n}',
    );
    expect(phone).toContain(
      '.tl-lane:is([data-lane="builds"], [data-lane="tests"]) .tl-seg {\n  top: calc(var(--row) * 20px + 11px);\n  height: 14px;\n  padding: 0;\n  border: 0;\n  border-radius: 2px;\n  box-shadow: 0 0 0 1px var(--card);\n',
    );
    expect(CSS).not.toContain('data-lane="runs"');
    expect(CSS).not.toContain("data-dense");
    expect(CSS).not.toContain("tl-under");
    // The name column is var(--name) wide, padding and all, so the hours' lines, drawn from
    // var(--name), start where the tracks do.
    expect(phone).toContain(".tl-name {\n  flex: 0 0 var(--name);\n  box-sizing: border-box;\n");
    expect(phone).toContain(".tl-hours {\n  position: absolute;\n  left: var(--name);\n");
    // A message is a tick on its sender's line, in a press area of 24 px.
    expect(CSS).toContain(
      '.tl-message-mark::before {\n  content: "";\n  display: block;\n  width: 2px;\n  height: 8px;\n',
    );
    expect(CSS).toContain(
      ".tl-message-mark {\n  position: absolute;\n  z-index: 2;\n  top: 24px;\n  width: 24px;\n  height: 24px;\n  min-height: 0;\n",
    );
    // The gate states' marks are in the legend wide only, beside the lanes that draw them: a rule
    // as specific as the legend's items' own hides them on a phone.
    expect(phone).toContain(".tl-legend .tl-wide {\n  display: none;\n}");
    expect(wide).toContain("  .tl-legend .tl-wide {\n    display: inline-flex;\n  }");
    // A light page draws the segments' words in its own colour.
    expect(CSS).toContain(
      "@media not (prefers-color-scheme: dark) {\n  .tl-seg {\n    color: var(--foreground);\n  }\n}",
    );
    // Each width a word is weighed at shows it from there, and its shorter one below.
    for (const width of WIDTHS) {
      expect(CSS).toContain(
        `@container (max-width: ${width - 1}.5px) {\n  .tl-from-${width} {\n    display: none;\n  }\n}`,
      );
      expect(CSS).toContain(
        `@container (min-width: ${width}px) {\n  .tl-to-${width} {\n    display: none;\n  }\n}`,
      );
    }
    // Every gate state has a pattern of its own besides its colour.
    for (const [state, pattern] of [
      ["missing", "dotted"],
      ["waived", "dashed"],
      ["held", "double"],
      ["unknown", "dotted"],
    ]) {
      expect(CSS.includes(`.tl-g-${state}::before {`) && CSS.includes(pattern as string)).toBe(
        true,
      );
    }
    // Every colour is a theme token (mixed in sRGB at most), so an imported theme colours the
    // timeline too.
    for (const written of ["#", "rgb(", "rgba(", "hsl("]) expect(CSS).not.toContain(written);
  });

  it("a failed read says its reason's first line as a sentence, never a stack", () => {
    // Each says it is tried again, which the page's store does by itself.
    expect(TIMELINE_EVERY).toBe(60);
    const again = (said: string) => `${said} Trying again…`;
    expect(failed("Failed to fetch")).toBe(
      again("The timeline could not be read: Failed to fetch."),
    );
    expect(failed(" Board snapshot unavailable. ")).toBe(
      again("The timeline could not be read: Board snapshot unavailable."),
    );
    expect(
      failed("TypeError: Failed to fetch\n    at window.fetch (shell.js:1:2)\n    at 0xe43e"),
    ).toBe(again("The timeline could not be read: TypeError: Failed to fetch."));
    expect(failed("")).toBe(again("The timeline could not be read."));
    expect(failed(" . ")).toBe(again("The timeline could not be read."));
    // A line's carriage return and every trailing point go; a point between words stays.
    expect(failed("gone.\r\nmore")).toBe(again("The timeline could not be read: gone."));
    expect(failed("a. b...")).toBe(again("The timeline could not be read: a. b."));
  });

  it("the next read is the timeline's own while open, none once closed or after a failure", () => {
    const read = (timeline: TaskTimeline) => nextRead({ timeline });
    expect(read({ ...pictures(), every: 45 })).toBe(45);
    expect(read({ ...pictures(), every: 0 })).toBe(TIMELINE_EVERY);
    const unsaid = pictures();
    delete unsaid.every;
    expect(read(unsaid)).toBe(TIMELINE_EVERY);
    expect(read({ ...pictures(), closed: pictures().now })).toBeNull();
    // A failed read is the page's store's to try again.
    expect(nextRead({ error: "Failed to fetch" })).toBeNull();
    expect(nextRead(null)).toBeNull();
    // A browser's timer waits an `i32` of ms at most.
    expect(LONGEST_WAIT).toBe(2 ** 31 - 1);
  });

  it("show all counts the steps of a phone's list", () => {
    const steps = rows(pictures()).length;
    expect(showAll(steps)).toBe(`Show all ${steps} steps`);
    expect(showAll(1)).toBe("Show all 1 step");
  });
});

/**
 * A week-long task of 24 review rounds, two minutes each, each sent back: the case the review
 * found the verdicts printed over each other.
 */
function week(): TaskTimeline {
  const now = pictures().now;
  const made = now - 7 * 86_400;
  const stages: Stage[] = [aStage({ status: "ready", from: made, to: made + 3600 })];
  const rounds: TaskTimeline["rounds"] = [];
  let from = made + 3600;
  for (let number = 1; number <= 24; number += 1) {
    const review = from + 6 * 3600;
    stages.push(aStage({ status: "in_progress", from, to: review }));
    stages.push(aStage({ status: "review", from: review, to: review + 120, round: number }));
    rounds.push({
      round: number,
      from: review,
      taken: review + 30,
      to: review + 120,
      by: "lucid-heron",
      verdict: "changes",
    });
    from = review + 120;
  }
  stages.push(aStage({ status: "in_progress", from }));
  return {
    ...pictures(),
    created: made,
    first_claimed: made + 3600,
    bounces: 24,
    stages,
    claims: [{ by: "quiet-wren", from: made + 3600, to: null }],
    rounds,
    runs: [],
    gates: [
      {
        name: "review",
        states: [{ state: "missing", from: made + 7 * 3600, by: null, detail: "Changes asked" }],
      },
    ],
  };
}

describe("a custom workflow's places", () => {
  const HOUR = 3600;
  const NAMES: Record<string, string> = {
    build: "Build",
    review: "Review",
    verify: "Verify",
    working: "Working",
    awaiting: "Awaiting review",
    blocked: "Blocked",
  };
  const STATUS: Record<string, string> = {
    working: "in_progress",
    awaiting: "review",
    blocked: "blocked",
  };
  /** A move to `phase`/`state` `hours` in, by `by` and `edge` (none: an activation's). */
  const moved = (
    hours: number,
    phase: string,
    state: string,
    by: string,
    edge?: string,
  ): PlaceSpan => ({
    phase,
    phase_label: NAMES[phase] ?? phase,
    state,
    state_label: NAMES[state] ?? state,
    status: STATUS[state] ?? "in_progress",
    from: hours * HOUR,
    by,
    ...(edge === undefined ? {} : { edge }),
  });
  /** Two review stages with a rework between them and a block in Build: open, nine hours in. */
  const PLACES = [
    moved(0, "build", "working", "owner"),
    moved(2, "review", "awaiting", "misty-otter-30", "request_review"),
    moved(3, "build", "working", "jade-heron-2", "changes"),
    moved(4, "build", "blocked", "misty-otter-30", "block"),
    moved(5, "build", "working", "owner", "resume"),
    moved(6, "review", "awaiting", "misty-otter-30", "request_review"),
    moved(7, "verify", "awaiting", "jade-heron-2", "approve"),
  ];
  const held = (fields: Partial<TaskTimeline> = {}) =>
    aTimeline({ now: 9 * HOUR, places: PLACES, ...fields });
  const spans = (key: "phase" | "state" | "place", timeline = held()) =>
    stretches(timeline, key).map((one) => [one.label, one.from / HOUR, one.to / HOUR]);

  it("are stretches of one phase, one state or one place, the last until now or the close", () => {
    expect(spans("phase")).toEqual([
      ["Build", 0, 2],
      ["Review", 2, 3],
      ["Build", 3, 6],
      ["Review", 6, 7],
      ["Verify", 7, 9],
    ]);
    // A state the task kept across a phase's change is one stretch.
    expect(spans("state")).toEqual([
      ["Working", 0, 2],
      ["Awaiting review", 2, 3],
      ["Working", 3, 4],
      ["Blocked", 4, 5],
      ["Working", 5, 6],
      ["Awaiting review", 6, 9],
    ]);
    expect(spans("place").map(([label]) => label)).toEqual([
      "Build · Working",
      "Review · Awaiting review",
      "Build · Working",
      "Build · Blocked",
      "Build · Working",
      "Review · Awaiting review",
      "Verify · Awaiting review",
    ]);
    expect(spans("phase", held({ closed: 8 * HOUR })).at(-1)).toEqual(["Verify", 7, 8]);
    expect(stretches(aTimeline(), "phase")).toEqual([]);
  });

  it("are told by phase and by state, a phase the task came back to told whole", () => {
    expect(placeTotals(held(), "phase")).toBe("Build 5 h · Review 2 h · Verify 2 h");
    expect(placeTotals(held(), "state")).toBe("Working 4 h · Awaiting review 4 h · Blocked 1 h");
    expect(placeTotals(aTimeline(), "phase")).toBeNull();
    // The machine's time on runs is told apart, and never in a place's.
    const run = aRun("test", "test-rs", null, [HOUR, HOUR + 60, 2 * HOUR]);
    expect(totals(held({ runs: [run] }))[1]).toBe("59 m, queued 1 m");
    expect(placeTotals(held({ runs: [run] }), "phase")).toBe(placeTotals(held(), "phase"));
  });

  it("are rows of a phone's list, each its bar's share of the longest, the last running", () => {
    const listed = rows(held());
    expect(said(listed)).toEqual([
      ["00:00", "Build · Working", "2 h", "", true],
      ["02:00", "Review · Awaiting review", "1 h", "", true],
      ["03:00", "Build · Working", "1 h", "", true],
      ["04:00", "Build · Blocked", "1 h", "", true],
      ["05:00", "Build · Working", "1 h", "", true],
      ["06:00", "Review · Awaiting review", "1 h", "", true],
      ["07:00", "Verify · Awaiting review", "2 h", "", true],
    ]);
    expect(listed.map((row) => row.tone)).toEqual([
      "primary",
      "question",
      "primary",
      "dislike",
      "primary",
      "question",
      "question",
    ]);
    expect(listed.map((row) => row.bar?.work)).toEqual([
      BAR_SPAN,
      BAR_SPAN / 2,
      BAR_SPAN / 2,
      BAR_SPAN / 2,
      BAR_SPAN / 2,
      BAR_SPAN / 2,
      BAR_SPAN,
    ]);
    expect(listed.map((row) => row.bar?.running)).toEqual([
      false,
      false,
      false,
      false,
      false,
      false,
      true,
    ]);
    expect(listed[1]?.title).toBe(
      "Review · Awaiting review 1 h, from 02:00 to 03:00, moved there by misty-otter-30 (request review)",
    );
    expect(listed[0]?.title).toBe("Build · Working 2 h, from 00:00 to 02:00, moved there by owner");
    const closed = held({ closed: 8 * HOUR });
    const [last] = stretches(closed, "place").slice(-1);
    expect(last && placeRow(last, closed, (seconds) => seconds).bar?.running).toBe(false);
  });

  it("are a phase lane and a state lane after the stages', and none for a task no workflow held", () => {
    const drawn = lanes(held());
    expect(drawn.lanes.map((lane) => [lane.key, lane.name, lane.segs.length])).toEqual([
      ["stage", "Stage", 0],
      ["phase", "Phase", 5],
      ["state", "State", 6],
    ]);
    const blocked = drawn.lanes[2]?.segs[3];
    expect(blocked?.tone).toBe("dislike");
    expect(blocked?.title).toBe(
      "Blocked 1 h, from 04:00 to 05:00, moved there by misty-otter-30 (block)",
    );
    expect(stops(drawn)).toBe(11);
    expect(lanes(aTimeline({ now: HOUR })).lanes.map((lane) => lane.key)).toEqual(["stage"]);
    // A stretch with no time drawn has no segment.
    const instant = held({
      places: [moved(0, "build", "working", "owner"), moved(0, "review", "awaiting", "owner")],
    });
    expect(lanes(instant).lanes[1]?.segs.map((seg) => seg.title.split(" ")[0])).toEqual(["Review"]);
  });

  it("are read as the server answers them, and left out, never the timeline, where unreadable", () => {
    const answer = (places: unknown) => ({ ...aTimeline({ now: 1 }), places });
    expect(readTimeline(answer(PLACES))?.places).toEqual(
      PLACES.map((place) => ({ edge: null, ...place })),
    );
    const unreadable = readTimeline(answer([{ ...PLACES[0], from: "noon" }]));
    expect(unreadable).not.toBeNull();
    expect(unreadable && "places" in unreadable).toBe(false);
    expect(readTimeline(answer("none"))?.places).toBeUndefined();
    expect(readTimeline(aTimeline({ now: 1 }))?.places).toBeUndefined();
  });
});

describe("each stage's cost after its time", () => {
  /** A row of the pictures' task's tokens: `amount` dollars in `role` and `round`, its calls
   * from `first` to `last` minutes after the task was made. */
  function aRow(role: string, round: number, first: number, last: number, amount: number) {
    const row: TokenRow = {
      handle: `${role}-1`,
      slug: pictures().slug,
      role,
      model: "claude-opus-5-5",
      speed: "standard",
      round,
      tokens: {
        input: 1000,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_read: 0,
        output: 0,
        calls: 7,
      },
      cost: {
        dollars: amount,
        input: amount,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_read: 0,
        output: 0,
        unpriced: [],
        prices: "2026-11-16",
      },
      first: at(first),
      last: at(last),
    };
    return row;
  }

  /** The pictures' task's tokens on its stages: its first work and what its claimer spent while
   * it was reserved, round 1's reviewer, the fixes after it, and round 2's reviewer. */
  function costs(timeline = pictures()): Costs {
    const rows = [
      aRow("executor", 1, 42, 150, 30.43),
      aRow("reserved", 1, 40, 40, 0.32),
      aRow("reviewer", 2, 155, 250, 3.78),
      aRow("executor", 2, 153, 270, 7.03),
      aRow("reviewer", 3, 290, 300, 0.64),
    ];
    return { stages: carried(timeline.stages, rows, timeline.slug), prices: "2026-11-16" };
  }

  it("tries three widths, longest first: its word, time and cost; its time and cost; its time", () => {
    const all = ["in progress 3 h 14 m $30.75", "3 h 14 m $30.75", "3 h 14 m"];
    // 27, 15 and 8 characters: 174, 102 and 60 px with their padding, of 12 % of the track.
    const fits = fittedFirst(all, 12);
    expect(fits).toEqual([
      { text: all[0], from: 1450, to: null },
      { text: all[1], from: 850, to: 1450 },
      { text: all[2], from: 500, to: 850 },
    ]);
    expect(shown(fits, 1600)).toBe(all[0]);
    expect(shown(fits, 1400)).toBe(all[1]);
    expect(shown(fits, 800)).toBe(all[2]);
    expect(shown(fits, 450)).toBe("");
    // Room for the longest at every width: it alone.
    expect(fittedFirst(all, 60)).toEqual([{ text: all[0], from: null, to: null }]);
    // Room for its time and not its cost: the time, as with no cost.
    expect(fittedFirst(all, 4)).toEqual([{ text: all[2], from: 1500, to: null }]);
    expect(fittedFirst(all, 4)).toEqual(fitted("in progress 3 h 14 m", "3 h 14 m", 4));
    // No room for any, and none to try.
    expect(fittedFirst(all, 1)).toEqual([]);
    expect(fittedFirst([], 50)).toEqual([]);
    // A text said twice, as a stage with no cost says its time, is tried once.
    expect(fittedFirst(["ready 41 m", "41 m", "41 m"], 10)).toEqual(
      fitted("ready 41 m", "41 m", 10),
    );
  });

  it("says each stretch's cost after its time in the stage lane, where it fits", () => {
    const stage = (timeline: TaskTimeline, with_: Costs | undefined) =>
      lanes(timeline, with_).lanes.find((lane) => lane.key === "stage") as Lane;
    const texts = (px: number) => stage(pictures(), costs()).segs.map((seg) => shown(seg.text, px));
    expect(texts(1600)).toEqual([
      "ready 41 m",
      "in progress 1 h 52 m $30.75",
      "review 52 m $3.78",
      "in progress 1 h 10 m $7.03",
      "review 28 m $0.64",
      "waiting to land 1 h 15 m",
    ]);
    // The second review's 28 m are 70 px of a 950 px track: room for its time (36 px) and not for
    // its time and cost (72 px), so it shows its time, as with no cost.
    expect(texts(950)).toEqual([
      "ready 41 m",
      "in progress 1 h 52 m $30.75",
      "review 52 m $3.78",
      "in progress 1 h 10 m $7.03",
      "28 m",
      "waiting to land 1 h 15 m",
    ]);
    // Narrower, a stretch drops its word before its cost.
    expect(texts(500)).toEqual(["41 m", "1 h 52 m $30.75", "52 m", "1 h 10 m", "28 m", "1 h 15 m"]);
    // Its words always say it: whose calls, when they ran, how many, and that it is an estimate.
    const titles = stage(pictures(), costs()).segs.map((seg) => seg.title);
    expect(titles[1]).toBe(
      "in progress 1 h 52 m $30.75, from 09:43 to 11:35: executor $30.43, 09:44–11:32, in 7 calls; " +
        "reserved $0.32, at 09:42, in 7 calls; estimated at API list prices read 2026-11-16, not money spent",
    );
    expect(titles[4]).toBe(
      "review 28 m $0.64, from 13:37 to 14:05: reviewer $0.64, 13:52–14:02, in 7 calls; " +
        "estimated at API list prices read 2026-11-16, not money spent",
    );
    // A stretch that carries nothing, and every stretch of a task with no tokens, is as it was.
    expect(titles[0]).toBe("ready 41 m, from 09:02 to 09:43");
    expect(titles[5]).toBe(stage(pictures(), undefined).segs[5]?.title);
    const none: Costs = { stages: carried(pictures().stages, [], ""), prices: "2026-11-16" };
    expect(lanes(pictures(), none)).toEqual(lanes(pictures()));
    expect(rows(pictures(), none)).toEqual(rows(pictures()));
  });

  it("ends each stage's row on a phone with its time and its cost, and says it in its words", () => {
    const listed = rows(pictures(), costs()).filter((row) => !row.sub);
    expect(listed.map((row) => [row.took, row.cost])).toEqual([
      ["41 m", undefined],
      ["1 h 52 m", "$30.75"],
      ["52 m", "$3.78"],
      ["1 h 10 m", "$7.03"],
      ["28 m", "$0.64"],
      ["1 h 15 m", undefined],
    ]);
    expect(listed[2]?.title).toBe(
      "Review, round 1 · lucid-heron: from 11:35 to 12:27, moved there by quiet-wren: " +
        "reviewer $3.78, 11:37–13:12, in 7 calls; " +
        "estimated at API list prices read 2026-11-16, not money spent",
    );
    // The runs, the gate changes and the rows' order are as with no cost.
    const plain = rows(pictures());
    expect(rows(pictures(), costs()).map((row) => row.label)).toEqual(
      plain.map((row) => row.label),
    );
  });

  it("names the day of a call made another day than its stretch began", () => {
    const timeline = aTimeline({
      slug: "a/b",
      now: 3 * 86400,
      stages: [aStage({ status: "in_progress", from: 3600 })],
    });
    const row = { ...aRow("executor", 1, 0, 0, 2), slug: "a/b", first: 7200, last: 86400 + 7200 };
    const with_: Costs = { stages: carried(timeline.stages, [row], "a/b"), prices: "2026-11-16" };
    expect(lanes(timeline, with_).lanes[0]?.segs[0]?.title).toContain(
      ": executor $2.00, 02:00–01-02 02:00, in 7 calls; estimated",
    );
  });

  it("keeps the row of a stage left within the minute when it carries a cost", () => {
    const stages = [
      aStage({ status: "in_progress", from: 0, to: 3600 }),
      aStage({ status: "review", from: 3600, to: 3620, round: 1 }),
      aStage({ status: "done", from: 3620 }),
    ];
    const timeline = aTimeline({ slug: "a/b", now: 7200, closed: 3620, stages });
    expect(rows(timeline).map((row) => row.label)).toEqual(["In progress", "Done"]);
    const row = { ...aRow("reviewer", 2, 0, 0, 0.21), slug: "a/b", first: 3601, last: 3619 };
    const with_: Costs = { stages: carried(stages, [row], "a/b"), prices: "2026-11-16" };
    expect(rows(timeline, with_).map((row) => [row.label, row.took, row.cost])).toEqual([
      ["In progress", "1 h", undefined],
      ["Review, round 1", "0 m", "$0.21"],
      ["Done", null, undefined],
    ]);
  });

  it("parts the first total where the cost goes, the two the whole of it", () => {
    expect(totalParts(pictures())).toEqual(["6 h 18 m so far", " · 2 review rounds · 1 bounce"]);
    expect(totalParts(pictures()).join("")).toBe(totals(pictures())[0]);
    expect(totalParts(aTimeline({ now: 60, closed: 60 }))[0]).toBe("1 m in all");
  });
});

describe("a part of the hours shown", () => {
  /** The pictures' hours start at 09:00 on their clock: `minutes` after it. */
  const START = 1_794_380_400;
  const nine = (minutes: number) => START + minutes * 60;
  /** 12:00 to 13:00 on the pictures' clock. */
  const NOON = { from: nine(180), to: nine(240) };

  it("places every part in the view, clamped to it, and says which are outside it", () => {
    const scale = new Scale(pictures(), NOON);
    expect([scale.zoomed, scale.from, scale.to, scale.shown]).toEqual([
      true,
      nine(180),
      nine(240),
      3600,
    ]);
    // The whole hours stay the lanes' own.
    expect([scale.start, scale.span]).toEqual([START, 22_800]);
    expect([scale.at(nine(195)), scale.at(nine(170)), scale.at(nine(300))]).toEqual([25, 0, 100]);
    expect(scale.width(nine(170), nine(210))).toBe(50);
    expect(scale.place(nine(170), nine(210))).toEqual({
      left: 0,
      width: 50,
      from: nine(170),
      to: nine(210),
      out: false,
    });
    expect(scale.place(nine(100), nine(120)).out).toBe(true);
    expect(scale.place(nine(250), nine(260)).out).toBe(true);
    // An instant at either end is in it.
    expect([scale.shows(nine(240), nine(240)), scale.shows(nine(170), nine(180))]).toEqual([
      true,
      true,
    ]);
    // Whether a part is drawn at all is the whole hours', whatever the view.
    expect(scale.spans(nine(100), nine(120))).toBe(true);
    expect(scale.spans(nine(100), nine(100))).toBe(false);
    expect(scale.spans(nine(-60), nine(-30))).toBe(false);
    // No view, or one with no length, is the whole hours.
    for (const whole of [
      new Scale(pictures()),
      new Scale(pictures(), { from: nine(9), to: nine(9) }),
    ]) {
      expect([whole.zoomed, whole.from, whole.to, whole.shown]).toEqual([
        false,
        START,
        whole.end,
        22_800,
      ]);
    }
  });

  it("marks minutes when zoomed, from a minute up, each on its own local minute", () => {
    const labels = (view: { from: number; to: number }) =>
      new Scale(pictures(), view).ticks(false).map((tick) => tick.label);
    // 11:05:30 to 12:18:30: every ten minutes, from the first whole ten after the start.
    const hour = { from: nine(125) + 30, to: nine(198) + 30 };
    expect(labels(hour)).toEqual(["11:10", "11:20", "11:30", "11:40", "11:50", "12:00", "12:10"]);
    const first = new Scale(pictures(), hour).ticks(false)[0];
    expect(first?.left).toBeCloseTo(((nine(130) - hour.from) / (hour.to - hour.from)) * 100, 10);
    // Five minutes: every minute; three hours: every half hour; a day's hours: by the hour up.
    expect(labels({ from: nine(60), to: nine(65) })).toEqual([
      "10:00",
      "10:01",
      "10:02",
      "10:03",
      "10:04",
      "10:05",
    ]);
    expect(labels({ from: nine(0), to: nine(180) })).toEqual([
      "09:00",
      "09:30",
      "10:00",
      "10:30",
      "11:00",
      "11:30",
      "12:00",
    ]);
    // The whole hours keep their steps from the hour up.
    expect(new Scale(pictures()).ticks(false).map((tick) => tick.label)).toEqual([
      "09:00",
      "10:00",
      "11:00",
      "12:00",
      "13:00",
      "14:00",
      "15:00",
    ]);
  });

  it("keeps the hours near a view's end where now is past it, and none near now in it", () => {
    // 12:00 to 13:00, now 15:20: 13:00 is at the view's end, far from now.
    expect(new Scale(pictures(), NOON).ticks(true).at(-1)?.label).toBe("13:00");
    // 14:20 to 15:20, now at its end: no mark within 4 % before now.
    const latest = new Scale(pictures(), { from: nine(320), to: nine(380) });
    expect(latest.ticks(true).at(-1)?.label).toBe("15:10");
  });

  it("lays out the same parts at any zoom, each placed and its words fitted in the view", () => {
    const timeline = pictures();
    const whole = lanes(timeline);
    const noon = lanes(timeline, undefined, NOON);
    // The keyboard's stops and every lane's parts are the whole task's, whatever the zoom.
    expect(stops(noon)).toBe(stops(whole));
    expect(noon.lanes.map((lane) => [lane.key, lane.segs.length, lane.rows])).toEqual(
      whole.lanes.map((lane) => [lane.key, lane.segs.length, lane.rows]),
    );
    expect(noon.gates.map((gate) => gate.spans.length)).toEqual(
      whole.gates.map((gate) => gate.spans.length),
    );
    // Outside the view: ready, the first stretch of work and round 2; in it: round 1's review and
    // the second stretch of work.
    const stage = noon.lanes[0]?.segs ?? [];
    expect(stage.map((seg) => seg.out)).toEqual([true, true, false, false, true, true]);
    const review = stage[2];
    expect([review?.left, review?.width]).toEqual([0, 45]);
    // The second stretch of work, from 12:27 past the view's end: its words fitted to what shows.
    const work = stage[3];
    expect([work?.left, work?.width]).toEqual([45, 55]);
    expect(work?.text[0]).toEqual({ text: "in progress 1 h 10 m", from: null, to: null });
    expect(whole.lanes[0]?.segs[3]?.text[0]?.text).toBe("in progress 1 h 10 m");
    expect(whole.lanes[0]?.segs[3]?.text[0]?.from).not.toBeNull();
    // A verdict is said after its round's end where that end shows; round 2's ends outside.
    const reviewer = noon.lanes.find((lane) => lane.key === "reviewer")?.segs ?? [];
    expect(reviewer.map((seg) => seg.after?.[0].text ?? null)).toEqual([
      null,
      "changes",
      null,
      null,
    ]);
    expect(reviewer.map((seg) => seg.title)).toEqual(
      whole.lanes.find((lane) => lane.key === "reviewer")?.segs.map((seg) => seg.title),
    );
    // Now (15:20) is past the view: no line at it; nor any tick near the end left out for it.
    expect(noon.now).toBeNull();
    expect(lanes(timeline, undefined, { from: nine(330), to: nine(390) }).now?.label).toBe(
      "now 15:20",
    );
    const certificate = noon.gates.find((gate) => gate.name === "certificate")?.spans ?? [];
    expect(certificate.map((span) => span.out)).toEqual([true, true, true]);
  });

  it("hatches only what of the time no run is kept of shows", () => {
    const timeline = { ...pictures(), runs_from: nine(150) };
    expect(lanes(timeline).notKept?.width).toBeGreaterThan(0);
    const partly = lanes(timeline, undefined, { from: nine(120), to: nine(180) }).notKept;
    expect([partly?.left, partly?.width]).toEqual([0, 50]);
    expect(lanes(timeline, undefined, NOON).notKept).toBeNull();
  });
});
