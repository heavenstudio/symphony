// The Reports page, `/reports`: how the project's work flows, how many agents work it and in which
// roles, its decisions, its code and its board's health, by the hour, the day or the week, so a
// change to a gate or a limit shows in the speed and the quality of what lands. It draws the one
// read REPORTS_PATH answers (crates/symphony-api's `report_page`), summed by the server from the
// project's day files: four figures over the window, then the charts on one time axis, two columns
// on a wide screen. A period a source has no record of is a gap, hatched, never a zero.

import { type CSSProperties, useEffect, useState } from "react";
import { Button } from "./button";
import { type Item, keepLater, readKept, visitLater } from "./cache";
import {
  type Axis,
  ChartCard,
  type ChartData,
  format,
  GhostChart,
  type Mark,
  seriesKey,
  short,
  sum,
  type Unit,
} from "./chart";
import { PageNotices } from "./live";
import { href } from "./project";
import { getIf, said } from "./request";
import { field } from "./shared";
import {
  amount,
  costSaid,
  dollars,
  estimate,
  isCost,
  isTokens,
  total,
  unpricedSaid,
} from "./spend";
import { Backoff } from "./state";
import { load, save } from "./storage";
// biome-ignore lint/style/noRestrictedImports: no-timed-waits baseline: reports reads again every EVERY_MS with no feed of its own, where a feed event of a roll-up is the wait
import { sleep } from "./timing";
import { failed, ready } from "./trouble";
import { NearViewport } from "./viewport";
import {
  type AgentCost,
  type Grain,
  type LandedSpend,
  REPORTS_PATH,
  type ReportMark,
  type Spend,
} from "./wire";

const HOUR = 3_600;
const DAY = 86_400;

/** The grain `word` names (`hour`, `day`, `week`); none for another word. Grain::parse's. */
export function parseGrain(word: string): Grain | null {
  return word === "hour" || word === "day" || word === "week" ? word : null;
}

/**
 * The days a window reaches back when none is asked: two by the hour, fourteen by the day, twelve
 * weeks by the week. Grain::default_days's.
 */
export function defaultDays(grain: Grain): number {
  return { hour: 2, day: 14, week: 84 }[grain];
}

/** The windows the page offers for each grain, in days. */
export function ranges(grain: Grain): number[] {
  return { hour: [1, 2, 7], day: [7, 14, 30, 90], week: [28, 84] }[grain];
}

/** A window's name on its button: `24 hours`, `14 days`, `12 weeks`. */
export function rangeName(grain: Grain, days: number): string {
  if (grain === "hour" && days === 1) return "24 hours";
  if (grain === "week") return `${Math.floor(days / 7)} weeks`;
  return `${days} days`;
}

/** The grain's name on its button and in the page's words. */
function grainName(grain: Grain): string {
  return { hour: "Hours", day: "Days", week: "Weeks" }[grain];
}

/**
 * One period of the answer (`ReportBucket` on the wire), read by path where the page needs a value,
 * as the server writes it: a source with no record of the period is `null`, and a count by status
 * leaves out its zeros.
 */
export class Bucket {
  constructor(readonly raw: unknown) {}

  /**
   * The number at `path` (`flow.held.ready`): none where a member on the way is missing or `null`,
   * a source with no record of the period, and zero where only the last is missing, as a count by
   * status leaves out its zeros.
   */
  number(path: string): number | null {
    return this.at(path, true);
  }

  /**
   * The number at `path` (`flow.done`), none where any member on the way is missing or `null`, the
   * last too: a figure the answer always writes, `null` where it has none, so a missing one is an
   * answer from a server that does not write it, no record, never a zero.
   */
  written(path: string): number | null {
    return this.at(path, false);
  }

  private at(path: string, zeroIfLeftOut: boolean): number | null {
    let value = this.raw;
    const keys = path.split(".");
    for (const [at, key] of keys.entries()) {
      const found = field(value, key);
      if (found !== undefined && found !== null) {
        value = found;
      } else if (
        found === undefined &&
        zeroIfLeftOut &&
        at === keys.length - 1 &&
        typeof value === "object" &&
        value !== null
      ) {
        return 0;
      } else {
        return null;
      }
    }
    return typeof value === "number" ? value : null;
  }

  /** Whether the source `key` (`flow`, `agents`, `health`…) recorded the period. */
  has(key: string): boolean {
    const found = field(this.raw, key);
    return found !== undefined && found !== null;
  }

  /** Whether any source recorded the period: the roll-up, the agents, the board's health or git. */
  recorded(): boolean {
    return ["flow", "agents", "health", "git"].some((source) => this.has(source));
  }

  /** The items of the list at `key` of the member `source`, each read the same way. */
  list(source: string, key: string): Bucket[] {
    const found = field(field(this.raw, source), key);
    return Array.isArray(found) ? found.map((item) => new Bucket(item)) : [];
  }

  /** The text at `key`, empty where there is none. */
  text(key: string): string {
    const found = field(this.raw, key);
    return typeof found === "string" ? found : "";
  }

  /** Its first second, and its end. */
  span(): [number, number] {
    const start = this.number("start") ?? 0;
    return [start, this.number("end") ?? start];
  }
}

/**
 * The answer of REPORTS_PATH as the page reads it: the window and what it says of the periods
 * (`ReportsPage` on the wire), each period kept as the browser parsed it.
 */
export interface Page {
  grain: Grain;
  days: number;
  from: number;
  to: number;
  buckets: Bucket[];
  first: number | null;
  marks: ReportMark[];
  unread: string[];
  /** What the tasks landed in the window took; none where none landed. */
  landed: LandedSpend | null;
  /** The day the API list prices every dollar is estimated at were read; empty from a server that
   * prices nothing. */
  prices: string;
}

/** The list `value` holds, none where it is missing; null where it is something else. */
function listed(value: unknown): unknown[] | null {
  if (value === undefined) return [];
  return Array.isArray(value) ? value : null;
}

/**
 * A change's mark as the server writes it: what it changed is a list of names, or left out by a
 * server built before the instructions were marked, whose marks are settings changes.
 */
function isMark(value: unknown): value is Omit<ReportMark, "changed"> {
  const changed = listed(field(value, "changed"));
  return (
    typeof field(value, "at") === "number" &&
    typeof field(value, "commit") === "string" &&
    typeof field(value, "subject") === "string" &&
    changed?.every((name) => typeof name === "string") === true
  );
}

/** What a mark that names nothing changed: the settings, all a mark said before it named any. */
const SETTINGS = ["settings"];

const isCount = (value: unknown) => Number.isSafeInteger(value) && (value as number) >= 0;
const isSpend = (value: unknown): value is Spend =>
  isTokens(field(value, "tokens")) && isCost(field(value, "cost"));

/**
 * The landed tasks' spend as the server writes it, or none where it is not: absent or `null` is
 * none landed, as from a server that wrote none.
 */
function readLanded(value: unknown): LandedSpend | null | undefined {
  if (value === undefined || value === null) return null;
  const median = field(value, "median") ?? null;
  const dearest = listed(field(value, "dearest"));
  return ["tasks", "without", "again"].every((count) => isCount(field(value, count) ?? 0)) &&
    (median === null || typeof median === "number") &&
    dearest?.every(
      (task) =>
        typeof field(task, "slug") === "string" &&
        typeof field(task, "at") === "number" &&
        isSpend(task),
    ) === true &&
    isSpend(field(value, "first_round")) &&
    isSpend(field(value, "later_rounds"))
    ? ({ ...(value as LandedSpend), median, dearest } as LandedSpend)
    : undefined;
}

/**
 * The reports as this client reads them, or none for a value without what the page draws: a grain,
 * its days, the window's ends, the periods (each read by path as it is drawn), the first roll-up,
 * the changes of the settings and the instructions and the files not read, each of its type.
 */
export function readReports(value: unknown): Page | null {
  const grain = field(value, "grain");
  const days = field(value, "days");
  const from = field(value, "from");
  const to = field(value, "to");
  const first = field(value, "first") ?? null;
  const buckets = listed(field(value, "buckets"));
  const marks = listed(field(value, "marks"));
  const unread = listed(field(value, "unread"));
  const landed = readLanded(field(value, "landed"));
  const prices = field(value, "prices") ?? "";
  if (
    landed === undefined ||
    typeof prices !== "string" ||
    typeof grain !== "string" ||
    parseGrain(grain) === null ||
    typeof days !== "number" ||
    !Number.isInteger(days) ||
    days < 0 ||
    typeof from !== "number" ||
    typeof to !== "number" ||
    (first !== null && typeof first !== "number") ||
    buckets === null ||
    marks === null ||
    !marks.every(isMark) ||
    unread === null ||
    !unread.every((line) => typeof line === "string")
  ) {
    return null;
  }
  return {
    grain: grain as Grain,
    days,
    from,
    to,
    buckets: buckets.map((bucket) => new Bucket(bucket)),
    first,
    marks: marks.map((mark) => {
      const changed = listed(field(mark, "changed")) as string[];
      return { ...mark, changed: changed.length > 0 ? changed : SETTINGS };
    }),
    unread: unread as string[],
    landed,
    prices,
  };
}

/**
 * Where a series' value is read from in a period: the number at a path; the number at a path the
 * answer always writes (none where it is missing, as in an answer from a server built before it);
 * the mean of the sessions running work in a role, or every other role's together for none; the
 * estimated dollars or the tokens of a role's model calls, or every other role's together for none;
 * or the sum of the numbers at two paths.
 */
type Pick =
  | { at: string }
  | { written: string }
  | { role: string | null }
  | { spend: string | null; of: "dollars" | "tokens" }
  | { sum: [string, string] };

/** The roles a chart of the agents names; the rest are its other roles. */
const NAMED = ["orchestrator", "executor", "reviewer"];

/** Every token of a role's spend, none where its tokens are not the wire's. */
function tokensOf(spend: unknown): number | null {
  const tokens = field(spend, "tokens");
  return isTokens(tokens) ? total(tokens) : null;
}

/** The value `pick` reads in `bucket`: none where its source has no record of the period. */
function picked(pick: Pick, bucket: Bucket): number | null {
  if ("at" in pick) return bucket.number(pick.at);
  if ("written" in pick) return bucket.written(pick.written);
  if ("role" in pick) {
    if (!bucket.has("agents")) return null;
    const means = bucket
      .list("agents", "running")
      .filter((level) => {
        const name = level.text("role");
        return pick.role === null ? !NAMED.includes(name) : name === pick.role;
      })
      .flatMap((level) => {
        const mean = level.number("mean");
        return mean === null ? [] : [mean];
      });
    return sum(means);
  }
  if ("spend" in pick) {
    const roles = field(bucket.raw, "spend");
    if (!Array.isArray(roles)) return null;
    return sum(
      roles
        .filter((one) => {
          const name = field(one, "role");
          return pick.spend === null ? !NAMED.includes(name as string) : name === pick.spend;
        })
        .flatMap((one) => {
          const value =
            pick.of === "dollars" ? field(field(one, "cost"), "dollars") : tokensOf(one);
          return typeof value === "number" ? [value] : [];
        }),
    );
  }
  const [one, other] = pick.sum;
  const first = bucket.number(one);
  const second = bucket.number(other);
  return first === null || second === null ? null : first + second;
}

/** A series: its name, how it is drawn and where its value is read from. */
interface Series {
  name: string;
  mark: Mark;
  pick: Pick;
}

/**
 * The source a chart's coverage is read from: the roll-up's hours, the agents' or the board's
 * health, or the hours whose tokens the roll-up told.
 */
type Source = "rollup" | "agents" | "health" | "spend";

interface Spec {
  id: string;
  title: string;
  /** What it shows; `{per}` is the period's word. */
  note: string;
  unit: Unit;
  source: Source;
  series: Series[];
}

/** A series drawn as `mark`, read at `path`. */
const at = (name: string, mark: Mark, path: string): Series => ({ name, mark, pick: { at: path } });
/** A bar read at `path`. */
const bar = (name: string, path: string) => at(name, "bar", path);
/** A line read at `path`. */
const line = (name: string, path: string) => at(name, "line", path);
/** A bar of the mean of the sessions running work in `role`, or in every other role for none. */
const role = (name: string, which: string | null): Series => ({
  name,
  mark: "bar",
  pick: { role: which },
});
/** The bars of each role's estimated dollars or tokens, the roles a chart of the agents names. */
const spent = (of: "dollars" | "tokens"): Series[] =>
  (
    [
      ["Orchestrators", "orchestrator"],
      ["Executors", "executor"],
      ["Reviewers", "reviewer"],
      ["Other roles", null],
    ] as const
  ).map(([name, which]) => ({ name, mark: "bar", pick: { spend: which, of } }));

/** The heading of the section the landed tasks' costs follow the charts of. */
export const SPEND = "Tokens and cost";

/** How the roll-up tells an hour's tokens, said under each chart of them. */
const TOLD =
  "An hour's tokens are what the board's usage gained between two hourly roll-ups, shared among " +
  "the hours between them by time: an hour no two roll-ups span is a gap, never a zero.";

/** The page's sections, each a heading and its charts. */
export const SECTIONS: [string, Spec[]][] = [
  [
    "Flow",
    [
      {
        id: "done",
        title: "Tasks done",
        note:
          "Top-level tasks done, per {per}, each once at its last close: one approved again after " +
          "its landing sent it back counts once. Hours and days are UTC; just tasks stats counts " +
          "the same tasks by this machine's days.",
        unit: "count",
        source: "rollup",
        series: [{ name: "Done", mark: "bar", pick: { written: "flow.done" } }],
      },
      {
        id: "open",
        title: "Open work",
        note: "Top-level tasks in each open status at the end of each {per}.",
        unit: "count",
        source: "rollup",
        series: [
          bar("Ready", "flow.held.ready"),
          bar("In progress", "flow.held.in_progress"),
          bar("Review", "flow.held.review"),
          bar("Blocked", "flow.held.blocked"),
        ],
      },
      {
        id: "task-hours",
        title: "Task-hours",
        note: "Hours top-level tasks spent in each open status, per {per}.",
        unit: "hours",
        source: "rollup",
        series: [
          bar("Ready", "flow.task_hours.ready"),
          bar("In progress", "flow.task_hours.in_progress"),
          bar("Review", "flow.task_hours.review"),
          bar("Blocked", "flow.task_hours.blocked"),
        ],
      },
      {
        id: "lead",
        title: "Lead time",
        note: "Creation to last close of the top-level tasks done in each {per}.",
        unit: "seconds",
        source: "rollup",
        series: [line("Median", "lead.median"), line("p90", "lead.p90")],
      },
      {
        id: "waits",
        title: "Waits between hands",
        note: "The median wait that ended in each {per}, by kind.",
        unit: "seconds",
        source: "rollup",
        series: [
          line("Review to reviewer", "waits.review_to_reviewer.median"),
          line("Changes to resumed", "waits.changes_to_resumed.median"),
          line("Changes to review", "waits.changes_to_review.median"),
          line("Approved to landed", "waits.approved_to_landed.median"),
        ],
      },
    ],
  ],
  [
    "Agents",
    [
      {
        id: "agents",
        title: "Agents at work",
        note:
          "Sessions running a role on a task in progress or in review, the mean over each {per}; " +
          "a session with two roles counts in both.",
        unit: "mean",
        source: "agents",
        series: [
          role("Orchestrators", "orchestrator"),
          role("Executors", "executor"),
          role("Reviewers", "reviewer"),
          role("Other roles", null),
          line("Sessions, each once", "agents.running_agents.mean"),
        ],
      },
      {
        id: "sessions",
        title: "Sessions",
        note: "Sessions that connected, and changes to stale and to lost, per {per}.",
        unit: "count",
        source: "agents",
        series: [
          line("Started", "agents.started"),
          line("Went stale", "agents.went_stale"),
          line("Went lost", "agents.went_lost"),
        ],
      },
      {
        id: "load",
        title: "Machine load",
        note: "The machine's load average over a minute, its peak and mean in each {per}.",
        unit: "load",
        source: "agents",
        series: [line("Peak", "agents.load_1m.peak"), line("Mean", "agents.load_1m.mean")],
      },
    ],
  ],
  [
    SPEND,
    [
      {
        id: "cost",
        title: "Estimated cost",
        note:
          "What the model calls the board recorded cost in each {per}, by role, estimated at API " +
          "list prices read {prices}: never money spent, as a subscription is not billed by the " +
          `token. Tokens of a model with no price are in no bar. ${TOLD}`,
        unit: "dollars",
        source: "spend",
        series: spent("dollars"),
      },
      {
        id: "tokens",
        title: "Tokens",
        note:
          "The tokens the board recorded in each {per}, by role: input, cache writes, cache reads " +
          `and output together, every model's. ${TOLD}`,
        unit: "tokens",
        source: "spend",
        series: spent("tokens"),
      },
    ],
  ],
  [
    "Decisions and code",
    [
      {
        id: "decisions",
        title: "Decisions",
        note: "Cards answered and auto-approved in each {per}, and those open at its end.",
        unit: "count",
        source: "rollup",
        series: [
          bar("Answered", "decisions.answered"),
          bar("Auto-approved", "decisions.auto_approved"),
          line("Open", "decisions.open"),
        ],
      },
      {
        id: "commits",
        title: "Commits",
        note: "Commits that reached the default branch, and its merges, per {per}.",
        unit: "count",
        source: "rollup",
        series: [bar("Commits", "git.commits"), bar("Merges", "git.merges")],
      },
      {
        id: "lines",
        title: "Lines changed",
        note:
          "Lines added and removed on the default branch, per {per}; generated files and lock " +
          "files left out.",
        unit: "count",
        source: "rollup",
        series: [bar("Added", "git.added"), at("Removed", "below", "git.removed")],
      },
    ],
  ],
  [
    "Quality",
    [
      {
        id: "defects",
        title: "Bounces, reds and reverts",
        note:
          "Moves from review back to work, reds filed, reverts, and changes asked by review, per " +
          "{per}.",
        unit: "count",
        source: "rollup",
        series: [
          line("Bounces", "flow.bounces"),
          line("Reds filed", "flow.reds_filed"),
          line("Reverts", "git.reverts"),
          line("Changes asked", "verdicts.changes"),
        ],
      },
      {
        id: "fix",
        title: "Time to fix a red",
        note: "Filing to close of the reds closed done in each {per}.",
        unit: "seconds",
        source: "rollup",
        series: [line("Median", "red_fix.median"), line("p90", "red_fix.p90")],
      },
    ],
  ],
  [
    "Board health",
    [
      {
        id: "health",
        title: "Board requests",
        note:
          "Requests to the board that took a second or more, that got no answer, and failed " +
          "dispatch and sweep passes, per {per}.",
        unit: "count",
        source: "health",
        series: [
          line("Slow", "health.slow"),
          line("Unanswered", "health.unanswered"),
          {
            name: "Failed passes",
            mark: "line",
            pick: { sum: ["health.dispatch.failed", "health.sweep.failed"] },
          },
        ],
      },
    ],
  ],
];

/** Every path a series of SECTIONS reads, by its chart and name: none for a series by role. */
export function paths(): [string, string, string][] {
  return SECTIONS.flatMap(([, specs]) =>
    specs.flatMap((spec) =>
      spec.series.flatMap((series): [string, string, string][] => {
        const { pick } = series;
        const read =
          "at" in pick
            ? [pick.at]
            : "written" in pick
              ? [pick.written]
              : "sum" in pick
                ? pick.sum
                : [];
        return read.map((path) => [spec.id, series.name, path]);
      }),
    ),
  );
}

/** How much of `bucket` `source` recorded, from 0 to 1. */
function cover(bucket: Bucket, source: Source): number {
  if (source === "spend") return Array.isArray(field(bucket.raw, "spend")) ? 1 : 0;
  const [start, end] = bucket.span();
  const seconds =
    source === "rollup"
      ? (bucket.number("rolled") ?? 0) * HOUR
      : (bucket.number(source === "agents" ? "agents.observed_s" : "health.covered_s") ?? 0);
  return Math.min(Math.max(seconds / Math.max(end - start, 1), 0), 1);
}

/** The most phases or states a chart of the workflow names: the rest are its "Other". */
export const PLACES_NAMED = 4;

/** The workflow's charts: by phase and by state, of the same hours, never added together. */
const WORKFLOW: [string, string, string, string][] = [
  [
    "phases",
    "Time by phase",
    "phases",
    "Task-hours top-level tasks spent in each phase of the project's workflow, per {per}, a phase " +
      "renamed since under the name its hours were spent under. Time on builds and tests is the " +
      "runs', never counted here.",
  ],
  [
    "states",
    "Time by state",
    "states",
    "Task-hours top-level tasks spent in each coordination state, per {per}: the same hours as by " +
      "phase, counted again by state.",
  ],
];

/** One phase's or state's hours in a period as the server writes them: `[id, label, hours]`. */
function entries(bucket: Bucket, list: string): [string, string, number][] {
  return bucket.list("workflow", list).flatMap((entry): [string, string, number][] => {
    const hours = entry.written("hours");
    const id = entry.text("id");
    return hours === null || id === "" ? [] : [[id, entry.text("label") || id, hours]];
  });
}

/**
 * The series of a chart of the workflow's `list` (`phases`, `states`) over `buckets`: one for each
 * ID and label it records, named by its label, its ID after it where two IDs share a label, the
 * largest over the window first; past PLACES_NAMED + 1 of them, the PLACES_NAMED largest and
 * "Other" for the rest. A period whose roll-up records no workflow is none in each, a gap.
 */
export function placeSeries(buckets: Bucket[], list: string): [string, (number | null)[]][] {
  const totals = new Map<string, [string, string, number]>();
  const key = (id: string, label: string) => `${id}\u0000${label}`;
  for (const bucket of buckets) {
    for (const [id, label, hours] of entries(bucket, list)) {
      const kept = totals.get(key(id, label));
      totals.set(key(id, label), [id, label, (kept?.[2] ?? 0) + hours]);
    }
  }
  const labels = new Map<string, number>();
  for (const [, label] of totals.values()) labels.set(label, (labels.get(label) ?? 0) + 1);
  const ranked = [...totals.values()]
    .map(([id, label, hours]): [string, string, number] => [
      key(id, label),
      (labels.get(label) ?? 0) > 1 ? `${label} (${id})` : label,
      hours,
    ])
    .sort((a, b) => b[2] - a[2] || (a[1] < b[1] ? -1 : a[1] > b[1] ? 1 : 0));
  const named = ranked.length > PLACES_NAMED + 1 ? ranked.slice(0, PLACES_NAMED) : ranked;
  const rest = new Set(ranked.slice(named.length).map(([kept]) => kept));
  const valuesOf = (pick: (id: string, label: string) => boolean) =>
    buckets.map((bucket) =>
      bucket.has("workflow")
        ? sum(
            entries(bucket, list)
              .filter(([id, label]) => pick(id, label))
              .map(([, , hours]) => hours),
          ) + 0
        : null,
    );
  const series: [string, (number | null)[]][] = named.map(([kept, name]) => [
    name,
    valuesOf((id, label) => key(id, label) === kept),
  ]);
  if (rest.size > 0) series.push(["Other", valuesOf((id, label) => rest.has(key(id, label)))]);
  return series;
}

/**
 * The section of a custom workflow's charts out of `page`, each series `hidden` names hidden on its
 * own chart alone; none while no period's roll-up records a workflow, as on a board that runs
 * none.
 */
export function workflowSection(page: Page, hidden: string[]): [string, ChartData[]] | null {
  if (!page.buckets.some((bucket) => bucket.has("workflow"))) return null;
  return [
    "Workflow",
    WORKFLOW.map(([id, title, list, note]) => ({
      id,
      title,
      note: note.replaceAll("{per}", page.grain),
      unit: "hours" as const,
      series: placeSeries(page.buckets, list).map(([name, values]) => ({
        name,
        mark: "bar" as const,
        values,
        hidden: hidden.includes(seriesKey(id, name)),
      })),
      cover: page.buckets.map((bucket) => (bucket.has("workflow") ? cover(bucket, "rollup") : 0)),
      instead: null,
    })),
  ];
}

/**
 * Every section's heading and its charts, ready to draw, out of `page`, each series `hidden` names
 * by its key (`seriesKey`) hidden on its own chart alone: the workflow's after Flow where a period
 * records one.
 */
export function charts(page: Page, hidden: string[]): [string, ChartData[]][] {
  const drawn = fixedCharts(page, hidden);
  const workflow = workflowSection(page, hidden);
  if (workflow !== null) drawn.splice(1, 0, workflow);
  return drawn;
}

/** The sections SECTIONS names, ready to draw. */
function fixedCharts(page: Page, hidden: string[]): [string, ChartData[]][] {
  return SECTIONS.map(([heading, specs]) => [
    heading,
    specs.map((spec) => ({
      id: spec.id,
      title: spec.title,
      note: spec.note
        .replaceAll("{per}", page.grain)
        .replaceAll("{prices}", page.prices === "" ? "on a day not named" : page.prices),
      unit: spec.unit,
      series: spec.series.map((series) => ({
        name: series.name,
        mark: series.mark,
        values: page.buckets.map((bucket) => picked(series.pick, bucket)),
        hidden: hidden.includes(seriesKey(spec.id, series.name)),
      })),
      cover: page.buckets.map((bucket) => cover(bucket, spec.source)),
      instead:
        spec.id === "lead" && page.grain === "hour"
          ? "The roll-up keeps lead time by the day: switch to Days or Weeks to see it."
          : null,
    })),
  ]);
}

const WEEKDAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/** The UTC date of `at`: its weekday's name, its day, its month's name and its hour. */
function date(at: number): [string, number, string, number] {
  const day = new Date(Math.floor(at / DAY) * DAY * 1000);
  const hour = Math.floor((((at % DAY) + DAY) % DAY) / HOUR);
  return [WEEKDAYS[day.getUTCDay()] ?? "", day.getUTCDate(), MONTHS[day.getUTCMonth()] ?? "", hour];
}

const twoDigits = (n: number) => String(n).padStart(2, "0");

/** A period as the readout and the table name it. */
export function period(grain: Grain, start: number): string {
  const [weekday, day, month, hour] = date(start);
  if (grain === "hour") return `${weekday} ${day} ${month}, ${twoDigits(hour)}:00 UTC`;
  return grain === "day" ? `${weekday} ${day} ${month}` : `Week of ${day} ${month}`;
}

/** A period as the axis labels it: its day, or its hour by the hour (its day at midnight). */
function tick(grain: Grain, start: number): string {
  const [, day, month, hour] = date(start);
  return grain === "hour" && hour !== 0 ? `${twoDigits(hour)}:00` : `${day} ${month}`;
}

/** Half away from zero, as Rust's `round`. */
function round(value: number): number {
  return Math.sign(value) * Math.round(Math.abs(value));
}

/**
 * The page's one time axis out of `page`: each period's name and hours, at most six labels under
 * the plot (by the hour on the clock's round hours, else counted back from the last period) and the
 * changes of the settings and of the instructions the agents work under, each saying what changed
 * (`rules, skill work changed, …`), its commit's subject and its hash.
 */
export function axis(page: Page): Axis {
  const count = page.buckets.length;
  const every = Math.max(Math.ceil(count / 6), 1);
  const spans = page.buckets.map((bucket) => bucket.span());
  const marks = page.marks.flatMap((mark): [number, number, string][] => {
    const at = spans.findIndex(([start, end]) => start <= mark.at && mark.at < end);
    const [start, end] = spans[at] ?? [0, 0];
    if (at < 0) return [];
    const within = (mark.at - start) / Math.max(end - start, 1);
    const text = `${mark.changed.join(", ")} changed, ${mark.subject} (${mark.commit})`;
    return [[((at + within) / count) * 100, at, text]];
  });
  let ticks: [number, string][];
  if (page.grain === "hour") {
    // Hours a round step apart, on the clock's own hours: midnights, noons, every sixth.
    const by = [1, 2, 3, 4, 6, 12, 24, 48].find((step) => count <= 6 * step) ?? 48;
    ticks = spans.flatMap(([start], at): [number, string][] => {
      const hour = round(start / HOUR);
      return ((hour % by) + by) % by === 0 ? [[at, tick(page.grain, start)]] : [];
    });
  } else {
    // Days and weeks counted back from the last, which is always labelled.
    ticks = [];
    for (let at = count - 1; at >= 0; at -= every) {
      ticks.unshift([at, tick(page.grain, spans[at]?.[0] ?? 0)]);
    }
  }
  return {
    periods: spans.map(([start]) => period(page.grain, start)),
    hours: spans.map(([start, end]) => Math.max((end - start) / HOUR, 0)),
    ticks,
    marks,
  };
}

/** One of the four figures over the window. */
export interface Figure {
  label: string;
  value: string;
  detail: string;
}

function missing(label: string, why: string): Figure {
  return { label, value: "–", detail: why };
}

/**
 * The four figures over the window: the tasks done, the lead time, the agents at work and the
 * decisions open at its end.
 */
export function figures(page: Page): [Figure, Figure, Figure, Figure] {
  const per = page.grain;
  // The periods `source` recorded, and the sum of the numbers at `path` over them.
  const recorded = (source: string) => page.buckets.filter((bucket) => bucket.has(source));
  const numbers = (of: Bucket[], path: string, read = (b: Bucket) => b.number(path)) =>
    of.flatMap((bucket) => {
      const value = read(bucket);
      return value === null ? [] : [value];
    });
  const total = (of: Bucket[], path: string) => sum(numbers(of, path)) + 0;
  const rolled = recorded("flow");
  // The periods whose roll-up counted the tasks done: a day file may not record them.
  const flows = rolled.filter((bucket) => bucket.written("flow.done") !== null);
  let done: Figure;
  if (rolled.length === 0) {
    done = missing("Done", "no roll-up in the window");
  } else if (flows.length === 0) {
    done = missing("Done", "no roll-up in the window counted them");
  } else {
    const count = sum(numbers(flows, "flow.done", (bucket) => bucket.written("flow.done")));
    const mean = format("mean", round((count / flows.length) * 10) / 10);
    // The sum is the window's only where every period of it counted them. A period rolled up
    // without them was rolled up, in part at least, before the roll-up counted them.
    const periods = page.buckets.length;
    const early =
      rolled.length === flows.length
        ? ""
        : `, ${rolled.length - flows.length} rolled up before the count began`;
    done = {
      label: "Done",
      value: short(count, 0),
      detail:
        flows.length < periods
          ? `${mean} a ${per} on average · ${flows.length} of ${periods} ${per}s counted${early}`
          : `${mean} a ${per} on average`,
    };
  }
  const leads = recorded("lead");
  const closed = total(leads, "lead.n");
  let lead: Figure;
  if (page.grain === "hour") {
    lead = missing("Lead time", "kept by the day");
  } else if (closed <= 0) {
    lead = missing("Lead time", "no task closed in the window");
  } else {
    const median =
      sum(
        leads.flatMap((bucket) => {
          const median = bucket.number("lead.median");
          const n = bucket.number("lead.n");
          return median === null || n === null ? [] : [median * n];
        }),
      ) / closed;
    const p90 = numbers(leads, "lead.p90").reduce((most, value) => Math.max(most, value), 0);
    lead = {
      label: "Lead time",
      value: format("seconds", median),
      detail: `median of ${short(closed, 0)} closed · p90 up to ${format("seconds", p90)}`,
    };
  }
  const watched = recorded("agents");
  const seconds = total(watched, "agents.observed_s");
  let agents: Figure;
  if (seconds <= 0) {
    agents = missing("Agents at work", "no agents' hours in the window");
  } else {
    const mean =
      sum(
        watched.flatMap((bucket) => {
          const mean = bucket.number("agents.running_agents.mean");
          const observed = bucket.number("agents.observed_s");
          return mean === null || observed === null ? [] : [mean * observed];
        }),
      ) / seconds;
    const peak = numbers(watched, "agents.running_agents.peak").reduce(
      (most, value) => Math.max(most, value),
      0,
    );
    agents = {
      label: "Agents at work",
      value: format("mean", round(mean * 10) / 10),
      detail: `running on average · ${short(peak, 0)} at most`,
    };
  }
  const cards = recorded("decisions");
  const last = cards.at(-1);
  const decisions =
    last === undefined
      ? missing("Open decisions", "no cards read in the window")
      : {
          label: "Open decisions",
          value: short(last.number("decisions.open") ?? 0, 0),
          detail:
            `${short(total(cards, "decisions.auto_approved"), 0)} auto-approved · ` +
            `${short(total(cards, "decisions.answered"), 0)} answered in the window`,
        };
  return [done, lead, agents, decisions];
}

/** What the window's periods told of tokens, summed. */
export interface WindowSpend {
  dollars: number;
  tokens: number;
  /** Whether some of them had no price. */
  unpriced: boolean;
  /** The periods that told any. */
  told: number;
}

/** The tokens the periods of `page` told, all told: none where no period told any. */
export function windowSpend(page: Page): WindowSpend | null {
  const told = page.buckets.flatMap((bucket) => {
    const roles = field(bucket.raw, "spend");
    return Array.isArray(roles) ? [roles as unknown[]] : [];
  });
  if (told.length === 0) return null;
  const spend: WindowSpend = { dollars: 0, tokens: 0, unpriced: false, told: told.length };
  for (const role of told.flat()) {
    const cost = field(role, "cost");
    if (isCost(cost)) {
      spend.dollars += cost.dollars;
      spend.unpriced ||= cost.unpriced.length > 0;
    }
    spend.tokens += tokensOf(role) ?? 0;
  }
  return spend;
}

/** Dollars as a figure says them: `+ unpriced` after where some tokens had no price. */
function dollarsSaid(figure: number, unpriced: boolean): string {
  if (unpriced && figure === 0) return "unpriced";
  return unpriced ? `${dollars(figure)} + unpriced` : dollars(figure);
}

/** The models `costs` name no price for, each once, with its tokens summed. */
function unpricedOf(costs: AgentCost[]): AgentCost {
  const merged: AgentCost["unpriced"] = [];
  for (const one of costs.flatMap((cost) => cost.unpriced)) {
    const kept = merged.find((each) => each.model === one.model && each.speed === one.speed);
    if (kept === undefined) {
      merged.push({ ...one, tokens: { ...one.tokens } });
      continue;
    }
    for (const kind of [
      "input",
      "cache_write_5m",
      "cache_write_1h",
      "cache_read",
      "output",
      "calls",
    ] as const) {
      kept.tokens[kind] += one.tokens[kind];
    }
  }
  const [first] = costs;
  return { ...(first as AgentCost), unpriced: merged };
}

/**
 * Under the charts of tokens: the window's all told; what the tasks landed in it took, each with
 * its subtasks (the median, the dearest ten, each a link to its page, and their first rounds
 * against the rounds after); and that every dollar is an estimate at API list prices, with the
 * day they were read, never money spent.
 */
export function SpendSummary({ page }: { page: Page }) {
  const told = windowSpend(page);
  const { landed, grain } = page;
  const periods = page.buckets.length;
  const unpriced =
    landed === null || landed.tasks === 0
      ? ""
      : unpricedSaid(unpricedOf([landed.first_round.cost, landed.later_rounds.cost]));
  return (
    <div className="reports-spend">
      <p>
        {told === null
          ? `No ${grain} of the window told its tokens: an hour's are told once two roll-ups span it.`
          : `${told.told === periods ? "In the window" : `In the ${told.told} of ${periods} ${grain}s that told them`}: ` +
            `${dollarsSaid(told.dollars, told.unpriced)} estimated, ${amount(told.tokens)} tokens.`}
      </p>
      <h3>Cost per landed task</h3>
      {landed === null ? (
        <p>No task landed in the window.</p>
      ) : landed.tasks === 0 ? (
        <p>{`${landed.without} landed, none with tokens recorded.`}</p>
      ) : (
        <>
          <p>
            {`${landed.tasks} landed with tokens recorded` +
              (landed.without > 0 ? ` (${landed.without} with none, in no figure)` : "") +
              `, each with its subtasks: a median of ${
                landed.median === null ? "unpriced" : dollars(landed.median)
              } a task. The dearest:`}
          </p>
          <ol className="reports-dearest">
            {landed.dearest.map((task) => (
              <li key={task.slug} className="reports-landed-task">
                <a href={href(`/task/${task.slug}`)}>{task.slug}</a>
                <span>{`${costSaid(task.cost)} · ${amount(total(task.tokens))} tokens`}</span>
              </li>
            ))}
          </ol>
          <p>
            {`First rounds ${costSaid(landed.first_round.cost)} ` +
              `(${amount(total(landed.first_round.tokens))} tokens), the review rounds after them ` +
              `${costSaid(landed.later_rounds.cost)} ` +
              `(${amount(total(landed.later_rounds.tokens))} tokens): ${landed.again} of ` +
              `${landed.tasks} went past their first round.`}
          </p>
        </>
      )}
      <p className="reports-note">
        {`Every dollar here is ${estimate(page.prices === "" ? "on a day not named" : page.prices)}; ` +
          "a figure marked + unpriced leaves out the tokens of a model the price table does not name" +
          (unpriced === "" ? "." : `: ${unpriced}.`)}
      </p>
    </div>
  );
}

/** The four figures, each with the id of the chart it sums up, which its link opens. */
export function linkedFigures(page: Page): [Figure, string][] {
  const [done, lead, agents, decisions] = figures(page);
  return [
    [done, "done"],
    [lead, "lead"],
    [agents, "agents"],
    [decisions, "decisions"],
  ];
}

/**
 * `page` without its leading periods from before the project's roll-up began (`first`): the
 * periods no source recorded that end by its first file's first hour, all hatching on every chart
 * and in no figure. A period a source recorded stops the trim (agents watched before the first
 * roll-up), and so does one from the roll-up's time that no file holds: a day the server did not
 * run or whose file went unread is a gap, shown hatched. With no roll-up yet the page stays whole:
 * nothing tells a period before it from a gap.
 */
export function fromFirstRecord(page: Page): Page {
  const { first } = page;
  if (first === null) return page;
  let before = 0;
  for (const bucket of page.buckets) {
    if (bucket.recorded() || bucket.span()[1] > first) break;
    before += 1;
  }
  return before < page.buckets.length ? { ...page, buckets: page.buckets.slice(before) } : page;
}

/** What the page says while the roll-up has written no file: when the next one is due. */
export function firstRollUp(page: Page): string {
  const [, , , hour] = date(page.to);
  return (
    "No roll-up yet. The machine's shared server writes this project's first day file at its " +
    `first board tick after it starts, then once an hour: the next is due by ${twoDigits(hour)}:00 ` +
    "UTC. A server of a project's own writes none."
  );
}

/** A style that sets custom properties. */
function css(properties: Record<string, string>): CSSProperties {
  return properties as CSSProperties;
}

export interface ReportsViewProps {
  grain: Grain;
  days: number;
  page: Page | null;
  error?: string | null;
  /** The series hidden, by their keys (`seriesKey`). */
  hidden?: string[];
  onAsk: (grain: Grain, days: number) => void;
  /** Takes the key of the series a legend's button hides or shows again. */
  onToggle?: (key: string) => void;
  /** Whether the chart the address names lands as its window draws: until the reader chooses one. */
  lands?: boolean;
  /**
   * While the page shown is the copy this device kept and no read has answered since: when it was
   * kept, in ms.
   */
  device?: number | null;
}

/**
 * The page under its header: the window's controls, its four figures, the marked changes and
 * the charts, or the ghost of them while the first answer is on its way. A page drawn from what
 * this device kept is marked so (`data-cache`) and says it as the other pages do, in their notices
 * (`PageNotices`: out of the page's flow, so its going moves nothing, with the reserve that keeps
 * the page's end clear of it and the toasts above it), until a read answers: that it is checking,
 * or, while the read fails, that the server is not reachable.
 */
export function ReportsView({
  grain,
  days,
  page,
  error = null,
  hidden = [],
  onAsk,
  onToggle,
  lands = true,
  device = null,
}: ReportsViewProps) {
  // Busy while the window asked for is on its way, and no longer once its read failed: the page
  // then says so, drawn, and a region busy on the screen would hold `symphony shot` to its timeout.
  const busy = (page === null || page.grain !== grain || page.days !== days) && error === null;
  const kept = page !== null ? device : null;
  return (
    <div
      className="reports"
      data-ready={page !== null ? "reports" : ready(false, error !== null)}
      data-cache={kept !== null ? "device" : undefined}
      aria-busy={busy}
    >
      <header className="reports-head">
        <div>
          <h1>Reports</h1>
          <p className="reports-sub">
            {`${grainName(grain)} · the last ${rangeName(grain, days)} · times in UTC`}
          </p>
        </div>
        <Controls grain={grain} days={days} onAsk={onAsk} />
      </header>
      {error !== null && (
        <p className="reports-error" role="alert">
          {`The reports could not be read. ${failed(error, true)}`}
        </p>
      )}
      {page === null ? (
        <GhostReports />
      ) : (
        // A new window is a new axis: the period tapped on the last one means nothing on it.
        <Report
          key={`${page.grain}-${page.days}-${page.from}`}
          page={page}
          hidden={hidden}
          onToggle={onToggle}
          lands={lands}
        />
      )}
      <PageNotices reconnecting={false} device={kept !== null ? [kept, error !== null] : null} />
    </div>
  );
}

/** The grain's buttons and the window's. */
function Controls({
  grain,
  days,
  onAsk,
}: {
  grain: Grain;
  days: number;
  onAsk: (grain: Grain, days: number) => void;
}) {
  return (
    <div className="reports-controls">
      {/* biome-ignore lint/a11y/useSemanticElements: a group of toggles, as the Dioxus page draws it */}
      <div role="group" aria-label="Period">
        {(["hour", "day", "week"] as const).map((each) => (
          <Button
            key={each}
            size="sm"
            aria-pressed={each === grain}
            data-grain={each}
            onClick={() => onAsk(each, defaultDays(each))}
          >
            {grainName(each)}
          </Button>
        ))}
      </div>
      {/* biome-ignore lint/a11y/useSemanticElements: a group of toggles, as the Dioxus page draws it */}
      <div role="group" aria-label="Window">
        {ranges(grain).map((each) => (
          <Button
            key={each}
            size="sm"
            aria-pressed={each === days}
            data-days={each}
            onClick={() => onAsk(grain, each)}
          >
            {rangeName(grain, each)}
          </Button>
        ))}
      </div>
    </div>
  );
}

interface ReportProps {
  page: Page;
  hidden: string[];
  onToggle: ((key: string) => void) | undefined;
  lands: boolean;
}

/**
 * One window's report, its axis from the first period recorded or the roll-up's start
 * (`fromFirstRecord`). The period tapped is the same on every chart; the series `hidden` names are
 * hidden on theirs. Each figure links to its chart while the charts are drawn. A chart's id is an
 * anchor of its slot (`chart-ID`), held in its ghost until it mounts, so the address of a chart not
 * drawn yet finds its place, and a slot the address names mounts at once and lands its chart
 * (`NearViewport`, the shell's `reviewLand`), as a document's part does: on a fresh load, whose
 * charts come after the shell has looked for the address, too. The shell lands once a navigation,
 * so a new window or the hourly slide, which mount every chart again, moves no one. Nor does the
 * first window to draw charts once the reader chose it (`lands` false), when the first answer
 * drew none (nothing recorded, a failed read) and no landing started.
 */
function Report({ page: whole, hidden, onToggle, lands }: ReportProps) {
  const [tapped, setTapped] = useState<number | null>(null);
  const page = fromFirstRecord(whole);
  const shownAxis = axis(page);
  const drawn = page.buckets.some((bucket) => bucket.recorded());
  const unread = page.unread.length;
  return (
    <>
      {page.first === null && (
        <section className="reports-empty" aria-label="No roll-up yet">
          <p>{firstRollUp(page)}</p>
        </section>
      )}
      {unread > 0 && (
        <details className="reports-unread">
          <summary>
            {`${unread} ${unread === 1 ? "file" : "files"} not read: their hours show as gaps`}
          </summary>
          <ul>
            {page.unread.map((line) => (
              <li key={line}>{line}</li>
            ))}
          </ul>
        </details>
      )}
      <dl className="reports-figures">
        {linkedFigures(page).map(([figure, chart]) => (
          <div key={figure.label} className="reports-figure">
            <dt>{drawn ? <a href={`#chart-${chart}`}>{figure.label}</a> : figure.label}</dt>
            <dd className="value">{figure.value}</dd>
            <dd className="detail">{figure.detail}</dd>
          </div>
        ))}
      </dl>
      {shownAxis.marks.length > 0 && <MarkedChanges axis={shownAxis} />}
      {drawn &&
        charts(page, hidden).map(([heading, section]) => (
          <section key={heading} className="reports-section" aria-label={heading}>
            <h2>{heading}</h2>
            <div className="reports-charts">
              {section.map((chart) => (
                <NearViewport
                  key={chart.id}
                  height={330}
                  className="chart-slot"
                  anchors={[`chart-${chart.id}`]}
                  lands={lands}
                  ghost={<GhostChart />}
                >
                  <ChartCard
                    chart={chart}
                    axis={shownAxis}
                    selected={tapped}
                    onSelect={setTapped}
                    {...(onToggle ? { onToggle } : {})}
                  />
                </NearViewport>
              ))}
            </div>
            {heading === SPEND && <SpendSummary page={page} />}
          </section>
        ))}
    </>
  );
}

/** How many marked changes the page lists before it folds the rest under a disclosure. */
const MARKS_SHOWN = 3;

/**
 * The window's changes of the settings and of the instructions the agents work under (a skill, the
 * rules), newest first: the newest MARKS_SHOWN, a line each while the rest are folded
 * behind "N more changes" (reports.css), so a window of many still shows its first chart on a
 * phone's first screen.
 */
function MarkedChanges({ axis: { marks, periods } }: { axis: Axis }) {
  const newest = marks
    .toReversed()
    .map(([, at, text]) => <li key={text}>{`${periods[at]}: ${text}`}</li>);
  const folded = newest.length - MARKS_SHOWN;
  return (
    <div className="reports-marks">
      <ul aria-label="Changes to the settings and the agents' instructions in the window">
        {newest.slice(0, MARKS_SHOWN)}
      </ul>
      {folded > 0 && (
        <details>
          <summary>{`${folded} more ${folded === 1 ? "change" : "changes"}`}</summary>
          <ul>{newest.slice(MARKS_SHOWN)}</ul>
        </details>
      )}
    </div>
  );
}

/** The page waiting for its first answer: one status line, four figures' boxes and two charts'. */
function GhostReports() {
  return (
    <>
      <p className="status-text" role="status">
        Loading the reports…
      </p>
      <div className="reports-figures ghost-figures" aria-hidden="true">
        {["a", "b", "c", "d"].map((figure) => (
          <div key={figure} className="ghost-figure">
            <span className="bone" style={css({ "--w": "50%" })} />
            <span className="bone value" />
            <span className="bone" style={css({ "--w": "80%" })} />
          </div>
        ))}
      </div>
      <div className="reports-charts">
        <GhostChart />
        <GhostChart />
      </div>
    </>
  );
}

/** The window the page opens on while this browser has chosen none: the last 24 hours, by the hour. */
export const FIRST_WINDOW: [Grain, number] = ["hour", 1];

/**
 * Where this browser keeps the series it hid, a key (`seriesKey`) a line, beside the window
 * (`reports-grain`, `reports-days`), each under the page's project (storage.ts).
 */
export const HIDDEN = "reports-hidden";

/**
 * The page's grain and window as this browser last chose them, else FIRST_WINDOW; a window the grain
 * does not offer is its grain's default.
 */
export function chosen(read: (key: string) => string | null): [Grain, number] {
  const grain = parseGrain(read("reports-grain") ?? "");
  if (grain === null) return FIRST_WINDOW;
  const kept = read("reports-days") ?? "";
  const days = /^\+?\d+$/.test(kept) ? Number(kept) : null;
  return [grain, days !== null && ranges(grain).includes(days) ? days : defaultDays(grain)];
}

/** The series this browser hid, by their keys, as HIDDEN keeps them; none where it keeps none. */
export function hiddenSeries(read: (key: string) => string | null): string[] {
  return (read(HIDDEN) ?? "")
    .split("\n")
    .map((key) => (key.endsWith("\r") ? key.slice(0, -1) : key))
    .filter((key) => key !== "");
}

/** `hidden` with the series `key` shown again where it was hidden, else hidden. */
export function toggle(hidden: string[], key: string): string[] {
  return hidden.includes(key) ? hidden.filter((kept) => kept !== key) : [...hidden, key];
}

/** How often an open page reads its window again, in ms. */
export const EVERY_MS = 300_000;

/** What a read of the reports tells the page. */
export interface ReportsCheck {
  /** They answered, with their tag; none for a `304` of the copy shown. */
  answered(read: Page | null, etag: string): void;
  /** A read failed, or answered what is not a report: said, and the copy shown kept. */
  failed(said: string): void;
}

/**
 * Reads the window (`grain`, `days`) at once and then every EVERY_MS (through `wait`), naming the
 * tag of the copy shown (`etag`) so an hour no file changed in answers `304`, until `signal` ends
 * it. A read that failed is tried again sooner, after Backoff's waits (1 s, doubling to 30 s).
 */
export async function followReports(
  grain: Grain,
  days: number,
  etag: () => string,
  signal: AbortSignal,
  check: ReportsCheck,
  wait: (ms: number, signal: AbortSignal) => Promise<void> = sleep,
): Promise<void> {
  const url = `${REPORTS_PATH}?grain=${grain}&days=${days}`;
  let backoff = new Backoff();
  while (!signal.aborted) {
    let next = EVERY_MS;
    try {
      const answer = await getIf(url, etag(), signal);
      if (signal.aborted) return;
      if (answer.same) {
        check.answered(null, "");
      } else {
        const read = readReports(answer.value);
        if (read === null) throw new Error("The reports' answer is not a report");
        check.answered(read, answer.etag);
      }
      backoff = new Backoff();
    } catch (failure) {
      if (signal.aborted) return;
      check.failed(said(failure));
      next = backoff.after(0);
    }
    await wait(next, signal);
  }
}

/**
 * The Reports page of the checkout `scope` names: each window it drew a part, the answer as the
 * server sent it (a few KB by the day or the week, some 140 KB for a week by the hour, nine windows
 * in all), so it is kept whole. It is named here, not in cache.ts, so the chunk the pages that follow
 * the board load carries nothing of this page's.
 */
export function reportsItem(scope: string): Item {
  return { kind: "reports", id: `reports:${scope === "" ? "" : `@${scope}/`}`, pinned: false };
}

/** The part of a reports item that holds one window's answer: `hour:1`, `day:14`. */
export function reportsWindow(grain: string, days: number): string {
  return `${grain}:${days}`;
}

/** The answer `page` was read from, as this device keeps it: each period as the server sent it. */
function asKept(page: Page): unknown {
  return { ...page, buckets: page.buckets.map((bucket) => bucket.raw) };
}

/** What a window's kept copy and its reads tell the page. */
export interface ReportsShow {
  /** The copy this device kept, to draw at once: when it was kept, in ms. */
  kept(page: Page, saved: number): void;
  /** The server answered: the window as it is now, none where the copy shown still is. */
  answered(page: Page | null): void;
  /** A read failed: what is shown stays. */
  failed(said: string): void;
}

/**
 * A window of the reports opened: the copy of it this device kept (`item`, a part a window) drawn
 * at once, then `followReports` naming that copy's tag, so a window no roll-up changed since costs
 * no body, and each new answer kept in its place. A window never kept, or kept in a form this
 * client cannot read, is read whole.
 */
export async function openReports(
  item: Item,
  grain: Grain,
  days: number,
  signal: AbortSignal,
  show: ReportsShow,
  wait?: (ms: number, signal: AbortSignal) => Promise<void>,
): Promise<void> {
  const part = reportsWindow(grain, days);
  const kept = await readKept(item, part, readReports);
  if (signal.aborted) return;
  let etag = kept?.etag ?? "";
  if (kept !== null) show.kept(kept.value, kept.saved);
  await followReports(
    grain,
    days,
    () => etag,
    signal,
    {
      answered(read, tag) {
        if (read !== null) {
          etag = tag;
          keepLater(item, part, tag, asKept(read));
        }
        show.answered(read);
      },
      failed: show.failed,
    },
    wait,
  );
}

/**
 * The page, its read and its window: the window this browser last chose, read at once, again
 * whenever another is chosen, and every five minutes while the page is open, naming the copy shown
 * so an hour no file changed in answers `304`. A read that fails keeps what is shown and says why,
 * until a read answers or another window is chosen.
 * Each window's last answer is kept on this device (`openReports`) and drawn at once on a return,
 * or when its window is chosen again, marked as this device's until a read answers. The series
 * this browser hid stay hidden in every window, and each hidden or shown again is kept at once.
 */
export function ReportsPage({ scope = "" }: { scope?: string }) {
  const [[grain, days], setWindow] = useState(() => chosen(load));
  const [hidden, setHidden] = useState(() => hiddenSeries(load));
  const [page, setPage] = useState<Page | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [device, setDevice] = useState<number | null>(null);
  useEffect(() => visitLater(reportsItem(scope)), [scope]);
  // Each choice reads again, the same window too, as a choice of it does. Once the reader has made
  // one, the address lands no chart.
  const [asked, setAsked] = useState(0);
  useEffect(() => {
    void asked;
    const controller = new AbortController();
    void openReports(reportsItem(scope), grain, days, controller.signal, {
      kept(read, saved) {
        setPage(read);
        setDevice(saved);
      },
      answered(read) {
        if (read !== null) setPage(read);
        setDevice(null);
        setError(null);
      },
      failed: setError,
    });
    return () => controller.abort();
  }, [scope, grain, days, asked]);
  return (
    <main className="reports-page">
      <ReportsView
        grain={grain}
        days={days}
        page={page}
        error={error}
        hidden={hidden}
        lands={asked === 0}
        device={device}
        onAsk={(nextGrain, nextDays) => {
          save("reports-grain", nextGrain);
          save("reports-days", String(nextDays));
          setWindow([nextGrain, nextDays]);
          // The read that failed was the window left: the one chosen is not said unreachable
          // before its own read is tried.
          setError(null);
          setAsked((count) => count + 1);
        }}
        onToggle={(key) => {
          const next = toggle(hidden, key);
          save(HIDDEN, next.join("\n"));
          setHidden(next);
        }}
      />
    </main>
  );
}
