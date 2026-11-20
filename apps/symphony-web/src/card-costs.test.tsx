import { useContext } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WaitCard } from "./boardwait";
import { CardCosts, CardCostsContext, CardCostsOf, cardCost, readCosts } from "./card-costs";
import { MountContext } from "./mount";
import { aTask, CASES, reply, server, turn, until } from "./testing";
import { TASK_COSTS_MOST } from "./wire";

afterEach(() => vi.unstubAllGlobals());

/** The slugs an ask names, from its address. */
const named = (url: string) =>
  JSON.parse(decodeURIComponent(url.split("slugs=")[1] ?? "[]")) as string[];

const answer = (costs: { slug: string; dollars: number; unpriced?: boolean }[]) =>
  reply(200, { prices: "2026-11-16", costs });

/** The costs the last `Seen` drawn was given. */
let given: CardCosts | null = null;

/** A component that keeps the costs it is given. */
function Seen() {
  given = useContext(CardCostsContext);
  return null;
}

describe("a card's cost", () => {
  it("is read as the route writes it, and none that serde would refuse", () => {
    expect(readCosts(CASES.samples.taskCosts)?.costs.map((cost) => cost.slug)).toEqual([
      "w/a",
      "w/b",
    ]);
    for (const wrong of [
      { costs: [] },
      { prices: "d", costs: {} },
      { prices: "d", costs: [{ dollars: 1 }] },
      { prices: "d", costs: [{ slug: "a/b", dollars: "1" }] },
      { prices: "d", costs: [{ slug: "a/b", dollars: 1, unpriced: "yes" }] },
      null,
    ]) {
      expect(readCosts(wrong)).toBeNull();
    }
  });

  it("says its dollars, a + where some tokens had no price, and is an estimate in its title", () => {
    expect(cardCost({ slug: "a/b", dollars: 12.345, unpriced: false }, "2026-11-16")).toEqual({
      said: "$12.35",
      title:
        "Its tokens and its subtasks', estimated at API list prices read 2026-11-16, not money spent",
    });
    const some = cardCost({ slug: "a/b", dollars: 0.5, unpriced: true }, "2026-11-16");
    expect(some.said).toBe("$0.50+");
    expect(some.title).toContain("+ some of a model with no price, in no figure");
    const none = cardCost({ slug: "a/b", dollars: 0, unpriced: true }, "2026-11-16");
    expect(none.said).toBe("unpriced");
    expect(cardCost({ slug: "a/b", dollars: 0, unpriced: false }, "2026-11-16").said).toBe("$0.00");
  });
});

describe("the board's cards' costs", () => {
  it("ask for every card named in one turn in one read, once each until its record changes", async () => {
    const asked = server();
    const mount = new AbortController();
    const costs = new CardCosts("", mount.signal);
    const heard = vi.fn();
    costs.subscribe(heard);
    costs.want("w/a", 1);
    costs.want("w/b", 1);
    costs.want("w/a", 1);
    await turn();
    expect(asked.map((one) => named(one.url))).toEqual([["w/a", "w/b"]]);
    expect(asked[0]?.url).toMatch(/^\/api\/task-costs\?slugs=/);
    asked[0]?.answer(answer([{ slug: "w/a", dollars: 2.5 }]));
    await until(() => expect(heard).toHaveBeenCalledTimes(1));
    expect(costs.get("w/a")?.said).toBe("$2.50");
    expect(costs.get("w/b")).toBeUndefined();
    // Drawn again with its record unchanged: asked for no more. Changed: asked for again, alone.
    costs.want("w/a", 1);
    costs.want("w/b", 2);
    await turn();
    expect(asked.map((one) => named(one.url))).toEqual([["w/a", "w/b"], ["w/b"]]);
    mount.abort();
  });

  it("ask at most TASK_COSTS_MOST slugs a read", async () => {
    const asked = server();
    const mount = new AbortController();
    const costs = new CardCosts("branch~one", mount.signal);
    for (let n = 0; n <= TASK_COSTS_MOST; n++) costs.want(`w/t${n}`, 1);
    await turn();
    expect(asked[0]?.url).toContain("/@branch~one/api/task-costs");
    expect(named(asked[0]?.url ?? "")).toHaveLength(TASK_COSTS_MOST);
    asked[0]?.answer(answer([]));
    await until(() => expect(asked).toHaveLength(2));
    expect(named(asked[1]?.url ?? "")).toEqual([`w/t${TASK_COSTS_MOST}`]);
    mount.abort();
  });

  it("leave a card without its figure when a read fails, and ask again once it is drawn", async () => {
    for (const failure of [reply(503, { error: "Board unavailable" }), reply(200, {})]) {
      const asked = server();
      const mount = new AbortController();
      const costs = new CardCosts("", mount.signal);
      costs.want("w/a", 1);
      await turn();
      asked[0]?.answer(failure);
      await until(() => {
        costs.want("w/a", 1);
        expect(asked).toHaveLength(2);
      });
      expect(costs.get("w/a")).toBeUndefined();
      mount.abort();
    }
  });

  it("are given to the cards drawn under the board's root, and heard no more once let go", async () => {
    const asked = server();
    const mount = new AbortController();
    given = null;
    renderToStaticMarkup(
      <MountContext.Provider value={mount.signal}>
        <CardCostsOf scope="branch~one">
          <Seen />
        </CardCostsOf>
      </MountContext.Provider>,
    );
    const costs = given as CardCosts | null;
    if (costs === null) throw new Error("no costs were given");
    const heard = vi.fn();
    costs.subscribe(heard)();
    costs.want("w/a", 1);
    await turn();
    expect(asked[0]?.url).toContain("/@branch~one/api/task-costs");
    asked[0]?.answer(answer([{ slug: "w/a", dollars: 1 }]));
    await until(() => expect(costs.get("w/a")?.said).toBe("$1.00"));
    expect(heard).not.toHaveBeenCalled();
    mount.abort();
  });

  it("ask nothing, and say nothing, once the page is gone", async () => {
    const asked = server();
    const mount = new AbortController();
    const costs = new CardCosts("", mount.signal);
    const heard = vi.fn();
    costs.subscribe(heard);
    costs.want("w/a", 1);
    await turn();
    mount.abort();
    asked[0]?.answer(answer([{ slug: "w/a", dollars: 1 }]));
    await turn();
    costs.want("w/b", 1);
    await turn();
    expect([asked.length, heard.mock.calls.length, costs.get("w/a")]).toEqual([1, 0, undefined]);
  });

  it("draw a card's figure once read, and none outside a board or before", async () => {
    const task = aTask("w/a", { updated: 5 });
    expect(renderToStaticMarkup(<WaitCard task={task} now={10} />)).not.toContain("card-cost");
    const asked = server();
    const mount = new AbortController();
    const costs = new CardCosts("", mount.signal);
    const drawn = () =>
      renderToStaticMarkup(
        <CardCostsContext.Provider value={costs}>
          <WaitCard task={task} now={10} />
        </CardCostsContext.Provider>,
      );
    expect(drawn()).not.toContain("card-cost");
    costs.want("w/a", 5);
    await turn();
    asked[0]?.answer(answer([{ slug: "w/a", dollars: 1.5, unpriced: true }]));
    await until(() => expect(costs.get("w/a")).toBeDefined());
    expect(drawn()).toContain(
      '<span class="card-cost" title="Its tokens and its subtasks&#x27;, estimated at API list ' +
        "prices read 2026-11-16, not money spent; + some of a model with no price, in no " +
        'figure">$1.50+</span><span class="prio">',
    );
    mount.abort();
  });
});
