import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Parts, PartsContext, type Shown } from "./parts";
import { type TaskEvent, tokensPart } from "./task-live";
import { TokensBody } from "./task-page";
import { putStyle, readTaskTokens, roundName, TaskTokensPanel, TokenFigures } from "./task-tokens";
import { CASES, reply, until } from "./testing";
import type { TaskTokens } from "./wire";

/** As the route writes them: an executor in round 1 with tokens of a model with no price, a
 * reviewer in round 2. */
const TOKENS = readTaskTokens(CASES.samples.taskTokens) as TaskTokens;
const NONE = readTaskTokens(CASES.samples.emptyTaskTokens) as TaskTokens;

afterEach(() => vi.unstubAllGlobals());

/** The text a drawing says, its tags taken out. */
const text = (html: string) => html.replace(/<[^>]+>/g, " ").replace(/\s+/g, " ");

describe("a task's tokens", () => {
  it("put their style in the page once, before the project's theme", () => {
    type Made = { tag: string; id: string; textContent: unknown };
    const theme: Made = { tag: "style", id: "", textContent: "theme" };
    const head: Made[] = [theme];
    vi.stubGlobal("document", {
      getElementById: (id: string) => head.find((element) => element.id === id) ?? null,
      createElement: (tag: string): Made => ({ tag, id: "", textContent: null }),
      head: {
        querySelector: (selector: string) => (selector === "[data-symphony-theme]" ? theme : null),
        insertBefore: (element: Made, before: Made | null) =>
          head.splice(before === null ? head.length : head.indexOf(before), 0, element),
      },
    });
    putStyle();
    putStyle();
    expect(head).toHaveLength(2);
    expect(head[0]).toMatchObject({ tag: "style", id: "task-tokens-style" });
    expect(head[1]).toBe(theme);
    // Its text is the sheet's, which Vitest empties.
    expect(typeof head[0]?.textContent).toBe("string");
  });

  it("are read as the route writes them, and none that serde would refuse", () => {
    expect(TOKENS.by_role.map((role) => role.role)).toEqual(["executor", "reviewer"]);
    expect(NONE.by_role).toEqual([]);
    const whole = CASES.samples.taskTokens as Record<string, unknown>;
    for (const wrong of [
      { ...whole, total: { input: 1 } },
      { ...whole, cost: { ...(whole.cost as object), prices: null } },
      { ...whole, by_role: null },
      { ...whole, by_role: [{ role: 1, tokens: TOKENS.total, cost: TOKENS.cost }] },
      { ...whole, by_role: [{ role: "executor", tokens: TOKENS.total }] },
      { ...whole, by_round: [{ round: 1.5, tokens: TOKENS.total, cost: TOKENS.cost }] },
      { ...whole, by_round: {} },
      null,
    ]) {
      expect(readTaskTokens(wrong)).toBeNull();
    }
  });

  it("say the figures all told, by role and by round, with the prices' day and what had none", () => {
    const said = text(renderToStaticMarkup(<TokenFigures tokens={TOKENS} />));
    expect(said).toContain(
      "187k tokens in 8 calls (input 20 · cache write 4.6k · cache read 180k · output 3k)",
    );
    expect(said).toContain(
      "$3.00 , estimated at API list prices read 2026-11-16, not money spent (input $0.02 · " +
        "cache write $0.60 · cache read $0.90 · output $1.48)",
    );
    expect(said).toContain("No price for gpt-x 93.8k tokens: in no figure.");
    expect(said).toContain("executor 93.8k 4 $2.00 + unpriced");
    expect(said).toContain("reviewer 93.8k 4 $1.00");
    expect(said).toContain("round 1 93.8k 4 $2.00 + unpriced");
    expect(said).toContain("round 2 93.8k 4 $1.00");
  });

  it("say unpriced where no token had a price, and draw no table where none was recorded", () => {
    const unpriced = { ...TOKENS, cost: { ...TOKENS.cost, dollars: 0 } };
    expect(text(renderToStaticMarkup(<TokenFigures tokens={unpriced} />))).toContain(
      "Unpriced : the API list prices read 2026-11-16 have none for these models.",
    );
    const none = renderToStaticMarkup(<TokenFigures tokens={NONE} />);
    expect(none).toBe("<p>No tokens recorded for this task.</p>");
    // A call that read nothing still drew a call.
    const call = { ...NONE, total: { ...NONE.total, calls: 1 } };
    expect(renderToStaticMarkup(<TokenFigures tokens={call} />)).toContain("<table>");
  });

  it("name a session that worked no task's round apart", () => {
    expect([roundName(0), roundName(1), roundName(12)]).toEqual(["no task", "round 1", "round 12"]);
  });
});

/** A part not answered yet. */
const PENDING: Shown<unknown> = {
  value: undefined,
  stale: false,
  error: null,
  retrying: false,
  missing: false,
  answered: false,
};

/** The page's store with the tokens' part of `tools/x` answered as `status` and `value` say. */
async function answered(status: number, value: unknown) {
  vi.stubGlobal("document", { getElementById: () => null });
  vi.stubGlobal("fetch", async () => reply(status, value));
  const mount = new AbortController();
  const parts = new Parts<TaskEvent>(mount.signal);
  const part = tokensPart("", "tools/x");
  parts.open(part);
  await until(() =>
    expect(parts.shown(part).answered || parts.shown(part).error !== null).toBe(true),
  );
  return { parts, part, mount };
}

describe("the task page's tokens panel", () => {
  it("draws its pending, answered, failed, unreadable and missing states", async () => {
    const nothing = () => {};
    expect(renderToStaticMarkup(<TaskTokensPanel answer={PENDING} retry={nothing} />)).toContain(
      "Reading the tokens…",
    );
    for (const [status, value, words, retry] of [
      [200, CASES.samples.taskTokens, "187k tokens", false],
      [200, CASES.samples.emptyTaskTokens, "No tokens recorded for this task.", false],
      [200, {}, "The task&#x27;s tokens are not readable. Retry", true],
      [503, { error: "Board unavailable" }, "Board unavailable", true],
      [404, { error: "gone" }, "No tokens recorded for this task.", false],
    ] as const) {
      const { parts, part, mount } = await answered(status, value);
      const html = renderToStaticMarkup(
        <TaskTokensPanel answer={parts.shown(part)} retry={nothing} />,
      );
      mount.abort();
      expect([status, text(html)]).toEqual([status, expect.stringContaining(words)]);
      expect([status, html.includes("Retry")]).toEqual([status, retry]);
    }
  });

  it("says it reads until its chunk comes, its retry where the chunk failed, then the chunk", async () => {
    const { parts, mount } = await answered(200, CASES.samples.taskTokens);
    const body = (Panel: typeof TaskTokensPanel | null, failed: boolean) =>
      text(
        renderToStaticMarkup(
          <PartsContext.Provider value={parts}>
            <TokensBody scope="" slug="tools/x" Panel={Panel} failed={failed} onRetry={() => {}} />
          </PartsContext.Provider>,
        ),
      );
    expect(body(null, false)).toContain("Reading the tokens…");
    expect(body(null, true)).toContain("The tokens could not be loaded. Retry");
    expect(body(TaskTokensPanel, false)).toContain("187k tokens in 8 calls");
    mount.abort();
  });

  it("is read again when the task itself changes, never for another task or a post", () => {
    const part = tokensPart("branch~one", "tools/x");
    expect(part.url).toContain("/@branch~one/api/task-tokens?slug=tools%2Fx");
    const hear = (changes: TaskEvent["changes"]) => part.hear?.(TOKENS, { changes, page: null });
    expect(hear([])).toBeNull();
    expect(hear([{ region: "task", change: { key: "tools/x", removed: true } }])).toEqual({
      value: TOKENS,
      again: true,
    });
    expect(hear([{ region: "task", change: { key: "tools/y", removed: true } }])).toBeNull();
    expect(hear([{ region: "branches", moved: ["tools/x"] }])).toBeNull();
  });
});
