import { afterEach, describe, expect, it, vi } from "vitest";
import {
  answered,
  BOARD,
  boardItem,
  COUNTS,
  column,
  columns,
  drawn,
  FRESH,
  failed,
  gone,
  keepLater,
  keptView,
  lost,
  opened,
  QUEUE,
  readKept,
  shown,
  visitLater,
  WORK,
} from "./cache";
import { CACHE_FORMAT } from "./wire";

describe("a board's item", () => {
  it("is the main checkout's, or a checkout's by its scope", () => {
    expect(boardItem("")).toEqual({ kind: "board", id: "board:", pinned: false });
    expect(boardItem("proposal~x").id).toBe("board:@proposal~x/");
    expect(BOARD).not.toBe(QUEUE);
  });
});

describe("the task board's kept reads", () => {
  it("are the counts, the machines' work and each column's first page", () => {
    expect(new Set([BOARD, QUEUE, COUNTS, WORK, column("ready")]).size).toBe(5);
    expect(column("in_progress")).toBe("column:in_progress");
  });

  it("are the default view's alone", () => {
    expect(keptView("", 1, 1)).toBe(true);
    expect(keptView("lamp", 1, 1)).toBe(false);
    expect(keptView("", 7, 1)).toBe(false);
    expect(keptView("", 1, 0)).toBe(false);
  });

  it("are a column for each status, the machines' two and the owner's sign-off, once each", () => {
    expect(columns(["ready", "build", "review"], [["done", 1]])).toEqual([
      "ready",
      "build",
      "review",
      "test",
      "signoff",
    ]);
    expect(columns([], [["done", 1]])).toEqual(["done", "build", "test", "signoff"]);
  });
});

describe("what a page shows from this device", () => {
  it("is nothing until a part is drawn from it", () => {
    expect(shown(FRESH)).toBeNull();
  });

  it("says the oldest copy drawn until the feed is open and every part answered", () => {
    let f = drawn(FRESH, BOARD, 20);
    f = drawn(f, QUEUE, 10);
    f = drawn(f, QUEUE, 30);
    expect(f.unanswered).toEqual([BOARD, QUEUE]);
    expect(shown(f)).toEqual([10, false]);
    f = answered(f, BOARD);
    f = opened(f);
    expect(shown(f)).toEqual([10, false]);
    f = answered(f, QUEUE);
    expect(shown(f)).toBeNull();
  });

  it("is settled by the feed opening after every part answered", () => {
    const f = answered(drawn(FRESH, BOARD, 5), BOARD);
    expect(shown(f)).toEqual([5, false]);
    expect(shown(opened(f))).toBeNull();
  });

  it("says the server is not reachable while a read fails and the feed is closed", () => {
    let f = failed(drawn(FRESH, QUEUE, 9), QUEUE);
    f = failed(f, QUEUE);
    expect(f.failing).toEqual([QUEUE]);
    expect(shown(f)).toEqual([9, true]);
    f = opened(f);
    expect(shown(f)).toEqual([9, false]);
    f = lost(f);
    expect(shown(f)).toEqual([9, true]);
    expect(lost(f)).toBe(f);
    expect(shown(answered(opened(f), QUEUE))).toBeNull();
  });

  it("drops a part the page no longer shows, answered or not", () => {
    let f = failed(drawn(drawn(FRESH, COUNTS, 4), column("ready"), 2), column("ready"));
    f = opened(gone(f, column("ready")));
    expect(f.unanswered).toEqual([COUNTS]);
    expect(f.failing).toEqual([]);
    expect(shown(gone(f, COUNTS))).toBeNull();
  });

  it("changes by new values, the old one as it was", () => {
    const before = drawn(FRESH, BOARD, 1);
    answered(before, BOARD);
    expect(before.unanswered).toEqual([BOARD]);
    expect(FRESH.unanswered).toEqual([]);
  });
});

describe("the shell's store", () => {
  afterEach(() => vi.unstubAllGlobals());

  const fake = (kept: unknown) => ({
    read: vi.fn(async () => kept),
    keep: vi.fn(async () => undefined),
    visit: vi.fn(async () => undefined),
    forget: vi.fn(async () => undefined),
  });

  it("gives back what it kept, read as the page reads it", async () => {
    const store = fake({ value: { n: 1 }, etag: '"e"', saved: 7 });
    vi.stubGlobal("symphonyCache", store);
    const kept = await readKept(boardItem(""), QUEUE, (value) => value as { n: number });
    expect(kept).toEqual({ value: { n: 1 }, etag: '"e"', saved: 7 });
    expect(store.read).toHaveBeenCalledWith(CACHE_FORMAT, "", "board:", QUEUE);
  });

  it("forgets an item whose kept value this client cannot read", async () => {
    const store = fake({ value: "old", etag: "", saved: 1 });
    vi.stubGlobal("symphonyCache", store);
    expect(await readKept(boardItem(""), QUEUE, () => null)).toBeNull();
    expect(store.forget).toHaveBeenCalledWith(CACHE_FORMAT, "", "board:");
  });

  it.each([
    ["nothing kept", null],
    ["no tag", { value: 1, saved: 1 }],
    ["no time", { value: 1, etag: "" }],
  ])("misses with %s", async (_, value) => {
    vi.stubGlobal("symphonyCache", fake(value));
    expect(await readKept(boardItem(""), QUEUE, (read) => read)).toBeNull();
  });

  it("misses when it fails, and when there is none", async () => {
    vi.stubGlobal("symphonyCache", {
      ...fake(null),
      read: async () => Promise.reject(new Error("x")),
    });
    expect(await readKept(boardItem(""), QUEUE, (read) => read)).toBeNull();
    vi.stubGlobal("symphonyCache", undefined);
    expect(await readKept(boardItem(""), QUEUE, (read) => read)).toBeNull();
    keepLater(boardItem(""), QUEUE, "", 1);
    visitLater(boardItem(""));
  });

  it("keeps a read and a visit, the page not waiting", () => {
    const store = fake(null);
    vi.stubGlobal("symphonyCache", store);
    keepLater(boardItem("s"), QUEUE, '"e"', { n: 1 });
    visitLater(boardItem("s"));
    expect(store.keep).toHaveBeenCalledWith(
      CACHE_FORMAT,
      "",
      "board:@s/",
      "board",
      QUEUE,
      '"e"',
      { n: 1 },
      undefined,
      false,
    );
    expect(store.visit).toHaveBeenCalledWith(CACHE_FORMAT, "", "board:@s/", "board", false);
  });
});
