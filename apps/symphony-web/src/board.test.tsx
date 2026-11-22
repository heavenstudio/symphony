import { renderToStaticMarkup as render } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  BoardRow,
  boardShape,
  byColumn,
  firstPageHeight,
  GhostBoard,
  GhostCard,
  keepShape,
  laneSide,
  MORE,
  PAGE,
  ROW,
  recallGhost,
  recallShape,
  sameRow,
  searched,
  TaskBoard,
  type TaskBoardProps,
  TaskColumn,
  UnderWay,
  UnreadBoard,
  windowChosen,
  windowOf,
} from "./board";
import { STAGE_KEY, STAGES } from "./boardwait";
import { aMachine, anOperation, aTask, aWork, memoryStorage } from "./testing";
import { NO_WORK } from "./work";

beforeEach(() => {
  vi.stubGlobal("innerWidth", 1440);
  vi.stubGlobal("localStorage", memoryStorage());
});
afterEach(() => vi.unstubAllGlobals());
const board = (props: Partial<TaskBoardProps> = {}) =>
  render(
    <TaskBoard
      scope=""
      tasks={[]}
      work={NO_WORK}
      connected
      signoff={[]}
      counts={[]}
      statuses={[]}
      refreshed={0}
      onFilter={() => {}}
      onLoad={() => {}}
      {...props}
    />,
  );
const columns = (html: string) => [...html.matchAll(/data-status="([^"]*)"/g)].map((m) => m[1]);
const rows = (html: string) => [...html.matchAll(/id="row-([^"]*)"/g)].map((m) => m[1]);

describe("a board with no task", () => {
  const empty: [string, number][] = [
    ["ready", 0],
    ["done", 0],
  ];
  it("says in one line how the first is filed, with the guide's link", () => {
    const html = board({ counts: empty });
    expect(html).toContain(
      '<p class="first-steps">No task is open on this board, and none finished today. Add one with the + button, or have an agent file it with the create-task skill or <code>just tasks create</code>: <a href="',
    );
    expect(html).toContain('/docs/operations/GETTING_STARTED.md" target="_blank"');
    expect(html.match(/first-steps/g)).toHaveLength(1);
  });
  it("says it of a new board, which counts no stage at all", () => {
    expect(board({ counts: [] }).match(/first-steps/g)).toHaveLength(1);
  });
  it("says nothing of it once a task is there, open or finished", () => {
    for (const counts of [
      [["ready", 1]],
      [
        ["ready", 0],
        ["done", 2],
      ],
    ] as [string, number][][]) {
      expect(board({ counts })).not.toContain("first-steps");
    }
  });
});

describe("the approved B board", () => {
  it("shows lifecycle and machine columns, with Backlog in the toolbar and no Blocked or signoff column", () => {
    const html = board({ signoff: ["a"], statuses: ["blocked", "ready"] });
    expect(columns(html)).toEqual([
      "concept",
      "design",
      "ready",
      "in_progress",
      "build",
      "test",
      "review",
      "done",
    ]);
    expect(html).toContain("board-backlog");
    expect(html).toContain("No build observations reported.");
    expect(html).toContain("No test observations reported.");
    expect(html).toContain('aria-expanded="false" aria-controls="board-filters"');
    expect(html).not.toContain('class="board-scroll"');
    expect(html).not.toContain("Waiting for your sign-off");
    expect(html).not.toContain("Task groups");
    expect(html).not.toContain('aria-label="Status"');
    expect(html).toContain('aria-label="Finished tasks"');
    expect(html).toContain('type="search"');
  });
  it("draws complete machine counts and cards alongside lifecycle columns", () => {
    vi.stubGlobal("location", { hash: "#column-build" });
    const html = board({
      tasks: [aTask("build/a", { status: "in_progress" })],
      work: aWork([
        aMachine("desk", [anOperation("desk", "a", "build", "running", { task: "build/a" })]),
      ]),
      counts: [
        ["build", 2],
        ["test", 1],
      ],
    });
    expect(html).toContain('build <span class="count">2</span>');
    expect(html).toContain('test <span class="count">1</span>');
    expect(rows(html)).toEqual(["build/a"]);
    expect(columns(html)).toEqual(STAGES);
    expect(html).not.toContain('data-open-stage="build"');
    expect(html).not.toContain("No build observations");
  });
  it("keeps machine columns on disconnect while cards return to their lifecycle stages", () => {
    vi.stubGlobal("location", { hash: "#column-in_progress" });
    const html = board({ connected: false, tasks: [aTask("a", { status: "in_progress" })] });
    expect(columns(html)).toEqual(STAGES);
    expect(rows(html)).toEqual(["a"]);
    expect(html).toContain("Disconnected · build activity unknown");
    expect(html).toContain("Disconnected · test activity unknown");
  });
  it("distinguishes reported empty machine activity from missing observations", () => {
    const html = board({ work: aWork([aMachine("desk", [])]) });
    expect(columns(html)).toEqual(STAGES);
    expect(html).toContain("No tasks with fresh build activity.");
    expect(html).toContain("No tasks with fresh test activity.");
    expect(html).not.toContain("observations reported.");
  });
  it("keeps blocked tasks in their original column before ordinary work and parked last", () => {
    vi.stubGlobal("location", { hash: "#column-review" });
    const task = (slug: string, kind: string, since: number) =>
      aTask(slug, {
        status: "blocked",
        from: "review",
        waits_on: { kind, who: "named", why: "needed", since },
      });
    const html = board({
      tasks: [
        task("park", "parked", 1),
        aTask("own", { status: "review", updated: 2 }),
        task("check", "check", 1),
        task("decision", "decision", 2),
      ],
      counts: [["review", 4]],
      waiting: {
        kinds: [
          ["decision", 1],
          ["check", 1],
          ["working", 1],
          ["parked", 1],
        ],
        columns: [{ status: "review", ordinary: 1, blocked: 3, oldest: 1 }],
      },
    });
    expect(rows(html)).toEqual(["decision", "check", "own", "park"]);
    expect(html).toContain('review <span class="count">1</span>');
    expect(html).toContain("<b>+3 blocked</b>");
    expect(html).toContain("4 open");
    expect(html).toContain("Your decisions <b>1</b>");
  });
  it("uses complete strip counts, not only the loaded rows", () => {
    const html = board({
      tasks: [aTask("only")],
      waiting: {
        kinds: [
          ["decision", 32],
          ["queued", 67],
        ],
        columns: [],
      },
    });
    expect(html).toContain("99 open");
    expect(html).toContain("Your decisions <b>32</b>");
    expect(html.indexOf("k-decision")).toBeLessThan(html.indexOf("k-nobody"));
    expect(html.indexOf("k-check")).toBeLessThan(html.indexOf("k-task"));
  });
  it("shows Review first on a phone and the saved stage on a return", () => {
    vi.stubGlobal("innerWidth", 390);
    expect(columns(board())).toEqual(["review"]);
    localStorage.setItem(STAGE_KEY, "ready");
    expect(columns(board())).toEqual(["ready"]);
    localStorage.setItem(STAGE_KEY, "unknown");
    expect(columns(board())).toEqual(["review"]);
  });
  // The wait strip and the stage tabs are rows that scroll across on a phone (scrollrow.ts), which
  // finds each row's chosen one by `aria-pressed` and `aria-current`: one of each, whatever the
  // counts, a stage with nothing in it saying its zero whole.
  it("draws a phone's wait strip and stage tabs as scrolling rows, one chosen in each", () => {
    vi.stubGlobal("innerWidth", 390);
    localStorage.setItem(STAGE_KEY, "design");
    const html = board({
      counts: [
        ["design", 12],
        ["review", 19],
        ["concept", 0],
      ],
      waiting: {
        kinds: [["check", 30]],
        columns: [
          { status: "design", ordinary: 0, blocked: 12, oldest: null },
          { status: "review", ordinary: 1, blocked: 18, oldest: null },
        ],
      },
    });
    expect(html).toContain(
      '<nav class="wait-strip scroll-row" aria-label="Filter tasks by what they wait on">',
    );
    expect(html).toContain('<nav class="stage-tabs scroll-row" aria-label="Task stages">');
    expect(html.match(/aria-current="page"/g)).toHaveLength(1);
    expect(html).toContain('aria-current="page">Design <i>0</i><b>+12</b></button>');
    expect(html).toContain(">Review <i>1</i><b>+18</b></button>");
    expect(html).toContain(">Concept <i>0</i></button>");
    expect(html.match(/aria-pressed="true"/g)).toHaveLength(1);
    expect(html).toContain('class="open-count" aria-pressed="true">30 open</button>');
  });
  // A phone opens on Review, with In progress next and the Build and Test tabs past the row's
  // edge: the row over them says how much is under way, a build or a test among it.
  it("says on a phone how many tasks are under way, the builds and tests among them", () => {
    vi.stubGlobal("innerWidth", 390);
    const html = board({
      work: aWork([aMachine("desk", [])]),
      counts: [
        ["in_progress", 3],
        ["build", 9],
        ["test", 14],
        ["review", 2],
      ],
      waiting: {
        kinds: [],
        columns: [{ status: "in_progress", ordinary: 1, blocked: 2, oldest: null }],
      },
    });
    expect(html).toContain('<nav class="under-way" aria-label="Work under way">');
    expect(html).toContain('<span class="under-way-all"><b>24</b> under way</span>');
    expect(html).toContain('data-stage="in_progress"><b>1</b> in progress</button>');
    expect(html).toContain('data-stage="build"><b>9</b> building</button>');
    expect(html).toContain('data-stage="test"><b>14</b> testing</button>');
    expect(html.indexOf("under-way")).toBeLessThan(html.indexOf("wait-strip"));
  });
  it("counts the tasks in progress alone while no machine reports its work", () => {
    vi.stubGlobal("innerWidth", 390);
    const html = board({ counts: [["in_progress", 5]] });
    expect(html).toContain("<b>5</b> under way");
    expect(html).toContain("<b>5</b> in progress");
    expect(html).not.toContain("building");
    expect(html).not.toContain("testing");
    expect(board()).toContain("<b>0</b> under way");
  });
  it("draws the row of work under way on a phone alone, its room kept in the ghost there", () => {
    expect(board({ counts: [["in_progress", 5]] })).not.toContain("under-way");
    expect(render(<GhostBoard shape={[]} />)).not.toContain("under-way");
    vi.stubGlobal("innerWidth", 390);
    expect(render(<GhostBoard shape={[]} />)).toContain('<div class="under-way"></div>');
  });
  it("draws no row of work under way on a board whose listing names a custom workflow", () => {
    vi.stubGlobal("innerWidth", 390);
    const waiting = { kinds: [], columns: [], workflow: 3 };
    const held = board({ counts: [["in_progress", 5]], waiting });
    expect(held).not.toContain("under-way");
    expect(held).toContain("wait-strip");
    // Nor does its ghost, once the board it kept was a custom workflow's.
    const ghost = render(<GhostBoard shape={[]} workflow />);
    expect(ghost).not.toContain("under-way");
    expect(ghost).toContain('<nav class="wait-strip"');
  });
  it("names the tasks no machine has by their stage, never as the strip's Working", () => {
    vi.stubGlobal("innerWidth", 390);
    const html = board({
      counts: [["in_progress", 4]],
      waiting: { kinds: [["working", 7]], columns: [] },
    });
    expect(html).toContain("<b>4</b> in progress");
    expect(html).toContain("Working <b>7</b>");
    // The strip's chip is the one thing the page reads as working.
    expect(html.replace(/<[^>]*>/g, " ").match(/working/gi)).toEqual(["Working"]);
  });
  it("gives each count of the work under way its lane", () => {
    const shown: string[] = [];
    const row = UnderWay({
      inProgress: 1,
      machines: [2, 3],
      onStage: (stage) => shown.push(stage),
    });
    for (const cell of [row.props.children].flat(2)) cell?.props?.onClick?.();
    expect(shown).toEqual(["in_progress", "build", "test"]);
  });
  it("opens the linked row's original stage before a saved phone tab", () => {
    vi.stubGlobal("innerWidth", 390);
    vi.stubGlobal("location", { hash: "#row-work%2Fa" });
    localStorage.setItem(STAGE_KEY, "design");
    expect(
      columns(
        board({
          tasks: [
            aTask("work/a", {
              status: "blocked",
              from: "ready",
              waits_on: { kind: "decision", who: "design-choice", why: "Needed", since: 1 },
            }),
          ],
        }),
      ),
    ).toEqual(["ready"]);
  });
  it("opens a linked row in a hidden desktop rail", () => {
    vi.stubGlobal("location", { hash: "#row-work/a" });
    expect(columns(board({ tasks: [aTask("work/a", { status: "backlog" })] }))).toEqual([
      "backlog",
    ]);
  });
  it("orders ordinary work by age then natural slug and finished tasks newest first", () => {
    vi.stubGlobal("location", { hash: "#column-ready" });
    expect(
      rows(
        board({
          tasks: [
            aTask("a10", { updated: 1 }),
            aTask("a2", { updated: 1 }),
            aTask("new", { updated: 9, priority: 0 }),
          ],
        }),
      ),
    ).toEqual(["a2", "a10", "new"]);
    vi.stubGlobal("location", { hash: "#column-done" });
    expect(
      rows(
        board({
          tasks: [
            aTask("old", { status: "done", updated: 1 }),
            aTask("new", { status: "done", updated: 9 }),
          ],
        }),
      ),
    ).toEqual(["new", "old"]);
  });
});

describe("the stages of the lane scroller", () => {
  const frame = { left: 24, right: 1416 };
  it.each([
    ["a lane inside the frame", { left: 24, right: 304 }, "shown"],
    ["a lane a pixel's rounding past either edge", { left: 23.4, right: 1416.6 }, "shown"],
    ["a lane cut at the right edge", { left: 1176, right: 1456 }, "after"],
    ["a lane beyond the right edge", { left: 2040, right: 2320 }, "after"],
    ["a lane cut at the left edge", { left: -100, right: 180 }, "before"],
    ["a lane beyond the left edge", { left: -840, right: -560 }, "before"],
    ["a lane wider than the frame", { left: 0, right: 2000 }, "before"],
  ] as const)("reads %s", (_, lane, side) => {
    expect(laneSide(frame, lane)).toBe(side);
  });
  it("offers every stage over the lanes, in their order, each lane's heading taking the focus", () => {
    const html = board();
    expect(
      [...html.matchAll(/<button type="button" data-side="shown">([^<]*)</g)].map(
        (found) => found[1],
      ),
    ).toEqual(["Concept", "Design", "Ready", "In progress", "Build", "Test", "Review", "Done"]);
    expect(html.indexOf('class="stage-jump"')).toBeLessThan(html.indexOf("waiting-columns"));
    expect(html).toContain('<h2 tabindex="-1">concept');
  });
  it("offers none over one stage's list or a phone's one stage", () => {
    vi.stubGlobal("location", { hash: "#row-work/a" });
    expect(board({ tasks: [aTask("work/a", { status: "backlog" })] })).not.toContain("stage-jump");
    vi.stubGlobal("location", { hash: "" });
    vi.stubGlobal("innerWidth", 390);
    expect(board()).not.toContain("stage-jump");
  });
  it("keeps the row's room in the ghost, on a desk alone", () => {
    expect(render(<GhostBoard shape={[]} />)).toContain('<div class="ghost-jump"></div>');
    expect(render(<GhostBoard shape={[]} />)).not.toContain("stage-jump");
    vi.stubGlobal("innerWidth", 390);
    expect(render(<GhostBoard shape={[]} />)).not.toContain("ghost-jump");
  });
});

describe("lazy cards and placeholders", () => {
  it("reserves the page and its Load more button without reading all tasks", () => {
    expect(PAGE).toBe(30);
    expect(firstPageHeight(0)).toBe(0);
    expect(firstPageHeight(2)).toBe(2 * ROW);
    expect(firstPageHeight(31)).toBe(30 * ROW + MORE);
    expect(firstPageHeight(1000000)).toBe(30 * ROW + MORE);
    expect(render(<GhostCard />)).toContain(`height:${ROW - 8}px`);
  });
  it("holds a column's first page and only offers more after it answers", () => {
    vi.stubGlobal("location", { hash: "#column-ready" });
    const props = {
      status: "ready",
      work: NO_WORK,
      connected: true,
      count: 31,
      query: "",
      days: 1,
      onLoad: () => {},
      refreshed: 0,
    };
    const unread = render(<TaskColumn {...props} tasks={[]} />);
    expect(unread).toContain(`min-height:${30 * ROW + MORE}px`);
    expect(unread).not.toContain("Load more tasks");
    expect(render(<TaskColumn {...props} tasks={[aTask("a")]} />)).toContain("Load more tasks");
  });
  it("keeps unchanged row objects and column lists", () => {
    const a = aTask("a");
    const b = aTask("b", { status: "review" });
    const before = new Map([
      ["ready", [a]],
      ["review", [b]],
    ]);
    const next = byColumn([a, b], (task) => task.status, before);
    expect(next.get("ready")).toBe(before.get("ready"));
    expect(byColumn([a, aTask("c")], (task) => task.status, before).get("ready")).not.toBe(
      before.get("ready"),
    );
    const props = { task: a, work: NO_WORK, connected: true, now: 1 };
    expect(sameRow(props, { ...props })).toBe(true);
    expect(sameRow(props, { ...props, now: 2 })).toBe(false);
    expect(sameRow(props, { ...props, task: aTask("a") })).toBe(false);
    expect(render(<BoardRow {...props} />)).toContain(`min-height:${ROW}px`);
  });
  it("keeps counts for the default ghost and uses the saved phone stage", () => {
    const shape = boardShape([
      ["review", 4],
      ["ready", 6],
      ["blocked", 3],
    ]);
    expect(shape).toEqual(STAGES.map((s) => [s, s === "review" ? 4 : s === "ready" ? 6 : 0]));
    expect(recallShape(keepShape(shape))).toEqual(shape);
    expect(recallGhost(keepShape(shape))).toEqual({ shape, workflow: false });
    // A custom workflow's board is kept as such, its line read as no stage.
    expect(keepShape(shape, true)).toBe(`${keepShape(shape)}\nworkflow`);
    expect(recallGhost(keepShape(shape, true))).toEqual({ shape, workflow: true });
    expect(recallGhost(null)).toEqual({ shape: recallShape(null), workflow: false });
    expect(recallGhost("workflow")).toEqual({ shape: recallShape(null), workflow: true });
    expect(recallShape("invalid")).toEqual(
      STAGES.map((s) => [s, ["build", "test"].includes(s) ? 0 : 1]),
    );
    vi.stubGlobal("innerWidth", 390);
    localStorage.setItem(STAGE_KEY, "ready");
    const html = render(<GhostBoard shape={shape} />);
    expect([...html.matchAll(/data-for="([^"]*)"/g)].map((m) => m[1])).toEqual(["ready"]);
    expect(html.match(/waiting-card-ghost/g)).toHaveLength(6);
  });
  it("says a failed first read and offers retry", () => {
    const html = render(<UnreadBoard why="offline" retry={() => {}} />);
    expect(html).toContain("The task board could not be read");
    expect(html).toContain("offline");
    expect(html).toContain("Retry");
  });
});

it("keeps search and finished-window requests together", () => {
  const read = vi.fn();
  const filters = { search: "words", days: 1 };
  expect(searched(filters, "next", read)).toEqual({ search: "next", days: 1 });
  expect(read).toHaveBeenLastCalledWith("next", 1);
  expect(windowChosen(filters, "7", read)).toEqual({ search: "words", days: 7 });
  expect(read).toHaveBeenLastCalledWith("words", 7);
  expect(windowOf("bad")).toBe(1);
});

it("labels a cross-stage list with the selected wait kind", () => {
  const html = render(
    <TaskColumn
      status="waiting"
      label="On checks"
      tasks={[]}
      count={32}
      work={NO_WORK}
      connected
      query=""
      days={1}
      onLoad={() => {}}
      refreshed={0}
    />,
  );
  expect(html).toContain('On checks <span class="count">32</span>');
});

it("follows a focused task to its new phone stage without following unrelated moves", () => {
  vi.stubGlobal("innerWidth", 390);
  const focused = { getAttribute: () => "follow", querySelectorAll: () => [] };
  vi.stubGlobal("document", { activeElement: { closest: () => focused } });
  const html = board({ tasks: [aTask("follow", { status: "in_progress" })] });
  expect(columns(html)).toEqual(["in_progress"]);
  vi.stubGlobal("document", { activeElement: null });
  expect(columns(board({ tasks: [aTask("follow", { status: "in_progress" })] }))).toEqual([
    "review",
  ]);
});

it("opens a hidden rail stage for a task holding the focus on a wide board", () => {
  const focused = { getAttribute: () => "follow", querySelectorAll: () => [] };
  vi.stubGlobal("document", { activeElement: { closest: () => focused } });
  const html = board({ tasks: [aTask("follow", { status: "backlog" })] });
  expect(columns(html)).toEqual(["backlog"]);
  expect(html).toContain("Back to stages");
  expect(html).toContain('class="kanban waiting-columns wait-list"');
});
