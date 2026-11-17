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
  MORE,
  PAGE,
  ROW,
  recallShape,
  sameRow,
  searched,
  TaskBoard,
  type TaskBoardProps,
  TaskColumn,
  UnreadBoard,
  windowChosen,
  windowOf,
} from "./board";
import { STAGE_KEY, STAGES } from "./boardwait";
import { aMachine, aTask, aWork, memoryStorage } from "./testing";
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

describe("the approved B board", () => {
  it("shows every lifecycle stage, a Backlog rail and one machine rail, with no Blocked or signoff column", () => {
    const html = board({ signoff: ["a"], statuses: ["blocked", "ready"] });
    expect(columns(html)).toEqual([
      "concept",
      "design",
      "ready",
      "in_progress",
      "build-test",
      "review",
      "done",
    ]);
    expect(html).toContain('class="backlog-rail"');
    expect(html).toContain("No machine reports this project");
    expect(html).not.toContain('class="board-scroll"');
    expect(html).not.toContain("Waiting for your sign-off");
    expect(html).not.toContain("Task groups");
    expect(html).not.toContain('aria-label="Status"');
    expect(html).toContain('aria-label="Finished tasks"');
    expect(html).toContain('type="search"');
  });
  it("exposes the reporting machine stages from the rail", () => {
    const html = board({
      work: aWork([aMachine("desk", [])]),
      counts: [
        ["build", 2],
        ["test", 1],
      ],
    });
    expect(html).toContain(">Build 2</button>");
    expect(html).toContain(">Test 1</button>");
    expect(html).not.toContain("No machine reports");
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
    expect(recallShape("invalid")).toEqual(STAGES.map((s) => [s, 1]));
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
