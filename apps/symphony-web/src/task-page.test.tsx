import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { drawn, FRESH } from "./cache";
import { STALE } from "./shared";
import { LiveTask } from "./task-live";
import {
  Brief,
  type BriefRead,
  briefAdd,
  briefBars,
  briefOf,
  Composer,
  GhostBrief,
  GhostHead,
  GhostTask,
  GhostThread,
  LiveTaskPage,
  NOTHING_READ,
  OwnerControls,
  recipients,
  TaskDetail,
  type TaskDetailProps,
  TaskMessage,
  TaskPageView,
} from "./task-page";
import { aRole, aTask, memoryStorage } from "./testing";
import type { Block, TaskBrief, TaskEntry, TaskPage } from "./wire";
import { CHECKING } from "./words";
import { NO_WORK } from "./work";

beforeEach(() => {
  vi.stubGlobal("document", { getElementById: () => null, querySelector: () => null });
  vi.stubGlobal("localStorage", memoryStorage());
});

afterEach(() => {
  STALE.set(false);
  vi.unstubAllGlobals();
});

const html = (node: ReactElement) => renderToStaticMarkup(node);

const paragraph = (text: string): Block => ({ type: "paragraph", spans: [{ type: "text", text }] });

/** Message `id` of the task's thread, by `sender`, of `kind`, with `blocks`. */
function anEntry(id: number, sender: string, kind = "message", blocks: Block[] = []): TaskEntry {
  return {
    message: { id, slug: "fixture/task", sender, recipient: null, kind, text: "Said", ts: 1 },
    stamp: "2026-11-11",
    blocks,
  };
}

/** The page of `task`, with whatever else `more` says. */
function aPage(
  task = aTask("fixture/task", { status: "review", priority: 1 }),
  more: Partial<TaskPage> = {},
): TaskPage {
  return {
    machine_work: NO_WORK,
    task,
    blocks: [],
    children: [],
    children_count: 0,
    brief_omitted: 0,
    brief_next: 0,
    brief_revision: "",
    sessions: {},
    local: false,
    gates: null,
    ...more,
  };
}

/**
 * A task's page as its read stands, `props` saying otherwise, under an address naming `region`: the
 * part drawn whole, as a link to it draws it at once, when the parts below the screen wait for it.
 */
function detail(props: Partial<TaskDetailProps> = {}, region = "") {
  vi.stubGlobal("location", { hash: region === "" ? "" : `#${region}-fixture/task` });
  const page = props.page ?? aPage();
  return html(
    <TaskDetail
      page={page}
      work={NO_WORK}
      connected
      tasks={[]}
      entries={[]}
      older={false}
      loaded
      offer={null}
      failed={false}
      nudged={0}
      onHistory={() => {}}
      onChildren={() => {}}
      onOffer={() => {}}
      {...props}
    />,
  );
}

const STATUSES = [
  "backlog",
  "concept",
  "design",
  "ready",
  "in_progress",
  "blocked",
  "review",
  "done",
  "cancelled",
];

describe("the owner's controls", () => {
  it("show the task's priority, and a cancel only while it is open", () => {
    for (const status of STATUSES) {
      for (const priority of [0, 1, 2, 3, 4]) {
        const drawn = html(
          <OwnerControls slug="fixture/task" priority={priority} status={status} />,
        );
        expect(drawn).toContain('for="task-priority"');
        expect(drawn).toContain('id="task-priority"');
        expect(drawn).toContain(`<span>p${priority}</span>`);
        expect(drawn).toContain('aria-haspopup="listbox" aria-expanded="false"');
        expect(drawn.includes(">Cancel task</button>"), `${status}: ${drawn}`).toBe(
          !["done", "cancelled"].includes(status),
        );
        expect(drawn).not.toContain("Confirm cancellation");
        expect(drawn, "empty feedback takes no space").not.toContain('role="status"');
      }
    }
  });
});

describe("a task's actions while the page may be out of date", () => {
  const OFFER = { action: "land", label: "Land it", branch: "perf/x", payload: "{}" };
  const controls = () =>
    html(
      <>
        <OwnerControls slug="fixture/task" priority={1} status="review" />
        <Composer slug="fixture/task" roles={[]} approval={OFFER} />
      </>,
    );

  it("wait, disabled and saying so, the draft staying editable", () => {
    STALE.set(true);
    const held = controls();
    for (const said of ["Cancel task", "Send", "✓ Land it"]) {
      expect(held, said).not.toContain(`>${said}</button>`);
    }
    expect(held.split(`>${CHECKING}</button>`)).toHaveLength(4);
    // The priority's select, the cancel, the send and the landing.
    expect(held.split(' disabled=""')).toHaveLength(5);
    expect(held).toMatch(/<textarea[^>]*id="task-message"/);
    expect(held).not.toMatch(/<textarea[^>]*disabled/);
  });

  it("each say what they do once the page is current", () => {
    const current = controls();
    for (const said of ["Cancel task", "Send", "✓ Land it"]) {
      expect(current, said).toContain(`>${said}</button>`);
    }
    expect(current).not.toContain(CHECKING);
    expect(current).not.toContain(' disabled=""');
    expect(current).toContain('<code class="approval-branch">perf/x</code>');
  });
});

describe("the composer", () => {
  it("draws the draft this device kept for the task, and whom it goes to", () => {
    localStorage.setItem("task-draft:fixture/task", "Half a thought");
    localStorage.setItem("task-to:fixture/task", "@reviewer");
    const drawn = html(
      <Composer slug="fixture/task" roles={[aRole("executor", "amber")]} approval={null} />,
    );
    expect(drawn).toContain(">Half a thought</textarea>");
    expect(drawn).toContain("<span>@reviewer</span>");
    expect(drawn).toContain('<form id="task-send" class="verdict" data-posting="false"');
  });

  it("offers no landing without a payload to post", () => {
    const drawn = html(
      <Composer
        slug="fixture/task"
        roles={[]}
        approval={{ action: "land", label: "Land it", branch: "b", payload: null }}
      />,
    );
    expect(drawn).not.toContain("Land it");
    expect(drawn).not.toContain("approval-branch");
  });

  it("sends to everyone on the task, a role, or a holder of one", () => {
    expect(recipients([aRole("executor", "amber"), aRole("reviewer", "birch")])).toEqual([
      ["", "everyone on it"],
      ["@executor", "@executor"],
      ["@reviewer", "@reviewer"],
      ["@orchestrator", "@orchestrator"],
      ["amber", "amber"],
      ["birch", "birch"],
    ]);
  });
});

describe("a message of a task's thread", () => {
  it("is literal text from the owner, blocks from an agent, and says to whom and a verdict", () => {
    const owner = html(<TaskMessage entry={anEntry(1, "owner")} />);
    expect(owner).toContain('<div class="entry owner"><time>2026-11-11</time>');
    expect(owner).toContain('<div class="entry-text literal">Said</div>');
    const agent = html(
      <TaskMessage entry={anEntry(2, "amber", "verdict", [paragraph("Approved")])} />,
    );
    expect(agent).toContain('class="entry agent"');
    expect(agent).toContain("<b>verdict </b>");
    expect(agent).toContain('<div class="entry-text"><p');
    expect(agent).toContain("Approved");
    const addressed = {
      ...anEntry(3, "owner"),
      message: { ...anEntry(3, "owner").message, recipient: "amber" },
    };
    expect(html(<TaskMessage entry={addressed} />)).toContain("owner → amber");
  });

  it("from the owner, draws its files' blocks when the server sent them", () => {
    const upload = html(
      <TaskMessage entry={anEntry(1, "owner", "message", [paragraph("A file")])} />,
    );
    expect(upload).not.toContain("literal");
    expect(upload).toContain("A file");
  });

  it("from the system, is literal text", () => {
    const system = html(<TaskMessage entry={anEntry(1, "board", "system", [paragraph("x")])} />);
    expect(system).toContain('class="entry system"');
    expect(system).toContain('<div class="entry-text literal">Said</div>');
  });
});

describe("a task's page", () => {
  it("is one list whose items are the thread's messages", () => {
    const drawn = detail(
      { entries: [anEntry(1, "owner"), anEntry(2, "cobalt-otter")] },
      "thread-region",
    );
    expect(drawn.split('class="history"')).toHaveLength(2);
    const list = drawn.split('<ul class="history">')[1]?.split("</ul>")[0] ?? "";
    expect(list.split("<li ")).toHaveLength(3);
    expect(list).toContain('id="message-1"');
    expect(list).toContain('id="message-2"');
  });

  it("draws its head: slug, title, state, who filed it, its facts and its roles", () => {
    const task = aTask("fixture/task", {
      title: "Fix <it>",
      status: "in_progress",
      created_by: "owner",
      parent: "fixture",
      depends: ["fixture/before"],
      links: ["docs/TASKS.md"],
      roles: [aRole("executor", "amber")],
    });
    const drawn = detail({ page: aPage(task) });
    expect(drawn).toContain('<article class="task" data-slug="fixture/task">');
    expect(drawn).toContain("<code>fixture/task</code>");
    expect(drawn).toContain("<h1>Fix &lt;it&gt;</h1>");
    expect(drawn).toContain('<span class="state pill st-in_progress">in progress</span>');
    expect(drawn).toContain('<span class="by">filed by owner</span>');
    expect(drawn).toContain('<li>part of <a href="/task/fixture">fixture</a></li>');
    expect(drawn).toContain('<li>after <a href="/task/fixture/before">fixture/before</a></li>');
    expect(drawn).toContain('<li>implements <a href="/docs/TASKS.md">docs/TASKS.md</a></li>');
    expect(drawn).toContain(">executor amber");
    expect(drawn).toContain("<h2>Thread</h2>");
  });

  it("offers its subtasks while it has some, to load or load more", () => {
    expect(detail({}, "subtasks")).not.toContain("Subtasks");
    const counted = aPage(undefined, { children_count: 2 });
    expect(detail({ page: counted }, "subtasks")).toContain(">Load subtasks</button>");
    const child = aTask("fixture/task/1", { title: "First", status: "done" });
    const more = detail({ page: counted, tasks: [child] }, "subtasks");
    expect(more).toContain(">Load more subtasks</button>");
    expect(more).toContain('id="child-fixture/task/1"');
    const all = detail({ page: counted, tasks: [child, aTask("fixture/task/2")] }, "subtasks");
    expect(all).not.toContain("Load more subtasks");
    expect(all).not.toContain(">Load subtasks</button>");
    expect(all).toContain('<span class="count">2</span>');
  });

  it("offers earlier messages and, before the thread is read, its loading", () => {
    const waiting = detail({ older: true, loaded: false }, "thread-region");
    expect(waiting).toContain(">Load earlier messages</button>");
    expect(waiting).toContain(">Load messages</button>");
    expect(waiting).toContain('<div id="task-thread" data-pending="true">');
    const read = detail({}, "thread-region");
    expect(read).toContain('<div id="task-thread" data-pending="false">');
    expect(read).not.toContain("Load earlier messages");
    expect(read).not.toContain(">Load messages</button>");
  });

  it("offers the landing only for the revision it was offered for", () => {
    const offer = { action: "land", label: "Land it", branch: "perf/x", payload: "{}" };
    const page = aPage(aTask("fixture/task", { updated: 5 }));
    expect(detail({ page, offer: [offer, 5] }, "composer")).toContain("✓ Land it");
    expect(detail({ page, offer: [offer, 4] }, "composer")).not.toContain("Land it");
  });
});

describe("a long brief", () => {
  const brief = (revision: string, omitted: number) =>
    html(
      <Brief
        slug="fixture/task"
        revision={revision}
        blocks={[paragraph("start")]}
        omitted={omitted}
        next={24_000}
      />,
    );

  it("edited from short to long offers its remaining text", () => {
    expect(brief("short", 0)).not.toContain("brief-more");
    const long = brief("long", 90_000);
    expect(long).toContain("brief-more");
    expect(long).toContain("90000 bytes of it are not shown yet");
  });

  const chunk = (revision: string, text: string, next: number, omitted: number): TaskBrief => ({
    blocks: [paragraph(text)],
    next,
    omitted,
    revision,
  });

  it("read ends with its revision", () => {
    const read = briefAdd(
      NOTHING_READ,
      "one",
      24_000,
      150_000,
      chunk("one", "second part", 124_000, 50_000),
    );
    const held = briefOf(read, "one");
    expect([held?.blocks, held?.next, held?.omitted]).toEqual([
      [paragraph("second part")],
      124_000,
      50_000,
    ]);
    expect(briefOf(read, "two"), "an edited brief starts from its page").toBeNull();
    const again = briefAdd(read, "two", 24_000, 30_000, chunk("two", "new rest", 54_000, 0));
    expect(again, "nothing read of the first revision follows the second's start").toEqual({
      revision: "two",
      blocks: [paragraph("new rest")],
      next: 54_000,
      omitted: 0,
      failed: "",
    } satisfies BriefRead);
  });

  it("is not joined to a chunk cut from another revision", () => {
    const read = briefAdd(
      NOTHING_READ,
      "one",
      24_000,
      150_000,
      chunk("two", "misaligned", 124_000, 9),
    );
    expect([read.blocks.length, read.next, read.omitted]).toEqual([0, 24_000, 150_000]);
    expect(read.failed).toContain("The brief changed");
    const joined = briefAdd(read, "one", 24_000, 150_000, chunk("one", "rest", 124_000, 50_000));
    expect([joined.blocks.length, joined.failed]).toEqual([1, ""]);
    const failed = briefAdd(joined, "one", 24_000, 150_000, "offline");
    expect(
      [failed.blocks.length, failed.next, failed.failed],
      "a failed read keeps what was read",
    ).toEqual([1, 124_000, "offline"]);
  });
});

describe("a task's ghost", () => {
  /** The bars of `part` of `drawn`, up to its next pill. */
  const bars = (drawn: string, part: string) => {
    const start = drawn.indexOf(part);
    const end = drawn.indexOf("ghost-pill", start);
    return drawn.slice(start, end < 0 ? drawn.length : end).split('class="bone"').length - 1;
  };

  it("holds its head's title and facts at the heights kept, a bar for each line they may hold", () => {
    const kept = html(<GhostHead height={311} title={97} facts={67} />);
    expect(kept).toContain("height:311px");
    expect(kept).toContain('class="ghost-title kept" style="height:97px"');
    expect(kept).toContain('class="ghost-meta ghost-facts" style="height:67px"');
    expect(bars(kept, "ghost-title kept")).toBe(6);
    expect(bars(kept, "ghost-facts")).toBe(6);
  });

  it("guesses a title of one line wide, two on a phone, and one fact when none was kept", () => {
    const guessed = html(<GhostHead height={null} title={null} facts={null} />);
    expect(guessed).not.toContain("kept");
    expect(guessed).not.toContain("ghost-facts");
    expect(guessed).not.toContain("height:");
    expect(bars(guessed, "ghost-title")).toBe(2);
  });

  it("draws only the bars of the brief its kept box shows whole, all with no height kept", () => {
    const paragraphs = (height: number | null) => {
      const drawn = html(<GhostBrief height={height} />);
      return [
        drawn
          .split('class="skeleton"')
          .slice(1)
          .map((part) => part.split('class="bone"').length - 1),
        drawn,
      ] as const;
    };
    const [all, none] = paragraphs(null);
    expect(all).toEqual([4, 3]);
    expect(none).not.toContain("height:");
    for (const [height, expected] of [
      [0, []],
      [52, []],
      [53, [1]],
      [71, [1]],
      [72, [2]],
      [91, [3]],
      [109, [3]],
      [110, [4]],
      [137, [4]],
      [138, [4, 1]],
      [157, [4, 2]],
      [175, [4, 2]],
      [176, [4, 3]],
      [900, [4, 3]],
      [2 ** 32 - 1, [4, 3]],
    ] as const) {
      const [drawnBars, drawn] = paragraphs(height);
      expect(drawnBars, `a box ${height} px tall`).toEqual(expected);
      expect(drawn).toContain(`height:${height}px`);
    }
  });

  it("counts the brief's bars as its box holds them", () => {
    expect(briefBars(null)).toEqual([4, 3]);
    expect(briefBars(-5)).toEqual([0, 0]);
    expect(briefBars(138)).toEqual([4, 1]);
  });

  it("says once that the task loads, the thread's own ghost silent", () => {
    const drawn = html(<GhostTask slug="fixture/task" />);
    expect(drawn).toContain("Loading task…");
    // The gates' outline is hidden from what is read out, its own status with it.
    expect(drawn).toMatch(
      /<div aria-hidden="true"><section class="task-gates card" data-gates="reading"[\s\S]*Reading the gates…/,
    );
    expect(drawn).not.toContain("Loading the thread…");
    expect(drawn).toContain('<div aria-busy="true">');
    expect(drawn).toContain('class="ghost-tl"');
    expect(html(<GhostThread />)).toContain("Loading the thread…");
    expect(html(<GhostThread announce={false} />)).not.toContain("Loading the thread…");
  });
});

describe("the route's task as its read stands", () => {
  const host = { redraw: () => {}, freshness: () => {}, stale: () => {} };
  const live = (slug = "fixture/task") =>
    new LiveTask("", slug, host, new AbortController().signal);

  it("is the task once its page is read", () => {
    const task = live();
    task.d.page = aPage();
    const drawn = html(<LiveTaskPage live={task} connected />);
    expect(drawn).toContain('<div class="task-page" data-ready="task">');
    expect(drawn).toContain('<article class="task"');
  });

  it("is its ghost while another task's page is shown", () => {
    const task = live("fixture/other");
    task.d.page = aPage();
    const drawn = html(<LiveTaskPage live={task} connected />);
    expect(drawn).toContain("Loading task…");
    expect(drawn).not.toContain("<article");
  });

  it("says no task has its name, or why it could not be read", () => {
    const missing = live();
    missing.d.missing = "fixture/task";
    const none = html(<LiveTaskPage live={missing} connected />);
    expect(none).toContain("No task fixture/task");
    expect(none).toContain('href="/tasks"');
    const failing = live();
    failing.d.error = "The server answered 500";
    expect(html(<LiveTaskPage live={failing} connected />)).toContain(
      "The task could not be loaded",
    );
  });

  it("under the board's notices, waits for the board, or says it could not be read", () => {
    const waiting = html(<TaskPageView live={null} stale={false} freshness={FRESH} />);
    expect(waiting).toContain("Loading the board…");
    expect(waiting).toContain('data-feed="live"');
    const task = live();
    task.trouble = "The server answered 503";
    const trouble = html(<TaskPageView live={task} stale freshness={FRESH} />);
    expect(trouble).toContain("The board could not be loaded");
    expect(trouble).toContain("It is also tried again on its own.");
    expect(trouble).toContain('data-feed="stale"');
  });

  it("draws the task once the board is read or a copy from this device is", () => {
    const task = live();
    task.d.page = aPage();
    const device = html(
      <TaskPageView live={task} stale={false} freshness={drawn(FRESH, "page", 5)} />,
    );
    expect(device).toContain('data-ready="board"');
    expect(device).toContain('data-cache="device"');
    expect(device).toContain('data-feed="stale"');
    expect(device).toContain("<article");
    const shown = live();
    shown.boardShown = true;
    expect(html(<TaskPageView live={shown} stale={false} freshness={FRESH} />)).toContain(
      "Loading task…",
    );
  });
});
