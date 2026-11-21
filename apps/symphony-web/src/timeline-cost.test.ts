import { describe, expect, it } from "vitest";
import { dollars } from "./spend";
import { CASES } from "./testing";
import {
  type Carried,
  carried,
  carriedFigure,
  carriedWords,
  costFigure,
  readTaskTokens,
  roundsOf,
  tookTokens,
  totalWords,
} from "./timeline-cost";
import type { AgentCost, AgentTokens, Stage, TaskTokens, TokenRow } from "./wire";

const SLUG = "tools/workflow-revision-guards";

/** The stages of the task the approved design draws, as its timeline recorded them: status,
 * from, to and, for a review, its number. */
const STAGES: Stage[] = (
  [
    ["ready", 1795046965.4, 1795072112.5, 0],
    ["in_progress", 1795072112.5, 1795083775.8, 0],
    ["review", 1795083775.8, 1795084284.5, 1],
    ["in_progress", 1795084284.5, 1795086628.6, 0],
    ["review", 1795086628.6, 1795086861.8, 2],
    ["done", 1795086861.8, 1795132641.4, 0],
    ["review", 1795132641.4, 1795134886.9, 3],
    ["in_progress", 1795134886.9, 1795139051.0, 0],
    ["review", 1795139051.0, 1795139175.7, 4],
    ["in_progress", 1795139175.7, 1795143034.0, 0],
    ["review", 1795143034.0, 1795143318.2, 5],
    ["in_progress", 1795143318.2, 1795145545.4, 0],
    ["review", 1795145545.4, 1795145576.0, 6],
    ["done", 1795145576.0, null, 0],
  ] as const
).map(([status, from, to, round]) => ({ status, from, to, round }));

function tokens(calls: number, input = 0): AgentTokens {
  return { input, cache_write_5m: 0, cache_write_1h: 0, cache_read: 0, output: 0, calls };
}

function cost(amount: number, unpriced: string[] = []): AgentCost {
  return {
    dollars: amount,
    input: amount,
    cache_write_5m: 0,
    cache_write_1h: 0,
    cache_read: 0,
    output: 0,
    unpriced: unpriced.map((model) => ({ model, speed: "standard", tokens: tokens(1, 1000) })),
    prices: "2026-11-16",
  };
}

function row(
  role: string,
  round: number,
  first: number,
  last: number,
  calls: number,
  amount: number,
  fields: Partial<TokenRow> = {},
): TokenRow {
  return {
    handle: `${role}-1`,
    slug: SLUG,
    role,
    model: "claude-opus-5-5",
    speed: "standard",
    round,
    tokens: tokens(calls, calls * 1000),
    cost: cost(amount),
    first,
    last,
    ...fields,
  };
}

/** That task's thirteen rows, as the board recorded them: $80.02 in 884 calls. */
const ROWS: TokenRow[] = [
  row("reviewer", 2, 1795083839.862279, 1795084288.631473, 48, 3.7834692),
  row("reviewer", 3, 1795086785.707946, 1795086860.981354, 8, 0.6444798),
  row("reviewer", 4, 1795134650.933333, 1795134907.099368, 29, 4.2336168),
  row("reviewer", 5, 1795139064.596429, 1795139185.398185, 14, 3.60281),
  row("reviewer", 6, 1795143037.715704, 1795145539.763228, 21, 4.7936542),
  row("reviewer", 7, 1795145548.146142, 1795145563.445489, 4, 0.2129138),
  row("executor", 1, 1795072115.883158, 1795083775.383301, 349, 30.430627),
  row("executor", 2, 1795083777.281189, 1795086625.691532, 94, 7.0338238),
  row("executor", 3, 1795086630.197779, 1795086639.993331, 2, 0.0760884),
  row("executor", 4, 1795132665.307121, 1795139033.895445, 160, 13.225875),
  row("executor", 5, 1795139053.867474, 1795143030.568916, 124, 9.211979),
  row("executor", 6, 1795143036.657329, 1795145544.436137, 28, 2.443202),
  row("reserved", 1, 1795072104.565521, 1795072112.049708, 3, 0.3228336),
];

const stage = (status: string, from: number, to: number | null, round = 0): Stage => ({
  status,
  from,
  to,
  round,
});

const sum = (held: Carried[]) => held.reduce((all, one) => all + one.dollars, 0);
const all = (rows: TokenRow[]) => rows.reduce((total, one) => total + one.cost.dollars, 0);
const figures = (held: Carried[]) => held.map((one) => carriedFigure(one));

/** Every row is on exactly one stretch, so the stretches add up to the rows. */
function addsUp(stages: Stage[], rows: TokenRow[], slug = SLUG): Carried[] {
  const held = carried(stages, rows, slug);
  expect(held).toHaveLength(stages.length);
  const placed = held.flatMap((one) => one.rows);
  expect(placed).toHaveLength(rows.length);
  expect(new Set(placed).size).toBe(rows.length);
  expect(sum(held)).toBeCloseTo(all(rows), 9);
  expect(held.reduce((n, one) => n + one.calls, 0)).toBe(
    rows.reduce((n, one) => n + one.tokens.calls, 0),
  );
  return held;
}

describe("which stretch carries what", () => {
  it("counts a stretch's round as the board counts a row's", () => {
    expect(roundsOf(STAGES)).toEqual([1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7]);
    expect(roundsOf([])).toEqual([]);
    // A review that waits to land after its approval is the same round's.
    expect(
      roundsOf([
        stage("in_progress", 0, 10),
        stage("review", 10, 20, 1),
        { ...stage("review", 20, null, 1), approved: true },
      ]),
    ).toEqual([1, 2, 2]);
  });

  it("puts the approved design's task's rows on its stretches, to its table's figures", () => {
    const held = addsUp(STAGES, ROWS);
    expect(figures(held)).toEqual([
      null, // ready
      "$30.75", // in progress 3 h 14 m: executor $30.43 and reserved $0.32
      "$3.78", // review 1
      "$7.03", // in progress 39 m
      "$0.72", // review 2: reviewer $0.64 and the executor's $0.08, no in-progress stretch after
      null, // done, reopened
      "$4.23", // review 3
      "$13.23", // in progress 1 h 9 m
      "$3.60", // review 4
      "$9.21", // in progress 1 h 4 m
      "$4.79", // review 5
      "$2.44", // in progress 37 m
      "$0.21", // review 6
      null, // done
    ]);
    expect(dollars(sum(held))).toBe("$80.02");
    expect(held[1]?.rows.map((one) => one.role)).toEqual(["executor", "reserved"]);
    expect(held[1]?.calls).toBe(352);
    expect(held[4]?.rows.map((one) => one.role)).toEqual(["reviewer", "executor"]);
  });

  it("puts a round's executor on the last of two in-progress stretches a block parted", () => {
    const stages = [
      stage("ready", 0, 100),
      stage("in_progress", 100, 200),
      stage("blocked", 200, 300),
      stage("in_progress", 300, 400),
      stage("review", 400, 500, 1),
    ];
    const rows = [
      row("executor", 1, 110, 390, 9, 5),
      row("reserved", 1, 90, 99, 1, 0.5),
      row("reviewer", 2, 410, 490, 2, 1),
    ];
    const held = addsUp(stages, rows);
    expect(figures(held)).toEqual([null, null, null, "$5.50", "$1.00"]);
    expect(held[1]).toMatchObject({ on: 3, with: [] });
    expect(held[3]).toMatchObject({ on: null, with: [1] });
    expect(held[2]).toMatchObject({ on: null, with: [] });
    const when = (at: number) => `t${at}`;
    const from = (index: number) => when((stages[index] as Stage).from);
    expect(carriedWords(held[1], SLUG, "2026-11-16", when, from)).toBe(
      "; its executor's calls are counted on the stretch from t300, the last of its round",
    );
    expect(carriedWords(held[3], SLUG, "2026-11-16", when, from)).toBe(
      ": executor $5.00, t110–t390, in 9 calls; reserved $0.50, t90–t99, in 1 call; " +
        "all of its round's executor's calls, those of the stretch from t100 among them; " +
        "estimated at API list prices read 2026-11-16, not money spent",
    );
    expect(carriedWords(held[2], SLUG, "2026-11-16", when, from)).toBe("");
  });

  it("puts a round's executor on its review where no in-progress stretch follows", () => {
    const stages = [
      stage("in_progress", 0, 100),
      stage("review", 100, 200, 1),
      stage("done", 200, null),
    ];
    const rows = [
      row("executor", 1, 1, 99, 5, 4),
      row("executor", 2, 120, 130, 1, 0.25),
      row("reviewer", 2, 110, 190, 3, 1),
    ];
    expect(figures(addsUp(stages, rows))).toEqual(["$4.00", "$1.25", null]);
  });

  it("puts a reviewer's rows on the review, not on its wait to land", () => {
    const stages = [
      stage("in_progress", 0, 100),
      stage("review", 100, 150, 1),
      { ...stage("review", 150, null, 1), approved: true },
    ];
    const rows = [row("reviewer", 2, 110, 140, 3, 1), row("executor", 2, 160, 170, 1, 0.5)];
    expect(figures(addsUp(stages, rows))).toEqual([null, "$1.50", null]);
  });

  it("puts a row the rule places nowhere on the stretch its first call fell in", () => {
    const stages = [
      stage("ready", 0, 100),
      stage("in_progress", 100, 200),
      stage("review", 200, 300, 1),
      stage("in_progress", 300, 400),
      stage("done", 400, null),
    ];
    const rows = [
      // A session that worked no task: round 0.
      row("executor", 0, 250, 260, 1, 0.01),
      // A subtask's row, whose round is its own task's.
      row("executor", 1, 310, 390, 4, 2, { slug: `${SLUG}/fix` }),
      // Another role.
      row("orchestrator", 1, 150, 380, 6, 3),
      // A round no stretch has, its first call after every stretch began and before any.
      row("reviewer", 9, 999, 1000, 1, 0.04),
      row("executor", 8, 5, 6, 1, 0.08),
      // A reviewer's of round 1, which no review opens.
      row("reviewer", 1, 120, 130, 1, 0.16),
    ];
    const held = addsUp(stages, rows);
    expect(held.map((one) => one.rows.map((r) => r.cost.dollars))).toEqual([
      [],
      [3, 0.08, 0.16],
      [0.01],
      [2, 0.04],
      [],
    ]);
    const when = (at: number) => `t${at}`;
    expect(carriedWords(held[3], SLUG, "2026-11-16", when, when)).toBe(
      `: executor on ${SLUG}/fix $2.00, t310–t390, in 4 calls; reviewer $0.04, t999–t1000, ` +
        "in 1 call; estimated at API list prices read 2026-11-16, not money spent",
    );
  });

  it("puts what a task never in progress took on its last stretch drawn", () => {
    const reserved = [row("reserved", 1, 50, 60, 2, 0.32)];
    expect(figures(addsUp([stage("ready", 0, null)], reserved))).toEqual(["$0.32"]);
    expect(
      figures(addsUp([stage("ready", 0, 100), stage("cancelled", 100, null)], reserved)),
    ).toEqual(["$0.32", null]);
    expect(figures(addsUp([stage("cancelled", 0, null)], reserved))).toEqual(["$0.32"]);
    // No stage, no stretch: nothing to put it on.
    expect(carried([], reserved, SLUG)).toEqual([]);
  });

  it("says a stretch with tokens of a model with no price as the total does", () => {
    const stages = [stage("in_progress", 0, 100), stage("review", 100, 200, 1)];
    const rows = [
      row("executor", 1, 1, 50, 2, 1.23, { cost: cost(1.23, ["gpt-x"]) }),
      row("reviewer", 2, 110, 110, 1, 0, { cost: cost(0, ["gpt-x"]) }),
    ];
    const held = addsUp(stages, rows);
    expect(figures(held)).toEqual(["$1.23+", "unpriced"]);
    const when = (at: number) => `t${at}`;
    expect(carriedWords(held[1], SLUG, "2026-11-16", when, when)).toBe(
      ": reviewer unpriced, at t110, in 1 call; estimated at API list prices read 2026-11-16, not money spent",
    );
  });

  it("says nothing of a stretch that carries nothing, or of none", () => {
    const held = carried([stage("ready", 0, null)], [], SLUG);
    expect(carriedFigure(held[0])).toBeNull();
    expect(carriedFigure(undefined)).toBeNull();
    expect(carriedWords(held[0], SLUG, "", String, String)).toBe("");
    expect(carriedWords(undefined, SLUG, "", String, String)).toBe("");
  });
});

describe("the total's words", () => {
  const TOKENS = readTaskTokens(CASES.samples.taskTokens) as TaskTokens;
  const NONE = readTaskTokens(CASES.samples.emptyTaskTokens) as TaskTokens;

  it("reads the wire's sample, its rows with it, and a server's that sends none", () => {
    expect(TOKENS.rows).toHaveLength(2);
    expect(TOKENS.rows[0]).toMatchObject({ role: "executor", round: 1 });
    const { rows: _, ...older } = CASES.samples.taskTokens as Record<string, unknown>;
    expect(readTaskTokens(older)?.rows).toEqual([]);
    expect(NONE.rows).toEqual([]);
  });

  it("refuses what is no task's tokens", () => {
    const whole = CASES.samples.taskTokens as Record<string, unknown>;
    const aRow = (whole.rows as Record<string, unknown>[])[0];
    for (const broken of [
      null,
      {},
      { ...whole, total: {} },
      { ...whole, cost: { dollars: "3" } },
      { ...whole, by_role: "none" },
      { ...whole, by_role: [{ role: 1 }] },
      { ...whole, rows: "none" },
      { ...whole, rows: [{ ...aRow, round: 1.5 }] },
      { ...whole, rows: [{ ...aRow, first: "then" }] },
      { ...whole, rows: [{ ...aRow, last: null }] },
      { ...whole, rows: [{ ...aRow, role: 7 }] },
      { ...whole, rows: [{ ...aRow, slug: undefined }] },
      { ...whole, rows: [{ ...aRow, tokens: {} }] },
      { ...whole, rows: [{ ...aRow, cost: {} }] },
    ]) {
      expect(readTaskTokens(broken)).toBeNull();
    }
  });

  it("knows a task that took no tokens", () => {
    expect(tookTokens(TOKENS)).toBe(true);
    expect(tookTokens(NONE)).toBe(false);
    expect(tookTokens({ ...NONE, total: tokens(1) })).toBe(true);
  });

  it("says the total, that it is an estimate, its split, its tokens and what had no price", () => {
    expect(costFigure(TOKENS.cost)).toBe("$3.00+");
    expect(totalWords(TOKENS)).toBe(
      "$3.00, estimated at API list prices read 2026-11-16, not money spent: " +
        "executor $2.00+ · reviewer $1.00 " +
        "(input $0.02 · cache write $0.60 · cache read $0.90 · output $1.48); " +
        "187k tokens in 8 calls; no price for gpt-x 93.8k tokens: in no figure",
    );
  });

  it("says the design's total as its card words it", () => {
    const total: TaskTokens = {
      total: {
        input: 1838,
        cache_write_5m: 0,
        cache_write_1h: 3018348,
        cache_read: 203335683,
        output: 759705,
        calls: 884,
      },
      cost: {
        dollars: 80.0153726,
        input: 0.007352,
        cache_write_5m: 0,
        cache_write_1h: 24.146784,
        cache_read: 40.6671366,
        output: 15.1941,
        unpriced: [],
        prices: "2026-11-16",
      },
      by_role: [
        { role: "executor", tokens: tokens(757), cost: cost(62.4215962) },
        { role: "reviewer", tokens: tokens(124), cost: cost(17.2709438) },
        { role: "reserved", tokens: tokens(3), cost: cost(0.3228336) },
      ],
      by_round: [],
      rows: ROWS,
    };
    expect(totalWords(total)).toBe(
      "$80.02, estimated at API list prices read 2026-11-16, not money spent: " +
        "executor $62.42 · reviewer $17.27 · reserved $0.32 " +
        "(input $0.01 · cache write $24.15 · cache read $40.67 · output $15.19); " +
        "207M tokens in 884 calls",
    );
  });

  it("says a total none of whose models has a price, and one with no role", () => {
    const unpriced: TaskTokens = {
      total: tokens(4, 4000),
      cost: cost(0, ["gpt-x"]),
      by_role: [],
      by_round: [],
      rows: [],
    };
    expect(costFigure(unpriced.cost)).toBe("unpriced");
    expect(totalWords(unpriced)).toBe(
      "Unpriced: the API list prices read 2026-11-16 have none for these models; " +
        "4k tokens in 4 calls; no price for gpt-x 1k tokens: in no figure",
    );
  });
});
