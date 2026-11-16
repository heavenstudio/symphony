import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import fixture from "../../../crates/symphony-api/fixtures/task-gates.json";
import {
  byline,
  civil,
  codeSpans,
  countOf,
  counts,
  DEFAULT_ROOM,
  evidence,
  failed,
  fnv1a,
  gatesOf,
  KEPT_MAX,
  keep,
  latest,
  localMinutes,
  noGates,
  pieces,
  type Range,
  Room,
  readGate,
  readGatesSeen,
  readState,
  readTaskGates,
  recall,
  seenOf,
  signed,
  unsaidCommit,
  waitingGates,
  waits,
  waitsOn,
  when,
  workOf,
} from "./gates-model";
import { NO_ANSWER } from "./request";
import { aMachine, anOperation, aRole, aTask, aWork } from "./testing";
import { NOT_IN_TIME } from "./trouble";
import type { Gate, GatesSeen, MachineWork, Role, TaskGates, WorkState } from "./wire";
import { moment, parseMoment } from "./words";

/** 2026-11-11 13:10 UTC, the day of every time in the fixture. */
const NOW = 1_794_402_600;

/** The A pictures' task: certificate missing, dependency waived, nightly met, review met. */
const pictured = () => readTaskGates(structuredClone(fixture)) as TaskGates;

/** A gate as the section reads it from `value`. */
function gate(value: object): Gate {
  const read = readGate(value);
  if (read === null) throw new Error(`not a gate: ${JSON.stringify(value)}`);
  return read;
}

const answer = (gates: Gate[]): TaskGates => ({ slug: "tools/x", branch: "tools/x", gates });

const room = (count: number, waiting: number, drawn: [number, number] | null = null): Room => ({
  seen: { count, waiting, unsigned: 0 },
  drawn,
});

// The reader's clock is UTC unless a test says otherwise, as the Dioxus client's host was.
beforeEach(() => {
  vi.spyOn(Date.prototype, "getTimezoneOffset").mockReturnValue(0);
});
afterEach(() => vi.restoreAllMocks());

describe("the section's state from a read", () => {
  it("is the answer, and an empty one for a task the board has not", () => {
    expect(gatesOf({ found: null })).toEqual({ kind: "read", answer: noGates() });
    expect(gatesOf({ found: pictured() })).toEqual({ kind: "read", answer: pictured() });
  });

  it("says a read the server never answered took too long, and any other failure why", () => {
    expect(gatesOf({ failed: NOT_IN_TIME })).toEqual({ kind: "slow" });
    // A post's words are no read's.
    expect(gatesOf({ failed: NO_ANSWER })).toEqual({ kind: "failed", why: NO_ANSWER });
    expect(gatesOf({ failed: "The server answered 500." })).toEqual({
      kind: "failed",
      why: "The server answered 500.",
    });
    expect(gatesOf({ failed: "" })).toEqual({ kind: "failed", why: "" });
  });

  it("gives each task nothing gates an answer of its own", () => {
    const one = noGates();
    one.gates.push(gate({ name: "x", state: "met" }));
    expect(noGates().gates).toEqual([]);
  });
});

describe("what a failed read says", () => {
  it("is that the gates could not be read, with the reason once, and the rest works", () => {
    expect(failed("The server answered 500.")).toBe(
      "The gates could not be read: The server answered 500. The rest of the page still works.",
    );
    expect(failed(" ")).toBe("The gates could not be read. The rest of the page still works.");
    expect(failed("")).toBe("The gates could not be read. The rest of the page still works.");
    expect(failed("...")).toBe("The gates could not be read. The rest of the page still works.");
    expect(failed("  gone..  ")).toBe(
      "The gates could not be read: gone. The rest of the page still works.",
    );
  });

  it("trims what Rust's trim does, and a dot only where the reason ends", () => {
    expect(failed("\u{3000}gone\u{a0}")).toContain("read: gone. The rest");
    expect(failed("gone . ")).toBe(
      "The gates could not be read: gone . The rest of the page still works.",
    );
    expect(failed("\u{feff}gone")).toContain("read: \u{feff}gone. The rest");
  });

  it("is the browser target's sentence for a refused snapshot", () => {
    expect(failed("Board snapshot unavailable (the server answered 500)")).toBe(
      "The gates could not be read: Board snapshot unavailable (the server answered 500). The rest of the page still works.",
    );
  });
});

/** A task in `status`, held by `roles` and depending on `depends`. */
const task = (status: string, roles: Role[] = [], depends: string[] = []) =>
  aTask("tools/x", { status, roles, depends });

/** What a task's gates hold of an unassessed task's stamps: review missing, the rest waived. */
const stamped = answer([
  gate({ name: "review", state: "missing", detail: "Not reviewed yet" }),
  gate({ name: "check", state: "unknown" }),
  gate({ name: "tests", state: "waived" }),
]);

/** `stamped` with a card the task posted waiting on the owner. */
const asking = answer([...stamped.gates, gate({ name: "owner", state: "held" })]);

describe("what the task waits on", () => {
  it("in review, names every missing or held gate in their order", () => {
    const named = (states: [string, string][]) =>
      waitsOn(answer(states.map(([name, state]) => gate({ name, state }))), task("review"));
    expect(named([])).toBeNull();
    expect(
      named([
        ["review", "met"],
        ["dependency", "waived"],
      ]),
    ).toBeNull();
    expect(named([["stamps", "unknown"]])).toBeNull();
    expect(
      named([
        ["certificate", "missing"],
        ["review", "met"],
      ]),
    ).toEqual({ tone: "gate-missing", names: ["certificate"] });
    expect(
      named([
        ["owner", "held"],
        ["certificate", "missing"],
      ]),
    ).toEqual({ tone: "gate-held", names: ["owner", "certificate"] });
    expect(
      named([
        ["certificate", "missing"],
        ["check", "missing"],
        ["owner", "held"],
        ["nightly", "met"],
      ]),
    ).toEqual({ tone: "gate-missing", names: ["certificate", "check", "owner"] });
    expect(
      waitsOn(stamped, task("review", [aRole("reviewer", "rapid-badger")])),
      "its reviewer is the review gate's byline, not the pill's",
    ).toEqual({ tone: "gate-missing", names: ["review"] });
  });

  it("in review, takes the state of the first it waits on, whatever comes before it", () => {
    const later = answer([
      gate({ name: "nightly", state: "met" }),
      gate({ name: "later", state: "expired" }),
      gate({ name: "owner", state: "held" }),
    ]);
    expect(waitsOn(later, task("review"))).toEqual({ tone: "gate-held", names: ["owner"] });
  });

  it("in progress, names its executor, not the review it has not reached", () => {
    const working = task("in_progress", [
      aRole("orchestrator", "silver-sparrow"),
      aRole("executor", "dusky-plover"),
    ]);
    expect(waitsOn(stamped, working)).toEqual({
      tone: "st-in_progress",
      names: ["executor dusky-plover"],
    });
    expect(waitsOn(noGates(), working), "whatever its gates, even none: it is the stage's").toEqual(
      { tone: "st-in_progress", names: ["executor dusky-plover"] },
    );
    expect(
      waitsOn(stamped, task("in_progress", [aRole("executor", "a"), aRole("executor", "b")])),
    ).toEqual({ tone: "st-in_progress", names: ["executor a", "executor b"] });
    expect(
      waitsOn(stamped, task("in_progress", [aRole("orchestrator", "silver-sparrow")])),
      "nobody executes it: no executor to name, and no gate it waits on yet",
    ).toBeNull();
  });

  it("in progress, says an executor that is gone lost or left, never at work", () => {
    for (const health of ["lost", "left"]) {
      expect(
        waitsOn(stamped, task("in_progress", [aRole("executor", "dusky-plover", health)])),
        health,
      ).toEqual({
        tone: "gate-missing",
        names: [`executor dusky-plover ${health}`],
      });
    }
    expect(
      waitsOn(
        stamped,
        task("design", [aRole("executor", "a", "stale"), aRole("executor", "b", "lost")]),
      ),
      "one still there: the stage's colour, the gone one said gone",
    ).toEqual({ tone: "st-design", names: ["executor a", "executor b lost"] });
    expect(
      waitsOn(asking, task("in_progress", [aRole("executor", "dusky-plover", "left")])),
      "a held gate after a gone executor: nobody works it still",
    ).toEqual({ tone: "gate-missing", names: ["executor dusky-plover left", "owner"] });
  });

  it("in concept or design, names the executor working its stage", () => {
    for (const status of ["concept", "design"]) {
      expect(waitsOn(stamped, task(status, [aRole("executor", "ochre-crane")])), status).toEqual({
        tone: `st-${status}`,
        names: ["executor ochre-crane"],
      });
      expect(waitsOn(stamped, task(status)), `${status} with nobody`).toBeNull();
    }
  });

  it("blocked, names every task it depends on", () => {
    const blocked = task(
      "blocked",
      [aRole("executor", "umber-ibis")],
      ["tools/board-postgresql", "tools/machine-runs-the-release"],
    );
    expect(waitsOn(stamped, blocked)).toEqual({
      tone: "st-blocked",
      names: ["tools/board-postgresql", "tools/machine-runs-the-release"],
    });
    expect(
      waitsOn(stamped, task("blocked", [aRole("executor", "umber-ibis")])),
      "blocked by hand: its thread says why, not its gates",
    ).toBeNull();
  });

  it("before it starts, waits on nothing", () => {
    for (const status of ["backlog", "ready"]) {
      expect(waitsOn(stamped, task(status, [aRole("executor", "vivid-robin")])), status).toBeNull();
    }
  });

  it("while open, names each held gate after who its status waits on", () => {
    expect(waitsOn(asking, task("in_progress", [aRole("executor", "dusky-plover")]))).toEqual({
      tone: "st-in_progress",
      names: ["executor dusky-plover", "owner"],
    });
    expect(waitsOn(asking, task("blocked", [], ["tools/a"]))).toEqual({
      tone: "st-blocked",
      names: ["tools/a", "owner"],
    });
    expect(
      waitsOn(asking, task("blocked")),
      "a landing that holds it for the owner's approve",
    ).toEqual({ tone: "gate-held", names: ["owner"] });
    for (const status of ["backlog", "ready", "concept"]) {
      expect(waitsOn(asking, task(status)), status).toEqual({
        tone: "gate-held",
        names: ["owner"],
      });
    }
  });

  it("done or cancelled, waits on nothing, whatever its gates say", () => {
    const landed = answer([
      gate({ name: "review", state: "missing", detail: "Landed with no review verdict" }),
      gate({ name: "owner", state: "held" }),
    ]);
    for (const status of ["done", "cancelled"]) {
      expect(
        waitsOn(landed, task(status, [aRole("executor", "quiet-swift", "left")])),
        status,
      ).toBeNull();
    }
  });
});

describe("the counts", () => {
  it("go from what no longer stands in the way to what the task waits on", () => {
    const states = ["missing", "met", "held", "unknown", "waived", "met", "missing", "met"];
    const every = answer(states.map((state, n) => gate({ name: `g${n}`, state })));
    expect(counts(every)).toBe("3 met · 1 waived · 1 unknown · 1 held · 2 missing");
    expect(counts(pictured())).toBe("2 met · 1 waived · 1 missing");
    expect(counts(answer([]))).toBe("");
  });
});

/** crates/symphony-api's gates.rs: what an answer counts. */
describe("an answer's gates", () => {
  const ranked = answer(
    (
      [
        ["certificate", "missing"],
        ["check", "missing"],
        ["hold", "held"],
        ["stamps", "unknown"],
        ["dependency", "waived"],
        ["nightly", "met"],
        ["review", "met"],
      ] as const
    ).map(([name, state]) => gate({ name, state })),
  );

  it("count each state, and wait on what is missing or held", () => {
    expect(countOf(ranked, "met")).toBe(2);
    expect(countOf(ranked, "waived")).toBe(1);
    expect(countOf(ranked, "unknown")).toBe(1);
    expect(seenOf(ranked)).toEqual({ count: 7, waiting: 3, unsigned: 7 });
    expect(seenOf(noGates())).toEqual({ count: 0, waiting: 0, unsigned: 0 });
    expect(waitingGates(ranked).map((one) => one.name)).toEqual(["certificate", "check", "hold"]);
    expect(seenOf(pictured())).toEqual({ count: 4, waiting: 1, unsigned: 0 });
  });

  it("count those with no who or when to draw", () => {
    const gates = [...unsignedGates(), ...signedGates()];
    expect(seenOf(answer(gates))).toEqual({ count: 9, waiting: 9, unsigned: 5 });
  });
});

/** A review gate missing, changed by `edit`. */
const review = (edit: Partial<Gate> = {}): Gate => ({
  ...gate({ name: "review", state: "missing" }),
  ...edit,
});

const unsignedGates = () => [
  review(),
  review({ by: "" }),
  review({ commit: "" }),
  review({ commit: "4be91c2aa10", detail: "Verify on 4be91c2 running" }),
  review({ commit: "4be91c2", why: "4be91c2 is the tree it read" }),
];

const signedGates = () => [
  review({ by: "lucid-heron" }),
  review({ at: 1_794_398_700 }),
  review({ commit: "4be91c2aa10" }),
  review({ commit: "4be91c2", detail: "Green on main a71f0d3" }),
];

/** crates/symphony-blocks' gates.rs: one gate's state and signature. */
describe("a gate", () => {
  it("reads a state word written by a newer server as unknown", () => {
    for (const [word, state] of [
      ["missing", "missing"],
      ["held", "held"],
      ["unknown", "unknown"],
      ["waived", "waived"],
      ["met", "met"],
      ["expired", "unknown"],
      ["", "unknown"],
      ["Met", "unknown"],
    ] as const) {
      expect(readState(word), word).toBe(state);
    }
    for (const notWord of [undefined, null, 3, true, ["met"], { met: true }]) {
      expect(readState(notWord)).toBeNull();
    }
    expect(waits("missing") && waits("held")).toBe(true);
    expect(waits("unknown") || waits("waived") || waits("met")).toBe(false);
  });

  it("is signed by who, when or a commit its words do not say", () => {
    for (const one of unsignedGates()) expect(signed(one), JSON.stringify(one)).toBe(false);
    for (const one of signedGates()) expect(signed(one), JSON.stringify(one)).toBe(true);
    expect(unsaidCommit(review({ commit: "4be91c2aa10" }))).toBe("4be91c2");
    expect(unsaidCommit(review({ commit: "4be91c2aa10", detail: "Verify on 4be91c2" }))).toBeNull();
    expect(unsaidCommit(review({ commit: "4be9" })), "a short commit is said as it is").toBe(
      "4be9",
    );
    expect(unsaidCommit(review({ commit: null })), "none").toBeNull();
    expect(signed(review({ by: null, at: null, commit: null }))).toBe(false);
  });

  it("says its commit's first seven characters, not its first seven code units", () => {
    expect(unsaidCommit(review({ commit: "😀😀😀😀😀😀😀😀" }))).toBe("😀😀😀😀😀😀😀");
  });

  it("with only its name and state reads with every other field empty", () => {
    expect(readGate({ name: "review", state: "met" })).toEqual({
      name: "review",
      state: "met",
      detail: "",
    });
  });
});

describe("a byline", () => {
  // Now on the reader's clock, read in each test, under its clock.
  const line = (value: object) => byline(gate(value), localMinutes(NOW)).join(" · ");

  it("says who, when and the commit its words do not name", () => {
    expect(
      byline(
        gate({ name: "a", state: "met", by: "lucid-heron", at: 1_794_398_700 }),
        localMinutes(NOW),
      ),
    ).toEqual(["lucid-heron", "12:05"]);
    expect(line({ name: "a", state: "met", at: 1_794_398_700 })).toBe("12:05");
    expect(line({ name: "a", state: "met", by: "nightly" })).toBe("nightly");
    expect(line({ name: "a", state: "met", by: "" }), "an empty handle says nothing").toBe("");
    expect(line({ name: "a", state: "met" })).toBe("");
    expect(
      line({
        name: "a",
        state: "met",
        by: "studio",
        at: 1_794_363_240 - 86_400,
        commit: "e19d3a0c4b7f2a9e",
      }),
      "a full commit id is said short, on another day with its date",
    ).toBe("studio · 11-10 02:14 · e19d3a0");
    expect(line({ name: "a", state: "met", commit: "e19d3a0", detail: "green at e19d3a0" })).toBe(
      "",
    );
    expect(
      line({ name: "a", state: "waived", commit: "e19d3a0", why: "e19d3a0 is the same tree" }),
      "nor one its why names",
    ).toBe("");
    expect(line({ name: "a", state: "met", commit: "" }), "an empty commit says nothing").toBe("");
  });

  it("says a time at the epoch, which is a time all the same", () => {
    expect(line({ name: "a", state: "met", at: 0 })).toBe("1970-01-01");
    expect(line({ name: "a", state: "met", by: null, at: null, commit: null })).toBe("");
  });
});

describe("a link's evidence", () => {
  const linked = (detail: string, link: string, commit: string | null = null) => {
    const [start, end] = evidence(detail, link, commit);
    return detail.slice(start, end);
  };

  it("is the name its path says, else the commit, else the round, else all of it", () => {
    const dependency = "Waits on tools/slot-pool-by-tree, waived";
    expect(linked(dependency, "/task/tools/slot-pool-by-tree")).toBe("tools/slot-pool-by-tree");
    expect(linked("Card c in docs/analysis/x.md waits on the owner", "/docs/analysis/x.md#c")).toBe(
      "docs/analysis/x.md",
    );
    expect(
      linked("Verify on 4be91c2a running", "https://ci.example/runs/9", "4be91c2"),
      "the commit as the words say it, longer than the gate's",
    ).toBe("4be91c2a");
    expect(
      linked("CI run 4521 passed", "https://ci.example/runs/4521?x=1"),
      "a web address's own path",
    ).toBe("4521");
    expect(linked("Approved in round 2 of 2", "/task/tools/x#message-124")).toBe("round 2 of 2");
    expect(
      linked("Changes asked in Round 1: the task moves", "/task/tools/x#message-9"),
      "never the route's own word `task`",
    ).toBe("Round 1");
    expect(linked("Played around 3 times", "/task/tools/x"), "`around 3` is no round").toBe(
      "Played around 3 times",
    );
    expect(linked("all of it", "https://ci.example")).toBe("all of it");
    expect(
      linked("tools/slot-pool-by-tree-two waits", "/task/tools/slot-pool-by-tree"),
      "a name inside a longer one is not it",
    ).toBe("tools/slot-pool-by-tree-two waits");
    expect(linked("", "/task/x")).toBe("");
    const url = "https://ci.example.com/builds/0123abcd/log?attempt=3";
    expect(
      linked(`Verify ${url} is running on a merge tree`, url),
      "the link itself, where the words write it out",
    ).toBe(url);
    expect(
      linked("Verify https://ci.example.com/builds/0123abcd/log, attempt 3", url),
      "without its query, a sentence's comma after it",
    ).toBe("https://ci.example.com/builds/0123abcd/log");
    expect(
      linked("See /task/tools/x.", "/task/tools/x#message-9", "4be91c2"),
      "a path of the app written out",
    ).toBe("/task/tools/x");
    expect(
      linked(
        "Verify https://ci.example.com/builds/0123abcd/log/raw is slow",
        "https://ci.example.com/builds/0123abcd/log",
      ),
      "a longer address that starts like it is not it, nor are its own path's parts",
    ).toBe("Verify https://ci.example.com/builds/0123abcd/log/raw is slow");
    expect(
      linked("cd-2 then ab-1", "/task/ab-1/cd-2"),
      "of two runs as long, the one the path says first",
    ).toBe("ab-1");
    expect(
      linked("ab-1, then ab-1/cd-2", "/task/ab-1/cd-2"),
      "the longest run said, wherever the words say it",
    ).toBe("ab-1/cd-2");
    expect(
      linked("Run 4521 of 77", "https://ci.example//runs//4521/"),
      "a path with empty segments still finds its name",
    ).toBe("4521");
  });

  it("weighs two runs as Rust does, by their UTF-8 bytes", () => {
    // `ab-é` and `ab-12` are five bytes each: the first the path says wins, though `ab-é` is
    // four code units and `ab-12` five.
    expect(linked("ab-12 and ab-é", "/task/ab-é/x/ab-12")).toBe("ab-é");
    // A commit of two characters and four bytes is long enough to look for.
    expect(linked("at éé now", "https://ci.example", "éé")).toBe("éé");
    expect(linked("at abc now", "https://ci.example", "abc"), "three bytes are not").toBe(
      "at abc now",
    );
  });

  it("is a word of its own, a letter outside the ASCII ones before it included", () => {
    expect(linked("𝐀tools/x waits", "/task/tools/x"), "a surrogate pair's letter").toBe(
      "𝐀tools/x waits",
    );
    expect(linked("— tools/x —", "/task/tools/x"), "a dash outside ASCII ends a word").toBe(
      "tools/x",
    );
    expect(linked("tools/xé", "/task/tools/x")).toBe("tools/xé");
  });

  it("finds the commit only where it ends, its further hex digits with it", () => {
    expect(linked("tree 4be91c2ff, done", "https://h", "4be91c2")).toBe("4be91c2ff");
    expect(linked("tree 4be91c2g", "https://h", "4be91c2"), "a letter past hex ends no word").toBe(
      "tree 4be91c2g",
    );
    expect(linked("x4be91c2 then 4be91c2", "https://h", "4be91c2"), "the first said whole").toBe(
      "4be91c2",
    );
    expect(linked("no commit", "https://h", ""), "an empty commit is none").toBe("no commit");
  });

  it("finds a round past one a word runs into, and its total only when it has digits", () => {
    expect(linked("around 3 then round 4", "/task/x")).toBe("round 4");
    expect(linked("round 2 of many", "/task/x")).toBe("round 2");
    expect(linked("ROUND 3 OF 4", "/task/x"), "` of ` is the words' own, in its case").toBe(
      "ROUND 3",
    );
    expect(linked("round two", "/task/x")).toBe("round two");
    expect(linked("round 7", "/task/x")).toBe("round 7");
  });

  it("is the link's path for an address without a scheme's slashes, which has none", () => {
    expect(linked("see mailto:x", "mailto:x")).toBe("mailto:x");
    expect(linked("tools/x-1 waits", "ci.example/tools/x-1"), "no `://`, no path").toBe(
      "tools/x-1 waits",
    );
    expect(linked("host ci.example only", "https://ci.example?q=1")).toBe("host ci.example only");
  });

  it("cuts a link that is all query or fragment to nothing, so only the whole is written out", () => {
    expect(linked("see #top here", "#top")).toBe("#top");
    expect(linked("see top here", "#top")).toBe("see top here");
  });
});

describe("the pieces of a detail", () => {
  it("cut code spans around the linked words", () => {
    const text = "`just verify` green at `4be91c2` now";
    expect(pieces(text, null)).toEqual([
      [
        [true, "just verify"],
        [false, " green at "],
        [true, "4be91c2"],
        [false, " now"],
      ],
      [],
      [],
    ]);
    const at = text.indexOf("4be91c2");
    expect(pieces(text, [at, at + 7])).toEqual([
      [
        [true, "just verify"],
        [false, " green at "],
      ],
      [[true, "4be91c2"]],
      [[false, " now"]],
    ]);
    const green = text.indexOf("green");
    expect(pieces(text, [green, green + 5])).toEqual([
      [
        [true, "just verify"],
        [false, " "],
      ],
      [[false, "green"]],
      [
        [false, " at "],
        [true, "4be91c2"],
        [false, " now"],
      ],
    ]);
    expect(pieces(text, [0, text.length]), "all of it").toEqual([[], codeSpans(text), []]);
    expect(pieces("", null)).toEqual([[], [], []]);
  });

  it("cut a code span the link starts or ends inside", () => {
    const text = "a `bcd` e";
    const linked: Range = [text.indexOf("c"), text.indexOf("e") + 1];
    expect(pieces(text, linked)).toEqual([
      [
        [false, "a "],
        [true, "b"],
      ],
      [
        [true, "cd"],
        [false, " e"],
      ],
      [],
    ]);
  });
});

describe("code spans", () => {
  it("pair their backticks and keep a lone one", () => {
    expect(codeSpans("")).toEqual([]);
    expect(codeSpans("plain words")).toEqual([[false, "plain words"]]);
    expect(codeSpans("`just verify` green at 4be91c2")).toEqual([
      [true, "just verify"],
      [false, " green at 4be91c2"],
    ]);
    expect(codeSpans("ran `a` then `b`")).toEqual([
      [false, "ran "],
      [true, "a"],
      [false, " then "],
      [true, "b"],
    ]);
    expect(codeSpans("an empty `` pair")).toEqual([
      [false, "an empty "],
      [true, ""],
      [false, " pair"],
    ]);
    expect(codeSpans("one `left open")).toEqual([[false, "one `left open"]]);
    expect(codeSpans("`a` and one ` more")).toEqual([
      [true, "a"],
      [false, " and one ` more"],
    ]);
  });
});

describe("the room", () => {
  const seen = (count: number, waiting: number): GatesSeen => ({ count, waiting, unsigned: 0 });
  const kept = room(4, 1, [390, 612]);

  it("is the server's count with this browser's height while they agree", () => {
    expect(Room.of(null, null)).toEqual(DEFAULT_ROOM);
    expect(Room.of(null, kept)).toEqual(kept);
    expect(Room.of(seen(4, 1), kept)).toEqual(kept);
    expect(Room.of(seen(4, 2), kept), "the server's, when it says otherwise").toEqual(
      room(4, 2, null),
    );
    const unsigned = { ...seen(4, 1), unsigned: 1 };
    expect(Room.of(unsigned, kept), "or when one gate lost its who and when").toEqual({
      seen: unsigned,
      drawn: null,
    });
    expect(Room.of(seen(0, 0), null)).toEqual(room(0, 0, null));
    expect(Room.held(kept, 390)).toBe(612);
    expect(Room.held(kept, 1280)).toBeNull();
    expect(Room.held(kept, null)).toBeNull();
    expect(Room.held(room(4, 1, null), 390)).toBeNull();
  });

  it("is four gates, one waited on, with nothing said", () => {
    expect(DEFAULT_ROOM).toEqual({ seen: { count: 4, waiting: 1, unsigned: 0 }, drawn: null });
  });
});

describe("the rooms a browser keeps", () => {
  it("are the newest of each task, at most sixty-four", () => {
    let kept = keep("", "tools/x", room(4, 1, [390, 612]));
    expect(kept).toBe("4 1 0 390 612 tools/x");
    expect(recall(kept, "tools/x")).toEqual(room(4, 1, [390, 612]));
    expect(recall(kept, "tools/y")).toBeNull();
    kept = keep(kept, "tools/y", room(0, 0, null));
    expect(kept).toBe("0 0 0 0 0 tools/y\n4 1 0 390 612 tools/x");
    expect(recall(kept, "tools/y")).toEqual(room(0, 0, null));
    const unsigned: Room = { seen: { count: 2, waiting: 0, unsigned: 1 }, drawn: [1280, 140] };
    kept = keep(kept, "tools/x", unsigned);
    expect(kept, "one line a task").toBe("2 0 1 1280 140 tools/x\n0 0 0 0 0 tools/y");
    expect(recall(kept, "tools/x")).toEqual(unsigned);
    expect(
      recall("4 1 390 612 tools/x", "tools/x"),
      "a line of the shape before the unsigned count is no room",
    ).toBeNull();
    let many = "";
    for (let n = 0; n < 70; n += 1) many = keep(many, `t/${n}`, room(1, 0, null));
    expect(many.split("\n")).toHaveLength(KEPT_MAX);
    expect(KEPT_MAX).toBe(64);
    expect(recall(many, "t/69")).toEqual(room(1, 0, null));
    expect(recall(many, "t/5"), "the oldest went").toBeNull();
    expect(recall("not a room\n1 x 0 0 0 tools/x", "tools/x")).toBeNull();
    expect(recall("3 1 0 0 0 tools/with space", "tools/with space")).toEqual(room(3, 1, null));
  });

  it("are read as Rust reads its lines and numbers", () => {
    expect(recall("", "tools/x")).toBeNull();
    expect(recall("+3 1 0 0 0 tools/x", "tools/x"), "a plus before the digits").toEqual(
      room(3, 1, null),
    );
    expect(recall("03 1 0 390 007 tools/x", "tools/x")).toEqual(room(3, 1, [390, 7]));
    expect(recall("3 1 0 4294967295 1 tools/x", "tools/x")?.drawn).toEqual([4294967295, 1]);
    expect(recall("3 1 0 4294967296 1 tools/x", "tools/x"), "past a u32").toBeNull();
    expect(recall("-3 1 0 0 0 tools/x", "tools/x")).toBeNull();
    expect(recall("3 1 0 0 0  tools/x", "tools/x"), "two spaces make a slug of ` tools/x`").toBe(
      null,
    );
    expect(recall("3 1 0 0 0  tools/x", " tools/x")).toEqual(room(3, 1, null));
    expect(recall("3 1 0 0 0", "")).toBeNull();
    expect(recall("3 1 0 0 0 ", ""), "an empty slug is a slug").toEqual(room(3, 1, null));
    expect(recall("x\r\n3 1 0 0 0 tools/x\r\n", "tools/x"), "a CR before each LF").toEqual(
      room(3, 1, null),
    );
    expect(recall("3 1 0 0 0 tools/x\r", "tools/x"), "a CR kept at the end").toBeNull();
    expect(recall("\n\n3 1 0 0 0 tools/x\n", "tools/x")).toEqual(room(3, 1, null));
    expect(recall("3 1 0 0 0 tools/x\n4 1 0 0 0 tools/x", "tools/x"), "the first").toEqual(
      room(3, 1, null),
    );
  });

  it("keep a line they cannot read until it ages out", () => {
    const kept = keep("not a room\n\n1 0 0 0 0 tools/y", "tools/x", room(2, 1, null));
    expect(kept).toBe("2 1 0 0 0 tools/x\nnot a room\n\n1 0 0 0 0 tools/y");
    expect(keep("", "", room(1, 0, null))).toBe("1 0 0 0 0 ");
  });
});

describe("a task never opened", () => {
  it("takes the room of the last task drawn as wide", () => {
    let kept = keep("", "tools/x", room(4, 1, [390, 612]));
    kept = keep(kept, "tools/y", room(2, 1, [1280, 300]));
    expect(latest(kept, 1280)).toEqual(room(2, 1, [1280, 300]));
    expect(latest(kept, 390)).toEqual(room(4, 1, [390, 612]));
    expect(latest(kept, 800), "no task drawn as wide").toBeNull();
    expect(latest(kept, null), "a width unknown").toBeNull();
    // Opened again and read before it drew, a task's room stands for no width.
    kept = keep(kept, "tools/x", room(3, 0, null));
    expect(latest(kept, 390)).toBeNull();
    expect(latest(kept, 1280)).toEqual(room(2, 1, [1280, 300]));
    expect(latest("", 390)).toBeNull();
    expect(latest("not a room\n2 1 0 390 80 tools/x", 390)).toEqual(room(2, 1, [390, 80]));
  });

  it("takes none from a line with no slug or numbers it cannot read", () => {
    expect(latest("2 1 0 390 80", 390)).toBeNull();
    expect(latest("2 1 0 390 x tools/x\n1 0 0 390 9 tools/y", 390)).toEqual(room(1, 0, [390, 9]));
  });
});

/** A test of `task`'s run `nonce` in `state`, as the Dioxus client's test reports one. */
const run = (task: string, nonce: string | null, state: WorkState) =>
  anOperation("alpha", nonce, "test", state, {
    task,
    checkout: "/c",
    recipe: "just test-rs",
    phase_since: 1,
  });

const onAlpha = (operations: ReturnType<typeof run>[]): MachineWork =>
  aWork([aMachine("alpha", operations)]);

describe("a task's work", () => {
  it("changes when its runs start, move on or end, and not otherwise", () => {
    const queued = onAlpha([run("tools/x", "n1", "queued"), run("tools/y", "n2", "running")]);
    const running = onAlpha([run("tools/x", "n1", "running"), run("tools/y", "n2", "running")]);
    const later = structuredClone(running);
    const [machine] = later.machines;
    if (machine === undefined) throw new Error("no machine");
    machine.sequence = 9;
    machine.age_ms = 4_000;
    machine.operations = [
      { ...(machine.operations[0] as ReturnType<typeof run>), phase_since: 30 },
    ];
    const ended = onAlpha([run("tools/y", "n2", "running")]);
    expect(workOf(queued, "tools/x")).not.toBe(workOf(running, "tools/x"));
    expect(
      workOf(running, "tools/x"),
      "another task's run ending, or the clock, changes nothing",
    ).toBe(workOf(later, "tools/x"));
    expect(workOf(running, "tools/x")).not.toBe(workOf(ended, "tools/x"));
    expect(workOf(ended, "tools/x")).toBe(0);
    expect(workOf(aWork([]), "tools/x")).toBe(0);
    // Two runs of the task, listed in either order, on one machine or two, are the same work.
    const both = onAlpha([run("tools/x", "n1", "running"), run("tools/x", "n3", "queued")]);
    const swapped = onAlpha([run("tools/x", "n3", "queued"), run("tools/x", "n1", "running")]);
    const apart = aWork([aMachine("alpha", [run("tools/x", "n1", "running")])], {
      unassigned: [run("tools/x", "n3", "queued")],
    });
    expect(workOf(both, "tools/x")).toBe(workOf(swapped, "tools/x"));
    expect(workOf(both, "tools/x")).toBe(workOf(apart, "tools/x"));
    expect(workOf(both, "tools/x")).not.toBe(workOf(running, "tools/x"));
    // The second run moving on changes it, as the first's would.
    const moved = onAlpha([run("tools/x", "n1", "running"), run("tools/x", "n3", "running")]);
    expect(workOf(both, "tools/x")).not.toBe(workOf(moved, "tools/x"));
  });

  it("is each run's FNV-1a hash summed as Rust's u64 sums them, to its low 53 bits", () => {
    expect(fnv1a("")).toBe(0xcbf29ce484222325n);
    expect(fnv1a("a")).toBe(0xaf63dc4c8601ec8cn);
    expect(fnv1a("foobar")).toBe(0x85944171f73967e8n);
    const work = onAlpha([run("tools/x", "n1", "running"), run("tools/x", null, "queued")]);
    const sum =
      (fnv1a("n1 /c just test-rs Running") + fnv1a(" /c just test-rs Queued")) % 2n ** 64n;
    expect(workOf(work, "tools/x")).toBe(Number(BigInt.asUintN(53, sum)));
    expect(Number.isSafeInteger(workOf(work, "tools/x"))).toBe(true);
  });

  it("is none for runs of no task, of another, or under the task alone", () => {
    const work = onAlpha([
      { ...run("tools/x", "n1", "running"), task: null },
      run("tools/x-1", "n2", "running"),
      { ...run("tools/y", "n3", "running"), under: ["tools/x"] },
    ]);
    expect(workOf(work, "tools/x")).toBe(0);
  });

  it("hashes a state this build does not know by its word", () => {
    const paused = { ...run("tools/x", "n1", "running"), state: "paused" as WorkState };
    expect(workOf(onAlpha([paused]), "tools/x")).toBe(
      Number(BigInt.asUintN(53, fnv1a("n1 /c just test-rs paused"))),
    );
  });
});

describe("a time", () => {
  const at = (text: string) => parseMoment(text) as number;
  const now = at("2026-11-11 13:10");

  it("is said as short as today lets it be", () => {
    expect(when(at("2026-11-11 00:00"), now)).toBe("00:00");
    expect(when(at("2026-11-11 23:59"), now)).toBe("23:59");
    expect(when(at("2026-11-10 23:59"), now)).toBe("11-10 23:59");
    expect(when(at("2026-01-01 00:00"), now)).toBe("01-01 00:00");
    expect(
      when(at("2026-11-12 08:00"), now),
      "a clock ahead of the reader's still says its day",
    ).toBe("11-12 08:00");
    expect(when(at("2025-12-31 23:59"), now)).toBe("2025-12-31");
    expect(when(at("2024-02-29 12:00"), now)).toBe("2024-02-29");
    expect(when(at("1969-12-31 23:00"), now)).toBe("1969-12-31");
    expect(
      when(at("1969-12-31 23:00"), at("1969-12-31 23:30")),
      "minutes before the epoch fall in the day before it",
    ).toBe("23:00");
  });

  it("writes a year before the common era's first as Rust's `{:04}` does", () => {
    const year = (moment(1, 1, 1, 0, 0) as number) - 2 * 366 * 1440;
    expect(when(year, now)).toMatch(/^-\d{3}-12-3\d$/);
    expect(when(moment(12, 3, 4, 0, 0) as number, now)).toBe("0012-03-04");
  });
});

describe("the reader's minutes", () => {
  it("are UTC minutes rounded down where the browser's clock is UTC", () => {
    expect(localMinutes(0)).toBe(0);
    expect(localMinutes(59.9)).toBe(0);
    expect(localMinutes(60)).toBe(1);
    expect(localMinutes(-1)).toBe(-1);
    expect(localMinutes(NOW)).toBe(NOW / 60);
  });

  it("take the browser's offset at that moment, its whole minutes", () => {
    const offset = vi.spyOn(Date.prototype, "getTimezoneOffset");
    offset.mockReturnValue(-60);
    expect(localMinutes(NOW), "an hour east of UTC").toBe(NOW / 60 + 60);
    offset.mockReturnValue(300);
    expect(localMinutes(NOW)).toBe(NOW / 60 - 300);
    offset.mockReturnValue(-1.25);
    expect(localMinutes(NOW), "a local mean time's seconds dropped").toBe(NOW / 60 + 1);
    offset.mockReturnValue(Number.NaN);
    expect(localMinutes(NOW), "a date the browser cannot tell").toBe(NOW / 60);
  });
});

describe("the calendar", () => {
  it("dates every day from 1900 to 2100 as the day it was counted from", () => {
    const from = (moment(1900, 1, 1, 0, 0) as number) / 1440;
    const to = (moment(2100, 12, 31, 0, 0) as number) / 1440;
    for (let day = from; day <= to; day += 1) {
      const [year, month, date] = civil(day);
      const counted = moment(year, month, date, 0, 0);
      if (counted === null || counted / 1440 !== day) {
        throw new Error(`day ${day} is ${year}-${month}-${date}`);
      }
    }
    expect(civil(0)).toEqual([1970, 1, 1]);
    expect(civil(-1)).toEqual([1969, 12, 31]);
  });
});

describe("the gates' read", () => {
  it("is the server's answer as crates/symphony-api writes it", () => {
    expect(readTaskGates(structuredClone(fixture))).toEqual(fixture);
  });

  it("reads an answer with only its slug and branch with no tip and no gates", () => {
    const read = readTaskGates({ slug: "tools/x", branch: "tools/x" });
    expect(read?.tip).toBeUndefined();
    expect(read?.gates).toEqual([]);
  });

  it("reads a state word it does not know as unknown, and a detail left out as empty", () => {
    const read = readTaskGates({
      slug: "s",
      branch: "b",
      gates: [{ name: "later", state: "expired" }],
    });
    expect(read?.gates).toEqual([{ name: "later", state: "unknown", detail: "" }]);
  });

  it("takes none, or a field of the server's, for what it may leave out", () => {
    const read = readTaskGates({
      slug: "s",
      branch: "b",
      tip: null,
      newer: { field: true },
      gates: [
        { name: "g", state: "met", by: null, at: null, commit: null, why: null, link: null },
        { name: "h", state: "met", past: [{ any: "thing" }], extra: 1 },
      ],
    });
    expect(read?.gates[0]).toEqual({
      name: "g",
      state: "met",
      by: null,
      at: null,
      commit: null,
      why: null,
      link: null,
      detail: "",
    });
    expect(read?.gates[1]).toMatchObject({ past: [{ any: "thing" }], extra: 1 });
  });

  it.each([
    ["no object", null],
    ["a word", "gates"],
    ["a list", []],
    ["no branch", { slug: "s", gates: [] }],
    ["a branch of another type", { slug: "s", branch: 1, gates: [] }],
    ["gates of null", { slug: "s", branch: "b", gates: null }],
    ["gates of another type", { slug: "s", branch: "b", gates: { name: "g" } }],
    ["a gate of null", { slug: "s", branch: "b", gates: [null] }],
    ["a gate without a name", { slug: "s", branch: "b", gates: [{ state: "met" }] }],
    ["a name of another type", { slug: "s", branch: "b", gates: [{ name: 1, state: "met" }] }],
    ["a gate without a state", { slug: "s", branch: "b", gates: [{ name: "g" }] }],
    ["a state of null", { slug: "s", branch: "b", gates: [{ name: "g", state: null }] }],
    ["a state of another type", { slug: "s", branch: "b", gates: [{ name: "g", state: 4 }] }],
    [
      "a detail of null",
      { slug: "s", branch: "b", gates: [{ name: "g", state: "met", detail: null }] },
    ],
    [
      "a detail of another type",
      { slug: "s", branch: "b", gates: [{ name: "g", state: "met", detail: ["x"] }] },
    ],
    [
      "a who of another type",
      { slug: "s", branch: "b", gates: [{ name: "g", state: "met", by: 1 }] },
    ],
    [
      "a when of another type",
      { slug: "s", branch: "b", gates: [{ name: "g", state: "met", at: "12:05" }] },
    ],
    [
      "a commit of another type",
      { slug: "s", branch: "b", gates: [{ name: "g", state: "met", commit: 4 }] },
    ],
    [
      "a why of another type",
      { slug: "s", branch: "b", gates: [{ name: "g", state: "met", why: {} }] },
    ],
    [
      "a link of another type",
      { slug: "s", branch: "b", gates: [{ name: "g", state: "met", link: false }] },
    ],
  ])("refuses %s, as serde would", (_, value) => {
    expect(readTaskGates(value)).toBeNull();
  });
});

describe("what the server says of a task's last answer", () => {
  it("is its counts, none said of the gates it waited on or had no who and when being none", () => {
    expect(readGatesSeen({ count: 4, waiting: 1, unsigned: 2 })).toEqual({
      count: 4,
      waiting: 1,
      unsigned: 2,
    });
    expect(readGatesSeen({ count: 4, waiting: 1 }), "a page of a server one build older").toEqual({
      count: 4,
      waiting: 1,
      unsigned: 0,
    });
    expect(readGatesSeen({ count: 0 })).toEqual({ count: 0, waiting: 0, unsigned: 0 });
  });

  it.each([
    ["no object", null],
    ["no count", { waiting: 1 }],
    ["a count of null", { count: null }],
    ["a negative count", { count: -1 }],
    ["a count with a fraction", { count: 2.5 }],
    ["a count past the safe integers", { count: 2 ** 53 }],
    ["a count as a word", { count: "4" }],
    ["a waiting of null", { count: 4, waiting: null }],
    ["an unsigned of null", { count: 4, unsigned: null }],
    ["an unsigned below zero", { count: 4, unsigned: -2 }],
  ])("refuses %s, as serde would", (_, value) => {
    expect(readGatesSeen(value)).toBeNull();
  });

  it("is what an answer read says of itself", () => {
    expect(seenOf(answer([gate({ name: "g", state: "met", by: "x" })]))).toEqual({
      count: 1,
      waiting: 0,
      unsigned: 0,
    });
  });
});
