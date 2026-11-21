import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import {
  ChartCard,
  type ChartData,
  ChartTable,
  coverage,
  format,
  GhostChart,
  gap,
  keyed,
  type Mark,
  NO_AXIS,
  place,
  Readout,
  runs,
  type SeriesData,
  scaleOf,
  segments,
  seriesKey,
  short,
  shown,
  step,
  sum,
  type Unit,
  upTo,
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
