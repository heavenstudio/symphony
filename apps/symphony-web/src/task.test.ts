import { describe, expect, it } from "vitest";
import {
  byPriority,
  byRecent,
  holderHealth,
  isTask,
  merge,
  readItemChange,
  readListing,
  readSlugs,
  readTask,
  readTaskChange,
  slugOrder,
} from "./task";
import { aRole, aTask, CASES } from "./testing";
import type { Task } from "./wire";

describe("the order slugs are listed in", () => {
  it("is the Rust's for every pair the cases hold", () => {
    expect(CASES.slugOrder.length).toBeGreaterThan(100);
    for (const [a, b, order] of CASES.slugOrder) expect(slugOrder(a, b), `${a} ${b}`).toBe(order);
  });

  it("reads a run of digits by its number, its zeros spelled apart", () => {
    const sorted = ["ready-30", "ready-3", "ready-10", "ready-9", "ready-03"].sort(slugOrder);
    expect(sorted).toEqual(["ready-03", "ready-3", "ready-9", "ready-10", "ready-30"]);
    expect(slugOrder("a", "a")).toBe(0);
    expect(slugOrder("a", "ab")).toBe(-1);
    expect(slugOrder("ab", "a")).toBe(1);
    expect(slugOrder("é", "z")).toBe(1);
  });
});

describe("tasks in a column", () => {
  const tasks = [
    aTask("b", { priority: 2, updated: 10 }),
    aTask("c", { priority: 1, updated: 30 }),
    aTask("a", { priority: 2, updated: 30 }),
    aTask("d", { priority: 1, updated: 10 }),
  ];
  const slugs = (order: (a: Task, b: Task) => number, of: Task[] = tasks) =>
    [...of].sort(order).map((task) => task.slug);

  it("sort open work by priority then slug, finished work newest first then slug", () => {
    expect(slugs(byPriority)).toEqual(["c", "d", "a", "b"]);
    expect(slugs(byRecent)).toEqual(["a", "c", "b", "d"]);
  });

  it("order a slug's numbers by their value, as the server pages a column", () => {
    const numbered = [
      aTask("ready-30", { priority: 3, updated: 10 }),
      aTask("ready-3", { priority: 3, updated: 10 }),
      aTask("ready-10", { priority: 3, updated: 10 }),
      aTask("ready-9", { priority: 3, updated: 10 }),
      aTask("urgent-40", { priority: 1, updated: 10 }),
    ];
    expect(slugs(byPriority, numbered)).toEqual([
      "urgent-40",
      "ready-3",
      "ready-9",
      "ready-10",
      "ready-30",
    ]);
    expect(slugs(byRecent, numbered)).toEqual([
      "ready-3",
      "ready-9",
      "ready-10",
      "ready-30",
      "urgent-40",
    ]);
  });
});

describe("a task as the client reads it", () => {
  it("is the server's, its parent none when it has none", () => {
    const task = aTask("w/a");
    expect(readTask(task)).toEqual(task);
    const { parent: _, ...orphan } = task;
    expect(readTask(orphan)).toEqual(task);
    expect(readTask({ ...task, parent: "w" })?.parent).toBe("w");
  });

  it.each([
    ["nothing", null],
    ["no slug", { ...aTask("w/a"), slug: undefined }],
    ["a priority that is text", { ...aTask("w/a"), priority: "1" }],
    ["no time it was updated", { ...aTask("w/a"), updated: undefined }],
    ["a parent that is no text", { ...aTask("w/a"), parent: 3 }],
    ["links that are no texts", { ...aTask("w/a"), links: [1] }],
    ["a role without its health", { ...aTask("w/a"), roles: [{ handle: "a", role: "executor" }] }],
    ["roles that are no list", { ...aTask("w/a"), roles: "executor" }],
  ])("is none for %s", (_, value) => {
    expect(readTask(value)).toBeNull();
  });

  it("is a task, not a slice of one, while it has no parent", () => {
    expect(isTask(aTask("w/a"))).toBe(true);
    expect(isTask({ parent: undefined } as unknown as Task)).toBe(true);
    expect(isTask(aTask("w/a/1", { parent: "w/a" }))).toBe(false);
  });
});

describe("a listing as the client reads it", () => {
  it("is the server's", () => {
    const listing = readListing(CASES.samples.listing);
    expect(listing).toEqual(CASES.samples.listing);
    expect(listing?.next).toBe("w/b");
    expect(readListing(CASES.samples.emptyListing)).toEqual(CASES.samples.emptyListing);
  });

  it("takes what serde defaults as none", () => {
    expect(readListing({ tasks: [aTask("w/a")] })).toEqual({
      tasks: [aTask("w/a")],
      statuses: [],
      signoff: [],
      counts: [],
      lifecycle_counts: [],
      next: null,
    });
  });

  it.each([
    ["nothing", null],
    ["no tasks", { statuses: [] }],
    ["a task it cannot read", { tasks: [{ slug: "w/a" }] }],
    ["statuses that are no texts", { tasks: [], statuses: [1] }],
    ["a sign-off that is no list", { tasks: [], signoff: "w/a" }],
    ["counts that are no pairs", { tasks: [], counts: [["ready"]] }],
    ["a count that is text", { tasks: [], lifecycle_counts: [["ready", "3"]] }],
    ["a cursor that is no text", { tasks: [], next: 3 }],
  ])("is none for %s", (_, value) => {
    expect(readListing(value)).toBeNull();
  });
});

describe("a change the feed sends", () => {
  it("of a task's fields is read as it was sent", () => {
    expect(readTaskChange(CASES.samples.taskChange)).toEqual(CASES.samples.taskChange);
    expect(readTaskChange(CASES.samples.bareTaskChange)).toEqual(CASES.samples.bareTaskChange);
    expect(readTaskChange({ key: "w/a", fields: { title: null } })).not.toBeNull();
  });

  it.each([
    ["nothing", null],
    ["no key", { fields: {} }],
    ["no fields", { key: "w/a" }],
    ["a title that is no text", { key: "w/a", fields: { title: 1 } }],
    ["a priority that is text", { key: "w/a", fields: { priority: "1" } }],
    ["roles it cannot read", { key: "w/a", fields: { roles: [{}] } }],
    ["an instruction that is no flag", { key: "w/a", fields: { instruction: "yes" } }],
  ])("of a task's fields is none for %s", (_, value) => {
    expect(readTaskChange(value)).toBeNull();
  });

  it("of a task adds, replaces or removes it", () => {
    expect(readItemChange(CASES.samples.taskAdded)).toEqual({
      ...(CASES.samples.taskAdded as object),
      removed: false,
    });
    expect(readItemChange(CASES.samples.taskRemoved)).toEqual({ key: "w/a", removed: true });
    expect(readItemChange({ key: "w/a", value: null })).toEqual({ key: "w/a", removed: false });
  });

  it.each([
    ["nothing", null],
    ["no key", { removed: true }],
    ["a removal that is no flag", { key: "w/a", removed: "yes" }],
    ["a task it cannot read", { key: "w/a", value: { slug: "w/a" } }],
  ])("of a task is none for %s", (_, value) => {
    expect(readItemChange(value)).toBeNull();
  });

  it("of the owner's sign-off is its slugs", () => {
    expect(readSlugs(["w/a", "w/b"])).toEqual(["w/a", "w/b"]);
    expect(readSlugs([])).toEqual([]);
    expect(readSlugs([1])).toBeNull();
    expect(readSlugs("w/a")).toBeNull();
  });
});

describe("a task's changed fields", () => {
  it("land in the task, the rest of it as it was", () => {
    const task = aTask("w/a", { parent: "w", links: ["docs/A.md"] });
    const changed = merge(task, {
      title: "Renamed",
      status: "review",
      priority: 1,
      roles: [aRole("executor", "amber")],
      body: null,
    });
    expect(changed).toEqual({
      ...task,
      title: "Renamed",
      status: "review",
      priority: 1,
      roles: [aRole("executor", "amber")],
    });
    expect(task.title).toBe("w/a");
  });

  it("clear its parent with a parent of none, and move it with another", () => {
    const task = aTask("w/a", { parent: "w" });
    expect(merge(task, { parent: "" }).parent).toBeNull();
    expect(merge(task, { parent: "x" }).parent).toBe("x");
    expect(merge(task, { parent: null }).parent).toBe("w");
    expect(merge(task, {})).toEqual(task);
  });
});

describe("a holder's health", () => {
  const held = (slug: string, roles: [string, string, string][]) =>
    aTask(slug, { roles: roles.map(([handle, role, health]) => aRole(role, handle, health)) });
  const healths = (tasks: Task[]) =>
    tasks.map((task) => task.roles.map((role) => [role.handle, role.health]));

  // The feed says a holder's health once, as its session: every role of that handle, on every
  // task, says it, and no other role changes.
  it("reaches every role its holder holds and no other", () => {
    const tasks = [
      held("work/a", [
        ["amber", "executor", "live"],
        ["birch", "orchestrator", "live"],
      ]),
      held("work/b", [["amber", "reviewer", "live"]]),
      held("work/c", [["birch", "executor", "live"]]),
      held("work/d", []),
    ];
    const [changed, next] = holderHealth(tasks, "amber", "stale");
    expect(changed).toBe(true);
    expect(healths(next)).toEqual([
      [
        ["amber", "stale"],
        ["birch", "live"],
      ],
      [["amber", "stale"]],
      [["birch", "live"]],
      [],
    ]);
    expect(next[2]).toBe(tasks[2]);
    // A session that holds nothing changes nothing.
    expect(holderHealth(next, "cedar", "left")).toEqual([false, next]);
    expect(holderHealth(next, "amber", "stale")[0]).toBe(false);
  });

  it("reads lost for a session gone from the board", () => {
    const [, next] = holderHealth(
      [held("work/a", [["amber", "executor", "stale"]])],
      "amber",
      null,
    );
    expect(healths(next)).toEqual([[["amber", "lost"]]]);
  });
});
