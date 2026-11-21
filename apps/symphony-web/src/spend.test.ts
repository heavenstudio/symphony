import { expect, it } from "vitest";
import {
  amount,
  calls,
  costSaid,
  dollars,
  estimate,
  isCost,
  isTokens,
  total,
  unpricedSaid,
} from "./spend";
import type { AgentCost, AgentTokens } from "./wire";

const tokens = (n: number): AgentTokens => ({
  input: n,
  cache_write_5m: n,
  cache_write_1h: n,
  cache_read: n * 1000,
  output: n,
  calls: n,
});

const cost = (amount: number, unpriced: AgentCost["unpriced"] = []): AgentCost => ({
  dollars: amount,
  input: 0,
  cache_write_5m: 0,
  cache_write_1h: 0,
  cache_read: 0,
  output: amount,
  unpriced,
  prices: "2026-11-16",
});

it("says an amount in three figures at most, rounded down, as the board does", () => {
  // The cases of crates/tasks/src/usage.rs.
  for (const [n, said] of [
    [0, "0"],
    [999, "999"],
    [1_000, "1k"],
    [1_050, "1k"],
    [1_150, "1.1k"],
    [12_345, "12.3k"],
    [99_999, "99.9k"],
    [456_789, "456k"],
    [999_999, "999k"],
    [1_000_000, "1M"],
    [7_890_000, "7.8M"],
    [2_500_000_000, "2.5G"],
  ] as const)
    expect(amount(n), `${n}`).toBe(said);
  expect(total(tokens(2))).toBe(2008);
  expect([calls(1), calls(0), calls(6)]).toEqual(["1 call", "0 calls", "6 calls"]);
});

it("says dollars to the cent, half a cent up", () => {
  for (const [amount, said] of [
    [0, "$0.00"],
    [0.004999, "$0.00"],
    [0.005, "$0.01"],
    [0.075, "$0.08"],
    [7.27, "$7.27"],
    [9.706, "$9.71"],
    [1234.5, "$1234.50"],
    [1.005, "$1.01"],
    [0.995, "$1.00"],
  ] as const)
    expect(dollars(amount), `${amount}`).toBe(said);
});

it("names what had no price beside the cost, never in it", () => {
  const sol = { model: "gpt-x", speed: "standard", tokens: tokens(1) };
  const fast = { model: "claude-opus-5-5", speed: "fast", tokens: tokens(1200) };
  expect(costSaid(cost(1.23))).toBe("$1.23");
  expect(costSaid(cost(1.23, [sol]))).toBe("$1.23 + unpriced");
  expect(costSaid(cost(0, [sol]))).toBe("unpriced");
  expect(costSaid(cost(0))).toBe("$0.00");
  expect(unpricedSaid(cost(0, [sol, fast]))).toBe(
    "gpt-x 1k tokens · claude-opus-5-5 fast 1.2M tokens",
  );
  expect(unpricedSaid(cost(0, [sol, sol]), ", ")).toBe("gpt-x 1k tokens, gpt-x 1k tokens");
  expect(estimate("2026-11-16")).toBe(
    "estimated at API list prices read 2026-11-16, not money spent",
  );
});

it("reads tokens and a cost only as the wire writes them", () => {
  expect(isTokens(tokens(3))).toBe(true);
  expect(isTokens({ ...tokens(3), extra: 1 })).toBe(true);
  for (const bad of [null, {}, { ...tokens(1), calls: -1 }, { ...tokens(1), input: "1" }])
    expect(isTokens(bad)).toBe(false);
  expect(isTokens({ ...tokens(1), output: 1.5 })).toBe(false);
  const sol = { model: "gpt-x", speed: "standard", tokens: tokens(1) };
  expect(isCost(cost(1, [sol]))).toBe(true);
  const { unpriced: _, ...bare } = cost(1);
  expect(isCost(bare)).toBe(true);
  for (const bad of [
    null,
    { ...cost(1), prices: 5 },
    { ...cost(1), dollars: Number.NaN },
    { ...cost(1), unpriced: "none" },
    { ...cost(1), unpriced: [{ ...sol, tokens: {} }] },
    { ...cost(1), unpriced: [{ ...sol, model: 1 }] },
    { ...cost(1), unpriced: [{ ...sol, speed: null }] },
  ])
    expect(isCost(bad)).toBe(false);
});
