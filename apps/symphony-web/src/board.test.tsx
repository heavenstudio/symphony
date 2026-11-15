import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  BoardContext,
  BoardRow,
  boardShape,
  byColumn,
  CARDS_MAX,
  type ColumnProps,
  card,
  drawnRows,
  firstPageHeight,
  GAP,
  GhostBoard,
  grownTree,
  type Kept,
  keepCards,
  keepRows,
  keepShape,
  keepsWatching,
  MORE,
  NO_TREE,
  PAGE,
  ROW,
  readTree,
  recallCards,
  recallShape,
  rememberCards,
  rowCard,
  sameRow,
  searched,
  statusLabel,
  TaskBoard,
  type TaskBoardProps,
  TaskColumn,
  type Tree,
  TreeReads,
  titleLines,
  UnreadBoard,
  windowChosen,
  windowOf,
  withMachineStages,
} from "./board";
import { layoutAt, rowHeight } from "./cardheight";
import {
  aMachine,
  anOperation,
  aRole,
  asking,
  aTask,
  aWork,
  memoryStorage,
  reply,
  server,
} from "./testing";
import { UNREACHABLE } from "./trouble";
import { KEEP_MS } from "./viewport";
import type { MachineWork, Task } from "./wire";
import { NO_WORK, phaseLabels } from "./work";

beforeEach(() => {
  vi.stubGlobal("innerWidth", 1280);
  vi.stubGlobal("localStorage", memoryStorage());
});

afterEach(() => vi.unstubAllGlobals());

const LIFECYCLE = [
  "backlog",
  "concept",
  "design",
  "ready",
  "in_progress",
  "review",
  "done",
  "blocked",
  "cancelled",
];

/** A machine that reports, with nothing going. */
const reported = (): MachineWork =>
  aWork([aMachine("alpha", [], { label: "Alpha", freshness: "incomplete", complete: false })], {
    freshness: "incomplete",
  });

const board = (props: Partial<TaskBoardProps>) =>
  renderToStaticMarkup(
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

/** The board's columns, in order, as `data-status` names them. */
const columns = (html: string) => [...html.matchAll(/data-status="([^"]*)"/g)].map((m) => m[1]);
/** The rows a mounted column holds, in order, by their tasks' slugs. */
const columnRows = (html: string) => [...html.matchAll(/id="row-([^"]*)"/g)].map((m) => m[1]);

describe("the board's columns", () => {
  // No machine reports the project's work: Build and Test are one narrow rail, said once, and a
  // server's listing that names them draws no column of either.
  it("are one rail for Build and Test of a project no machine reports for", () => {
    for (const statuses of [LIFECYCLE, [...LIFECYCLE, "build", "test"]]) {
      const html = board({ statuses });
      expect(columns(html)).toEqual([
        "concept",
        "design",
        "ready",
        "in_progress",
        "build-test",
        "review",
        "done",
        "backlog",
        "blocked",
        "cancelled",
      ]);
      expect(html.match(/No machine reports this project/g)).toHaveLength(1);
      expect(html).not.toContain("No tasks with fresh");
      expect(html).not.toContain("Machine context");
      expect(html).toContain("/agents#machine-context");
    }
    expect(board({ statuses: LIFECYCLE, connected: false })).toContain(
      "Disconnected · build and test activity unknown",
    );
  });

  it("bring back Build and Test once a machine reports, even with nothing going", () => {
    const html = board({ statuses: LIFECYCLE, work: reported() });
    expect(columns(html).slice(0, 8)).toEqual([
      "concept",
      "design",
      "ready",
      "in_progress",
      "build",
      "test",
      "review",
      "done",
    ]);
    expect(columns(html)).not.toContain("build-test");
    expect(html.match(/No tasks with fresh/g)).toHaveLength(2);
    expect(html.match(/Machine context/g)).toHaveLength(1);
  });

  it("are the counts' statuses when the listing names none", () => {
    const html = board({ counts: [["ready", 3]] });
    expect(columns(html)).toEqual(["ready", "build-test"]);
    expect(html).toContain('<span class="count">3</span>');
  });

  it("open on the owner's sign-off while a task waits for it, its task in no other column", () => {
    const tasks = [aTask("w/a", { status: "review" }), aTask("w/b", { status: "review" })];
    const draw = () =>
      board({ tasks, signoff: ["w/a"], statuses: ["review"], counts: [["review", 2]] });
    const html = draw();
    expect(columns(html)).toEqual(["signoff", "build-test", "review"]);
    expect(html).toContain('<section class="column signoff" data-status="signoff"');
    expect(html).toContain('Waiting for your sign-off <span class="count">1</span>');
    // Each column's rows, mounted as the address names it.
    vi.stubGlobal("location", { hash: "#column-signoff" });
    expect(columnRows(draw())).toEqual(["w/a"]);
    vi.stubGlobal("location", { hash: "#column-review" });
    expect(columnRows(draw())).toEqual(["w/b"]);
  });

  it("are the pipeline's in a scroller the keyboard reaches, the rest after it", () => {
    const html = board({ statuses: LIFECYCLE });
    expect(html).toContain(
      '<div class="board-scroll" tabindex="0" role="region" aria-label="Task pipeline columns" aria-describedby="board-scroll-hint">',
    );
    expect(html).toContain("Columns continue right →");
    expect(html.indexOf('data-status="done"')).toBeLessThan(html.indexOf('class="offline"'));
    expect(html.indexOf('class="offline"')).toBeLessThan(html.indexOf('data-status="backlog"'));
  });

  it("list each task by priority, then by slug, a subtask only while searching", () => {
    const tasks = [
      aTask("w/c", { priority: 3 }),
      aTask("w/b", { priority: 1 }),
      aTask("w/a", { priority: 3 }),
      aTask("w/a/1", { priority: 0, parent: "w/a" }),
    ];
    vi.stubGlobal("location", { hash: "#column-ready" });
    const html = board({ tasks, statuses: ["ready"], counts: [["ready", 3]] });
    expect(columnRows(html)).toEqual(["w/b", "w/a", "w/c"]);
  });

  it("list finished work newest first, as the server pages it", () => {
    const tasks = [
      aTask("w/c", { status: "done", priority: 0, updated: 1 }),
      aTask("w/b", { status: "done", updated: 3 }),
      aTask("w/a", { status: "done", updated: 2 }),
    ];
    vi.stubGlobal("location", { hash: "#column-done" });
    expect(columnRows(board({ tasks, statuses: ["done"], counts: [["done", 3]] }))).toEqual([
      "w/b",
      "w/a",
      "w/c",
    ]);
  });

  it("offer Load more once a page is in and the column holds more", () => {
    vi.stubGlobal("location", { hash: "#column-ready" });
    const tasks = [aTask("w/a")];
    expect(board({ tasks, statuses: ["ready"], counts: [["ready", 2]] })).toContain(
      "Load more tasks",
    );
    expect(board({ tasks, statuses: ["ready"], counts: [["ready", 1]] })).not.toContain(
      "Load more tasks",
    );
    // While its first page is read, the column holds its ghosts, and nothing more to load.
    const reading = board({ statuses: ["ready"], counts: [["ready", 2]] });
    expect(reading).toContain('<div style="min-height:278px" data-pending="true">');
    expect(reading).not.toContain("Load more tasks");
  });

  it("show the task groups' toggle, closed", () => {
    const html = board({});
    expect(html).toContain('<h2 id="task-groups-title">Task groups</h2>');
    expect(html).toContain('aria-expanded="false" aria-controls="task-groups"');
    expect(html).toContain("Show task groups");
    expect(html).not.toContain('id="task-groups"');
  });

  it("are filtered by status and by the window of finished tasks", () => {
    const html = board({ statuses: LIFECYCLE });
    expect(html).toContain(">Search tasks and messages <input");
    expect(html).toContain('placeholder="Slug or words"');
    expect(html).toContain('aria-label="Status"');
    expect(html).toContain("<span>All statuses</span>");
    expect(html).toContain('aria-label="Finished tasks"');
    expect(html).toContain("<span>Last 24 hours</span>");
  });
});

describe("the Status filter", () => {
  it("names each status as a sentence starts", () => {
    for (const [status, label] of [
      ["backlog", "Backlog"],
      ["in_progress", "In progress"],
      ["build", "Build"],
      ["a_b_c", "A b c"],
      ["_x", " x"],
      ["élan", "Élan"],
      ["", ""],
    ]) {
      expect(statusLabel(status as string), status).toBe(label);
    }
  });

  // Build and Test go where the board draws them, between In progress and Review, whether the
  // server named them (anywhere, even twice) or not; with no In progress they go last.
  it("lists Build and Test after In progress", () => {
    const all = [
      "backlog",
      "concept",
      "design",
      "ready",
      "in_progress",
      "build",
      "test",
      "review",
      "done",
      "blocked",
      "cancelled",
    ];
    for (const given of [
      LIFECYCLE,
      [...LIFECYCLE, "build", "test"],
      ["test", "build", ...LIFECYCLE, "build"],
    ]) {
      expect(withMachineStages(given)).toEqual(all);
    }
    expect(withMachineStages(["backlog", "done"])).toEqual(["backlog", "done", "build", "test"]);
    expect(withMachineStages([])).toEqual(["build", "test"]);
  });
});

describe("a column's reserve", () => {
  // A column reserves a card for each row of the page its first read brings, and the Load more
  // button only when its tasks are more than that page; with nothing kept, each a typical card.
  it("is a card per row of its page, and Load more past it", () => {
    expect(PAGE).toBe(30);
    expect(firstPageHeight(0, [])).toBe(0);
    expect(firstPageHeight(1, [])).toBe(ROW);
    expect(firstPageHeight(17, [])).toBe(17 * ROW);
    expect(firstPageHeight(30, [])).toBe(30 * ROW);
    expect(firstPageHeight(31, [])).toBe(30 * ROW + MORE);
    expect(firstPageHeight(Number.MAX_SAFE_INTEGER, [])).toBe(30 * ROW + MORE);
  });

  it("is each card it kept at its height, and the guess past them", () => {
    const kept: Kept[] = [
      [134, 1],
      [113, 0],
      [192, 3],
    ];
    expect(card(kept, 1, "ready")).toEqual([113, 0]);
    // A link's row and a role's.
    expect(card(kept, 3, "review")).toEqual([ROW, 2]);
    expect(card([], 0, "in_progress")).toEqual([ROW, 2]);
    // As tall, with no role chip where no agent has taken the tasks up.
    for (const unstaffed of ["concept", "design", "ready", "backlog"]) {
      expect(card(kept, 3, unstaffed)).toEqual([ROW, 1]);
    }
    expect(firstPageHeight(2, kept)).toBe(134 + 113);
    expect(firstPageHeight(5, kept)).toBe(134 + 113 + 192 + 2 * ROW);
    expect(firstPageHeight(31, kept)).toBe(134 + 113 + 192 + 27 * ROW + MORE);
  });

  // A row waiting to be drawn holds the height its own card will draw at, chips among it: a row of
  // them for its links, one for its roles and one for its build and test phases.
  it("holds each waiting row at its own card's height", () => {
    const bare = aTask("load/ready-1", { title: "Two words" });
    const linked = { ...bare, links: ["docs/TASKS.md"] };
    const staffed = { ...bare, roles: [aRole("executor", "opal-plover")] };
    const both = { ...linked, roles: staffed.roles };
    const building = aWork([
      aMachine("alpha", [anOperation("alpha", "b", "build", "running", { task: "load/ready-1" })]),
    ]);
    const phases = phaseLabels(building, "load/ready-1", true);
    expect(phases).toHaveLength(1);
    for (const layout of ["desk", "phone"] as const) {
      expect(rowCard(both, building, true, layout)).toEqual([rowHeight(both, phases, layout), 3]);
      expect(rowCard(bare, NO_WORK, true, layout)).toEqual([rowHeight(bare, [], layout), 0]);
      expect(rowCard(linked, NO_WORK, true, layout)).toEqual([rowHeight(linked, [], layout), 1]);
      expect(rowCard(staffed, NO_WORK, true, layout)).toEqual([rowHeight(staffed, [], layout), 1]);
      expect(rowCard(both, NO_WORK, true, layout)).toEqual([rowHeight(both, [], layout), 2]);
    }
    expect(rowCard(staffed, NO_WORK, true, "desk")[0]).toBeGreaterThan(
      rowCard(bare, NO_WORK, true, "desk")[0],
    );
  });
});

describe("a card's ghost", () => {
  // As the board draws them: 18 px of border and padding, a 20.3 px slug line, 2 px, the title's
  // lines of 20.3 px, the chip rows' 12 px of margins, 21 px for a row of roles or of phases and
  // 17 px for one of links, and 8 px under the card.
  it("draws the lines of title its box has room for", () => {
    const drawn = (lines: number, rows: number[]): Kept => {
      const height = 18 + 20.3 + 2 + lines * 20.3 + 12 + rows.reduce((sum, row) => sum + row, 0);
      return [Math.round(height) + 8, rows.length];
    };
    for (let lines = 1; lines <= 5; lines += 1) {
      for (const rows of [[], [21], [17], [17, 21], [21, 17, 21]]) {
        expect(titleLines(drawn(lines, rows)), `${lines} lines over ${rows}`).toBe(lines);
      }
    }
    expect(titleLines([ROW, 2])).toBe(2);
    expect(titleLines([20, 1])).toBe(1);
    expect(titleLines([20, 255])).toBe(1);
    expect(titleLines([900, 0])).toBe(5);
  });
});

describe("a column's kept cards", () => {
  // A browser keeps the cards of each column at each width, newest first, its own line again when
  // drawn again, and 32 lines at most.
  it("are those drawn last in a window as wide", () => {
    let kept = keepCards("", "ready", 1280, [
      [134, 1],
      [113, 0],
      [160, 2],
    ]);
    expect(kept).toBe("1280 ready 134c 113 160cc");
    kept = keepCards(kept, "ready", 390, [[160, 1]]);
    kept = keepCards(kept, "ready_x", 1280, [[101, 0]]);
    expect(recallCards(kept, "ready", 1280)).toEqual([
      [134, 1],
      [113, 0],
      [160, 2],
    ]);
    expect(recallCards(kept, "ready", 390)).toEqual([[160, 1]]);
    expect(recallCards(kept, "ready", 800)).toEqual([]);
    expect(recallCards(kept, "review", 1280)).toEqual([]);
    // Drawn again: one line, first, with the new cards; a column of none keeps none.
    kept = keepCards(kept, "ready", 1280, []);
    expect(kept).toBe("1280 ready\n1280 ready_x 101\n390 ready 160c");
    expect(recallCards(kept, "ready", 1280)).toEqual([]);
    // A pitch it cannot read ends the column's cards there.
    expect(recallCards("1280 ready 134c tall 113", "ready", 1280)).toEqual([[134, 1]]);
    expect(recallCards("1280 ready 134c c 113", "ready", 1280)).toEqual([[134, 1]]);
    expect(recallCards("1280", "ready", 1280)).toEqual([]);
    const many = Array.from({ length: 40 }, (_, n) => n).reduce(
      (kept, n) => keepCards(kept, `s${n}`, 1280, [[n, 0]]),
      "",
    );
    expect(many.split("\n")).toHaveLength(CARDS_MAX);
    expect(recallCards(many, "s39", 1280)).toEqual([[39, 0]]);
    // The oldest went.
    expect(recallCards(many, "s7", 1280)).toEqual([]);
  });

  // A column whose rows draw their cards one after another keeps each row as it stands, a row
  // still waiting at the place it holds: again whenever one changed, never what it kept already,
  // and nothing for a column of no rows (so a line kept before stays).
  it("are kept again as rows that draw late change", () => {
    const a: Kept = [98, 1];
    const held: Kept = [139, 2];
    const c: Kept = [141, 0];
    expect(keepRows([a, held, c], [])).toEqual([a, held, c]);
    expect(keepRows([a, held, c], [a, held, c])).toBeNull();
    const b: Kept = [121, 1];
    expect(keepRows([a, b, c], [a, held, c])).toEqual([a, b, c]);
    expect(keepRows([a, [121, 2], c], [a, b, c])).toEqual([a, [121, 2], c]);
    expect(keepRows([a, b, [162, 0]], [a, b, c])).toEqual([a, b, [162, 0]]);
    expect(keepRows([], [a])).toBeNull();
  });

  // A column's keep watches its rows draw for as long as one still waits near the screen, and
  // stops once every row has drawn, once none waiting is within the sentinel's reach (a row past
  // it draws only on a scroll), or at KEEP_MS, the bound on its work.
  it("are watched until what is near the screen has drawn", () => {
    expect(keepsWatching(false, true, 100)).toBe(true);
    expect(keepsWatching(false, true, KEEP_MS - 100)).toBe(true);
    expect(keepsWatching(false, true, KEEP_MS)).toBe(false);
    expect(keepsWatching(true, true, 100)).toBe(false);
    expect(keepsWatching(true, false, 100)).toBe(false);
    expect(keepsWatching(false, false, 100)).toBe(false);
  });
});

describe("a column", () => {
  const column = (props: Partial<ColumnProps>) =>
    renderToStaticMarkup(
      <TaskColumn
        status="ready"
        work={NO_WORK}
        connected={false}
        tasks={[]}
        count={0}
        query=""
        days={1}
        onLoad={() => {}}
        refreshed={0}
        {...props}
      />,
    );
  const guessed = `class="ghost-card" style="height:${ROW - GAP}px"`;

  // A column not yet near the screen draws a card's box for each row of its first page, in the
  // height that page will take, and says it is busy.
  it("not yet near the screen draws a card's box per task of its first page", () => {
    let html = column({ count: 40 });
    expect(html.split(guessed)).toHaveLength(PAGE + 1);
    // A typical card's row of links, and no role chip in a column no agent has taken up.
    expect(html.match(/ghost-pill/g)).toHaveLength(PAGE);
    expect(html).toContain(`min-height:${30 * ROW + MORE}px`);
    expect(html).toContain('aria-busy="true"');
    // The board says once that it loads.
    expect(html).not.toContain('role="status"');
    html = column({ status: "review", count: 2 });
    expect(html.split(guessed)).toHaveLength(3);
    expect(html.match(/ghost-pill/g)).toHaveLength(2 * 2);
    html = column({ status: "review" });
    expect(html).toContain('aria-busy="false"');
    expect(html).not.toContain("ghost-card");
  });

  it("draws the cards this browser kept for it in a window as wide", () => {
    localStorage.setItem("board-cards", "1280 ready 120c 99");
    const html = column({ count: 3 });
    expect(html).toContain(`class="ghost-card" style="height:${120 - GAP}px"`);
    expect(html).toContain(`class="ghost-card" style="height:${99 - GAP}px"`);
    expect(html.split(guessed)).toHaveLength(2);
    expect(html).toContain(`min-height:${120 + 99 + ROW}px`);
  });

  it("says Build's machine context, and its emptiness", () => {
    const html = column({ status: "build" });
    expect(html).toContain('<h2>build <span class="count">0</span></h2>');
    expect(html).toContain("Machine context");
    expect(html).toContain("No tasks with fresh build activity.");
    expect(column({ status: "test", count: 1 })).not.toContain("No tasks with fresh");
  });

  it("names its status in words", () => {
    expect(column({ status: "in_progress", count: 4 })).toContain(
      '<h2>in progress <span class="count">4</span></h2>',
    );
  });
});

describe("a board drawn again", () => {
  const a = aTask("w/a");
  const b = aTask("w/b");
  const c = aTask("w/c", { status: "review" });
  const place = (task: Task) => task.status;

  it("places each task in its column, in their order", () => {
    const columns = byColumn([a, c, b], place, new Map());
    expect([...columns.keys()]).toEqual(["ready", "review"]);
    expect(columns.get("ready")).toEqual([a, b]);
    expect(columns.get("review")).toEqual([c]);
  });

  it("keeps the list of a column whose tasks are the same objects, and only of one", () => {
    const before = byColumn([a, b, c], place, new Map());
    const again = byColumn([a, b, c], place, before);
    expect(again.get("ready")).toBe(before.get("ready"));
    expect(again.get("review")).toBe(before.get("review"));
    // One task changed: its column's list is new, the other column's kept.
    const renamed = { ...b, title: "Renamed" };
    const changed = byColumn([a, renamed, c], place, before);
    expect(changed.get("ready")).not.toBe(before.get("ready"));
    expect(changed.get("ready")).toEqual([a, renamed]);
    expect(changed.get("review")).toBe(before.get("review"));
    // A task gone, or the same tasks in another order, is a new list too.
    expect(byColumn([a, c], place, before).get("ready")).toEqual([a]);
    expect(byColumn([b, a, c], place, before).get("ready")).not.toBe(before.get("ready"));
    expect(byColumn([], place, before).size).toBe(0);
  });

  it("draws a row again only when its card or its layout would change", () => {
    const layout = layoutAt(1280);
    const row = { task: a, work: NO_WORK, connected: true, layout };
    expect(sameRow(row, { ...row })).toBe(true);
    expect(sameRow(row, { ...row, layout: layoutAt(400) })).toBe(false);
    expect(sameRow(row, { ...row, task: { ...a } })).toBe(false);
    expect(sameRow(row, { ...row, connected: false })).toBe(false);
    // Another machine's work that leaves this card's phases as they were draws nothing again.
    const elsewhere = aWork([
      aMachine("alpha", [anOperation("alpha", "b", "build", "running", { task: "w/z" })]),
    ]);
    expect(sameRow(row, { ...row, work: elsewhere })).toBe(true);
    const here = aWork([
      aMachine("alpha", [anOperation("alpha", "b", "build", "running", { task: "w/a" })]),
    ]);
    expect(sameRow(row, { ...row, work: here })).toBe(false);
  });

  it("is memoized: a row by `sameRow`, a column by what it is given", () => {
    const memo = Symbol.for("react.memo");
    const { $$typeof, compare } = BoardRow as unknown as { $$typeof: symbol; compare: unknown };
    expect([$$typeof, compare]).toEqual([memo, sameRow]);
    expect((TaskColumn as unknown as { $$typeof: symbol }).$$typeof).toBe(memo);
    expect(
      renderToStaticMarkup(
        <BoardRow {...{ task: a, work: NO_WORK, connected: true, layout: layoutAt(1280) }} />,
      ),
    ).toContain('id="row-w/a"');
  });
});

describe("a card a live update moves to another column", () => {
  const a = aTask("w/a");
  const row = () =>
    renderToStaticMarkup(
      <BoardRow {...{ task: a, work: NO_WORK, connected: true, layout: layoutAt(1280) }} />,
    );
  /** The focus on a link of the board's card of `slug`, in its old row, still on the page. */
  const focusOn = (slug: string) => {
    const card = { getAttribute: () => slug, querySelectorAll: () => [] };
    vi.stubGlobal("document", {
      activeElement: {
        closest: (selector: string) => (selector.endsWith("[data-slug]") ? card : null),
      },
    });
  };

  it("draws its card in its new row at once while the focus is in it, a ghost otherwise", () => {
    expect(row()).toContain('class="viewport-placeholder"');
    focusOn("w/b");
    expect(row()).toContain('class="viewport-placeholder"');
    focusOn("w/a");
    const html = row();
    expect(html).not.toContain("viewport-placeholder");
    expect(html).toContain('<div class="taskcard p2" data-slug="w/a">');
  });

  it("draws its new column's rows at once, that column not yet near the screen", () => {
    const column = () =>
      renderToStaticMarkup(
        <TaskColumn
          status="in_progress"
          work={NO_WORK}
          connected
          tasks={[aTask("w/b", { status: "in_progress" }), aTask("w/a", { status: "in_progress" })]}
          count={2}
          query=""
          days={1}
          onLoad={() => {}}
          refreshed={0}
        />,
      );
    expect(columnRows(column())).toEqual([]);
    focusOn("w/c");
    expect(columnRows(column())).toEqual([]);
    focusOn("w/a");
    const html = column();
    expect(columnRows(html)).toEqual(["w/b", "w/a"]);
    // Only the focused card's row draws its card; the other is its ghost until it is near.
    expect(html.match(/class="taskcard /g)).toHaveLength(1);
  });
});

describe("the Finished tasks field", () => {
  it("names a week by its 7, and the last 24 hours by anything else", () => {
    expect(windowOf("7")).toBe(7);
    expect(windowOf("1")).toBe(1);
    expect(windowOf("")).toBe(1);
  });

  it("reads the board in the window chosen, under the search typed, the same window again too", () => {
    const onFilter = vi.fn();
    let filters = { search: "", days: 1 };
    filters = windowChosen(filters, "7", onFilter);
    expect(filters).toEqual({ search: "", days: 7 });
    expect(onFilter).toHaveBeenLastCalledWith("", 7);
    filters = searched(filters, "slow", onFilter);
    filters = windowChosen(filters, "7", onFilter);
    expect(filters).toEqual({ search: "slow", days: 7 });
    expect(onFilter).toHaveBeenLastCalledWith("slow", 7);
    filters = windowChosen(filters, "1", onFilter);
    expect(filters).toEqual({ search: "slow", days: 1 });
    expect(onFilter).toHaveBeenLastCalledWith("slow", 1);
    expect(onFilter).toHaveBeenCalledTimes(4);
  });
});

describe("the search", () => {
  it("reads the board under each key's search, in the window chosen", () => {
    const onFilter = vi.fn();
    let filters = { search: "", days: 7 };
    filters = searched(filters, "s", onFilter);
    expect(filters).toEqual({ search: "s", days: 7 });
    expect(onFilter).toHaveBeenLastCalledWith("s", 7);
    filters = searched(filters, "sl", onFilter);
    expect(onFilter).toHaveBeenLastCalledWith("sl", 7);
    // Cleared, the board is read with no search: a search A, then B, then A again, is three.
    filters = searched(filters, "", onFilter);
    expect(filters).toEqual({ search: "", days: 7 });
    expect(onFilter.mock.calls).toEqual([
      ["s", 7],
      ["sl", 7],
      ["", 7],
    ]);
  });

  it("leaves the filters it was given as they were", () => {
    const filters = { search: "a", days: 1 };
    searched(filters, "b", () => {});
    windowChosen(filters, "7", () => {});
    expect(filters).toEqual({ search: "a", days: 1 });
  });
});

/** An element of the page: its box's height, what it holds by selector, and whether it holds any. */
function element(height: number, found: Record<string, unknown[]> = {}, holds = true) {
  return {
    getBoundingClientRect: () => ({ top: 0, bottom: height, left: 0, right: 100, height }),
    querySelector: (selector: string) => found[selector]?.[0] ?? null,
    querySelectorAll: (selector: string) => found[selector] ?? [],
    firstElementChild: holds ? {} : null,
    closest: () => null,
  } as unknown as Element;
}

/** A row whose card drew `height` px tall with `chips` rows of chips (and an empty one). */
const drawnRow = (height: number, chips: number) =>
  element(height + 9, {
    ".taskcard": [
      element(height, {
        ".chips": [...Array.from({ length: chips }, () => element(20)), element(0, {}, false)],
      }),
    ],
  });
/** A row still waiting, holding `height` px with `pills` of its ghost's chips. */
const waitingRow = (height: number, pills: number) =>
  element(height, { ".ghost-pill": Array.from({ length: pills }, () => element(10)) });

describe("a column's drawn rows", () => {
  it("are each drawn card's pitch and rows of chips, or the place a waiting row holds", () => {
    const column = element(400, {
      '[id^="row-"]': [drawnRow(130.4, 2), waitingRow(139, 1), drawnRow(98, 0)],
    });
    expect(drawnRows(column)).toEqual([
      [
        [130 + GAP, 2],
        [139, 1],
        [98 + GAP, 0],
      ],
      false,
    ]);
    expect(drawnRows(element(400, { '[id^="row-"]': [drawnRow(98, 1)] }))).toEqual([
      [[98 + GAP, 1]],
      true,
    ]);
  });

  it("are a page's at most", () => {
    const rows = Array.from({ length: PAGE + 5 }, () => drawnRow(90, 1));
    expect(drawnRows(element(400, { '[id^="row-"]': rows }))[0]).toHaveLength(PAGE);
  });
});

describe("a column's keep for the next visit", () => {
  beforeEach(() => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    vi.stubGlobal("innerHeight", 800);
  });
  afterEach(() => vi.useRealTimers());

  /** A page whose `.kanban` column `ready` is as `column()` gives it each time it is looked up. */
  const page = (column: () => Element | null) =>
    vi.stubGlobal("document", {
      getElementById: () => null,
      querySelector: (selector: string) =>
        selector === '.kanban .column[data-status="ready"]' ? column() : null,
    });

  it("keeps its rows as they draw, until every one has", async () => {
    let rows = [drawnRow(98, 1), waitingRow(139, 2)];
    const waiting = element(139, {});
    page(() =>
      element(400, {
        '[id^="row-"]': rows,
        "[data-pending=true]": rows.length === 2 && rows[1] !== undefined ? [waiting] : [],
      }),
    );
    const keep = rememberCards("ready", new AbortController().signal);
    await vi.advanceTimersByTimeAsync(100);
    expect(localStorage.getItem("board-cards")).toBe("1280 ready 106c 139cc");
    rows = [drawnRow(98, 1), drawnRow(120, 2)];
    await vi.advanceTimersByTimeAsync(100);
    await keep;
    expect(localStorage.getItem("board-cards")).toBe("1280 ready 106c 128cc");
  });

  it("stops once no waiting row is near the screen, and keeps nothing it kept already", async () => {
    const set = vi.spyOn(localStorage, "setItem");
    page(() => element(400, { '[id^="row-"]': [waitingRow(139, 2)] }));
    await Promise.all([
      rememberCards("ready", new AbortController().signal),
      vi.advanceTimersByTimeAsync(100),
    ]);
    expect(set).toHaveBeenCalledTimes(1);
  });

  it("stops once the column is gone, or the page is", async () => {
    page(() => null);
    const gone = rememberCards("ready", new AbortController().signal);
    await vi.advanceTimersByTimeAsync(100);
    await gone;
    const leaving = new AbortController();
    page(() => element(400, { '[id^="row-"]': [drawnRow(98, 1)] }));
    const left = rememberCards("ready", leaving.signal);
    leaving.abort();
    await left;
    expect(localStorage.getItem("board-cards")).toBeNull();
  });
});

describe("the task groups", () => {
  it("read a page of the roots, or of a task's subtasks from where the last left", async () => {
    const asked = server();
    const stays = new AbortController().signal;
    const roots = readTree("", null, null, stays);
    await asking(asked, 1);
    expect(asked[0]?.url).toBe("/api/task-board?roots=1&older=1");
    asked[0]?.answer(reply(200, { tasks: [aTask("w")], counts: [["ready", 2]], next: "w" }));
    expect(await roots).toMatchObject({ tasks: [aTask("w")], next: "w" });
    const children = readTree("wt", "w/a b", "w/a 1", stays);
    await asking(asked, 2);
    expect(asked[1]?.url).toBe("/@wt/api/task-board?parent=w%2Fa%20b&older=1&cursor=w%2Fa%201");
    asked[1]?.answer(reply(200, { tasks: "none" }));
    expect(await children).toBe("The task board's answer is not a listing");
    const failed = readTree("", null, null, stays);
    await asking(asked, 3);
    asked[2]?.answer(reply(503, { error: "down" }));
    expect(await failed).toContain("down");
  });

  it("give a read up on the wire once the part that asked is gone", async () => {
    const asked = server();
    const part = new AbortController();
    const read = readTree("", null, null, part.signal);
    await asking(asked, 1);
    expect(asked[0]?.signal?.aborted).toBe(false);
    part.abort();
    expect(asked[0]?.signal?.aborted).toBe(true);
    asked[0]?.answer(reply(200, { tasks: [], counts: [] }));
    await read;
  });

  it("grow by each page read, each task once, and say why a read failed", () => {
    const page = (slugs: string[], total: number) =>
      ({ tasks: slugs.map((slug) => aTask(slug)), counts: [["ready", total]] }) as never;
    let tree = grownTree({ rows: [], total: 0, trouble: "x" }, page(["a", "b"], 3), false);
    expect(tree).toEqual({ rows: [aTask("a"), aTask("b")], total: 3, trouble: "" });
    tree = grownTree(tree, page(["b", "c"], 3), true);
    expect(tree.rows.map((task) => task.slug)).toEqual(["a", "b", "c"]);
    tree = grownTree(tree, "The server could not be reached", true);
    expect(tree.rows).toHaveLength(3);
    expect(tree.trouble).toBe("The server could not be reached");
    expect(grownTree(tree, page(["d"], 1), false).rows.map((task) => task.slug)).toEqual(["d"]);
  });

  it("open on their toggle, closed, and draw nothing of their tree until opened", () => {
    expect(board({})).not.toContain("Load more task groups");
  });

  /** A part's reads, its mount and its own signal, and what it shows after each change. */
  function part(parent: string | null = null) {
    const mount = new AbortController();
    const own = new AbortController();
    const shown: Tree[] = [];
    let tree = NO_TREE;
    const reads = new TreeReads("", parent, mount.signal, (change) => {
      tree = change(tree);
      shown.push(tree);
    });
    return { reads, mount, own, shown };
  }
  const listing = (slugs: string[], total: number, next: string | null = null) => ({
    tasks: slugs.map((slug) => aTask(slug)),
    counts: [["ready", total]],
    next,
  });

  it("read nothing before their part is drawn", async () => {
    const asked = server();
    const { reads, shown } = part();
    await reads.load(false);
    expect(asked).toHaveLength(0);
    expect(shown).toHaveLength(0);
  });

  it("read the first page, then each next one from where the last left, each task once", async () => {
    const asked = server();
    const { reads, own, shown } = part("w/a");
    reads.open(own.signal);
    const first = reads.load(false);
    await asking(asked, 1);
    expect(asked[0]?.url).toBe("/api/task-board?parent=w%2Fa&older=1");
    expect(asked[0]?.signal?.aborted).toBe(false);
    asked[0]?.answer(reply(200, listing(["w/a/1", "w/a/2"], 3, "w/a/2")));
    await first;
    expect(shown.at(-1)).toEqual({ rows: [aTask("w/a/1"), aTask("w/a/2")], total: 3, trouble: "" });
    const more = reads.load(true);
    await asking(asked, 2);
    expect(asked[1]?.url).toBe("/api/task-board?parent=w%2Fa&older=1&cursor=w%2Fa%2F2");
    asked[1]?.answer(reply(200, listing(["w/a/2", "w/a/3"], 3)));
    await more;
    expect(shown.at(-1)?.rows.map((task) => task.slug)).toEqual(["w/a/1", "w/a/2", "w/a/3"]);
    // The last page named none after it: the next read starts again from the top.
    const again = reads.load(true);
    await asking(asked, 3);
    expect(asked[2]?.url).toBe("/api/task-board?parent=w%2Fa&older=1");
    asked[2]?.answer(reply(200, listing(["w/a/1"], 1)));
    await again;
    // The first page again replaces what was shown.
    const top = reads.load(false);
    await asking(asked, 4);
    expect(asked[3]?.url).toBe("/api/task-board?parent=w%2Fa&older=1");
    asked[3]?.answer(reply(200, listing(["w/a/9"], 1)));
    await top;
    expect(shown.at(-1)).toEqual({ rows: [aTask("w/a/9")], total: 1, trouble: "" });
  });

  it("keep what they showed when a read fails, saying why, and keep its place for the next", async () => {
    const asked = server();
    const { reads, own, shown } = part();
    reads.open(own.signal);
    const first = reads.load(false);
    await asking(asked, 1);
    expect(asked[0]?.url).toBe("/api/task-board?roots=1&older=1");
    asked[0]?.answer(reply(200, listing(["r/a"], 2, "r/a")));
    await first;
    const failed = reads.load(true);
    await asking(asked, 2);
    asked[1]?.answer(reply(503, { error: "the board is down" }));
    await failed;
    expect(shown.at(-1)?.rows).toEqual([aTask("r/a")]);
    expect(shown.at(-1)?.trouble).toContain("the board is down");
    // A retry of the next page asks from where the last page left.
    const retry = reads.load(true);
    await asking(asked, 3);
    expect(asked[2]?.url).toBe("/api/task-board?roots=1&older=1&cursor=r%2Fa");
    asked[2]?.answer(reply(200, listing(["r/b"], 2)));
    await retry;
    expect(shown.at(-1)).toEqual({ rows: [aTask("r/a"), aTask("r/b")], total: 2, trouble: "" });
  });

  it("drop an answer that comes once their part is gone, or their kind is stopped", async () => {
    const asked = server();
    const gone = part();
    gone.reads.open(gone.own.signal);
    const late = gone.reads.load(false);
    await asking(asked, 1);
    gone.own.abort();
    expect(asked[0]?.signal?.aborted).toBe(true);
    asked[0]?.answer(reply(200, listing(["r/a"], 1)));
    await late;
    expect(gone.shown).toHaveLength(0);
    // Nor does a part that is gone read again.
    await gone.reads.load(false);
    expect(asked).toHaveLength(1);
    const stopped = part();
    stopped.reads.open(stopped.own.signal);
    const after = stopped.reads.load(false);
    await asking(asked, 2);
    stopped.mount.abort();
    asked[1]?.answer(reply(200, listing(["r/a"], 1)));
    await after;
    expect(stopped.shown).toHaveLength(0);
    await stopped.reads.load(true);
    expect(asked).toHaveLength(2);
  });

  it("read under the part's newest signal once it is drawn again", async () => {
    const asked = server();
    const { reads, own, shown } = part();
    reads.open(own.signal);
    own.abort();
    const again = new AbortController();
    reads.open(again.signal);
    const read = reads.load(false);
    await asking(asked, 1);
    expect(asked[0]?.signal?.aborted).toBe(false);
    asked[0]?.answer(reply(200, listing(["r/a"], 1)));
    await read;
    expect(shown).toHaveLength(1);
    // That signal ending gives its read up on the wire.
    const next = reads.load(true);
    await asking(asked, 2);
    again.abort();
    expect(asked[1]?.signal?.aborted).toBe(true);
    asked[1]?.answer(reply(200, listing(["r/b"], 2)));
    await next;
    expect(shown).toHaveLength(1);
  });
});

describe("the board's shape", () => {
  const ghostColumns = (shape: [string, number][]) =>
    [...renderToStaticMarkup(<GhostBoard shape={shape} />).matchAll(/data-for="([^"]*)"/g)].map(
      (m) => m[1],
    );

  // The board's shape names the columns the board draws, in its order, whatever the server lists
  // and whether a machine reports.
  it("is the columns the board draws, in their order", () => {
    for (const [work, statuses] of [
      [NO_WORK, LIFECYCLE],
      [NO_WORK, [...LIFECYCLE, "build", "test"]],
      [reported(), LIFECYCLE],
      [reported(), ["ready", "backlog"]],
    ] as [MachineWork, string[]][]) {
      const shape = boardShape([], statuses, 0, work.machines.length > 0);
      expect(shape.map(([status]) => status)).toEqual(columns(board({ work, statuses })));
      expect(ghostColumns(shape)).toEqual(shape.map(([status]) => status));
    }
  });

  // Each column of the shape has its count, none where the board counts none; sign-off comes
  // first while a task waits for it; with no statuses listed, the counted ones are the board's.
  it("counts each column, and opens on the tasks waiting for sign-off", () => {
    const counts: [string, number][] = [
      ["ready", 14],
      ["backlog", 3],
    ];
    expect(boardShape(counts, [], 2, false)).toEqual([
      ["signoff", 2],
      ["ready", 14],
      ["build-test", 0],
      ["backlog", 3],
    ]);
    expect(boardShape(counts, ["ready", "review"], 0, true)).toEqual([
      ["ready", 14],
      ["build", 0],
      ["test", 0],
      ["review", 0],
    ]);
  });

  // The shape is kept as lines of `STATUS COUNT` and read back as it was; one never kept, or kept
  // unreadable, is the pipeline and the columns off it with a task in each.
  it("reads back as it was kept, and none as a task in each column", () => {
    const shape = boardShape([["ready", 14]], [], 1, false);
    const kept = keepShape(shape);
    expect(kept).toBe("signoff 1\nready 14\nbuild-test 0");
    expect(recallShape(kept)).toEqual(shape);
    const fallback = recallShape(null);
    expect(recallShape("not a shape\n")).toEqual(fallback);
    expect(recallShape("ready many")).toEqual(fallback);
    expect(fallback.map(([status]) => status)).toEqual([
      "concept",
      "design",
      "ready",
      "in_progress",
      "build",
      "test",
      "review",
      "done",
      "backlog",
      "blocked",
      "cancelled",
    ]);
    expect(fallback.every(([, count]) => count === 1)).toBe(true);
  });
});

describe("the board's ghost", () => {
  const ghost = (shape: [string, number][]) => renderToStaticMarkup(<GhostBoard shape={shape} />);

  // Each ghost column is the column it stands for, with a card per task it held (a page's at most,
  // then the Load more button's room; none for an empty column); Build and Test draw their lines
  // of text where they hold no card.
  it("draws each column it last had with its cards", () => {
    const html = ghost([
      ["signoff", 1],
      ["ready", 20],
      ["build", 0],
      ["test", 0],
      ["review", 2],
      ["done", 31],
      ["blocked", 0],
    ]);
    const column = (status: string) => {
      const start = html.indexOf(`data-for="${status}"`);
      const end = html.indexOf("data-for=", start + 1);
      return html.slice(start, end < 0 ? html.length : end);
    };
    const cards = (status: string, height: number) =>
      column(status).split(`class="ghost-card" style="height:${height}px"`).length - 1;
    const guessed = ROW - GAP;
    const more = `style="height:${MORE}px"></div>`;
    expect(cards("signoff", guessed)).toBe(1);
    expect(cards("ready", guessed)).toBe(20);
    expect(column("ready").match(/ghost-pill/g)).toHaveLength(20);
    expect(column("ready")).not.toContain(more);
    expect(cards("review", guessed)).toBe(2);
    expect(column("review").match(/ghost-pill/g)).toHaveLength(2 * 2);
    expect(cards("done", guessed)).toBe(PAGE);
    expect(column("done")).toContain(more);
    for (const empty of ["build", "test", "blocked"]) {
      expect(column(empty)).not.toContain("ghost-card");
    }
    // Heading, Machine context and the empty line; then heading and the empty line.
    expect(column("build").match(/class="bone"/g)).toHaveLength(3);
    expect(column("test").match(/class="bone"/g)).toHaveLength(2);
    expect(html.indexOf('data-for="signoff"')).toBeLessThan(html.indexOf("ghost-pipeline"));
    expect(html.indexOf("ghost-offline")).toBeLessThan(html.indexOf('data-for="blocked"'));
    // The page says once that it loads.
    expect(html).not.toContain('role="status"');
  });

  it("draws the rail's reasons and link where no machine reported", () => {
    const html = ghost([["build-test", 0]]);
    const rail = html.slice(html.indexOf('data-for="build-test"'));
    // Its heading, its five lines of why, and its link.
    expect(rail.match(/class="bone"/g)).toHaveLength(1 + 5 + 1);
  });

  it("draws a title's last line short, a card's lines as its box has room for", () => {
    const html = renderToStaticMarkup(<GhostBoard shape={[["ready", 1]]} />);
    expect(html).toContain('<span class="bone" style="--w:62%"></span>');
  });

  // The board's ghost is drawn in classes of its own: a wait for the board's grid, a column, a
  // card or the scroller never takes the ghost for the board.
  it("carries none of the classes the board is found by", () => {
    const html = ghost(recallShape(null));
    const classes = [...html.matchAll(/class="([^"]*)"/g)].flatMap((m) =>
      (m[1] ?? "").split(/\s+/),
    );
    expect(classes).toContain("ghost-kanban");
    expect(html).not.toContain("data-status");
    for (const real of ["kanban", "pipeline", "board-scroll", "offline", "column", "taskcard"]) {
      expect(classes, real).not.toContain(real);
    }
  });
});

describe("a board whose first read failed", () => {
  // It says why in place of its ghost, in the server's words (a board not made yet names the
  // command that makes it), with a retry, and draws none of the board's classes nor the ghost's.
  it("says why in place of its ghost", () => {
    let html = renderToStaticMarkup(
      <UnreadBoard
        why="the task board cannot be reached: the alpha board is not made yet, and a reader never makes a board: nothing was read or written; `just tasks list` in alpha makes it (the server answered 503)"
        retry={() => {}}
      />,
    );
    expect(html).toMatch(
      /^<section class="trouble card unread-board" role="status"><h2>The task board could not be read<\/h2><p>the task board cannot be reached: the alpha board is not made yet, and a reader never makes a board: nothing was read or written; `just tasks list` in alpha makes it \(the server answered 503\)\. Trying again…<\/p>/,
    );
    expect(html).toMatch(/>Retry<\/button>/);
    for (const drawn of ["ghost-", "kanban", "bone"]) expect(html).not.toContain(drawn);
    // A server out of reach sends the reader to the connection, as every page does, and the board
    // is read again by itself.
    html = renderToStaticMarkup(<UnreadBoard why={UNREACHABLE} retry={() => {}} />);
    expect(html).toContain(
      "<p>The server could not be reached; check the connection. Trying again…</p>",
    );
  });
});

describe("the Build column's machine context", () => {
  it("keeps uncertainty compact and sends the inventory to the agents page", () => {
    for (const connected of [true, false]) {
      const html = renderToStaticMarkup(<BoardContext work={NO_WORK} connected={connected} />);
      expect(html).toContain("<details");
      expect(html).toContain("capacity unknown");
      expect(html).toContain("/agents#machine-context");
      expect(html.includes("Disconnected")).toBe(!connected);
    }
    const partial = aWork(
      [
        aMachine("alpha", [anOperation("alpha", "build", "build", "running")], {
          freshness: "incomplete",
        }),
      ],
      { freshness: "incomplete", unassigned: [anOperation(null, "legacy", "test", "queued")] },
    );
    const html = renderToStaticMarkup(<BoardContext work={partial} connected />);
    expect(html).toContain("Observation status · partial inventory");
    expect(html).not.toContain("Builds: 0 running");
  });
});
