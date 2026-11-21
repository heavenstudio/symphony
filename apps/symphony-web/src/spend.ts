// Tokens and their estimated cost, said as the board's terminal says them (`tasks show`,
// crates/tasks/src/usage.rs): an amount in three figures at most, a dollar figure to the cent,
// and with every figure that it is an estimate at API list prices, never money spent, as a
// subscription is not billed by the token. The task page, the agents page and Reports share it.

import { field } from "./shared";
import type { AgentCost, AgentTokens } from "./wire";

const UNITS: readonly (readonly [number, string])[] = [
  [1_000_000_000, "G"],
  [1_000_000, "M"],
  [1_000, "k"],
];

/** `n` tokens in three figures at most, rounded down: `950`, `12.3k`, `456k`, `7.8M` (Rust's
 * `usage::amount`). */
export function amount(n: number): string {
  for (const [size, unit] of UNITS) {
    if (n >= size) {
      // Tenths, rounded down, so 999.95k never reads as 1000.0k; none from 100 on.
      const tenths = Math.floor(n / (size / 10));
      return tenths >= 1000 || tenths % 10 === 0
        ? `${Math.floor(tenths / 10)}${unit}`
        : `${Math.floor(tenths / 10)}.${tenths % 10}${unit}`;
    }
  }
  return `${n}`;
}

/** Every token of the four kinds. */
export function total(tokens: AgentTokens): number {
  return (
    tokens.input + tokens.cache_write_5m + tokens.cache_write_1h + tokens.cache_read + tokens.output
  );
}

/** `n` model calls. */
export function calls(n: number): string {
  return `${n} call${n === 1 ? "" : "s"}`;
}

/** Dollars to the cent, half a cent up: `$0.00`, `$7.27`, `$1234.50` (Rust's `price::dollars`).
 * The wire's dollars are a float of an exact sum, so a hair under half a cent is half a cent. */
export function dollars(amount: number): string {
  const cents = Math.floor(amount * 100 + 0.5 + 1e-7);
  return `$${Math.floor(cents / 100)}.${`${cents % 100}`.padStart(2, "0")}`;
}

/** The figure a line says: the dollars, `+ unpriced` after them where some tokens had no price,
 * `unpriced` alone where none had one (Rust's `Cost::said`). */
export function costSaid(cost: AgentCost): string {
  const unpriced = cost.unpriced.length > 0;
  if (unpriced && cost.dollars === 0) return "unpriced";
  return unpriced ? `${dollars(cost.dollars)} + unpriced` : dollars(cost.dollars);
}

/** The models no price was found for, each with its tokens: `gpt-x 1.2M tokens`, a speed other
 * than standard after the model (Rust's `Cost::unpriced_said`). */
export function unpricedSaid(cost: AgentCost, between = " · "): string {
  return cost.unpriced
    .map(
      (one) =>
        `${one.model}${one.speed === "standard" ? "" : ` ${one.speed}`} ${amount(total(one.tokens))} tokens`,
    )
    .join(between);
}

/** What every dollar figure is: said beside it, with the day the prices were read. */
export function estimate(prices: string): string {
  return `estimated at API list prices read ${prices}, not money spent`;
}

const count = (value: unknown) =>
  typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
const money = (value: unknown) => typeof value === "number" && Number.isFinite(value);

/** Whether `value` is tokens as the wire writes them. */
export function isTokens(value: unknown): value is AgentTokens {
  return ["input", "cache_write_5m", "cache_write_1h", "cache_read", "output", "calls"].every(
    (kind) => count(field(value, kind)),
  );
}

/** Whether `value` is a cost as the wire writes it; a server that names no unpriced tokens names
 * none. */
export function isCost(value: unknown): value is AgentCost {
  const unpriced = field(value, "unpriced") ?? [];
  return (
    ["dollars", "input", "cache_write_5m", "cache_write_1h", "cache_read", "output"].every((kind) =>
      money(field(value, kind)),
    ) &&
    typeof field(value, "prices") === "string" &&
    Array.isArray(unpriced) &&
    unpriced.every(
      (one) =>
        typeof field(one, "model") === "string" &&
        typeof field(one, "speed") === "string" &&
        isTokens(field(one, "tokens")),
    )
  );
}
