import { describe, expect, it } from "vitest";
import { aMachine, anOperation, aWork, CASES } from "./testing";
import type { MachineWork } from "./wire";
import {
  drawsAlike,
  freshnessWord,
  isFor,
  NO_WORK,
  phaseLabel,
  phases,
  placesAlike,
  readWork,
  samePhases,
  scopedPhases,
  shownPhases,
  shownScoped,
  taskStage,
} from "./work";

const { works, tasks, placed, alike } = CASES.stages;
const work = (name: string): MachineWork => works[name] as MachineWork;

describe("the stage a task stands in", () => {
  it.each(placed)("under %s, for task %i (connected: %s), is %s", (name, at, connected, stage) => {
    const task = tasks[at];
    expect(task).toBeDefined();
    if (task) expect(taskStage(work(name), task, connected)).toBe(stage);
  });

  it("is its status for every task while no machine reports", () => {
    for (const task of tasks) expect(taskStage(NO_WORK, task, true)).toBe(task.status);
  });
});

describe("two works", () => {
  it.each(alike)("%s and %s place every task alike: %s", (a, b, same) => {
    expect(placesAlike(work(a), work(b))).toBe(same);
    expect(placesAlike(work(b), work(a))).toBe(same);
  });
});

describe("the machines' work as the client reads it", () => {
  it("is the server's, its guidance and freshness as it sent them", () => {
    expect(readWork(CASES.samples.work)).toEqual(CASES.samples.work);
    expect(readWork(CASES.samples.emptyWork)).toEqual(NO_WORK);
  });

  it("takes a missing freshness for unknown and missing guidance for none", () => {
    expect(readWork({ machines: [], unassigned: [] })).toEqual(NO_WORK);
  });

  it.each([
    ["nothing", null],
    ["an unknown freshness", { freshness: "hot", machines: [], unassigned: [] }],
    ["no machines", { unassigned: [] }],
    ["no unassigned work", { machines: [] }],
    [
      "a machine without its id",
      { machines: [{ ...aMachine("a", []), machine_id: 1 }], unassigned: [] },
    ],
    [
      "a machine of another freshness",
      { machines: [{ ...aMachine("a", []), freshness: "x" }], unassigned: [] },
    ],
    [
      "a machine without completeness",
      { machines: [{ ...aMachine("a", []), complete: "yes" }], unassigned: [] },
    ],
    [
      "an operation of an unknown kind",
      {
        machines: [
          {
            ...aMachine("a", []),
            operations: [{ ...anOperation("a", "1", "build", "running"), kind: "lint" }],
          },
        ],
        unassigned: [],
      },
    ],
    [
      "an operation of an unknown state",
      {
        machines: [],
        unassigned: [{ ...anOperation("a", "1", "build", "running"), state: "done" }],
      },
    ],
    [
      "an operation's task that is no text",
      { machines: [], unassigned: [{ ...anOperation("a", "1", "build", "running"), task: 3 }] },
    ],
    [
      "an operation without its recipe",
      {
        machines: [],
        unassigned: [{ ...anOperation("a", "1", "build", "running"), recipe: null }],
      },
    ],
    [
      "tasks above an operation that are not texts",
      { machines: [], unassigned: [{ ...anOperation("a", "1", "build", "running"), under: [1] }] },
    ],
    [
      "tasks above an operation that are no list",
      {
        machines: [],
        unassigned: [{ ...anOperation("a", "1", "build", "running"), under: "sim" }],
      },
    ],
  ])("is none for %s", (_, value) => {
    expect(readWork(value)).toBeNull();
  });
});

describe("an operation", () => {
  it("is its own task's work and that of each task it is under", () => {
    const own = anOperation("alpha", "1", "build", "running");
    expect(isFor(own, "sim/a")).toBe(true);
    expect(isFor(own, "sim")).toBe(false);
    const under = { ...own, under: ["sim"] };
    expect(isFor(under, "sim")).toBe(true);
    expect(isFor({ ...own, task: null }, "sim/a")).toBe(false);
  });
});

describe("a card's build and test phases", () => {
  // Explicit attribution and nonce deduplication, as the Dioxus client's machine_work tests.
  it("count one wrapper of an operation on one machine once, and keep another machine's apart", () => {
    const build = anOperation("alpha", "same-nonce", "build", "running");
    const rows = phases(
      aWork([
        aMachine("alpha", [build, build]),
        aMachine("beta", [anOperation("beta", "same-nonce", "test", "queued")]),
      ]),
      "sim/a",
    );
    expect(rows.map(phaseLabel)).toEqual(["Build running · alpha", "Test queued · beta"]);
  });

  it("show a task's builds and tests, never its setups, commands or reservations", () => {
    const work = aWork([
      aMachine("alpha", [
        anOperation("alpha", "build", "build", "queued"),
        anOperation("alpha", "native", "test", "running", { task: null }),
        anOperation("alpha", "setup", "setup", "running"),
        anOperation("alpha", "command", "command", "running"),
        anOperation("alpha", "reservation", "reservation", "queued"),
      ]),
    ]);
    expect(phases(work, "sim/a").map(phaseLabel)).toEqual(["Build queued · alpha"]);
    expect(phases(work, "sim/other")).toEqual([]);
    // The card of a task the run's task is under carries its chip too, once the server says so.
    expect(phases(work, "sim")).toEqual([]);
    const placed = aWork([
      aMachine("alpha", [anOperation("alpha", "build", "build", "queued", { under: ["sim"] })]),
    ]);
    expect(phases(placed, "sim").map(phaseLabel)).toEqual(["Build queued · alpha"]);
    expect(phases(placed, "sim/a")).toHaveLength(1);
  });

  it("say how sure a partial, stale or contradicted observation is", () => {
    const test = anOperation("alpha", "test", "test", "running");
    const label = (work: MachineWork) => phases(work, "sim/a").map(phaseLabel)[0];
    expect(label(aWork([aMachine("alpha", [test], { complete: false })]))).toBe(
      "Observed Test running · alpha · partial inventory",
    );
    expect(label(aWork([aMachine("alpha", [test], { freshness: "stale" })]))).toBe(
      "Observed Test running · alpha · stale",
    );
    expect(label(aWork([aMachine("alpha", [{ ...test, machine_id: "foreign" }])]))).toBe(
      "Observed Test running · Machine unknown · conflicting evidence",
    );
    // A second report of the same operation that says otherwise contradicts the first.
    const queued = { ...test, state: "queued" as const };
    expect(label(aWork([aMachine("alpha", [test, queued])]))).toBe(
      "Observed Test running · alpha · conflicting evidence",
    );
    expect(label(aWork([aMachine("alpha", [test, { ...test, under: ["sim"] }])]))).toBe(
      "Observed Test running · alpha · conflicting evidence",
    );
  });

  it("keep each one no machine vouches for apart, however alike", () => {
    const legacy = anOperation(null, null, "build", "queued");
    const rows = phases(aWork([], { unassigned: [legacy, legacy] }), "sim/a");
    expect(rows.map(phaseLabel)).toEqual([
      "Observed Build queued · Machine unknown · unknown",
      "Observed Build queued · Machine unknown · unknown",
    ]);
    const claimed = {
      ...legacy,
      machine_id: "claimed-alpha",
      operation_id: "same-untrusted-nonce",
    };
    expect(phases(aWork([], { unassigned: [claimed, claimed] }), "sim/a")).toHaveLength(2);
    // A machine's own report without an id is never taken for another's.
    const unnamed = anOperation("alpha", null, "build", "queued");
    expect(phases(aWork([aMachine("alpha", [unnamed, unnamed])]), "sim/a")).toHaveLength(2);
  });

  // What a card's phase chips read (work-phases.tsx), in their order, read as disconnected while
  // the feed is, and none for a task with no work.
  it("read as disconnected while the feed is, a machine's own and no other", () => {
    const chips = (work: MachineWork, slug: string, connected: boolean) =>
      shownPhases(work, slug, connected).map(phaseLabel);
    const work = aWork(
      [
        aMachine("alpha", [
          anOperation("alpha", "build", "build", "running"),
          anOperation("alpha", "test", "test", "queued"),
        ]),
      ],
      { unassigned: [anOperation(null, null, "test", "queued")] },
    );
    expect(chips(work, "sim/a", true)).toEqual([
      "Build running · alpha",
      "Test queued · alpha",
      "Observed Test queued · Machine unknown · unknown",
    ]);
    expect(chips(work, "sim/a", false)).toEqual([
      "Observed Build running · alpha · disconnected",
      "Observed Test queued · alpha · disconnected",
      "Observed Test queued · Machine unknown · unknown",
    ]);
    expect(shownPhases(work, "sim/a", false)[0]?.freshness).toBe("disconnected");
    expect(chips(work, "sim/b", true)).toEqual([]);
  });
});

describe("two reports of the machines' work", () => {
  const build = anOperation("alpha", "b", "build", "running");
  const work = aWork([aMachine("alpha", [build])]);

  it("draw alike for a heartbeat: another producer, sequence or age", () => {
    expect(drawsAlike(work, work)).toBe(true);
    const beat = aWork([
      aMachine("alpha", [build], { producer_id: "p2", sequence: 9, age_ms: 900 }),
    ]);
    expect(drawsAlike(work, beat)).toBe(true);
    expect(drawsAlike(NO_WORK, { ...NO_WORK })).toBe(true);
  });

  it("differ for anything the page draws", () => {
    expect(drawsAlike(work, aWork([aMachine("alpha", [build], { freshness: "stale" })]))).toBe(
      false,
    );
    expect(drawsAlike(work, aWork([aMachine("alpha", [{ ...build, state: "queued" }])]))).toBe(
      false,
    );
    expect(drawsAlike(work, aWork([aMachine("alpha", [build], { label: "Alpha" })]))).toBe(false);
    expect(drawsAlike(work, aWork([aMachine("alpha", [build])], { freshness: "stale" }))).toBe(
      false,
    );
    expect(drawsAlike(work, aWork([]))).toBe(false);
  });
});

describe("two lists of phases", () => {
  const build = anOperation("alpha", "b", "build", "running");
  const work = aWork([aMachine("alpha", [build])]);

  it("are alike when they draw the same chips, whatever else their machines say", () => {
    const later = aWork([aMachine("alpha", [build], { sequence: 9, age_ms: 900 })]);
    expect(samePhases(phases(work, "sim/a"), phases(later, "sim/a"))).toBe(true);
    expect(samePhases([], [])).toBe(true);
  });

  it("differ in a chip's count, words, look or title", () => {
    const same = (other: MachineWork) => samePhases(phases(work, "sim/a"), phases(other, "sim/a"));
    expect(same(aWork([]))).toBe(false);
    expect(same(aWork([aMachine("alpha", [build, { ...build, operation_id: "c" }])]))).toBe(false);
    expect(same(aWork([aMachine("alpha", [{ ...build, state: "queued" }])]))).toBe(false);
    expect(same(aWork([aMachine("alpha", [build], { freshness: "stale" })]))).toBe(false);
    expect(same(aWork([aMachine("alpha", [{ ...build, recipe: "other" }])]))).toBe(false);
    expect(same(aWork([aMachine("alpha", [{ ...build, resource: "slot" }])]))).toBe(false);
    expect(same(aWork([aMachine("alpha", [{ ...build, reason: "waits" }])]))).toBe(false);
    expect(same(aWork([aMachine("alpha", [build], { label: "Alpha" })]))).toBe(false);
    expect(samePhases(shownPhases(work, "sim/a", true), shownPhases(work, "sim/a", false))).toBe(
      false,
    );
  });
});

describe("a freshness", () => {
  it.each([
    ["fresh", "fresh"],
    ["stale", "stale"],
    ["disconnected", "disconnected"],
    ["incomplete", "partial inventory"],
    ["conflict", "conflicting evidence"],
    ["unknown", "unknown"],
  ] as const)("%s reads %s", (freshness, word) => {
    expect(freshnessWord(freshness)).toBe(word);
  });
});

describe("an agent's, a machine's and the unassigned phases", () => {
  const op = anOperation;

  it("deduplicate a machine's reports of one operation without merging other machines'", () => {
    const build = op("alpha", "same-nonce", "build", "running");
    const twice = aWork([
      aMachine("alpha", [build, { ...build }]),
      aMachine("beta", [op("beta", "same-nonce", "test", "queued")]),
    ]);
    expect(scopedPhases(twice, { agent: "worker-a" }).map(phaseLabel)).toEqual([
      "Build running · alpha",
      "Test queued · beta",
    ]);
  });

  it("count only builds and tests, owned by the agent or the task they name", () => {
    const work = aWork([
      aMachine("alpha", [
        op("alpha", "build", "build", "queued"),
        op("alpha", "native", "test", "running", { task: null }),
        op("alpha", "setup", "setup", "running"),
        op("alpha", "command", "command", "running"),
        op("alpha", "reservation", "reservation", "queued"),
      ]),
    ]);
    expect(scopedPhases(work, { task: "sim/a" }).map(phaseLabel)).toEqual(["Build queued · alpha"]);
    expect(scopedPhases(work, { agent: "worker-a" })).toHaveLength(2);
    expect(scopedPhases(work, { agent: "other-worker" })).toEqual([]);
    expect(scopedPhases(work, { machine: "alpha" })).toHaveLength(2);
    expect(scopedPhases(work, { machine: "beta" })).toEqual([]);
  });

  it("keep a stale machine's sibling stale in every scope, and an unproven row apart", () => {
    const work = aWork([
      aMachine(
        "alpha",
        [
          op("alpha", "test", "test", "running"),
          op("alpha", "new-sibling", "build", "queued", { task: "sim/new" }),
        ],
        { freshness: "stale", age_ms: 10_002 },
      ),
    ]);
    for (const scope of [{ task: "sim/a" }, { agent: "worker-a" }, { machine: "alpha" }]) {
      expect(shownScoped(work, scope, true).map(phaseLabel)).toContain(
        "Observed Test running · alpha · stale",
      );
    }
    const foreign = aWork([
      aMachine("alpha", [op("foreign", "test", "test", "running")], { freshness: "stale" }),
    ]);
    expect(scopedPhases(foreign, { machine: "foreign" }).map(phaseLabel)).toEqual([
      "Observed Test running · Machine unknown · conflicting evidence",
    ]);
    expect(scopedPhases(foreign, { machine: "alpha" })).toEqual([]);
  });

  it("of the unassigned are the operations no machine vouches for, never deduplicated", () => {
    const legacy = op(null, null, "build", "queued");
    const claimed = op("claimed-alpha", "same-untrusted-nonce", "build", "queued");
    const work = aWork([aMachine("alpha", [op("alpha", "b", "test", "running")])], {
      unassigned: [legacy, legacy, claimed, claimed],
    });
    const rows = scopedPhases(work, "unassigned");
    expect(rows.map(phaseLabel)).toEqual(
      Array(4).fill("Observed Build queued · Machine unknown · unknown"),
    );
    expect(scopedPhases(work, { machine: "claimed-alpha" })).toHaveLength(2);
    expect(shownScoped(work, "unassigned", false).map((row) => row.freshness)).toEqual(
      Array(4).fill("unknown"),
    );
  });
});
