// The estimated cost each card of the task board shows (docs/TASKS.md, "What the tokens cost"): one
// read apart from the board's listings, which carry none of it, of the cards drawn. A card names
// its task once it has painted, as it comes near the screen (its own effect), and the cards named
// in one draw are asked for together (`TASK_COSTS_PATH`, at most TASK_COSTS_MOST a read), so the
// board's first paint waits on none of it and a column scrolled into view is one read. A task is
// asked again only when its record changes (a move, a post), never at each heartbeat that adds
// tokens. A read that fails leaves its cards without a figure, asked again once they change.

import {
  createContext,
  createElement,
  type ReactNode,
  useContext,
  useEffect,
  useMemo,
  useSyncExternalStore,
} from "react";
import { useMount } from "./mount";
import { scoped } from "./project";
import { getJson } from "./request";
import { field } from "./shared";
import { dollars } from "./spend";
import { TASK_COSTS_MOST, TASK_COSTS_PATH, type TaskCost, type TaskCosts } from "./wire";

/** A card's figure: what it says, and what its title says of it. */
export interface CardCost {
  said: string;
  title: string;
}

/** The costs as the route writes them, or none. */
export function readCosts(value: unknown): TaskCosts | null {
  const costs = field(value, "costs");
  return typeof field(value, "prices") === "string" &&
    Array.isArray(costs) &&
    costs.every(
      (cost) =>
        typeof field(cost, "slug") === "string" &&
        typeof field(cost, "dollars") === "number" &&
        ["boolean", "undefined"].includes(typeof field(cost, "unpriced")),
    )
    ? (value as TaskCosts)
    : null;
}

/** A card's figure for `cost`, estimated at the prices read on `prices`: `$1.23`, a `+` after it
 * where some tokens had no price, and `unpriced` where none had one (`tasks show`'s "no API list
 * price"), never a `$0.00+` that reads as a cost. */
export function cardCost(cost: TaskCost, prices: string): CardCost {
  const unpriced = cost.unpriced === true;
  return {
    said:
      unpriced && cost.dollars === 0
        ? "unpriced"
        : `${dollars(cost.dollars)}${unpriced ? "+" : ""}`,
    title:
      `Its tokens and its subtasks', estimated at API list prices read ${prices}, not money spent` +
      (unpriced ? "; + some of a model with no price, in no figure" : ""),
  };
}

/** The board's cards' costs, kept for the page's life and read under its mount's `signal`. */
export class CardCosts {
  private readonly known = new Map<string, CardCost>();
  /** Each slug asked for, by its record's `updated` when it was. */
  private readonly asked = new Map<string, number>();
  private waiting: string[] = [];
  private readonly heard = new Set<() => void>();

  constructor(
    private readonly scope: string,
    private readonly signal: AbortSignal,
  ) {}

  /** `slug`'s card was drawn with its record as of `updated`: its cost is asked for unless it
   * was at that, with every other card named before this turn ends. */
  want(slug: string, updated: number): void {
    if (this.asked.get(slug) === updated || this.signal.aborted) return;
    this.asked.set(slug, updated);
    if (this.waiting.length === 0) queueMicrotask(() => void this.read());
    this.waiting.push(slug);
  }

  /** `slug`'s figure, once read; none before, or for a task that took no tokens. */
  get(slug: string): CardCost | undefined {
    return this.known.get(slug);
  }

  /** Hear each read that answered; what it returns stops it. */
  readonly subscribe = (heard: () => void): (() => void) => {
    this.heard.add(heard);
    return () => {
      this.heard.delete(heard);
    };
  };

  private async read(): Promise<void> {
    const named = this.waiting;
    this.waiting = [];
    for (let at = 0; at < named.length; at += TASK_COSTS_MOST) {
      const slugs = named.slice(at, at + TASK_COSTS_MOST);
      const url = `${TASK_COSTS_PATH}?slugs=${encodeURIComponent(JSON.stringify(slugs))}`;
      try {
        const read = readCosts(await getJson(scoped(this.scope, url), this.signal));
        if (this.signal.aborted) return;
        if (read === null) throw new Error("The cards' costs are not readable");
        for (const cost of read.costs) this.known.set(cost.slug, cardCost(cost, read.prices));
        for (const heard of this.heard) heard();
      } catch {
        // Asked again once its card changes: a figure is no part of what the board must show.
        for (const slug of slugs) this.asked.delete(slug);
      }
    }
  }
}

/** The board page's costs; none outside a board, where a card draws no figure. */
export const CardCostsContext = createContext<CardCosts | null>(null);

/** The costs of the cards drawn under it, of the project `scope`, read under the page's mount. */
export function CardCostsOf({ scope, children }: { scope: string; children: ReactNode }) {
  const mount = useMount();
  const costs = useMemo(() => new CardCosts(scope, mount), [scope, mount]);
  return createElement(CardCostsContext.Provider, { value: costs }, children);
}

const NONE = () => () => {};

/** The figure of the card of `slug`, its record as of `updated`, which it asks for once drawn. */
export function useCardCost(slug: string, updated: number): CardCost | undefined {
  const costs = useContext(CardCostsContext);
  useEffect(() => costs?.want(slug, updated), [costs, slug, updated]);
  const figure = () => costs?.get(slug);
  return useSyncExternalStore(costs?.subscribe ?? NONE, figure, figure);
}
