import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import {
  ChartCard,
  type ChartData,
  ChartTable,
  categoryAxis,
  coverage,
  DataTable,
  type Dot,
  DotChart,
  type DotsData,
  dotStep,
  format,
  GhostChart,
  gap,
  keyed,
  logPlace,
  logScale,
  type Mark,
  NO_AXIS,
  place,
  Readout,
  type Row,
  RowChart,
  type RowsData,
  rowKey,
  rowScale,
  rowTotal,
  runs,
  type SeriesData,
  scaleOf,
  segments,
  seriesKey,
  short,
  shown,
  stacked,
  step,
  sum,
  type Unit,
  upTo,
  valued,
} from "./chart";
import { childrenOf } from "./testing";

function series(name: string, mark: Mark, values: (number | null)[]): SeriesData {
  return { name, mark, values, hidden: false };
}

/** `data` hidden. */
const hide = (data: SeriesData): SeriesData => ({ ...data, hidden: true });

function chart(unit: Unit, data: SeriesData[]): ChartData {
  const periods = data[0]?.values.length ?? 0;
  return {
    id: "test",
    title: "Test",
    note: "A test chart.",
    unit,
    series: data,
    cover: Array.from({ length: periods }, () => 1),
    instead: null,
  };
}

/** Open work as the live board drew it on 14 Nov: Ready far over the other stages. */
function openWork(): SeriesData[] {
  return [
    series("Ready", "bar", [107, 39]),
    series("In progress", "bar", [7, 10]),
    series("Review", "bar", [2, 1]),
    series("Blocked", "bar", [1, null]),
  ];
}

describe("a figure's words", () => {
  it("drop trailing zeros, and never write a negative zero", () => {
    expect(short(2.5, 2)).toBe("2.5");
    expect(short(3, 2)).toBe("3");
    expect(short(-0, 2)).toBe("0");
    expect(short(-0.001, 2)).toBe("0");
    expect(short(100, 0)).toBe("100");
  });

  it.each<[Unit, number, string]>([
    ["count", 12, "12"],
    ["count", 0.5, "0.5"],
    ["count", 1234, "1.2k"],
    ["count", -300, "-300"],
    ["mean", 2, "2"],
    ["mean", 2.75, "2.75"],
    ["seconds", 45, "45s"],
    ["seconds", 90, "1.5m"],
    ["seconds", 720, "12m"],
    ["seconds", 5400, "1.5h"],
    ["seconds", 200_000, "2.3d"],
    ["hours", 12.3, "12.3h"],
    ["hours", 3, "3h"],
    ["load", 6.5, "6.5"],
    ["tokens", 950, "950"],
    ["tokens", 12_345, "12.3k"],
    ["tokens", 4_500_000, "4.5M"],
    ["tokens", 2_000_000_000, "2G"],
    ["dollars", 1.25, "$1.25"],
    ["dollars", 0.004, "$0"],
    ["dollars", 12.5, "$12.50"],
    ["dollars", 8.2, "$8.20"],
    ["dollars", 0.4, "$0.40"],
    ["dollars", 20, "$20"],
    ["dollars", 19.999, "$20"],
    ["dollars", 0.01, "$0.01"],
    ["dollars", 999.5, "$999.50"],
    ["dollars", 1_234, "$1.2k"],
    // An empty sum of floats is negative zero, and a tiny negative rounds to it.
    ["mean", -0, "0"],
    ["count", -0.001, "0"],
    ["seconds", -0, "0s"],
    ["hours", -0.01, "0h"],
  ])("of a %s chart write %d as %s", (unit, value, text) => {
    expect(format(unit, value)).toBe(text);
  });

  it("say how much of a period its source recorded, and nothing for a whole one or a gap", () => {
    expect(coverage(0.75, 24)).toBe("18 of 24 h recorded");
    expect(coverage(0.5, 1)).toBe("0.5 of 1 h recorded");
    expect(coverage(1, 24)).toBeNull();
    expect(coverage(0, 24)).toBeNull();
  });

  it("sum from a negative zero, as Rust's float sum starts", () => {
    expect(Object.is(sum([]), -0)).toBe(true);
    expect(sum([1, 2.5])).toBe(3.5);
    expect(upTo(3)).toEqual([0, 1, 2]);
    expect(seriesKey("open", "Ready")).toBe("open:Ready");
  });
});

describe("a chart's scale", () => {
  it("steps round and holds every stack and line", () => {
    const counts = chart("count", [
      series("a", "bar", [3, 4, null]),
      series("b", "bar", [1, 3, null]),
      series("c", "line", [6, null, null]),
    ]);
    expect(scaleOf(counts).ticks).toEqual([0, 2, 4, 6, 8]);
    expect(Object.is(scaleOf(counts).low, 0)).toBe(true);
    const lines = scaleOf(
      chart("count", [series("added", "bar", [120]), series("removed", "below", [45])]),
    );
    expect([lines.low, lines.high, lines.ticks.length]).toEqual([-50, 150, 5]);
    expect(place(lines, 0)).toBeCloseTo(25, 9);
    expect(scaleOf(chart("seconds", [series("w", "line", [5400])])).ticks).toEqual([
      0, 1800, 3600, 5400,
    ]);
    expect(scaleOf(chart("count", [series("n", "bar", [null, null])])).ticks).toEqual([0, 1]);
    expect(scaleOf(chart("mean", [series("m", "bar", [2.5])])).ticks).toEqual([0, 1, 2, 3]);
    expect(scaleOf(chart("count", [series("s", "line", [1])])).ticks).toEqual([0, 1]);
    expect(scaleOf(chart("count", [series("t", "line", [10])])).ticks).toEqual([0, 5, 10]);
    expect(scaleOf(chart("count", [series("h", "bar", [100])])).ticks).toEqual([
      0, 25, 50, 75, 100,
    ]);
    expect(scaleOf(chart("hours", [series("h", "bar", [10])])).ticks).toEqual([0, 2.5, 5, 7.5, 10]);
    expect(scaleOf(chart("count", [])).ticks).toEqual([0, 1]);
  });

  it("takes a round duration for seconds, and a power of ten past the longest", () => {
    expect(step("seconds", 0.2)).toBe(1);
    expect(step("seconds", 1_000)).toBe(1_800);
    expect(step("seconds", 2_100_000)).toBe(2_500_000);
    expect(step("count", 0.3)).toBe(1);
    expect(step("count", 2.2)).toBe(5);
    expect(step("mean", 0.3)).toBe(0.5);
    expect(step("load", 0.9)).toBe(1);
    // Tokens step by whole ones, as a count does; dollars by a cent at least.
    expect(step("tokens", 0.3)).toBe(1);
    expect(step("tokens", 2_200_000)).toBe(2_500_000);
    expect(step("dollars", 0.001)).toBe(0.01);
    expect(step("dollars", 0.3)).toBe(0.5);
    expect(scaleOf(chart("dollars", [series("d", "bar", [0, 0])])).ticks).toEqual([0, 0.01]);
  });

  it("gives a hidden series no room, so the rest fill the plot", () => {
    expect(scaleOf(chart("count", openWork())).high).toBe(150);
    const stages = openWork().map((data, at) => (at === 0 ? hide(data) : data));
    const rest = chart("count", stages);
    const scale = scaleOf(rest);
    expect(scale.ticks).toEqual([0, 5, 10, 15]);
    expect(
      segments(rest, 1, scale).map((part) => [
        part.series,
        Math.round(part.bottom),
        part.end,
        part.after,
      ]),
    ).toEqual([
      [1, 0, false, false],
      [2, 67, true, true],
    ]);
    expect(gap(rest, 1)).toBe(false);
    const lines = chart("count", [
      series("added", "bar", [2]),
      hide(series("removed", "below", [45])),
      hide(series("open", "line", [50])),
    ]);
    const kept = scaleOf(lines);
    expect([kept.low, kept.high]).toEqual([0, 2]);
    expect(Object.is(kept.low, 0)).toBe(true);
    expect(segments(lines, 0, kept).every((part) => part.up)).toBe(true);
    const none = chart("count", [
      hide(series("a", "bar", [5, null])),
      hide(series("b", "bar", [null, null])),
    ]);
    expect(scaleOf(none).ticks).toEqual([0, 1]);
    expect(segments(none, 0, scaleOf(none))).toEqual([]);
    expect([gap(none, 0), gap(none, 1)]).toEqual([false, true]);
  });
});

describe("a chart's marks", () => {
  it("stack a bar up and down, its far ends rounded and a gap over each part below", () => {
    const stacked = chart("count", [
      series("a", "bar", [2, 0]),
      series("b", "bar", [2, 4]),
      series("c", "below", [2, null]),
    ]);
    const scale = scaleOf(stacked);
    expect([scale.low, scale.high]).toEqual([-2, 4]);
    expect(
      segments(stacked, 0, scale).map((part) => [
        part.series,
        Math.round(part.bottom),
        Math.round(part.height),
        part.up,
        part.end,
        part.after,
      ]),
    ).toEqual([
      [0, 33, 33, true, false, false],
      [1, 67, 33, true, true, true],
      [2, 0, 33, false, true, false],
    ]);
    // A zero draws nothing: the next part is the first.
    const parts = segments(stacked, 1, scale);
    expect(parts).toHaveLength(1);
    expect(parts[0]).toMatchObject({ series: 1, end: true, after: false });
  });

  it("break a line across a gap, and a period alone between gaps is a dot", () => {
    const line = chart("count", [series("l", "line", [1, 2, null, 3, null, 4, 4])]);
    const scale = scaleOf(line);
    expect(runs(line.series[0]?.values ?? [], scale)).toEqual([
      ["0.5,75 1.5,50", "5.5,0 6.5,0"],
      [[3.5, 25]],
    ]);
    expect(runs([null, null], scale)).toEqual([[], []]);
  });

  it("show the period tapped, else the last with a value, else none", () => {
    const line = chart("count", [series("l", "line", [1, 2, null])]);
    expect(shown(line, 0, 3)).toBe(0);
    expect(shown(line, null, 3)).toBe(1);
    expect(shown(line, 5, 3)).toBe(1);
    expect(shown(chart("count", [series("n", "bar", [null])]), null, 1)).toBeNull();
  });

  it("move between the periods by the arrow keys, Home and End", () => {
    expect(keyed("ArrowLeft", 2, 5)).toBe(1);
    expect(keyed("ArrowRight", 2, 5)).toBe(3);
    expect(keyed("ArrowLeft", 0, 5)).toBe(0);
    expect(keyed("ArrowRight", 4, 5)).toBe(4);
    expect(keyed("Home", 3, 5)).toBe(0);
    expect(keyed("End", 1, 5)).toBe(4);
    expect(keyed("ArrowLeft", null, 5)).toBe(4);
    expect(keyed("End", null, 0)).toBe(0);
    expect(keyed("Enter", 2, 5)).toBeNull();
  });
});

/** Open work over three days, the second a gap, and the page's axis for them. */
function openChart(): ChartData {
  return {
    ...chart("count", [
      series("Ready", "bar", [2, null, 0]),
      series("In progress", "bar", [0, null, 5]),
      series("Review", "bar", [1, null, 0]),
      series("Blocked", "bar", [0, null, 0]),
    ]),
    id: "open",
    title: "Open work",
    cover: [1, 0, 5 / 11],
  };
}

const AXIS = {
  periods: ["Mon 9 Nov", "Tue 10 Nov", "Wed 11 Nov"],
  hours: [24, 24, 11],
  ticks: [
    [0, "9 Nov"],
    [1, "10 Nov"],
    [2, "11 Nov"],
  ] as [number, string][],
  marks: [[25 / 3, 0, "settings changed, settings: land without the check (4f1c0a9b2d3e)"]] as [
    number,
    number,
    string,
  ][],
};

const card = (data: ChartData, selected: number | null, axis = AXIS) =>
  renderToStaticMarkup(
    <ChartCard chart={data} axis={axis} selected={selected} onSelect={() => {}} />,
  );

describe("a chart's card", () => {
  it("shows the last period recorded until one is tapped, and a gap says it has no data", () => {
    let html = card(openChart(), null);
    expect(html).toContain("<strong>Wed 11 Nov</strong> · 5 of 11 h recorded");
    expect(html).toContain("In progress <strong>5</strong>");
    expect(html).toContain('class="chart-col on"');
    expect(html.split('class="chart-col').length - html.split('class="chart-cols"').length).toBe(3);
    html = card(openChart(), 1);
    expect(html).toContain('class="chart-col gap on"');
    expect(html).toContain("<strong>Tue 10 Nov</strong> · no data recorded");
    expect(html).not.toContain("Ready <strong>");
    html = card(openChart(), 0);
    expect(html).toContain("Ready <strong>2</strong>");
    expect(html).toContain("Review <strong>1</strong>");
    expect(html).toContain("Blocked <strong>0</strong>");
    expect(html).toContain("Settings changed, settings: land without the check (4f1c0a9b2d3e)");
    expect(html).toContain(
      'title="settings changed, settings: land without the check (4f1c0a9b2d3e)"',
    );
    expect(childrenOf(Readout({ chart: openChart(), axis: AXIS, at: 0 }), "chart-mark-note")).toBe(
      "Settings changed, settings: land without the check (4f1c0a9b2d3e)",
    );
    expect(html.match(/class="seg up/g)).toHaveLength(3);
    expect(html).toContain('class="chart-mark" style="left:8.33%"');
    expect(html).toContain('aria-expanded="false">Show the table</button>');
    expect(html).toContain(
      'tabindex="0" role="group" aria-label="Open work: the left and right arrow keys move between periods"',
    );
    expect(html).toContain('<span style="left:83.33%">11 Nov</span>');
  });

  it("draws a line's runs as strokes, a period alone as a dot and the period shown ringed", () => {
    const lead = {
      ...chart("seconds", [series("Median", "line", [3600, null, 1800, 1200])]),
      id: "lead",
      title: "Lead time",
    };
    const html = card(lead, 0, { ...AXIS, periods: [...AXIS.periods, "Thu 12 Nov"] });
    expect(html.match(/<polyline/g)).toHaveLength(1);
    expect(html).toContain('<line class="dot" x1="0.5" y1="0" x2="0.5" y2="0"></line>');
    expect(html).toContain('<line class="ring" x1="0.5" y1="0" x2="0.5" y2="0"></line>');
    expect(html).toContain('points="2.5,50 3.5,66.67"');
    expect(html).toContain('<svg class="chart-lines" viewBox="0 0 4 100"');
  });

  it("has a legend whose buttons say which series are shown, a hidden one out of the plot", () => {
    let html = card(openChart(), 0);
    expect(html).toContain('<ul class="chart-legend" aria-label="Open work: series shown">');
    expect(
      html.match(/<button class="chart-key" type="button" aria-pressed="true">/g),
    ).toHaveLength(4);
    const hidden = {
      ...openChart(),
      series: openChart().series.map((d, at) => (at ? d : hide(d))),
    };
    html = card(hidden, 0);
    expect(html.match(/aria-pressed="true"/g)).toHaveLength(3);
    expect(html).toContain(
      'aria-pressed="false"><span class="swatch" style="--c:var(--chart-1)" aria-hidden="true"></span>Ready</button>',
    );
    expect(html).not.toContain("Ready <strong>");
    expect(html).toContain("In progress <strong>0</strong>");
    expect(html).not.toContain("%;--c:var(--chart-1)");
    expect(html).toContain("%;--c:var(--chart-3)");
    const table = renderToStaticMarkup(<ChartTable chart={hidden} axis={AXIS} />);
    expect(table).not.toContain(">Ready</th>");
    expect(table).toContain('<th scope="col">In progress</th>');
    expect(table).toContain('<th scope="row">Mon 9 Nov</th><td>0</td><td>1</td><td>0</td>');
    // A hidden line draws no stroke or dot: each of these two draws two alone and the one shown.
    const a = series("a", "line", [1, null, 2]);
    const b = series("b", "line", [3, null, 1]);
    const dots = (data: SeriesData[]) =>
      card(chart("count", data), null).match(/class="dot"/g)?.length ?? 0;
    expect(dots([a, b])).toBe(6);
    expect(dots([a, hide(b)])).toBe(3);
    // A chart of one series has nothing to hide.
    expect(card(chart("count", [series("Done", "bar", [1])]), null)).not.toContain("chart-key");
  });

  it("says in its plot's place what the window cannot show", () => {
    const html = card(
      { ...openChart(), instead: "Switch to Days or Weeks to see it." },
      null,
      NO_AXIS,
    );
    expect(html).toContain('<p class="chart-instead">Switch to Days or Weeks to see it.</p>');
    expect(html).not.toContain("chart-plot");
  });

  it("has a table of every period, a gap as a dash", () => {
    const done = { ...chart("count", [series("Done", "bar", [3, null, 4])]), title: "Tasks done" };
    const html = renderToStaticMarkup(<ChartTable chart={done} axis={AXIS} />);
    expect(html).toContain("<caption>Tasks done, by period</caption>");
    // One text, as the Dioxus page writes it, so the browser shapes it alike.
    expect(childrenOf(ChartTable({ chart: done, axis: AXIS }), "caption")).toBe(
      "Tasks done, by period",
    );
    expect(html).toContain('<th scope="row">Mon 9 Nov</th><td>3</td>');
    expect(html).toContain('<th scope="row">Tue 10 Nov</th><td>–</td>');
  });

  it("reads out nothing for a chart with no period recorded", () => {
    const html = renderToStaticMarkup(
      <Readout chart={chart("count", [series("n", "bar", [null])])} axis={NO_AXIS} at={null} />,
    );
    expect(html).toBe(
      '<div class="chart-readout" aria-live="polite"><p class="chart-when">No period recorded yet</p></div>',
    );
  });

  it("has a ghost of its box, its title and its plot", () => {
    expect(renderToStaticMarkup(<GhostChart />)).toBe(
      '<div class="ghost-chart" aria-hidden="true"><span class="bone" style="--w:40%"></span>' +
        '<span class="bone" style="--w:70%"></span><div class="ghost-plot"></div></div>',
    );
  });
});

describe("a period with no value", () => {
  it("is a gap unless its chart says what none is and its source recorded it", () => {
    const landed = {
      ...chart("dollars", [series("Median", "line", [5, null, null])]),
      cover: [1, 0, 0.5],
    };
    expect([gap(landed, 0), gap(landed, 1), gap(landed, 2)]).toEqual([false, true, true]);
    const said = { ...landed, none: "no task landed" };
    expect([gap(said, 0), gap(said, 1), gap(said, 2)]).toEqual([false, true, false]);
    expect([valued(said, 0), valued(said, 2)]).toEqual([true, false]);
    // A hidden series keeps its record: the period is no gap, and no value is shown either.
    expect(valued({ ...said, series: said.series.map(hide) }, 0)).toBe(true);
  });

  it("is read out in its chart's words, each period's own line after its name", () => {
    const landed: ChartData = {
      ...chart("dollars", [series("Median", "line", [5, null, null])]),
      cover: [1, 0, 5 / 11],
      none: "no task landed",
      whens: ["3 landed", null, null],
      extras: [["median $13.39 a task"], [], []],
    };
    const readout = (at: number) =>
      renderToStaticMarkup(<Readout chart={landed} axis={{ ...AXIS, marks: [] }} at={at} />)
        .replace(/<[^>]+>/g, " ")
        .replace(/\s+/g, " ")
        .trim();
    expect(readout(0)).toBe("Mon 9 Nov · 3 landed Median $5 median $13.39 a task");
    expect(readout(1)).toBe("Tue 10 Nov · no data recorded");
    expect(readout(2)).toBe("Wed 11 Nov · no task landed · 5 of 11 h recorded");
    expect(card(landed, 2)).toContain('class="chart-col on"');
    expect(card(landed, 1)).toContain('class="chart-col gap on"');
  });
});

describe("columns over categories", () => {
  /** The mean by review rounds: no review, one, and four or more with nothing after its first. */
  function rounds(): ChartData {
    return {
      ...chart("dollars", [
        series("First round", "bar", [0.75, 2, 4]),
        series("Review rounds", "bar", [0, 1.5, 0]),
      ]),
      id: "rounds",
      title: "By review rounds",
      categories: [
        ["No review", "2 tasks"],
        ["1 review", "1 task"],
        ["4 or more", "1 task"],
      ],
    };
  }
  const names = [
    "Landed with no review · 2 tasks",
    "Landed after 1 review · 1 task",
    "Landed after 4 reviews or more · 1 task",
  ];

  it("name each column in the readout and its table, and have no hour or change", () => {
    expect(categoryAxis(names)).toEqual({ periods: names, hours: [0, 0, 0], ticks: [], marks: [] });
    const html = card(rounds(), 1, categoryAxis(names));
    expect(html).toContain("<strong>Landed after 1 review · 1 task</strong>");
    expect(html).not.toContain("h recorded");
    const table = renderToStaticMarkup(<ChartTable chart={rounds()} axis={categoryAxis(names)} />);
    expect(table).toContain("<caption>By review rounds</caption>");
    expect(table).toContain('<th scope="col">Group</th>');
    expect(table).toContain(
      '<th scope="row">Landed after 1 review · 1 task</th><td>$2</td><td>$1.50</td>',
    );
  });

  it("write each column's total over it and its name under it, in place of the time labels", () => {
    expect([0, 1, 2].map((at) => stacked(rounds(), at))).toEqual([0.75, 3.5, 4]);
    const hidden = { ...rounds(), series: rounds().series.map((d, at) => (at ? hide(d) : d)) };
    expect(stacked(hidden, 1)).toBe(2);
    const html = card(rounds(), null, categoryAxis(names));
    expect(html.match(/class="seg up[^"]* cat"/g)).toHaveLength(4);
    expect(html).toContain('<span class="cat-total" style="bottom:87.5%">$3.50</span>');
    expect(html).toContain('<div class="cat-x" style="--n:3" aria-hidden="true">');
    expect(html).toContain("<span><b>No review</b>2 tasks</span>");
    expect(html).not.toContain('class="chart-x"');
    // A column with nothing shown has no total over it.
    const empty = {
      ...rounds(),
      series: rounds().series.map((d) => ({ ...d, values: [0, 0, 0] })),
    };
    expect(card(empty, null, categoryAxis(names))).not.toContain("cat-total");
  });
});

/** A chart of bars by row: the dearest tasks, first round and review rounds stacked. */
function dearest(rows: Row[], side = false): RowsData {
  return {
    id: "dearest",
    title: "Dearest tasks",
    note: "The dearest first.",
    unit: "dollars",
    series: [
      { name: "First round", hidden: false },
      { name: "Review rounds", hidden: false },
    ],
    rows,
    tableToggle: "Show all 2 as a table",
    table: () => ({
      caption: "Every task",
      head: ["Task", "Cost"],
      rows: rows.map((row) => ({ key: row.key, cells: [row.label, "$1"], href: row.href })),
    }),
    side,
  };
}

function row(label: string, values: number[], fields: Partial<Row> = {}): Row {
  return {
    key: label,
    label,
    href: `/task/${label}`,
    values,
    colour: null,
    meta: null,
    said: [],
    ...fields,
  };
}

const rowsCard = (data: RowsData, selected: number | null) =>
  renderToStaticMarkup(<RowChart chart={data} selected={selected} onSelect={() => {}} />);

describe("a chart of bars by row", () => {
  it("scales from zero to a round value over the longest bar, a hidden part taking no room", () => {
    expect(rowScale("dollars", 127)).toEqual({ low: 0, high: 150, ticks: [0, 50, 100, 150] });
    expect(rowScale("dollars", 0)).toEqual(rowScale("dollars", -1));
    expect(rowScale("count", 0).high).toBeGreaterThan(0);
    const data = dearest([row("a/b", [3, 1]), row("c/d", [2, 0])]);
    expect(data.rows.map((one) => rowTotal(data, one))).toEqual([4, 2]);
    const hidden = { ...data, series: data.series.map((d, at) => ({ ...d, hidden: at === 1 })) };
    expect(rowTotal(hidden, row("a/b", [3, 1]))).toBe(3);
  });

  it("draws each row's name a link, its parts stacked and its total, the first read out", () => {
    const data = dearest([
      row("app/charts", [30, 10], { said: ["1.2M tokens", "2 reviews"] }),
      row("docs/guide", [10, 0], { href: null }),
    ]);
    let html = rowsCard(data, null);
    expect(html).toContain(
      '<figure class="chart hchart" id="chart-dearest" aria-labelledby="chart-dearest-title">',
    );
    expect(html).toContain(
      '<div class="hbar on"><a class="hbar-label" href="/task/app/charts">app/charts</a>',
    );
    expect(html).toContain('<span class="hseg" style="width:75%;--c:var(--chart-1)"></span>');
    expect(html).toContain('<span class="hseg" style="width:25%;--c:var(--chart-2)"></span>');
    expect(html).toContain('<span class="hbar-value">$40</span>');
    // A row with no page names itself in text, and a part of nothing draws no segment.
    expect(html).toContain('<span class="hbar-label">docs/guide</span>');
    expect(html.match(/class="hseg"/g)).toHaveLength(3);
    expect(html).toContain("<strong>app/charts</strong>");
    expect(html).toContain("First round <strong>$30</strong>");
    expect(html).toContain("<li>1.2M tokens</li><li>2 reviews</li>");
    // The scale steps by $20 to the $40 the longest bar takes.
    expect(html).toContain('<span style="left:100%">$40</span>');
    expect(html).toContain(
      'aria-label="Dearest tasks: the up and down arrow keys move between rows"',
    );
    expect(html).toContain('aria-expanded="false">Show all 2 as a table</button>');
    expect(html).toContain('<ul class="chart-legend" aria-label="Dearest tasks: series shown">');
    html = rowsCard(data, 1);
    expect(html).toContain('<div class="hbar on"><span class="hbar-label">docs/guide</span>');
    expect(html).toContain("Review rounds <strong>$0</strong>");
    // A pick past the rows reads out the first.
    expect(rowsCard(data, 7)).toContain("<strong>app/charts</strong>");
  });

  it("leaves a hidden part out of each bar and the readout, its button not pressed", () => {
    const data = dearest([row("app/charts", [30, 10])]);
    const hidden = { ...data, series: data.series.map((d, at) => ({ ...d, hidden: at === 1 })) };
    const html = rowsCard(hidden, null);
    expect(html).toContain(
      'aria-pressed="false"><span class="swatch" style="--c:var(--chart-2)" aria-hidden="true"></span>Review rounds</button>',
    );
    expect(html).not.toContain("Review rounds <strong>");
    expect(html).not.toContain('--c:var(--chart-2)"></span></span>');
    expect(html).toContain('<span class="hbar-value">$30</span>');
  });

  it("puts a short name beside its bar in the row's own colour, with no legend for one part", () => {
    const kinds: RowsData = {
      ...dearest(
        [
          row("feature", [13.39], {
            href: null,
            colour: "var(--chart-1)",
            meta: "45 tasks · $1,280 in all",
          }),
          row("No kind", [9], { href: null, colour: "var(--chart-none)" }),
        ],
        true,
      ),
      id: "kinds",
      title: "By kind",
      series: [{ name: "Median a task", hidden: false }],
    };
    const html = rowsCard(kinds, null);
    expect(html).toContain('<figure class="chart hchart side" id="chart-kinds"');
    expect(html).not.toContain("chart-legend");
    expect(html).toContain('<span class="hseg" style="width:89.27%;--c:var(--chart-1)"></span>');
    expect(html).toContain("--c:var(--chart-none)");
    expect(html).toContain('<span class="hbar-meta">45 tasks · $1,280 in all</span>');
    expect(html).toContain(
      '<span class="swatch" style="--c:var(--chart-1)" aria-hidden="true"></span>Median a task <strong>$13.39</strong>',
    );
  });

  it("reads out nothing with no row", () => {
    const html = rowsCard(dearest([]), null);
    expect(html).toContain('<p class="chart-when"><strong></strong></p></div>');
    expect(html).not.toContain("hbar ");
  });

  it("lists every row in its table, each name a link where it has a page", () => {
    const table = dearest([
      row("app/charts", [1, 0]),
      row("docs/guide", [1, 0], { href: null }),
    ]).table();
    const html = renderToStaticMarkup(
      <DataTable
        table={{ ...table, rows: [...table.rows, { key: "x", cells: ["x", "$1"], href: null }] }}
      />,
    );
    expect(html).toContain("<caption>Every task</caption>");
    expect(html).toContain('<th scope="col">Task</th><th scope="col">Cost</th>');
    expect(html).toContain(
      '<th scope="row"><a href="/task/app/charts">app/charts</a></th><td>$1</td>',
    );
    expect(html).toContain('<th scope="row">docs/guide</th><td>$1</td>');
  });
});

describe("a log scale", () => {
  it("runs from a round value at or under the least to one at or over the most", () => {
    expect(logScale([12, 4_800], [1])).toEqual({
      low: 10,
      high: 10_000,
      ticks: [10, 100, 1_000, 10_000],
    });
    expect(logScale([0.4, 37], [1, 5])).toEqual({
      low: 0.1,
      high: 50,
      ticks: [0.1, 0.5, 1, 5, 10, 50],
    });
    // A value on a round one is its end; zero, below zero and no value at all leave 1 to 10.
    expect(logScale([1, 5], [1, 5])).toEqual({ low: 1, high: 5, ticks: [1, 5] });
    expect(logScale([0, -3, Number.POSITIVE_INFINITY], [1])).toEqual({
      low: 1,
      high: 10,
      ticks: [1, 10],
    });
    // One value alone still spans a step.
    expect(logScale([100], [1])).toEqual({ low: 100, high: 1_000, ticks: [100, 1_000] });
  });

  it("places a value by its logarithm, and anything at the low end of a scale of no span", () => {
    const scale = logScale([10, 10_000], [1]);
    expect(logPlace(scale, 10)).toBe(0);
    expect(logPlace(scale, 100)).toBeCloseTo(100 / 3);
    expect(logPlace(scale, 10_000)).toBe(100);
    expect(logPlace({ low: 5, high: 5, ticks: [5] }, 5)).toBe(0);
  });
});

/** Every task as a dot, two kinds, its cost against its lines. */
function spread(dots: Dot[]): DotsData {
  return {
    id: "spread",
    title: "Every task, cost against size",
    note: "Both on a log scale.",
    xUnit: "count",
    yUnit: "dollars",
    xName: "lines changed",
    series: [
      { name: "feature", colour: "var(--chart-1)", hidden: false },
      { name: "No kind", colour: "var(--chart-none)", hidden: false },
    ],
    dots,
    tableToggle: "Show the table",
    table: () => ({ caption: "Every dot", head: ["Task"], rows: [] }),
  };
}

function dot(label: string, x: number, y: number, kind = 0, fields: Partial<Dot> = {}): Dot {
  return {
    key: label,
    label,
    href: `/task/${label}`,
    x,
    y,
    series: kind,
    said: [`$${y}`],
    ...fields,
  };
}

const dotsCard = (data: DotsData, selected: number | null) =>
  renderToStaticMarkup(<DotChart chart={data} selected={selected} onSelect={() => {}} />);

describe("a chart of dots", () => {
  const dots = [dot("a/small", 10, 1), dot("b/mid", 100, 5, 1), dot("c/big", 1_000, 50)];

  it("places each dot on two log scales in its series' colour, and asks for a tap", () => {
    const html = dotsCard(spread(dots), null);
    expect(html).toContain('<figure class="chart dchart" id="chart-spread"');
    expect(html).toContain('<p class="chart-when">Tap a dot to name it</p>');
    expect(html).toContain('<span class="pt" style="left:0%;bottom:0%;--c:var(--chart-1)"></span>');
    expect(html).toContain(
      '<span class="pt" style="left:50%;bottom:41.14%;--c:var(--chart-none)"></span>',
    );
    expect(html).toContain(
      '<span class="pt" style="left:100%;bottom:100%;--c:var(--chart-1)"></span>',
    );
    expect(html).toContain('<span style="bottom:100%">$50</span>');
    expect(html).toContain('<span style="left:50%">100</span>');
    expect(html.match(/class="chart-grid across"/g)).toHaveLength(3);
    expect(html).toContain('<p class="chart-x-name">lines changed</p>');
    expect(html).toContain(
      '<span class="swatch dot" style="--c:var(--chart-none)" aria-hidden="true"></span>No kind</button>',
    );
    expect(html).toContain(
      'aria-label="Every task, cost against size: the arrow keys move between dots"',
    );
  });

  it("names the dot tapped, its page a link, and rings it", () => {
    const html = dotsCard(spread(dots), 1);
    expect(html).toContain(
      '<strong><a href="/task/b/mid">b/mid</a></strong></p><ul><li>$5</li></ul>',
    );
    expect(html).toContain('<span class="pt on" style="left:50%');
    const unlinked = dotsCard(spread([dot("x", 10, 1, 0, { href: null })]), 0);
    expect(unlinked).toContain("<strong>x</strong>");
  });

  it("step through the dots shown by the arrow keys, a hidden series' skipped", () => {
    const data = spread([...dots, dot("d/huge", 2_000, 90, 1)]);
    expect([rowKey("ArrowUp"), rowKey("ArrowDown"), rowKey("Home"), rowKey("x")]).toEqual([
      "ArrowLeft",
      "ArrowRight",
      "Home",
      "x",
    ]);
    expect(dotStep(data, null, "ArrowRight")).toBe(0);
    expect(dotStep(data, null, "Home")).toBe(0);
    expect(dotStep(data, null, "ArrowUp")).toBe(3);
    expect(dotStep(data, null, "Escape")).toBeNull();
    expect(dotStep(data, 0, "ArrowRight")).toBe(1);
    expect(dotStep(data, 1, "ArrowDown")).toBe(2);
    expect(dotStep(data, 1, "ArrowUp")).toBe(0);
    expect(dotStep(data, 2, "End")).toBe(3);
    expect(dotStep(data, 2, "Enter")).toBeNull();
    // With No kind hidden: the second and the fourth are out of the order, and a pick of one of
    // them steps as from none.
    const hidden = { ...data, series: data.series.map((s, at) => ({ ...s, hidden: at === 1 })) };
    expect(dotStep(hidden, 0, "ArrowRight")).toBe(2);
    expect(dotStep(hidden, 2, "ArrowRight")).toBe(2);
    expect(dotStep(hidden, 1, "Home")).toBe(0);
    expect(dotStep(hidden, 1, "ArrowLeft")).toBe(2);
    const none = { ...data, series: data.series.map((s) => ({ ...s, hidden: true })) };
    expect(dotStep(none, null, "ArrowRight")).toBeNull();
  });

  it("leaves a hidden series' dots out of the plot, its scales and the readout", () => {
    const data = spread(dots);
    const hidden = { ...data, series: data.series.map((s, at) => ({ ...s, hidden: at === 0 })) };
    const html = dotsCard(hidden, 0);
    expect(html.match(/class="pt/g)).toHaveLength(1);
    expect(html).toContain('<p class="chart-when">Tap a dot to name it</p>');
    expect(html).toContain(
      'aria-pressed="false"><span class="swatch dot" style="--c:var(--chart-1)"',
    );
    const none = { ...data, series: data.series.map((s) => ({ ...s, hidden: true })) };
    expect(dotsCard(none, null)).toContain('<p class="chart-when">No dot to show</p>');
    // One series has nothing to hide.
    expect(
      dotsCard({ ...data, series: [data.series[0] as DotsData["series"][number]] }, null),
    ).not.toContain("chart-legend");
  });
});
