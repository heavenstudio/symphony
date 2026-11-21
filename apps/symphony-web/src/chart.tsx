// One chart of the Reports page, a period per column on the page's one time axis: bars as boxes of
// the page (their width at most 24 px whatever the screen, their data end rounded, a 2 px gap of the
// card between stacked parts), lines as one SVG over them whose strokes keep their width however
// the plot stretches. A period no file recorded is hatched, never drawn as a zero, and a line breaks
// across it. A tap or a key on a period shows every series' value in the chart's readout, on every
// chart at once; the table under it says them all. Each entry of a legend is a button that hides its
// series or shows it again: a hidden series leaves the plot, the readout and the table, and the axis
// scales to the series shown.

import { type CSSProperties, type KeyboardEvent, useState } from "react";
import { fixed } from "./words";

/**
 * How a series is drawn: a bar from zero up, stacked on the bars before it; a bar from zero down,
 * stacked under the ones before it (the other side of a diverging chart); or a line.
 */
export type Mark = "bar" | "below" | "line";

/**
 * What a chart's figures count, which says how they are written and how its axis steps: things
 * counted (tasks, commits, lines, requests), a mean over the time watched (sessions at work), a
 * duration in seconds, task-hours, the machine's load average, model tokens, or dollars.
 */
export type Unit = "count" | "mean" | "seconds" | "hours" | "load" | "tokens" | "dollars";

/** The sizes a count of tokens is written in, the largest first. */
const TOKEN_SIZES: readonly (readonly [number, string])[] = [
  [1e9, "G"],
  [1e6, "M"],
  [1e3, "k"],
];

/** `n` from 0 up to it, not included: the periods of an axis, the indexes of a list. */
export function upTo(n: number): number[] {
  return Array.from({ length: n }, (_, at) => at);
}

/**
 * `value` to at most `places` decimals, with no trailing zero: `2.5`, `3`, `0.25`; a value that
 * rounds to zero is `0`, never `-0` (an empty sum of floats is negative zero).
 */
export function short(value: number, places: number): string {
  let text = fixed(value, places);
  if (text.includes(".")) text = text.replace(/0+$/, "").replace(/\.$/, "");
  return text === "-0" ? "0" : text;
}

/**
 * `value` as a chart of `unit` writes it: `12`, `1.2k`, `2.25`, `45s`, `12m`, `3.5h`, `2.1d`, `4.5M`
 * tokens, `$1.25`.
 */
export function format(unit: Unit, value: number): string {
  switch (unit) {
    case "count":
      return Math.abs(value) >= 1000 ? `${short(value / 1000, 1)}k` : short(value, 2);
    case "mean":
    case "load":
      return short(value, 2);
    case "hours":
      return `${short(value, 1)}h`;
    case "tokens": {
      const size = TOKEN_SIZES.find(([size]) => Math.abs(value) >= size);
      return size === undefined ? short(value, 0) : `${short(value / size[0], 1)}${size[1]}`;
    }
    case "dollars": {
      // Money keeps its cents (`$8.20`), but a whole dollar, as an axis's ticks are, none (`$20`).
      if (Math.abs(value) >= 1000) return `$${short(value / 1000, 1)}k`;
      const cents = fixed(value, 2);
      return cents.endsWith(".00") ? `$${short(value, 0)}` : `$${cents}`;
    }
    case "seconds": {
      const seconds = Math.abs(value);
      if (seconds < 60) return `${short(value, 0)}s`;
      if (seconds < 3_600) return `${short(value / 60, seconds < 600 ? 1 : 0)}m`;
      if (seconds < 172_800) return `${short(value / 3_600, 1)}h`;
      return `${short(value / 86_400, 1)}d`;
    }
  }
}

/** The smallest step an axis of `unit` takes: a whole one of what is counted. */
function leastStep(unit: Unit): number {
  if (unit === "dollars") return 0.01;
  return unit === "count" || unit === "seconds" || unit === "tokens" ? 1 : 0.5;
}

/** The sum of `values`, from negative zero as Rust's float sum starts. */
export function sum(values: number[]): number {
  return values.reduce((total, value) => total + value, -0);
}

/**
 * One series: its name, how it is drawn, its value in each period, none where its source has no
 * record of the period, and whether this browser hid it.
 */
export interface SeriesData {
  name: string;
  mark: Mark;
  values: (number | null)[];
  hidden: boolean;
}

/** The key a hidden series is kept by: its chart's id and its name, `open:Ready`. */
export function seriesKey(chart: string, series: string): string {
  return `${chart}:${series}`;
}

/** One chart, ready to draw. */
export interface ChartData {
  id: string;
  title: string;
  /** What it shows and in what unit, a sentence. */
  note: string;
  unit: Unit;
  /** Its series in their fixed order: the first takes `--chart-1`, the next `--chart-2`, and so on. */
  series: SeriesData[];
  /** How much of each period its source recorded, from 0 to 1. */
  cover: number[];
  /** What it says in place of the plot when the window cannot show it (lead time by the hour). */
  instead: string | null;
}

/** The value of the `series`th series of `chart` in period `at`. */
export function valueAt(chart: ChartData, series: number, at: number): number | null {
  return chart.series[series]?.values[at] ?? null;
}

/**
 * Whether no series of `chart` has a value in period `at`, hidden or not: a gap. Hiding a series
 * hides its values, never its record.
 */
export function gap(chart: ChartData, at: number): boolean {
  return chart.series.every((_, series) => valueAt(chart, series, at) === null);
}

/** The page's one time axis, which every chart shares. */
export interface Axis {
  /**
   * Each period as the readout and the table name it: `Mon 9 Nov`, `Week of 9 Nov`,
   * `Mon 9 Nov, 14:00 UTC`.
   */
  periods: string[];
  /** The hours of each period, for the readout's coverage. */
  hours: number[];
  /** The periods labelled under the plot, each with its label. */
  ticks: [number, string][];
  /**
   * The changes of the settings and the agents' instructions: where each stands across the plot (0
   * to 100), its period and what it says (`settings changed, SUBJECT (HASH)`).
   */
  marks: [number, number, string][];
}

export const NO_AXIS: Axis = { periods: [], hours: [], ticks: [], marks: [] };

/**
 * A chart's vertical scale: from `low` (zero, or below it for a diverging chart) to `high`, and the
 * ticks between, a whole step apart.
 */
export interface Scale {
  low: number;
  high: number;
  ticks: number[];
}

/** The round durations an axis of seconds steps by. */
const DURATIONS = [
  1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1_800, 3_600, 7_200, 10_800, 21_600, 43_200, 86_400,
  172_800, 259_200, 604_800, 1_209_600,
];

/**
 * The step an axis of `unit` takes for `raw`: 1, 2, 2.5 or 5 times a power of ten (a whole number
 * for a count), or a round duration for seconds, never less than the unit's least.
 */
export function step(unit: Unit, raw: number): number {
  const least = leastStep(unit);
  const asked = Math.max(raw, least);
  if (unit === "seconds") {
    const round = DURATIONS.find((duration) => duration >= asked);
    if (round !== undefined) return round;
  }
  const magnitude = 10 ** Math.floor(Math.log10(asked));
  const fraction = asked / magnitude;
  // A count's axis steps by whole ones: 2.5 only where it is 25, 250….
  const nice =
    [1, 2, 2.5, 5, 10]
      .filter(
        (nice) => (unit !== "count" && unit !== "tokens") || Number.isInteger(nice * magnitude),
      )
      .find((nice) => fraction <= nice) ?? 10;
  return Math.max(nice * magnitude, least);
}

/**
 * The scale that holds every period's stacked bars and every line of `chart` it shows: a hidden
 * series takes no room, so hiding a large one makes the rest larger.
 */
export function scaleOf(chart: ChartData): Scale {
  const periods = chart.series[0]?.values.length ?? 0;
  let up = 0;
  let down = 0;
  for (const at of upTo(periods)) {
    let bars = 0;
    let below = 0;
    chart.series.forEach((data, series) => {
      if (data.hidden) return;
      const value = Math.max(valueAt(chart, series, at) ?? 0, 0);
      if (data.mark === "bar") bars += value;
      else if (data.mark === "below") below += value;
      else up = Math.max(up, value);
    });
    up = Math.max(up, bars);
    down = Math.max(down, below);
  }
  if (up + down <= 0) up = leastStep(chart.unit);
  const by = step(chart.unit, (up + down) / 4);
  const high = Math.ceil(up / by) * by;
  // From zero, so a chart with nothing below its axis starts at 0, not -0.
  const low = 0 - Math.ceil(down / by) * by;
  const ticks: number[] = [];
  for (let tick = low; tick <= high + by / 2; tick += by) ticks.push(tick);
  return { low, high, ticks };
}

/** Where `value` stands from the bottom of the plot of `scale`, 0 to 100. */
export function place(scale: Scale, value: number): number {
  return ((value - scale.low) / (scale.high - scale.low)) * 100;
}

/**
 * One part of a period's bar: its series, where it starts and how tall it is (0 to 100), whether it
 * goes up from zero or down, whether it is its stack's far end (rounded) and whether a part is under
 * it (a gap of the card between them).
 */
export interface Segment {
  series: number;
  bottom: number;
  height: number;
  up: boolean;
  end: boolean;
  after: boolean;
}

/** The bar parts of period `at` of `chart` on `scale`, its hidden series left out of the stack. */
export function segments(chart: ChartData, at: number, scale: Scale): Segment[] {
  const parts: Segment[] = [];
  for (const up of [true, false]) {
    const mark: Mark = up ? "bar" : "below";
    let base = 0;
    const first = parts.length;
    chart.series.forEach((data, series) => {
      const value = valueAt(chart, series, at) ?? 0;
      if (data.hidden || data.mark !== mark || value <= 0) return;
      const [from, to] = up ? [base, base + value] : [-(base + value), -base];
      parts.push({
        series,
        bottom: place(scale, from),
        height: place(scale, to) - place(scale, from),
        up,
        end: false,
        after: parts.length > first,
      });
      base += value;
    });
    const last = parts.length > first ? parts.at(-1) : undefined;
    if (last) last.end = true;
  }
  return parts;
}

/**
 * A line series across the periods on `scale`: each run of two periods or more with a value as the
 * points of a polyline (`x,y`, a period one unit wide, y down from the top, 0 to 100), and each
 * period with a value alone between gaps as a dot.
 */
export function runs(values: (number | null)[], scale: Scale): [string[], [number, number][]] {
  const lines: string[] = [];
  const dots: [number, number][] = [];
  let run: [number, number][] = [];
  const flush = () => {
    const [one] = run;
    if (run.length === 1 && one) dots.push(one);
    else if (run.length > 1) lines.push(run.map(([x, y]) => `${x},${short(y, 2)}`).join(" "));
    run = [];
  };
  values.forEach((value, at) => {
    if (value === null) flush();
    else run.push([at + 0.5, 100 - place(scale, value)]);
  });
  flush();
  return [lines, dots];
}

/** The period a chart shows in its readout: the one tapped, else the last with a value. */
export function shown(chart: ChartData, selected: number | null, periods: number): number | null {
  if (selected !== null && selected < periods) return selected;
  return (
    upTo(periods)
      .reverse()
      .find((at) => !gap(chart, at)) ?? null
  );
}

/**
 * `part` of `whole` hours as the readout says it, when the source recorded only part of the
 * period: `18 of 24 h recorded`; none for a whole period or none of it.
 */
export function coverage(part: number, whole: number): string | null {
  const hours = part * whole;
  return part > 0 && part < 0.99
    ? `${short(hours, hours < 10 ? 1 : 0)} of ${short(whole, 0)} h recorded`
    : null;
}

/** A style that sets custom properties alongside the rest. */
function css(properties: Record<string, string>): CSSProperties {
  return properties as CSSProperties;
}

/** A series' colour, by its place in its chart. */
function colour(series: number): string {
  return `var(--chart-${series + 1})`;
}

/** A series' swatch, in its colour: a box for a bar, a stroke for a line. */
function Swatch({ series, mark }: { series: number; mark: Mark }) {
  return (
    <span
      className={mark === "line" ? "swatch line" : "swatch"}
      style={css({ "--c": colour(series) })}
      aria-hidden="true"
    />
  );
}

/**
 * The period the key `key` picks on a plot of `periods` showing `at`: the one before or after it,
 * the first or the last, else the last where none is shown; none for another key.
 */
export function keyed(key: string, at: number | null, periods: number): number | null {
  const last = Math.max(periods - 1, 0);
  let by: number;
  switch (key) {
    case "ArrowLeft":
      by = -1;
      break;
    case "ArrowRight":
      by = 1;
      break;
    case "Home":
      by = -last;
      break;
    case "End":
      by = last;
      break;
    default:
      return null;
  }
  return at === null ? last : Math.min(Math.max(at + by, 0), last);
}

export interface ChartCardProps {
  chart: ChartData;
  axis: Axis;
  selected: number | null;
  onSelect: (at: number) => void;
  /** Takes the key (`seriesKey`) of the series a legend's button hides or shows again. */
  onToggle?: (key: string) => void;
}

/** One chart. */
export function ChartCard({ chart, axis, selected, onSelect, onToggle }: ChartCardProps) {
  const [table, setTable] = useState(false);
  const periods = axis.periods.length;
  const titleId = `chart-${chart.id}-title`;
  if (chart.instead !== null) {
    return (
      <figure className="chart" id={`chart-${chart.id}`} aria-labelledby={titleId}>
        <figcaption>
          <h3 id={titleId}>{chart.title}</h3>
          <p className="chart-note">{chart.note}</p>
        </figcaption>
        <p className="chart-instead">{chart.instead}</p>
      </figure>
    );
  }
  const scale = scaleOf(chart);
  const at = shown(chart, selected, periods);
  const onKeyDown = (event: KeyboardEvent) => {
    const next = keyed(event.key, at, periods);
    if (next === null) return;
    event.preventDefault();
    onSelect(next);
  };
  return (
    <figure className="chart" id={`chart-${chart.id}`} aria-labelledby={titleId}>
      <figcaption>
        <h3 id={titleId}>{chart.title}</h3>
        <p className="chart-note">{chart.note}</p>
      </figcaption>
      {chart.series.length > 1 && (
        <ul className="chart-legend" aria-label={`${chart.title}: series shown`}>
          {chart.series.map((data, series) => (
            <li key={data.name}>
              <button
                className="chart-key"
                type="button"
                aria-pressed={!data.hidden}
                onClick={() => onToggle?.(seriesKey(chart.id, data.name))}
              >
                <Swatch series={series} mark={data.mark} />
                {data.name}
              </button>
            </li>
          ))}
        </ul>
      )}
      <Readout chart={chart} axis={axis} at={at} />
      {/* biome-ignore lint/a11y/useSemanticElements: a group of periods, as the Dioxus page draws it */}
      <div
        className="chart-plot"
        // biome-ignore lint/a11y/noNoninteractiveTabindex: the plot is a group the arrow keys move through
        tabIndex={0}
        role="group"
        aria-label={`${chart.title}: the left and right arrow keys move between periods`}
        onKeyDown={onKeyDown}
      >
        <div className="chart-y" aria-hidden="true">
          {scale.ticks.map((tick) => (
            <span key={tick} style={{ bottom: `${short(place(scale, tick), 2)}%` }}>
              {format(chart.unit, tick)}
            </span>
          ))}
        </div>
        <div className="chart-area">
          {scale.ticks.map((tick) => (
            <div
              key={tick}
              className={tick === 0 ? "chart-grid zero" : "chart-grid"}
              style={{ bottom: `${short(place(scale, tick), 2)}%` }}
            />
          ))}
          {axis.marks.map(([left, , text]) => (
            <div
              key={left}
              className="chart-mark"
              style={{ left: `${short(left, 2)}%` }}
              title={text}
            />
          ))}
          <div className="chart-cols" style={css({ "--n": String(periods) })}>
            {upTo(periods).map((period) => (
              // biome-ignore lint/a11y/useKeyWithClickEvents lint/a11y/noStaticElementInteractions: the plot's keys move between the periods a tap picks
              <div
                key={period}
                className={`chart-col${gap(chart, period) ? " gap" : ""}${at === period ? " on" : ""}`}
                onClick={() => onSelect(period)}
              >
                {segments(chart, period, scale).map((part) => (
                  <span
                    key={part.series}
                    className={`seg ${part.up ? "up" : "down"}${part.end ? " end" : ""}${part.after ? " after" : ""}`}
                    style={css({
                      bottom: `${short(part.bottom, 2)}%`,
                      height: `${short(part.height, 2)}%`,
                      "--c": colour(part.series),
                    })}
                  />
                ))}
              </div>
            ))}
          </div>
          <svg
            className="chart-lines"
            viewBox={`0 0 ${periods} 100`}
            preserveAspectRatio="none"
            aria-hidden="true"
          >
            {chart.series.map(
              (data, series) =>
                data.mark === "line" &&
                !data.hidden && (
                  <LineSeries
                    key={data.name}
                    series={series}
                    values={data.values}
                    scale={scale}
                    at={at}
                  />
                ),
            )}
          </svg>
        </div>
      </div>
      <div className="chart-x" aria-hidden="true">
        {axis.ticks.map(([tick, label]) => (
          <span
            key={tick}
            style={{ left: `${short(((tick + 0.5) / Math.max(periods, 1)) * 100, 2)}%` }}
          >
            {label}
          </span>
        ))}
      </div>
      <button
        className="chart-table-toggle"
        type="button"
        aria-expanded={table}
        onClick={() => setTable(!table)}
      >
        {table ? "Hide the table" : "Show the table"}
      </button>
      {table && <ChartTable chart={chart} axis={axis} />}
    </figure>
  );
}

interface LineSeriesProps {
  series: number;
  values: (number | null)[];
  scale: Scale;
  at: number | null;
}

/** A line series' strokes, its dots alone between gaps and the period shown ringed. */
function LineSeries({ series, values, scale, at }: LineSeriesProps) {
  const [lines, dots] = runs(values, scale);
  const value = at === null ? null : (values[at] ?? null);
  const x = at === null ? 0 : at + 0.5;
  const y = value === null ? "" : short(100 - place(scale, value), 2);
  return (
    <g stroke={colour(series)}>
      {lines.map((points) => (
        <polyline key={points} points={points} />
      ))}
      {dots.map(([dot, high]) => (
        <line key={dot} className="dot" x1={dot} y1={short(high, 2)} x2={dot} y2={short(high, 2)} />
      ))}
      {value !== null && (
        <>
          <line className="ring" x1={x} y1={y} x2={x} y2={y} />
          <line className="dot" x1={x} y1={y} x2={x} y2={y} />
        </>
      )}
    </g>
  );
}

/** The values of the period `at` shows, one line per series shown, in the text's own ink. */
export function Readout({ chart, axis, at }: { chart: ChartData; axis: Axis; at: number | null }) {
  if (at === null) {
    return (
      <div className="chart-readout" aria-live="polite">
        <p className="chart-when">No period recorded yet</p>
      </div>
    );
  }
  const part = chart.cover[at];
  const whole = axis.hours[at];
  const recorded = part === undefined || whole === undefined ? null : coverage(part, whole);
  const empty = gap(chart, at);
  return (
    <div className="chart-readout" aria-live="polite">
      <p className="chart-when">
        <strong>{axis.periods[at] ?? ""}</strong>
        {empty && " · no data recorded"}
        {recorded !== null && ` · ${recorded}`}
      </p>
      {!empty && (
        <ul>
          {chart.series.map((data, series) => {
            if (data.hidden) return null;
            const value = valueAt(chart, series, at);
            return (
              <li key={data.name}>
                <Swatch series={series} mark={data.mark} />
                {`${data.name} `}
                <strong>{value === null ? "no data" : format(chart.unit, value)}</strong>
              </li>
            );
          })}
        </ul>
      )}
      {axis.marks
        .filter(([, period]) => period === at)
        .map(([, , text]) => (
          <p key={text} className="chart-mark-note">
            {`${text.charAt(0).toUpperCase()}${text.slice(1)}`}
          </p>
        ))}
    </div>
  );
}

/**
 * Every period's values of the series shown, oldest first: what the chart draws, for a reader who
 * reads it as text.
 */
export function ChartTable({ chart, axis }: { chart: ChartData; axis: Axis }) {
  const columns = chart.series
    .map((data, series) => ({ name: data.name, hidden: data.hidden, series }))
    .filter((column) => !column.hidden);
  return (
    <div className="chart-table">
      <table>
        <caption>{`${chart.title}, by period`}</caption>
        <thead>
          <tr>
            <th scope="col">Period</th>
            {columns.map((column) => (
              <th key={column.name} scope="col">
                {column.name}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {upTo(axis.periods.length).map((at) => (
            <tr key={at}>
              <th scope="row">{axis.periods[at]}</th>
              {columns.map((column) => {
                const value = valueAt(chart, column.series, at);
                return (
                  <td key={column.name}>{value === null ? "–" : format(chart.unit, value)}</td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/** A chart still to come: its box, a bar for its title and note, and its plot's place. */
export function GhostChart() {
  return (
    <div className="ghost-chart" aria-hidden="true">
      <span className="bone" style={css({ "--w": "40%" })} />
      <span className="bone" style={css({ "--w": "70%" })} />
      <div className="ghost-plot" />
    </div>
  );
}
