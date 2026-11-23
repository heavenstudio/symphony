import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import { seriesKey } from "./chart";
import {
  DEAREST,
  dearestChart,
  FigureCards,
  kindColour,
  kindName,
  kindsChart,
  LandedView,
  LEFT_OUT,
  landedEmpty,
  landedFigures,
  landedIntro,
  linesSaid,
  per100,
  readLanded,
  reviewsSaid,
  roundsAxis,
  roundsChart,
  spreadChart,
  wholeDollars,
} from "./reports-landed";
import { CASES } from "./testing";
import type { LandedPage, LandedRow } from "./wire";

type Wire = Record<string, unknown>;

/** Monday 2026-11-09, 00:00 UTC. */
const MONDAY = 1_794_182_400;

/**
 * The wire's own sample: a feature with some tokens unpriced, reviewed twice, its 400 lines read; a
 * docs task landed before lines were kept; a task given no kind that changed no counted line; one
 * more landed with no tokens recorded, and a day file that did not read.
 */
const sample = () => structuredClone(CASES.samples.landed) as LandedPage;

function read(value: unknown): LandedPage {
  const page = readLanded(JSON.parse(JSON.stringify(value)));
  if (page === null) throw new Error("The tab reads no landed tasks in it");
  return page;
}

/** A landed task as the server writes its row, `fields` over a priced feature of 100 lines. */
function task(slug: string, fields: Partial<LandedRow> = {}): LandedRow {
  return {
    slug,
    at: MONDAY,
    kind: "feature",
    first: 1,
    later: 0,
    tokens: 1000,
    unpriced: false,
    reviews: 1,
    lines: 100,
    ...fields,
  };
}

/** `tasks` as the tab's whole read, its groups and figures those of the sample. */
const withTasks = (tasks: LandedRow[]): LandedPage => ({ ...read(sample()), tasks });

/** Markup's text alone, its spaces one. */
const text = (html: string) =>
  html
    .replace(/<[^>]+>/g, " ")
    .replace(/\s+/g, " ")
    .trim();

afterEach(() => vi.unstubAllGlobals());

describe("the landed tasks' read", () => {
  it("reads the wire's own samples as written", () => {
    expect(read(sample())).toEqual(CASES.samples.landed);
    expect(read(CASES.samples.emptyLanded)).toEqual(CASES.samples.emptyLanded);
  });

  it("reads a member left out as its zero, its empty list or none, and one added as nothing", () => {
    expect(read({ grain: "week", days: 84, from: 1, to: 2 })).toEqual({
      grain: "week",
      days: 84,
      from: 1,
      to: 2,
      tasks: [],
      without: 0,
      figures: {
        tasks: 0,
        back: 0,
        lined: 0,
        lines: 0,
        unlined: 0,
        unread: 0,
        cost: 0,
        later: 0,
        lined_cost: 0,
        median: null,
        mean: null,
        p90: null,
      },
      kinds: [],
      reviews: [],
      kinds_from: null,
      prices: "",
      unread: [],
    });
    const short = read({
      ...sample(),
      tasks: [{ slug: "a/b", at: 1 }],
      kinds: [{}],
      reviews: [{}],
    });
    expect(short.tasks).toEqual([
      {
        slug: "a/b",
        at: 1,
        kind: null,
        first: 0,
        later: 0,
        tokens: 0,
        unpriced: false,
        reviews: 0,
        lines: null,
      },
    ]);
    expect(short.kinds).toEqual([
      { kind: null, tasks: 0, median: null, cost: 0, lined_cost: 0, lines: 0 },
    ]);
    expect(short.reviews).toEqual([{ reviews: 0, tasks: 0, first: 0, later: 0, median: null }]);
    const whole = sample();
    const newer = { ...whole, newer: 1, tasks: whole.tasks.map((row) => ({ ...row, newer: 2 })) };
    expect(read(newer)).toEqual(CASES.samples.landed);
  });

  const row = (wrong: Wire) => ({ tasks: [{ ...sample().tasks[0], ...wrong }] });
  it.each<[string, Wire | null]>([
    ["no value", null],
    ["a grain it does not offer", { grain: "month" }],
    ["days in part", { days: 1.5 }],
    ["a start as text", { from: "Monday" }],
    ["no end", { to: undefined }],
    ["those without tokens below none", { without: -1 }],
    ["tasks that are no list", { tasks: {} }],
    ["a task with no slug", row({ slug: undefined })],
    ["a task landed at no time", row({ at: "today" })],
    ["a kind that is no text", row({ kind: 3 })],
    ["lines in part", row({ lines: 1.5 })],
    ["lines below none", row({ lines: -1 })],
    ["unpriced as text", row({ unpriced: "yes" })],
    ["reviews below none", row({ reviews: -1 })],
    ["a cost as text", row({ first: "$2" })],
    ["a median as text", { figures: { median: "1" } }],
    ["a count of tasks in part", { figures: { tasks: 1.5 } }],
    ["a cost in all as text", { figures: { cost: "5" } }],
    ["kinds that are no list", { kinds: "feature" }],
    ["a kind named by no text", { kinds: [{ kind: 3 }] }],
    ["a kind's median as text", { kinds: [{ kind: "fix", median: "1" }] }],
    ["review rounds that are no list", { reviews: {} }],
    ["a review round's cost as text", { reviews: [{ reviews: 1, first: "1" }] }],
    ["files not read named by no text", { unread: [3] }],
    ["files not read as no list", { unread: "x" }],
    ["the first kind's day as text", { kinds_from: "Monday" }],
    ["prices that are no text", { prices: 16 }],
  ])("is none for %s", (_, wrong) => {
    expect(readLanded(wrong === null ? null : { ...sample(), ...wrong })).toBeNull();
  });
});

describe("the words", () => {
  it("say dollars whole, reviews and lines counted, and what 100 lines cost", () => {
    expect([wholeDollars(1874.4), wholeDollars(1.5), wholeDollars(0)]).toEqual([
      "$1,874",
      "$2",
      "$0",
    ]);
    expect([reviewsSaid(0), reviewsSaid(1), reviewsSaid(3)]).toEqual([
      "no review",
      "1 review",
      "3 reviews",
    ]);
    expect([linesSaid(null), linesSaid(0), linesSaid(1), linesSaid(1200)]).toEqual([
      "lines not read",
      "no line counted",
      "1 line",
      "1,200 lines",
    ]);
    expect([per100(3.5, 400), per100(1, 0), per100(1, null)]).toEqual([0.875, null, null]);
  });

  it("colour each kind its own, the same on every chart, and none or one unknown grey", () => {
    expect(["feature", "fix", "red", "tests", "docs"].map(kindColour)).toEqual([
      "var(--chart-1)",
      "var(--chart-3)",
      "var(--chart-2)",
      "var(--chart-4)",
      "var(--chart-5)",
    ]);
    expect([kindColour(null), kindColour("chore")]).toEqual([
      "var(--chart-none)",
      "var(--chart-none)",
    ]);
    expect([kindName("fix"), kindName(null)]).toEqual(["fix", "No kind"]);
  });

  it("open the tab saying what it counts, and say why it has nothing to draw", () => {
    expect(landedIntro(read(sample()))).toBe(
      "The 3 top-level tasks landed in the window with tokens recorded, each priced once with " +
        "its subtasks, at its landing: by task, not by day.",
    );
    expect(landedIntro({ ...withTasks([task("a/b")]), grain: "week" })).toBe(
      "The one top-level task landed in the window with tokens recorded, each priced once with " +
        "its subtasks, at its landing: by task, not by week.",
    );
    const empty = read(CASES.samples.emptyLanded);
    expect(landedEmpty(empty)).toBe("No top-level task landed in the window.");
    expect(landedEmpty({ ...empty, without: 2 })).toBe(
      "2 top-level tasks landed in the window, none with tokens recorded: no figure to draw.",
    );
    expect(landedEmpty({ ...empty, without: 1 })).toContain("1 top-level task landed");
  });
});

describe("the figures", () => {
  it("count the tasks landed, the median a task, 100 lines' cost and those back after review", () => {
    expect(landedFigures(read(sample()))).toEqual([
      [
        {
          label: "Landed",
          value: "3",
          detail: "with tokens recorded · 1 without, in no figure",
        },
        "dearest",
      ],
      [{ label: "Median a task", value: "$1.00", detail: "mean $1.67 · p90 $3.00" }, "kinds"],
      [
        {
          label: "Per 100 lines changed",
          value: "$0.88",
          detail: "1 task, 400 lines · 1 changed none counted · 1 with no lines read",
        },
        "spread",
      ],
      [
        {
          label: "Back after review",
          value: "1",
          detail: "of 3 needed a second review · review rounds took $2 of $5",
        },
        "rounds",
      ],
    ]);
  });

  it("say what is missing where no task landed, none had tokens, a price or lines read", () => {
    const empty = read(CASES.samples.emptyLanded);
    expect(landedFigures(empty).map(([figure]) => [figure.value, figure.detail])).toEqual([
      ["–", "no task landed in the window"],
      ["–", "no task landed with tokens"],
      ["–", "no task landed with tokens"],
      ["–", "no task landed with tokens"],
    ]);
    expect(landedFigures({ ...empty, without: 3 })[0]?.[0]).toEqual({
      label: "Landed",
      value: "0",
      detail: "with tokens recorded · 3 without, in no figure",
    });
    // Two landed, one priced; no landing's lines read.
    const whole = read(sample());
    const part = {
      ...whole,
      without: 0,
      tasks: [task("a/b", { lines: null }), task("c/d", { lines: null, unpriced: true })],
      figures: { ...whole.figures, tasks: 1, lined: 0, lined_cost: 0, lines: 0 },
    };
    const [landed, median, lines] = landedFigures(part).map(([figure]) => figure);
    expect(landed?.detail).toBe("with tokens recorded · none without");
    expect(median?.detail).toBe("mean $1.67 · p90 $3.00 · 1 unpriced, in no figure");
    expect(lines).toEqual({
      label: "Per 100 lines changed",
      value: "–",
      detail: "no landing's lines read in the window",
    });
    expect(landedFigures({ ...part, figures: { ...part.figures, median: null } })[1]?.[0]).toEqual({
      label: "Median a task",
      value: "–",
      detail: "no task's tokens priced",
    });
    // Every line counted: nothing more said after the count.
    const counted = { ...whole.figures, unlined: 0, unread: 0 };
    expect(landedFigures({ ...whole, figures: counted })[2]?.[0].detail).toBe("1 task, 400 lines");
  });

  it("are each a link to the chart they sum up while the charts are drawn", () => {
    const figures = landedFigures(read(sample()));
    const linked = renderToStaticMarkup(<FigureCards figures={figures} linked />);
    expect(linked.match(/<dt><a href="#chart-[a-z]+">/g)).toEqual([
      '<dt><a href="#chart-dearest">',
      '<dt><a href="#chart-kinds">',
      '<dt><a href="#chart-spread">',
      '<dt><a href="#chart-rounds">',
    ]);
    expect(linked).toContain('<dd class="value">$1.00</dd>');
    const plain = renderToStaticMarkup(<FigureCards figures={figures} linked={false} />);
    expect(plain).toContain("<dt>Landed</dt>");
    expect(plain).not.toContain("href=");
  });
});

describe("the dearest tasks", () => {
  it("are every task as a bar, its first round and its review rounds stacked, its name a link", () => {
    const chart = dearestChart(read(sample()), []);
    expect(chart.note).toMatch(/^Every top-level task landed in the window, the dearest first, /);
    expect(chart.series).toEqual([
      { name: "First round", hidden: false },
      { name: "Review rounds", hidden: false },
    ]);
    expect(chart.side).toBe(false);
    expect(chart.rows.map((row) => [row.label, row.href, row.values, row.said])).toEqual([
      [
        "app/charts",
        "/task/app/charts",
        [2, 1.5],
        ["120k tokens", "2 reviews", "400 lines", "some tokens unpriced, in no dollar"],
      ],
      ["docs/guide", "/task/docs/guide", [1, 0], ["120k tokens", "no review", "lines not read"]],
      [
        "generated/outputs",
        "/task/generated/outputs",
        [0.5, 0],
        ["120k tokens", "no review", "no line counted"],
      ],
    ]);
    expect(chart.tableToggle).toBe("Show all 3 as a table");
  });

  it("are the ten dearest past ten, their table listing every one", () => {
    const tasks = Array.from({ length: 12 }, (_, at) => task(`t/${at}`, { first: 12 - at }));
    const chart = dearestChart(withTasks(tasks), [seriesKey("dearest", "Review rounds")]);
    expect(chart.rows).toHaveLength(DEAREST);
    expect(chart.note).toMatch(
      /^The 10 top-level tasks landed in the window that cost the most, each with its subtasks/,
    );
    expect(chart.series[1]?.hidden).toBe(true);
    expect(chart.tableToggle).toBe("Show all 12 as a table");
    expect(chart.table().rows).toHaveLength(12);
  });

  it("list each task in their table with its kind, its parts, tokens, reviews and lines", () => {
    const table = dearestChart(read(sample()), []).table();
    expect(table.caption).toBe("Every task landed in the window, the dearest first");
    expect(table.head).toEqual([
      "Task",
      "Kind",
      "Cost",
      "First round",
      "Review rounds",
      "Tokens",
      "Reviews",
      "Lines",
      "Per 100 lines",
    ]);
    expect(table.rows.map((row) => [row.href, row.cells])).toEqual([
      [
        "/task/app/charts",
        [
          "app/charts",
          "feature",
          "$3.50 + unpriced",
          "$2.00",
          "$1.50",
          "120k",
          "2",
          "400",
          "$0.88",
        ],
      ],
      [
        "/task/docs/guide",
        ["docs/guide", "docs", "$1.00", "$1.00", "$0.00", "120k", "0", "–", "–"],
      ],
      [
        "/task/generated/outputs",
        ["generated/outputs", "none", "$0.50", "$0.50", "$0.00", "120k", "0", "0", "–"],
      ],
    ]);
    // A task none of whose tokens had a price costs no dollar.
    const unpriced = withTasks([task("a/b", { first: 0, unpriced: true })]);
    expect(dearestChart(unpriced, []).table().rows[0]?.cells[2]).toBe("unpriced");
  });
});

describe("the kinds", () => {
  it("are each a bar of its median in its own colour, No kind grey and last", () => {
    const chart = kindsChart(read(sample()));
    expect(chart.side).toBe(true);
    expect(chart.series).toEqual([{ name: "Median a task", hidden: false }]);
    expect(
      chart.rows.map((row) => [row.label, row.href, row.values, row.colour, row.meta]),
    ).toEqual([
      ["feature", null, [3.5], "var(--chart-1)", "1 task · $3.50 in all · $0.88 per 100 lines"],
      ["docs", null, [1], "var(--chart-5)", "1 task · $1.00 in all · no lines counted"],
      ["No kind", null, [0.5], "var(--chart-none)", "1 task · $0.50 in all · no lines counted"],
    ]);
    expect(chart.note).toContain("a task landed before kinds were kept counts under No kind.");
    expect(chart.note).toMatch(/ The first with a kind landed 9 Nov 2026\.$/);
    expect(chart.table()).toEqual({
      caption: "By kind",
      head: ["Kind", "Tasks", "Median a task", "In all", "Per 100 lines"],
      rows: [
        { key: "feature", href: null, cells: ["feature", "1", "$3.50", "$3.50", "$0.88"] },
        { key: "docs", href: null, cells: ["docs", "1", "$1.00", "$1.00", "–"] },
        { key: "No kind", href: null, cells: ["No kind", "1", "$0.50", "$0.50", "–"] },
      ],
    });
  });

  it("say when no task had a kind, nothing of the day when every one had, and none unpriced", () => {
    const whole = read(sample());
    const none = kindsChart({ ...whole, kinds_from: null });
    expect(none.note).toMatch(/ No task landed in the window was filed with a kind\.$/);
    const kinded = { ...whole, kinds: whole.kinds.filter((kind) => kind.kind !== null) };
    expect(kindsChart(kinded).note).toMatch(/counts under No kind\.$/);
    const unpriced = {
      ...whole,
      kinds: [{ ...(whole.kinds[0] as LandedPage["kinds"][number]), median: null }],
    };
    const chart = kindsChart(unpriced);
    expect(chart.rows[0]?.values).toEqual([0]);
    expect(chart.table().rows[0]?.cells[2]).toBe("–");
  });
});

describe("the review rounds", () => {
  it("are a column a number of reviews, the mean's first round and review rounds stacked", () => {
    const page = read(sample());
    const chart = roundsChart(page, []);
    expect(chart.series.map((series) => [series.name, series.mark, series.values])).toEqual([
      ["First round", "bar", [0.75, 2]],
      ["Review rounds", "bar", [0, 1.5]],
    ]);
    expect(chart.cover).toEqual([1, 1]);
    expect(chart.instead).toBeNull();
    expect(chart.categories).toEqual([
      ["No review", "2 tasks"],
      ["2 reviews", "1 task"],
    ]);
    expect(chart.extras).toEqual([["median $0.75 a task"], ["median $3.50 a task"]]);
    expect(roundsAxis(page).periods).toEqual([
      "Landed with no review · 2 tasks",
      "Landed after 2 reviews · 1 task",
    ]);
  });

  it("group four reviews or more, and say no median where none was priced", () => {
    const page = {
      ...read(sample()),
      reviews: [
        { reviews: 1, tasks: 1, first: 1, later: 0, median: 1 },
        { reviews: 4, tasks: 3, first: 2, later: 6, median: null },
      ],
    };
    const chart = roundsChart(page, [seriesKey("rounds", "First round")]);
    expect(chart.categories).toEqual([
      ["1 review", "1 task"],
      ["4 or more", "3 tasks"],
    ]);
    expect(chart.extras).toEqual([["median $1.00 a task"], []]);
    expect(chart.series.map((series) => series.hidden)).toEqual([true, false]);
    expect(roundsAxis(page).periods).toEqual([
      "Landed after 1 review · 1 task",
      "Landed after 4 reviews or more · 3 tasks",
    ]);
  });
});

describe("every task's cost against its size", () => {
  it("is a dot a task with lines and a cost, saying which it leaves out", () => {
    const chart = spreadChart(read(sample()), []);
    expect(chart.note).toMatch(
      / No dot for 1 that changed no counted line and 1 landed before lines were kept\.$/,
    );
    expect([chart.xUnit, chart.yUnit, chart.xName]).toEqual(["count", "dollars", "lines changed"]);
    expect(chart.series).toEqual([{ name: "feature", colour: "var(--chart-1)", hidden: false }]);
    expect(chart.dots).toEqual([
      {
        key: "app/charts",
        label: "app/charts",
        href: "/task/app/charts",
        x: 400,
        y: 3.5,
        series: 0,
        said: ["$3.50 + unpriced", "400 lines", "$0.88 per 100 lines"],
      },
    ]);
    expect(chart.table()).toEqual({
      caption: "Every dot, by its lines",
      head: ["Task", "Kind", "Cost", "Lines", "Per 100 lines"],
      rows: [
        {
          key: "app/charts",
          href: "/task/app/charts",
          cells: ["app/charts", "feature", "$3.50 + unpriced", "400", "$0.88"],
        },
      ],
    });
  });

  it("orders the dots by lines, then cost, then name, a series a kind in the kinds' order", () => {
    const tasks = [
      task("z/fix", { kind: "fix", lines: 50, first: 2 }),
      task("b/feature", { lines: 50, first: 2 }),
      task("a/chore", { kind: "chore", lines: 10, first: 9 }),
      task("c/none", { kind: null, lines: 50, first: 1 }),
      task("d/free", { lines: 20, first: 0 }),
    ];
    const chart = spreadChart(withTasks(tasks), [seriesKey("spread", "No kind")]);
    expect(chart.dots.map((dot) => [dot.label, dot.series])).toEqual([
      ["a/chore", 2],
      ["c/none", 2],
      ["b/feature", 0],
      ["z/fix", 1],
    ]);
    expect(chart.series.map((series) => [series.name, series.colour, series.hidden])).toEqual([
      ["feature", "var(--chart-1)", false],
      ["fix", "var(--chart-3)", false],
      ["No kind", "var(--chart-none)", true],
    ]);
    expect(chart.note).toMatch(/ No dot for 1 with no price\.$/);
    expect(chart.table().rows.map((row) => row.cells[1])).toEqual([
      "chore",
      "none",
      "feature",
      "fix",
    ]);
    // Every task drawn: nothing said left out.
    expect(spreadChart(withTasks([task("a/b")]), []).note).toMatch(/lock files are left out\.$/);
  });
});

/** The tab drawn as text, the chart the address names (`hash`) drawn at once. */
function tab(page: LandedPage, hash = "", hidden: string[] = []) {
  vi.stubGlobal("location", { hash });
  return renderToStaticMarkup(<LandedView page={page} hidden={hidden} onToggle={() => {}} lands />);
}

describe("the tab", () => {
  it("draws its sentence, the files not read, four linked figures and a place for each chart", () => {
    const html = tab(read(sample()));
    expect(html).toContain(
      '<section class="reports-section reports-landed" aria-label="Landed tasks"><p class="reports-sub">The 3 top-level tasks landed',
    );
    expect(html).toContain(
      '<details class="reports-unread"><summary>1 file not read: the tasks they hold are missing</summary><ul><li>/repo/.git/symphony/reports/2026-11-08.json: not read</li></ul></details>',
    );
    expect(html.match(/<dt><a href="#chart-/g)).toHaveLength(4);
    expect(html.match(/class="viewport-slot chart-slot"/g)).toHaveLength(4);
    for (const id of ["dearest", "kinds", "rounds", "spread"]) {
      expect(html).toContain(`<span id="chart-${id}"></span><div class="ghost-chart"`);
    }
    expect(html).not.toContain("<figure");
    expect(text(html)).toContain(
      "Every dollar here is estimated at API list prices read 2026-11-16, not money spent; a " +
        "figure marked + unpriced leaves out the tokens of a model the price table does not name.",
    );
    expect(html).toContain(`<p class="reports-note">${LEFT_OUT.replaceAll("'", "&#x27;")}</p>`);
    expect(LEFT_OUT).toContain("Codex");
    expect(LEFT_OUT).toContain("orchestrators");
    expect(LEFT_OUT).toContain("last answer");
    const files = tab({ ...read(sample()), unread: ["a", "b"] });
    expect(files).toContain("<summary>2 files not read: the tasks they hold are missing</summary>");
    expect(tab({ ...read(sample()), unread: [] })).not.toContain("reports-unread");
  });

  it("draws the chart the address names at once, each in its own shape", () => {
    let html = tab(read(sample()), "#chart-dearest");
    expect(html).toContain('<figure class="chart hchart" id="chart-dearest"');
    expect(html).toContain('<a class="hbar-label" href="/task/app/charts">app/charts</a>');
    expect(html.match(/<figure/g)).toHaveLength(1);
    html = tab(read(sample()), "#chart-kinds");
    expect(html).toContain('<figure class="chart hchart side" id="chart-kinds"');
    expect(html).toContain('<span class="hbar-label">No kind</span>');
    html = tab(read(sample()), "#chart-rounds");
    expect(html).toContain('<figure class="chart" id="chart-rounds"');
    expect(html).toContain("<span><b>2 reviews</b>1 task</span>");
    expect(html).toContain("<strong>Landed after 2 reviews · 1 task</strong>");
    html = tab(read(sample()), "#chart-spread", [seriesKey("spread", "feature")]);
    expect(html).toContain('<figure class="chart dchart" id="chart-spread"');
    expect(html).toContain('<p class="chart-when">No dot to show</p>');
  });

  it("says why it has no chart where no task landed with tokens, its figures no links", () => {
    const empty = read(CASES.samples.emptyLanded);
    let html = tab(empty);
    expect(html).toContain(
      '<section class="reports-empty" aria-label="No landed task"><p>No top-level task landed in the window.</p></section>',
    );
    expect(html).toContain("<dt>Landed</dt>");
    expect(html).not.toContain("chart-slot");
    expect(html).not.toContain('class="reports-sub"');
    expect(text(html)).toContain("estimated at API list prices read on a day not named");
    html = tab({ ...empty, without: 2 });
    expect(html).toContain("2 top-level tasks landed in the window, none with tokens recorded");
  });
});
