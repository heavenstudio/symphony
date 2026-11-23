import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { byColumn, SHAPE } from "./board";
import {
  type BoardHost,
  emptyBoard,
  LiveBoard,
  readChanges,
  sameTask,
  TasksPage,
  TasksView,
  upsert,
  withListed,
} from "./boardlive";
import { ReadCursor } from "./journal";
import {
  aListing,
  aMachine,
  anOperation,
  fakeAnswer as answer,
  aTask,
  aWork,
  CASES,
  feedEvent as event,
  memoryStorage,
} from "./testing";
import type { MachineWork, Task, TaskListing } from "./wire";
import { statusNotice } from "./words";
import { NO_WORK } from "./work";

// React's `flushSync` stood in for, as no DOM runs here: whether a draw comes within it.
const react = vi.hoisted(() => ({ flushing: false }));

vi.mock("react-dom", async (original) => ({
  ...(await original<typeof import("react-dom")>()),
  flushSync: (draw: () => void) => {
    react.flushing = true;
    try {
      draw();
    } finally {
      react.flushing = false;
    }
  },
}));

/** The wall clock the board reads: a task updated before it by more than its window is old. */
const NOW = 1_800_000_000_000;

/** A listing of `tasks`, counted as `counts`, and whatever else `more` says. */
function listing(tasks: Task[], counts: [string, number][] = [], more: Partial<TaskListing> = {}) {
  return aListing(tasks, counts, { orchestrators: [], unstarted: 0, ...more });
}

type Route = (url: string, etag: string) => Response | Promise<Response>;

interface Kept {
  value: unknown;
  etag: string;
  saved: number;
}

/**
 * A live board over a server that answers each read as `route` does, on a page whose device kept
 * `kept` (by part of the board's item). What it asked, the host's calls and the store's.
 */
function live(route: Route = () => answer(503), kept: Record<string, Kept> = {}) {
  const asked: [string, string][] = [];
  const held = vi.fn();
  vi.stubGlobal("document", { getElementById: () => null, querySelector: () => null });
  vi.stubGlobal("window", { reviewHoldPlace: held });
  vi.stubGlobal("localStorage", memoryStorage());
  vi.stubGlobal("fetch", async (url: string, init?: RequestInit) => {
    const etag = (init?.headers as Record<string, string> | undefined)?.["If-None-Match"] ?? "";
    asked.push([url, etag]);
    return route(url, etag);
  });
  const store = {
    read: vi.fn(async (_format: number, _project: string, _id: string, part: string) => kept[part]),
    keep: vi.fn(async () => undefined),
    visit: vi.fn(async () => undefined),
    forget: vi.fn(async () => undefined),
  };
  vi.stubGlobal("symphonyCache", store);
  const host = {
    redraw: vi.fn(),
    refresh: vi.fn(),
    freshness: vi.fn(),
    stale: vi.fn(),
  } satisfies BoardHost;
  const page = new AbortController();
  const board = new LiveBoard("", host, page.signal, () => NOW);
  return { board, d: board.d, asked, host, store, held, page };
}

/** The urls asked, without their tags. */
const urls = (asked: [string, string][]) => asked.map(([url]) => url);

const COUNTS_URL = "/api/task-board?view=waiting&counts=1&q=&days=1&activity=1&work=0";
const column = (status: string, more = "") =>
  `/api/task-board?view=waiting&q=&status=${status}&days=1${more}&activity=1&work=0`;

/** A promise the test settles: what a read waits on while the test does something meanwhile. */
function later<T>(): { promise: Promise<T>; settle: (value: T) => void } {
  let settle: (value: T) => void = () => {};
  const promise = new Promise<T>((done) => {
    settle = done;
  });
  return { promise, settle };
}

const building = (slug: string, state: "queued" | "running" = "running"): MachineWork =>
  aWork([aMachine("alpha", [anOperation("alpha", "b", "build", state, { task: slug })])]);

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval"] });
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

/** Lets `ms` pass, and what it settles run. */
const pass = (ms = 0) => vi.advanceTimersByTimeAsync(ms);

describe("a feed event's changes", () => {
  it("are read whole, each region the board follows", () => {
    const changes = readChanges(
      event(
        ["machine-work", CASES.samples.work],
        ["task", CASES.samples.taskAdded],
        ["task-fields", CASES.samples.taskChange],
        ["session", { key: "amber", value: { health: "stale" } }],
        ["message", {}],
        ["task-counts", {}],
        ["task-recent-counts", {}],
        ["workflow", { generation: 2 }],
        ["tasks-status", { available: false, slow: true }],
        ["queue", {}],
        ["task-signoff", "unread"],
      ),
    );
    expect(changes?.map((change) => change.region)).toEqual([
      "machine-work",
      "task",
      "task-fields",
      "session",
      "message",
      "counts",
      "counts",
      "counts",
      "tasks-status",
    ]);
    expect(changes?.[3]).toEqual({ region: "session", key: "amber", health: "stale" });
    expect(changes?.[8]).toEqual({ region: "tasks-status", available: false, slow: true });
  });

  it("say a session gone, and a board's state not said, as none", () => {
    expect(readChanges(event(["session", { key: "amber" }]))).toEqual([
      { region: "session", key: "amber", health: null },
    ]);
    expect(readChanges(event(["session", {}]))).toEqual([
      { region: "session", key: "", health: null },
    ]);
    expect(readChanges(event(["tasks-status", { available: "yes" }]))).toEqual([
      { region: "tasks-status", available: undefined, slow: false },
    ]);
  });

  it.each([
    ["machines' work", ["machine-work", { machines: "none" }]],
    ["task", ["task", { value: 1 }]],
    ["task's fields", ["task-fields", { key: 1 }]],
  ] as [string, [string, unknown]][])("are none for a %s it cannot read", (_, change) => {
    expect(readChanges(event(["message", {}], change))).toBeNull();
  });

  it("are none in an event without any", () => {
    expect(readChanges({ version: 1 })).toEqual([]);
  });
});

describe("a task the feed sends", () => {
  const a = aTask("w/a");
  const b = aTask("w/b");

  it("is added last, replaced in its place, or removed", () => {
    expect(upsert([a], { key: "w/b", value: b })).toEqual([a, b]);
    const renamed = { ...a, title: "Renamed" };
    expect(upsert([a, b], { key: "w/a", value: renamed })).toEqual([renamed, b]);
    expect(upsert([a, b], { key: "w/a", removed: true })).toEqual([b]);
    expect(upsert([a], { key: "w/a", value: null })).toEqual([a]);
  });

  it("read again as it stands keeps the tasks as they were, the same objects", () => {
    const tasks = [a, b];
    expect(upsert(tasks, { key: "w/a", value: aTask("w/a") })).toBe(tasks);
    const moved = upsert(tasks, { key: "w/a", value: aTask("w/a", { priority: 0 }) });
    expect(moved).not.toBe(tasks);
    expect(moved[1]).toBe(b);
  });
});

describe("two reads of a task", () => {
  it("applies live round counts and clears an unavailable summary on full replacement", () => {
    const { board, d, host } = live();
    const task = aTask("w/a", { status: "done", updated: NOW / 1000 });
    d.tasks = [{ ...task, review_rounds: 1 }];
    expect(board.apply(event(["task-fields", { key: "w/a", fields: { review_rounds: 3 } }]))).toBe(
      false,
    );
    expect(d.tasks[0]?.review_rounds).toBe(3);
    expect(board.apply(event(["task-fields", { key: "w/a", fields: { review_rounds: 0 } }]))).toBe(
      false,
    );
    expect(d.tasks[0]?.review_rounds).toBe(0);
    expect(board.apply(event(["task", { key: "w/a", value: task }]))).toBe(false);
    expect(d.tasks).toHaveLength(1);
    expect(d.tasks[0]?.review_rounds).toBeUndefined();
    expect(host.redraw).toHaveBeenCalledTimes(3);
  });
  it("are the same when they say the same, whatever objects they are", () => {
    const a = aTask("w/a", { roles: [] });
    expect(sameTask(a, a)).toBe(true);
    expect(sameTask(a, aTask("w/a", { roles: [] }))).toBe(true);
    expect(sameTask(a, aTask("w/a", { title: "Renamed" }))).toBe(false);
    expect(sameTask(a, aTask("w/b"))).toBe(false);
  });
});

describe("a page of tasks put on the board", () => {
  const a = aTask("w/a");
  const b = aTask("w/b");

  it("puts each in its place, or last when new, in the page's order", () => {
    const c = aTask("w/c");
    const renamed = aTask("w/a", { title: "Renamed" });
    const placed = withListed([a, b], [c, renamed], new Map());
    expect(placed).toEqual([renamed, b, c]);
    expect(placed[1]).toBe(b);
  });

  it("keeps the object of a task read as the board held it, and only of one", () => {
    const before = new Map([
      ["w/a", a],
      ["w/b", b],
    ]);
    const again = [aTask("w/a"), aTask("w/b", { title: "Renamed" })];
    // The column's own tasks were taken off for its first page: they come back last.
    const placed = withListed([], again, before);
    expect(placed[0]).toBe(a);
    expect(placed[1]).not.toBe(b);
    expect(placed[1]?.title).toBe("Renamed");
    const tasks = [a, b];
    expect(withListed(tasks, [], before)).not.toBe(tasks);
    expect(withListed(tasks, [], before)).toEqual(tasks);
  });
});

describe("what the board says of its own state", () => {
  it("is nothing unless it is unavailable, then whether it is slow", () => {
    expect(statusNotice(undefined, true)).toBe("");
    expect(statusNotice(true, true)).toBe("");
    expect(statusNotice(false, true)).toBe("The task board is slow to answer. Retrying…");
    expect(statusNotice(false, false)).toBe("The task board is unavailable. Reconnecting…");
  });
});

describe("the journal of feed events", () => {
  it("keeps each event for the reads out since before it, and drops it once none is", () => {
    const journal = emptyBoard().journal;
    const first = new ReadCursor(journal);
    journal.version += 1;
    journal.events.push([journal.version, "one"]);
    const second = new ReadCursor(journal);
    const third = new ReadCursor(journal);
    journal.version += 1;
    journal.events.push([journal.version, "two"]);
    expect(first.events()).toEqual(["one", "two"]);
    expect(second.events()).toEqual(["two"]);
    first.release();
    first.release();
    expect(journal.events).toEqual([[2, "two"]]);
    second.release();
    expect(journal.readers.get(1)).toBe(1);
    third.release();
    expect(journal.events).toEqual([]);
    expect(journal.readers.size).toBe(0);
  });
});

describe("the live board's device copy", () => {
  const tasks = [aTask("w/a", { status: "in_progress" }), aTask("w/b", { status: "review" })];
  // Every listing carries the view's complete counts, a column's as the counts' own.
  const counts: [string, number][] = [
    ["in_progress", 1],
    ["review", 1],
  ];
  const kept = {
    counts: {
      value: listing([], counts, { statuses: ["in_progress", "review"] }),
      etag: '"c"',
      saved: 20,
    },
    work: { value: NO_WORK, etag: "", saved: 10 },
    "column:in_progress": { value: listing([tasks[0] as Task], counts), etag: '"p"', saved: 30 },
    "column:review": { value: listing([tasks[1] as Task], counts), etag: '"r"', saved: 40 },
  };

  it("draws the counts, the machines' work and each column kept, before the server answers", async () => {
    const { board, d, host, asked } = live(() => answer(503), kept);
    await board.drawKept();
    expect(asked).toEqual([]);
    expect(d.ready).toBe(true);
    expect(d.counts).toEqual(counts);
    expect(d.tasks.map((task) => task.slug)).toEqual(["w/a", "w/b"]);
    expect(d.drawn).toEqual(["in_progress", "review"]);
    expect(d.fromDevice).toEqual(["w/a", "w/b"]);
    expect(d.copies.get("counts")?.etag).toBe('"c"');
    expect(board.fresh.saved).toBe(10);
    expect(board.fresh.unanswered).toEqual([
      "work",
      "counts",
      "column:in_progress",
      "column:review",
    ]);
    expect(host.redraw).toHaveBeenCalled();
  });

  it("draws nothing when it kept no counts", async () => {
    const { board, d, host } = live(() => answer(503), { work: kept.work });
    await board.drawKept();
    expect(d.ready).toBe(false);
    expect(host.redraw).not.toHaveBeenCalled();
  });

  it("is checked once the board's version is read, and a task no answer lists goes", async () => {
    const { board, d, asked, host } = live((url, etag) => {
      if (url === COUNTS_URL) return answer(304);
      if (url === "/api/machine-work") return answer(200, NO_WORK);
      if (url === column("in_progress")) return answer(200, listing([]), '"p2"');
      if (url === column("review")) return etag === '"r"' ? answer(304) : answer(500);
      return answer(404);
    }, kept);
    board.kept = board.drawKept();
    await board.read();
    expect(new Set(urls(asked))).toEqual(
      new Set([column("in_progress"), column("review"), "/api/machine-work", COUNTS_URL]),
    );
    expect(asked).toContainEqual([COUNTS_URL, '"c"']);
    expect(d.waiting).toBe(false);
    expect(d.drawn).toEqual([]);
    // w/a was the only task its column held, and its fresh page has none: it is gone.
    expect(d.tasks.map((task) => task.slug)).toEqual(["w/b"]);
    expect(d.fromDevice).toEqual([]);
    expect(board.fresh.unanswered).toEqual([]);
    expect(host.refresh).toHaveBeenCalledTimes(1);
  });
});

describe("the live board's counts", () => {
  it("are read for the page's view, kept for the device, and drawn", async () => {
    const { board, d, store, host } = live(() =>
      answer(200, listing([], [["ready", 3]], { statuses: ["ready"], signoff: ["w/a"] }), '"c"'),
    );
    await board.counts();
    expect(d.ready).toBe(true);
    expect(d.counts).toEqual([["ready", 3]]);
    expect(d.counted).toBe(1);
    expect(store.keep).toHaveBeenCalledWith(
      expect.any(Number),
      "",
      "board:waiting",
      "board",
      "counts",
      '"c"',
      expect.anything(),
      undefined,
      false,
    );
    expect(board.fresh.unanswered).toEqual([]);
    expect(host.redraw).toHaveBeenCalled();
  });

  it("name the copy the page holds, and a 304 is that copy", async () => {
    const { board, d, asked } = live((_, etag) =>
      etag === '"c"' ? answer(304) : answer(200, listing([], [["ready", 1]]), '"c"'),
    );
    await board.counts();
    d.counts = [];
    await board.counts();
    expect(asked).toEqual([
      [COUNTS_URL, ""],
      [COUNTS_URL, '"c"'],
    ]);
    expect(d.counts).toEqual([["ready", 1]]);
  });

  it("are neither kept nor named by a tag under a search", async () => {
    const { board, d, asked, store } = live(() => answer(200, listing([]), '"c"'));
    d.query = "lamp shade";
    d.days = 7;
    await board.counts();
    expect(asked).toEqual([
      ["/api/task-board?view=waiting&counts=1&q=lamp%20shade&days=7&activity=1&work=0", ""],
    ]);
    expect(store.keep).not.toHaveBeenCalled();
  });

  it("drop an answer another view or a newer read replaced", async () => {
    const held = later<Response>();
    const { board, d } = live(() => held.promise);
    const read = board.counts();
    d.view += 1;
    held.settle(answer(200, listing([], [["ready", 1]])));
    await read;
    expect(d.counted).toBe(0);
    expect(d.ready).toBe(false);
  });

  it("say why they could not be read, unless the page shows this device's copy", async () => {
    const { board, d, host } = live(() => answer(503, { error: "the board is down" }));
    await board.counts();
    // A read nothing tries again (a refresh's, a search's, the feed's open) says its reason alone.
    expect(d.error).toBe("the board is down (the server answered 503)");
    expect(board.fresh.failing).toEqual(["counts"]);
    expect(host.redraw).toHaveBeenCalled();
    const device = live(() => answer(503));
    device.board.fresh = { ...device.board.fresh, saved: 1, unanswered: ["counts"] };
    await device.board.check();
    expect(device.d.error).toBe("");
  });

  it("say they try again while the check still asks for them, whoever's read failed", async () => {
    let up = false;
    const { board, d } = live((url) => {
      if (url === "/api/machine-work") return answer(200, NO_WORK);
      return up ? answer(200, listing([], [["ready", 1]])) : answer(503, { error: "down" });
    });
    await board.check();
    expect(d.error).toBe("down (the server answered 503). Trying again…");
    expect(board.fresh.failing).toEqual(["counts"]);
    // The feed's open reads them too: the check still asks again after it, so it says so.
    await board.counts();
    expect(d.error).toBe("down (the server answered 503). Trying again…");
    // The check's next try answers, and what was said goes.
    up = true;
    await pass(1_000);
    expect(d.counted).toBe(1);
    expect(d.error).toBe("");
    // Once the check has its counts, a refresh's read that fails is asked again by nothing.
    up = false;
    await board.counts();
    expect(d.error).toBe("down (the server answered 503)");
    // Unreachable, it says to check the connection, and still that it tries again.
    const away = live((url) =>
      url === "/api/machine-work"
        ? answer(200, NO_WORK)
        : Promise.reject(new TypeError("Failed to fetch")),
    );
    await away.board.check();
    expect(away.d.error).toBe(
      "The server could not be reached; check the connection. Trying again…",
    );
  });

  it("say so of an answer that is no listing", async () => {
    const { board, d } = live(() => answer(200, { tasks: "none" }));
    await board.counts();
    expect(d.error).toBe("The task board's answer is not a listing");
  });

  it("keep the board's shape for the next visit's ghost, once the machines' work is in", async () => {
    const { board } = live((url) =>
      url === "/api/machine-work"
        ? answer(200, NO_WORK)
        : answer(200, listing([], [["ready", 2]], { statuses: ["ready"] })),
    );
    await board.counts();
    expect(localStorage.getItem(SHAPE)).toBeNull();
    await board.machineWork();
    expect(localStorage.getItem(SHAPE)).toBe(
      "concept 0\ndesign 0\nready 2\nin_progress 0\nbuild 0\ntest 0\nreview 0\ndone 0",
    );
  });

  it("keep that a custom workflow held the board, so its phone ghost reserves no row it lacks", async () => {
    const waiting = { kinds: [], columns: [], workflow: 2 };
    const { board } = live((url) =>
      url === "/api/machine-work"
        ? answer(200, NO_WORK)
        : answer(200, listing([], [["ready", 2]], { statuses: ["ready"], waiting })),
    );
    await board.counts();
    await board.machineWork();
    expect(localStorage.getItem(SHAPE)).toBe(
      "concept 0\ndesign 0\nready 2\nin_progress 0\nbuild 0\ntest 0\nreview 0\ndone 0\nworkflow",
    );
  });

  // The counts count a task waiting on the owner in its stage, and the board draws it in the
  // sign-off lane: once a column's first page brings it, the next visit's ghost outlines no box
  // for it in its stage.
  it("keep complete original-stage counts without a separate sign-off lane", async () => {
    const counts: [string, number][] = [
      ["ready", 1],
      ["review", 2],
    ];
    const more = { statuses: ["ready", "review"], signoff: ["w/a"] };
    const review = [aTask("w/a", { status: "review" }), aTask("w/b", { status: "review" })];
    const { board, d } = live((url) => {
      if (url === "/api/machine-work") return answer(200, NO_WORK);
      return answer(200, listing(url === column("review") ? review : [], counts, more));
    });
    await board.counts();
    await board.machineWork();
    expect(localStorage.getItem(SHAPE)).toBe(
      "concept 0\ndesign 0\nready 1\nin_progress 0\nbuild 0\ntest 0\nreview 2\ndone 0",
    );
    d.waiting = false;
    void board.column("ready", false, "", 1);
    await pass(250);
    expect(localStorage.getItem(SHAPE)).toBe(
      "concept 0\ndesign 0\nready 1\nin_progress 0\nbuild 0\ntest 0\nreview 2\ndone 0",
    );
    void board.column("review", false, "", 1);
    await pass(250);
    expect(localStorage.getItem(SHAPE)).toBe(
      "concept 0\ndesign 0\nready 1\nin_progress 0\nbuild 0\ntest 0\nreview 2\ndone 0",
    );
  });
});

describe("the live board's machine work", () => {
  it("is read once and kept, unless the feed said it meanwhile", async () => {
    const held = later<Response>();
    const { board, d, store } = live(() => held.promise);
    const read = board.machineWork();
    board.apply(event(["machine-work", building("w/a")]));
    held.settle(answer(200, NO_WORK));
    await read;
    expect(d.work).toEqual(building("w/a"));
    expect(d.worked).toBe(1);
    expect(store.keep).not.toHaveBeenCalled();
    expect(board.fresh.unanswered).toEqual([]);
  });

  it("says why it could not be read, and that it tries again: only the check reads it", async () => {
    const { board, d } = live(() => answer(200, { machines: 1 }));
    await board.machineWork();
    expect(d.error).toBe("The machines' work is not readable. Trying again…");
    expect(board.fresh.failing).toEqual(["work"]);
    const refused = live(() => answer(503));
    await refused.board.machineWork();
    expect(refused.d.error).toBe("The server answered 503. Trying again…");
  });

  it("says nothing of a failure once the feed answered for the work meanwhile", async () => {
    const held = later<Response>();
    const { board, d, host } = live(() => held.promise);
    const read = board.machineWork();
    expect(board.heard(event(["machine-work", building("w/a")]))).toBe(true);
    host.redraw.mockClear();
    held.settle(answer(503));
    await read;
    // The feed's work stays, and nothing tries the read again: no failure is said.
    expect(d.work).toEqual(building("w/a"));
    expect(d.error).toBe("");
    expect(board.fresh.failing).toEqual([]);
    expect(host.redraw).not.toHaveBeenCalled();
  });

  it("takes back what a failure said once the feed answers for the work, not the board's notice", async () => {
    const { board, d, host } = live(() => answer(503));
    await board.machineWork();
    expect(d.error).toBe("The server answered 503. Trying again…");
    host.redraw.mockClear();
    // The feed's work is the work drawn: nothing else draws again for it, the words' going does.
    expect(board.heard(event(["machine-work", NO_WORK]))).toBe(true);
    expect(d.error).toBe("");
    expect(board.fresh.failing).toEqual([]);
    expect(host.redraw).toHaveBeenCalledTimes(1);
    // The board's own notice of its state is no read's failure: the work's answer leaves it.
    await board.machineWork();
    expect(board.heard(event(["tasks-status", { available: false }]))).toBe(true);
    expect(d.error).toBe(statusNotice(false, false));
    expect(board.heard(event(["machine-work", building("w/a")]))).toBe(true);
    expect(board.fresh.failing).toEqual([]);
    expect(d.error).toBe(statusNotice(false, false));
  });

  it("takes back what a failure said once the check's next try answers", async () => {
    const first = later<Response>();
    let works = 0;
    const { board, d, host } = live((url) => {
      if (url !== "/api/machine-work") return answer(200, listing([], [["ready", 1]]));
      works += 1;
      return works === 1 ? first.promise : answer(200, NO_WORK);
    });
    const checking = board.check();
    await pass();
    // The counts answered first, so nothing else takes the words away.
    expect(d.counted).toBe(1);
    first.settle(answer(503));
    await checking;
    expect(d.error).toBe("The server answered 503. Trying again…");
    host.redraw.mockClear();
    await pass(1_000);
    expect(works).toBe(2);
    expect(d.worked).toBe(1);
    expect(d.error).toBe("");
    expect(board.fresh.failing).toEqual([]);
    expect(host.redraw).toHaveBeenCalled();
    await pass(60_000);
    expect(works).toBe(2);
  });
});

describe("the live board's reads", () => {
  it("are given up on the wire once the page ends, and change nothing", async () => {
    const { board, d, page, host } = live();
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
    const reads = [board.counts(), board.machineWork()];
    expect(signals.map((signal) => signal.aborted)).toEqual([false, false]);
    page.abort();
    expect(signals.map((signal) => signal.aborted)).toEqual([true, true]);
    await Promise.all(reads);
    expect(d.error).toBe("");
    expect(d.counted).toBe(0);
    expect(d.worked).toBe(0);
    expect(host.redraw).not.toHaveBeenCalled();
  });
});

describe("a column of the live board", () => {
  const ready = [aTask("w/a"), aTask("w/b")];

  /** A board whose version was read, over a server answering each column with `pages`. */
  const opened = (pages: Record<string, unknown>) => {
    const page = live((url) => (url in pages ? answer(200, pages[url]) : answer(404)));
    page.d.waiting = false;
    return page;
  };

  it("reads nothing before the board's version is read", async () => {
    const { board, asked } = live();
    await board.column("ready", false, "", 1);
    expect(asked).toEqual([]);
  });

  it("reads its first page after a pause, the last of several asks only", async () => {
    const { board, d, asked, host } = opened({
      [column("ready")]: listing(ready, [["ready", 2]], { next: "w/b" }),
    });
    const first = board.column("ready", false, "", 1);
    const second = board.column("ready", false, "", 1);
    await pass(250);
    await Promise.all([first, second]);
    expect(urls(asked)).toEqual([column("ready")]);
    expect(d.tasks).toEqual(ready);
    expect(d.cursors.get("ready")).toBe("w/b");
    expect(host.redraw).toHaveBeenCalled();
  });

  it("reads no page it checked at this generation", async () => {
    const { board, d, asked } = opened({});
    d.checked.set("ready", d.generation);
    await board.column("ready", false, "", 1);
    expect(asked).toEqual([]);
  });

  it("reads its next page from where the last left, the tasks added after them", async () => {
    const { board, d, asked } = opened({
      [column("ready")]: listing([aTask("w/a")], [["ready", 2]], { next: "w/a" }),
      [column("ready", "&cursor=w%2Fa")]: listing([aTask("w/b")], [["ready", 2]]),
    });
    void board.column("ready", false, "", 1);
    await pass(250);
    void board.column("ready", true, "", 1);
    await pass(250);
    expect(urls(asked)).toEqual([column("ready"), column("ready", "&cursor=w%2Fa")]);
    expect(d.tasks.map((task) => task.slug)).toEqual(["w/a", "w/b"]);
    expect(d.cursors.has("ready")).toBe(false);
  });

  it("holds Load more behind the first page's read, and asks for the next page after it", async () => {
    const { board, d, asked } = opened({
      [column("ready")]: listing([aTask("w/a")], [["ready", 2]], { next: "w/a" }),
      [column("ready", "&cursor=w%2Fa")]: listing([aTask("w/b")], [["ready", 2]]),
    });
    void board.column("ready", false, "", 1);
    await board.column("ready", true, "", 1);
    expect(d.pendingMore).toEqual(["ready"]);
    await pass(250);
    await pass(250);
    expect(urls(asked)).toEqual([column("ready"), column("ready", "&cursor=w%2Fa")]);
    expect(d.pendingMore).toEqual([]);
    expect(d.tasks.map((task) => task.slug)).toEqual(["w/a", "w/b"]);
  });

  it("says why it could not be read", async () => {
    const { board, d } = opened({});
    void board.column("ready", false, "", 1);
    await pass(250);
    expect(d.error).toContain("404");
    expect(board.fresh.failing).toEqual(["column:ready"]);
  });

  it("reads a first page that failed again by itself, waiting longer each time, until it answers", async () => {
    let refused = 2;
    const { board, d, asked } = live((url) =>
      url === column("ready") && refused-- > 0
        ? answer(503)
        : answer(200, listing(ready, [["ready", 2]])),
    );
    d.waiting = false;
    void board.column("ready", false, "", 1);
    await pass(250);
    expect(asked).toHaveLength(1);
    expect(d.error).toBe("The server answered 503. Trying again…");
    expect(d.tasks).toEqual([]);
    await pass(999);
    expect(asked).toHaveLength(1);
    await pass(1);
    expect(asked).toHaveLength(2);
    expect(d.error).toBe("The server answered 503. Trying again…");
    await pass(2000);
    // Answered: its tasks are drawn, nothing is said of the failure and nothing is asked again.
    expect(urls(asked)).toEqual([column("ready"), column("ready"), column("ready")]);
    expect(d.tasks).toEqual(ready);
    expect(d.error).toBe("");
    expect(board.fresh.failing).toEqual([]);
    await pass(120_000);
    expect(asked).toHaveLength(3);
  });

  it("keeps saying why while another read still fails", async () => {
    const { board, d } = live((url) =>
      url === column("ready") ? answer(200, listing(ready, [["ready", 2]])) : answer(503),
    );
    d.waiting = false;
    void board.column("done", false, "", 1);
    await pass(250);
    void board.column("ready", false, "", 1);
    await pass(250);
    expect(d.tasks).toEqual(ready);
    expect(board.fresh.failing).toEqual(["column:done"]);
    expect(d.error).toBe("The server answered 503. Trying again…");
  });

  it("answered, leaves what the board says of its own state", async () => {
    const { board, d } = opened({ [column("ready")]: listing(ready, [["ready", 2]]) });
    void board.column("ready", false, "", 1);
    board.heard(event(["tasks-status", { available: false, slow: false }]));
    const notice = statusNotice(false, false);
    expect(d.error).toBe(notice);
    await pass(250);
    expect(d.tasks).toEqual(ready);
    expect(d.error).toBe(notice);
  });

  it("stops trying a failed first page once a newer read of it, another view or the page's end replaces it", async () => {
    const newer = live();
    newer.d.waiting = false;
    void newer.board.column("ready", false, "", 1);
    await pass(250);
    // A refresh's read of the column takes over: one chain of tries, not two.
    void newer.board.column("ready", false, "", 1);
    await pass(250);
    expect(newer.asked).toHaveLength(2);
    await pass(1000);
    expect(newer.asked).toHaveLength(3);
    // Its tries would go on, into the next board's server.
    newer.page.abort();

    const viewed = live();
    viewed.d.waiting = false;
    void viewed.board.column("ready", false, "", 1);
    await pass(250);
    viewed.d.view += 1;
    await pass(120_000);
    expect(viewed.asked).toHaveLength(1);

    const gone = live();
    gone.d.waiting = false;
    void gone.board.column("ready", false, "", 1);
    await pass(250);
    gone.page.abort();
    await pass(120_000);
    expect(gone.asked).toHaveLength(1);
  });

  it("takes a first page given up as the page ends for no failure, and tries nothing again", async () => {
    const given: (AbortSignal | null)[] = [];
    const { board, d, asked, page, host } = live();
    vi.stubGlobal("fetch", (url: string, init?: RequestInit) => {
      asked.push([url, ""]);
      given.push(init?.signal ?? null);
      return new Promise<Response>((_, refuse) =>
        init?.signal?.addEventListener("abort", () =>
          refuse(new DOMException("gone", "AbortError")),
        ),
      );
    });
    d.waiting = false;
    void board.column("ready", false, "", 1);
    await pass(250);
    expect(asked).toHaveLength(1);
    host.redraw.mockClear();
    page.abort();
    await pass(120_000);
    expect(given[0]?.aborted).toBe(true);
    expect([d.error, board.fresh.failing, asked.length]).toEqual(["", [], 1]);
    expect(host.redraw).not.toHaveBeenCalled();
  });

  it("reads a page Load more asked for once: a failure is said, not tried again, and gone once a click's read answers", async () => {
    let refused = true;
    const { board, d, asked } = live((url) => {
      if (url === column("ready")) {
        return answer(200, listing([aTask("w/a")], [["ready", 2]], { next: "w/a" }));
      }
      return refused ? answer(503) : answer(200, listing([aTask("w/b")], [["ready", 2]]));
    });
    d.waiting = false;
    void board.column("ready", false, "", 1);
    await pass(250);
    void board.column("ready", true, "", 1);
    await pass(250);
    expect(d.error).toBe("The server answered 503");
    await pass(120_000);
    expect(asked).toHaveLength(2);
    refused = false;
    void board.column("ready", true, "", 1);
    await pass(250);
    expect(d.tasks.map((task) => task.slug)).toEqual(["w/a", "w/b"]);
    expect(d.error).toBe("");
  });

  it("drops an answer for a view replaced meanwhile", async () => {
    const held = later<Response>();
    const { board, d } = live(() => held.promise);
    d.waiting = false;
    void board.column("ready", false, "", 1);
    await pass(250);
    d.view += 1;
    held.settle(answer(200, listing(ready)));
    await pass();
    expect(d.tasks).toEqual([]);
  });

  it("replaces only its own stage's tasks with its first page", async () => {
    const { board, d } = opened({ [column("ready")]: listing([aTask("w/c")], [["ready", 1]]) });
    d.tasks = [aTask("w/a"), aTask("w/b", { status: "review" })];
    void board.column("ready", false, "", 1);
    await pass(250);
    expect(d.tasks.map((task) => task.slug)).toEqual(["w/b", "w/c"]);
  });

  it("read again as the board held it keeps its tasks, so nothing is drawn again", async () => {
    const page = listing([aTask("w/a"), aTask("w/b")], [["ready", 2]]);
    const { board, d } = opened({ [column("ready")]: page });
    d.tasks = [aTask("w/a"), aTask("w/b"), aTask("w/c", { status: "review" })];
    const [a, b, c] = d.tasks;
    void board.column("ready", false, "", 1);
    await pass(250);
    // The column's first page replaced its tasks, each the object the board held.
    expect(d.tasks).toHaveLength(3);
    expect(d.tasks[0]).toBe(c);
    expect(d.tasks[1]).toBe(a);
    expect(d.tasks[2]).toBe(b);
  });

  it("is drawn with the feed's events since its read began", async () => {
    const held = later<Response>();
    const { board, d } = live(() => held.promise);
    d.waiting = false;
    void board.column("ready", false, "", 1);
    await pass(250);
    board.apply(event(["task", { key: "w/z", value: aTask("w/z") }]));
    held.settle(answer(200, listing(ready, [["ready", 3]])));
    await pass();
    expect(d.tasks.map((task) => task.slug)).toEqual(["w/a", "w/b", "w/z"]);
    expect(d.journal.events).toEqual([]);
  });

  /** `count` ready tasks, `w/r-1` on. */
  const readyRows = (count: number, from = 1) =>
    Array.from({ length: count }, (_, at) => aTask(`w/r-${from + at}`));

  it("read again past its first page asks for every row it shows, and keeps them", async () => {
    const shown = readyRows(45);
    const filed = aTask("w/filed", { priority: 0 });
    const { board, d, asked, store } = opened({
      [column("ready", "&rows=46")]: listing([filed, ...readyRows(45)], [["ready", 46]], {
        next: "w/r-45",
      }),
    });
    // Two pages and a half loaded, a task filed by the feed, a card of another column.
    const review = aTask("w/review", { status: "review" });
    d.tasks = [...shown, review, filed];
    d.copies.set("column:ready", { etag: '"first"', list: listing(readyRows(30)) });
    void board.column("ready", false, "", 1);
    await pass(250);
    // One read of them all, the column's first long one, which names no tag and is not kept on
    // the device.
    expect(asked).toEqual([[column("ready", "&rows=46"), ""]]);
    expect(store.keep).not.toHaveBeenCalled();
    expect(d.copies.get("column:ready")?.etag).toBe('"first"');
    // Every row stands, each the object the board held, and Load more goes on after the last.
    expect(d.tasks).toHaveLength(47);
    expect(d.tasks[0]).toBe(review);
    shown.forEach((task, at) => {
      expect(d.tasks[at + 2]).toBe(task);
    });
    expect(d.tasks[1]).toBe(filed);
    expect(d.cursors.get("ready")).toBe("w/r-45");
  });

  it("read again past its first page names its last long answer's tag, and a 304 is that answer", async () => {
    const answered = listing(readyRows(46), [["ready", 50]], { next: "w/r-46" });
    // What the server holds for the read, and its tag.
    let served = { tag: '"long-1"', list: answered };
    const { board, d, asked, store, host } = live((url, etag) => {
      if (url !== column("ready", "&rows=46")) return answer(404);
      return etag === served.tag ? answer(304) : answer(200, served.list, served.tag);
    });
    d.waiting = false;
    d.tasks = readyRows(46);
    d.copies.set("column:ready", { etag: '"first"', list: listing(readyRows(30)) });
    const refresh = async () => {
      void board.column("ready", false, "", 1);
      await pass(250);
    };
    await refresh();
    expect(d.long.get("column:ready")).toEqual({ etag: '"long-1"', list: answered });
    const drawn = d.tasks;
    host.redraw.mockClear();
    // Nothing changed in the column: the next refresh names the tag, and the 304 draws the
    // answer the page holds, each card the object it was.
    await refresh();
    expect(asked).toEqual([
      [column("ready", "&rows=46"), ""],
      [column("ready", "&rows=46"), '"long-1"'],
    ]);
    expect(d.tasks).toHaveLength(46);
    d.tasks.forEach((task, at) => {
      expect(task).toBe(drawn[at]);
    });
    expect(d.cursors.get("ready")).toBe("w/r-46");
    expect(d.counts).toEqual([["ready", 50]]);
    expect(host.redraw).toHaveBeenCalled();
    expect(board.fresh.failing).toEqual([]);
    // Neither read is kept on the device, nor touches the first page it keeps.
    expect(store.keep).not.toHaveBeenCalled();
    expect(d.copies.get("column:ready")?.etag).toBe('"first"');
    // A card of the column changed: the server answers the whole read, which replaces the one held.
    const retitled = listing(
      [aTask("w/r-1", { title: "Retitled" }), ...readyRows(45, 2)],
      [["ready", 50]],
      { next: "w/r-46" },
    );
    served = { tag: '"long-2"', list: retitled };
    await refresh();
    expect(asked.at(-1)).toEqual([column("ready", "&rows=46"), '"long-1"']);
    expect(d.tasks[0]?.title).toBe("Retitled");
    expect(d.long.get("column:ready")).toEqual({ etag: '"long-2"', list: retitled });
    await refresh();
    expect(asked.at(-1)).toEqual([column("ready", "&rows=46"), '"long-2"']);
    expect(d.tasks[0]?.title).toBe("Retitled");
  });

  it("holds a long answer under a search too, and names it on no page-long read", async () => {
    const searched = (more: string) =>
      `/api/task-board?view=waiting&q=w&status=ready&days=1${more}&activity=1&work=0`;
    const { board, d, asked } = live((url) =>
      url === searched("&rows=31")
        ? answer(200, listing(readyRows(31), [["ready", 31]]), '"found"')
        : answer(200, listing(readyRows(30), [["ready", 30]]), '"page"'),
    );
    d.waiting = false;
    d.query = "w";
    d.tasks = readyRows(31);
    d.long.set("column:ready", { etag: '"long"', list: listing(readyRows(31)) });
    void board.column("ready", false, "w", 1);
    await pass(250);
    // A search's long read names the tag held, whichever view it came from: the server answers
    // `304` only to the same bytes.
    expect(asked).toEqual([[searched("&rows=31"), '"long"']]);
    expect(d.long.get("column:ready")?.etag).toBe('"found"');
    // A page or less: the search's first page names no tag, the default view's its device copy.
    d.tasks = readyRows(30);
    void board.column("ready", false, "w", 1);
    await pass(250);
    d.query = "";
    d.copies.set("column:ready", { etag: '"first"', list: listing(readyRows(30)) });
    void board.column("ready", false, "", 1);
    await pass(250);
    expect(asked.slice(1)).toEqual([
      [searched(""), ""],
      [column("ready"), '"first"'],
    ]);
    expect(d.long.get("column:ready")?.etag).toBe('"found"');
  });

  it("keeps the long answer it held when a long read fails or a newer view replaces it", async () => {
    const held = later<Response>();
    let failing = true;
    const { board, d, asked } = live(() =>
      failing ? answer(500, { error: "unavailable" }) : held.promise,
    );
    d.waiting = false;
    d.tasks = readyRows(31);
    const kept = { etag: '"long"', list: listing(readyRows(31)) };
    d.long.set("column:ready", kept);
    void board.column("ready", false, "", 1);
    await pass(250);
    expect(board.fresh.failing).toEqual(["column:ready"]);
    expect(d.long.get("column:ready")).toBe(kept);
    failing = false;
    void board.column("ready", false, "", 1);
    await pass(250);
    d.view += 1;
    held.settle(answer(200, listing(readyRows(31, 2)), '"dropped"'));
    await pass();
    expect(asked.map(([, etag]) => etag)).toEqual(['"long"', '"long"']);
    expect(d.long.get("column:ready")).toBe(kept);
  });

  it("counts the rows it shows: its own stage's tasks as the stages place them, no subtask", async () => {
    const { board, d, asked } = opened({});
    const progress = (count: number) =>
      Array.from({ length: count }, (_, at) => aTask(`w/p-${at + 1}`, { status: "in_progress" }));
    d.tasks = [
      ...readyRows(31),
      aTask("w/sub", { parent: "w/r-1" }),
      aTask("w/review", { status: "review" }),
      ...progress(33),
    ];
    // The machine builds one task in progress: the board draws it in Build.
    d.work = building("w/p-33");
    expect(board.shownRows("ready")).toBe(31);
    expect(board.shownRows("in_progress")).toBe(32);
    expect(board.shownRows("build")).toBe(1);
    expect(board.shownRows("review")).toBe(1);
    expect(board.shownRows("done")).toBe(0);
    void board.column("in_progress", false, "", 1);
    await pass(250);
    expect(urls(asked)).toEqual([column("in_progress", "&rows=32")]);
    // With the feed lost, the stages are the tasks' statuses alone.
    d.connected = false;
    expect(board.shownRows("in_progress")).toBe(33);
    expect(board.shownRows("build")).toBe(0);
  });

  it("showing a page or less reads its first page as the device keeps it, its copy named", async () => {
    const page = listing(readyRows(30), [["ready", 30]]);
    const { board, d, asked, store } = opened({ [column("ready")]: page });
    d.tasks = readyRows(30);
    d.copies.set("column:ready", { etag: '"first"', list: page });
    void board.column("ready", false, "", 1);
    await pass(250);
    expect(asked).toEqual([[column("ready"), '"first"']]);
    expect(store.keep).toHaveBeenCalled();
  });

  it("asks Load more for its next page alone, however many rows it shows", async () => {
    const { board, d, asked } = opened({
      [column("ready", "&cursor=w%2Fr-45")]: listing(readyRows(5, 46), [["ready", 50]]),
    });
    d.tasks = readyRows(45);
    d.cursors.set("ready", "w/r-45");
    void board.column("ready", true, "", 1);
    await pass(250);
    expect(urls(asked)).toEqual([column("ready", "&cursor=w%2Fr-45")]);
    expect(d.tasks).toHaveLength(50);
  });

  it("loaded past its first page keeps its rows over a feed event's refresh", async () => {
    const pages: Record<string, unknown> = {
      [column("ready")]: listing(readyRows(30), [["ready", 40]], { next: "w/r-30" }),
      [column("ready", "&cursor=w%2Fr-30")]: listing(readyRows(10, 31), [["ready", 40]], {
        next: "w/r-40",
      }),
    };
    const { board, d, asked } = opened(pages);
    void board.column("ready", false, "", 1);
    await pass(250);
    void board.column("ready", true, "", 1);
    await pass(250);
    expect(d.tasks).toHaveLength(40);
    // A task filed at the column's top: the feed adds it, its counts ask for a refresh, and the
    // refresh reads every row again.
    const filed = aTask("w/filed", { priority: 0 });
    expect(
      board.apply(event(["task", { key: "w/filed", value: filed }], ["task-counts", {}])),
    ).toBe(true);
    expect(d.cursors.has("ready")).toBe(false);
    pages[column("ready", "&rows=41")] = listing([filed, ...readyRows(40)], [["ready", 41]], {
      next: "w/r-40",
    });
    void board.column("ready", false, "", 1);
    await pass(250);
    expect(urls(asked).at(-1)).toBe(column("ready", "&rows=41"));
    expect(d.tasks.map((task) => task.slug)).toEqual([
      "w/filed",
      ...readyRows(40).map((task) => task.slug),
    ]);
    expect(d.cursors.get("ready")).toBe("w/r-40");
  });
});

describe("a search of the live board", () => {
  it("retries the new facet's counts after the initial check ended and clears its alert", async () => {
    let failing = true;
    const { board, d, asked, page } = live(() =>
      failing ? answer(503) : answer(200, listing([], [["ready", 1]])),
    );
    d.counted = 2;
    d.worked = 2;
    void board.filter("", 1, "nobody");
    await pass(250);
    expect(d.error).toContain("Trying again");
    expect(board.fresh.failing).toContain("counts");
    failing = false;
    await pass(1000);
    expect(d.counted).toBe(3);
    expect(d.error).toBe("");
    expect(board.fresh.failing).toEqual([]);
    expect(urls(asked)).toEqual(
      Array(2).fill(
        "/api/task-board?view=waiting&wait=nobody&counts=1&q=&days=1&activity=1&work=0",
      ),
    );
    await pass(4000);
    expect(asked).toHaveLength(2);
    page.abort();
  });

  it("ends a failed filter's retry when a later view replaces it or the page leaves", async () => {
    const { board, asked, page } = live(() => answer(503));
    void board.filter("old", 1, "check");
    await pass(250);
    void board.filter("new", 1, "queued");
    await pass(250);
    await pass(1000);
    expect(urls(asked).filter((url) => url.includes("q=old"))).toHaveLength(1);
    expect(urls(asked).filter((url) => url.includes("q=new"))).toHaveLength(2);
    page.abort();
    await pass(4000);
    expect(asked).toHaveLength(3);
  });

  it("clears what the page showed and reads the counts once the typing stops", async () => {
    const { board, d, asked, host } = live(() => answer(200, listing([])));
    d.tasks = [aTask("w/a")];
    d.drawn = ["ready"];
    board.fresh = { ...board.fresh, saved: 1, unanswered: ["column:ready"] };
    void board.filter("l", 1);
    expect(d.tasks).toEqual([]);
    expect(d.drawn).toEqual([]);
    expect(board.fresh.unanswered).toEqual([]);
    expect(host.redraw).toHaveBeenCalled();
    await pass(100);
    void board.filter("lamp", 7);
    await pass(250);
    expect(urls(asked)).toEqual([
      "/api/task-board?view=waiting&counts=1&q=lamp&days=7&activity=1&work=0",
    ]);
    expect(d.query).toBe("lamp");
    expect(d.days).toBe(7);
  });

  it("of the window already chosen reads the board again, its columns first", async () => {
    const { board, d, asked, host } = live(() =>
      answer(200, listing([aTask("w/a")], [["ready", 1]])),
    );
    d.waiting = false;
    d.tasks = [aTask("w/a")];
    void board.filter("", 1);
    expect(d.tasks).toEqual([]);
    // Each column shown asks for its page again, as a new search or window has it do.
    expect(host.refresh).toHaveBeenCalledTimes(1);
    void board.column("ready", false, "", 1);
    await pass(250);
    // Both after their 250 ms: the counts' pause for the next key, the column's own.
    expect(urls(asked).sort()).toEqual([COUNTS_URL, column("ready")]);
    expect(d.tasks.map((task) => task.slug)).toEqual(["w/a"]);
  });
});

describe("what the live board gives its columns", () => {
  it("is one function for each ask, whatever is drawn again", async () => {
    const { board, d, asked } = live(() => answer(200, listing([aTask("w/a")], [["ready", 1]])));
    d.waiting = false;
    const { load, search } = board;
    board.apply(event(["task", { key: "w/b", value: aTask("w/b") }]));
    expect(board.load).toBe(load);
    expect(board.search).toBe(search);
    load("ready", false, "", 1);
    await pass(250);
    expect(urls(asked)).toEqual([column("ready")]);
    search("lamp", 7);
    expect(d.query).toBe("lamp");
    expect(d.days).toBe(7);
  });

  it("reads a column asked again within its rest once, the last ask's, and another column beside it", async () => {
    const { board, d, asked } = live(() => answer(200, listing([aTask("w/a")], [["ready", 1]])));
    d.waiting = false;
    board.load("ready", false, "", 1);
    await pass(200);
    board.load("ready", false, "", 1);
    board.load("done", false, "", 1);
    // The first ask's rest is over, its timer with it.
    expect(vi.getTimerCount()).toBe(2);
    await pass(200);
    expect(urls(asked)).toEqual([]);
    await pass(50);
    expect(urls(asked).sort()).toEqual([column("done"), column("ready")]);
  });

  it("after one card's update, is the other columns' tasks as they were", () => {
    const { board, d } = live();
    d.tasks = [aTask("w/a"), aTask("w/b"), aTask("w/c", { status: "review" })];
    const place = (task: Task) => task.status;
    const before = byColumn(d.tasks, place, new Map());
    board.apply(event(["task-fields", { key: "w/b", fields: { title: "Renamed" } }]));
    const after = byColumn(d.tasks, place, before);
    // Only the renamed card's column changed, and in it only that card.
    expect(after.get("review")).toBe(before.get("review"));
    expect(after.get("ready")).not.toBe(before.get("ready"));
    expect(after.get("ready")?.[0]).toBe(before.get("ready")?.[0]);
    expect(after.get("ready")?.[1]?.title).toBe("Renamed");
  });
});

describe("a feed event on the live board", () => {
  it("is applied once read, nothing for one it cannot read", () => {
    const { board, d, held, host } = live();
    expect(board.apply(event(["task", { value: 1 }]))).toBeNull();
    expect(d.journal.version).toBe(0);
    expect(host.redraw).not.toHaveBeenCalled();
    expect(board.apply(event(["task", { key: "w/a", value: aTask("w/a") }]))).toBe(false);
    expect(d.tasks.map((task) => task.slug)).toEqual(["w/a"]);
    expect(held).toHaveBeenCalledWith("[data-slug]");
    expect(host.redraw).toHaveBeenCalled();
  });

  it("holds the reader's place, then draws the change at once, within the event", () => {
    const { board, held, host } = live();
    const order: string[] = [];
    held.mockImplementation(() => order.push("held"));
    host.redraw.mockImplementation(() => order.push(react.flushing ? "drawn" : "drawn later"));
    board.apply(event(["task", { key: "w/a", value: aTask("w/a") }]));
    // A refresh asked for, with nothing the page draws changed, holds and draws alike.
    board.apply(event(["task-counts", {}]));
    expect(order).toEqual(["held", "drawn", "held", "drawn"]);
    expect(react.flushing).toBe(false);
  });

  it("moves no count for the machines' heartbeat, only for work placed elsewhere", () => {
    const { board, d } = live();
    d.cursors.set("ready", "w/a");
    expect(board.apply(event(["machine-work", building("w/a")]))).toBe(true);
    expect(d.cursors.size).toBe(0);
    const generation = d.generation;
    const drawn = d.work;
    const beat = building("w/a");
    beat.machines[0] = {
      ...(beat.machines[0] as MachineWork["machines"][0]),
      sequence: 2,
      age_ms: 900,
    };
    expect(board.apply(event(["machine-work", beat]))).toBe(false);
    expect(d.generation).toBe(generation);
    // The heartbeat changes nothing the page draws: the board keeps the work it drew.
    expect(d.work).toBe(drawn);
    expect(d.worked).toBe(2);
    expect(board.apply(event(["machine-work", building("w/a", "queued")]))).toBe(true);
    expect(d.work).toEqual(building("w/a", "queued"));
  });

  it("draws nothing again for a heartbeat, and again for work the page shows changed", () => {
    const { board, d, host } = live();
    board.apply(event(["machine-work", building("w/a")]));
    host.redraw.mockClear();
    const beat = building("w/a");
    beat.machines[0] = { ...(beat.machines[0] as MachineWork["machines"][0]), sequence: 7 };
    board.apply(event(["machine-work", beat]));
    expect(host.redraw).not.toHaveBeenCalled();
    const stale = building("w/a");
    stale.machines[0] = {
      ...(stale.machines[0] as MachineWork["machines"][0]),
      freshness: "stale",
    };
    board.apply(event(["machine-work", stale]));
    expect(host.redraw).toHaveBeenCalledTimes(1);
    expect(d.work).toEqual(stale);
  });

  it("drops a task finished before the window, but not while searching", () => {
    const { board, d } = live();
    const old = (slug: string, status: string) =>
      aTask(slug, { status, updated: NOW / 1000 - 86_400 - 1 });
    d.tasks = [aTask("w/a"), aTask("w/b")];
    d.fromDevice = ["w/a"];
    board.apply(event(["task", { key: "w/a", value: old("w/a", "done") }]));
    board.apply(event(["task", { key: "w/b", value: old("w/b", "cancelled") }]));
    board.apply(event(["task", { key: "w/c", value: old("w/c", "review") }]));
    expect(d.tasks.map((task) => task.slug)).toEqual(["w/c"]);
    expect(d.fromDevice).toEqual([]);
    d.days = 7;
    board.apply(event(["task", { key: "w/d", value: old("w/d", "done") }]));
    expect(d.tasks.map((task) => task.slug)).toEqual(["w/c", "w/d"]);
    d.query = "lamp";
    expect(board.apply(event(["task", { key: "w/e", value: aTask("w/e") }]))).toBe(true);
    expect(d.tasks.map((task) => task.slug)).toEqual(["w/c", "w/d"]);
  });

  it("lands a task's fields in it, reading again for a status or a parent", () => {
    const { board, d } = live();
    d.tasks = [aTask("w/a"), aTask("w/b")];
    expect(board.apply(event(["task-fields", { key: "w/a", fields: { title: "Renamed" } }]))).toBe(
      false,
    );
    expect(d.tasks[0]?.title).toBe("Renamed");
    expect(d.tasks[1]).toEqual(aTask("w/b"));
    expect(board.apply(event(["task-fields", { key: "w/x", fields: { status: "done" } }]))).toBe(
      true,
    );
    expect(board.apply(event(["task-fields", { key: "w/b", fields: { parent: "w" } }]))).toBe(true);
    expect(d.tasks[1]?.parent).toBe("w");
    d.query = "x";
    expect(board.apply(event(["task-fields", { key: "w/a", fields: { title: "Again" } }]))).toBe(
      true,
    );
  });

  it("says a holder's health on every role it holds", () => {
    const { board, d, host } = live();
    d.tasks = [
      aTask("w/a", {
        roles: [{ handle: "amber", role: "executor", since: 0, expires: null, health: "live" }],
      }),
    ];
    board.apply(event(["session", { key: "amber", value: { health: "stale" } }]));
    expect(d.tasks[0]?.roles[0]?.health).toBe("stale");
    host.redraw.mockClear();
    board.apply(event(["session", { key: "cedar", value: { health: "stale" } }]));
    expect(host.redraw).toHaveBeenCalledTimes(1); // refresh the complete orchestrator metadata
  });

  it("reads again for a message or the counts only where they move the view", () => {
    const { board, d } = live();
    expect(board.apply(event(["message", {}]))).toBe(false);
    expect(board.apply(event(["task-counts", {}]))).toBe(true);
    expect(board.apply(event(["task-recent-counts", {}]))).toBe(true);
    d.query = "lamp";
    expect(board.apply(event(["message", {}]))).toBe(true);
  });

  it("says when the board is unavailable, and reads again once it is back", () => {
    const { board, d, host } = live();
    expect(board.apply(event(["tasks-status", { available: false, slow: false }]))).toBe(false);
    expect(d.error).toBe("The task board is unavailable. Reconnecting…");
    host.redraw.mockClear();
    board.apply(event(["tasks-status", { available: false, slow: false }]));
    expect(host.redraw).not.toHaveBeenCalled();
    expect(board.apply(event(["tasks-status", { available: true }]))).toBe(true);
    expect(d.error).toBe("");
  });

  it("is kept for the reads still out, and only for them", () => {
    const { board, d } = live();
    board.apply(event(["message", {}]));
    expect(d.journal.events).toEqual([]);
    const cursor = new ReadCursor(d.journal);
    const said = event(["tasks-status", { available: false, slow: false }]);
    board.apply(said);
    expect(cursor.events()).toEqual([said]);
    d.error = "";
    board.replay(cursor);
    expect(d.error).toBe("The task board is unavailable. Reconnecting…");
    cursor.release();
  });

  it("heard: answers for the machines' work, and loses the stream when unreadable", () => {
    const { board } = live();
    board.fresh = { ...board.fresh, saved: 1, unanswered: ["work"] };
    expect(board.heard(event(["machine-work", NO_WORK]))).toBe(true);
    expect(board.fresh.unanswered).toEqual([]);
    expect(board.heard(event(["machine-work", 1]))).toBe(false);
  });

  it("heard: reads the counts again after a pause, then each column, until no event moved them meanwhile", async () => {
    const held: ReturnType<typeof later<Response>>[] = [];
    const { board, asked, host } = live(() => {
      const read = later<Response>();
      held.push(read);
      return read.promise;
    });
    board.heard(event(["task-counts", {}]));
    board.heard(event(["task-counts", {}]));
    await pass(749);
    expect(asked).toEqual([]);
    await pass(1);
    expect(urls(asked)).toEqual([COUNTS_URL]);
    // Another event moves the counts while they are read: they are read again.
    board.heard(event(["task-counts", {}]));
    held[0]?.settle(answer(200, listing([])));
    await pass();
    expect(host.refresh).not.toHaveBeenCalled();
    await pass(750);
    expect(urls(asked)).toEqual([COUNTS_URL, COUNTS_URL]);
    held[1]?.settle(answer(200, listing([])));
    await pass();
    expect(host.refresh).toHaveBeenCalledTimes(1);
    // The refresh is over: the next event starts another.
    board.heard(event(["task-counts", {}]));
    await pass(750);
    expect(asked).toHaveLength(3);
  });
});

describe("the live board's feed", () => {
  it("lost, places stages by status alone and counts by lifecycle", () => {
    const { board, d, host } = live();
    d.counts = [["build", 1]];
    d.lifecycleCounts = [["in_progress", 1]];
    d.cursors.set("ready", "w/a");
    board.setStale(true);
    expect(host.stale).toHaveBeenCalledWith(true);
    expect(d.connected).toBe(false);
    expect(d.counts).toEqual([["in_progress", 1]]);
    expect(d.cursors.size).toBe(0);
    expect(host.refresh).toHaveBeenCalledTimes(1);
    d.lifecycleCounts = [];
    board.setStale(true);
    expect(d.counts).toEqual([["in_progress", 1]]);
  });

  it("open again, reads the counts by the machines' activity, then each column", async () => {
    const { board, d, asked, host } = live(() => answer(200, listing([])));
    board.setStale(false);
    expect(asked).toEqual([]);
    d.connected = false;
    const view = d.view;
    board.setStale(false);
    expect(d.connected).toBe(true);
    expect(d.view).toBe(view + 1);
    await pass();
    expect(urls(asked)).toEqual([COUNTS_URL]);
    expect(host.refresh).toHaveBeenCalledTimes(1);
    expect(host.stale).toHaveBeenCalledWith(false);
  });
});

describe("the live board's check", () => {
  it("asks again for what failed, waiting longer each time, until it answers", async () => {
    let tries = 0;
    const { board, d, asked } = live((url) => {
      if (url === "/api/machine-work") return answer(200, NO_WORK);
      tries += 1;
      return tries < 3 ? answer(503) : answer(200, listing([], [["ready", 1]]));
    });
    await board.check();
    expect(urls(asked)).toEqual(["/api/machine-work", COUNTS_URL]);
    expect(d.counted).toBe(0);
    // Said as a read the page tries again, which it is.
    expect(d.error).toBe("The server answered 503. Trying again…");
    await pass(1000);
    expect(tries).toBe(2);
    await pass(1999);
    expect(tries).toBe(2);
    await pass(1);
    expect(tries).toBe(3);
    expect(d.counted).toBe(1);
    expect(d.error).toBe("");
    await pass(60_000);
    expect(tries).toBe(3);
  });

  it("stops asking once another check starts, or the page goes", async () => {
    const { board, asked, page } = live((url) =>
      url === "/api/machine-work" ? answer(200, NO_WORK) : answer(503),
    );
    await board.check();
    board.d.round += 1;
    await pass(5_000);
    expect(urls(asked).filter((url) => url === COUNTS_URL)).toHaveLength(1);
    const again = live(() => answer(503));
    await again.board.check();
    again.page.abort();
    await pass(5_000);
    expect(again.asked).toHaveLength(2);
    expect(page.signal.aborted).toBe(false);
  });
});

describe("the board's loop on the live board", () => {
  it("says why the board could not be read once what the device kept is drawn", async () => {
    const { board, d, host } = live();
    const run = board.run({ refresh: vi.fn(), connected: vi.fn(), heard: vi.fn() });
    const kept = later<void>();
    board.kept = kept.promise;
    run.setTrouble("The server could not be reached");
    await pass();
    expect(d.error).toBe("");
    kept.settle();
    await pass();
    expect(d.error).toBe("The server could not be reached");
    expect(host.redraw).toHaveBeenCalled();
    run.show({ version: 3, ping_ms: 1, build: "" });
    expect(run.board.current?.version).toBe(3);
    run.setStale(true);
    expect(host.stale).toHaveBeenCalledWith(true);
    run.views.freshen((f) => ({ ...f, open: true }));
    expect(board.fresh.open).toBe(true);
    expect(run.views.heard(event(["message", {}]))).toBe(true);
    await run.views.read();
    expect(d.waiting).toBe(false);
  });

  it("does nothing more once the page is gone", async () => {
    const { board, d, page, host } = live();
    const run = board.run({ refresh: vi.fn(), connected: vi.fn(), heard: vi.fn() });
    page.abort();
    run.setTrouble("gone");
    await board.read();
    await pass();
    expect(d.error).toBe("");
    expect(d.waiting).toBe(true);
    expect(host.refresh).not.toHaveBeenCalled();
  });
});

describe("the task board's page", () => {
  it("opens on the board's ghost under its heading, saying it loads", () => {
    vi.stubGlobal("innerWidth", 1280);
    vi.stubGlobal("localStorage", memoryStorage());
    vi.stubGlobal("document", { getElementById: () => null });
    const html = renderToStaticMarkup(<TasksPage scope="" />);
    expect(html).toMatch(/^<main data-ready="board" data-feed="live" aria-busy="true">/);
    expect(html).toContain('<span class="status-text" role="status">Loading tasks…</span>');
    // The heading alone: the header's Agents link leads where its Who is connected did.
    expect(html).toContain('<div class="page-head"><h1>Board</h1></div>');
    expect(html).toContain('class="ghost-kanban waiting-columns"');
    expect(html).toContain('class="stale page-notices notice-stack" hidden=""');
  });

  /** The page as `board` stands, live or not, with what this device shows. */
  const view = (board: LiveBoard, stale = false, device: number | null = null) =>
    renderToStaticMarkup(
      <TasksView
        scope=""
        board={board}
        stale={stale}
        freshness={device === null ? board.fresh : { ...board.fresh, saved: device }}
        refreshed={0}
      />,
    );

  it("draws the board once its counts are in, its notices saying what the feed does", () => {
    const { board, d } = live();
    vi.stubGlobal("innerWidth", 1280);
    d.ready = true;
    d.counts = [["ready", 2]];
    let html = view(board);
    expect(html).toMatch(/^<main data-ready="board" data-feed="live" aria-busy="false">/);
    expect(html).not.toContain("Loading tasks…");
    expect(html).toContain(
      '<section aria-label="Task columns" tabindex="0" class="kanban waiting-columns">',
    );
    expect(html).toContain('<span class="count">2</span>');
    html = view(board, true);
    expect(html).toContain('data-feed="stale"');
    expect(html).toContain("Reconnecting: the board may be out of date");
    d.error = "The task board is unavailable. Reconnecting…";
    expect(view(board)).toContain('role="alert">The task board is unavailable. Reconnecting…</p>');
    html = view(board, false, NOW);
    expect(html).toContain('data-feed="stale" data-cache="device"');
    expect(html).toContain("Shown from this device");
  });

  it("says why the board could not be read in place of its ghost, once", () => {
    const { board, d } = live();
    vi.stubGlobal("innerWidth", 1280);
    d.error = "The server could not be reached";
    const html = view(board);
    expect(html).toMatch(/^<main data-ready="board" data-feed="live" aria-busy="false">/);
    expect(html).toContain("The task board could not be read");
    expect(html).not.toContain("ghost-kanban");
    expect(html).not.toContain('role="alert"');
  });

  it("opens a phone's ghost with the row of work under way the board it kept draws", () => {
    vi.stubGlobal("innerWidth", 390);
    const storage = memoryStorage();
    vi.stubGlobal("localStorage", storage);
    vi.stubGlobal("document", { getElementById: () => null });
    storage.setItem(SHAPE, "ready 2");
    expect(renderToStaticMarkup(<TasksPage scope="" />)).toContain('<div class="under-way"></div>');
    storage.setItem(SHAPE, "ready 2\nworkflow");
    const held = renderToStaticMarkup(<TasksPage scope="" />);
    expect(held).not.toContain("under-way");
    expect(held).toContain('<nav class="wait-strip"');
  });

  it("opens on the shape this browser kept of the board", () => {
    vi.stubGlobal("innerWidth", 1280);
    const storage = memoryStorage();
    storage.setItem(SHAPE, "signoff 1\nready 2");
    vi.stubGlobal("localStorage", storage);
    vi.stubGlobal("document", { getElementById: () => null });
    const html = renderToStaticMarkup(<TasksPage scope="" />);
    expect([...html.matchAll(/data-for="([^"]*)"/g)].map((m) => m[1])).toEqual([
      "concept",
      "design",
      "ready",
      "in_progress",
      "build",
      "test",
      "review",
      "done",
    ]);
  });
});

describe("the complete waiting projection", () => {
  it("reads the chosen wait across stages, pages it, and never overwrites the default cache", async () => {
    const first = aTask("a", {
      status: "blocked",
      from: "review",
      waits_on: { kind: "check", who: "lint", why: "needed", since: 1 },
    });
    const second = aTask("b", {
      status: "blocked",
      from: "ready",
      waits_on: { kind: "check", who: "lint", why: "needed", since: 2 },
    });
    const { board, d, asked, store } = live((url) =>
      answer(
        200,
        listing(
          url.includes("cursor=") ? [second] : [first],
          [
            ["review", 1],
            ["ready", 1],
          ],
          {
            next: url.includes("cursor=") ? null : "second",
            waiting: {
              kinds: [
                ["check", 2],
                ["queued", 9],
              ],
              columns: [
                { status: "review", ordinary: 0, blocked: 1, oldest: 1 },
                { status: "ready", ordinary: 0, blocked: 1, oldest: 2 },
              ],
            },
            orchestrators: [{ handle: "orch", health: "stale" }],
            unstarted: 9,
          },
        ),
      ),
    );
    d.waiting = false;
    void board.filter("", 1, "check");
    await pass(250);
    expect(d.wait).toBe("check");
    expect(d.projection?.kinds).toEqual([
      ["check", 2],
      ["queued", 9],
    ]);
    expect(d.orchestrators).toEqual([{ handle: "orch", health: "stale" }]);
    expect(d.unstarted).toBe(9);
    void board.column("waiting", false, "", 1);
    await pass(250);
    void board.column("waiting", true, "", 1);
    await pass(250);
    expect(d.tasks.map((t) => t.slug)).toEqual(["a", "b"]);
    expect(asked.every(([url]) => url.includes("view=waiting&wait=check"))).toBe(true);
    expect(asked.some(([url]) => url.includes("status="))).toBe(false);
    expect(asked.at(-1)?.[0]).toContain("cursor=second");
    expect(store.keep).not.toHaveBeenCalled();
  });
  it("replaces a blocked row's original column and asks a long refresh for every shown row", () => {
    const { board, d } = live();
    d.tasks = [aTask("blocked", { status: "blocked", from: "review" }), aTask("ready")];
    board.showColumn("review", false, listing([aTask("new", { status: "review" })]));
    expect(d.tasks.map((t) => t.slug)).toEqual(["ready", "new"]);
    d.wait = "check";
    expect(board.shownRows("waiting")).toBe(2);
    board.showColumn("waiting", false, listing([aTask("only")]));
    expect(d.tasks.map((t) => t.slug)).toEqual(["only"]);
  });
  it("refreshes complete facets when a wait or original stage changes", () => {
    const { board } = live();
    expect(
      board.apply(
        event([
          "task-fields",
          {
            key: "a",
            fields: { waits_on: { kind: "check", who: "lint", why: "needed", since: 1 } },
          },
        ]),
      ),
    ).toBe(true);
    expect(board.apply(event(["task-fields", { key: "a", fields: { from: "review" } }]))).toBe(
      true,
    );
  });
});
