import { describe, expect, it } from "vitest";
import {
  arrive,
  codePointOrder,
  concernsThread,
  dismiss,
  expire,
  isRead,
  NEW_DRAFT,
  NO_TOASTS,
  pendingTargets,
  readActivity,
  readAskReply,
  readDraft,
  readIds,
  readPending,
  sameDraft,
  saveOutcome,
  TOASTS,
  type Toasts,
  titleLength,
  trimmed,
} from "./panel-model";
import { Failure } from "./request";
import type { ActivityItem, Pending, PendingItem } from "./wire";

function activity(id: string, fields: Partial<ActivityItem> = {}): ActivityItem {
  return {
    id,
    kind: "question",
    path: "docs/PROBE.md",
    slug: `Unread ${id}`,
    ts: "2026-11-02T06:00:00Z",
    text: "A real activity notification",
    href: "/docs/PROBE.md",
    answered: false,
    ...fields,
  };
}

const up = (toasts: Toasts) => toasts.shown.map(([, item]) => item.id);

describe("the activity toasts", () => {
  it("leave a later batch up when an earlier one expires", () => {
    const first = arrive(NO_TOASTS, [activity("a")]);
    const second = arrive(first, [activity("b"), activity("c")]);
    expect(first.batches).not.toBe(second.batches);
    const expired = expire(second, first.batches);
    expect(up(expired)).toEqual(["b", "c"]);
    expect(up(expire(expired, first.batches))).toEqual(["b", "c"]);
    expect(up(expire(expired, second.batches))).toEqual([]);
  });

  it("take down no later toast when a dismissed batch expires", () => {
    const first = arrive(NO_TOASTS, [activity("a"), activity("b")]);
    const later = arrive(dismiss(dismiss(first, "a"), "b"), [activity("c"), activity("d")]);
    expect(up(expire(later, first.batches))).toEqual(["c", "d"]);
  });

  it("keep at most three up, the newest", () => {
    const first = arrive(NO_TOASTS, [activity("a"), activity("b")]);
    const second = arrive(first, [activity("c"), activity("d")]);
    expect(up(second)).toEqual(["b", "c", "d"]);
    const third = arrive(
      second,
      [0, 1, 2, 3].map((n) => activity(`e${n}`)),
    );
    expect(up(third)).toEqual(["e1", "e2", "e3"]);
    expect(third.shown).toHaveLength(TOASTS);
    expect(up(expire(third, second.batches))).toEqual(["e1", "e2", "e3"]);
  });

  it("take down the dismissed toast alone", () => {
    const shown = dismiss(arrive(NO_TOASTS, [activity("a"), activity("b")]), "a");
    expect(up(shown)).toEqual(["b"]);
    expect(up(dismiss(shown, "missing"))).toEqual(["b"]);
  });
});

describe("the asks' news", () => {
  it("concerns the conversation whose thread moved, and every one after a reconnect", () => {
    const moved = {
      threads: [
        { id: 7, path: "docs/A.md" },
        { id: 9, path: "docs/B.md" },
      ],
    };
    expect(concernsThread(moved, 7)).toBe(true);
    expect(concernsThread(moved, 8)).toBe(false);
    expect(concernsThread({}, 7)).toBe(false);
    expect(concernsThread(true, 8)).toBe(true);
    expect(concernsThread({ recipient: null }, 8)).toBe(true);
  });

  it("names a thread by its id and its path, each of its type", () => {
    expect(concernsThread({ threads: [{ id: 7 }] }, 7)).toBe(false);
    expect(concernsThread({ threads: [{ id: "7", path: "a" }] }, 7)).toBe(false);
    expect(concernsThread({ threads: [{ id: 7.9, path: "a" }] }, 7)).toBe(true);
    expect(concernsThread({ threads: "7" }, 7)).toBe(false);
    for (const nothing of [false, null, undefined, 7, "moved"]) {
      expect(concernsThread(nothing, 7)).toBe(false);
    }
  });
});

describe("a task save's reply", () => {
  it("clears a draft only for a confirmed task", () => {
    expect(saveOutcome({ slug: "inbox/task" })).toEqual({ saved: "inbox/task" });
    expect(saveOutcome({ pending: true, error: "still checking", slug: "inbox/task" })).toEqual({
      checking: "still checking",
    });
    expect(saveOutcome({ pending: true })).toEqual({
      checking: "Checking whether the task was saved…",
    });
    expect(saveOutcome({ pending: false, slug: "a", error: null })).toEqual({ saved: "a" });
    for (const reply of [{}, { slug: "" }, { slug: "  " }, { slug: " " }, { error: "x" }]) {
      expect(() => saveOutcome(reply)).toThrow("The server did not confirm a saved task");
    }
  });

  it("is an unknown outcome when it cannot be read", () => {
    for (const reply of [null, "saved", [], { slug: 7 }, { pending: "yes" }, { error: 1 }]) {
      let thrown: unknown;
      try {
        saveOutcome(reply);
      } catch (failure) {
        thrown = failure;
      }
      expect(thrown).toBeInstanceOf(Failure);
      expect((thrown as Failure).kind).toBe("uncertain");
    }
  });
});

describe("a task's title", () => {
  it("is counted by its characters once trimmed as Rust trims", () => {
    expect(titleLength("  ab  ")).toBe(2);
    expect(titleLength("\u0085é😀　")).toBe(2);
    expect(trimmed("﻿a ")).toBe("﻿a");
    expect(trimmed(" \n a b \t")).toBe("a b");
  });
});

describe("the draft", () => {
  it("takes each field it holds of its type, the rest at their defaults", () => {
    expect(readDraft({ open: true, title: "T", ask: 4 })).toEqual({
      ...NEW_DRAFT,
      priority: "",
      open: true,
      title: "T",
      ask: 4,
    });
    expect(readDraft({ ...NEW_DRAFT, extra: 1 })).toEqual(NEW_DRAFT);
  });

  it("is none when a field is of another type", () => {
    for (const value of [
      null,
      "draft",
      [],
      { open: "yes" },
      { ask: 1.5 },
      { ask: "1" },
      { priority: 2 },
      { request: null },
    ]) {
      expect(readDraft(value)).toBeNull();
    }
  });

  it("is the same as another field for field", () => {
    expect(sameDraft(NEW_DRAFT, { ...NEW_DRAFT })).toBe(true);
    expect(sameDraft(NEW_DRAFT, { ...NEW_DRAFT, open: true })).toBe(false);
    expect(sameDraft(NEW_DRAFT, { ...NEW_DRAFT, ask: 1 })).toBe(false);
  });
});

describe("the frame's reads, as the panel takes them", () => {
  it("are the activity's items, each with every field of its type", () => {
    expect(readActivity({ items: [activity("a")] })).toEqual([activity("a")]);
    expect(readActivity({ items: [] })).toEqual([]);
    expect(readActivity({ items: [{ ...activity("a"), answered: "no" }] })).toBeNull();
    expect(readActivity({ items: [{ id: "a" }] })).toBeNull();
    expect(readActivity(undefined)).toBeNull();
  });

  it("are the pending cards and their documents' order", () => {
    const pending = {
      items: [{ path: "docs/A.md", slug: "one", href: "/docs/A.md#one", branch: null }],
      docs: ["docs/A.md"],
    };
    expect(readPending(pending)).toEqual(pending);
    expect(readPending({ items: [], docs: [] })).toEqual({ items: [], docs: [] });
    expect(readPending({ items: [{ path: "a", slug: "b", href: "c", branch: 1 }], docs: [] })).toBe(
      null,
    );
    expect(readPending({ items: [] })).toBeNull();
  });

  it("are ids each once, in Rust's order", () => {
    expect(readIds(["b", "a", "b", "é", "z"])).toEqual(["a", "b", "z", "é"]);
    expect(readIds([])).toEqual([]);
    expect(readIds(["a", 1])).toBeNull();
    expect(readIds("a")).toBeNull();
    expect(readIds(null)).toBeNull();
  });

  it("order text by code point, past the units JavaScript compares", () => {
    expect(codePointOrder("￿", "😀")).toBeLessThan(0);
    expect("￿" < "😀").toBe(false);
    expect(codePointOrder("ab", "a")).toBeGreaterThan(0);
    expect(codePointOrder("a", "a")).toBe(0);
  });
});

describe("an activity item", () => {
  it("is read once answered or seen on this device", () => {
    expect(isRead(activity("a"), null)).toBe(false);
    expect(isRead(activity("a"), ["b"])).toBe(false);
    expect(isRead(activity("a"), ["a"])).toBe(true);
    expect(isRead(activity("a", { answered: true }), null)).toBe(true);
  });
});

describe("the chat's conversation", () => {
  const ask = {
    id: 4,
    anchor: "",
    quote: "",
    status: "open",
    messages: [{ seq: 1, by: "owner", blocks: [] }],
  };

  it("is the ask and whom chat goes to", () => {
    expect(readAskReply({ ask, recipient: "agent" })).toEqual({ ask, recipient: "agent" });
    expect(readAskReply({ ask })).toEqual({ ask, recipient: null });
    expect(readAskReply({ ask, recipient: null })).toEqual({ ask, recipient: null });
  });

  it("is none for an answer the chat cannot draw", () => {
    expect(readAskReply({ ask, recipient: 7 })).toBeNull();
    expect(readAskReply({ ask: { ...ask, messages: "none" } })).toBeNull();
    expect(readAskReply({})).toBeNull();
    expect(readAskReply(null)).toBeNull();
  });
});

describe("the pending bar's steps", () => {
  const card = (path: string, slug: string): PendingItem => ({
    path,
    slug,
    href: `/${path}#${slug}`,
  });
  const pending: Pending = {
    items: [
      card("docs/A.md", "a1"),
      card("docs/B.md", "b1"),
      card("docs/B.md", "b2"),
      card("docs/B.md", "b3"),
      card("docs/C.md", "c1"),
    ],
    docs: ["docs/A.md", "docs/B.md", "docs/C.md"],
  };
  const tops =
    (places: Record<string, number>) =>
    (slug: string): number | null =>
      places[slug] ?? null;

  it("go to the cards above and below the reader's place", () => {
    const [prev, next] = pendingTargets(
      "docs/B.md",
      pending,
      tops({ b1: -300, b2: 60, b3: 500 }),
      "",
      800,
    );
    expect(prev?.slug).toBe("b1");
    expect(next?.slug).toBe("b3");
  });

  it("go from the card the address names while it is on the screen", () => {
    const [prev, next] = pendingTargets(
      "docs/B.md",
      pending,
      tops({ b1: -300, b2: 60, b3: 500 }),
      "b2",
      800,
    );
    expect([prev?.slug, next?.slug]).toEqual(["b1", "b3"]);
    const first = pendingTargets("docs/B.md", pending, tops({ b1: 20 }), "b1", 800);
    expect([first[0]?.slug, first[1]?.slug]).toEqual(["a1", "b2"]);
    const last = pendingTargets("docs/B.md", pending, tops({ b3: 20 }), "b3", 800);
    expect([last[0]?.slug, last[1]?.slug]).toEqual(["b2", "c1"]);
  });

  it("forget the address once its card is off the screen", () => {
    const [prev, next] = pendingTargets(
      "docs/B.md",
      pending,
      tops({ b1: -900, b2: -400, b3: 900 }),
      "b2",
      800,
    );
    expect([prev?.slug, next?.slug]).toEqual(["b2", "b3"]);
  });

  it("go past this page's cards to the documents before and after it", () => {
    const [prev, next] = pendingTargets("docs/B.md", pending, tops({}), "", 800);
    expect([prev?.slug, next?.slug]).toEqual(["a1", "c1"]);
    const home = pendingTargets("", pending, tops({}), "", 800);
    expect([home[0], home[1]?.slug]).toEqual([null, "a1"]);
    const end = pendingTargets("docs/C.md", pending, tops({}), "", 800);
    expect([end[0]?.slug, end[1]]).toEqual(["b3", null]);
  });

  it("put a card of a document the list does not order before every other", () => {
    const loose: Pending = {
      items: [card("docs/X.md", "x1"), ...pending.items],
      docs: pending.docs,
    };
    const [prev] = pendingTargets("docs/A.md", loose, tops({}), "", 800);
    expect(prev?.slug).toBe("x1");
    const [none, next] = pendingTargets("docs/X.md", loose, tops({ x1: 200 }), "", 800);
    expect([none, next?.slug]).toEqual([null, "x1"]);
  });
});
