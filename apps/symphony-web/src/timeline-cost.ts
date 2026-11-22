// What a task's tokens cost on its timeline (docs/workflow/TASKS.md, "What the tokens cost"): the total
// after the task's time, and each row of the board's record (a session's calls on the task in one
// role and one review round, `/api/task-tokens`'s `rows`) on the one stage stretch its calls were
// made for, so the stretches add up to the total. No DOM, and nothing of the task kind's own: the
// timeline's chunk carries it.
//
// A stretch's round is one more than the reviews started before it, a review's the round it
// opens, which is how the board counts a row's round.
//
// - An in-progress stretch carries the executor's rows of its round, and what the claiming
//   session spent while it held the task reserved; of two in one round (a block between them),
//   the last carries them.
// - A review stretch carries its reviewer's rows of the round it opens, and the executor's of
//   that round when no in-progress stretch follows (fixes made while an approving review ran).
// - A row the rule gives no stretch (a subtask's, whose round is its own task's; a session that
//   worked no task, round 0; another role; a round with no stretch of its kind) goes by the time
//   of its first call: to the in-progress or review stretch that holds it, else the last begun
//   before it, else the first. A task never in progress or in review carries them on its last
//   stretch still drawn.
//
// A row is never split and nothing is apportioned by time: the record keeps a row's first and
// last call, which a stretch's words say, so a reader sees where a figure spills past its edge.

import { field } from "./shared";
import {
  amount,
  calls,
  dollars,
  estimate,
  figure,
  isCost,
  isTokens,
  total,
  unpricedSaid,
} from "./spend";
import type { AgentCost, Stage, TaskTokens, TokenRow } from "./wire";

/** Statuses a task ends in: an instant on the timeline, which no lane draws. */
const CLOSED = ["done", "cancelled"];

const spent = (value: unknown) => isTokens(field(value, "tokens")) && isCost(field(value, "cost"));

function isRow(value: unknown): value is TokenRow {
  return (
    typeof field(value, "slug") === "string" &&
    typeof field(value, "role") === "string" &&
    Number.isSafeInteger(field(value, "round")) &&
    Number.isFinite(field(value, "first")) &&
    Number.isFinite(field(value, "last")) &&
    spent(value)
  );
}

/** A task's tokens as the wire writes them, or none; a server that sends no rows sends none. */
export function readTaskTokens(value: unknown): TaskTokens | null {
  const roles = field(value, "by_role");
  const rows = field(value, "rows") ?? [];
  if (
    !isTokens(field(value, "total")) ||
    !isCost(field(value, "cost")) ||
    !Array.isArray(roles) ||
    !roles.every((role) => typeof field(role, "role") === "string" && spent(role)) ||
    !Array.isArray(rows) ||
    !rows.every(isRow)
  ) {
    return null;
  }
  return { ...(value as TaskTokens), rows };
}

/** Whether any model call was recorded: a task with none shows no cost anywhere. */
export function tookTokens(tokens: TaskTokens): boolean {
  return total(tokens.total) > 0 || tokens.total.calls > 0;
}

/** The figure of `cost`. */
export function costFigure(cost: AgentCost): string {
  return figure(cost.dollars, cost.unpriced.length > 0);
}

/**
 * What the total says of itself when it is pointed at, tapped or focused: that it is an estimate
 * and of what day's prices, its split by role and by kind, the tokens and the calls, and the
 * models no price was found for.
 */
export function totalWords(tokens: TaskTokens): string {
  const { total: all, cost } = tokens;
  const roles = tokens.by_role.map((role) => `${role.role} ${costFigure(role.cost)}`).join(" · ");
  const kinds =
    `input ${dollars(cost.input)} · cache write ` +
    `${dollars(cost.cache_write_5m + cost.cache_write_1h)} · cache read ` +
    `${dollars(cost.cache_read)} · output ${dollars(cost.output)}`;
  const priced = cost.dollars > 0 || cost.unpriced.length === 0;
  const first = priced
    ? `${dollars(cost.dollars)}, ${estimate(cost.prices)}`
    : `Unpriced: the API list prices read ${cost.prices} have none for these models`;
  return (
    first +
    (roles === "" ? "" : `: ${roles}`) +
    (priced ? ` (${kinds})` : "") +
    `; ${amount(total(all))} tokens in ${calls(all.calls)}` +
    (cost.unpriced.length > 0 ? `; no price for ${unpricedSaid(cost)}: in no figure` : "")
  );
}

/** What a stage stretch carries: the rows put on it, and what they come to. */
export interface Carried {
  rows: TokenRow[];
  dollars: number;
  /** Some of its tokens were of a model with no price, in no dollar. */
  unpriced: boolean;
  tokens: number;
  calls: number;
  /** The stretch of the same round, earlier, whose executor's calls are among these (a block
   * between the two): the index of its stage. */
  with: number[];
  /** The later stretch of its round that carries this one's executor's calls; none where it
   * carries its own. */
  on: number | null;
}

const NOTHING = (): Carried => ({
  rows: [],
  dollars: 0,
  unpriced: false,
  tokens: 0,
  calls: 0,
  with: [],
  on: null,
});

/** The round of each of `stages` as the board counts a row's: one more than the reviews started
 * before it, a review's own among them. */
export function roundsOf(stages: Stage[]): number[] {
  let reviews = 0;
  return stages.map((stage) => {
    if (stage.status === "review") reviews = Math.max(reviews, stage.round ?? 0);
    return 1 + reviews;
  });
}

const working = (stage: Stage) => stage.status === "in_progress";
const reviewing = (stage: Stage) => stage.status === "review";

/** The stretch a row the rule places nowhere goes on, by the time `at` of its first call. */
function byTime(stages: Stage[], at: number): number {
  const carrying = stages.flatMap((stage, index) =>
    working(stage) || reviewing(stage) ? [index] : [],
  );
  if (carrying.length === 0) {
    const drawn = stages.findLastIndex((stage) => !CLOSED.includes(stage.status));
    return Math.max(drawn, 0);
  }
  const begun = carrying.filter((index) => (stages[index] as Stage).from <= at);
  return begun.at(-1) ?? (carrying[0] as number);
}

/**
 * What each of `stages` carries of `rows`, the task's own being those of `slug`: every row on
 * exactly one stretch, so they add up to the task's total. None for a task with no stage.
 */
export function carried(stages: Stage[], rows: TokenRow[], slug: string): Carried[] {
  const on = stages.map(NOTHING);
  if (stages.length === 0) return on;
  const rounds = roundsOf(stages);
  const inRound = (round: number, kind: (stage: Stage) => boolean) =>
    stages.flatMap((stage, index) => (kind(stage) && rounds[index] === round ? [index] : []));
  for (const row of rows) {
    const own = row.slug === slug && row.round >= 1;
    const work = inRound(row.round, working);
    // The review that opens the round: the first of its stretches, before any wait to land.
    const review = inRound(row.round, reviewing)[0];
    let index: number | undefined;
    if (own && row.role === "reviewer") index = review;
    else if (own && (row.role === "executor" || row.role === "reserved")) {
      index = work.at(-1) ?? review;
      if (row.role === "executor" && work.length > 1) {
        const last = work.at(-1) as number;
        for (const earlier of work.slice(0, -1)) {
          (on[earlier] as Carried).on = last;
          if (!(on[last] as Carried).with.includes(earlier))
            (on[last] as Carried).with.push(earlier);
        }
      }
    }
    const held = on[index ?? byTime(stages, row.first)] as Carried;
    held.rows.push(row);
    held.dollars += row.cost.dollars;
    held.unpriced ||= row.cost.unpriced.length > 0;
    held.tokens += total(row.tokens);
    held.calls += row.tokens.calls;
  }
  return on;
}

/** What a stretch's label and its phone row say after its time: its figure, or none where it
 * carries nothing. */
export function carriedFigure(held: Carried | undefined): string | null {
  return held === undefined || held.rows.length === 0 ? null : figure(held.dollars, held.unpriced);
}

/** A sum of the rows of one kind on a stretch. */
interface Part {
  name: string;
  dollars: number;
  unpriced: boolean;
  calls: number;
  first: number;
  last: number;
}

/** Whose calls a row's were: its role, a subtask's named. */
function partName(row: TokenRow, slug: string): string {
  return row.slug === slug ? row.role : `${row.role} on ${row.slug}`;
}

/**
 * What a stretch's words say of what it carries, after its time and its hours: whose calls they
 * were, by role, each with its cost, when they ran (first to last) and their count, and that the
 * figures are an estimate. `when` says a time; `from` names a stage by its index, for a round
 * whose executor's cost is on another stretch than the one read. Empty where there is nothing to
 * say.
 */
export function carriedWords(
  held: Carried | undefined,
  slug: string,
  prices: string,
  when: (at: number) => string,
  from: (index: number) => string,
): string {
  if (held === undefined) return "";
  if (held.rows.length === 0) {
    return held.on === null
      ? ""
      : `; its executor's calls are counted on the stretch from ${from(held.on)}, the last of its round`;
  }
  const parts: Part[] = [];
  for (const row of held.rows) {
    const name = partName(row, slug);
    let part = parts.find((one) => one.name === name);
    if (part === undefined) {
      part = { name, dollars: 0, unpriced: false, calls: 0, first: row.first, last: row.last };
      parts.push(part);
    }
    part.dollars += row.cost.dollars;
    part.unpriced ||= row.cost.unpriced.length > 0;
    part.calls += row.tokens.calls;
    part.first = Math.min(part.first, row.first);
    part.last = Math.max(part.last, row.last);
  }
  const said = parts.map((part) => {
    const [first, last] = [when(part.first), when(part.last)];
    const ran = first === last ? `at ${first}` : `${first}–${last}`;
    return `${part.name} ${figure(part.dollars, part.unpriced)}, ${ran}, in ${calls(part.calls)}`;
  });
  const whole =
    held.with.length === 0
      ? ""
      : `; all of its round's executor's calls, those of the stretch from ${held.with.map(from).join(" and ")} among them`;
  return `: ${said.join("; ")}${whole}; ${estimate(prices)}`;
}
