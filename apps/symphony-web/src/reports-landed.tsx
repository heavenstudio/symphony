// The Reports page's Landed tasks tab (`/reports?view=landed`): the top-level tasks landed in the
// window, each priced once with its subtasks at its last landing in it, by task rather than by
// period. It draws the read REPORTS_LANDED_PATH answers (crates/symphony-api's `LandedPage`): four
// figures, each a link to its chart, then four charts (chart.tsx): the dearest ten as bars, the
// median by kind as bars, the mean by review rounds as columns, and every task as a dot of its cost
// against the lines its landing changed. A kind is drawn in its own colour, a task given none grey
// and last. What no figure counts is said under them, as is what every dollar is.

import { type ReactNode, useState } from "react";
import {
  ChartCard,
  type ChartData,
  categoryAxis,
  DotChart,
  type DotsData,
  format,
  GhostChart,
  RowChart,
  type RowsData,
  seriesKey,
  short,
  type TableData,
} from "./chart";
import { isKind, KINDS } from "./kind";
import { href } from "./project";
import type { Figure } from "./reports";
import { field } from "./shared";
import { amount, dollars, estimate } from "./spend";
import { NearViewport } from "./viewport";
import type { Grain, KindRow, LandedFigures, LandedPage, LandedRow, ReviewRow } from "./wire";

/** Each kind's colour, the same on every chart: a red's the palette's red-orange. */
const KIND_COLOURS: Record<string, string> = {
  feature: "var(--chart-1)",
  fix: "var(--chart-3)",
  red: "var(--chart-2)",
  tests: "var(--chart-4)",
  docs: "var(--chart-5)",
};

/** The colour of a task of `kind`: grey for none, or for a kind this client does not know. */
export function kindColour(kind: string | null): string {
  return (kind !== null ? KIND_COLOURS[kind] : undefined) ?? "var(--chart-none)";
}

/** A kind as the charts name it: `No kind` for none. */
export function kindName(kind: string | null): string {
  return kind ?? "No kind";
}

/** The most rows the Dearest tasks chart draws; its table lists them all. */
export const DEAREST = 10;

/** The review rounds' last group, which holds every task reviewed as often or more. */
const REVIEWS_GROUPED = 4;

const isCount = (value: unknown) => Number.isSafeInteger(value) && (value as number) >= 0;
const isMoney = (value: unknown) => typeof value === "number" && Number.isFinite(value);
const isMaybe = (value: unknown, test: (value: unknown) => boolean) =>
  value === null || value === undefined || test(value);
const isText = (value: unknown) => typeof value === "string";

/** Whether every member `counts` names of `value` is a count and every one `money` names a sum. */
function holds(value: unknown, counts: string[], money: string[], maybe: string[] = []): boolean {
  return (
    counts.every((name) => isCount(field(value, name) ?? 0)) &&
    money.every((name) => isMoney(field(value, name) ?? 0)) &&
    maybe.every((name) => isMaybe(field(value, name), isMoney))
  );
}

/** The list at `name` of `value`, empty where it is missing; none where it is something else. */
function list(value: unknown, name: string): unknown[] | null {
  const found = field(value, name);
  if (found === undefined) return [];
  return Array.isArray(found) ? found : null;
}

/** `value` read as a landed task's row, none where a member the tab draws is not of its type. */
function readRow(value: unknown): LandedRow | null {
  const slug = field(value, "slug");
  const at = field(value, "at");
  const kind = field(value, "kind") ?? null;
  const lines = field(value, "lines") ?? null;
  const unpriced = field(value, "unpriced") ?? false;
  if (
    !isText(slug) ||
    !isMoney(at) ||
    !isMaybe(kind, isText) ||
    !isMaybe(lines, isCount) ||
    typeof unpriced !== "boolean" ||
    !holds(value, ["tokens", "reviews"], ["first", "later"])
  ) {
    return null;
  }
  return {
    slug: slug as string,
    at: at as number,
    kind: kind as string | null,
    first: (field(value, "first") as number | undefined) ?? 0,
    later: (field(value, "later") as number | undefined) ?? 0,
    tokens: (field(value, "tokens") as number | undefined) ?? 0,
    unpriced,
    reviews: (field(value, "reviews") as number | undefined) ?? 0,
    lines: lines as number | null,
  };
}

/** The members of `value` `names` names, each as it is or `fallback` where it is missing. */
function taken<T>(value: unknown, names: readonly string[], fallback: unknown): T {
  return Object.fromEntries(names.map((name) => [name, field(value, name) ?? fallback])) as T;
}

const FIGURE_COUNTS = ["tasks", "back", "lined", "lines", "unlined", "unread"];
const FIGURE_MONEY = ["cost", "later", "lined_cost"];
const FIGURE_SPREAD = ["median", "mean", "p90"];

/**
 * The Landed tasks read as this client draws it, or none for a value without what the tab draws:
 * the window, each task's row, the window's figures, by kind and by reviews, and the files not
 * read, each of its type. A member a newer server adds changes nothing; one left out is its zero,
 * its empty list or none.
 */
export function readLanded(value: unknown): LandedPage | null {
  const grain = field(value, "grain");
  const days = field(value, "days");
  const from = field(value, "from");
  const to = field(value, "to");
  const figures = field(value, "figures") ?? {};
  const tasks = list(value, "tasks");
  const kinds = list(value, "kinds");
  const reviews = list(value, "reviews");
  const unread = list(value, "unread");
  const kindsFrom = field(value, "kinds_from") ?? null;
  const prices = field(value, "prices") ?? "";
  const rows = tasks?.map(readRow) ?? null;
  if (
    !["hour", "day", "week"].includes(grain as string) ||
    !isCount(days) ||
    !isMoney(from) ||
    !isMoney(to) ||
    !isCount(field(value, "without") ?? 0) ||
    !holds(figures, FIGURE_COUNTS, FIGURE_MONEY, FIGURE_SPREAD) ||
    rows === null ||
    rows.some((row) => row === null) ||
    kinds === null ||
    !kinds.every(
      (kind) =>
        isMaybe(field(kind, "kind"), isText) &&
        holds(kind, ["tasks", "lines"], ["cost", "lined_cost"], ["median"]),
    ) ||
    reviews === null ||
    !reviews.every((row) => holds(row, ["reviews", "tasks"], ["first", "later"], ["median"])) ||
    unread === null ||
    !unread.every(isText) ||
    !isMaybe(kindsFrom, isMoney) ||
    !isText(prices)
  ) {
    return null;
  }
  return {
    grain: grain as Grain,
    days: days as number,
    from: from as number,
    to: to as number,
    tasks: rows as LandedRow[],
    without: (field(value, "without") as number | undefined) ?? 0,
    figures: {
      ...taken<LandedFigures>(figures, [...FIGURE_COUNTS, ...FIGURE_MONEY], 0),
      ...taken<Pick<LandedFigures, "median" | "mean" | "p90">>(figures, FIGURE_SPREAD, null),
    },
    kinds: kinds.map((kind) => ({
      ...taken<KindRow>(kind, ["tasks", "cost", "lined_cost", "lines"], 0),
      kind: (field(kind, "kind") as string | null | undefined) ?? null,
      median: (field(kind, "median") as number | null | undefined) ?? null,
    })),
    reviews: reviews.map((row) => ({
      ...taken<ReviewRow>(row, ["reviews", "tasks", "first", "later"], 0),
      median: (field(row, "median") as number | null | undefined) ?? null,
    })),
    kinds_from: kindsFrom as number | null,
    prices: prices as string,
    unread: unread as string[],
  };
}

/** `n` dollars to the whole dollar, its thousands apart: `$1,874`. */
export function wholeDollars(n: number): string {
  return `$${Math.round(n).toLocaleString("en-US")}`;
}

/** `n` things of `one`, its plural an `s` more: `1 task`, `29 tasks`. */
function counted(n: number, one: string): string {
  return `${n.toLocaleString("en-US")} ${one}${n === 1 ? "" : "s"}`;
}

/** How many reviews a task took, as a row's readout says it. */
export function reviewsSaid(reviews: number): string {
  return reviews === 0 ? "no review" : counted(reviews, "review");
}

/** A landing's lines as a readout says them: none read, none counted, or how many. */
export function linesSaid(lines: number | null): string {
  if (lines === null) return "lines not read";
  return lines === 0 ? "no line counted" : counted(lines, "line");
}

/** What 100 of `lines` cost at `cost`, in dollars; none with no line. */
export function per100(cost: number, lines: number | null): number | null {
  return lines === null || lines <= 0 ? null : (cost * 100) / lines;
}

/** A task's whole cost, as a figure says it: `+ unpriced` after where some tokens had none. */
function costOf(task: LandedRow): string {
  const cost = task.first + task.later;
  if (task.unpriced && cost === 0) return "unpriced";
  return task.unpriced ? `${dollars(cost)} + unpriced` : dollars(cost);
}

function missing(label: string, why: string): Figure {
  return { label, value: "–", detail: why };
}

/**
 * The tab's four figures, each with the id of the chart it sums up: how many landed with tokens
 * recorded, the median a task, what 100 changed lines cost and how many were back after review.
 */
export function landedFigures(page: LandedPage): [Figure, string][] {
  const { figures } = page;
  const n = page.tasks.length;
  const landed: Figure =
    n === 0 && page.without === 0
      ? missing("Landed", "no task landed in the window")
      : {
          label: "Landed",
          value: short(n, 0),
          detail:
            page.without === 0
              ? "with tokens recorded · none without"
              : `with tokens recorded · ${page.without} without, in no figure`,
        };
  const unpriced = n - figures.tasks;
  const median: Figure =
    figures.median === null
      ? missing("Median a task", n === 0 ? "no task landed with tokens" : "no task's tokens priced")
      : {
          label: "Median a task",
          value: dollars(figures.median),
          detail:
            `mean ${dollars(figures.mean ?? 0)} · p90 ${dollars(figures.p90 ?? 0)}` +
            (unpriced > 0 ? ` · ${unpriced} unpriced, in no figure` : ""),
        };
  const lineCost = per100(figures.lined_cost, figures.lines);
  const lines: Figure =
    lineCost === null
      ? missing(
          "Per 100 lines changed",
          n === 0 ? "no task landed with tokens" : "no landing's lines read in the window",
        )
      : {
          label: "Per 100 lines changed",
          value: dollars(lineCost),
          detail:
            `${counted(figures.lined, "task")}, ${format("count", figures.lines)} lines` +
            (figures.unlined > 0 ? ` · ${figures.unlined} changed none counted` : "") +
            (figures.unread > 0 ? ` · ${figures.unread} with no lines read` : ""),
        };
  const back: Figure =
    n === 0
      ? missing("Back after review", "no task landed with tokens")
      : {
          label: "Back after review",
          value: short(figures.back, 0),
          detail:
            `of ${n} needed a second review · review rounds took ` +
            `${wholeDollars(figures.later)} of ${wholeDollars(figures.cost)}`,
        };
  return [
    [landed, "dearest"],
    [median, "kinds"],
    [lines, "spread"],
    [back, "rounds"],
  ];
}

/** Every landed task as the tables list it: its cost, its parts, tokens, reviews and lines. */
function tasksTable(page: LandedPage, caption: string): TableData {
  return {
    caption,
    head: [
      "Task",
      "Kind",
      "Cost",
      "First round",
      "Review rounds",
      "Tokens",
      "Reviews",
      "Lines",
      "Per 100 lines",
    ],
    rows: page.tasks.map((task) => {
      const lineCost = per100(task.first + task.later, task.lines);
      return {
        key: task.slug,
        href: href(`/task/${task.slug}`),
        cells: [
          task.slug,
          task.kind ?? "none",
          costOf(task),
          dollars(task.first),
          dollars(task.later),
          amount(task.tokens),
          String(task.reviews),
          task.lines === null ? "–" : task.lines.toLocaleString("en-US"),
          lineCost === null ? "–" : dollars(lineCost),
        ],
      };
    }),
  };
}

/** The ten dearest tasks as bars, their work up to their first review and after it stacked. */
export function dearestChart(page: LandedPage, hidden: string[]): RowsData {
  const id = "dearest";
  const n = page.tasks.length;
  return {
    id,
    title: "Dearest tasks",
    note:
      `${n > DEAREST ? `The ${DEAREST} top-level tasks landed in the window that cost the most` : "Every top-level task landed in the window, the dearest first"}, ` +
      "each with its subtasks: the work up to its first review, and every round after it. Each " +
      "name opens the task.",
    unit: "dollars",
    series: ["First round", "Review rounds"].map((name) => ({
      name,
      hidden: hidden.includes(seriesKey(id, name)),
    })),
    rows: page.tasks.slice(0, DEAREST).map((task) => ({
      key: task.slug,
      label: task.slug,
      href: href(`/task/${task.slug}`),
      values: [task.first, task.later],
      colour: null,
      meta: null,
      said: [
        `${amount(task.tokens)} tokens`,
        reviewsSaid(task.reviews),
        linesSaid(task.lines),
        ...(task.unpriced ? ["some tokens unpriced, in no dollar"] : []),
      ],
    })),
    tableToggle: `Show all ${n.toLocaleString("en-US")} as a table`,
    table: () => tasksTable(page, "Every task landed in the window, the dearest first"),
    side: false,
  };
}

/** What a kind's row says under its bar: how many, what in all, what 100 lines cost. */
function kindSaid(kind: KindRow): string[] {
  const lineCost = per100(kind.lined_cost, kind.lines);
  return [
    counted(kind.tasks, "task"),
    `${dollars(kind.cost)} in all`,
    lineCost === null ? "no lines counted" : `${dollars(lineCost)} per 100 lines`,
  ];
}

/** The day a kind was first met in the window, as the By kind chart's note says it. */
function day(at: number): string {
  const date = new Date(at * 1000);
  const months = "Jan Feb Mar Apr May Jun Jul Aug Sep Oct Nov Dec".split(" ");
  return `${date.getUTCDate()} ${months[date.getUTCMonth()] ?? ""} ${date.getUTCFullYear()}`;
}

/** The median a task of each kind as bars, each in its kind's colour, No kind last. */
export function kindsChart(page: LandedPage): RowsData {
  const none = page.kinds.some((kind) => kind.kind === null);
  const from =
    page.kinds_from === null
      ? " No task landed in the window was filed with a kind."
      : none
        ? ` The first with a kind landed ${day(page.kinds_from)}.`
        : "";
  return {
    id: "kinds",
    title: "By kind",
    note:
      "The median cost of a task landed in the window, by the kind it was filed with, the kinds " +
      `that cost the most in all first; a task landed before kinds were kept counts under No kind.${from}`,
    unit: "dollars",
    series: [{ name: "Median a task", hidden: false }],
    rows: page.kinds.map((kind) => ({
      key: kindName(kind.kind),
      label: kindName(kind.kind),
      href: null,
      values: [kind.median ?? 0],
      colour: kindColour(kind.kind),
      meta: kindSaid(kind).join(" · "),
      said: kindSaid(kind),
    })),
    tableToggle: "Show the table",
    table: () => ({
      caption: "By kind",
      head: ["Kind", "Tasks", "Median a task", "In all", "Per 100 lines"],
      rows: page.kinds.map((kind) => {
        const lineCost = per100(kind.lined_cost, kind.lines);
        return {
          key: kindName(kind.kind),
          href: null,
          cells: [
            kindName(kind.kind),
            String(kind.tasks),
            kind.median === null ? "–" : dollars(kind.median),
            dollars(kind.cost),
            lineCost === null ? "–" : dollars(lineCost),
          ],
        };
      }),
    }),
    side: true,
  };
}

/** A number of reviews as a column names it: `No review`, `2 reviews`, `4 or more`. */
function reviewsName(reviews: number): string {
  if (reviews >= REVIEWS_GROUPED) return `${REVIEWS_GROUPED} or more`;
  return reviews === 0 ? "No review" : counted(reviews, "review");
}

/** The same, as the readout names the column it shows. */
function landedAfter(row: ReviewRow): string {
  const after =
    row.reviews === 0
      ? "Landed with no review"
      : row.reviews >= REVIEWS_GROUPED
        ? `Landed after ${REVIEWS_GROUPED} reviews or more`
        : `Landed after ${counted(row.reviews, "review")}`;
  return `${after} · ${counted(row.tasks, "task")}`;
}

/** The mean cost by review rounds as columns, the first round and the review rounds stacked. */
export function roundsChart(page: LandedPage, hidden: string[]): ChartData {
  const id = "rounds";
  const { reviews } = page;
  return {
    id,
    title: "By review rounds",
    note:
      "The mean cost of a task landed in the window, by how many reviews it took: its first " +
      "round, and the review rounds after it.",
    unit: "dollars",
    series: (
      [
        ["First round", (row: ReviewRow) => row.first],
        ["Review rounds", (row: ReviewRow) => row.later],
      ] as const
    ).map(([name, pick]) => ({
      name,
      mark: "bar" as const,
      values: reviews.map(pick),
      hidden: hidden.includes(seriesKey(id, name)),
    })),
    cover: reviews.map(() => 1),
    instead: null,
    extras: reviews.map((row) =>
      row.median === null ? [] : [`median ${dollars(row.median)} a task`],
    ),
    categories: reviews.map((row) => [reviewsName(row.reviews), counted(row.tasks, "task")]),
  };
}

/** The axis of `roundsChart`'s columns, each named as the readout says it. */
export function roundsAxis(page: LandedPage) {
  return categoryAxis(page.reviews.map(landedAfter));
}

/** `parts` joined as a sentence lists them: `a`, `a and b`, `a, b and c`. */
function listed(parts: string[]): string {
  return parts.length < 2
    ? (parts[0] ?? "")
    : `${parts.slice(0, -1).join(", ")} and ${parts.at(-1)}`;
}

/** Every task as a dot, its cost against its lines, in its kind's colour. */
export function spreadChart(page: LandedPage, hidden: string[]): DotsData {
  const id = "spread";
  const drawn = page.tasks
    .filter((task) => task.lines !== null && task.lines > 0 && task.first + task.later > 0)
    .sort(
      (a, b) =>
        (a.lines ?? 0) - (b.lines ?? 0) ||
        a.first + a.later - (b.first + b.later) ||
        (a.slug < b.slug ? -1 : 1),
    );
  const kinds: (string | null)[] = [
    ...KINDS.filter((kind) => drawn.some((task) => task.kind === kind)),
    ...(drawn.some((task) => !isKind(task.kind)) ? [null] : []),
  ];
  const kindOf = (task: LandedRow) => (isKind(task.kind) ? task.kind : null);
  const unlined = page.tasks.filter((task) => task.lines === 0).length;
  const unread = page.tasks.filter((task) => task.lines === null).length;
  const unpriced = page.tasks.filter(
    (task) => task.lines !== null && task.lines > 0 && task.first + task.later <= 0,
  ).length;
  const left = [
    ...(unlined > 0 ? [`${unlined} that changed no counted line`] : []),
    ...(unread > 0 ? [`${unread} landed before lines were kept`] : []),
    ...(unpriced > 0 ? [`${unpriced} with no price`] : []),
  ];
  return {
    id,
    title: "Every task, cost against size",
    note:
      "Each top-level task landed in the window, its cost with its subtasks against the lines its " +
      "landing changed, both on a log scale: a dot above the others cost more for its size. " +
      "Generated files and lock files are left out." +
      (left.length > 0 ? ` No dot for ${listed(left)}.` : ""),
    xUnit: "count",
    yUnit: "dollars",
    xName: "lines changed",
    series: kinds.map((kind) => ({
      name: kindName(kind),
      colour: kindColour(kind),
      hidden: hidden.includes(seriesKey(id, kindName(kind))),
    })),
    dots: drawn.map((task) => {
      const cost = task.first + task.later;
      const lines = task.lines ?? 0;
      return {
        key: task.slug,
        label: task.slug,
        href: href(`/task/${task.slug}`),
        x: lines,
        y: cost,
        series: kinds.indexOf(kindOf(task)),
        said: [
          costOf(task),
          linesSaid(lines),
          `${dollars(per100(cost, lines) ?? 0)} per 100 lines`,
        ],
      };
    }),
    tableToggle: "Show the table",
    table: () => ({
      caption: "Every dot, by its lines",
      head: ["Task", "Kind", "Cost", "Lines", "Per 100 lines"],
      rows: drawn.map((task) => ({
        key: task.slug,
        href: href(`/task/${task.slug}`),
        cells: [
          task.slug,
          task.kind ?? "none",
          costOf(task),
          (task.lines ?? 0).toLocaleString("en-US"),
          dollars(per100(task.first + task.later, task.lines) ?? 0),
        ],
      })),
    }),
  };
}

/** What no figure of the tab counts, said under it. */
export const LEFT_OUT =
  "No figure here counts a Codex session's tokens, which no hook records, so a task a Codex " +
  "agent worked looks cheap; the orchestrators' tokens, which are on no task and count only in " +
  "All in, under Over time; or a session's last answer after its last tool call.";

/** The sentence that opens the tab, saying what it counts. */
export function landedIntro(page: LandedPage): string {
  const n = page.tasks.length;
  const tasks =
    n === 1 ? "The one top-level task" : `The ${n.toLocaleString("en-US")} top-level tasks`;
  return (
    `${tasks} landed in the window with tokens recorded, each priced once with its subtasks, at ` +
    `its landing: by task, not by ${page.grain}.`
  );
}

/** What the tab says in place of its charts when no task landed with tokens recorded. */
export function landedEmpty(page: LandedPage): string {
  return page.without === 0
    ? "No top-level task landed in the window."
    : `${counted(page.without, "top-level task")} landed in the window, none with tokens recorded: no figure to draw.`;
}

/** The four figures, each a link to its chart while the charts are drawn. */
export function FigureCards({ figures, linked }: { figures: [Figure, string][]; linked: boolean }) {
  return (
    <dl className="reports-figures">
      {figures.map(([figure, chart]) => (
        <div key={figure.label} className="reports-figure">
          <dt>{linked ? <a href={`#chart-${chart}`}>{figure.label}</a> : figure.label}</dt>
          <dd className="value">{figure.value}</dd>
          <dd className="detail">{figure.detail}</dd>
        </div>
      ))}
    </dl>
  );
}

export interface LandedViewProps {
  page: LandedPage;
  hidden: string[];
  onToggle: ((key: string) => void) | undefined;
  /** Whether the chart the address names lands as the tab draws, as the Over time tab's do. */
  lands: boolean;
}

/** The ghost heights of the four charts, near what each draws with ten rows. */
const HEIGHTS = { dearest: 520, kinds: 420, rounds: 400, spread: 440 };

/**
 * The tab drawn: its sentence, the files not read, the four figures and, with any task landed
 * with tokens recorded, its four charts, each held in its ghost until it nears the screen; what
 * every dollar is and what no figure counts under them. The row, column or dot picked is each
 * chart's own.
 */
export function LandedView({ page, hidden, onToggle, lands }: LandedViewProps) {
  const [dearest, setDearest] = useState<number | null>(null);
  const [kind, setKind] = useState<number | null>(null);
  const [rounds, setRounds] = useState<number | null>(null);
  const [dot, setDot] = useState<number | null>(null);
  const drawn = page.tasks.length > 0;
  const unread = page.unread.length;
  const toggle = onToggle ? { onToggle } : {};
  const slot = (id: keyof typeof HEIGHTS, chart: ReactNode) => (
    <NearViewport
      key={id}
      height={HEIGHTS[id]}
      className="chart-slot"
      anchors={[`chart-${id}`]}
      lands={lands}
      ghost={<GhostChart />}
    >
      {chart}
    </NearViewport>
  );
  return (
    <section className="reports-section reports-landed" aria-label="Landed tasks">
      {drawn && <p className="reports-sub">{landedIntro(page)}</p>}
      {unread > 0 && (
        <details className="reports-unread">
          <summary>
            {`${unread} ${unread === 1 ? "file" : "files"} not read: the tasks they hold are missing`}
          </summary>
          <ul>
            {page.unread.map((line) => (
              <li key={line}>{line}</li>
            ))}
          </ul>
        </details>
      )}
      <FigureCards figures={landedFigures(page)} linked={drawn} />
      {drawn ? (
        <div className="reports-charts">
          {slot(
            "dearest",
            <RowChart
              chart={dearestChart(page, hidden)}
              selected={dearest}
              onSelect={setDearest}
              {...toggle}
            />,
          )}
          {slot("kinds", <RowChart chart={kindsChart(page)} selected={kind} onSelect={setKind} />)}
          {slot(
            "rounds",
            <ChartCard
              chart={roundsChart(page, hidden)}
              axis={roundsAxis(page)}
              selected={rounds}
              onSelect={setRounds}
              {...toggle}
            />,
          )}
          {slot(
            "spread",
            <DotChart
              chart={spreadChart(page, hidden)}
              selected={dot}
              onSelect={setDot}
              {...toggle}
            />,
          )}
        </div>
      ) : (
        <section className="reports-empty" aria-label="No landed task">
          <p>{landedEmpty(page)}</p>
        </section>
      )}
      <p className="reports-note">
        {`Every dollar here is ${estimate(page.prices === "" ? "on a day not named" : page.prices)}; ` +
          "a figure marked + unpriced leaves out the tokens of a model the price table does not name."}
      </p>
      <p className="reports-note">{LEFT_OUT}</p>
    </section>
  );
}
