// One chart of the Reports page, a period per column on the page's one time axis: bars as boxes of
// the page (their width at most 24 px whatever the screen, their data end rounded, a 2 px gap of the
// card between stacked parts), lines as one SVG over them whose strokes keep their width however
// the plot stretches. A period no file recorded is hatched, never drawn as a zero, and a line breaks
// across it. A tap or a key on a period shows every series' value in the chart's readout, on every
// chart at once; the table under it says them all. Each entry of a legend is a button that hides its
// series or shows it again: a hidden series leaves the plot, the readout and the table, and the axis
// scales to the series shown.
//
// The Landed tasks tab draws three more shapes with the same card, legend, readout and table, and
// no library: the same columns over categories in place of periods (`ChartData.categories`), each
// column's total over it and its name under it; bars by row (`RowChart`), a row a name with its
// parts stacked along one scale; and dots (`DotChart`), a point a thing on two log scales. A tap or
// the arrow keys pick a row or a dot, as they pick a period. Every shape draws its card, its legend
// and its table's button with the same parts (`Frame`, `Legend`, `TableToggle`), and reads its
// plot's keys through one `stepper`.

import { type CSSProperties, type KeyboardEvent, type ReactNode, useState } from "react";
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
  unit: Unit;
  /** Its series in their fixed order: the first takes `--chart-1`, the next `--chart-2`, and so on. */
  series: SeriesData[];
  /** How much of each period its source recorded, from 0 to 1. */
  cover: number[];
  /** What it says in place of the plot when the window cannot show it (lead time by the hour). */
  instead: string | null;
  /**
   * What the readout says of each period its source recorded with no value in any series (`no task
   * landed`): such a period is drawn empty, not hatched. Without it a period with no value is a gap.
   */
  none?: string[];
  /** What the readout says after each period's name (`8 landed`), none where it says nothing. */
  whens?: (string | null)[];
  /** What the readout says after the series' values in each period (`median $13.39 a task`). */
  extras?: string[][];
  /**
   * Columns over categories rather than periods of time: each one's name and the line under it
   * (`1 review`, `111 tasks`), written under the plot in place of the axis's labels, with each
   * column's total over it. Its axis names each category in its readout (`categoryAxis`).
   */
  categories?: [string, string][];
}

/** The value of the `series`th series of `chart` in period `at`. */
export function valueAt(chart: ChartData, series: number, at: number): number | null {
  return chart.series[series]?.values[at] ?? null;
}

/** Whether some series of `chart` has a value in period `at`, hidden or not. */
export function valued(chart: ChartData, at: number): boolean {
  return chart.series.some((_, series) => valueAt(chart, series, at) !== null);
}

/**
 * Whether period `at` of `chart` is a gap, hatched: no series has a value in it, hidden or not, and,
 * for a chart that says what a period with none is (`none`), its source recorded none of it. Hiding
 * a series hides its values, never its record.
 */
export function gap(chart: ChartData, at: number): boolean {
  if (valued(chart, at)) return false;
  return chart.none === undefined || (chart.cover[at] ?? 0) <= 0;
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
      .find((at) => valued(chart, at)) ?? null
  );
}

/**
 * The axis of a chart of columns over categories, each named in the readout and its table by
 * `names` (`Landed after 2 reviews · 29 tasks`): no hour to cover, no label under the plot (the
 * chart writes its categories there) and no change marked.
 */
export function categoryAxis(names: string[]): Axis {
  return { periods: names, hours: names.map(() => 0), ticks: [], marks: [] };
}

/** The total of the bars of period `at` of `chart` that it shows, stacked up from zero. */
export function stacked(chart: ChartData, at: number): number {
  return sum(
    chart.series.flatMap((data, series) =>
      data.hidden || data.mark !== "bar" ? [] : [Math.max(valueAt(chart, series, at) ?? 0, 0)],
    ),
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
 * What every shape of chart says over its plot: its title alone, no note under it that says what it
 * shows (RULES.md `code/pages-explain-nothing`).
 */
interface Head {
  id: string;
  title: string;
}

/** A chart's card, of every shape: its title, then what `children` draw. */
function Frame({
  chart,
  className,
  children,
}: {
  chart: Head;
  className: string;
  children: ReactNode;
}) {
  const titleId = `chart-${chart.id}-title`;
  return (
    <figure className={className} id={`chart-${chart.id}`} aria-labelledby={titleId}>
      <figcaption>
        <h3 id={titleId}>{chart.title}</h3>
      </figcaption>
      {children}
    </figure>
  );
}

/**
 * The legend of a chart of more than one series, none for one: a button a series, each with its
 * swatch, that hides it or shows it again.
 */
function Legend<T extends { name: string; hidden: boolean }>({
  chart,
  swatch,
  onToggle,
}: {
  chart: Head & { series: T[] };
  swatch: (data: T, series: number) => ReactNode;
  onToggle: ((key: string) => void) | undefined;
}) {
  if (chart.series.length < 2) return null;
  return (
    <ul className="chart-legend" aria-label={`${chart.title}: series shown`}>
      {chart.series.map((data, series) => (
        <li key={data.name}>
          <button
            className="chart-key"
            type="button"
            aria-pressed={!data.hidden}
            onClick={() => onToggle?.(seriesKey(chart.id, data.name))}
          >
            {swatch(data, series)}
            {data.name}
          </button>
        </li>
      ))}
    </ul>
  );
}

/** A chart's table under its button, which says `label` while it is closed: made once opened. */
function TableToggle({ label, table }: { label: string; table: () => ReactNode }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button
        className="chart-table-toggle"
        type="button"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
      >
        {open ? "Hide the table" : label}
      </button>
      {open && table()}
    </>
  );
}

/** A plot's keys: the place `step` names for a key picked, and that key's own action prevented. */
function stepper(step: (key: string) => number | null, onSelect: (at: number) => void) {
  return (event: KeyboardEvent) => {
    const next = step(event.key);
    if (next === null) return;
    event.preventDefault();
    onSelect(next);
  };
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
  const periods = axis.periods.length;
  if (chart.instead !== null) {
    return (
      <Frame chart={chart} className="chart">
        <p className="chart-instead">{chart.instead}</p>
      </Frame>
    );
  }
  const scale = scaleOf(chart);
  const at = shown(chart, selected, periods);
  const onKeyDown = stepper((key) => keyed(key, at, periods), onSelect);
  return (
    <Frame chart={chart} className="chart">
      <Legend
        chart={chart}
        swatch={(data, series) => <Swatch series={series} mark={data.mark} />}
        onToggle={onToggle}
      />
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
                    className={`seg ${part.up ? "up" : "down"}${part.end ? " end" : ""}${part.after ? " after" : ""}${chart.categories ? " cat" : ""}`}
                    style={css({
                      bottom: `${short(part.bottom, 2)}%`,
                      height: `${short(part.height, 2)}%`,
                      "--c": colour(part.series),
                    })}
                  />
                ))}
                {chart.categories && stacked(chart, period) > 0 && (
                  <span
                    className="cat-total"
                    style={{ bottom: `${short(place(scale, stacked(chart, period)), 2)}%` }}
                  >
                    {format(chart.unit, stacked(chart, period))}
                  </span>
                )}
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
      {chart.categories ? (
        <div className="cat-x" style={css({ "--n": String(periods) })} aria-hidden="true">
          {chart.categories.map(([name, under]) => (
            <span key={name}>
              <b>{name}</b>
              {under}
            </span>
          ))}
        </div>
      ) : (
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
      )}
      <TableToggle label="Show the table" table={() => <ChartTable chart={chart} axis={axis} />} />
    </Frame>
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
  // A period its source recorded with nothing to draw, said in the chart's own words.
  const none = !empty && !valued(chart, at) ? (chart.none?.[at] ?? null) : null;
  const when = chart.whens?.[at] ?? null;
  return (
    <div className="chart-readout" aria-live="polite">
      <p className="chart-when">
        <strong>{axis.periods[at] ?? ""}</strong>
        {empty && " · no data recorded"}
        {none !== null && ` · ${none}`}
        {when !== null && ` · ${when}`}
        {recorded !== null && ` · ${recorded}`}
      </p>
      {!empty && none === null && (
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
          {(chart.extras?.[at] ?? []).map((extra) => (
            <li key={extra}>{extra}</li>
          ))}
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
 * reads it as text. A chart of columns over categories lists each by its readout's name.
 */
export function ChartTable({ chart, axis }: { chart: ChartData; axis: Axis }) {
  const columns = chart.series
    .map((data, series) => ({ name: data.name, hidden: data.hidden, series }))
    .filter((column) => !column.hidden);
  return (
    <div className="chart-table">
      <table>
        <caption>{chart.categories ? chart.title : `${chart.title}, by period`}</caption>
        <thead>
          <tr>
            <th scope="col">{chart.categories ? "Group" : "Period"}</th>
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

/** A table a chart of rows or dots lists: its caption, its heads, and a row a thing, its first cell
 * a link where the thing has a page. */
export interface TableData {
  caption: string;
  head: string[];
  rows: { key: string; cells: string[]; href: string | null }[];
}

/** `table` drawn as a chart's table: it scrolls within its chart, never widening the page. */
export function DataTable({ table }: { table: TableData }) {
  return (
    <div className="chart-table">
      <table>
        <caption>{table.caption}</caption>
        <thead>
          <tr>
            {table.head.map((head) => (
              <th key={head} scope="col">
                {head}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {table.rows.map((row) => (
            <tr key={row.key}>
              {row.cells.map((cell, at) =>
                at === 0 ? (
                  <th key="name" scope="row">
                    {row.href === null ? cell : <a href={row.href}>{cell}</a>}
                  </th>
                ) : (
                  // A cell's place, not its text, names it: two cells of a row may say the same.
                  // biome-ignore lint/suspicious/noArrayIndexKey: a row's cells never move
                  <td key={at}>{cell}</td>
                ),
              )}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/** The scale of bars from zero to at most `most`, a round step apart: four ticks or so. */
export function rowScale(unit: Unit, most: number): Scale {
  const up = most > 0 ? most : leastStep(unit);
  const by = step(unit, up / 3);
  const high = Math.ceil(up / by) * by;
  const ticks: number[] = [];
  for (let tick = 0; tick <= high + by / 2; tick += by) ticks.push(tick);
  return { low: 0, high, ticks };
}

/**
 * One row of a chart of bars by row: its name, the page it opens (none for a thing with no page),
 * its parts' values in its chart's series' order, its own colour where its chart colours rows (each
 * kind its own) rather than parts, the line under its bar, and what the readout says after its
 * parts' values.
 */
export interface Row {
  key: string;
  label: string;
  href: string | null;
  values: number[];
  colour: string | null;
  meta: string | null;
  said: string[];
}

/** A chart of bars by row, ready to draw. */
export interface RowsData {
  id: string;
  title: string;
  unit: Unit;
  /**
   * The parts each bar stacks, in their fixed order: the first takes `--chart-1`, the next
   * `--chart-2`; a chart of one part draws no legend, its rows wearing their own colours.
   */
  series: { name: string; hidden: boolean }[];
  rows: Row[];
  /** What its table's button says, and the table it opens, made once it is opened. */
  tableToggle: string;
  table: () => TableData;
  /** Whether a row's name stands beside its bar rather than over it: short names, a kind's. */
  side: boolean;
}

/** The total of the parts of `row` that `chart` shows. */
export function rowTotal(chart: RowsData, row: Row): number {
  return sum(row.values.filter((_, part) => chart.series[part]?.hidden !== true)) + 0;
}

/** The colour of part `part` of `row`: the row's own where it has one, else its part's. */
function rowColour(row: Row, part: number): string {
  return row.colour ?? colour(part);
}

/** The arrow keys of a list of rows, read as the plot reads its left and right. */
export function rowKey(key: string): string {
  return key === "ArrowUp" ? "ArrowLeft" : key === "ArrowDown" ? "ArrowRight" : key;
}

export interface RowChartProps {
  chart: RowsData;
  /** The row the readout shows: the one picked, else the first. */
  selected: number | null;
  onSelect: (at: number) => void;
  onToggle?: (key: string) => void;
}

/** A chart of bars by row: a row a thing, its parts stacked along the one scale. */
export function RowChart({ chart, selected, onSelect, onToggle }: RowChartProps) {
  const count = chart.rows.length;
  const scale = rowScale(chart.unit, Math.max(0, ...chart.rows.map((row) => rowTotal(chart, row))));
  const at = selected !== null && selected < count ? selected : count > 0 ? 0 : null;
  const row = at === null ? undefined : chart.rows[at];
  const onKeyDown = stepper((key) => keyed(rowKey(key), at, count), onSelect);
  return (
    <Frame chart={chart} className={`chart hchart${chart.side ? " side" : ""}`}>
      <Legend
        chart={chart}
        swatch={(_, series) => <Swatch series={series} mark="bar" />}
        onToggle={onToggle}
      />
      <div className="chart-readout" aria-live="polite">
        <p className="chart-when">
          <strong>{row?.label ?? ""}</strong>
        </p>
        {row !== undefined && (
          <ul>
            {chart.series.map((data, part) =>
              data.hidden ? null : (
                <li key={data.name}>
                  <span
                    className="swatch"
                    style={css({ "--c": rowColour(row, part) })}
                    aria-hidden="true"
                  />
                  {`${data.name} `}
                  <strong>{format(chart.unit, row.values[part] ?? 0)}</strong>
                </li>
              ),
            )}
            {row.said.map((said) => (
              <li key={said}>{said}</li>
            ))}
          </ul>
        )}
      </div>
      <div className="hplot">
        <div className="hgrid" aria-hidden="true">
          {scale.ticks.map((tick) => (
            <span key={tick} style={{ left: `${short(place(scale, tick), 2)}%` }} />
          ))}
        </div>
        {/* biome-ignore lint/a11y/useSemanticElements: a group of rows, as the plot is a group of periods */}
        <div
          className="hbars"
          // biome-ignore lint/a11y/noNoninteractiveTabindex: the rows are a group the arrow keys move through
          tabIndex={0}
          role="group"
          aria-label={`${chart.title}: the up and down arrow keys move between rows`}
          onKeyDown={onKeyDown}
        >
          {chart.rows.map((each, index) => (
            // biome-ignore lint/a11y/useKeyWithClickEvents lint/a11y/noStaticElementInteractions: the group's keys move between the rows a tap picks
            <div
              key={each.key}
              className={`hbar${index === at ? " on" : ""}`}
              onClick={() => onSelect(index)}
            >
              {each.href === null ? (
                <span className="hbar-label">{each.label}</span>
              ) : (
                <a className="hbar-label" href={each.href}>
                  {each.label}
                </a>
              )}
              <span className="hbar-track">
                {each.values.map((value, part) =>
                  chart.series[part]?.hidden || value <= 0 ? null : (
                    <span
                      // biome-ignore lint/suspicious/noArrayIndexKey: a row's parts are its chart's series, in their fixed order
                      key={part}
                      className="hseg"
                      style={css({
                        width: `${short(place(scale, value), 2)}%`,
                        "--c": rowColour(each, part),
                      })}
                    />
                  ),
                )}
              </span>
              <span className="hbar-value">{format(chart.unit, rowTotal(chart, each))}</span>
              {each.meta !== null && <span className="hbar-meta">{each.meta}</span>}
            </div>
          ))}
        </div>
      </div>
      <div className="chart-x hx" aria-hidden="true">
        {scale.ticks.map((tick) => (
          <span key={tick} style={{ left: `${short(place(scale, tick), 2)}%` }}>
            {format(chart.unit, tick)}
          </span>
        ))}
      </div>
      <TableToggle label={chart.tableToggle} table={() => <DataTable table={chart.table()} />} />
    </Frame>
  );
}

/**
 * A log scale over `values`, each above zero: from the largest round value at or under the least
 * to the smallest above it at or over the most, a round value being one of `marks` (1 and 5, or 1
 * alone) times a power of ten; its ticks the round values between. With no value, 1 to 10.
 */
export function logScale(values: number[], marks: readonly number[]): Scale {
  const positive = values.filter((value) => value > 0 && Number.isFinite(value));
  if (positive.length === 0) return { low: 1, high: 10, ticks: [1, 10] };
  const least = Math.min(...positive);
  const most = Math.max(...positive);
  const round: number[] = [];
  for (
    let power = Math.floor(Math.log10(least)) - 1;
    power <= Math.ceil(Math.log10(most)) + 1;
    power++
  ) {
    // A power of ten below one by division, so 0.1 and 0.5 are written as they read.
    for (const mark of marks) round.push(power < 0 ? mark / 10 ** -power : mark * 10 ** power);
  }
  round.sort((a, b) => a - b);
  const low = round.filter((value) => value <= least).at(-1) ?? least;
  const high = round.find((value) => value >= most && value > low) ?? most;
  return { low, high, ticks: round.filter((value) => value >= low && value <= high) };
}

/** Where `value` stands along the log scale `scale`, 0 to 100. */
export function logPlace(scale: Scale, value: number): number {
  const span = Math.log10(scale.high) - Math.log10(scale.low);
  return span <= 0 ? 0 : ((Math.log10(value) - Math.log10(scale.low)) / span) * 100;
}

/** One dot: what it names, its page, where it stands (`x` across, `y` up), its series, and what
 * the readout says of it. */
export interface Dot {
  key: string;
  label: string;
  href: string | null;
  x: number;
  y: number;
  series: number;
  said: string[];
}

/** `parts` joined as a sentence lists them: `a`, `a and b`, `a, b and c`. */
function listed(parts: string[]): string {
  return parts.length < 2
    ? (parts[0] ?? "")
    : `${parts.slice(0, -1).join(", ")} and ${parts.at(-1)}`;
}

/** A chart of dots on two log scales, ready to draw. */
export interface DotsData {
  id: string;
  title: string;
  /** What the dots' places count across and up: `count` lines, `dollars`. */
  xUnit: Unit;
  yUnit: Unit;
  /** What the axis across is, written under it: `lines changed`. */
  xName: string;
  /** The series, each its own colour (`--chart-1`… or grey), hidden by its legend's button. */
  series: { name: string; colour: string; hidden: boolean }[];
  /** The dots, in the order the arrow keys step through them: across, then up. */
  dots: Dot[];
  /** The rows no dot stands for, each with why (`2 that changed no counted line`). */
  undrawn: string[];
  tableToggle: string;
  table: () => TableData;
}

/**
 * The dot `key` moves to from `selected`, by its place in `chart.dots`: the arrows step through the
 * dots shown, across and then up as they are listed, Home and End to either end. With none picked,
 * or one hidden since, the forward arrows and Home pick the first and the others the last. None
 * for any other key or with no dot shown.
 */
export function dotStep(chart: DotsData, selected: number | null, key: string): number | null {
  const order = chart.dots.flatMap((each, index) =>
    chart.series[each.series]?.hidden === true ? [] : [index],
  );
  const step = rowKey(key);
  const from = selected === null ? -1 : order.indexOf(selected);
  const next =
    from !== -1
      ? keyed(step, from, order.length)
      : step === "ArrowRight" || step === "Home"
        ? 0
        : step === "ArrowLeft" || step === "End"
          ? order.length - 1
          : null;
  return next === null ? null : (order[next] ?? null);
}

/** The round values a log axis of dollars ticks at, and one of counts. */
const DOLLAR_MARKS = [1, 5] as const;
const COUNT_MARKS = [1] as const;

export interface DotChartProps {
  chart: DotsData;
  /** The dot the readout shows, by its place in `chart.dots`: none until one is picked. */
  selected: number | null;
  onSelect: (at: number) => void;
  onToggle?: (key: string) => void;
}

/** A chart of dots: a thing a point, its place on two log scales; a tap names it. */
export function DotChart({ chart, selected, onSelect, onToggle }: DotChartProps) {
  const shownDots = chart.dots.filter((dot) => chart.series[dot.series]?.hidden !== true);
  const across = logScale(
    shownDots.map((dot) => dot.x),
    chart.xUnit === "dollars" ? DOLLAR_MARKS : COUNT_MARKS,
  );
  const up = logScale(
    shownDots.map((dot) => dot.y),
    chart.yUnit === "dollars" ? DOLLAR_MARKS : COUNT_MARKS,
  );
  const picked = selected === null ? undefined : chart.dots[selected];
  const dot = picked && chart.series[picked.series]?.hidden !== true ? picked : undefined;
  const onKeyDown = stepper((key) => dotStep(chart, selected, key), onSelect);
  return (
    <Frame chart={chart} className="chart dchart">
      <Legend
        chart={chart}
        swatch={(data) => (
          <span className="swatch dot" style={css({ "--c": data.colour })} aria-hidden="true" />
        )}
        onToggle={onToggle}
      />
      <div className="chart-readout" aria-live="polite">
        {dot === undefined ? (
          <p className="chart-when">
            {shownDots.length === 0 ? "No dot to show" : "Tap a dot to name it"}
          </p>
        ) : (
          <>
            <p className="chart-when">
              <strong>{dot.href === null ? dot.label : <a href={dot.href}>{dot.label}</a>}</strong>
            </p>
            <ul>
              {dot.said.map((said) => (
                <li key={said}>{said}</li>
              ))}
            </ul>
          </>
        )}
      </div>
      {/* biome-ignore lint/a11y/useSemanticElements: a group of dots, as the plot is a group of periods */}
      <div
        className="chart-plot dplot"
        // biome-ignore lint/a11y/noNoninteractiveTabindex: the dots are a group the arrow keys move through
        tabIndex={0}
        role="group"
        aria-label={`${chart.title}: the arrow keys move between dots`}
        onKeyDown={onKeyDown}
      >
        <div className="chart-y" aria-hidden="true">
          {up.ticks.map((tick) => (
            <span key={tick} style={{ bottom: `${short(logPlace(up, tick), 2)}%` }}>
              {format(chart.yUnit, tick)}
            </span>
          ))}
        </div>
        <div className="chart-area">
          {up.ticks.map((tick) => (
            <div
              key={tick}
              className="chart-grid"
              style={{ bottom: `${short(logPlace(up, tick), 2)}%` }}
            />
          ))}
          {across.ticks.map((tick) => (
            <div
              key={tick}
              className="chart-grid across"
              style={{ left: `${short(logPlace(across, tick), 2)}%` }}
            />
          ))}
          <div className="dots">
            {chart.dots.map((each, index) =>
              chart.series[each.series]?.hidden === true ? null : (
                // biome-ignore lint/a11y/useKeyWithClickEvents lint/a11y/noStaticElementInteractions: the group's keys move between the dots a tap picks
                <span
                  key={each.key}
                  className={`pt${index === selected ? " on" : ""}`}
                  style={css({
                    left: `${short(logPlace(across, each.x), 2)}%`,
                    bottom: `${short(logPlace(up, each.y), 2)}%`,
                    "--c": chart.series[each.series]?.colour ?? colour(0),
                  })}
                  onClick={() => onSelect(index)}
                />
              ),
            )}
          </div>
        </div>
      </div>
      <div className="chart-x" aria-hidden="true">
        {across.ticks.map((tick) => (
          <span key={tick} style={{ left: `${short(logPlace(across, tick), 2)}%` }}>
            {format(chart.xUnit, tick)}
          </span>
        ))}
      </div>
      <p className="chart-x-name">{chart.xName}</p>
      {chart.undrawn.length > 0 && (
        <p className="chart-instead">{`No dot for ${listed(chart.undrawn)}.`}</p>
      )}
      <TableToggle label={chart.tableToggle} table={() => <DataTable table={chart.table()} />} />
    </Frame>
  );
}

/** A chart still to come: its box, a bar for its title, and its plot's place. */
export function GhostChart() {
  return (
    <div className="ghost-chart" aria-hidden="true">
      <span className="bone" style={css({ "--w": "40%" })} />
      <div className="ghost-plot" />
    </div>
  );
}
