import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FRESH, opened, PAGE, THREAD, waits } from "./cache";
import type { FrameRefresh } from "./frame";
import { ANSWER_MS } from "./request";
import {
  boardNotice,
  type Change,
  gatesBranch,
  gatesPart,
  hearPage,
  hearThread,
  LiveTask,
  linksCard,
  namesGates,
  offerPart,
  pageFields,
  pagePart,
  readChanges,
  rejoined,
  type TaskHost,
  threadPart,
  timelinePart,
  withEntries,
} from "./task-live";
import {
  aListing,
  aMachine,
  anOperation,
  fakeAnswer as answer,
  aRole,
  aTask,
  aTaskPage,
  aWork,
  CASES,
  feedEvent as event,
  feeds,
  memoryStorage,
  say,
} from "./testing";
import { NOT_IN_TIME } from "./trouble";
import {
  BOARD_PATH,
  FEED_EVENT,
  type Task,
  type TaskChange,
  type TaskEntry,
  type TaskPage,
} from "./wire";

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

/** The page of `task`, listing `children` of `count`, and whatever else `more` says. */
function aPage(
  task: Task = aTask("w/a"),
  children: Task[] = [],
  more: Partial<TaskPage> = {},
): TaskPage {
  return aTaskPage(task, { children, children_count: children.length, ...more });
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
const listing = (tasks: Task[], counts: [string, number][], next: string | null = null) =>
  aListing(tasks, counts, { next });

const change = (key: string, fields: TaskChange["fields"]): TaskChange => ({ key, fields });

const PAGE_URL = "/api/task-page?slug=w%2Fa";
const THREAD_URL = "/api/task-history?slug=w%2Fa";
const OTHER_URL = "/api/task-page?slug=w%2Fb";
/** The page's and the thread's parts, as the page's store keys them. */
const PAGE_KEY = "page w/a";
const THREAD_KEY = "thread w/a";
const GATES_URL = "/api/task-gates?slug=w%2Fa";
const OFFER_URL = "/api/task-offer?slug=w%2Fa";
const TIMELINE_URL = "/api/task-timeline?slug=w%2Fa";
const GATES = { slug: "w/a", branch: "w/a", gates: [] };
const OFFER = { action: "land", label: "Land it", branch: "w/a", payload: "{}" };

type Route = (url: string, etag: string) => Response | Promise<Response>;

interface Kept {
  value: unknown;
  etag: string;
  saved: number;
}

/**
 * The live page of `w/a` over a server that answers each read as `route` does, on a page whose
 * device kept `kept` (by the item's id and part, as `task:w/a page`). What it asked, the host's
 * calls and the device's.
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
  return { task, asked, given, host, store, held, page };
}

/** The ids of the messages the thread of `task` shows. */
const shownIds = (task: LiveTask) => ids(task.thread().value?.entries ?? []);

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

describe("what changes a task's gates", () => {
  /** The changes of a feed event, each a region and its data, as the page reads them. */
  const changes = (...given: [string, unknown][]) => readChanges(event(...given)) as Change[];
  const running = aWork([
    aMachine("m", [anOperation("m", "1", "test", "running", { task: "w/a" })]),
  ]);
  const page = aPage(aTask("w/a", { links: ["docs/X.md"] }), [], { machine_work: running });
  const answered = { ...GATES, branch: "feature/a" };
  /** Whether the event of `given` names the gates of `w/a` answered as `gates` (none before an
   * answer), on `on`. */
  const names = (
    given: [string, unknown][],
    gates: typeof GATES | null = answered,
    on: TaskPage | null = page,
  ) => namesGates("w/a", gates ?? undefined, { changes: changes(...given), page: on });

  it("is the task itself, a verdict on it, and a new tip of the branch they were answered for", () => {
    expect(names([["task", { key: "w/a", value: aTask("w/a") }]])).toBe(true);
    expect(names([["task", { key: "w/a", removed: true }]])).toBe(true);
    expect(names([["task-fields", change("w/a", { title: "T" })]])).toBe(true);
    expect(names([["task-fields", change("w/b", { title: "T" })]])).toBe(false);
    expect(names([["task", { key: "w/b", value: aTask("w/b") }]])).toBe(false);
    expect(names([["message", anEntry(9, "w/a", "amber", "verdict").message]])).toBe(true);
    expect(names([["message", anEntry(9, "w/a").message]]), "a plain post").toBe(false);
    expect(names([["message", anEntry(9, "w/b", "amber", "verdict").message]])).toBe(false);
    expect(names([["branches", ["feature/a"]]])).toBe(true);
    expect(names([["branches", ["w/a"]]]), "not the branch they were answered for").toBe(false);
    // Before an answer, or one that names no branch, they follow the task's slug.
    expect(names([["branches", ["w/a"]]], null)).toBe(true);
    expect(names([["branches", ["w/a"]]], { ...GATES, branch: "" })).toBe(true);
    expect(names([["branches", ["feature/a"]]], null)).toBe(false);
  });

  it("is a card in a document the task links, and a run of its moved, as the page stood", () => {
    expect(names([["pending", { key: "/docs/X.md#c" }]])).toBe(true);
    expect(names([["pending", { key: "/docs/Y.md#c" }]])).toBe(false);
    expect(names([["pending", {}]]), "a card no key names").toBe(false);
    // The page's own work before the event: the same is no move, another is.
    expect(names([["machine-work", running]])).toBe(false);
    expect(names([["machine-work", aWork([])]])).toBe(true);
    expect(names([["machine-work", running]], answered, aPage())).toBe(true);
    // With no page drawn when it came, neither can be told.
    expect(names([["pending", { key: "/docs/X.md#c" }]], answered, null)).toBe(false);
    expect(names([["machine-work", aWork([])]], answered, null)).toBe(false);
  });

  it("is none of the rest, and any one change of several", () => {
    expect(names([["session", { key: "amber", value: { health: "stale" } }]])).toBe(false);
    expect(names([["tasks-status", { available: true }]])).toBe(false);
    expect(names([])).toBe(false);
    expect(
      names([
        ["message", anEntry(9, "w/a").message],
        ["branches", ["feature/a"]],
      ]),
    ).toBe(true);
  });

  it("asks the gates' part to read again, the answer shown meanwhile", () => {
    const part = gatesPart("", "w/a");
    const event = { changes: changes(["branches", ["feature/a"]]), page };
    expect(part.hear?.(answered, event)).toEqual({ value: answered, again: true });
    expect(part.hear?.(answered, { ...event, changes: changes(["branches", ["x"]]) })).toBeNull();
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

describe("what the feed leaves of a task's page", () => {
  const child = aTask("w/a/1", { parent: "w/a" });
  const page = () => aPage(aTask("w/a", { roles: [aRole("executor", "amber")] }), [child]);
  const record = (key: string, value: Task | null, removed = false): Change => ({
    region: "task",
    change: { key, value, removed },
  });

  it("reads the page again for its own record, and says it gone once removed", () => {
    const before = page();
    expect(hearPage("w/a", before, [record("w/a", aTask("w/a"))])).toEqual({
      value: before,
      again: true,
    });
    expect(hearPage("w/a", before, [record("w/a", null, true)])).toEqual({
      value: undefined,
      gone: true,
    });
    // A page that could not be found is read for a task filed under its slug.
    expect(hearPage("w/a", undefined, [record("w/a", aTask("w/a"))])).toEqual({
      value: undefined,
      again: true,
    });
  });

  it("puts a subtask that came, changed, left or was removed in its list", () => {
    const came = hearPage("w/a", page(), [record("w/a/2", aTask("w/a/2", { parent: "w/a" }))]);
    expect(came?.value?.children.map((task) => task.slug)).toEqual(["w/a/1", "w/a/2"]);
    expect(came?.again).toBe(true);
    const changed = hearPage("w/a", page(), [record("w/a/1", { ...child, title: "T" })]);
    expect([changed?.value?.children[0]?.title, changed?.again]).toEqual(["T", false]);
    const left = hearPage("w/a", page(), [record("w/a/1", { ...child, parent: "w/b" })]);
    expect([left?.value?.children, left?.again]).toEqual([[], true]);
    const removed = hearPage("w/a", page(), [record("w/a/1", null, true)]);
    expect([removed?.value?.children, removed?.again]).toEqual([[], true]);
    expect(hearPage("w/a", page(), [record("w/z", aTask("w/z"))])).toBeNull();
    expect(
      hearPage("w/a", undefined, [record("w/a/2", aTask("w/a/2", { parent: "w/a" }))]),
    ).toBeNull();
  });

  it("merges changed fields, read again only for what they do not carry", () => {
    const before = page();
    const titled = hearPage("w/a", before, [
      { region: "task-fields", change: change("w/a", { title: "Renamed" }) },
    ]);
    expect([titled?.value?.task.title, titled?.again]).toEqual(["Renamed", false]);
    const moved = hearPage("w/a", before, [
      { region: "task-fields", change: change("w/a", { status: "review" }) },
    ]);
    expect([moved?.value?.task.status, moved?.again]).toEqual(["review", true]);
    expect(
      hearPage("w/a", undefined, [
        { region: "task-fields", change: change("w/a", { status: "review" }) },
      ]),
    ).toBeNull();
  });

  it("says a holder's health, the machines' work, and reads again once the board is back", () => {
    const stale = hearPage("w/a", page(), [{ region: "session", key: "amber", health: "stale" }]);
    expect(stale?.value?.task.roles[0]?.health).toBe("stale");
    expect(
      hearPage("w/a", page(), [{ region: "session", key: "cedar", health: "stale" }]),
    ).toBeNull();
    expect(
      hearPage("w/a", undefined, [{ region: "session", key: "amber", health: "stale" }]),
    ).toBeNull();
    const work = aWork([]);
    expect(hearPage("w/a", page(), [{ region: "machine-work", work }])?.value?.machine_work).toBe(
      work,
    );
    expect(hearPage("w/a", undefined, [{ region: "machine-work", work }])).toBeNull();
    const before = page();
    expect(
      hearPage("w/a", before, [{ region: "tasks-status", available: true, slow: false }]),
    ).toEqual({ value: before, again: true });
    expect(
      hearPage("w/a", before, [{ region: "tasks-status", available: false, slow: false }]),
    ).toBeNull();
    // What the page does not follow leaves it as it is.
    expect(
      hearPage("w/a", before, [
        { region: "message", entry: anEntry(1) },
        { region: "pending", key: null },
        { region: "branches", moved: [] },
      ]),
    ).toBeNull();
  });
});

describe("a thread's newest page answered again", () => {
  const thread = (entries: TaskEntry[], before: number | null, gap: number | null = null) => ({
    entries,
    before,
    gap,
  });

  it("is the answer where nothing was shown", () => {
    expect(rejoined(thread([anEntry(3)], 2), thread([], null))).toEqual(thread([anEntry(3)], 2));
  });

  it("keeps every message shown, and the cursor before the earliest of them", () => {
    const held = thread([anEntry(1), anEntry(2), anEntry(3)], null);
    const next = rejoined(thread([anEntry(3), anEntry(4)], 2), held);
    expect(ids(next.entries)).toEqual([1, 2, 3, 4]);
    expect([next.before, next.gap]).toEqual([null, null]);
    expect(next.entries[2]).toBe(held.entries[2]);
  });

  it("names the gap where it starts after the newest shown, unless it is the whole thread", () => {
    const held = thread([anEntry(4), anEntry(5)], 3);
    const apart = rejoined(thread([anEntry(20), anEntry(21)], 19), held);
    expect([ids(apart.entries), apart.before, apart.gap]).toEqual([[4, 5, 20, 21], 3, 5]);
    expect(rejoined(thread([anEntry(20)], null), held).gap).toBeNull();
    // A gap not yet read stays one.
    expect(rejoined(thread([anEntry(5)], 4), { ...held, gap: 2 }).gap).toBe(2);
    expect(rejoined(thread([], null), held).gap).toBeNull();
  });
});

describe("what the feed leaves of a task's thread", () => {
  const message = (entry: TaskEntry): Change => ({ region: "message", entry });

  it("adds each message of its task once, in id order", () => {
    const thread = { entries: [anEntry(2)], before: 1, gap: null };
    const heard = hearThread("w/a", thread, [
      message(anEntry(5)),
      message(anEntry(2)),
      message(anEntry(3)),
      message(anEntry(9, "w/b")),
      { region: "pending", key: null },
    ]);
    expect(ids(heard?.value?.entries ?? [])).toEqual([2, 3, 5]);
    expect(heard?.value?.before).toBe(1);
  });

  it("is nothing for a thread not drawn, or an event with none of its messages", () => {
    expect(hearThread("w/a", undefined, [message(anEntry(1))])).toBeNull();
    const thread = { entries: [anEntry(2)], before: null, gap: null };
    expect(hearThread("w/a", thread, [message(anEntry(2)), message(anEntry(3, "w/b"))])).toBeNull();
  });
});

describe("a task's parts", () => {
  it("name their reads under the route's checkout, by the task's slug", () => {
    expect([pagePart("", "w/a").url, pagePart("", "w/a").key]).toEqual([PAGE_URL, "page w/a"]);
    expect([threadPart("", "w/a").url, threadPart("", "w/a").key]).toEqual([
      THREAD_URL,
      "thread w/a",
    ]);
    expect(pagePart("wt", "w/a").url).toBe(`/@wt${PAGE_URL}`);
    expect([gatesPart("", "w/a").url, gatesPart("", "w/a").key]).toEqual([GATES_URL, "gates w/a"]);
    expect([offerPart("", "w/a").url, offerPart("", "w/a").key]).toEqual([OFFER_URL, "offer w/a"]);
    expect([timelinePart("", "w/a").url, timelinePart("", "w/a").key]).toEqual([
      TIMELINE_URL,
      "timeline w/a",
    ]);
    // The page and the thread alone are kept on the device, and the thread is the first part.
    expect([pagePart("", "w/a").kept?.part, threadPart("", "w/a").kept?.part]).toEqual([
      PAGE,
      THREAD,
    ]);
    expect([
      gatesPart("", "w/a").kept,
      offerPart("", "w/a").kept,
      timelinePart("", "w/a").kept,
    ]).toEqual([undefined, undefined, undefined]);
    expect([threadPart("", "w/a").first, pagePart("", "w/a").first]).toEqual([true, undefined]);
  });

  it("read their answers, and refuse what they cannot", () => {
    expect(pagePart("", "w/a").read({ task: {} })).toBeNull();
    expect(threadPart("", "w/a").read({ entries: "none" })).toBeNull();
    expect(threadPart("", "w/a").read(history([anEntry(1)], 3))).toEqual({
      entries: [anEntry(1)],
      before: 3,
      gap: null,
    });
    expect(gatesPart("", "w/a").read({ branch: 1 })).toBeNull();
    expect(gatesPart("", "w/a").read(GATES)).toEqual(GATES);
    expect(offerPart("", "w/a").read({ action: 1 })).toBeNull();
    expect(offerPart("", "w/a").read(OFFER)).toEqual(OFFER);
    expect(timelinePart("", "w/a").read(null)).toBeNull();
    expect(timelinePart("", "w/a").read({ any: 1 })).toEqual({ any: 1 });
  });

  it("keep the subtasks the page read, and join a thread's answer to one the server gave", () => {
    const part = pagePart("", "w/a");
    const child = aTask("w/a/1", { parent: "w/a" });
    const next = aPage(aTask("w/a", { title: "Again" }));
    expect(part.merge?.(next, aPage(aTask("w/a"), [child]), true).children).toEqual([child]);
    expect(part.merge?.(next, aPage(), true)).toBe(next);
    expect(part.merge?.(next, undefined, false)).toBe(next);
    expect(part.absent?.(aPage(aTask("w/other")))).toBe(true);
    expect(part.absent?.(aPage())).toBe(false);
    const thread = threadPart("", "w/a");
    const held = { entries: [anEntry(1)], before: null, gap: null };
    const answered = { entries: [anEntry(2)], before: 1, gap: null };
    // Over this device's copy the answer stands alone.
    expect(thread.merge?.(answered, held, false)).toBe(answered);
    expect(thread.merge?.(answered, undefined, true)).toBe(answered);
    expect(ids(thread.merge?.(answered, held, true).entries ?? [])).toEqual([1, 2]);
  });
});

const PAGE_KEPT = { value: aPage(), etag: '"p1"', saved: 1_000 };
const THREAD_KEPT = { value: history([anEntry(1), anEntry(2)], 1), etag: '"t1"', saved: 2_000 };
const BOTH_KEPT = { "task:w/a page": PAGE_KEPT, "task:w/a thread": THREAD_KEPT };

/** A server that answers the page and the thread of `w/a` (and whatever else `more` does). */
const serves =
  (more: (url: string, etag: string) => Response | Promise<Response> | null = () => null): Route =>
  (url, etag) =>
    more(url, etag) ??
    (url === PAGE_URL
      ? answer(200, aPage(), '"p1"', 1)
      : url === THREAD_URL
        ? answer(200, history([anEntry(1)], null), '"t1"', 1)
        : answer(503));

describe("a task's page opened", () => {
  it("reads its page and its thread at once, behind nothing, and keeps each on the device", async () => {
    const body = aPage();
    const { task, asked, store, host } = live(serves());
    expect([task.page(), task.drawn(), task.missing(), task.error()]).toEqual([
      null,
      false,
      false,
      "",
    ]);
    task.open();
    await pass();
    expect(store.visit).toHaveBeenCalledWith(expect.anything(), "", "task:w/a", "task", false);
    expect(asked).toEqual([
      [PAGE_URL, ""],
      [THREAD_URL, ""],
    ]);
    expect(task.page()).toEqual(body);
    expect([task.drawn(), task.missing(), task.error()]).toEqual([true, false, ""]);
    expect(shownIds(task)).toEqual([1]);
    expect(task.thread()).toMatchObject({ answered: true, stale: false });
    expect(keptBy(store)).toEqual([
      ["task:w/a", PAGE, '"p1"', body],
      ["task:w/a", THREAD, '"t1"', history([anEntry(1)], null)],
    ]);
    expect(host.redraw).toHaveBeenCalled();
    expect(task.url("/api/x")).toBe("/api/x");
  });

  it("draws what this device kept of it at once, marked stale, and a 304 confirms each", async () => {
    const replies = [later<Response>(), later<Response>()];
    let at = 0;
    const { task, asked, store, host } = live(
      () => (replies[at++] as (typeof replies)[0]).promise,
      BOTH_KEPT,
    );
    task.open();
    await pass();
    expect(task.page()).toEqual(PAGE_KEPT.value);
    expect(shownIds(task)).toEqual([1, 2]);
    expect(task.thread()).toMatchObject({ stale: true, answered: false });
    expect(task.parts.fresh).toMatchObject({ saved: 1_000, unanswered: [PAGE_KEY, THREAD_KEY] });
    expect(asked).toEqual([
      [PAGE_URL, '"p1"'],
      [THREAD_URL, '"t1"'],
    ]);
    expect(host.freshness).toHaveBeenCalled();
    replies[0]?.settle(answer(304, null, "", 4));
    replies[1]?.settle(answer(304, null, "", 4));
    await pass();
    // Answered, it is the device's copy no more once the feed is open too.
    expect(task.parts.fresh).toEqual({ ...FRESH, saved: 1_000 });
    task.parts.freshen(opened);
    expect(task.parts.fresh).toEqual({ ...FRESH, open: true });
    expect(task.thread()).toMatchObject({ stale: false, answered: true });
    expect(store.keep).not.toHaveBeenCalled();
  });

  it("draws nothing kept of another task", async () => {
    const { task, asked } = live(() => new Promise<Response>(() => {}), {
      "task:w/a page": { ...PAGE_KEPT, value: aPage(aTask("w/b")) },
    });
    task.open();
    await pass();
    expect(task.page()).toBeNull();
    expect(asked[0]).toEqual([PAGE_URL, ""]);
  });

  it("is missing where the server has no such task, or answers another's: nothing read again", async () => {
    const none = live(
      (url) => (url === PAGE_URL ? answer(404, { error: "no task" }) : answer(503)),
      {
        "task:w/a page": PAGE_KEPT,
      },
    );
    none.task.open();
    await pass();
    expect([none.task.missing(), none.task.page(), none.task.drawn()]).toEqual([true, null, false]);
    expect(none.store.forget).toHaveBeenCalledWith(expect.anything(), "", "task:w/a");
    const other = live(
      serves((url) => (url === PAGE_URL ? answer(200, aPage(aTask("w/other")), '"p"', 1) : null)),
    );
    other.task.open();
    await pass();
    expect([other.task.missing(), other.task.page()]).toEqual([true, null]);
    await settled();
    expect(urls(other.asked).filter((url) => url === PAGE_URL)).toHaveLength(1);
  });

  it("draws its thread while its page's read fails, says why, and tries the page again", async () => {
    let refused = 1;
    const { task, asked } = live(
      serves((url) => (url === PAGE_URL && refused-- > 0 ? answer(503) : null)),
    );
    task.open();
    await pass();
    // The page failed; the thread drew all the same.
    expect(task.drawn()).toBe(false);
    expect(shownIds(task)).toEqual([1]);
    expect(task.error()).toBe("The server answered 503");
    expect(task.parts.fresh.failing).toEqual([PAGE_KEY]);
    await pass(1_000);
    expect(urls(asked)).toEqual([PAGE_URL, THREAD_URL, PAGE_URL]);
    expect([task.drawn(), task.error()]).toEqual([true, ""]);
    await settled();
    expect(asked).toHaveLength(3);
  });

  it("draws its page while its thread's read fails, and says the thread's failure", async () => {
    const { task } = live(
      serves((url) => (url === THREAD_URL ? answer(200, { entries: 1 }) : null)),
    );
    task.open();
    await pass();
    expect(task.drawn()).toBe(true);
    expect(task.thread().value).toBeUndefined();
    expect(task.error()).toBe("The task's thread is not readable");
  });

  it("says the failure of the owner's offer while its read fails, and no more once it answers", async () => {
    let refused = 1;
    const { task } = live(
      serves((url) =>
        url === OFFER_URL ? (refused-- > 0 ? answer(503) : answer(200, OFFER, "", 1)) : null,
      ),
    );
    task.open();
    task.parts.open(offerPart("", "w/a"));
    await pass();
    expect([task.drawn(), shownIds(task), task.error()]).toEqual([
      true,
      [1],
      "The server answered 503",
    ]);
    await pass(1_000);
    expect(task.error()).toBe("");
    await settled();
  });

  it("tries a read with no answer in time again by itself", async () => {
    let unanswered = 1;
    const { task, asked } = live(
      serves((url) =>
        url === PAGE_URL && unanswered-- > 0 ? new Promise<Response>(() => {}) : null,
      ),
    );
    task.open();
    await pass(ANSWER_MS);
    expect(task.error()).toBe(NOT_IN_TIME);
    await waitEnds();
    expect(urls(asked).filter((url) => url === PAGE_URL)).toHaveLength(2);
    expect([task.drawn(), task.error()]).toEqual([true, ""]);
  });

  it("says the server is unreachable through the notices, not the page's error, over a copy", async () => {
    const { task } = live(() => answer(503), BOTH_KEPT);
    task.open();
    await pass();
    expect(task.error()).toBe("");
    expect([...task.parts.fresh.failing].sort()).toEqual([PAGE_KEY, THREAD_KEY]);
  });

  it("reads the page again at once when the reader retries", async () => {
    const { task, asked } = live(serves());
    task.open();
    await pass();
    task.again();
    await pass();
    expect(urls(asked)).toEqual([PAGE_URL, THREAD_URL, PAGE_URL]);
    expect(asked[2]).toEqual([PAGE_URL, '"p1"']);
  });

  it("writes nothing once the page has stopped", async () => {
    const reply = later<Response>();
    const { task, page, store, given } = live(() => reply.promise);
    task.open();
    await pass();
    page.abort();
    expect(given.every(([, signal]) => signal?.aborted)).toBe(true);
    reply.settle(answer(200, aPage(), '"p1"', 1));
    await pass();
    expect(task.page()).toBeNull();
    expect(store.keep).not.toHaveBeenCalled();
  });
});

describe("a task's page moved to another task", () => {
  it("draws the other from this device and reads it, nothing of the one left shown or read", async () => {
    const left = later<Response>();
    const { task, asked, given, host } = live(
      (url) =>
        url === OTHER_URL
          ? answer(200, aPage(aTask("w/b")), '"b1"', 1)
          : url.includes("w%2Fa")
            ? left.promise
            : answer(200, history([], null), "", 1),
      {
        ...BOTH_KEPT,
        "task:w/b page": { value: aPage(aTask("w/b", { title: "Kept" })), etag: '"b0"', saved: 5 },
      },
    );
    task.open();
    task.parts.open(gatesPart("", "w/a"));
    await pass();
    expect(task.page()?.task.slug).toBe("w/a");
    host.redraw.mockClear();
    task.moved("w/b");
    expect(task.slug).toBe("w/b");
    // At once: nothing of the task it left, its reads given up.
    expect([task.page(), task.thread().value]).toEqual([null, undefined]);
    expect(task.parts.fresh).toEqual(FRESH);
    expect(host.redraw).toHaveBeenCalled();
    expect(
      given.filter(([url]) => url.includes("w%2Fa")).map(([, signal]) => signal?.aborted),
    ).toEqual([true, true, true]);
    await pass();
    expect(asked.slice(3)).toEqual([
      [OTHER_URL, '"b0"'],
      ["/api/task-history?slug=w%2Fb", ""],
    ]);
    expect(task.page()?.task.title).toBe("w/b");
    // The answer of the task it left, late, draws nothing.
    left.settle(answer(200, aPage(), '"p2"', 1));
    await pass();
    expect(task.page()?.task.slug).toBe("w/b");
  });

  it("does nothing to stay on the same task", async () => {
    const { task, asked, host } = live();
    task.moved("w/a");
    await pass();
    expect(asked).toEqual([]);
    expect(host.redraw).not.toHaveBeenCalled();
  });
});

describe("a task's thread", () => {
  it("is read again at the reader's ask", async () => {
    const { task, asked } = live(serves());
    task.open();
    await pass();
    await task.history(false);
    expect(asked.at(-1)).toEqual([THREAD_URL, '"t1"']);
  });

  it("reads the page before the earliest shown, never kept, and none before the first", async () => {
    const { task, asked, store } = live(
      serves((url) =>
        url === THREAD_URL
          ? answer(200, history([anEntry(3)], 2), '"t1"', 1)
          : url === `${THREAD_URL}&before=2`
            ? answer(200, history([anEntry(1)], null))
            : null,
      ),
    );
    task.open();
    await pass();
    store.keep.mockClear();
    await task.history(true);
    expect(urls(asked).at(-1)).toBe(`${THREAD_URL}&before=2`);
    expect(shownIds(task)).toEqual([1, 3]);
    expect(task.thread().value?.before).toBeNull();
    expect(store.keep).not.toHaveBeenCalled();
    await task.history(true);
    expect(asked).toHaveLength(3);
  });

  it("shares an earlier thread read between the timeline and the thread, and stops a stuck cursor", async () => {
    const reply = later<Response>();
    const { task, asked } = live(
      serves((url) =>
        url === THREAD_URL
          ? answer(200, history([anEntry(3)], 2), '"t1"', 1)
          : url.includes("before=")
            ? reply.promise
            : null,
      ),
    );
    task.open();
    await pass();
    const first = task.history(true);
    const second = task.history(true);
    expect(first).toBe(second);
    expect(urls(asked).filter((url) => url.includes("before="))).toHaveLength(1);
    reply.settle(answer(200, history([anEntry(3)], 2)));
    expect(await first).toContain("made no progress");
    expect(task.thread().value?.before).toBe(2);
  });

  it("waits for its answer to read earlier messages while it shows this device's copy", async () => {
    const reply = later<Response>();
    const { task, asked } = live(
      serves((url) =>
        url === THREAD_URL
          ? reply.promise
          : url.includes("before=")
            ? answer(200, history([anEntry(1)], null))
            : null,
      ),
      { "task:w/a thread": { value: history([anEntry(5)], 4), etag: '"t1"', saved: 1 } },
    );
    task.open();
    await pass();
    await task.history(true);
    expect(urls(asked)).toEqual([PAGE_URL, THREAD_URL]);
    // The answer's cursor, not the copy's.
    reply.settle(answer(200, history([anEntry(6)], 2), '"t2"', 1));
    await pass();
    expect(urls(asked)).toEqual([PAGE_URL, THREAD_URL, `${THREAD_URL}&before=2`]);
    expect(shownIds(task)).toEqual([1, 6]);
  });

  it("says why an earlier page's read failed, until the page answers again", async () => {
    const { task, host } = live(
      serves((url) =>
        url === THREAD_URL
          ? answer(200, history([anEntry(3)], 2), '"t1"', 1)
          : url.includes("before=")
            ? answer(200, { entries: "none" })
            : null,
      ),
    );
    task.open();
    await pass();
    host.redraw.mockClear();
    await task.history(true);
    expect(task.error()).toBe("The task's thread is not readable");
    expect(host.redraw).toHaveBeenCalled();
    task.again();
    await pass();
    expect(task.error()).toBe("");
  });

  it("says nothing of a read that failed for a task the page left", async () => {
    const reply = later<Response>();
    const { task } = live(
      serves((url) =>
        url === THREAD_URL
          ? answer(200, history([anEntry(3)], 2), '"t1"', 1)
          : url.includes("before=")
            ? reply.promise
            : answer(404),
      ),
    );
    task.open();
    await pass();
    const reading = task.history(true);
    task.moved("w/b");
    reply.settle(answer(503));
    await reading;
    await pass();
    expect(task.error()).toBe("");
    const answered = later<Response>();
    const other = live(
      serves((url) =>
        url === THREAD_URL
          ? answer(200, history([anEntry(3)], 2), '"t1"', 1)
          : url.includes("before=")
            ? answered.promise
            : answer(404),
      ),
    );
    other.task.open();
    await pass();
    const earlier = other.task.history(true);
    other.task.moved("w/b");
    answered.settle(answer(200, history([anEntry(1)], null)));
    await earlier;
    expect(other.task.thread().value).toBeUndefined();
  });

  /** A thread answered `[4, 5]`, then, read again, a newest page that starts after them. */
  const apart = (pages: Record<string, Response>) => {
    let reads = 0;
    return live(
      serves((url) => {
        if (url === THREAD_URL) {
          reads += 1;
          return reads === 1
            ? answer(200, history([anEntry(4), anEntry(5)], 3), '"t1"', 1)
            : answer(200, history([anEntry(20), anEntry(21)], 19), '"t2"', 9);
        }
        return pages[url] ?? null;
      }),
    );
  };

  it("catches up from its newest message a page at a time when its answer starts after it", async () => {
    const { task, asked } = apart({
      [`${THREAD_URL}&after=5`]: answer(200, history([anEntry(6), anEntry(7)], null, 7)),
      [`${THREAD_URL}&after=7`]: answer(200, history([anEntry(8)], null, null)),
    });
    task.open();
    await pass();
    await task.history(false);
    await pass();
    expect(urls(asked).slice(2)).toEqual([
      THREAD_URL,
      `${THREAD_URL}&after=5`,
      `${THREAD_URL}&after=7`,
    ]);
    expect(shownIds(task)).toEqual([4, 5, 6, 7, 8, 20, 21]);
    expect(task.thread().value).toMatchObject({ before: 3, gap: null });
  });

  it("stops catching up at a cursor that does not move, or a read that fails", async () => {
    const still = apart({ [`${THREAD_URL}&after=5`]: answer(200, history([], null, 5)) });
    still.task.open();
    await pass();
    await still.task.history(false);
    await pass();
    expect(urls(still.asked).filter((url) => url.includes("after="))).toHaveLength(1);
    expect(still.task.thread().value?.gap).toBeNull();

    const failing = apart({ [`${THREAD_URL}&after=5`]: answer(200, { entries: 3 }) });
    failing.task.open();
    await pass();
    await failing.task.history(false);
    await pass();
    expect(failing.task.error()).toBe("The task's thread is not readable");
    // The gap stays, for the thread's next answer to read from.
    expect(failing.task.thread().value?.gap).toBe(5);
    await failing.task.history(false);
    await pass();
    expect(urls(failing.asked).filter((url) => url.includes("after="))).toHaveLength(2);
  });

  it("catches up once however often it is asked meanwhile, and not for a task it left", async () => {
    const reply = later<Response>();
    let reads = 0;
    const { task, asked } = live(
      serves((url) => {
        if (url === THREAD_URL) {
          reads += 1;
          return reads === 1
            ? answer(200, history([anEntry(4), anEntry(5)], 3), '"t1"', 1)
            : answer(200, history([anEntry(20 + reads)], 19), `"t${reads}"`, 9);
        }
        return url.includes("after=") ? reply.promise : url.includes("w%2Fb") ? answer(404) : null;
      }),
    );
    task.open();
    await pass();
    await task.history(false);
    await task.history(false);
    await pass();
    expect(urls(asked).filter((url) => url.includes("after="))).toHaveLength(1);
    task.moved("w/b");
    reply.settle(answer(200, history([anEntry(6)], null, null)));
    await pass();
    expect(task.thread().value).toBeUndefined();
  });
});

describe("a task's subtasks", () => {
  const counted = (count: number) =>
    serves((url) =>
      url === PAGE_URL
        ? answer(200, aPage(aTask("w/a"), [], { children_count: count }), '"p1"', 1)
        : null,
    );
  const CHILDREN_URL = "/api/task-board?parent=w%2Fa&older=1";

  it("are read a page at a time, the next from the cursor, counted by their columns", async () => {
    const first = [aTask("w/a/1", { parent: "w/a" }), aTask("w/a/2", { parent: "w/a" })];
    const second = [aTask("w/a/3", { parent: "w/a" })];
    const counts: [string, number][] = [
      ["ready", 2],
      ["done", 1],
    ];
    const base = counted(3);
    const { task, asked } = live((url, etag) =>
      url.includes("cursor=")
        ? answer(200, listing(second, counts))
        : url.startsWith("/api/task-board")
          ? answer(200, listing(first, counts, "c 1"))
          : base(url, etag),
    );
    task.open();
    await pass();
    await task.children(false);
    expect(task.page()?.children.map((child) => child.slug)).toEqual(["w/a/1", "w/a/2"]);
    await task.children(true);
    expect(urls(asked).slice(2)).toEqual([CHILDREN_URL, `${CHILDREN_URL}&cursor=c%201`]);
    expect(task.page()?.children.map((child) => child.slug)).toEqual(["w/a/1", "w/a/2", "w/a/3"]);
    expect(task.page()?.children_count).toBe(3);
    // The page read again keeps them.
    task.again();
    await pass();
    expect(task.page()?.children).toHaveLength(3);
  });

  it("say why a read failed", async () => {
    const base = counted(1);
    const { task } = live((url, etag) =>
      url.startsWith("/api/task-board") ? answer(200, { tasks: "none" }) : base(url, etag),
    );
    task.open();
    await pass();
    await task.children(false);
    expect(task.error()).toBe("The task board's answer is not a listing");
  });

  it("asked for over this device's copy, are read once the page is answered", async () => {
    const reply = later<Response>();
    const { task, asked } = live(
      (url) =>
        url === PAGE_URL
          ? reply.promise
          : url.startsWith("/api/task-board")
            ? answer(200, listing([aTask("w/a/1", { parent: "w/a" })], [["ready", 2]]))
            : answer(304, null, "", 1),
      {
        ...BOTH_KEPT,
        "task:w/a page": { ...PAGE_KEPT, value: aPage(aTask("w/a"), [], { children_count: 2 }) },
      },
    );
    task.open();
    await pass();
    await task.children(false);
    expect(urls(asked)).toEqual([PAGE_URL, THREAD_URL]);
    reply.settle(answer(304, null, "", 1));
    await pass();
    expect(urls(asked)).toEqual([PAGE_URL, THREAD_URL, CHILDREN_URL]);
    expect(task.page()?.children.map((child) => child.slug)).toEqual(["w/a/1"]);
  });

  it("are not read with a page that counts none, nor before they were asked for", async () => {
    const { task, asked } = live(counted(0));
    task.open();
    await pass();
    task.again();
    await pass();
    expect(urls(asked).filter((url) => url.startsWith("/api/task-board"))).toEqual([]);
    const unasked = live(counted(2));
    unasked.task.open();
    await pass();
    expect(urls(unasked.asked)).toEqual([PAGE_URL, THREAD_URL]);
  });

  it("are read again with the page when a task came under it", async () => {
    const base = counted(1);
    let listed = [aTask("w/a/1", { parent: "w/a" })];
    const { task, asked } = live((url, etag) =>
      url.startsWith("/api/task-board")
        ? answer(200, listing(listed, [["ready", listed.length]]))
        : base(url, etag),
    );
    task.open();
    await pass();
    await task.children(false);
    listed = [...listed, aTask("w/c", { parent: "w/a" })];
    task.heard({ ...event(["task-fields", change("w/c", { parent: "w/a" })]), version: 2 });
    await pass();
    expect(urls(asked).slice(2)).toEqual([CHILDREN_URL, PAGE_URL, CHILDREN_URL]);
    expect(task.page()?.children.map((child) => child.slug)).toEqual(["w/a/1", "w/c"]);
    expect(task.page()?.children_count).toBe(2);
  });

  it("change nothing of a task the page left while they were read", async () => {
    const reply = later<Response>();
    const base = counted(1);
    const { task } = live((url, etag) =>
      url.startsWith("/api/task-board")
        ? reply.promise
        : url.includes("w%2Fb")
          ? answer(404)
          : base(url, etag),
    );
    task.open();
    await pass();
    const reading = task.children(false);
    task.moved("w/b");
    reply.settle(answer(200, listing([aTask("w/a/1", { parent: "w/a" })], [["ready", 1]])));
    await reading;
    expect(task.page()).toBeNull();
  });
});

describe("a feed event on a task's page", () => {
  /** A page showing `w/a`, which links a document, its gates, its offer and its timeline. */
  async function shown(more: Parameters<typeof serves>[0] = () => null) {
    const page = live(
      serves(
        (url, etag) =>
          more(url, etag) ??
          (url === PAGE_URL
            ? answer(200, aPage(aTask("w/a", { links: ["docs/X.md"] })), '"p1"', 1)
            : url === GATES_URL
              ? answer(200, { ...GATES, branch: "feature/a" }, "", 1)
              : url === OFFER_URL
                ? answer(200, OFFER, "", 1)
                : url === TIMELINE_URL
                  ? answer(200, { rows: [] }, "", 1)
                  : null),
      ),
    );
    page.task.open();
    page.task.parts.open(gatesPart("", "w/a"));
    page.task.parts.open(offerPart("", "w/a"));
    page.task.parts.open(timelinePart("", "w/a"));
    await pass();
    page.asked.length = 0;
    return page;
  }
  let version = 1;
  /** The next feed event, of `changes`. */
  const next = (...changes: [string, unknown][]) => {
    version += 1;
    return { ...event(...changes), version };
  };

  it("holds the reader's place, and loses the stream when it cannot be read, changing nothing", async () => {
    const { task, held, host } = await shown();
    const before = task.page();
    host.redraw.mockClear();
    expect(task.heard(next(["message", {}], ["machine-work", CASES.samples.work]))).toBe(false);
    expect(task.heard({ changes: [] })).toBe(false);
    expect(task.page()).toBe(before);
    expect(host.redraw).not.toHaveBeenCalled();
    expect(held).not.toHaveBeenCalled();
    expect(task.heard(next())).toBe(true);
    expect(held).toHaveBeenCalledWith("[id^=message-], [id^=child-], [data-slug]");
  });

  it("holds the reader's place, then draws the change at once, within the event", async () => {
    const { task, held, host } = await shown();
    const order: string[] = [];
    held.mockImplementation(() => order.push("held"));
    host.redraw.mockImplementation(() => order.push(react.flushing ? "drawn" : "drawn later"));
    // A message of the page's task, which its thread draws.
    expect(task.heard(next(["message", anEntry(7).message]))).toBe(true);
    // Another task's message changes nothing the page draws: held, and nothing drawn.
    expect(task.heard(next(["message", anEntry(8, "w/b").message]))).toBe(true);
    // The board is slow to answer: the page's notice, drawn by the page itself.
    expect(task.heard(next(["tasks-status", { available: false, slow: true }]))).toBe(true);
    // As it was when the events were heard: a read they started answers after.
    expect([...order]).toEqual(["held", "drawn", "held", "held", "drawn"]);
    expect(react.flushing).toBe(false);
  });

  it("adds a message of its task once, and a verdict reads its gates again", async () => {
    const { task, asked } = await shown();
    const first = next(["message", anEntry(7).message]);
    task.heard(first);
    task.heard(first);
    task.heard(next(["message", anEntry(8, "w/b").message]));
    await pass();
    expect([shownIds(task), asked]).toEqual([[1, 7], []]);
    task.heard(next(["message", anEntry(9, "w/a", "amber", "verdict").message]));
    await pass();
    expect([shownIds(task), urls(asked)]).toEqual([[1, 7, 9], [GATES_URL]]);
  });

  it("drops an event at or before the answer its part holds", async () => {
    const { task } = await shown();
    task.heard({ ...event(["message", anEntry(7).message]), version: 1 });
    task.heard({
      ...event(["task-fields", change("w/a", { title: "Older" })]),
      version: 1,
    });
    expect(shownIds(task)).toEqual([1]);
    expect(task.page()?.task.title).toBe("w/a");
  });

  it("reads the gates again for a card in a document it links and a new tip of their branch", async () => {
    const { task, asked } = await shown();
    task.heard(next(["pending", { key: "/docs/Y.md#c" }], ["branches", ["w/b", "w/a"]]));
    task.heard(next(["pending", {}]));
    await pass();
    expect(asked).toEqual([]);
    task.heard(next(["pending", { key: "/docs/X.md#c" }]));
    await pass();
    expect(urls(asked)).toEqual([GATES_URL]);
    task.heard(next(["branches", ["feature/a"]]));
    await pass();
    expect(urls(asked)).toEqual([GATES_URL, GATES_URL]);
  });

  it("follows the task's slug as its branch before the gates answered", async () => {
    const { task, asked } = live(
      serves((url) => (url === GATES_URL ? answer(200, GATES, "", 5) : null)),
    );
    task.open();
    await pass();
    task.parts.open(gatesPart("", "w/a"));
    asked.length = 0;
    task.heard(next(["branches", ["w/a"]]));
    await pass();
    // Its first read was out: the one read serves, its answer newer than the event.
    expect(urls(asked)).toEqual([GATES_URL]);
  });

  it("reads the gates again when one of the task's builds or tests moved", async () => {
    const { task, asked } = await shown();
    task.heard(next(["machine-work", aWork([])]));
    await pass();
    expect(asked).toEqual([]);
    const work = aWork([
      aMachine("m", [anOperation("m", "1", "test", "running", { task: "w/a" })]),
    ]);
    task.heard(next(["machine-work", work]));
    await pass();
    expect(task.page()?.machine_work).toEqual(work);
    expect(urls(asked)).toEqual([GATES_URL]);
  });

  it("reads the gates no more for an event their answer already holds", async () => {
    const { task, asked } = await shown();
    // The gates were answered at version 1: each of these is in that answer.
    task.heard({
      ...event(["message", anEntry(9, "w/a", "amber", "verdict").message]),
      version: 1,
    });
    task.heard({ ...event(["branches", ["feature/a"]]), version: 0 });
    task.heard({ ...event(["pending", { key: "/docs/X.md#c" }]), version: 1 });
    await pass();
    expect(asked).toEqual([]);
    task.heard(next(["branches", ["feature/a"]]));
    await pass();
    expect(urls(asked)).toEqual([GATES_URL]);
  });

  it("reads the gates again when an answer read while their branch moved is older than the move", async () => {
    const reply = later<Response>();
    const { task, asked } = live(serves((url) => (url === GATES_URL ? reply.promise : null)));
    task.open();
    task.parts.open(gatesPart("", "w/a"));
    await pass();
    asked.length = 0;
    // Before their answer they follow the slug: the branch they will name moves meanwhile.
    task.heard(next(["branches", ["feature/a"]]));
    await pass();
    expect(asked).toEqual([]);
    reply.settle(answer(200, { ...GATES, branch: "feature/a" }, "", version - 1));
    await pass();
    // Judged on that answer, the move is theirs and after it: read once more.
    expect(urls(asked)).toEqual([GATES_URL]);
    expect(task.parts.shown(gatesPart("", "w/a")).value?.branch).toBe("feature/a");
  });

  it("says a holder's health on every role it holds", async () => {
    const { task } = await shown((url) =>
      url === PAGE_URL
        ? answer(200, aPage(aTask("w/a", { roles: [aRole("executor", "amber")] })), '"p1"', 1)
        : null,
    );
    task.heard(next(["session", { key: "amber", value: { health: "stale" } }]));
    expect(task.page()?.task.roles[0]?.health).toBe("stale");
  });

  it("says the board's state, and reads the page again once it is back", async () => {
    const { task, asked, host } = await shown();
    host.redraw.mockClear();
    task.heard(next(["tasks-status", { available: false, slow: false }]));
    expect(task.error()).toBe("The task board is unavailable. Reconnecting…");
    expect(host.redraw).toHaveBeenCalled();
    expect(asked).toEqual([]);
    task.heard(next(["tasks-status", { available: true }]));
    expect(task.error()).toBe("");
    await pass();
    expect(urls(asked)).toEqual([PAGE_URL]);
  });

  it("reads the page again for a new brief or status, never for a title", async () => {
    const { task, asked } = await shown();
    task.heard(next(["task-fields", { key: "w/a", fields: { title: "Renamed" } }]));
    await pass();
    expect(task.page()?.task.title).toBe("Renamed");
    // Its own task changed: the parts that follow it are read, the page not.
    expect(urls(asked).sort()).toEqual([GATES_URL, OFFER_URL, TIMELINE_URL].sort());
    asked.length = 0;
    task.heard(next(["task-fields", { key: "w/a", fields: { status: "review" } }]));
    await pass();
    expect(urls(asked)).toContain(PAGE_URL);
  });

  it("holds no offer for a task changed since it was asked, and asks again", async () => {
    const reply = later<Response>();
    let offers = 0;
    const { task, asked } = await shown((url) =>
      url === OFFER_URL && offers++ > 0 ? reply.promise : null,
    );
    const offer = offerPart("", "w/a");
    expect(task.parts.shown(offer).value).toEqual(OFFER);
    const changed = next(["task", { key: "w/a", value: aTask("w/a") }]);
    task.heard(changed);
    expect(task.parts.shown(offer).value).toEqual({ ...OFFER, payload: null });
    // One with no payload stays as it is.
    const bare = task.parts.shown(offer).value;
    task.heard(next(["task", { key: "w/a", value: aTask("w/a") }]));
    expect(task.parts.shown(offer).value).toBe(bare);
    reply.settle(answer(200, { ...OFFER, label: "Land again" }, "", version));
    await pass();
    expect(task.parts.shown(offer).value).toEqual({ ...OFFER, label: "Land again" });
    // One read of the offer for the two events, its answer as new as the later.
    expect(urls(asked).filter((url) => url === OFFER_URL)).toHaveLength(1);
    expect(new Set(urls(asked))).toEqual(new Set([GATES_URL, OFFER_URL, PAGE_URL, TIMELINE_URL]));
  });

  it("draws no offer asked before the task changed that lands after it, and asks again", async () => {
    const before = later<Response>();
    const after = later<Response>();
    let offers = 0;
    const { task, asked } = await shown((url) =>
      url === OFFER_URL ? (offers++ === 0 ? before.promise : after.promise) : null,
    );
    const offer = offerPart("", "w/a");
    expect(task.parts.shown(offer).value).toBeUndefined();
    // The task changes while the offer's first read is out.
    task.heard(next(["task", { key: "w/a", value: aTask("w/a") }]));
    // Its answer was read before the change: the offer of the task as it was, so no payload.
    before.settle(answer(200, OFFER, "", version - 1));
    await pass();
    expect(task.parts.shown(offer).value).toEqual({ ...OFFER, payload: null });
    // (The first read was asked as the page opened, before these were counted.)
    expect(urls(asked).filter((url) => url === OFFER_URL)).toHaveLength(1);
    // The read after the change brings the offer that stands.
    after.settle(answer(200, { ...OFFER, label: "Land again" }, "", version));
    await pass();
    expect(task.parts.shown(offer).value).toEqual({ ...OFFER, label: "Land again" });
    expect(urls(asked).filter((url) => url === OFFER_URL)).toHaveLength(1);
  });

  it("leaves the offer and the timeline as they are for a change at or before their answers", async () => {
    const { task, asked } = await shown();
    const offer = offerPart("", "w/a");
    asked.length = 0;
    // An event the answers already hold: nothing is nulled, nothing read.
    task.heard({ ...event(["task", { key: "w/a", value: aTask("w/a") }]), version: 1 });
    await pass();
    expect(task.parts.shown(offer).value).toEqual(OFFER);
    expect(urls(asked).filter((url) => url === OFFER_URL || url === TIMELINE_URL)).toEqual([]);
  });

  it("says its task was removed, and reads one filed under the slug it could not find", async () => {
    const { task, asked } = await shown();
    task.heard(next(["task", { key: "w/a", removed: true }]));
    expect([task.page(), task.missing()]).toEqual([null, true]);
    asked.length = 0;
    task.heard(next(["task", { key: "w/a", value: aTask("w/a") }]));
    await pass();
    expect(urls(asked)).toContain(PAGE_URL);
    expect(task.missing()).toBe(false);
  });

  it("asks nothing for a task that is not the route's, nor of a page that has drawn nothing", async () => {
    const { task, asked } = await shown();
    task.heard(next(["task-fields", { key: "w/b", fields: { status: "review", parent: "w/z" } }]));
    task.heard(next(["task", { key: "w/b", value: aTask("w/b") }]));
    await pass();
    expect(asked).toEqual([]);
    const bare = live(() => new Promise<Response>(() => {}));
    bare.task.open();
    await pass();
    bare.asked.length = 0;
    bare.host.redraw.mockClear();
    bare.task.heard(
      next(
        ["task-fields", { key: "w/a/9", fields: { title: "T" } }],
        ["session", { key: "amber" }],
        ["pending", { key: "/docs/X.md" }],
        ["branches", ["w/b"]],
        ["machine-work", aWork([])],
      ),
    );
    await pass();
    expect([bare.task.page(), bare.asked]).toEqual([null, []]);
    expect(bare.host.redraw).not.toHaveBeenCalled();
  });

  it("is applied again to an answer read while it came", async () => {
    const reply = later<Response>();
    let reads = 0;
    const { task } = await shown((url) => (url === PAGE_URL && reads++ > 0 ? reply.promise : null));
    task.again();
    await pass();
    task.heard({ ...event(["task-fields", change("w/a", { title: "Meanwhile" })]), version: 6 });
    reply.settle(answer(200, aPage(aTask("w/a", { title: "Before" })), '"p2"', 5));
    await pass();
    expect(task.page()?.task.title).toBe("Meanwhile");
  });
});

describe("the board under a task's page", () => {
  const FRAME: FrameRefresh = { refresh: vi.fn(), connected: vi.fn(), heard: vi.fn() };
  const BOARD = { version: 3, ping_ms: 1_000, build: "b" };

  it("is shown once, whatever of the task is drawn", async () => {
    const { task, host } = live();
    const run = task.run(FRAME);
    run.show(BOARD);
    expect([run.board.current, task.boardShown]).toEqual([BOARD, true]);
    expect(host.redraw).toHaveBeenCalledTimes(1);
    run.show({ ...BOARD, version: 4 });
    expect(run.board.current?.version).toBe(4);
    expect(host.redraw).toHaveBeenCalledTimes(1);
    expect(run.retry).toBe(task.retry);
    expect(run.views.frame).toBe(FRAME);
  });

  it("says its trouble and its staleness on the page, and nothing once stopped", async () => {
    const { task, host, page } = live();
    const run = task.run(FRAME);
    run.setTrouble("The board could not be read");
    expect(task.trouble).toBe("The board could not be read");
    run.setStale(true);
    expect(host.stale).toHaveBeenCalledWith(true);
    page.abort();
    run.setTrouble("Later");
    run.show(BOARD);
    expect([task.trouble, task.boardShown]).toEqual(["The board could not be read", false]);
  });

  it("makes the board's loop wait for nothing, and reads again a part answered at another version", async () => {
    let reads = 0;
    // Each part answered at version 1, then at the board's.
    const { task, asked } = live((url) =>
      url === PAGE_URL
        ? answer(200, aPage(), '"p1"', reads++ < 2 ? 1 : 3)
        : answer(200, history([anEntry(1)], null), '"t1"', reads++ < 2 ? 1 : 3),
    );
    const run = task.run(FRAME);
    // Before the board is read, there is no version to judge by.
    expect(run.views.read()).toBeUndefined();
    task.open();
    await pass();
    run.show(BOARD);
    expect(run.views.read()).toBeUndefined();
    await pass();
    // Both were answered at version 1; the feed opens from 3.
    expect(urls(asked)).toEqual([PAGE_URL, THREAD_URL, PAGE_URL, THREAD_URL]);
    // Opened again from the same version, nothing is read.
    run.views.read();
    await pass();
    expect(asked).toHaveLength(4);
    run.views.freshen((f) => ({ ...f, open: true }));
    expect(task.parts.fresh.open).toBe(true);
    expect(run.views.heard({ ...event(), version: 4 })).toBe(true);
    expect(run.views.heard({})).toBe(false);
  });

  it("starts the task's reads beside the board's, and follows the board until the page stops", async () => {
    const { task, asked, page, store } = live(() => answer(503));
    const listens = { addEventListener: () => {} };
    vi.stubGlobal("window", { ...listens, reviewHoldPlace: () => {} });
    vi.stubGlobal("document", { ...listens, getElementById: () => null });
    const started = task.start(FRAME);
    await pass();
    expect(store.visit).toHaveBeenCalled();
    expect(urls(asked).sort()).toEqual([BOARD_PATH, PAGE_URL, THREAD_URL].sort());
    expect(task.trouble).not.toBeNull();
    page.abort();
    await pass(60_000);
    await started;
    expect(asked).toHaveLength(3);
  });

  /**
   * A page started on a server whose board answers once the test says (`answerBoard`), at
   * `versions.next` from then on, and whose other reads `route` answers, with the feed's sockets
   * it makes (`opened`).
   */
  const begun = (route: Route, kept: Record<string, Kept> = {}) => {
    const board = later<Response>();
    const versions = { next: 7, reads: 0 };
    const run = live((url, etag) => {
      if (url !== BOARD_PATH) return route(url, etag);
      versions.reads += 1;
      return versions.reads === 1
        ? board.promise
        : answer(200, { ...(CASES.samples.board as object), version: versions.next }, '"b2"');
    }, kept);
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
    return { ...run, opened, done, answerBoard, versions };
  };
  /** Feed event `version`, of `changes`, as the socket says it. */
  const said = (version: number, ...changes: [string, unknown][]) =>
    JSON.stringify({ ...event(...changes), version });
  /** The reads asked, in an order of their own, the board's apart from the task's. */
  const sorted = (asked: [string, string][]) => [...asked].sort(([a], [b]) => a.localeCompare(b));

  it("draws the device's copy and asks for the page and the thread before the board answers", async () => {
    const { task, asked, opened, page, done, answerBoard } = begun(
      () => answer(304, null, "", 7),
      BOTH_KEPT,
    );
    await pass();
    // Behind nothing: each part's read beside the board's; only the feed waits for its version.
    expect(task.page()?.task.slug).toBe("w/a");
    expect(sorted(asked)).toEqual([
      [BOARD_PATH, ""],
      [THREAD_URL, '"t1"'],
      [PAGE_URL, '"p1"'],
    ]);
    expect(task.thread().answered).toBe(true);
    expect(opened).toEqual([]);
    // Gone before the board answers: its answer starts nothing.
    page.abort();
    answerBoard();
    await done;
    expect(asked).toHaveLength(3);
    expect(opened).toEqual([]);
  });

  it("opens the feed at the board's answer, while the page's and the thread's reads are out", async () => {
    const pageRead = later<Response>();
    const threadRead = later<Response>();
    const { task, asked, opened, page, done, answerBoard } = begun(
      (url) => (url === PAGE_URL ? pageRead.promise : threadRead.promise),
      BOTH_KEPT,
    );
    answerBoard();
    await pass();
    // One read of each, and the one socket, from the board's version: none waits for another.
    expect(asked).toHaveLength(3);
    expect(opened.map(({ url }) => url)).toEqual(["ws://127.0.0.1/api/feed?since=7&document="]);
    const source = opened[0]?.source;
    source?.dispatchEvent(new Event("open"));
    await pass();
    // Open, with neither read answered: still this device's copy.
    expect(task.parts.fresh).toMatchObject({
      open: true,
      saved: 1_000,
      unanswered: [PAGE_KEY, THREAD_KEY],
    });
    // What the feed says before the answers shows at once, on the copy.
    say(source, FEED_EVENT, said(8, ["task-fields", change("w/a", { title: "Heard" })]));
    say(source, FEED_EVENT, said(9, ["message", anEntry(5).message]));
    await pass();
    expect(task.page()?.task.title).toBe("Heard");
    expect(shownIds(task)).toEqual([1, 2, 5]);
    // The page as the server had it before the event: the event is applied onto its answer.
    pageRead.settle(answer(200, aPage(aTask("w/a", { title: "Read", updated: 3 })), '"p2"', 7));
    await pass();
    expect([task.page()?.task.title, task.page()?.task.updated]).toEqual(["Heard", 3]);
    expect(waits(task.parts.fresh, PAGE_KEY)).toBe(false);
    // The thread's newest page, without the message the feed brought: it stays.
    threadRead.settle(answer(200, history([anEntry(1), anEntry(2)], null), '"t2"', 7));
    await pass();
    expect(shownIds(task)).toEqual([1, 2, 5]);
    expect(task.parts.fresh).toEqual({ ...FRESH, open: true });
    // Three reads in all, and the one socket still open.
    expect(asked).toHaveLength(3);
    expect(opened.map(({ closed }) => closed)).toEqual([0]);
    page.abort();
    await done;
    expect(opened.map(({ closed }) => closed)).toEqual([1]);
  });

  it("opens the feed beside a first visit's read, whose answer takes what was heard meanwhile", async () => {
    const pageRead = later<Response>();
    const { task, asked, opened, page, done, answerBoard } = begun((url) =>
      url === PAGE_URL ? pageRead.promise : answer(200, history([], null), '"t"', 7),
    );
    answerBoard();
    await pass();
    expect(asked).toHaveLength(3);
    expect(opened).toHaveLength(1);
    const source = opened[0]?.source;
    source?.dispatchEvent(new Event("open"));
    // With no page to change yet, the event is kept for the answer.
    say(source, FEED_EVENT, said(8, ["task-fields", change("w/a", { title: "Heard" })]));
    await pass();
    expect(task.page()).toBeNull();
    pageRead.settle(answer(200, aPage(aTask("w/a", { title: "Read" })), '"p1"', 7));
    await pass();
    expect(task.page()?.task.title).toBe("Heard");
    expect(task.parts.fresh).toEqual({ ...FRESH, open: true });
    expect(asked).toHaveLength(3);
    page.abort();
    await done;
  });

  it("drops what the feed says at or before a read's answer", async () => {
    const { task, opened, page, done, answerBoard } = begun((url) =>
      url === PAGE_URL
        ? answer(200, aPage(aTask("w/a", { title: "Read" })), '"p1"', 9)
        : answer(200, history([anEntry(4)], null), '"t"', 9),
    );
    answerBoard();
    await pass();
    const source = opened[0]?.source;
    source?.dispatchEvent(new Event("open"));
    // The feed opened from 7; the answers were read at 9, so these are in them already.
    say(source, FEED_EVENT, said(8, ["task-fields", change("w/a", { title: "Older" })]));
    say(source, FEED_EVENT, said(9, ["task-fields", change("w/a", { title: "Old" })]));
    await pass();
    expect(task.page()?.task.title).toBe("Read");
    say(source, FEED_EVENT, said(10, ["task-fields", change("w/a", { title: "Newer" })]));
    await pass();
    expect(task.page()?.task.title).toBe("Newer");
    page.abort();
    await done;
  });

  it("tries a failed read again while the feed stays open", async () => {
    let tries = 0;
    const { task, asked, opened, page, done, answerBoard } = begun((url) => {
      if (url !== PAGE_URL) return answer(304, null, "", 7);
      tries += 1;
      return tries === 1 ? answer(503) : answer(304, null, "", 7);
    }, BOTH_KEPT);
    answerBoard();
    await pass();
    opened[0]?.source.dispatchEvent(new Event("open"));
    await pass();
    expect(task.parts.fresh).toMatchObject({
      open: true,
      unanswered: [PAGE_KEY],
      failing: [PAGE_KEY],
    });
    // The first of a failed read's waits, shorter than the feed's silence.
    await pass(1_000);
    expect(task.parts.fresh).toEqual({ ...FRESH, open: true });
    expect(urls(asked).filter((url) => url === PAGE_URL)).toHaveLength(2);
    expect(opened).toHaveLength(1);
    page.abort();
    await done;
  });

  it("reads again, once the feed opens again, only the parts answered before its new version", async () => {
    let threads = 0;
    const { task, asked, opened, page, done, answerBoard, versions } = begun((url) => {
      if (url === PAGE_URL) return answer(200, aPage(), '"p1"', 9);
      if (url === THREAD_URL) {
        threads += 1;
        return threads === 1
          ? answer(200, history([anEntry(4), anEntry(5)], 3), '"t1"', 7)
          : answer(200, history([anEntry(9), anEntry(10)], 8), '"t2"', 9);
      }
      // The messages posted while the feed was down, before the newest page's.
      if (url === `${THREAD_URL}&after=5`) return answer(200, history([anEntry(6), anEntry(7)]));
      return answer(200, history([]));
    });
    answerBoard();
    await pass();
    expect(shownIds(task)).toEqual([4, 5]);
    // The feed is lost; after its wait the board is read again, at a later version.
    versions.next = 9;
    asked.length = 0;
    opened[0]?.source.dispatchEvent(new Event("error"));
    await pass(1_000);
    expect(opened).toHaveLength(2);
    // The page was answered at 9 already: the thread alone is read again, and caught up from the
    // newest message it showed.
    expect(urls(asked)).toEqual([BOARD_PATH, THREAD_URL, `${THREAD_URL}&after=5`]);
    expect(shownIds(task)).toEqual([4, 5, 6, 7, 9, 10]);
    const source = opened[1]?.source;
    source?.dispatchEvent(new Event("open"));
    say(source, FEED_EVENT, said(10, ["message", anEntry(11).message]));
    await pass();
    expect(shownIds(task)).toEqual([4, 5, 6, 7, 9, 10, 11]);
    page.abort();
    await done;
  });

  it("reads a part once at a time: the feed opened again joins the read that is out", async () => {
    const first = later<Response>();
    let reads = 0;
    const { task, opened, page, done, answerBoard, store, versions } = begun((url) => {
      if (url !== PAGE_URL) return answer(200, history([]), '"t"', 9);
      reads += 1;
      return reads === 1
        ? first.promise
        : answer(200, aPage(aTask("w/a", { title: "New" })), '"p2"', 9);
    });
    answerBoard();
    await pass();
    expect(reads).toBe(1);
    // The feed is refused while the first read is out: no second read beside it.
    versions.next = 9;
    opened[0]?.source.dispatchEvent(new Event("error"));
    await pass(1_000);
    expect(opened).toHaveLength(2);
    expect(reads).toBe(1);
    // Its answer was read before the version the feed opened from: drawn, and read once more.
    first.settle(answer(200, aPage(aTask("w/a", { title: "Old" })), '"p1"', 7));
    await pass();
    expect(reads).toBe(2);
    expect(task.page()?.task.title).toBe("New");
    expect(
      keptBy(store)
        .filter(([, part]) => part === PAGE)
        .map(([, , etag]) => etag),
    ).toEqual(['"p1"', '"p2"']);
    page.abort();
    await done;
  });
});
