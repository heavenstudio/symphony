import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  AGENT_PAGE_PATH,
  type AgentPageRead,
  type AgentsHost,
  agentLine,
  agentSpent,
  agentsPageTakes,
  connectedAgents,
  connection,
  Followed,
  finished,
  followedTasks,
  followingLabel,
  happening,
  held,
  LiveAgents,
  lostAgents,
  meta,
  occupancy,
  put,
  readAgent,
  readAgentChange,
  readAgentPage,
  readAgents,
  readChanges,
  readProbes,
  trimmed,
  workingOn,
} from "./agents-live";
import {
  aMachine,
  anAgent,
  anOperation,
  fakeAnswer as answer,
  aRole,
  aTask,
  aWork,
  CASES,
  feedEvent as event,
} from "./testing";
import type { Agent, MachineObservation, Task } from "./wire";
import { freshnessWord, NO_WORK, phaseLabel, scopedPhases } from "./work";

const SAMPLES = CASES.samples;

// React's `flushSync` stood in for, as no DOM runs here: whether a draw comes within it.
const react = vi.hoisted(() => ({ flushing: false }));

vi.mock("react-dom", async (original) => ({
  ...(await original<typeof import("react-dom")>()),
  flushSync: <R>(draw: () => R): R => {
    react.flushing = true;
    try {
      return draw();
    } finally {
      react.flushing = false;
    }
  },
}));

/** The agents page as the server writes it, with every field. */
const PAGE = readAgentPage(SAMPLES.agentPage) as AgentPageRead;

describe("the agents page's reads", () => {
  it("read the page as the server writes it, with every field and with none it can leave out", () => {
    expect(PAGE.view.agents.map((agent) => agent.handle)).toEqual(["amber", "birch"]);
    expect(PAGE.view.probes).toEqual({ panes: "ok", slots: "unavailable" });
    expect(PAGE.view.work.machines.map((machine) => machine.machine_id)).toEqual(["alpha"]);
    expect(PAGE).toMatchObject({ total: 2, next: "birch", local: true });
    expect(PAGE.tasks.map((task) => task.slug)).toEqual(["w/a"]);
    expect(PAGE.view.agents[0]).toEqual(readAgent(field(SAMPLES.agentChange, "value")));
    const empty = readAgentPage(SAMPLES.emptyAgentPage);
    expect(empty).toMatchObject({ total: 0, next: null, local: false, tasks: [] });
    expect(empty?.view.agents).toEqual([]);
  });

  it("read what a server leaves out as serde's defaults", () => {
    const bare = SAMPLES.bareAgent as Record<string, unknown>;
    const {
      following,
      callback,
      chat,
      slots,
      session_url,
      in_process,
      go,
      parent,
      tokens,
      cost,
      ...older
    } = bare;
    expect(readAgent(older)).toEqual(readAgent(bare));
    expect(readAgent(older)).toMatchObject({ tokens: null, cost: null });
    expect(readAgent(bare)).toMatchObject({ handle: "birch", chat: false, go: null, parent: null });
    const { machine_work, ...view } = field(SAMPLES.emptyAgentPage, "view") as Record<
      string,
      unknown
    >;
    expect(readAgents(view)?.work).toBe(NO_WORK);
    const { local, ...page } = SAMPLES.emptyAgentPage as Record<string, unknown>;
    expect(readAgentPage(page)?.local).toBe(false);
  });

  it("refuse an agent serde would refuse, whichever field is wrong", () => {
    const whole = field(SAMPLES.agentChange, "value") as Record<string, unknown>;
    for (const [key, wrong] of [
      ["handle", null],
      ["health", 1],
      ["worktree", undefined],
      ["parent", 2],
      ["callback", false],
      ["tasks", null],
      ["tasks", [{ slug: "w/a", role: "executor" }]],
      ["following", null],
      ["following", "2"],
      // A `usize`: none below zero, none with a fraction, none past what a double holds whole.
      ["following", -1],
      ["following", 2.5],
      ["following", 2 ** 53],
      ["chat", "yes"],
      ["in_process", 1],
      ["last_tool_call", "now"],
      ["state_detail", { recipe: "test-rs" }],
      ["state_detail", { recipe: "test-rs", phase: "running", since: "1" }],
      ["last_message", { slug: "w/a", ts: "2", text: "Built." }],
      ["slots", { open: 3 }],
      ["slots", { open: -1, cap: 8 }],
      ["slots", { open: 3, cap: 8.5 }],
      ["go", { url: "https://example.com" }],
      ["go", { handle: "birch", pane: 2 }],
      ["tokens", { input: 1 }],
      ["tokens", { ...(whole.tokens as object), calls: -1 }],
      ["tokens", { ...(whole.tokens as object), output: 1.5 }],
      ["cost", { ...(whole.cost as object), dollars: "1" }],
      ["cost", { ...(whole.cost as object), prices: 1 }],
      ["cost", { ...(whole.cost as object), unpriced: [{ model: "x", speed: "standard" }] }],
      ["cost", { ...(whole.cost as object), unpriced: {} }],
    ] as [string, unknown][]) {
      expect([key, wrong, readAgent({ ...whole, [key]: wrong })]).toEqual([key, wrong, null]);
    }
    expect(
      readAgent({ ...whole, state_detail: { recipe: "r", phase: "p" } })?.state_detail,
    ).toEqual({ recipe: "r", phase: "p", since: null });
    expect(readAgent({ ...whole, go: { handle: "birch" } })?.go).toEqual({
      handle: "birch",
      url: null,
      pane: null,
    });
    // The counts' edges serde takes.
    const edges = { following: 0, slots: { open: 0, cap: Number.MAX_SAFE_INTEGER } };
    expect(readAgent({ ...whole, ...edges })).toMatchObject(edges);
  });

  it("say an agent's tokens with their estimated cost and the prices' day, and none it took none", () => {
    const whole = readAgent(field(SAMPLES.agentChange, "value")) as Agent;
    expect(agentSpent(whole)).toBe(
      "Tokens: 84k in 7 calls · $0.06 + unpriced, estimated at API list prices read " +
        "2026-11-16, not money spent; no price for gpt-x 6 tokens",
    );
    const priced = { ...whole, cost: { ...(whole.cost as Agent["cost"] & object), unpriced: [] } };
    expect(agentSpent(priced)).toBe(
      "Tokens: 84k in 7 calls · $0.06, estimated at API list prices read 2026-11-16, not money spent",
    );
    // A board that keeps no price table names the tokens alone.
    expect(agentSpent({ ...whole, cost: null })).toBe("Tokens: 84k in 7 calls");
    const none = { input: 0, cache_write_5m: 0, cache_write_1h: 0, cache_read: 0, output: 0 };
    expect(agentSpent({ ...whole, tokens: null, cost: null })).toBeNull();
    expect(agentSpent({ ...whole, tokens: { ...none, calls: 0 } })).toBeNull();
    // A call that read nothing is still a call.
    expect(agentSpent({ ...whole, tokens: { ...none, calls: 1 }, cost: null })).toBe(
      "Tokens: 0 in 1 call",
    );
  });

  it("refuse a page or a view without what the page draws", () => {
    const page = SAMPLES.agentPage as Record<string, unknown>;
    const view = page.view as Record<string, unknown>;
    for (const wrong of [
      { ...page, view: { ...view, generated: "3" } },
      { ...page, view: { ...view, probes: { slots: "ok" } } },
      { ...page, view: { ...view, agents: null } },
      { ...page, view: { ...view, agents: [{}] } },
      { ...page, view: { ...view, machine_work: { machines: null } } },
      { ...page, tasks: null },
      { ...page, tasks: [{ slug: "w/a" }] },
      { ...page, total: "2" },
      { ...page, total: -1 },
      { ...page, total: 1.5 },
      { ...page, next: 1 },
      { ...page, local: "yes" },
      null,
    ]) {
      expect(readAgentPage(wrong)).toBeNull();
    }
  });

  it("read the panes probe by either of its names, panes first", () => {
    expect(readProbes(SAMPLES.agentProbes)).toEqual({ panes: "ok", slots: "unavailable" });
    expect(readProbes({ herdr: "unavailable", slots: "ok" })).toEqual({
      panes: "unavailable",
      slots: "ok",
    });
    expect(readProbes({ panes: "ok", herdr: "unavailable", slots: "ok" })?.panes).toBe("ok");
    expect(readProbes({ panes: null, herdr: "unavailable", slots: "ok" })?.panes).toBe(
      "unavailable",
    );
    for (const wrong of [
      { slots: "ok" },
      { panes: "ok" },
      { panes: 1, herdr: "ok", slots: "ok" },
      { panes: "ok", herdr: 1, slots: "ok" },
    ]) {
      expect(readProbes(wrong)).toBeNull();
    }
  });

  it("read an agent the feed changed, added or removed", () => {
    expect(readAgentChange(SAMPLES.agentChange)).toMatchObject({ key: "amber", removed: false });
    expect(readAgentChange({ key: "amber", removed: true })).toEqual({
      key: "amber",
      removed: true,
    });
    expect(readAgentChange({ key: "amber", value: null })).toEqual({
      key: "amber",
      removed: false,
    });
    expect(readAgentChange({ key: 1 })).toBeNull();
    expect(readAgentChange({ key: "amber", removed: "yes" })).toBeNull();
    expect(readAgentChange({ key: "amber", value: { handle: "amber" } })).toBeNull();
  });
});

/** `value`'s `key`, as the page reads a field. */
function field(value: unknown, key: string): unknown {
  return (value as Record<string, unknown>)[key];
}

describe("an item put", () => {
  const key = (one: { id: string }) => one.id;
  it("is replaced in its place, added last or removed", () => {
    const items = [
      { id: "a", n: 1 },
      { id: "b", n: 1 },
    ];
    expect(put(items, { key: "a", value: { id: "a", n: 2 } }, key)).toEqual([
      { id: "a", n: 2 },
      { id: "b", n: 1 },
    ]);
    expect(put(items, { key: "c", value: { id: "c", n: 1 } }, key).map(key)).toEqual([
      "a",
      "b",
      "c",
    ]);
    expect(put(items, { key: "a", removed: true }, key).map(key)).toEqual(["b"]);
    expect(put(items, { key: "a", value: null }, key)).toBe(items);
  });
});

describe("an agent's line", () => {
  it("names only what is known, a dot between two", () => {
    expect(meta(["worker", "", "working", "", ""])).toBe("worker · working");
    expect(meta(["worker", "opus", "idle", "app/x", "x"])).toBe("worker · opus · idle · app/x · x");
    expect(meta(["", " ", ""])).toBe("");
    expect(agentLine(anAgent("a", { branch: "app/x", worktree: "/w/app-x" }))).toBe(
      "worker · opus · idle · app/x · app-x",
    );
    expect(agentLine(anAgent("a", { model: "", worktree: "/w/x/" }))).toBe("worker · idle");
  });

  it("trims the spaces Rust trims, and no other", () => {
    const nel = String.fromCharCode(0x85);
    const bom = String.fromCharCode(0xfeff);
    const ideographic = String.fromCharCode(0x3000);
    expect(trimmed(`${nel} worker\t${ideographic}`)).toBe("worker");
    expect(trimmed(`${bom}worker${bom}`)).toBe(`${bom}worker${bom}`);
    expect(trimmed(" \n ")).toBe("");
    expect(meta([nel, bom])).toBe(bom);
  });
});

describe("what an agent follows", () => {
  it("counts the open tasks it orchestrates as the server counts them, in words", () => {
    const whole = field(SAMPLES.agentChange, "value") as Record<string, unknown>;
    expect(readAgent(whole)?.following).toBe(2);
    expect(readAgent({ ...whole, following: 48 })?.following).toBe(48);
    const { following, ...older } = whole;
    expect(readAgent(older)?.following).toBe(0);
    expect(followingLabel(1)).toBe("Following 1 open task");
    expect(followingLabel(2)).toBe("Following 2 open tasks");
  });

  it("orders its tasks as the server pages them, the board's own in their place", () => {
    const task = (slug: string, priority: number, status: string, orchestrator?: string) =>
      aTask(slug, {
        priority,
        status,
        roles: orchestrator === undefined ? [] : [aRole("orchestrator", orchestrator)],
      });
    const page = (from: number, to: number) =>
      Array.from({ length: to - from + 1 }, (_, at) => task(`bulk/ready-${from + at}`, 3, "ready"));
    const board = [
      task("bulk/ready-5", 3, "review", "cobalt-otter"),
      task("app/urgent", 1, "ready", "cobalt-otter"),
      task("bulk/ready-12", 3, "done", "cobalt-otter"),
      task("bulk/ready-40", 3, "ready", "someone-else"),
    ];
    const slugs = (rows: Task[]) =>
      followedTasks(rows, board, "cobalt-otter").map((one) => [one.slug, one.status]);
    const expected = (numbers: number[]) => [
      ["app/urgent", "ready"],
      ...numbers.map((n) => [`bulk/ready-${n}`, n === 5 ? "review" : "ready"]),
    ];
    const upTo = (last: number) =>
      Array.from({ length: last }, (_, at) => at + 1).filter((n) => n !== 12);
    expect(slugs(page(1, 23))).toEqual(expected(upTo(23)));
    expect(slugs([...page(1, 23), ...page(24, 36)])).toEqual(expected(upTo(36)));
    expect(slugs([])).toEqual(expected([5]));
  });
});

describe("a machine's occupancy", () => {
  it("is its counts only from a fresh and complete report that has them", () => {
    const machine = aMachine("alpha", []);
    expect(occupancy(machine)).toBe("Builds: 0 running, 0 queued · Tests: 0 running, 0 queued");
    const counts = { running_builds: 1, queued_builds: 2, running_tests: 3, queued_tests: 4 };
    expect(occupancy({ ...machine, counts })).toBe(
      "Builds: 1 running, 2 queued · Tests: 3 running, 4 queued",
    );
    for (const freshness of [
      "stale",
      "disconnected",
      "incomplete",
      "conflict",
      "unknown",
    ] as const) {
      expect(occupancy({ ...machine, freshness })).toBe(
        `Occupancy unknown · ${freshnessWord(freshness)}`,
      );
    }
    expect(occupancy({ ...machine, complete: false })).toBe(
      "Occupancy unknown · partial inventory",
    );
    expect(occupancy({ ...machine, complete: false, freshness: "stale" })).toBe(
      "Occupancy unknown · stale",
    );
    expect(occupancy({ ...machine, counts: null })).toBe("Occupancy unknown · fresh");
  });
});

describe("the machines' work while the feed is lost", () => {
  it("claims no capacity, and keeps the exact phases last observed", () => {
    const work = aWork([aMachine("alpha", [anOperation("alpha", "test", "test", "running")])]);
    expect(connection(work, true)).toBe(work);
    const lost = connection(work, false);
    expect(lost.machines[0]?.freshness).toBe("disconnected");
    expect(lost.machines[0]?.counts).toBeNull();
    expect(lost.machines[0]?.operations).toBe(work.machines[0]?.operations);
    expect(work.machines[0]?.freshness).toBe("fresh");
    expect(occupancy(lost.machines[0] as MachineObservation)).toBe(
      "Occupancy unknown · disconnected",
    );
    expect(scopedPhases(lost, { agent: "worker-a" }).map(phaseLabel)).toEqual([
      "Observed Test running · alpha · disconnected",
    ]);
  });
});

describe("what an agent holds", () => {
  const tasks = [
    aTask("w/a", { status: "in_progress", roles: [aRole("executor", "amber")] }),
    aTask("w/b", { status: "ready", roles: [aRole("orchestrator", "amber")] }),
    aTask("w/c", { status: "done", updated: 1, roles: [aRole("reviewer", "amber")] }),
    aTask("w/d", { status: "cancelled", updated: 3, roles: [aRole("executor", "amber")] }),
    aTask("w/e", { status: "done", updated: 2, roles: [aRole("executor", "birch")] }),
    aTask("w/f", {
      status: "review",
      roles: [aRole("reviewer", "amber"), aRole("executor", "amber")],
    }),
  ];

  it("is the tasks it holds a role on, by the first it holds there", () => {
    const holding = held(tasks, "amber");
    expect(holding.map(([task, role]) => [task.slug, role.role])).toEqual([
      ["w/a", "executor"],
      ["w/b", "orchestrator"],
      ["w/c", "reviewer"],
      ["w/d", "executor"],
      ["w/f", "reviewer"],
    ]);
    expect(workingOn(holding).map(([task]) => task.slug)).toEqual(["w/a", "w/f"]);
    expect(finished(holding).map((task) => task.slug)).toEqual(["w/d", "w/c"]);
    expect(held(tasks, "cedar")).toEqual([]);
  });
});

describe("a task the page lists no record of", () => {
  const shown = (handle: string) => ["amber", "birch"].includes(handle);
  it("is taken when a shown agent works on it, or it finished", () => {
    const roles = (...pairs: [string, string][]) => ({
      roles: pairs.map(([handle, role]) => aRole(role, handle)),
    });
    expect(agentsPageTakes(roles(["amber", "orchestrator"], ["birch", "executor"]), shown)).toBe(
      true,
    );
    expect(agentsPageTakes(roles(["amber", "reviewer"]), shown)).toBe(true);
    expect(agentsPageTakes(roles(["amber", "orchestrator"]), shown)).toBe(false);
    expect(agentsPageTakes(roles(["cedar", "executor"]), shown)).toBe(false);
    expect(agentsPageTakes({ roles: [] }, shown)).toBe(false);
    for (const status of ["done", "cancelled"])
      expect(agentsPageTakes({ status }, shown)).toBe(true);
    for (const status of ["in_progress", "review", "ready"]) {
      expect(agentsPageTakes({ status }, shown)).toBe(false);
    }
    expect(
      agentsPageTakes({ title: "Renamed", priority: 1, status: null, roles: null }, shown),
    ).toBe(false);
  });
});

describe("the lists of agents", () => {
  it("are the connected ones and the lost, each by handle as Rust orders text", () => {
    const health = ["live", "stale", "lost", "live", "left", "lost"];
    const agents = ["b", "Z", "a", "é", "c", "d"].map((handle, at) =>
      anAgent(handle, { health: health[at] ?? "live" }),
    );
    expect(connectedAgents(agents).map((agent) => agent.handle)).toEqual(["Z", "b", "é"]);
    expect(lostAgents(agents).map((agent) => agent.handle)).toEqual(["a", "d"]);
  });

  it("happening now are the tasks in progress or in review", () => {
    const tasks = ["ready", "in_progress", "review", "done"].map((status) =>
      aTask(`w/${status}`, { status }),
    );
    expect(happening(tasks).map((task) => task.slug)).toEqual(["w/in_progress", "w/review"]);
  });
});

describe("a feed event's changes", () => {
  it("are read whole, each region the page follows", () => {
    const changes = readChanges(
      event(
        ["task", SAMPLES.taskAdded],
        ["task-fields", SAMPLES.taskChange],
        ["session", { key: "amber", value: { health: "stale" } }],
        ["session", { key: 1 }],
        ["agent", SAMPLES.agentChange],
        ["agent-probes", SAMPLES.agentProbes],
        ["machine-work", SAMPLES.work],
        ["tasks-status", { available: false, slow: true }],
        ["tasks-status", {}],
        ["message", { id: 1 }],
        ["pending", { key: "/docs/A.md#c" }],
      ),
      true,
    );
    expect(changes?.map((change) => change.region)).toEqual([
      "task",
      "task-fields",
      "session",
      "session",
      "agent",
      "agent-probes",
      "machine-work",
      "tasks-status",
      "tasks-status",
    ]);
    expect(changes?.[3]).toEqual({ region: "session", key: "", health: null });
    expect(changes?.[8]).toEqual({ region: "tasks-status", available: undefined, slow: false });
  });

  it("leave the probes unread while the page shows none", () => {
    expect(readChanges(event(["agent-probes", { slots: "ok" }]), false)).toEqual([]);
    expect(readChanges(event(["agent-probes", { slots: "ok" }]), true)).toBeNull();
  });

  it("are none when one of them cannot be read", () => {
    for (const [region, data] of [
      ["task", { key: 1 }],
      ["task-fields", { key: "w/a" }],
      ["agent", { key: "amber", value: {} }],
      ["machine-work", { machines: null }],
    ] as [string, unknown][]) {
      expect(readChanges(event(["agent", SAMPLES.agentChange], [region, data]), true)).toBeNull();
    }
  });
});

/** A page of the agents as the server writes one: `agents`, `tasks` and what `more` says. */
function agentPage(agents: Agent[], tasks: Task[] = [], more: Record<string, unknown> = {}) {
  return {
    view: { machine_work: NO_WORK, generated: 1, probes: { panes: "ok", slots: "ok" }, agents },
    tasks,
    total: agents.length,
    next: null,
    local: false,
    ...more,
  };
}

type Route = (url: string) => Response | Promise<Response>;

/** The live agents over a server that answers each read as `route` does; what it asked. */
function live(route: Route = () => answer(503)) {
  const asked: string[] = [];
  const held = vi.fn();
  vi.stubGlobal("document", { getElementById: () => null, querySelector: () => null });
  vi.stubGlobal("window", { reviewHoldPlace: held });
  vi.stubGlobal("fetch", async (url: string) => {
    asked.push(url);
    return route(url);
  });
  const host = {
    redraw: vi.fn(),
    board: vi.fn(),
    stale: vi.fn(),
    trouble: vi.fn(),
  } satisfies AgentsHost;
  const page = new AbortController();
  const agents = new LiveAgents("", host, page.signal);
  return { agents, d: agents.d, asked, host, held, page };
}

/** A promise the test settles: what a read waits on while the test does something meanwhile. */
function later<T>(): { promise: Promise<T>; settle: (value: T) => void } {
  let settle: (value: T) => void = () => {};
  const promise = new Promise<T>((done) => {
    settle = done;
  });
  return { promise, settle };
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval"] });
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

/** Lets `ms` pass, and what it settles run. */
const pass = (ms = 0) => vi.advanceTimersByTimeAsync(ms);
/** Ends the waits pending now (a retry's), and runs what they start. */
const waitEnds = () => vi.runOnlyPendingTimersAsync();
/** Runs every wait to its end, and those they start, until none is left: nothing tries again. */
const settled = () => vi.runAllTimersAsync();

describe("the agents' first read", () => {
  it("draws the page as the server answers it, both cursors as it says", async () => {
    const { agents, d, asked, host } = live(() => answer(200, SAMPLES.agentPage));
    d.lostCursor = "x";
    d.lostLoaded = true;
    d.error = "The task board is unavailable. Reconnecting…";
    await agents.load();
    expect(asked).toEqual([AGENT_PAGE_PATH]);
    expect(d.view?.agents.map((agent) => agent.handle)).toEqual(["amber", "birch"]);
    expect(d).toMatchObject({
      local: true,
      total: 2,
      agentCursor: "birch",
      lostCursor: null,
      lostLoaded: false,
      error: "",
    });
    expect(d.tasks.map((task) => task.slug)).toEqual(["w/a"]);
    expect(host.redraw).toHaveBeenCalled();
  });

  it("says why it failed, and keeps the agents it drew", async () => {
    const answers = [answer(500, { error: "The board is away" }), answer(200, { view: null })];
    const { agents, d } = live(() => answers.shift() as Response);
    d.view = PAGE.view;
    await agents.load();
    expect(d.error).toBe("The board is away (the server answered 500)");
    expect(d.view).toBe(PAGE.view);
    await agents.load();
    expect(d.error).toBe("The agents page's answer is not readable");
  });

  it("replays onto its answer what the feed said while it was read", async () => {
    const reply = later<Response>();
    const { agents, d } = live(() => reply.promise);
    const reading = agents.load();
    await pass();
    expect(agents.heard(event(["agent", { key: "cedar", value: anAgent("cedar") }]))).toBe(true);
    reply.settle(answer(200, agentPage([anAgent("amber")])));
    await reading;
    expect(d.view?.agents.map((agent) => agent.handle)).toEqual(["amber", "cedar"]);
    expect(d.journal.events).toEqual([]);
  });

  it("draws nothing once the page is gone", async () => {
    const { agents, d, host, page } = live(() => answer(200, SAMPLES.agentPage));
    page.abort();
    await agents.load();
    expect(d.view).toBeNull();
    expect(host.redraw).not.toHaveBeenCalled();
  });
});

describe("the agents' first read after the board's", () => {
  it("is tried again by itself while it fails, until it answers", async () => {
    let refused = 2;
    const { agents, d, asked } = live(() =>
      refused-- > 0 ? answer(503) : answer(200, SAMPLES.agentPage),
    );
    await agents.first();
    expect(d.error).toBe("The server answered 503");
    expect(asked).toHaveLength(1);
    await waitEnds();
    expect(asked).toHaveLength(2);
    await waitEnds();
    expect(asked).toHaveLength(3);
    expect(d.view?.agents).toHaveLength(2);
    expect(d.error).toBe("");
    await settled();
    expect(asked).toHaveLength(3);
  });

  it("stops trying once the next board read starts it again, or the page is gone", async () => {
    const { agents, asked, page } = live(() => answer(503));
    await agents.first();
    await agents.first();
    expect(asked).toHaveLength(2);
    await waitEnds();
    // The newer read's retry alone: the first one's stopped.
    expect(asked).toHaveLength(3);
    page.abort();
    await settled();
    expect(asked).toHaveLength(3);
  });
});

describe("the agents read again", () => {
  /** Live agents drawn from `PAGE`, the server answering `route`. */
  const drawn = (route: Route) => {
    const page = live(route);
    Object.assign(page.d, {
      view: PAGE.view,
      tasks: PAGE.tasks,
      total: 2,
      agentCursor: "birch",
      lostCursor: null,
      lostLoaded: false,
    });
    return page;
  };

  it("merges the first page for what the feed changed, keeping both cursors", async () => {
    const amber = anAgent("amber", { state: "working" });
    const { agents, d, asked } = drawn(() =>
      answer(200, agentPage([amber, anAgent("cedar")], [aTask("w/b")], { total: 5, next: "z" })),
    );
    d.error = "stale";
    d.lostCursor = "l";
    d.lostLoaded = true;
    await agents.refresh();
    expect(asked).toEqual([AGENT_PAGE_PATH]);
    expect(d.view?.agents.map((agent) => [agent.handle, agent.state])).toEqual([
      ["amber", "working"],
      ["birch", ""],
      ["cedar", "idle"],
    ]);
    expect(d.tasks.map((task) => task.slug)).toEqual(["w/a", "w/b"]);
    expect(d).toMatchObject({
      total: 5,
      agentCursor: "birch",
      lostCursor: "l",
      lostLoaded: true,
      error: "",
    });
  });

  it("reads the next page of the connected agents from its cursor, its count with it", async () => {
    const { agents, d, asked } = drawn(() =>
      answer(200, agentPage([anAgent("cedar")], [], { total: 9, next: "dune" })),
    );
    d.agentCursor = "b r&c";
    await agents.next(false);
    expect(asked).toEqual([`${AGENT_PAGE_PATH}?cursor=b%20r%26c`]);
    expect(d).toMatchObject({ agentCursor: "dune", total: 9, lostLoaded: false });
    d.agentCursor = null;
    await agents.next(false);
    expect(asked).toHaveLength(1);
  });

  it("reads the lost agents, the first page the first time, then from their cursor", async () => {
    const answers = [
      answer(200, agentPage([anAgent("lost-a", { health: "lost" })], [], { next: "lost-a" })),
      answer(200, agentPage([anAgent("lost-b", { health: "lost" })], [], { total: 0 })),
    ];
    const { agents, d, asked } = drawn(() => answers.shift() as Response);
    d.error = "kept";
    await agents.next(true);
    expect(d).toMatchObject({ lostCursor: "lost-a", lostLoaded: true, total: 2, error: "kept" });
    agents.more(true);
    await pass();
    expect(asked).toEqual([
      `${AGENT_PAGE_PATH}?health=lost`,
      `${AGENT_PAGE_PATH}?health=lost&cursor=lost-a`,
    ]);
    expect(d.lostCursor).toBeNull();
    expect(d.view?.agents.map((agent) => agent.handle)).toEqual([
      "amber",
      "birch",
      "lost-a",
      "lost-b",
    ]);
    await agents.next(true);
    expect(asked).toHaveLength(2);
  });

  it("says why a merge failed, and changes nothing else", async () => {
    const { agents, d } = drawn(() => answer(503, { error: "Busy" }));
    await agents.next(true);
    expect(d.error).toBe("Busy (the server answered 503)");
    expect(d.lostLoaded).toBe(false);
    expect(d.view).toBe(PAGE.view);
  });

  it("reads the first page afresh while none is drawn", async () => {
    const { agents, d } = live(() => answer(200, SAMPLES.agentPage));
    await agents.refresh();
    expect(d.agentCursor).toBe("birch");
  });

  it("merges nothing into a page not drawn, nor once the page is gone", async () => {
    const { agents, d, host, page } = live(() => answer(200, SAMPLES.agentPage));
    await agents.merge("", null);
    expect(d.view).toBeNull();
    expect(d.tasks).toEqual([]);
    page.abort();
    host.redraw.mockClear();
    await agents.merge("", true);
    expect(d.lostLoaded).toBe(false);
    expect(host.redraw).not.toHaveBeenCalled();
  });

  it("reads once at a time, and once more for what was asked meanwhile", async () => {
    const replies: ((response: Response) => void)[] = [];
    const { agents, asked } = drawn(
      () =>
        new Promise<Response>((settle) => {
          replies.push(settle);
        }),
    );
    const first = agents.reread();
    void agents.reread();
    void agents.reread();
    await pass();
    expect(asked).toHaveLength(1);
    replies.shift()?.(answer(200, agentPage([])));
    await pass();
    expect(asked).toHaveLength(2);
    replies.shift()?.(answer(200, agentPage([])));
    await first;
    expect(asked).toHaveLength(2);
  });
});

describe("a feed event on the agents page", () => {
  /** Live agents drawn from `agents` and `tasks`, the server answering every read with them. */
  const shown = (agents: Agent[], tasks: Task[] = []) => {
    const page = live(() => answer(200, agentPage(agents, tasks)));
    Object.assign(page.d, {
      view: { work: NO_WORK, generated: 1, probes: { panes: "ok", slots: "ok" }, agents },
      tasks,
    });
    return page;
  };

  it("puts in, replaces and removes the agents and tasks it names", () => {
    const { agents, d, host, held } = shown([anAgent("amber")], [aTask("w/a"), aTask("w/b")]);
    expect(
      agents.heard(
        event(
          ["agent", { key: "birch", value: anAgent("birch") }],
          ["agent", { key: "amber", removed: true }],
          ["task", SAMPLES.taskRemoved],
          ["task", { key: "w/c", value: aTask("w/c") }],
        ),
      ),
    ).toBe(true);
    expect(d.view?.agents.map((agent) => agent.handle)).toEqual(["birch"]);
    expect(d.tasks.map((task) => task.slug)).toContain("w/c");
    expect(host.redraw).toHaveBeenCalledTimes(1);
    expect(held).toHaveBeenCalledWith("[id^=message-], [id^=child-], [data-slug]");
  });

  it("holds the reader's place, then draws the change at once, within the event", () => {
    const { agents, held, host } = shown([anAgent("amber")], [aTask("w/a")]);
    const order: string[] = [];
    held.mockImplementation(() => order.push("held"));
    host.redraw.mockImplementation(() => order.push(react.flushing ? "drawn" : "drawn later"));
    agents.heard(event(["task", { key: "w/b", value: aTask("w/b") }]));
    // An event that changes nothing the page shows neither holds nor draws.
    agents.heard(event(["task-fields", { key: "w/x", fields: { title: "Elsewhere" } }]));
    agents.heard(event(["agent", { key: "birch", value: anAgent("birch") }]));
    expect(order).toEqual(["held", "drawn", "held", "drawn"]);
    expect(react.flushing).toBe(false);
  });

  it("changes the fields of a task it lists, and its holders' health", () => {
    const { agents, d, asked } = shown(
      [anAgent("amber")],
      [aTask("w/a", { roles: [aRole("executor", "amber")] })],
    );
    agents.heard(event(["task-fields", { key: "w/a", fields: { title: "Renamed" } }]));
    agents.heard(event(["session", { key: "amber", value: { health: "stale" } }]));
    expect(d.tasks[0]?.title).toBe("Renamed");
    expect(d.tasks[0]?.roles[0]?.health).toBe("stale");
    expect(asked).toEqual([]);
  });

  it("reads the page again for a task it lists no record of that a shown agent took", async () => {
    const { agents, asked, host } = shown([anAgent("amber")]);
    const roles = (handle: string, role: string) => ({ roles: [aRole(role, handle)] });
    agents.heard(event(["task-fields", { key: "w/x", fields: roles("cedar", "executor") }]));
    agents.heard(event(["task-fields", { key: "w/x", fields: roles("amber", "orchestrator") }]));
    await pass();
    expect(asked).toEqual([]);
    host.redraw.mockClear();
    agents.heard(event(["task-fields", { key: "w/x", fields: roles("amber", "executor") }]));
    expect(host.redraw).not.toHaveBeenCalled();
    await pass();
    expect(asked).toEqual([AGENT_PAGE_PATH]);
  });

  it("takes the probes and the machines' work, all of it", () => {
    const { agents, d } = shown([anAgent("amber")]);
    const work = aWork([aMachine("alpha", [anOperation("alpha", "b", "build", "running")])]);
    agents.heard(
      event(["agent-probes", { herdr: "unavailable", slots: "ok" }], ["machine-work", work]),
    );
    expect(d.view?.probes).toEqual({ panes: "unavailable", slots: "ok" });
    expect(d.view?.work).toEqual(work);
  });

  it("says the board's state, and reads the page again once it is back", async () => {
    const { agents, d, asked } = shown([anAgent("amber")]);
    agents.heard(event(["tasks-status", { available: false, slow: false }]));
    expect(d.error).toBe("The task board is unavailable. Reconnecting…");
    agents.heard(event(["tasks-status", { available: false, slow: false }]));
    await pass();
    expect(asked).toEqual([]);
    agents.heard(event(["tasks-status", { available: true }]));
    expect(d.error).toBe("");
    await pass();
    expect(asked).toEqual([AGENT_PAGE_PATH]);
  });

  it("changes nothing on a page with no agents drawn but the fields of the tasks it lists", () => {
    const { agents, d, host } = live();
    agents.heard(
      event(
        ["agent", SAMPLES.agentChange],
        ["task", SAMPLES.taskAdded],
        ["machine-work", SAMPLES.work],
        ["task-fields", { key: "w/x", fields: { status: "done" } }],
      ),
    );
    expect(d.view).toBeNull();
    expect(d.tasks).toEqual([]);
    expect(host.redraw).not.toHaveBeenCalled();
  });

  it("loses the stream when it cannot be read, changing nothing", () => {
    const { agents, d } = shown([anAgent("amber")]);
    expect(
      agents.heard(
        event(["agent", { key: "birch", value: anAgent("birch") }], ["task", { key: 1 }]),
      ),
    ).toBe(false);
    expect(d.view?.agents.map((agent) => agent.handle)).toEqual(["amber"]);
    expect(d.journal.version).toBe(0);
  });
});

describe("the board's loop on the agents page", () => {
  it("tells the page the board was read, the feed's state and why a read failed", async () => {
    const { agents, d, host } = live(() => answer(200, SAMPLES.agentPage));
    const run = agents.run({ refresh: vi.fn(), connected: vi.fn(), heard: vi.fn() });
    run.show({ version: 3, ping_ms: 1, build: "" });
    expect(run.board.current?.version).toBe(3);
    expect(host.board).toHaveBeenCalled();
    run.setStale(true);
    expect(host.stale).toHaveBeenCalledWith(true);
    run.setTrouble("The server could not be reached");
    expect(host.trouble).toHaveBeenCalledWith("The server could not be reached");
    run.views.freshen((f) => f);
    expect(run.views.heard(event(["agent", { key: "amber", removed: true }]))).toBe(true);
    await run.views.read();
    expect(d.view?.agents).toHaveLength(2);
    expect(run.retry).toBe(agents.retry);
  });

  it("reads the board, then the agents, until the page is gone", async () => {
    const { agents, asked, host, page } = live((url) =>
      url.startsWith("/api/board")
        ? answer(200, { version: 1, ping_ms: 60_000, build: "" })
        : answer(200, SAMPLES.agentPage),
    );
    const listen = { addEventListener: () => {}, removeEventListener: () => {} };
    vi.stubGlobal("window", listen);
    vi.stubGlobal("document", { ...listen, getElementById: () => null, querySelector: () => null });
    vi.stubGlobal("WebSocket", undefined);
    vi.stubGlobal("location", { reload: vi.fn() });
    vi.stubGlobal("sessionStorage", { getItem: () => null, setItem: () => {} });
    const running = agents.start({ refresh: vi.fn(), connected: vi.fn(), heard: vi.fn() });
    await pass();
    expect(asked.slice(0, 2)).toEqual(["/api/board", AGENT_PAGE_PATH]);
    expect(host.board).toHaveBeenCalled();
    page.abort();
    await pass(60_000);
    await running;
  });
});

describe("an agent's Following list", () => {
  const path = (more = "") =>
    `/api/task-board?holder=a%20b&role=orchestrator&open=1&older=1${more}`;
  const listing = (tasks: Task[], next: string | null = null) => ({
    tasks,
    statuses: [],
    signoff: [],
    counts: [["ready", tasks.length]],
    lifecycle_counts: [],
    next,
  });

  /** The list of `a b`'s, over a server that answers as `route` does. */
  const list = (route: Route) => {
    const page = live(route);
    const redraw = vi.fn();
    return { ...page, redraw, following: new Followed("a b", "", page.page.signal, redraw) };
  };

  it("reads its first page when it opens, and nothing when it closes", async () => {
    const { following, asked, redraw } = list(() =>
      answer(200, listing([aTask("w/a"), aTask("w/b")], "w/b")),
    );
    following.toggle(true);
    expect(following.open).toBe(true);
    following.toggle(true);
    await pass();
    expect(asked).toEqual([path()]);
    expect(following.rows.map((task) => task.slug)).toEqual(["w/a", "w/b"]);
    expect(following.cursor).toBe("w/b");
    following.toggle(false);
    expect(following.open).toBe(false);
    expect(asked).toHaveLength(1);
    expect(redraw).toHaveBeenCalled();
  });

  it("reads the next page from its cursor, a row it lists already kept once", async () => {
    const answers = [
      answer(200, listing([aTask("w/a")], "w/a")),
      answer(200, listing([aTask("w/a"), aTask("w/c")])),
      answer(200, listing([aTask("w/d")])),
    ];
    const { following, asked } = list(() => answers.shift() as Response);
    await following.load(false);
    await following.load(true);
    expect(asked).toEqual([path(), path("&cursor=w%2Fa")]);
    expect(following.rows.map((task) => task.slug)).toEqual(["w/a", "w/c"]);
    expect(following.cursor).toBeNull();
    await following.load(true);
    expect(asked[2]).toBe(path());
    expect(following.rows.map((task) => task.slug)).toEqual(["w/a", "w/c", "w/d"]);
  });

  it("says why a read failed, until another fails, its rows kept", async () => {
    const answers = [
      answer(200, listing([aTask("w/a")])),
      answer(500, { error: "Busy" }),
      answer(200, listing([aTask("w/b")])),
      answer(200, { tasks: null }),
    ];
    const { following } = list(() => answers.shift() as Response);
    await following.load(false);
    await following.load(true);
    expect(following.trouble).toBe("Busy (the server answered 500)");
    expect(following.rows.map((task) => task.slug)).toEqual(["w/a"]);
    await following.load(false);
    expect(following.trouble).toBe("Busy (the server answered 500)");
    expect(following.rows.map((task) => task.slug)).toEqual(["w/b"]);
    await following.load(false);
    expect(following.trouble).toBe("The task board's answer is not a listing");
  });

  it("changes nothing once the page is gone", async () => {
    const { following, page, redraw } = list(() => answer(200, listing([aTask("w/a")])));
    page.abort();
    await following.load(false);
    expect(following.rows).toEqual([]);
    expect(redraw).not.toHaveBeenCalled();
  });
});

describe("the agents page's requests", () => {
  it("are given up on the wire once the page ends, and change nothing", async () => {
    const { agents, d, host, page } = live();
    const redraw = vi.fn();
    const following = new Followed("amber", "", page.signal, redraw);
    const signals: AbortSignal[] = [];
    // As the browser's: a fetch whose signal ends refuses.
    vi.stubGlobal(
      "fetch",
      (_url: string, init?: RequestInit) =>
        new Promise<Response>((_answer, refuse) => {
          const signal = init?.signal;
          if (!signal) return;
          signals.push(signal);
          signal.addEventListener("abort", () => refuse(new DOMException("aborted", "AbortError")));
        }),
    );
    d.view = PAGE.view;
    d.agentCursor = "birch";
    const reads = [agents.load(), agents.next(true), agents.next(false), following.load(false)];
    expect(signals.map((signal) => signal.aborted)).toEqual([false, false, false, false]);
    page.abort();
    expect(signals.map((signal) => signal.aborted)).toEqual([true, true, true, true]);
    await Promise.all(reads);
    expect(d).toMatchObject({
      view: PAGE.view,
      agentCursor: "birch",
      lostLoaded: false,
      error: "",
    });
    expect(following.rows).toEqual([]);
    expect(following.trouble).toBe("");
    expect(host.redraw).not.toHaveBeenCalled();
    expect(redraw).not.toHaveBeenCalled();
  });
});
