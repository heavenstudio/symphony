import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  AgentLines,
  AgentRow,
  AgentsMain,
  AgentsPage,
  AgentsView,
  type AgentsViewProps,
  MachinesView,
  standing,
} from "./agents";
import {
  type AgentPageRead,
  type AgentsRead,
  connection,
  Followed,
  LiveAgents,
  readAgentPage,
  readAgents,
} from "./agents-live";
import {
  aMachine,
  anAgent,
  anOperation,
  aRole,
  aTask,
  aWork,
  CASES,
  type Found,
  fire,
  reading,
} from "./testing";
import type { Agent, HeldTask, MachineWork, Task } from "./wire";
import { NO_WORK } from "./work";

afterEach(() => vi.unstubAllGlobals());

/** A Following list never read, whatever `fields` say it holds. */
function followed(fields: Partial<Followed> = {}): Followed {
  return Object.assign(
    new Followed("cobalt-otter", "", new AbortController().signal, () => {}),
    fields,
  );
}

/** `agent`'s row, over `tasks` and `work`, its Following list as `following` holds it. */
function row(
  agent: Agent,
  {
    tasks = [],
    work = NO_WORK,
    local = false,
    following = followed(),
    names = new Map(),
  }: {
    tasks?: Task[];
    work?: MachineWork;
    local?: boolean;
    following?: Followed;
    names?: ReadonlyMap<string, string>;
  } = {},
): string {
  return renderToStaticMarkup(
    <AgentLines
      agent={agent}
      local={local}
      tasks={tasks}
      work={work}
      following={following}
      names={names}
    />,
  );
}

const agent = (fields: Partial<Agent> = {}) => anAgent("cobalt-otter", fields);

describe("an agent's row", () => {
  it("names the agent, its health and its line of what is known", () => {
    const html = row(agent({ model: "", state: "working", health: "stale" }));
    expect(html).toMatch(
      /^<div class="agent"><strong>cobalt-otter<\/strong> <span class="chip health-stale">stale<\/span><p class="muted">worker · working<\/p>/,
    );
  });

  it("shows the slots it holds against its cap on an orchestrator's, and none without one", () => {
    // Its slots, as PULL names them: the tasks it is building, not the open tasks it follows.
    const html = row(agent({ slots: { open: 3, cap: 8 }, following: 32 }));
    expect(html).toContain("<p>slots: 3 of 8</p>");
    expect(html).toContain("Following 32 open tasks");
    expect(html).not.toContain("open tasks:");
    expect(row(agent())).not.toContain("slots:");
  });

  it("says how to reach it: by its callback, or on a task while it has none", () => {
    const called = row(agent({ callback: "unix:/tmp/c.sock" }));
    expect(called).toContain(
      '<details class="agent-connection"><summary>Connection details</summary><p>Native callback: <code>unix:/tmp/c.sock</code></p><p>Send: <code>just agent-send cobalt-otter &quot;text&quot;</code></p></details>',
    );
    const none = row(agent());
    expect(none).toContain(
      "<p>Send on a task: <code>just tasks say SLUG &quot;text&quot; --to cobalt-otter</code></p>",
    );
    expect(none).toContain(
      "<p>No channel registered — <code>just tasks inbox --as cobalt-otter</code></p>",
    );
    expect(none).toContain(
      "<p>Register in that agent&#x27;s session: <code>just tasks callback --as cobalt-otter</code></p>",
    );
  });

  it("says where chat goes, its parent and the run it is in, when it has them", () => {
    const html = row(
      agent({
        chat: true,
        parent: "orch",
        state_detail: { recipe: "test-rs", phase: "running", since: null },
      }),
    );
    expect(html).toContain("<p>Chat goes to cobalt-otter.</p>");
    expect(html).toContain("<p>parent: orch</p>");
    expect(html).toContain("<p>test-rs · running</p>");
    const bare = row(agent());
    for (const absent of ["Chat goes", "parent:", " · running"]) expect(bare).not.toContain(absent);
  });

  it("carries its Go to session control, and one with nothing known has none", () => {
    const paned = agent({ go: { handle: "cobalt-otter", url: null, pane: "t7" } });
    expect(row(paned, { local: true })).toContain('data-go="focus"');
    expect(row(paned, { local: false })).toContain('data-go="screen"');
    expect(row(agent(), { local: true })).not.toContain("Go to session");
  });

  it("lists the open tasks it works on, not those it orchestrates or finished", () => {
    const tasks = [
      aTask("w/a", {
        title: "A",
        status: "in_progress",
        roles: [aRole("executor", "cobalt-otter")],
      }),
      aTask("w/b", { title: "B", status: "ready", roles: [aRole("orchestrator", "cobalt-otter")] }),
      aTask("w/c", { title: "C", status: "done", roles: [aRole("executor", "cobalt-otter")] }),
      aTask("w/d", { title: "D", status: "review", roles: [aRole("executor", "birch")] }),
    ];
    const html = row(agent(), { tasks });
    expect(html).toContain('<ul><li><a href="/task/w/a">A</a> · executor · in progress</li></ul>');
    expect(row(agent())).toContain("<ul></ul>");
  });

  it("counts the open tasks it follows in words, and following none offers nothing", () => {
    const following = (count: number) => row(agent({ kind: "orchestrator", following: count }));
    expect(following(0)).not.toContain("Following");
    expect(following(1)).toContain("<details><summary>Following 1 open task</summary></details>");
    expect(following(2)).toContain(">Following 2 open tasks</summary>");
    // The count is the server's, whatever roles the agent's held tasks name.
    const executes: HeldTask = { slug: "a/w", role: "executor", status: "in_progress" };
    expect(row(agent({ tasks: [executes] }))).not.toContain("Following");
  });

  it("says a task's place where a custom workflow holds it, by its names or its IDs till then", () => {
    const place = { phase: "build", state: "waiting_on", revision: 3 };
    const held = aTask("w/held", {
      title: "Held",
      status: "blocked",
      roles: [aRole("executor", "cobalt-otter")],
      workflow: place,
    });
    const names = new Map([["build/waiting_on", "Build · Waiting on"]]);
    expect(row(agent(), { tasks: [held], names })).toContain(
      '<a href="/task/w/held">Held</a> · executor · Build · Waiting on</li>',
    );
    expect(row(agent(), { tasks: [held] })).toContain(
      '<a href="/task/w/held">Held</a> · executor · build · waiting on</li>',
    );
    expect(standing(aTask("w/own", { status: "in_progress" }), names)).toBe("in progress");
    expect(standing({ ...held, workflow: null }, names)).toBe("blocked");
    const drawn = renderToStaticMarkup(
      <AgentRow
        agent={agent()}
        local={false}
        tasks={[held]}
        work={NO_WORK}
        scope=""
        names={names}
      />,
    );
    expect(drawn).toContain("· executor · Build · Waiting on</li>");
  });

  it("opens its Following list: why a read failed, the tasks it follows, and more to load", () => {
    const one = aTask("a/one", { title: "One", status: "review" });
    const open = followed({ open: true, rows: [one], trouble: "Busy" });
    const html = row(agent({ following: 2 }), { following: open });
    expect(html).toContain('<p role="status">Busy</p>');
    expect(html).toContain('id="following-cobalt-otter-a/one"');
    expect(html).toContain(">Load more followed tasks</button>");
    const both = followed({ open: true, rows: [one, aTask("a/two")] });
    const all = row(agent({ following: 2 }), { following: both });
    expect(all).toContain('<p role="status"></p>');
    expect(all).not.toContain("Load more followed tasks");
  });

  it("names the five newest tasks it finished, a dot between two and none after the last", () => {
    const finished = (count: number) =>
      Array.from({ length: count }, (_, at) =>
        aTask(`done/${at + 1}`, {
          title: `Task ${at + 1}`,
          status: "done",
          updated: at + 1,
          roles: [aRole("executor", "cobalt-otter")],
        }),
      );
    const line = (count: number) => {
      const html = row(agent(), { tasks: finished(count) });
      const at = html.indexOf("Finished: ");
      return html.slice(at, html.indexOf("</p>", at));
    };
    const two = line(2);
    expect(two).toBe(
      'Finished: <a href="/task/done/2">Task 2</a> · <a href="/task/done/1">Task 1</a>',
    );
    const seven = line(7);
    expect(seven.endsWith(">Task 3</a> · and 2 more")).toBe(true);
    expect(seven.split(" · ")).toHaveLength(6);
    expect(row(agent())).not.toContain("Finished");
  });

  it("links its latest message", () => {
    const html = row(agent({ last_message: { slug: "w/a", ts: 1, text: "Built." } }));
    expect(html).toContain('<p><a href="/task/w/a">Latest message</a>: Built.</p>');
  });

  it("shows its builds and tests as a task's card shows them, the lifecycle untouched", () => {
    const machine = (id: string, kind: "build" | "test", state: "queued" | "running") =>
      aMachine(id, [
        anOperation(id, "separate-machine-same-nonce", kind, state, {
          handle: "cobalt-otter",
          task: "fixture/task",
        }),
      ]);
    let work = aWork([machine("alpha", "build", "queued"), machine("beta", "test", "running")]);
    const steps: [MachineWork, string[]][] = [];
    steps.push([work, ["Build queued · alpha", "Test running · beta"]]);
    work = aWork([machine("alpha", "build", "running"), machine("beta", "test", "running")]);
    steps.push([work, ["Build running · alpha", "Test running · beta"]]);
    work = aWork([aMachine("alpha", []), machine("beta", "test", "running")]);
    steps.push([work, ["Test running · beta"]]);
    steps.push([connection(work, false), ["Observed Test running · beta · disconnected"]]);
    for (const [shown, expected] of steps) {
      const html = row(agent(), { work: shown });
      expect(html.match(/data-work-kind=/g)?.length ?? 0).toBe(expected.length);
      for (const label of expected) expect(html).toContain(label);
      expect(html).not.toContain("slots:");
    }
  });

  it("is drawn with its Following list closed", () => {
    const html = renderToStaticMarkup(
      <AgentRow agent={agent({ following: 1 })} local={false} tasks={[]} work={NO_WORK} scope="" />,
    );
    expect(html).toContain("<details><summary>Following 1 open task</summary></details>");
  });
});

/** The agents of the server's sample page. */
const SAMPLE = (readAgentPage(CASES.samples.agentPage) as AgentPageRead).view;

describe("the agents page's view", () => {
  const view = (fields: Partial<AgentsViewProps> = {}) =>
    renderToStaticMarkup(
      <AgentsView
        view={SAMPLE}
        work={SAMPLE.work}
        local={false}
        tasks={[]}
        total={0}
        more={false}
        lostMore={false}
        onMore={() => {}}
        scope=""
        {...fields}
      />,
    );

  it("heads the page, and says what the machine probes answered", () => {
    const html = view();
    expect(html).toMatch(
      /^<div class="page-head"><div><h1>Agents<\/h1><\/div><\/div><p class="muted">Terminal panes: ok · Test slots: unavailable<\/p><h2>Happening now<\/h2>/,
    );
  });

  it("shows the panes probe a server named herdr", () => {
    const older = readAgents({
      generated: 1.5,
      probes: { herdr: "unavailable", slots: "ok" },
      agents: [],
    });
    expect(view({ view: older as AgentsRead })).toContain(
      ">Terminal panes: unavailable · Test slots: ok</p>",
    );
  });

  it("holds a place for each task happening now and each agent connected, lost ones apart", () => {
    const tasks = ["ready", "in_progress", "review"].map((status) =>
      aTask(`w/${status}`, { status }),
    );
    const agents = [anAgent("b"), anAgent("a"), anAgent("gone", { health: "left" })];
    const lost = anAgent("l", { health: "lost" });
    const html = view({ view: { ...SAMPLE, agents: [...agents, lost] }, tasks });
    expect(html).toContain('id="now-w/in_progress"');
    expect(html).toContain('id="now-w/review"');
    expect(html).not.toContain('id="now-w/ready"');
    const connected = html.slice(html.indexOf('<div class="agents agents-card card">'));
    expect(connected.indexOf('id="agent-a"')).toBeLessThan(connected.indexOf('id="agent-b"'));
    expect(html).not.toContain('id="agent-gone"');
    expect(html).toMatch(/<details><summary>Lost agents<\/summary><div><div id="agent-l"/);
    expect(html).toContain('<details id="machine-context" class="machine-work"');
  });

  it("offers more agents while the server has more than it shows, and more lost ones", () => {
    const agents = [anAgent("a"), anAgent("b")];
    const shown = { view: { ...SAMPLE, agents } };
    expect(view({ ...shown, more: true, total: 3 })).toContain('id="more-agents"');
    expect(view({ ...shown, more: true, total: 2 })).not.toContain('id="more-agents"');
    expect(view({ ...shown, more: false, total: 3 })).not.toContain('id="more-agents"');
    expect(view({ lostMore: true })).toContain(">Load more lost agents</button></div></details>");
    expect(view({ lostMore: false })).not.toContain("Load more lost agents");
  });
});

describe("the machines' context", () => {
  const draw = (work: MachineWork) => renderToStaticMarkup(<MachinesView work={work} />);

  it("says occupancy is unknown with no machine", () => {
    expect(draw(NO_WORK)).toBe(
      '<details id="machine-context" class="machine-work" aria-label="Machine build and test work"><summary>Machine context</summary><p class="muted">Machine occupancy unknown</p></details>',
    );
  });

  it("gives each machine its row: occupancy, advisory targets, phases and its reason", () => {
    const work = aWork(
      [
        aMachine("alpha", [anOperation("alpha", "one", "build", "queued")], {
          complete: false,
          reason: "inventory cut short",
          label: "Alpha",
        }),
      ],
      { guidance: { alpha: { build: 2, test: 1 }, beta: { build: 0, test: 3 } } },
    );
    const html = draw(work);
    expect(html).toContain(
      '<div class="machine-work-row" data-machine-id="alpha"><strong>Alpha</strong>',
    );
    expect(html).toContain("<p>Occupancy unknown · partial inventory</p>");
    expect(html).toContain("Advisory targets · builds 2 · tests 1");
    expect(html).toContain("Advisory targets · builds 0 · tests 3");
    expect(html).toContain(
      '<div class="machine-work-row" data-machine-id="beta"><strong>beta</strong><p>Machine observation unknown</p>',
    );
    expect(html).not.toContain("These targets do not enforce admission.");
    expect(html).toContain('<p class="muted">Machine ID: <code>alpha</code></p>');
    expect(html).toContain("Observed Build queued · Alpha · partial inventory");
    expect(html).toContain('<p class="muted">inventory cut short</p>');
    expect(html).not.toContain("Builds: 0 running");
    expect(html).not.toContain("Machine occupancy unknown");
    expect(draw({ ...work, guidance: {} })).toContain(
      "Advisory targets · builds unknown · tests unknown",
    );
  });

  it("orders the machines only advised as Rust orders their ids", () => {
    const work = aWork([], {
      guidance: {
        b: { build: 1, test: null },
        "10": { build: null, test: 2 },
        a: { build: null, test: null },
      },
    });
    const html = draw(work);
    const ids = [...html.matchAll(/data-machine-id="([^"]+)"/g)].map((found) => found[1]);
    expect(ids).toEqual(["10", "a", "b"]);
    expect(html).toContain("Advisory targets · builds 1 · tests unknown");
  });

  it("says the work no machine vouches for leaves capacity unknown, in no machine's row", () => {
    const unknown = anOperation(null, "native", "build", "queued");
    const html = draw(aWork([], { unassigned: [unknown] }));
    expect(html).toContain(
      '<p class="muted">Some work has no proven machine identity; capacity remains unknown.</p>',
    );
    expect(html).toContain("Observed Build queued · Machine unknown · unknown");
    expect(html).not.toContain("data-machine-id");
  });

  it("keeps unattributed work, and claims no capacity while the feed is lost", () => {
    const alone = anOperation("alpha", "other", "build", "queued", { handle: null, task: null });
    expect(draw(aWork([aMachine("alpha", [alone])]))).toContain("Build queued · alpha");
    const work = aWork([aMachine("alpha", [anOperation("alpha", "test", "test", "running")])]);
    const lost = draw(connection(work, false));
    expect(lost).toContain("<p>Occupancy unknown · disconnected</p>");
    expect(lost).toContain("Observed Test running · alpha · disconnected");
    expect(lost).toContain('data-work-kind="test"');
  });
});

describe("the agents page", () => {
  /** Live agents as `fields` hold them. */
  const live = (fields: Partial<LiveAgents["d"]> = {}) => {
    const agents = new LiveAgents(
      "",
      { redraw: () => {}, board: () => {}, stale: () => {}, trouble: () => {} },
      new AbortController().signal,
    );
    Object.assign(agents.d, fields);
    return agents;
  };
  const main = (
    agents: LiveAgents | null,
    boardRead: boolean,
    trouble: string | null = null,
    stale = false,
  ) =>
    renderToStaticMarkup(
      <AgentsMain live={agents} boardRead={boardRead} stale={stale} trouble={trouble} />,
    );

  it("loads the board first, under its notices", () => {
    const html = renderToStaticMarkup(<AgentsPage scope="" />);
    expect(html).toMatch(/^<main data-ready="loading" data-feed="live"><div class="skeleton/);
    expect(html).toContain("Loading the board…");
    expect(html).toContain('class="stale page-notices notice-stack" hidden=""');
  });

  it("says why the board could not be loaded, trying again on its own", () => {
    const agents = live();
    const html = main(agents, false, "The server could not be reached");
    expect(html).toMatch(
      /^<main data-ready="failed" data-feed="live"><section class="trouble card"/,
    );
    expect(html).toContain("<h1>The board could not be loaded</h1>");
    expect(html).toContain(
      "<p>The server could not be reached; check the connection. Trying again…</p>",
    );
  });

  it("loads the agents once the board is read", () => {
    const html = main(live(), true);
    expect(html).toContain(
      '<main data-ready="board" data-feed="live"><div class="task-page" data-ready="loading"><p>Loading agents…</p></div>',
    );
  });

  it("says why the agents could not be loaded, in place of them and not in its notices", () => {
    const html = main(live({ error: "Busy (the server answered 503)" }), true);
    expect(html).toContain(
      '<div class="task-page" data-ready="failed"><section class="trouble card"',
    );
    expect(html).toContain("<h1>The agents could not be loaded</h1>");
    // The page reads them again by itself, and says so.
    expect(html).toContain("<p>Busy (the server answered 503). Trying again…</p>");
    expect(html).not.toContain('role="alert"');
  });

  it("draws the agents, a failed read since in its notices, the work disconnected while the feed is lost", () => {
    const work = aWork([aMachine("alpha", [])]);
    const agents = live({ view: { ...SAMPLE, work }, error: "Busy" });
    const html = main(agents, true, null, true);
    expect(html).toMatch(
      /^<main data-ready="board" data-feed="stale"><div class="task-page" data-ready="agents">/,
    );
    expect(html).toContain('<p class="pill wait" role="alert">Busy</p>');
    expect(html).toContain("Reconnecting: the board may be out of date");
    expect(html).toContain("<p>Occupancy unknown · disconnected</p>");
    expect(main(live({ view: { ...SAMPLE, work } }), true)).toContain(
      "<p>Builds: 0 running, 0 queued · Tests: 0 running, 0 queued</p>",
    );
  });

  it("asks the board's loop for another try when Retry is pressed", () => {
    const agents = live();
    const page = AgentsMain({ live: agents, boardRead: false, stale: false, trouble: "Gone" });
    const trouble = (element: Found) => element.props.title === "The board could not be loaded";
    fire(page, trouble, "retry");
    fire(page, trouble, "retry");
    expect(agents.retry.current).toBe(2);
    const none = AgentsMain({ live: null, boardRead: false, stale: false, trouble: "Gone" });
    expect(() => fire(none, trouble, "retry")).not.toThrow();
  });
});

describe("what a press asks for", () => {
  it("reads the next page of agents as the list nears its end or Load more is pressed", () => {
    const onMore = vi.fn();
    const agents = [anAgent("a"), anAgent("b")];
    const view = AgentsView({
      view: { ...SAMPLE, agents },
      work: NO_WORK,
      local: false,
      tasks: [],
      total: 3,
      more: true,
      lostMore: true,
      onMore,
      scope: "",
    });
    fire(view, (element) => element.props.id === "more-agents", "onEnter");
    fire(view, reading("Load more agents"), "onClick");
    expect(onMore.mock.calls).toEqual([[false], [false]]);
    onMore.mockClear();
    fire(view, (element) => element.type === "details", "onToggle");
    fire(view, reading("Load more lost agents"), "onClick");
    expect(onMore.mock.calls).toEqual([[true], [true]]);
  });

  it("opens and closes the Following list, and reads its next page on Load more", () => {
    const following = followed();
    const toggle = vi.spyOn(following, "toggle").mockImplementation(() => {});
    const drawn = (list: Followed) =>
      AgentLines({
        agent: agent({ following: 2 }),
        local: false,
        tasks: [],
        work: NO_WORK,
        following: list,
      });
    const list = (element: Found) => element.type === "details" && "onToggle" in element.props;
    fire(drawn(following), list, "onToggle", { currentTarget: { open: true } });
    fire(drawn(following), list, "onToggle", { currentTarget: { open: false } });
    expect(toggle.mock.calls).toEqual([[true], [false]]);
    const open = followed({ open: true, rows: [aTask("a/one")] });
    const load = vi.spyOn(open, "load").mockResolvedValue();
    fire(drawn(open), reading("Load more followed tasks"), "onClick");
    expect(load).toHaveBeenCalledWith(true);
  });
});
