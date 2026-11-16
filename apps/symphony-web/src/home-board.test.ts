import { afterEach, describe, expect, it, vi } from "vitest";
import fullBoard from "../../../crates/symphony-api/fixtures/board.json";
import emptyBoard from "../../../crates/symphony-api/fixtures/board-empty.json";
import deltaCard from "../../../crates/symphony-api/fixtures/delta-card.json";
import deltaRegions from "../../../crates/symphony-api/fixtures/delta-regions.json";
import { apply, heard, notify, readBoard, readDelta } from "./home-board";
import { CASES } from "./testing";
import type { Board, Card, Delta } from "./wire";

/** A fixture crates/symphony-api pins, as this client reads it. */
function board(value: unknown = fullBoard): Board {
  const read = readBoard(structuredClone(value));
  if (read === null) throw new Error("the fixture is no board");
  return read;
}

function delta(value: unknown): Delta {
  const read = readDelta(structuredClone(value));
  if (read === null) throw new Error("the fixture is no event");
  return read;
}

/** `value` with `path`'s field set to `to` (removed for undefined). */
function changed(value: unknown, path: (string | number)[], to: unknown): unknown {
  const copy = structuredClone(value) as Record<string | number, unknown>;
  let at: Record<string | number, unknown> = copy;
  for (const key of path.slice(0, -1)) at = at[key] as Record<string | number, unknown>;
  const last = path.at(-1) as string | number;
  if (to === undefined) delete at[last];
  else at[last] = to;
  return copy;
}

describe("the board home reads", () => {
  it("is the server's, the fixtures and the wire sample alike", () => {
    expect(board().next).toHaveLength(5);
    expect(board(emptyBoard).next).toEqual([]);
    expect(readBoard(CASES.samples.board)?.next[0]?.task).toBe("app/lamps");
  });

  it("takes the defaults Rust gives a field it may leave out", () => {
    const bare = changed(
      changed(fullBoard, ["counts", "tasks"], undefined),
      ["progress", "regions"],
      undefined,
    );
    expect(readBoard(bare)?.counts.tasks).toBeNull();
    expect(readBoard(bare)?.progress.regions).toEqual([]);
    const plain = { slug: "s", doc: "d", doc_title: "D", title: "T", why: "w" };
    expect(readBoard(changed(fullBoard, ["next"], [plain]))?.next).toEqual([plain]);
  });

  it.each([
    ["nothing", null],
    ["a version of another type", changed(fullBoard, ["version"], "4182")],
    ["a negative version", changed(fullBoard, ["version"], -1)],
    ["a fractional version", changed(fullBoard, ["version"], 1.5)],
    ["no build", changed(fullBoard, ["build"], undefined)],
    ["a ping past a u32", changed(fullBoard, ["ping_ms"], 2 ** 32)],
    ["no cards", changed(fullBoard, ["next"], undefined)],
    ["a card without a title", changed(fullBoard, ["next", 0, "title"], undefined)],
    ["a card's summary of null", changed(fullBoard, ["next", 0, "summary"], null)],
    ["a card's postponed of null", changed(fullBoard, ["next", 0, "postponed"], null)],
    ["a card's pictures of null", changed(fullBoard, ["next", 0, "pictures"], null)],
    ["a card's task of another type", changed(fullBoard, ["next", 0, "task"], 7)],
    [
      "a picture without a caption",
      changed(fullBoard, ["next", 0, "pictures", 0, "caption"], undefined),
    ],
    ["a picture's src of another type", changed(fullBoard, ["next", 0, "pictures", 0, "src"], 1)],
    ["a picture waiting of null", changed(fullBoard, ["next", 0, "pictures", 0, "waiting"], null)],
    ["no counts", changed(fullBoard, ["counts"], undefined)],
    ["a negative count", changed(fullBoard, ["counts", "waiting"], -1)],
    ["task counts without one", changed(fullBoard, ["counts", "tasks", "agents_live"], undefined)],
    ["no progress", changed(fullBoard, ["progress"], undefined)],
    ["regions of null", changed(fullBoard, ["progress", "regions"], null)],
    ["a region without bars", changed(fullBoard, ["progress", "regions", 0, "bars"], undefined)],
    ["a legend of two words", changed(fullBoard, ["progress", "regions", 0, "legend"], ["a", "b"])],
    [
      "a legend of four words",
      changed(fullBoard, ["progress", "regions", 0, "legend"], ["a", "b", "c", "d"]),
    ],
    [
      "a bar without a page",
      changed(fullBoard, ["progress", "regions", 0, "bars", 0, "page"], undefined),
    ],
    [
      "a bar's total of another type",
      changed(fullBoard, ["progress", "regions", 0, "bars", 0, "total"], "31"),
    ],
    ["no decisions waiting", changed(fullBoard, ["progress", "decisions_waiting"], undefined)],
  ])("is none for %s", (_, value) => {
    expect(readBoard(value)).toBeNull();
  });
});

describe("a feed event home reads", () => {
  it("is each change of a region it knows, one it does not left out", () => {
    expect(delta(deltaCard).changes.map((change) => change.region)).toEqual(["card"]);
    expect(delta(deltaRegions).changes.map((change) => change.region)).toEqual([
      "queue",
      "counts",
      "progress",
      "client",
    ]);
    const other = {
      version: 9,
      changes: [
        { region: "asks", data: { threads: [] } },
        { region: "task-asks", data: { questions: { owner: 1 }, verdicts: {} } },
        { region: "tasks" },
      ],
    };
    expect(readDelta(other)).toEqual({ version: 9, changes: [] });
  });

  it("gives counts and progress the defaults Rust gives them", () => {
    const counts = { waiting: 1, docs_waiting: 1, spot_check: 0, answered_today: 0 };
    const read = readDelta({
      version: 2,
      changes: [
        { region: "counts", data: counts },
        { region: "progress", data: { docs: 1, decisions_waiting: 0 } },
      ],
    });
    expect(read?.changes).toEqual([
      { region: "counts", data: { ...counts, tasks: null } },
      { region: "progress", data: { docs: 1, decisions_waiting: 0, regions: [] } },
    ]);
  });

  it.each([
    ["nothing", null],
    ["no version", { changes: [] }],
    ["no changes", { version: 2 }],
    ["changes of another type", { version: 2, changes: {} }],
    ["a change without a region", { version: 2, changes: [{ data: {} }] }],
    ["a region of another type", { version: 2, changes: [{ region: 1, data: {} }] }],
    ["a change of another type", { version: 2, changes: ["card"] }],
    ["data before its region", { version: 2, changes: [{ data: {}, region: "asks" }] }],
    ["a known region without data", { version: 2, changes: [{ region: "counts" }] }],
    ["a card it cannot read", { version: 2, changes: [{ region: "card", data: { slug: "s" } }] }],
    ["a queue of another type", { version: 2, changes: [{ region: "queue", data: {} }] }],
    [
      "counts it cannot read",
      { version: 2, changes: [{ region: "counts", data: { waiting: 1 } }] },
    ],
    [
      "progress it cannot read",
      { version: 2, changes: [{ region: "progress", data: { docs: -1 } }] },
    ],
    ["a client without a build", { version: 2, changes: [{ region: "client", data: {} }] }],
  ])("is none for %s", (_, value) => {
    expect(readDelta(value)).toBeNull();
  });
});

describe("a feed event applied to the board", () => {
  it("changes the card it names and nothing else, the rest the same objects", () => {
    const before = board();
    const { board: after, moved, notice } = apply(before, delta(deltaCard));
    expect(moved).toEqual([3]);
    expect(after.next[3]?.title).toBe(
      "The dock menu lists repair before trade, and refuel before both.",
    );
    expect(after.version).toBe(4183);
    for (const at of [0, 1, 2, 4]) expect(after.next[at]).toBe(before.next[at]);
    expect(after.counts).toBe(before.counts);
    expect(after.progress).toBe(before.progress);
    expect(after.build).toBe(before.build);
    expect(notice).toBeNull();
    expect(before.version).toBe(4182);
  });

  it("moves nothing but the version for a card not on the board", () => {
    const before = board();
    const card = { ...(before.next[0] as Card), slug: "somewhere-past-the-fifth" };
    const { board: after, moved } = apply(before, {
      version: 4183,
      changes: [{ region: "card", data: card }],
    });
    expect(moved).toEqual([]);
    expect(after.next).toEqual(before.next);
    expect(after.version).toBe(4183);
  });

  it("replaces a branch's card, never main's of the same slug", () => {
    const before = board();
    const main = before.next[0] as Card;
    const branch = { ...main, doc: `@design~policy/${main.doc}`, task: "design/policy" };
    before.next.splice(1, 0, branch);
    const revised = { ...branch, title: "Revised branch proposal" };
    const { board: after, moved } = apply(before, {
      version: 4190,
      changes: [{ region: "card", data: revised }],
    });
    expect(moved).toEqual([1]);
    expect(after.next[0]).toBe(main);
    expect(after.next[1]).toEqual(revised);
  });

  it("moves the queue, the counts, the progress and the build, each its region", () => {
    const { board: after, moved, notice } = apply(board(), delta(deltaRegions));
    expect(moved).toEqual(["queue", "counts", "progress"]);
    expect(after.next.map((card) => card.slug)).toEqual(["mining-yield-curve"]);
    expect(after.counts.waiting).toBe(36);
    expect(after.progress.regions[0]?.bars[0]?.done).toBe(22);
    expect(after.build).toBe("/assets/symphony-shell-3f9a1c0123456789.js");
    expect(notice).toBeNull();
  });

  it("keeps the queue's first five cards", () => {
    const cards = Array.from({ length: 7 }, (_, at) => ({
      ...(board().next[0] as Card),
      slug: `c${at}`,
    }));
    const { board: after } = apply(board(), {
      version: 4190,
      changes: [{ region: "queue", data: cards }],
    });
    expect(after.next.map((card) => card.slug)).toEqual(["c0", "c1", "c2", "c3", "c4"]);
  });

  it("changes nothing at or before the board's version: the feed replays from it", () => {
    const before = board();
    const stale = { ...delta(deltaCard), version: before.version };
    const outcome = apply(before, stale);
    expect(outcome).toEqual({ board: before, moved: [], notice: null });
    expect(outcome.board).toBe(before);
  });

  it("raises one notice naming the next card when more cards wait", () => {
    const before = board();
    const first = { ...(before.next[1] as Card), slug: "new-card", title: "A new card waits." };
    const queue = [first, ...before.next].slice(0, 5);
    const counts = { ...before.counts, waiting: before.counts.waiting + 2 };
    const { notice } = apply(before, {
      version: 4190,
      changes: [
        { region: "queue", data: queue },
        { region: "counts", data: counts },
      ],
    });
    expect(notice).toEqual({ title: "39 cards wait on you", body: "A new card waits." });
  });

  it("says card for the first one waiting, reading the queue after the whole event", () => {
    const before = board(emptyBoard);
    const card = board().next[1] as Card;
    const counts = { ...before.counts, waiting: 1, docs_waiting: 1 };
    const { notice } = apply(before, {
      version: 2,
      changes: [
        { region: "counts", data: counts },
        { region: "queue", data: [card] },
      ],
    });
    expect(notice).toEqual({ title: "1 card waits on you", body: card.title });
  });

  it("names no card when none is on the board", () => {
    const before = board(emptyBoard);
    const counts = { ...before.counts, waiting: 1 };
    const { notice } = apply(before, { version: 2, changes: [{ region: "counts", data: counts }] });
    expect(notice).toEqual({ title: "1 card waits on you", body: "" });
  });
});

describe("the owner told more cards wait", () => {
  afterEach(() => vi.unstubAllGlobals());

  /** The page's Notification, granted or not, and its tab hidden or not: what it showed. */
  const page = (permission: string, hidden: boolean) => {
    const shown: unknown[] = [];
    vi.stubGlobal(
      "Notification",
      Object.assign(
        class {
          constructor(title: string, options: unknown) {
            shown.push([title, options]);
          }
        },
        { permission },
      ),
    );
    vi.stubGlobal("document", { hidden });
    return shown;
  };

  it("is a notification once they allowed them, while the tab is hidden, one tag for all", () => {
    const shown = page("granted", true);
    notify({ title: "2 cards wait on you", body: "Next" });
    expect(shown).toEqual([["2 cards wait on you", { body: "Next", tag: "review-waiting" }]]);
  });

  it.each([
    ["not allowed", "default", true],
    ["refused", "denied", true],
    ["in front of them", "granted", false],
  ])("is nothing %s", (_, permission, hidden) => {
    const shown = page(permission as string, hidden as boolean);
    notify({ title: "t", body: "b" });
    expect(shown).toEqual([]);
  });

  it("is nothing where the browser has no notifications", () => {
    vi.stubGlobal("Notification", undefined);
    expect(() => notify({ title: "t", body: "b" })).not.toThrow();
  });

  it("is nothing where the browser refuses to make one, and the event is applied all the same", () => {
    // Chrome on Android: permission granted, but the constructor throws (a service worker shows one).
    let tried = 0;
    vi.stubGlobal(
      "Notification",
      Object.assign(
        class {
          constructor() {
            tried += 1;
            throw new TypeError("Illegal constructor");
          }
        },
        { permission: "granted" },
      ),
    );
    vi.stubGlobal("document", { hidden: true });
    expect(() => notify({ title: "t", body: "b" })).not.toThrow();
    const before = board(emptyBoard);
    const counts = { ...before.counts, waiting: 3 };
    const after = heard(before, { version: 2, changes: [{ region: "counts", data: counts }] });
    expect(after?.version).toBe(2);
    expect(after?.counts.waiting).toBe(3);
    expect(tried).toBe(2);
  });

  it("follows each event the board hears, which is none for one it cannot read", () => {
    const shown = page("granted", true);
    const before = board(emptyBoard);
    const counts = { ...before.counts, waiting: 3 };
    const after = heard(before, { version: 2, changes: [{ region: "counts", data: counts }] });
    expect(after?.counts.waiting).toBe(3);
    expect(shown).toEqual([["3 cards wait on you", { body: "", tag: "review-waiting" }]]);
    expect(heard(before, { version: "2" })).toBeNull();
    expect(heard(before, { version: 3, changes: [] })?.version).toBe(3);
    expect(shown).toHaveLength(1);
  });
});
