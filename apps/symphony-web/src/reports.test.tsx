import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import fixture from "../../../crates/symphony-api/fixtures/reports-page.json";
import { type ChartData, gap, seriesKey } from "./chart";
import {
  axis,
  Bucket,
  charts,
  chosen,
  defaultDays,
  EVERY_MS,
  FIRST_WINDOW,
  figures,
  firstRollUp,
  followReports,
  fromFirstRecord,
  HIDDEN,
  hiddenSeries,
  linkedFigures,
  type Page,
  parseGrain,
  paths,
  period,
  ReportsPage,
  ReportsView,
  rangeName,
  ranges,
  readReports,
  SECTIONS,
  toggle,
} from "./reports";
import { Backoff } from "./state";
import { asking, CASES, childrenOf, memoryStorage, reply, server } from "./testing";
import type { Grain } from "./wire";

const HOUR = 3_600;
const DAY = 86_400;
/** Monday 2026-11-09, 00:00 UTC. */
const MONDAY = 1_794_182_400;

type Wire = Record<string, unknown>;

/** A period as the server writes it (`ReportBucket`): no source with a record of it but `sources`. */
function bucket(start: number, end: number, sources: Wire = {}): Wire {
  return {
    start,
    end,
    rolled: 0,
    flow: null,
    decisions: null,
    git: null,
    waits: null,
    lead: null,
    verdicts: null,
    red_fix: null,
    agents: null,
    health: null,
    ...sources,
  };
}

/** A period's roll-up of the board (`BucketFlow`), each count by status without its zeros. */
function flow(fields: Wire): Wire {
  return {
    held: {},
    entered: {},
    done: null,
    task_hours: {},
    unclaimed_hours: 0,
    bounces: 0,
    landed: 0,
    reds_filed: 0,
    reds_closed: 0,
    ...fields,
  };
}

/** A period's agents (`BucketAgents`). */
function agents(fields: Wire): Wire {
  const none = { peak: 0, mean: 0 };
  return {
    observed_s: 0,
    samples: 0,
    running: [],
    running_agents: none,
    holding: [],
    holding_agents: none,
    live: [],
    started: 0,
    went_stale: 0,
    went_lost: 0,
    stale_s: 0,
    lost_s: 0,
    load_1m: null,
    load_15m: null,
    ...fields,
  };
}

/** Monday's agents: an executor, an orchestrator and a designer at work, three at most. */
const mondayAgents = () =>
  agents({
    observed_s: 3600,
    samples: 60,
    running: [
      { role: "executor", peak: 3, mean: 2 },
      { role: "orchestrator", peak: 1, mean: 1 },
      { role: "designer", peak: 1, mean: 0.5 },
    ],
    running_agents: { peak: 3, mean: 2.5 },
  });

/**
 * Three days: Monday rolled up whole with its agents watched, Tuesday recorded by no source,
 * Wednesday under way, half rolled up; a settings change on Monday.
 */
function wire(): Wire & { buckets: Wire[] } {
  return {
    grain: "day",
    days: 3,
    from: MONDAY,
    to: MONDAY + 2 * DAY + 11 * HOUR,
    buckets: [
      bucket(MONDAY, MONDAY + DAY, {
        rolled: 24,
        // One task approved again after its landing sent it back: four entries into done, three tasks.
        flow: flow({ entered: { done: 4 }, done: 3, held: { ready: 2, review: 1 }, bounces: 1 }),
        decisions: { open: 4, answered: 1, auto_approved: 2, withdrawn: 0 },
        git: { commits: 12, merges: 3, added: 1200, removed: 300, reverts: 1 },
        lead: { n: 2, median: 3600, p90: 7200 },
        agents: mondayAgents(),
      }),
      bucket(MONDAY + DAY, MONDAY + 2 * DAY),
      bucket(MONDAY + 2 * DAY, MONDAY + 2 * DAY + 11 * HOUR, {
        rolled: 5,
        flow: flow({ entered: { done: 4 }, done: 4, held: { in_progress: 5 } }),
        decisions: { open: 2, answered: 1, auto_approved: 0, withdrawn: 0 },
        lead: { n: 1, median: 1800, p90: 1800 },
      }),
    ],
    first: MONDAY - 30 * DAY,
    marks: [
      {
        at: MONDAY + 6 * HOUR,
        commit: "4f1c0a9b2d3e",
        subject: "settings: land without the check",
      },
    ],
    unread: [],
  };
}

/** `value` as the page reads it: sent as the server sends it and parsed, as the browser does. */
function read(value: unknown): Page {
  const page = readReports(JSON.parse(JSON.stringify(value)));
  if (page === null) throw new Error("The page reads no report in it");
  return page;
}

const page = () => read(wire());

function find(drawn: [string, ChartData[]][], id: string): ChartData {
  const chart = drawn.flatMap(([, section]) => section).find((one) => one.id === id);
  if (!chart) throw new Error(`No chart ${id}`);
  return chart;
}

/** The values of chart `id`'s `at`th series in `drawn`. */
const values = (drawn: [string, ChartData[]][], id: string, at = 0) =>
  find(drawn, id).series[at]?.values;

/** Each period's weekday, as the axis names it. */
const weekdays = (shown: Page) => axis(shown).periods.map((name) => name.slice(0, 3));

/** `wire` in a window of five days from the Saturday before, Saturday and Sunday recorded by no source. */
function wide(first: number | null) {
  const wide = wire();
  return {
    ...wide,
    days: 5,
    from: MONDAY - 2 * DAY,
    first,
    buckets: [
      bucket(MONDAY - 2 * DAY, MONDAY - DAY),
      bucket(MONDAY - DAY, MONDAY),
      ...wide.buckets,
    ],
  };
}

/** `shape` with every period recorded by no source. */
const unrecorded = <T extends { buckets: Wire[] }>(shape: T): T => ({
  ...shape,
  buckets: shape.buckets.map((one) => bucket(one.start as number, one.end as number)),
});

const view = (shown: Page | null, error: string | null = null) =>
  renderToStaticMarkup(
    <ReportsView grain="day" days={3} page={shown} error={error} onAsk={() => {}} />,
  );

/**
 * Each series' value in the first period of chart `id`: an empty sum is a negative zero, as in
 * Rust, where it equals zero as it does here.
 */
const firsts = (drawn: [string, ChartData[]][], id: string) =>
  find(drawn, id).series.map((data) => (data.values[0] ?? Number.NaN) + 0);

afterEach(() => vi.unstubAllGlobals());

describe("the grains", () => {
  it.each(CASES.grains)("read %j as crates/symphony-api does", (word, grain, days, longest) => {
    expect(parseGrain(word)).toBe(grain);
    if (grain === null) return;
    const read = grain as Grain;
    expect(defaultDays(read)).toBe(days);
    expect(ranges(read)).toContain(defaultDays(read));
    expect(Math.max(...ranges(read))).toBeLessThanOrEqual(longest ?? 0);
  });

  it("name their windows", () => {
    expect(rangeName("hour", 1)).toBe("24 hours");
    expect(rangeName("hour", 7)).toBe("7 days");
    expect(rangeName("week", 84)).toBe("12 weeks");
    expect(rangeName("day", 30)).toBe("30 days");
  });
});

describe("a period's numbers", () => {
  const one = new Bucket({
    start: 10,
    end: 70,
    flow: { held: { ready: 2 }, task_hours: {} },
    waits: { changes_to_review: null, review_to_reviewer: { median: 30.5 } },
    agents: null,
  });

  it("are none where the source has no record, and zero where a count left one out", () => {
    expect(one.number("flow.held.ready")).toBe(2);
    expect(one.number("flow.held.blocked")).toBe(0);
    expect(one.number("flow.task_hours.review")).toBe(0);
    expect(one.number("waits.review_to_reviewer.median")).toBe(30.5);
    expect(one.number("waits.changes_to_review.median")).toBeNull();
    expect(one.number("agents.started")).toBeNull();
    expect(one.number("git.commits")).toBeNull();
    expect(one.number("flow.held.ready.more")).toBeNull();
    expect([one.has("flow"), one.has("agents"), one.has("git")]).toEqual([true, false, false]);
    expect(one.span()).toEqual([10, 70]);
    expect(one.list("agents", "running")).toEqual([]);
    expect(one.text("start")).toBe("");
    expect(new Bucket(null).span()).toEqual([0, 0]);
    // An end left out reads as a count's zero, as a number does; one written null is its start.
    expect(new Bucket({ start: 5 }).span()).toEqual([5, 0]);
    expect(new Bucket({ start: 5, end: null }).span()).toEqual([5, 5]);
  });

  it("read a figure always written as none where an older server left it out", () => {
    const older = new Bucket({ flow: { entered: { done: 4 } } });
    expect([older.written("flow.done"), older.number("flow.done")]).toEqual([null, 0]);
    expect([older.written("flow.entered.done"), older.number("flow.entered.cancelled")]).toEqual([
      4, 0,
    ]);
    expect(new Bucket({ flow: { done: "3" } }).written("flow.done")).toBeNull();
  });
});

describe("the answer", () => {
  it("is read with what the page draws, the lists it leaves out empty", () => {
    expect(read({ grain: "week", days: 84, from: 1, to: 2 })).toEqual({
      grain: "week",
      days: 84,
      from: 1,
      to: 2,
      buckets: [],
      first: null,
      marks: [],
      unread: [],
    });
    const empty = read(CASES.samples.emptyReports);
    expect([empty.buckets, empty.first]).toEqual([[], null]);
  });

  it.each([
    ["no report", null],
    ["a grain it does not offer", { ...wire(), grain: "month" }],
    ["days in part", { ...wire(), days: 1.5 }],
    ["days below none", { ...wire(), days: -1 }],
    ["no window's start", { ...wire(), from: undefined }],
    ["an end as text", { ...wire(), to: "today" }],
    ["a first roll-up as text", { ...wire(), first: "Monday" }],
    ["periods that are no list", { ...wire(), buckets: {} }],
    ["periods null", { ...wire(), buckets: null }],
    ["a settings change without its commit", { ...wire(), marks: [{ at: 1, subject: "s" }] }],
    ["a file not read named by no text", { ...wire(), unread: [3] }],
  ])("is none for %s", (_, value) => {
    expect(readReports(value)).toBeNull();
  });
});

describe("the charts", () => {
  it("take each series from its source, none where it has no record", () => {
    const drawn = charts(page(), []);
    expect(drawn.map(([heading]) => heading)).toEqual([
      "Flow",
      "Agents",
      "Decisions and code",
      "Quality",
      "Board health",
    ]);
    const open = find(drawn, "open");
    expect(open.note).toBe("Top-level tasks in each open status at the end of each day.");
    expect(open.series[0]?.values).toEqual([2, null, 0]);
    expect([gap(open, 1), gap(open, 2)]).toEqual([true, false]);
    expect(open.cover).toEqual([1, 0, 5 / 11]);
    const working = find(drawn, "agents");
    // Orchestrators, executors, reviewers, the other roles together, each session once.
    expect(firsts(drawn, "agents")).toEqual([1, 2, 0, 0.5, 2.5]);
    expect(working.series[0]?.values[2]).toBeNull();
    const lines = find(drawn, "lines");
    expect(lines.series[1]?.mark).toBe("below");
    expect(lines.series[1]?.values[0]).toBe(300);
    expect(drawn.flatMap(([, section]) => section).every((chart) => chart.instead === null)).toBe(
      true,
    );
    expect(find(charts({ ...page(), grain: "hour" }, []), "lead").instead).toBe(
      "The roll-up keeps lead time by the day: switch to Days or Weeks to see it.",
    );
  });

  it("read the tasks done each once, and a period that did not count them is a gap", () => {
    const done = find(charts(page(), []), "done");
    expect(done.series[0]?.values).toEqual([3, null, 4]);
    expect(done.note).toContain("each once at its last close");
    expect(done.note).toContain("UTC");
    const uncounted = wire();
    for (const one of uncounted.buckets) {
      if (one.flow) (one.flow as Wire).done = null;
    }
    const drawn = charts(read(uncounted), []);
    expect(values(drawn, "done")).toEqual([null, null, null]);
    expect([gap(find(drawn, "done"), 0), gap(find(drawn, "done"), 2)]).toEqual([true, true]);
    expect(figures(read(uncounted))[0]).toEqual({
      label: "Done",
      value: "–",
      detail: "no roll-up in the window counted them",
    });
    ((uncounted.buckets[0] as Wire).flow as Wire).done = 3;
    expect(figures(read(uncounted))[0]).toMatchObject({
      value: "3",
      detail: "3 a day on average · 1 of 3 days counted, 1 rolled up before the count began",
    });
    ((uncounted.buckets[2] as Wire).flow as Wire).done = 4;
    const whole = { ...uncounted, buckets: [uncounted.buckets[0], uncounted.buckets[2]] };
    expect(figures(read(whole))[0]).toMatchObject({ value: "7", detail: "3.5 a day on average" });
  });

  it("draw gaps, never zeros, for an answer without the tasks done", () => {
    const older = wire();
    for (const one of older.buckets) {
      if (one.flow) delete (one.flow as Wire).done;
    }
    const drawn = charts(read(older), []);
    expect(values(drawn, "done")).toEqual([null, null, null]);
    expect(figures(read(older))[0].detail).toBe("no roll-up in the window counted them");
  });

  it("hide each series hidden on its own chart alone, its values kept", () => {
    const hidden = [seriesKey("open", "Ready"), "gone:Ready"];
    const drawn = charts(page(), hidden);
    const names = (id: string) =>
      find(drawn, id)
        .series.filter((data) => data.hidden)
        .map((data) => data.name);
    expect(names("open")).toEqual(["Ready"]);
    expect(names("task-hours")).toEqual([]);
    expect(
      drawn
        .flatMap(([, section]) => section)
        .filter((chart) => chart.id !== "open")
        .every((chart) => chart.series.every((data) => !data.hidden)),
    ).toBe(true);
    expect(values(drawn, "open")).toEqual(values(charts(page(), []), "open"));
  });

  it("read every series where the wire writes it", () => {
    const [written] = (CASES.samples.reports as { buckets: unknown[] }).buckets;
    const one = new Bucket(written);
    for (const [chart, name, path] of paths()) {
      expect(one.written(path), `${chart}: ${name} reads ${path}`).toEqual(expect.any(Number));
    }
    expect(paths()).toHaveLength(38);
    const drawn = charts(read(CASES.samples.reports), []);
    expect(firsts(drawn, "agents")).toEqual([1.5, 1.5, 1.5, 0, 2.5]);
    expect(values(drawn, "health", 2)).toEqual([3]);
  });

  it("read the wire's own fixture", () => {
    const shown = read(fixture);
    const drawn = charts(shown, []);
    expect(values(drawn, "done")?.[0]).toBe(5);
    expect(values(drawn, "task-hours", 3)?.[0]).toBe(0);
    expect(firsts(drawn, "agents")).toEqual([0, 2.125, 0.75, 0, 2.875]);
    expect(values(drawn, "health", 2)?.[0]).toBe(12);
    expect(values(drawn, "waits", 1)?.[0]).toBeNull();
    expect(figures(shown)[0].value).toBe("5");
  });
});

describe("the axis", () => {
  it("names a period by its UTC day, week or hour", () => {
    expect(period("day", MONDAY)).toBe("Mon 9 Nov");
    expect(period("week", MONDAY)).toBe("Week of 9 Nov");
    expect(period("hour", MONDAY + 14 * HOUR)).toBe("Mon 9 Nov, 14:00 UTC");
    expect(period("day", MONDAY - DAY)).toBe("Sun 8 Nov");
    expect(period("day", MONDAY + 23 * DAY)).toBe("Wed 2 Dec");
  });

  it("labels at most six periods and places each settings change", () => {
    const shown = axis(page());
    expect(shown.periods).toEqual(["Mon 9 Nov", "Tue 10 Nov", "Wed 11 Nov"]);
    expect(shown.hours).toEqual([24, 24, 11]);
    expect(shown.ticks).toEqual([
      [0, "9 Nov"],
      [1, "10 Nov"],
      [2, "11 Nov"],
    ]);
    expect(shown.marks).toHaveLength(1);
    const [left, at, text] = shown.marks[0] ?? [];
    expect([at, text]).toEqual([0, "settings: land without the check (4f1c0a9b2d3e)"]);
    expect(left).toBeCloseTo(25 / 3, 9);
    const outside = { ...wire(), marks: [{ at: MONDAY - DAY, commit: "c", subject: "s" }] };
    expect(axis(read(outside)).marks).toEqual([]);
    const fortnight = {
      ...wire(),
      days: 14,
      buckets: Array.from({ length: 14 }, (_, n) =>
        bucket(MONDAY + n * DAY, MONDAY + (n + 1) * DAY),
      ),
    };
    expect(axis(read(fortnight)).ticks.map(([at]) => at)).toEqual([1, 4, 7, 10, 13]);
  });

  it("labels the clock's round hours by the hour", () => {
    const hours = (count: number, first: number) =>
      axis(
        read({
          ...wire(),
          grain: "hour",
          buckets: Array.from({ length: count }, (_, n) =>
            bucket(first + n * HOUR, first + (n + 1) * HOUR),
          ),
        }),
      ).ticks.map(([at, label]) => `${at} ${label}`);
    expect(hours(168, MONDAY)).toEqual(["0 9 Nov", "48 11 Nov", "96 13 Nov", "144 15 Nov"]);
    expect(hours(48, MONDAY + 5 * HOUR)).toEqual(["7 12:00", "19 10 Nov", "31 12:00", "43 11 Nov"]);
    expect(hours(24, MONDAY + 11 * HOUR)).toEqual([
      "1 12:00",
      "5 16:00",
      "9 20:00",
      "13 10 Nov",
      "17 04:00",
      "21 08:00",
    ]);
    expect(hours(400, MONDAY)).toEqual([
      "0 9 Nov",
      "48 11 Nov",
      "96 13 Nov",
      "144 15 Nov",
      "192 17 Nov",
      "240 19 Nov",
      "288 21 Nov",
      "336 23 Nov",
      "384 25 Nov",
    ]);
  });

  it("starts at the project's first roll-up, a gap after it kept", () => {
    expect(weekdays(read(wide(MONDAY)))).toEqual(["Sat", "Sun", "Mon", "Tue", "Wed"]);
    const trimmed = fromFirstRecord(read(wide(MONDAY)));
    expect(weekdays(trimmed)).toEqual(["Mon", "Tue", "Wed"]);
    expect(trimmed.buckets).toEqual(page().buckets);
    expect(view(read(wide(MONDAY)))).toContain("3.5 a day on average · 2 of 3 days counted");
    // Only a period wholly before the first file goes: one it begins inside stays, a gap.
    expect(weekdays(fromFirstRecord(read(wide(MONDAY - 12 * HOUR))))).toEqual([
      "Sun",
      "Mon",
      "Tue",
      "Wed",
    ]);
    // Sunday's agents, before the first roll-up, are a record of it.
    const watched = wide(MONDAY);
    (watched.buckets[1] as Wire).agents = mondayAgents();
    expect(weekdays(fromFirstRecord(read(watched)))).toEqual(["Sun", "Mon", "Tue", "Wed"]);
    for (const first of [null, MONDAY + 30 * DAY]) {
      expect(fromFirstRecord(read(unrecorded(wide(first)))).buckets).toHaveLength(5);
    }
  });

  it("keeps a gap after the roll-up began however early in the window", () => {
    const early = {
      ...wide(MONDAY - 30 * DAY),
      unread: ["2026-11-07", "2026-11-08"].map(
        (day) => `/repo/.git/symphony/reports/${day}.json: not read`,
      ),
    };
    const kept = fromFirstRecord(read(early));
    expect(weekdays(kept)).toEqual(["Sat", "Sun", "Mon", "Tue", "Wed"]);
    expect(gap(find(charts(kept, []), "done"), 0)).toBe(true);
    expect(axis(kept).ticks.map(([, label]) => label)).toEqual([
      "7 Nov",
      "8 Nov",
      "9 Nov",
      "10 Nov",
      "11 Nov",
    ]);
    const html = view(read(early));
    expect(html).toContain("2 files not read: their hours show as gaps");
    expect(html).toContain("3.5 a day on average · 2 of 5 days counted");
  });
});

describe("the figures", () => {
  it("sum the window and say what is missing", () => {
    const [done, lead, working, decisions] = figures(page());
    expect([done.value, done.detail]).toEqual(["7", "3.5 a day on average · 2 of 3 days counted"]);
    expect([lead.value, lead.detail]).toEqual(["50m", "median of 3 closed · p90 up to 2h"]);
    expect([working.value, working.detail]).toEqual(["2.5", "running on average · 3 at most"]);
    expect([decisions.value, decisions.detail]).toEqual([
      "2",
      "2 auto-approved · 2 answered in the window",
    ]);
    const empty = read({ ...(CASES.samples.emptyReports as Wire), buckets: [bucket(0, 0)] });
    expect(figures(empty).map((figure) => [figure.value, figure.detail])).toEqual([
      ["–", "no roll-up in the window"],
      ["–", "no task closed in the window"],
      ["–", "no agents' hours in the window"],
      ["–", "no cards read in the window"],
    ]);
    expect(figures({ ...page(), grain: "hour" })[1].detail).toBe("kept by the day");
  });

  it("each name the chart it sums up, one the page draws", () => {
    const linked = linkedFigures(page());
    expect(linked.map(([figure, chart]) => [figure.label, chart])).toEqual([
      ["Done", "done"],
      ["Lead time", "lead"],
      ["Agents at work", "agents"],
      ["Open decisions", "decisions"],
    ]);
    expect(linked.map(([figure]) => figure)).toEqual(figures(page()));
    const titles = new Map(
      SECTIONS.flatMap(([, specs]) => specs.map((spec): [string, string] => [spec.id, spec.title])),
    );
    expect(linked.map(([, chart]) => titles.get(chart))).toEqual([
      "Tasks done",
      "Lead time",
      "Agents at work",
      "Decisions",
    ]);
  });
});

describe("a figure's link", () => {
  it("opens the chart it sums up, and none while no chart is drawn", () => {
    const html = view(page());
    expect(html.match(/<dt><a href="#chart-[a-z]+">/g)).toEqual([
      '<dt><a href="#chart-done">',
      '<dt><a href="#chart-lead">',
      '<dt><a href="#chart-agents">',
      '<dt><a href="#chart-decisions">',
    ]);
    // A chart still to mount holds its id in its ghost, so the link finds its place.
    expect(html).toContain('<span id="chart-decisions"></span><div class="ghost-chart"');
    for (const [, chart] of linkedFigures(page())) {
      expect(html.match(new RegExp(`id="chart-${chart}"`, "g"))).toHaveLength(1);
    }
    const nothing = view(read(unrecorded(wire())));
    expect(nothing).toContain("<dt>Done</dt>");
    expect(nothing).not.toContain("href=");
    expect(view(read({ ...unrecorded(wire()), first: null }))).not.toContain("href=");
  });

  it("is the address of a chart drawn at once, which the shell then lands, the rest to come", () => {
    vi.stubGlobal("location", { hash: "#chart-decisions" });
    const html = view(page());
    // The chart the address names is drawn as the report is, wherever it is on the page.
    expect(html).toMatch(/<figure class="chart" id="chart-decisions" aria-labelledby=/);
    expect(html.match(/id="chart-decisions"/g)).toHaveLength(1);
    // Every other chart is its ghost until it comes near the screen, its id held there.
    expect(html.match(/<figure class="chart"/g)).toHaveLength(1);
    expect(html).toContain('<span id="chart-done"></span><div class="ghost-chart"');
    // An address that names no chart draws none at once.
    vi.stubGlobal("location", { hash: "#flow" });
    expect(view(page())).not.toContain('<figure class="chart"');
  });

  it("lands no chart once the reader chose a window, its id still held in its ghost", () => {
    vi.stubGlobal("location", { hash: "#chart-decisions" });
    const html = renderToStaticMarkup(
      <ReportsView grain="day" days={3} page={page()} onAsk={() => {}} lands={false} />,
    );
    expect(html).not.toContain('<figure class="chart"');
    expect(html).toContain('<span id="chart-decisions"></span><div class="ghost-chart"');
  });
});

describe("the page's view", () => {
  it("is its ghost before the first answer, the window chosen marked", () => {
    const html = view(null);
    expect(html).toContain('aria-busy="true"');
    expect(html).toContain('data-ready="loading"');
    expect(html).toContain('<p class="status-text" role="status">Loading the reports…</p>');
    expect(html).toContain("Days · the last 3 days · times in UTC");
    expect(html).toContain('aria-pressed="true" data-grain="day"');
    expect(html).toContain('aria-pressed="false" data-days="7"');
    expect(html.match(/class="ghost-chart"/g)).toHaveLength(2);
    expect(html.match(/class="ghost-figure"/g)).toHaveLength(4);
    expect(html).not.toContain("chart-slot");
  });

  it("says why a read failed and keeps what it showed", () => {
    const why = "The server could not be reached";
    let html = view(null, why);
    expect(html).toContain('data-ready="failed"');
    expect(html).toContain(
      'role="alert">The reports could not be read. The server could not be reached; check the connection. Trying again…</p>',
    );
    html = view(page(), why);
    expect(html).toContain('data-ready="reports"');
    expect(html).toContain("The reports could not be read");
    expect(html).toContain("chart-slot");
  });

  it("writes its window and a failure each as one text, as the Dioxus page does", () => {
    // Text set as one run is shaped as one: the browser draws it as it drew the Dioxus page's.
    const drawn = ReportsView({
      grain: "day",
      days: 3,
      page: null,
      error: "The server could not be reached",
      onAsk: () => {},
    });
    expect(childrenOf(drawn, "reports-sub")).toBe("Days · the last 3 days · times in UTC");
    expect(childrenOf(drawn, "reports-error")).toBe(
      "The reports could not be read. The server could not be reached; check the connection. Trying again…",
    );
  });

  it("draws the figures, the settings changes and a place for every chart", () => {
    const html = view(page());
    expect(html).toContain('aria-busy="false"');
    expect(html).toContain('data-ready="reports"');
    expect(html).toContain('<dt><a href="#chart-done">Done</a></dt><dd class="value">7</dd>');
    expect(html).toContain(
      '<div class="reports-marks"><ul aria-label="Settings changes in the window"><li>Mon 9 Nov: ' +
        "settings changed, settings: land without the check (4f1c0a9b2d3e)</li></ul></div>",
    );
    // Each chart mounts once it comes near the screen, its ghost in its place until then.
    expect(html.match(/class="viewport-slot chart-slot"/g)).toHaveLength(14);
    expect(html).toContain('<section class="reports-section" aria-label="Board health"><h2>');
    expect(html).not.toContain("No roll-up yet");
    expect(html).not.toContain("not read");
  });

  it("lists the settings changes newest first, folding all but the newest three", () => {
    // `count` changes on Monday, the `n`th at `n + 1` o'clock, oldest first as the server sends them.
    const changes = (count: number) =>
      view(
        read({
          ...wire(),
          marks: Array.from({ length: count }, (_, n) => ({
            at: MONDAY + (n + 1) * HOUR,
            commit: `c${n}`,
            subject: `change ${n}`,
          })),
        }),
      );
    const items = (...ns: number[]) =>
      ns.map((n) => `<li>Mon 9 Nov: settings changed, change ${n} (c${n})</li>`).join("");
    const list = '<div class="reports-marks"><ul aria-label="Settings changes in the window">';
    expect(changes(3)).toContain(`${list}${items(2, 1, 0)}</ul></div>`);
    expect(changes(4)).toContain(
      `${list}${items(3, 2, 1)}</ul><details><summary>1 more settings change</summary>` +
        `<ul>${items(0)}</ul></details></div>`,
    );
    expect(changes(10)).toContain(
      `${list}${items(9, 8, 7)}</ul><details><summary>7 more settings changes</summary>` +
        `<ul>${items(6, 5, 4, 3, 2, 1, 0)}</ul></details></div>`,
    );
    expect(changes(0)).not.toContain("reports-marks");
  });

  it("is busy while the window asked for is not the one shown", () => {
    const html = renderToStaticMarkup(
      <ReportsView grain="week" days={84} page={page()} onAsk={() => {}} />,
    );
    expect(html).toContain('aria-busy="true"');
    expect(html).toContain('data-ready="reports"');
    expect(html).toContain('aria-pressed="true" data-days="84"');
  });

  it("says when the first roll-up comes while there is none", () => {
    const empty = { ...unrecorded(wire()), first: null };
    let html = view(read(empty));
    expect(html).toContain('<section class="reports-empty" aria-label="No roll-up yet">');
    expect(html).toContain("the next is due by 11:00 UTC");
    expect(html).not.toContain("chart-slot");
    expect(firstRollUp(read({ ...empty, to: MONDAY + 9 * HOUR + 59 }))).toContain("by 09:00 UTC");
    // The agents recorded before the first roll-up still draw.
    (empty.buckets[0] as Wire).agents = mondayAgents();
    html = view(
      read({
        ...empty,
        unread: ["/repo/.git/symphony/reports/2026-11-10.json: not a regular file"],
      }),
    );
    expect(html).toContain("No roll-up yet.");
    expect(html).toContain("chart-slot");
    expect(html).toContain("1 file not read: their hours show as gaps");
    expect(html).toContain(
      "<li>/repo/.git/symphony/reports/2026-11-10.json: not a regular file</li>",
    );
  });
});

describe("the window and the series hidden", () => {
  it("are this browser's when the page offers them", () => {
    const saved = (grain: string, days: string) => (key: string) =>
      ({ "reports-grain": grain, "reports-days": days })[key] ?? null;
    expect(chosen(() => null)).toEqual(["hour", 1]);
    expect(FIRST_WINDOW).toEqual(["hour", 1]);
    expect(ranges(FIRST_WINDOW[0])).toContain(FIRST_WINDOW[1]);
    expect(chosen(saved("hour", "7"))).toEqual(["hour", 7]);
    expect(chosen(saved("day", "14"))).toEqual(["day", 14]);
    expect(chosen(saved("week", "84"))).toEqual(["week", 84]);
    expect(chosen(saved("week", "+28"))).toEqual(["week", 28]);
    expect(chosen(saved("hour", "90"))).toEqual(["hour", 2]);
    expect(chosen(saved("day", "x"))).toEqual(["day", 14]);
    expect(chosen(saved("day", "7.0"))).toEqual(["day", 14]);
    expect(chosen(saved("month", "x"))).toEqual(["hour", 1]);
    expect(chosen(saved("month", "14"))).toEqual(["hour", 1]);
  });

  it("are kept a key a line, and a toggle hides one or shows it again", () => {
    expect(hiddenSeries(() => null)).toEqual([]);
    const kept = (value: string) => (key: string) => (key === HIDDEN ? value : null);
    expect(hiddenSeries(kept("open:Ready\n\ntask-hours:Blocked\r\n"))).toEqual([
      "open:Ready",
      "task-hours:Blocked",
    ]);
    expect(hiddenSeries(kept(""))).toEqual([]);
    let hidden = toggle([], seriesKey("open", "Ready"));
    expect(hidden).toEqual(["open:Ready"]);
    hidden = toggle(hidden, seriesKey("agents", "Sessions, each once"));
    expect(hidden).toEqual(["open:Ready", "agents:Sessions, each once"]);
    expect(hiddenSeries(() => hidden.join("\n"))).toEqual(hidden);
    hidden = toggle(hidden, seriesKey("open", "Ready"));
    expect(hidden).toEqual(["agents:Sessions, each once"]);
    expect(toggle(hidden, "agents:Sessions, each once")).toEqual([]);
  });

  it("open the page on the window this browser chose, waiting for its answer", () => {
    vi.stubGlobal("localStorage", memoryStorage());
    localStorage.setItem("reports-grain", "week");
    localStorage.setItem("reports-days", "28");
    const html = renderToStaticMarkup(<ReportsPage />);
    expect(html).toMatch(/^<main class="reports-page"><div class="reports" data-ready="loading"/);
    expect(html).toContain("Weeks · the last 4 weeks · times in UTC");
  });
});

describe("the reports' read", () => {
  /** A read of the window whose waits are recorded, the last ending it, and what it was told. */
  const follow = (grain: Grain, days: number, waits: number, page = new AbortController()) => {
    const asked = server();
    const told: unknown[] = [];
    const waited: number[] = [];
    const tags: string[] = [];
    const done = followReports(
      grain,
      days,
      () => {
        tags.push(`kept-${tags.length}`);
        return tags.at(-1) ?? "";
      },
      page.signal,
      {
        answered: (shown, etag) => told.push(["answered", shown?.days ?? null, etag]),
        failed: (why) => told.push(["failed", why]),
      },
      async (ms, signal) => {
        expect(signal).toBe(page.signal);
        waited.push(ms);
        if (waited.length === waits) page.abort();
      },
    );
    return { asked, told, waited, tags, done };
  };

  it("asks for the window at once and again on its timer, naming the copy shown", async () => {
    const { asked, told, waited, tags, done } = follow("day", 14, 6);
    await asking(asked, 1);
    expect(asked[0]?.url).toBe("/api/reports?grain=day&days=14");
    asked[0]?.answer(reply(200, { ...wire(), days: 14 }, "r1"));
    await asking(asked, 2);
    asked[1]?.answer(new Response(null, { status: 304 }));
    await asking(asked, 3);
    asked[2]?.answer(reply(503, {}));
    await asking(asked, 4);
    asked[3]?.answer(reply(200, { grain: "month" }, "r2"));
    await asking(asked, 5);
    asked[4]?.answer(reply(200, { ...wire(), days: 14 }, "r3"));
    await asking(asked, 6);
    asked[5]?.answer(reply(503, {}));
    await done;
    expect(told).toEqual([
      ["answered", 14, "r1"],
      ["answered", null, ""],
      ["failed", "The server answered 503"],
      ["failed", "The reports' answer is not a report"],
      ["answered", 14, "r3"],
      ["failed", "The server answered 503"],
    ]);
    // A read that failed is tried again on the feed's backoff, not the timer, and an answer puts
    // the timer back, the backoff after a failure starting again.
    const backoff = new Backoff();
    const first = backoff.after(0);
    expect(waited).toEqual([EVERY_MS, EVERY_MS, first, backoff.after(0), EVERY_MS, first]);
    expect(tags).toHaveLength(6);
  });

  it("stops, telling nothing and its read given up, once the page that asked is gone", async () => {
    const page = new AbortController();
    const { asked, told, waited, done } = follow("hour", 1, 1, page);
    await asking(asked, 1);
    expect(asked[0]?.signal?.aborted).toBe(false);
    page.abort();
    expect(asked[0]?.signal?.aborted).toBe(true);
    asked[0]?.answer(reply(200, wire(), "r1"));
    await done;
    expect(told).toEqual([]);
    expect(waited).toEqual([]);
    const failing = new AbortController();
    const second = follow("hour", 1, 1, failing);
    await asking(second.asked, 1);
    failing.abort();
    second.asked[0]?.answer(reply(503, {}));
    await second.done;
    expect(second.told).toEqual([]);
  });
});
