import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FRESH, opened, PAGE, THREAD, waits } from "./cache";
import type { FrameRefresh } from "./frame";
import { ANSWER_MS } from "./request";
import {
  boardNotice,
  emptyTask,
  gatesBranch,
  LiveTask,
  linksCard,
  newest,
  pageFields,
  readChanges,
  type TaskHost,
  withEntries,
} from "./task-live";
import { aRole, aTask, aWork, CASES, feeds, memoryStorage, say } from "./testing";
import { NOT_IN_TIME } from "./trouble";
import {
  BOARD_PATH,
  FEED_EVENT,
  type Task,
  type TaskChange,
  type TaskEntry,
  type TaskPage,
} from "./wire";
import { NO_WORK } from "./work";

/** A response as the page reads one: its status, its tag and its JSON body. */
function answer(status: number, body: unknown = null, etag = ""): Response {
  return {
    status,
    ok: status >= 200 && status < 300,
    headers: { get: (name: string) => (name === "ETag" && etag !== "" ? etag : null) },
    json: async () => body,
  } as unknown as Response;
}

/** The page of `task`, listing `children` of `count`, and whatever else `more` says. */
function aPage(
  task: Task = aTask("w/a"),
  children: Task[] = [],
  more: Partial<TaskPage> = {},
): TaskPage {
  return {
    machine_work: NO_WORK,
    task,
    blocks: [],
    children,
    children_count: children.length,
    brief_omitted: 0,
    brief_next: 0,
    brief_revision: "",
    sessions: {},
    local: false,
    gates: null,
    ...more,
  };
}

/** Message `id` of `slug`'s thread, by `sender`, of `kind`. */
function anEntry(id: number, slug = "w/a", sender = "owner", kind = "message"): TaskEntry {
  return {
    message: { id, slug, sender, recipient: null, kind, text: `Said ${id}`, ts: id },
    stamp: "",
    blocks: [],
  };
}

const ids = (entries: readonly TaskEntry[]) => entries.map((entry) => entry.message.id);

/** A thread's page of `entries`, with the cursors before and after it. */
const history = (entries: TaskEntry[], before: number | null = null, after: number | null = null) =>
  ({ entries, before, after }) as const;

/** A listing of `tasks`, counted as `counts`, its next page at `next`. */
const listing = (tasks: Task[], counts: [string, number][], next: string | null = null) => ({
  machine_work: NO_WORK,
  tasks,
  statuses: [],
  signoff: [],
  counts,
  lifecycle_counts: [],
  next,
});

const change = (key: string, fields: TaskChange["fields"]): TaskChange => ({ key, fields });

/** A feed event of `changes`, each a region and its data. */
const event = (...changes: [string, unknown][]) => ({
  version: 1,
  changes: changes.map(([region, data]) => ({ region, data })),
});

const PAGE_URL = "/api/task-page?slug=w%2Fa";
const THREAD_URL = "/api/task-history?slug=w%2Fa";
const OTHER_URL = "/api/task-page?slug=w%2Fb";

type Route = (url: string, etag: string) => Response | Promise<Response>;

interface Kept {
  value: unknown;
  etag: string;
  saved: number;
}

/**
 * The live page of `w/a` over a server that answers each read as `route` does, on a page whose
 * device kept `kept` (by the item's id and part, as `task:w/a page`). What it asked, the host's
 * calls and the store's.
 */
function live(route: Route = () => answer(503), kept: Record<string, Kept> = {}) {
  const asked: [string, string][] = [];
  /** Each read's url and the signal that gives its request up, in the order they were asked. */
  const given: [string, AbortSignal | null][] = [];
  const held = vi.fn();
  vi.stubGlobal("document", { getElementById: () => null, querySelector: () => null });
  vi.stubGlobal("window", { reviewHoldPlace: held });
  vi.stubGlobal("localStorage", memoryStorage());
  vi.stubGlobal("fetch", async (url: string, init?: RequestInit) => {
    const etag = (init?.headers as Record<string, string> | undefined)?.["If-None-Match"] ?? "";
    asked.push([url, etag]);
    given.push([url, init?.signal ?? null]);
    return route(url, etag);
  });
  const store = {
    read: vi.fn(
      async (_format: number, _project: string, id: string, part: string) => kept[`${id} ${part}`],
    ),
    keep: vi.fn(async () => undefined),
    visit: vi.fn(async () => undefined),
    forget: vi.fn(async () => undefined),
  };
  vi.stubGlobal("symphonyCache", store);
  const host = { redraw: vi.fn(), freshness: vi.fn(), stale: vi.fn() } satisfies TaskHost;
  const page = new AbortController();
  const task = new LiveTask("", "w/a", host, page.signal);
  return { task, d: task.d, asked, given, host, store, held, page };
}

/** The urls asked, without their tags. */
const urls = (asked: [string, string][]) => asked.map(([url]) => url);

/** What the store was asked to keep: each item's id, part, tag and value. */
const keptBy = (store: ReturnType<typeof live>["store"]) =>
  store.keep.mock.calls.map((call) => {
    const [, , id, , part, etag, value] = call as unknown as [
      number,
      string,
      string,
      string,
      string,
      string,
      unknown,
    ];
    return [id, part, etag, value];
  });

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

describe("a changed task's fields on a task's page", () => {
  /** A page over `w/a` listing its subtask `w/a/1`, of two. */
  const page = () =>
    aPage(aTask("w/a"), [aTask("w/a/1", { parent: "w/a" })], { children_count: 2 });

  it("go into its task, read again only for a new brief or status", () => {
    const [titled, done] = pageFields(page(), change("w/a", { title: "Renamed" }));
    expect(done).toEqual({ reload: false, childCame: false });
    expect(titled.task.title).toBe("Renamed");
    const [moved, again] = pageFields(titled, change("w/a", { status: "review" }));
    expect(again).toEqual({ reload: true, childCame: false });
    expect(moved.task.status).toBe("review");
    expect(pageFields(page(), change("w/a", { body: "New" }))[1].reload).toBe(true);
  });

  it("go into a subtask it lists, whose status moves no count", () => {
    const [next, done] = pageFields(
      page(),
      change("w/a/1", { status: "in_progress", title: "First" }),
    );
    expect(done).toEqual({ reload: false, childCame: false });
    expect([next.children[0]?.title, next.children[0]?.status]).toEqual(["First", "in_progress"]);
  });

  it("change nothing for a task the page does not show", () => {
    const before = page();
    const [next, done] = pageFields(before, change("w/b", { title: "Other" }));
    expect(done).toEqual({ reload: false, childCame: false });
    expect(next).toBe(before);
  });

  it("take a subtask that leaves from the list, and read one that comes with the list again", () => {
    const [left, done] = pageFields(page(), change("w/a/1", { parent: "w/b" }));
    expect(done).toEqual({ reload: true, childCame: false });
    expect(left.children).toEqual([]);
    const [came, again] = pageFields(left, change("w/c", { parent: "w/a" }));
    expect(again).toEqual({ reload: true, childCame: true });
    expect(came.children).toEqual([]);
    expect(pageFields(came, change("w/d", { parent: "" }))[1]).toEqual({
      reload: false,
      childCame: false,
    });
  });

  it("keep a subtask that stays under the task", () => {
    const [kept, done] = pageFields(page(), change("w/a/1", { parent: "w/a", title: "Same" }));
    expect(done).toEqual({ reload: false, childCame: false });
    expect(kept.children.map((child) => child.title)).toEqual(["Same"]);
  });
});

describe("a card the feed names", () => {
  it("counts for a task that links its document, whatever part either names", () => {
    const links = ["docs/analysis/x.md#part", "docs/B.md"];
    for (const [key, linked] of [
      ["/docs/analysis/x.md#card", true],
      ["docs/analysis/x.md#other", true],
      ["/docs/B.md#c", true],
      ["/docs/B.md", true],
      ["/docs/analysis/y.md#card", false],
      ["/docs/analysis/x.md.bak#card", false],
      ["/docs/analysis", false],
      ["#card", false],
      ["", false],
    ] as const) {
      expect(linksCard(links, key), key).toBe(linked);
    }
    expect(linksCard([], "/docs/B.md#c")).toBe(false);
    expect(linksCard([""], "#card"), "an empty path is no document").toBe(false);
  });
});

describe("the branch a task's gates follow", () => {
  it("is the one they were answered for, else the task's slug", () => {
    expect(gatesBranch("tools/x", ["tools/x", "feature/x"])).toBe("feature/x");
    expect(gatesBranch("tools/y", ["tools/x", "feature/x"])).toBe("tools/y");
    expect(gatesBranch("tools/x", null)).toBe("tools/x");
  });
});

describe("a thread's newest page answered", () => {
  it("replaces what this device drew and keeps what the feed brought meanwhile, in id order", () => {
    const shown = [anEntry(1), anEntry(2), anEntry(9)];
    const answered = [anEntry(2), anEntry(3)];
    expect(ids(newest(shown, [1, 2], answered))).toEqual([2, 3, 9]);
    expect(newest(shown, [1, 2], answered)[0]).toBe(answered[0]);
  });

  it("puts a message once though the feed brought it too", () => {
    const fed = { ...anEntry(4), stamp: "fed" };
    const read = { ...anEntry(4), stamp: "read" };
    expect(newest([fed], [], [read])).toEqual([read]);
  });

  it("is the server's page alone when nothing is shown", () => {
    expect(ids(newest([], [], [anEntry(5), anEntry(3)]))).toEqual([3, 5]);
  });
});

describe("a thread's page added to what is shown", () => {
  it("adds each message it lacks, in id order, keeping those it has", () => {
    const shown = [anEntry(3), anEntry(5)];
    const added = withEntries(shown, [{ ...anEntry(3), stamp: "again" }, anEntry(1), anEntry(4)]);
    expect(ids(added)).toEqual([1, 3, 4, 5]);
    expect(added[1]).toBe(shown[0]);
  });
});

describe("what the board says of itself", () => {
  it("is nothing while it is available or says nothing, else slow or unavailable", () => {
    expect(boardNotice(true, true)).toBe("");
    expect(boardNotice(undefined, true)).toBe("");
    expect(boardNotice(false, true)).toBe("The task board is slow to answer. Retrying…");
    expect(boardNotice(false, false)).toBe("The task board is unavailable. Reconnecting…");
  });
});

describe("a feed event's changes", () => {
  it("are read whole, each region the page follows, the rest passed over", () => {
    const changes = readChanges(
      event(
        ["task", CASES.samples.taskAdded],
        ["task-fields", CASES.samples.taskChange],
        ["session", { key: "amber", value: { health: "stale" } }],
        ["session", { value: null }],
        ["message", { ...anEntry(3).message, stamp: "now" }],
        ["pending", { key: "/docs/X.md#c" }],
        ["pending", {}],
        ["branches", ["w/a", 3, "w/b"]],
        ["machine-work", CASES.samples.work],
        ["tasks-status", { available: false, slow: true }],
        ["tasks-status", {}],
        ["agents", {}],
        ["queue", {}],
      ),
    );
    expect(changes?.map((one) => one.region)).toEqual([
      "task",
      "task-fields",
      "session",
      "session",
      "message",
      "pending",
      "pending",
      "branches",
      "machine-work",
      "tasks-status",
      "tasks-status",
    ]);
    expect(changes?.[2]).toEqual({ region: "session", key: "amber", health: "stale" });
    expect(changes?.[3]).toEqual({ region: "session", key: "", health: null });
    expect(changes?.[4]).toMatchObject({ region: "message", entry: { stamp: "now" } });
    expect(changes?.[5]).toEqual({ region: "pending", key: "/docs/X.md#c" });
    expect(changes?.[6]).toEqual({ region: "pending", key: null });
    expect(changes?.[7]).toEqual({ region: "branches", moved: ["w/a", "w/b"] });
    expect(changes?.[9]).toEqual({ region: "tasks-status", available: false, slow: true });
    expect(changes?.[10]).toEqual({ region: "tasks-status", available: undefined, slow: false });
  });

  it("are none when one the page follows cannot be read", () => {
    for (const [region, data] of [
      ["task", { key: 1 }],
      ["task-fields", { key: "w/a", fields: { status: 3 } }],
      ["message", { id: "x" }],
      ["machine-work", { machines: "none" }],
    ] as const) {
      expect(readChanges(event([region, data])), region).toBeNull();
    }
  });

  it("are none of an event with none", () => {
    expect(readChanges({ version: 1 })).toEqual([]);
    expect(readChanges(null)).toEqual([]);
  });
});

describe("a task's page before anything is read", () => {
  it("shows nothing and asks for nothing", () => {
    const d = emptyTask();
    expect([d.page, d.missing, d.error, d.entries, d.round]).toEqual([null, null, "", [], 0]);
    expect(emptyTask()).not.toBe(d);
  });
});

describe("a task's page read", () => {
  it("draws the server's answer and keeps it on the device with its tag", async () => {
    const body = aPage();
    const { task, d, asked, store, host } = live(() => answer(200, body, '"p1"'));
    await task.refresh();
    expect(asked).toEqual([[PAGE_URL, ""]]);
    expect(d.page).toEqual(body);
    expect(d.pageCopy).toEqual(['"p1"', body]);
    expect(keptBy(store)).toEqual([["task:w/a", PAGE, '"p1"', body]]);
    expect(host.redraw).toHaveBeenCalled();
    expect([d.missing, d.error]).toEqual([null, ""]);
  });

  it("names the copy it holds, and a 304 shows that copy again, keeping nothing", async () => {
    const body = aPage();
    const { task, d, asked, store } = live((_, etag) =>
      etag === '"p1"' ? answer(304) : answer(200, body, '"p1"'),
    );
    await task.refresh();
    await task.refresh();
    expect(asked).toEqual([
      [PAGE_URL, ""],
      [PAGE_URL, '"p1"'],
    ]);
    expect(store.keep).toHaveBeenCalledTimes(1);
    expect(d.page).toEqual(body);
  });

  it("keeps the subtasks it read for the same task", async () => {
    const child = aTask("w/a/1", { parent: "w/a" });
    const { task, d } = live(() => answer(200, aPage(aTask("w/a", { title: "Again" })), '"p2"'));
    d.page = aPage(aTask("w/a"), [child]);
    await task.refresh();
    expect(d.page?.task.title).toBe("Again");
    expect(d.page?.children).toEqual([child]);
  });

  it("forgets what this device drew of a task the server no longer has", async () => {
    const { task, d, store } = live(() => answer(404, { error: "no task" }));
    d.page = aPage();
    d.entries = [anEntry(1)];
    await task.refresh();
    expect(d.page).toBeNull();
    expect(d.entries).toEqual([]);
    expect(d.missing).toBe("w/a");
    expect(store.forget).toHaveBeenCalledWith(expect.anything(), "", "task:w/a");
  });

  it("says why it failed while nothing from this device is shown", async () => {
    const { task, d } = live(() => answer(503));
    await task.refresh();
    expect(d.error).not.toBe("");
    expect(task.fresh.failing).toEqual([PAGE]);
  });

  it("refuses an answer it cannot read", async () => {
    const { task, d } = live(() => answer(200, { task: {} }, '"p1"'));
    await task.refresh();
    expect(d.error).toBe("The task's page is not readable");
    expect(d.page).toBeNull();
  });

  it("writes nothing once the page has stopped or moved on", async () => {
    const reply = later<Response>();
    const { task, d, page, store } = live(() => reply.promise);
    const read = task.refresh();
    page.abort();
    reply.settle(answer(200, aPage(), '"p1"'));
    await read;
    expect(d.page).toBeNull();
    expect(store.keep).not.toHaveBeenCalled();

    const again = later<Response>();
    const other = live(() => again.promise);
    const moving = other.task.refresh();
    other.task.slug = "w/b";
    again.settle(answer(503));
    await moving;
    expect(other.d.error).toBe("");
  });
});

describe("a task's page read with nothing from this device", () => {
  it("tries a read with no answer in time again by itself until it answers", async () => {
    let unanswered = 2;
    const { task, d, asked } = live((url) =>
      url === PAGE_URL && unanswered-- > 0
        ? new Promise<Response>(() => {})
        : answer(200, aPage(), '"p1"'),
    );
    const loading = task.load();
    await pass(ANSWER_MS);
    await loading;
    // Said in a read's words, never a write's, and asked again by itself once its wait ends.
    expect(d.error).toBe(NOT_IN_TIME);
    expect(task.drawn()).toBe(false);
    expect(asked).toHaveLength(1);
    await waitEnds();
    expect(asked).toHaveLength(2);
    await pass(ANSWER_MS);
    expect(d.error).toBe(NOT_IN_TIME);
    await waitEnds();
    expect(asked).toHaveLength(3);
    expect(task.drawn()).toBe(true);
    expect(d.error).toBe("");
    await settled();
    expect(asked).toHaveLength(3);
  });

  it("tries a read the server refused again, as one with no answer", async () => {
    let refused = 1;
    const { task, d, asked } = live(() =>
      refused-- > 0 ? answer(503) : answer(200, aPage(), '"p1"'),
    );
    await task.load();
    expect(d.error).toBe("The server answered 503");
    await waitEnds();
    expect(asked).toHaveLength(2);
    expect(task.drawn()).toBe(true);
  });

  it("reads nothing again of a task the server does not have", async () => {
    const { task, d, asked } = live(() => answer(404));
    await task.load();
    expect(d.missing).toBe("w/a");
    await settled();
    expect(asked).toHaveLength(1);
  });

  it("takes a page answered under another name as no task of this one, and reads nothing again", async () => {
    const { task, d, asked } = live(() => answer(200, aPage(aTask("w/other")), '"p1"'));
    await task.load();
    expect([d.missing, d.page, d.error, task.drawn()]).toEqual(["w/a", null, "", false]);
    await settled();
    expect(urls(asked).filter((url) => url === PAGE_URL)).toHaveLength(1);
  });

  it("takes a read given up as the page leaves for no failure, and tries nothing again", async () => {
    const reads: ReturnType<typeof live>[] = [];
    const leaving = live(() => {
      const signal = reads[0]?.given.at(-1)?.[1];
      return new Promise<Response>((_, refuse) =>
        signal?.addEventListener("abort", () => refuse(new DOMException("gone", "AbortError"))),
      );
    });
    reads.push(leaving);
    const loading = leaving.task.load();
    await pass();
    leaving.page.abort();
    await loading;
    expect(leaving.d.error).toBe("");
    expect(leaving.task.fresh.failing).toEqual([]);
    await settled();
    expect(leaving.asked).toHaveLength(1);
  });

  it("stops trying the route it left once it moves to another task", async () => {
    const { task, asked } = live((url) => (url === PAGE_URL ? answer(503) : answer(404)));
    await task.load();
    task.slug = "w/b";
    await settled();
    expect(urls(asked)).toEqual([PAGE_URL]);
  });
});

describe("a task's page opened", () => {
  const PAGE_KEPT = { value: aPage(), etag: '"p1"', saved: 1_000 };
  const THREAD_KEPT = { value: history([anEntry(1), anEntry(2)], 1), etag: '"t1"', saved: 2_000 };

  it("draws what this device kept of it at once, page and thread, and counts the visit", async () => {
    const { task, d, store, host } = live(undefined, {
      "task:w/a page": PAGE_KEPT,
      "task:w/a thread": THREAD_KEPT,
    });
    await task.opened();
    expect(store.visit).toHaveBeenCalledWith(expect.anything(), "", "task:w/a", "task", false);
    expect(d.page).toEqual(PAGE_KEPT.value);
    expect(d.pageCopy).toEqual(['"p1"', PAGE_KEPT.value]);
    expect(ids(d.entries)).toEqual([1, 2]);
    expect(d.drawnEntries).toEqual([1, 2]);
    expect([d.before, d.threadLoaded, d.threadRequested, d.threadTag]).toEqual([
      1,
      true,
      true,
      '"t1"',
    ]);
    expect(task.fresh).toMatchObject({ saved: 1_000, unanswered: [PAGE, THREAD] });
    expect(host.freshness).toHaveBeenCalled();
    expect(host.redraw).toHaveBeenCalled();
  });

  it("draws the page alone when the thread was not kept", async () => {
    const { task, d } = live(undefined, { "task:w/a page": PAGE_KEPT });
    await task.opened();
    expect(d.page).toEqual(PAGE_KEPT.value);
    expect(d.threadLoaded).toBe(false);
    expect(task.fresh.unanswered).toEqual([PAGE]);
  });

  it("draws nothing kept of another task, and nothing when the page already shows it", async () => {
    const other = live(undefined, {
      "task:w/a page": { ...PAGE_KEPT, value: aPage(aTask("w/b")) },
    });
    await other.task.opened();
    expect(other.d.page).toBeNull();

    const shown = live(undefined, { "task:w/a page": PAGE_KEPT });
    const drawn = aPage(aTask("w/a", { title: "Drawn" }));
    shown.d.page = drawn;
    await shown.task.opened();
    expect(shown.d.page).toBe(drawn);
    expect(shown.task.fresh).toEqual(FRESH);
  });

  it("forgets a copy it cannot read", async () => {
    const { task, d, store } = live(undefined, {
      "task:w/a page": { ...PAGE_KEPT, value: { task: {} } },
    });
    await task.opened();
    expect(d.page).toBeNull();
    expect(store.forget).toHaveBeenCalled();
  });

  it("reads nothing more of the device's copy, nor forgets it, once the page stopped while it was read", async () => {
    const { task, d, store, page } = live(undefined, {
      "task:w/a page": PAGE_KEPT,
      // A thread this client cannot read, which a read would forget.
      "task:w/a thread": { ...THREAD_KEPT, value: { entries: "none" } },
    });
    store.read.mockImplementationOnce(async () => {
      page.abort();
      return PAGE_KEPT;
    });
    await task.opened();
    expect(store.read.mock.calls.map((call) => call[3])).toEqual([PAGE]);
    expect(store.forget).not.toHaveBeenCalled();
    expect(d.page).toBeNull();
  });

  it("then checks the page and the thread at once, a 304 confirming each", async () => {
    const { task, d, asked } = live(() => answer(304), {
      "task:w/a page": PAGE_KEPT,
      "task:w/a thread": THREAD_KEPT,
    });
    await task.opened();
    await task.load();
    expect(asked).toEqual([
      [PAGE_URL, '"p1"'],
      [THREAD_URL, '"t1"'],
    ]);
    // Answered, it is the device's copy no more once the feed is open too.
    expect(task.fresh).toEqual({ ...FRESH, saved: 1_000 });
    task.freshen(opened);
    expect(task.fresh).toEqual({ ...FRESH, open: true });
    expect(ids(d.entries)).toEqual([1, 2]);
    expect(d.drawnEntries).toEqual([]);
  });

  it("replaces the thread drawn when the server answers it anew, keeping what the feed brought", async () => {
    const { task, d, store } = live(
      (url) =>
        url === THREAD_URL
          ? answer(200, history([anEntry(2), anEntry(3)], 2), '"t2"')
          : answer(304),
      { "task:w/a page": PAGE_KEPT, "task:w/a thread": THREAD_KEPT },
    );
    await task.opened();
    task.heard(event(["message", anEntry(7).message]));
    await task.load();
    expect(ids(d.entries)).toEqual([2, 3, 7]);
    expect([d.before, d.threadTag]).toEqual([2, '"t2"']);
    expect(keptBy(store)).toContainEqual([
      "task:w/a",
      THREAD,
      '"t2"',
      history([anEntry(2), anEntry(3)], 2),
    ]);
  });

  it("asks each again, waiting longer after each failure, until it answers", async () => {
    let refused = 2;
    const { task, asked } = live(() => (refused-- > 0 ? answer(503) : answer(304)), {
      "task:w/a page": PAGE_KEPT,
    });
    await task.opened();
    await task.load();
    expect(asked).toHaveLength(1);
    expect(task.fresh.failing).toEqual([PAGE]);
    await pass(999);
    expect(asked).toHaveLength(1);
    await pass(1);
    expect(asked).toHaveLength(2);
    await pass(1_999);
    expect(asked).toHaveLength(2);
    await pass(1);
    expect(asked).toHaveLength(3);
    expect(waits(task.fresh, PAGE)).toBe(false);
    await pass(60_000);
    expect(asked).toHaveLength(3);
  });

  it("stops asking once a newer check starts", async () => {
    const { task, asked } = live(() => answer(503), { "task:w/a page": PAGE_KEPT });
    await task.opened();
    await task.load();
    await task.load();
    expect(asked).toHaveLength(2);
    await pass(1_000);
    // Only the newer check's retry: the first one's stopped.
    expect(asked).toHaveLength(3);
  });

  it("says the server is unreachable through the notices, not the page's error", async () => {
    const { task, d } = live(() => answer(503), {
      "task:w/a page": PAGE_KEPT,
      "task:w/a thread": THREAD_KEPT,
    });
    await task.opened();
    await task.load();
    expect(d.error).toBe("");
    expect([...task.fresh.failing].sort()).toEqual([PAGE, THREAD]);
  });

  it("reads the subtasks and the thread asked for meanwhile once the page is answered", async () => {
    const kept = { ...PAGE_KEPT, value: aPage(aTask("w/a"), [], { children_count: 2 }) };
    const { task, d, asked } = live(
      (url) =>
        url === PAGE_URL
          ? answer(304)
          : url.startsWith("/api/task-board")
            ? answer(200, listing([aTask("w/a/1", { parent: "w/a" })], [["ready", 2]]))
            : answer(200, history([anEntry(1)]), '"t1"'),
      { "task:w/a page": kept },
    );
    await task.opened();
    await task.history(false);
    await task.children(false);
    expect(asked).toEqual([]);
    expect([d.threadRequested, d.childRequested]).toEqual([true, true]);
    await task.load();
    await pass();
    expect(urls(asked)).toEqual([PAGE_URL, THREAD_URL, "/api/task-board?parent=w%2Fa&older=1"]);
    expect(ids(d.entries)).toEqual([1]);
    expect(d.page?.children.map((child) => child.slug)).toEqual(["w/a/1"]);
  });
});

describe("a task's page moved to another task", () => {
  it("draws the other from this device and reads it, dropping the copy left unanswered", async () => {
    const { task, d, asked } = live(
      (url) => (url === OTHER_URL ? answer(200, aPage(aTask("w/b")), '"b1"') : answer(503)),
      {
        "task:w/a page": { value: aPage(), etag: '"p1"', saved: 1 },
        "task:w/b page": { value: aPage(aTask("w/b")), etag: '"b0"', saved: 5 },
      },
    );
    await task.opened();
    expect(d.page?.task.slug).toBe("w/a");
    await task.moved("w/b");
    expect(task.slug).toBe("w/b");
    expect(asked).toEqual([[OTHER_URL, '"b0"']]);
    expect(d.page?.task.slug).toBe("w/b");
    expect(task.fresh.unanswered).toEqual([]);
  });

  it("shows nothing of the page and thread it left unanswered while the other is read", async () => {
    const reply = later<Response>();
    const { task, d, host } = live(() => reply.promise, {
      "task:w/a page": { value: aPage(), etag: '"p1"', saved: 1 },
      "task:w/a thread": { value: history([anEntry(1)]), etag: '"t1"', saved: 1 },
    });
    await task.opened();
    host.redraw.mockClear();
    const moving = task.moved("w/b");
    await pass();
    expect([d.page, d.entries, d.threadLoaded, d.threadRequested]).toEqual([
      null,
      [],
      false,
      false,
    ]);
    expect(task.fresh).toEqual(FRESH);
    expect(host.redraw).toHaveBeenCalled();
    reply.settle(answer(200, aPage(aTask("w/b")), '"b1"'));
    await moving;
    expect(d.page?.task.slug).toBe("w/b");
  });

  it("does nothing to stay on the same task", async () => {
    const { task, asked } = live();
    await task.moved("w/a");
    expect(asked).toEqual([]);
  });
});

describe("a task's thread", () => {
  it("reads its newest page once, kept on the device", async () => {
    const page = history([anEntry(4), anEntry(3)], 3);
    const { task, d, asked, store } = live(() => answer(200, page, '"t1"'));
    await task.history(false);
    await task.history(false);
    expect(urls(asked)).toEqual([THREAD_URL]);
    expect(ids(d.entries)).toEqual([3, 4]);
    expect([d.before, d.threadLoaded, d.threadTag]).toEqual([3, true, '"t1"']);
    expect(keptBy(store)).toEqual([["task:w/a", THREAD, '"t1"', page]]);
  });

  it("reads the page before the earliest shown, never kept, and none before the first", async () => {
    const { task, d, asked, store } = live(() => answer(200, history([anEntry(1)], null)));
    d.entries = [anEntry(3)];
    d.before = 2;
    await task.history(true);
    expect(urls(asked)).toEqual([`${THREAD_URL}&before=2`]);
    expect(ids(d.entries)).toEqual([1, 3]);
    expect(d.before).toBeNull();
    expect(store.keep).not.toHaveBeenCalled();
    await task.history(true);
    expect(asked).toHaveLength(1);
  });

  it("waits for its check to read earlier messages while it shows this device's copy", async () => {
    const { task, d, asked } = live(
      (url) =>
        url.includes("before=")
          ? answer(200, history([anEntry(1)]))
          : answer(200, history([anEntry(2)], 1), '"t2"'),
      { "task:w/a thread": { value: history([anEntry(2)], 1), etag: '"t1"', saved: 1 } },
    );
    task.freshen((f) => ({ ...f, unanswered: [THREAD] }));
    d.page = aPage();
    d.before = 1;
    await task.history(true);
    expect([asked.length, d.olderWaiting]).toEqual([0, true]);
    await task.checkThread();
    await pass();
    expect(urls(asked)).toEqual([THREAD_URL, `${THREAD_URL}&before=1`]);
    expect(ids(d.entries)).toEqual([1, 2]);
    expect(d.olderWaiting).toBe(false);
  });

  it("says why a read failed", async () => {
    const { task, d } = live(() => answer(200, { entries: "none" }, '"t"'));
    await task.history(false);
    expect(d.error).toBe("The task's thread is not readable");
    expect(d.threadLoaded).toBe(false);
  });

  it("catches up from its newest message a page at a time once the page is read again", async () => {
    const pages: Record<string, unknown> = {
      [`${THREAD_URL}&after=5`]: history([anEntry(6), anEntry(7)], null, 7),
      [`${THREAD_URL}&after=7`]: history([anEntry(8)], null, null),
    };
    const { task, d, asked } = live((url) =>
      url === PAGE_URL ? answer(200, aPage(), '"p1"') : answer(200, pages[url]),
    );
    d.page = aPage();
    d.threadLoaded = true;
    d.entries = [anEntry(5), anEntry(2)];
    await task.load();
    expect(urls(asked)).toEqual([PAGE_URL, `${THREAD_URL}&after=5`, `${THREAD_URL}&after=7`]);
    expect(ids(d.entries)).toEqual([2, 5, 6, 7, 8]);
  });

  it("stops catching up at a cursor that does not move, or a read that fails", async () => {
    const { task, d, asked } = live((url) =>
      url === PAGE_URL ? answer(200, aPage(), '"p1"') : answer(200, history([], null, 5)),
    );
    d.page = aPage();
    d.threadLoaded = true;
    d.entries = [anEntry(5)];
    await task.load();
    expect(asked).toHaveLength(2);

    const failing = live((url) => (url === PAGE_URL ? answer(200, aPage(), '"p1"') : answer(503)));
    failing.d.page = aPage();
    failing.d.threadLoaded = true;
    await failing.task.load();
    expect(failing.d.error).not.toBe("");
  });

  it("reads the newest page the page asked for on a page read anew", async () => {
    const { task, d, asked } = live((url) =>
      url === PAGE_URL ? answer(200, aPage(), '"p1"') : answer(200, history([anEntry(1)]), '"t1"'),
    );
    d.page = aPage();
    d.threadRequested = true;
    await task.load();
    expect(urls(asked)).toEqual([PAGE_URL, THREAD_URL]);
    expect(ids(d.entries)).toEqual([1]);
  });

  it("is checked against what the device drew only while that waits", async () => {
    const { task, d, asked } = live(() => answer(503));
    await task.checkThread();
    expect(d.error).toBe("");
    expect(task.fresh.failing).toEqual([]);
    expect(asked).toHaveLength(1);
  });
});

describe("a task's subtasks", () => {
  it("are read a page at a time, the next from the cursor, counted by their columns", async () => {
    const first = [aTask("w/a/1", { parent: "w/a" }), aTask("w/a/2", { parent: "w/a" })];
    const second = [aTask("w/a/3", { parent: "w/a" })];
    const { task, d, asked } = live((url) =>
      url.includes("cursor=")
        ? answer(
            200,
            listing(second, [
              ["ready", 2],
              ["done", 1],
            ]),
          )
        : answer(
            200,
            listing(
              first,
              [
                ["ready", 2],
                ["done", 1],
              ],
              "c 1",
            ),
          ),
    );
    d.page = aPage(aTask("w/a"), [], { children_count: 3 });
    await task.children(false);
    expect(d.childCursor).toBe("c 1");
    expect(d.page?.children.map((child) => child.slug)).toEqual(["w/a/1", "w/a/2"]);
    await task.children(true);
    expect(urls(asked)).toEqual([
      "/api/task-board?parent=w%2Fa&older=1",
      "/api/task-board?parent=w%2Fa&older=1&cursor=c%201",
    ]);
    expect(d.page?.children.map((child) => child.slug)).toEqual(["w/a/1", "w/a/2", "w/a/3"]);
    expect(d.page?.children_count).toBe(3);
    expect(d.childCursor).toBeNull();
  });

  it("say why a read failed", async () => {
    const { task, d } = live(() => answer(200, { tasks: "none" }));
    d.page = aPage();
    await task.children(false);
    expect(d.error).toBe("The task board's answer is not a listing");
  });

  it("asked for, are read with a page that counts some and lists none", async () => {
    const { task, d, asked } = live((url) =>
      url === PAGE_URL
        ? answer(200, aPage(aTask("w/a"), [], { children_count: 1 }), '"p2"')
        : answer(200, listing([aTask("w/a/1", { parent: "w/a" })], [["ready", 1]])),
    );
    d.page = aPage(aTask("w/a"), [], { children_count: 1 });
    d.childRequested = true;
    await task.load();
    expect(urls(asked)).toEqual([PAGE_URL, "/api/task-board?parent=w%2Fa&older=1"]);
    expect(d.page?.children.map((child) => child.slug)).toEqual(["w/a/1"]);
  });

  it("are read again with the page when a task came under it", async () => {
    const { task, d, asked } = live((url) =>
      url === PAGE_URL
        ? answer(200, aPage(aTask("w/a"), [], { children_count: 1 }), '"p2"')
        : answer(200, listing([aTask("w/c", { parent: "w/a" })], [["ready", 1]])),
    );
    d.page = aPage();
    d.childRequested = true;
    d.childCame = true;
    await task.refresh();
    await pass();
    expect(urls(asked)).toEqual([PAGE_URL, "/api/task-board?parent=w%2Fa&older=1"]);
    expect(d.childCame).toBe(false);
    expect(d.page?.children.map((child) => child.slug)).toEqual(["w/c"]);
  });
});

describe("the landing a task offers", () => {
  const OFFER = { action: "land", label: "Land it", branch: "w/a", payload: "{}" };

  it("is kept with the task's revision when it has a payload to post", async () => {
    const { task, d, asked } = live(() => answer(200, OFFER));
    d.page = aPage(aTask("w/a", { updated: 7 }));
    await task.offer();
    expect(urls(asked)).toEqual(["/api/task-offer?slug=w%2Fa"]);
    expect(d.approval).toEqual([OFFER, 7]);
  });

  it("is none without a payload, and nothing for a task that changed meanwhile", async () => {
    const bare = live(() => answer(200, { ...OFFER, payload: null }));
    bare.d.page = aPage();
    await bare.task.offer();
    expect(bare.d.approval).toBeNull();

    const reply = later<Response>();
    const moved = live(() => reply.promise);
    moved.d.page = aPage(aTask("w/a", { updated: 1 }));
    const asking = moved.task.offer();
    moved.d.page = aPage(aTask("w/a", { updated: 2 }));
    reply.settle(answer(200, OFFER));
    await asking;
    expect(moved.d.approval).toBeNull();
  });

  it("is not asked before the task's page is shown, and says why it failed", async () => {
    const none = live(() => answer(200, OFFER));
    await none.task.offer();
    expect(none.asked).toEqual([]);
    const broken = live(() => answer(200, { action: 1 }));
    broken.d.page = aPage();
    await broken.task.offer();
    expect(broken.d.error).toBe("The task's offer is not readable");
  });
});

describe("a feed event on a task's page", () => {
  /** A page showing `w/a` and its subtask `w/a/1`, read again on `w/a`'s route. */
  function shown(route: Route = () => answer(200, aPage(), '"p2"')) {
    const page = live(route);
    page.d.page = aPage(aTask("w/a", { links: ["docs/X.md"] }), [
      aTask("w/a/1", { parent: "w/a" }),
    ]);
    return page;
  }

  it("holds the reader's place and is kept for the reads still out, up to 128", () => {
    const { task, d, held } = shown();
    for (let at = 0; at < 130; at += 1) task.heard(event());
    expect(held).toHaveBeenCalledWith("[id^=message-], [id^=child-], [data-slug]");
    expect(d.events).toHaveLength(128);
    expect(d.events[0]?.[0]).toBe(3);
    expect(d.generation).toBe(130);
  });

  it("loses the stream when it cannot be read, changing nothing", () => {
    const { task, d, host } = shown();
    const before = d.page;
    expect(task.heard(event(["message", {}], ["machine-work", CASES.samples.work]))).toBe(false);
    expect(d.page).toBe(before);
    expect(host.redraw).not.toHaveBeenCalled();
  });

  it("adds a message of its task once, and a verdict nudges its gates", () => {
    const { task, d } = shown();
    task.heard(event(["message", anEntry(1).message]));
    task.heard(event(["message", anEntry(1).message]));
    task.heard(event(["message", anEntry(2, "w/b").message]));
    expect([ids(d.entries), d.gatesNudged]).toEqual([[1], 0]);
    task.heard(event(["message", anEntry(3, "w/a", "amber", "verdict").message]));
    expect([ids(d.entries), d.gatesNudged]).toEqual([[1, 3], 1]);
  });

  it("nudges the gates for a card in a document it links and a new tip of its branch", () => {
    const { task, d } = shown();
    task.heard(event(["pending", { key: "/docs/Y.md#c" }], ["branches", ["w/b"]]));
    expect(d.gatesNudged).toBe(0);
    task.heard(event(["pending", { key: "/docs/X.md#c" }], ["branches", ["w/a"]]));
    expect(d.gatesNudged).toBe(2);
    task.gatesAnswered("w/a", "feature/a");
    task.gatesAnswered("w/a", "feature/a");
    expect(d.gatesBranch).toEqual(["w/a", "feature/a"]);
    task.heard(event(["branches", ["w/a"]]));
    expect(d.gatesNudged).toBe(2);
    task.heard(event(["branches", ["feature/a"]]));
    expect(d.gatesNudged).toBe(3);
  });

  it("says a holder's health on every role it holds, the subtasks' too", () => {
    const { task, d, host } = live();
    d.page = aPage(aTask("w/a", { roles: [aRole("executor", "amber")] }), [
      aTask("w/a/1", { parent: "w/a", roles: [aRole("reviewer", "amber")] }),
    ]);
    task.heard(event(["session", { key: "amber", value: { health: "stale" } }]));
    expect(d.page?.task.roles[0]?.health).toBe("stale");
    expect(d.page?.children[0]?.roles[0]?.health).toBe("stale");
    host.redraw.mockClear();
    task.heard(event(["session", { key: "cedar", value: { health: "stale" } }]));
    expect(host.redraw).not.toHaveBeenCalled();
  });

  it("puts the machines' work in place", () => {
    const { task, d } = shown();
    const work = aWork([]);
    task.heard(event(["machine-work", work]));
    expect(d.page?.machine_work).toEqual(work);
  });

  it("says the board's state, and reads the page again once it is back", async () => {
    const { task, d, asked } = shown();
    task.heard(event(["tasks-status", { available: false, slow: false }]));
    expect(d.error).toBe("The task board is unavailable. Reconnecting…");
    expect(asked).toEqual([]);
    task.heard(event(["tasks-status", { available: true }]));
    expect(d.error).toBe("");
    await pass();
    expect(urls(asked)).toEqual([PAGE_URL]);
  });

  it("reads the page again for a new brief or status, never for a title", async () => {
    const { task, d, asked } = shown();
    task.heard(event(["task-fields", { key: "w/a", fields: { title: "Renamed" } }]));
    await pass();
    expect(d.page?.task.title).toBe("Renamed");
    expect(asked).toEqual([]);
    task.heard(event(["task-fields", { key: "w/a", fields: { status: "review" } }]));
    await pass();
    expect(urls(asked)).toEqual([PAGE_URL]);
  });

  it("puts a subtask that came, changed or left in its list, and reads the page for its count", async () => {
    const { task, d, asked } = shown();
    task.heard(event(["task", { key: "w/a/2", value: aTask("w/a/2", { parent: "w/a" }) }]));
    expect(d.page?.children.map((child) => child.slug)).toEqual(["w/a/1", "w/a/2"]);
    await pass();
    expect(asked).toHaveLength(1);
    task.heard(
      event(["task", { key: "w/a/1", value: aTask("w/a/1", { parent: "w/a", title: "T" }) }]),
    );
    await pass();
    expect(d.page?.children[0]?.title).toBe("T");
    expect(asked).toHaveLength(1);
    task.heard(event(["task", { key: "w/a/1", removed: true }]));
    expect(d.page?.children.map((child) => child.slug)).toEqual(["w/a/2"]);
    task.heard(event(["task", { key: "w/a/2", value: aTask("w/a/2", { parent: "w/b" }) }]));
    expect(d.page?.children).toEqual([]);
    task.heard(event(["task", { key: "w/z", value: aTask("w/z") }]));
    expect(d.page?.children).toEqual([]);
  });

  it("says its task was removed, and reads one filed under the slug it could not find", async () => {
    const { task, d, asked } = shown();
    task.heard(event(["task", { key: "w/a", removed: true }]));
    expect(d.page).toBeNull();
    expect([d.missing, d.error]).toEqual(["w/a", "This task was removed"]);
    await pass();
    expect(asked).toEqual([]);
    task.heard(event(["task", { key: "w/a", value: aTask("w/a") }]));
    await pass();
    expect(urls(asked)).toEqual([PAGE_URL]);
  });

  it("reads the page again for its own task's record", async () => {
    const { task, asked } = shown();
    task.heard(event(["task", { key: "w/a", value: aTask("w/a", { title: "New" }) }]));
    await pass();
    expect(urls(asked)).toEqual([PAGE_URL]);
  });

  it("asks nothing of a page that shows another task than the route's", async () => {
    const { task, asked } = shown();
    task.slug = "w/b";
    task.heard(event(["task-fields", { key: "w/a", fields: { status: "review" } }]));
    await pass();
    expect(asked).toEqual([]);
  });

  it("changes nothing on a page that has drawn nothing", () => {
    const { task, d, host } = live();
    task.heard(
      event(
        ["task-fields", { key: "w/a", fields: { title: "T" } }],
        ["session", { key: "amber" }],
        ["pending", { key: "/docs/X.md" }],
        ["branches", ["w/a"]],
        ["machine-work", aWork([])],
      ),
    );
    expect([d.page, d.gatesNudged]).toEqual([null, 0]);
    expect(host.redraw).not.toHaveBeenCalled();
  });

  it("reads the page once more for all the events that asked while it was read", async () => {
    const replies: ((response: Response) => void)[] = [];
    const { task, asked } = shown(
      () =>
        new Promise<Response>((done) => {
          replies.push(done);
        }),
    );
    const status = (to: string) => event(["task-fields", { key: "w/a", fields: { status: to } }]);
    task.heard(status("review"));
    task.heard(status("done"));
    task.heard(status("review"));
    await pass();
    expect(asked).toHaveLength(1);
    replies[0]?.(answer(200, aPage(), '"p2"'));
    await pass();
    expect(asked).toHaveLength(2);
    replies[1]?.(answer(200, aPage(), '"p3"'));
    await pass();
    expect(asked).toHaveLength(2);
  });

  it("is applied again to what a read answered while it came", async () => {
    const reply = later<Response>();
    const { task, d } = shown(() => reply.promise);
    const read = task.refresh();
    task.heard(event(["task-fields", { key: "w/a", fields: { title: "Meanwhile" } }]));
    reply.settle(answer(200, aPage(aTask("w/a", { title: "Before" })), '"p2"'));
    await read;
    expect(d.page?.task.title).toBe("Meanwhile");
  });

  it("reads the page whole when events it should replay were dropped", async () => {
    const reply = later<Response>();
    const { task, d, asked } = shown((_url, etag) =>
      etag === "" && asked.length === 1 ? reply.promise : answer(200, aPage(), '"p3"'),
    );
    const read = task.refresh();
    for (let at = 0; at < 130; at += 1) task.heard(event());
    reply.settle(answer(200, aPage(), '"p2"'));
    await read;
    await pass();
    expect(urls(asked)).toEqual([PAGE_URL, PAGE_URL]);
    expect(d.page).not.toBeNull();
  });
});

describe("the board under a task's page", () => {
  const FRAME: FrameRefresh = { refresh: vi.fn(), connected: vi.fn(), heard: vi.fn() };
  const BOARD = { version: 3, ping_ms: 1_000, build: "b" };

  it("is drawn once what this device kept of the task is", async () => {
    const { task, host } = live();
    const kept = later<void>();
    task.kept = kept.promise;
    const run = task.run(FRAME);
    run.show(BOARD);
    await pass();
    expect([run.board.current, task.boardShown]).toEqual([BOARD, false]);
    kept.settle();
    await pass();
    expect(task.boardShown).toBe(true);
    expect(host.redraw).toHaveBeenCalledTimes(1);
    run.show(BOARD);
    await pass();
    expect(host.redraw).toHaveBeenCalledTimes(1);
  });

  it("says its trouble and its staleness on the page, and nothing once stopped", async () => {
    const { task, host, page } = live();
    const run = task.run(FRAME);
    run.setTrouble("The board could not be read");
    await pass();
    expect(task.trouble).toBe("The board could not be read");
    run.setStale(true);
    expect(host.stale).toHaveBeenCalledWith(true);
    page.abort();
    run.setTrouble("Later");
    run.show(BOARD);
    await pass();
    expect([task.trouble, task.boardShown]).toEqual(["The board could not be read", false]);
  });

  it("reads the task once the board's version is read and the device's copy drawn", async () => {
    const { task, asked } = live(() => answer(200, aPage(), '"p1"'));
    const run = task.run(FRAME);
    // Nothing the board's loop waits for before it opens the feed.
    expect(run.views.read()).toBeUndefined();
    await pass();
    expect(urls(asked)).toEqual([PAGE_URL]);
    run.views.freshen((f) => ({ ...f, open: true }));
    expect(task.fresh.open).toBe(true);
    expect(run.views.heard(event())).toBe(true);
  });

  it("gives up every read still out once the page stops", async () => {
    const { task, d, given, page } = live(() => new Promise<Response>(() => {}));
    d.page = aPage();
    void task.refresh();
    void task.history(false);
    void task.checkThread();
    void task.children(false);
    void task.offer();
    await pass();
    const reads = () => given.map(([url, signal]) => [url.split("?")[0], signal?.aborted ?? null]);
    expect(reads()).toEqual([
      ["/api/task-page", false],
      ["/api/task-history", false],
      ["/api/task-history", false],
      ["/api/task-board", false],
      ["/api/task-offer", false],
    ]);
    page.abort();
    expect(
      reads().every(([, aborted]) => aborted === true),
      JSON.stringify(reads()),
    ).toBe(true);
  });

  it("reads nothing once stopped", async () => {
    const { task, asked, page } = live(() => answer(200, aPage(), '"p1"'));
    page.abort();
    await task.read();
    expect(asked).toEqual([]);
  });

  it("starts drawing the device's copy and following the board, until the page stops", async () => {
    const { task, asked, page, store } = live(() => answer(503));
    const listens = { addEventListener: () => {} };
    vi.stubGlobal("window", { ...listens, reviewHoldPlace: () => {} });
    vi.stubGlobal("document", { ...listens, getElementById: () => null });
    const started = task.start(FRAME);
    await pass();
    expect(store.visit).toHaveBeenCalled();
    expect(urls(asked)).toEqual([BOARD_PATH]);
    expect(task.trouble).not.toBeNull();
    page.abort();
    await pass(60_000);
    await started;
    expect(asked).toHaveLength(1);
  });

  /**
   * A page started on a server whose board answers once the test says (`answerBoard`) and whose
   * other reads `route` answers, with the feed's sockets it makes (`opened`).
   */
  const begun = (route: Route, kept: Record<string, Kept> = {}) => {
    const board = later<Response>();
    const run = live((url, etag) => (url === BOARD_PATH ? board.promise : route(url, etag)), kept);
    const listens = { addEventListener: () => {}, removeEventListener: () => {} };
    vi.stubGlobal("window", { ...listens, reviewHoldPlace: () => {} });
    vi.stubGlobal("document", {
      ...listens,
      getElementById: () => null,
      querySelector: () => null,
    });
    vi.stubGlobal("sessionStorage", memoryStorage());
    const opened = feeds("http://127.0.0.1/task/w/a");
    const done = run.task.start(FRAME);
    const answerBoard = () => board.settle(answer(200, CASES.samples.board, '"b1"'));
    return { ...run, opened, done, answerBoard };
  };
  const KEPT = {
    "task:w/a page": { value: aPage(), etag: '"p1"', saved: 1_000 },
    "task:w/a thread": { value: history([anEntry(1)], null), etag: '"t1"', saved: 2_000 },
  };
  /** Feed event `version`, of `changes`, as the socket says it. */
  const said = (version: number, ...changes: [string, unknown][]) =>
    JSON.stringify({ ...event(...changes), version });

  it("asks for nothing but the board, and makes no socket, before the board's version is read", async () => {
    const { asked, opened, page, done, d, answerBoard } = begun(() => answer(304), KEPT);
    await pass();
    // The device's copy is drawn meanwhile; its checks and the feed wait for the version.
    expect(d.page?.task.slug).toBe("w/a");
    expect(urls(asked)).toEqual([BOARD_PATH]);
    expect(opened).toEqual([]);
    // Gone before the board answers: its answer starts nothing.
    page.abort();
    answerBoard();
    await done;
    expect(urls(asked)).toEqual([BOARD_PATH]);
    expect(opened).toEqual([]);
  });

  it("opens the feed at the board's answer, while the page's and the thread's checks are out", async () => {
    const pageRead = later<Response>();
    const threadRead = later<Response>();
    const { task, d, asked, opened, page, done, answerBoard } = begun(
      (url) => (url === PAGE_URL ? pageRead.promise : threadRead.promise),
      KEPT,
    );
    answerBoard();
    await pass();
    // One read of each, and the one socket, from the board's version: none waits for another.
    expect(asked).toEqual([
      [BOARD_PATH, ""],
      [PAGE_URL, '"p1"'],
      [THREAD_URL, '"t1"'],
    ]);
    expect(opened.map(({ url }) => url)).toEqual(["ws://127.0.0.1/api/feed?since=7&document="]);
    const source = opened[0]?.source;
    source?.dispatchEvent(new Event("open"));
    await pass();
    // Open, with neither check answered: still this device's copy.
    expect(task.fresh).toMatchObject({ open: true, saved: 1_000, unanswered: [PAGE, THREAD] });
    // What the feed says before the answers shows at once, on the copy.
    say(source, FEED_EVENT, said(8, ["task-fields", change("w/a", { title: "Heard" })]));
    say(source, FEED_EVENT, said(9, ["message", anEntry(5).message]));
    await pass();
    expect(d.page?.task.title).toBe("Heard");
    expect(ids(d.entries)).toEqual([1, 5]);
    // The page as the server had it before the event: the event is applied onto its answer.
    pageRead.settle(answer(200, aPage(aTask("w/a", { title: "Read", updated: 3 })), '"p2"'));
    await pass();
    expect([d.page?.task.title, d.page?.task.updated]).toEqual(["Heard", 3]);
    expect(waits(task.fresh, PAGE)).toBe(false);
    // The thread's newest page, without the message the feed brought: it stays.
    threadRead.settle(answer(200, history([anEntry(1), anEntry(2)], null), '"t2"'));
    await pass();
    expect(ids(d.entries)).toEqual([1, 2, 5]);
    expect(task.fresh).toEqual({ ...FRESH, open: true });
    // Three reads in all, and the one socket still open.
    expect(asked).toHaveLength(3);
    expect(opened.map(({ closed }) => closed)).toEqual([0]);
    page.abort();
    await done;
    expect(opened.map(({ closed }) => closed)).toEqual([1]);
  });

  it("opens the feed beside a first visit's read, whose answer takes what was heard meanwhile", async () => {
    const pageRead = later<Response>();
    const { task, d, asked, opened, page, done, answerBoard } = begun(() => pageRead.promise);
    answerBoard();
    await pass();
    expect(asked).toEqual([
      [BOARD_PATH, ""],
      [PAGE_URL, ""],
    ]);
    expect(opened).toHaveLength(1);
    const source = opened[0]?.source;
    source?.dispatchEvent(new Event("open"));
    // With no page to change yet, the event is kept for the answer.
    say(source, FEED_EVENT, said(8, ["task-fields", change("w/a", { title: "Heard" })]));
    await pass();
    expect(d.page).toBeNull();
    pageRead.settle(answer(200, aPage(aTask("w/a", { title: "Read" })), '"p1"'));
    await pass();
    expect(d.page?.task.title).toBe("Heard");
    expect(task.fresh).toEqual({ ...FRESH, open: true });
    expect(asked).toHaveLength(2);
    page.abort();
    await done;
  });

  it("tries a failed check again while the feed stays open", async () => {
    let tries = 0;
    const { task, asked, opened, page, done, answerBoard } = begun((url) => {
      if (url !== PAGE_URL) return answer(304);
      tries += 1;
      return tries === 1 ? answer(503) : answer(304);
    }, KEPT);
    answerBoard();
    await pass();
    opened[0]?.source.dispatchEvent(new Event("open"));
    await pass();
    expect(task.fresh).toMatchObject({ open: true, unanswered: [PAGE], failing: [PAGE] });
    // The first of a failed read's waits, shorter than the feed's silence.
    await pass(1_000);
    expect(task.fresh).toEqual({ ...FRESH, open: true });
    expect(urls(asked)).toEqual([BOARD_PATH, PAGE_URL, THREAD_URL, PAGE_URL]);
    expect(opened).toHaveLength(1);
    page.abort();
    await done;
  });
});
