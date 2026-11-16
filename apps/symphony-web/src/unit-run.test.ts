import { describe, expect, it, vi } from "vitest";
import type { TestCase, TestModule, TestSpecification, TestSuite, Vitest } from "vitest/node";
import {
  BACKSTOP_MS,
  loadAverage,
  notRun,
  notRunLines,
  stall,
  timedOut,
  UnitRunReport,
} from "./unit-run";

const ROOT = "/checkout/apps/symphony-web";
const LOAD = [1.5, 2.25, 31] as const;
const LOAD_SAID = "load average 1.50 2.25 31.00 over 1, 5 and 15 minutes";
const TIMED_OUT = {
  message:
    'Test timed out in 5000ms.\nIf this is a long-running test, pass a timeout value as the last argument or configure it globally with "testTimeout".',
};

const at = (file: string) => `${ROOT}/${file}`;

function specification(file: string): TestSpecification {
  return { moduleId: at(file) } as unknown as TestSpecification;
}

function suite(fullName: string, errors: { message: string }[] = []): TestSuite {
  return { fullName, errors: () => errors } as unknown as TestSuite;
}

function testModule(
  file: string,
  state = "passed",
  errors: { message: string }[] = [],
  suites: TestSuite[] = [],
): TestModule {
  return {
    moduleId: at(file),
    state: () => state,
    errors: () => errors,
    children: {
      *allSuites() {
        yield* suites;
      },
    },
  } as unknown as TestModule;
}

function testCase(
  file: string,
  fullName: string,
  state: string,
  errors: { message: string }[] = [],
  duration?: number,
): TestCase {
  return {
    module: { moduleId: at(file) },
    fullName,
    result: () => ({ state, errors }),
    diagnostic: () => (duration === undefined ? undefined : { duration }),
  } as unknown as TestCase;
}

function vitestOf(root: string, logged: string[] = []): Vitest {
  return { config: { root }, logger: { error: (line: string) => logged.push(line) } } as never;
}

/** A report with its outside given: the lines it says, its fails, and a load each read takes. */
function report(loads: readonly (readonly number[])[] = [LOAD], root = ROOT) {
  const said: string[] = [];
  const fail = vi.fn();
  let read = 0;
  const load = () => loads[Math.min(read++, loads.length - 1)] ?? LOAD;
  const unit = new UnitRunReport({ load, fail, print: (line) => said.push(line) });
  unit.onInit(vitestOf(root));
  return { unit, said, fail };
}

describe("a timeout's words", () => {
  it("reads the time Vitest gave a test or a hook that ran out of it", () => {
    expect(timedOut(TIMED_OUT.message)).toBe(5000);
    expect(timedOut("Hook timed out in 10000ms.")).toBe(10000);
  });

  it("reads no other failure as one", () => {
    expect(timedOut("expected [ …(62) ] to have a length of 1 but got 62")).toBeNull();
    expect(timedOut("AssertionError: Test timed out in 5000ms")).toBeNull();
    expect(timedOut("")).toBeNull();
  });
});

describe("a stall's words", () => {
  it("give the load over 1, 5 and 15 minutes", () => {
    expect(loadAverage(LOAD)).toBe(LOAD_SAID);
    expect(loadAverage([0, 0.004, 700])).toBe(
      "load average 0.00 0.00 700.00 over 1, 5 and 15 minutes",
    );
  });

  it("say what ran out, its backstop, how long it took and the load", () => {
    expect(stall("src/a.test.ts > a test", 5000, 1_016_760, LOAD)).toBe(
      `src/a.test.ts > a test: stalled: the unit run's wall-clock backstop of 5 s ran out after 1016.8 s; ${LOAD_SAID}`,
    );
    expect(stall("src/a.test.ts", BACKSTOP_MS, 120_000, LOAD)).toBe(
      `src/a.test.ts: stalled: the unit run's wall-clock backstop of 120 s ran out after 120 s; ${LOAD_SAID}`,
    );
  });

  it("leave out the time taken where none was measured", () => {
    expect(stall("src/a.test.ts > a suite", 10_000, undefined, LOAD)).toBe(
      `src/a.test.ts > a suite: stalled: the unit run's wall-clock backstop of 10 s ran out; ${LOAD_SAID}`,
    );
  });
});

describe("the files a run did not run", () => {
  it("are those asked that did not end, in the order asked, each once", () => {
    expect(notRun(["c", "a", "b", "a"], ["b"])).toEqual(["c", "a"]);
    expect(notRun(["a", "b"], new Set(["a", "b", "z"]))).toEqual([]);
    expect(notRun([], [])).toEqual([]);
  });

  it("are said with how many were asked and the load, a line each", () => {
    expect(notRunLines(["src/queue.test.tsx"], 71, LOAD)).toEqual([
      `unit tests: 1 test file of 71 did not run or did not finish, so none of their tests is counted (no worker started them, or theirs ended first); ${LOAD_SAID}:`,
      "  src/queue.test.tsx",
    ]);
    const two = notRunLines(["src/related.test.tsx", "src/request.test.ts"], 71, LOAD);
    expect(two[0]).toMatch(/^unit tests: 2 test files of 71 did not run /);
    expect(two.slice(1)).toEqual(["  src/related.test.tsx", "  src/request.test.ts"]);
  });
});

describe("the unit run's report", () => {
  it("says nothing and fails nothing when every file it asked for ended", () => {
    const { unit, said, fail } = report();
    unit.onTestRunStart([specification("src/a.test.ts"), specification("src/b.test.tsx")]);
    unit.onTestCaseResult(testCase("src/a.test.ts", "a > passes", "passed"));
    unit.onTestCaseResult(testCase("src/a.test.ts", "a > fails", "failed", [{ message: "no" }]));
    unit.onTestRunEnd(
      [testModule("src/a.test.ts", "failed"), testModule("src/b.test.tsx", "skipped")],
      [],
      "failed",
    );
    expect(said).toEqual([]);
    expect(fail).not.toHaveBeenCalled();
  });

  it("names a file whose worker never started, which no module stands for, and fails the run", () => {
    // As on 11-16: 71 asked, queue.test.tsx's worker never answered, 70 counted.
    const { unit, said, fail } = report();
    unit.onTestRunStart([
      specification("src/board.test.tsx"),
      specification("src/queue.test.tsx"),
      specification("src/timeline.test.tsx"),
    ]);
    unit.onTestRunEnd(
      [testModule("src/board.test.tsx", "failed"), testModule("src/timeline.test.tsx", "passed")],
      [{ message: "[vitest-pool]: Failed to start forks worker" }],
      "failed",
    );
    expect(said).toEqual(notRunLines(["src/queue.test.tsx"], 3, LOAD));
    expect(fail).toHaveBeenCalledOnce();
  });

  it("names a file Vitest's error left unnamed, and one whose module never ended", () => {
    // As on 11-15: "Timeout starting forks runner." named no file.
    const { unit, said, fail } = report();
    unit.onTestRunStart(["a", "b", "c", "d"].map((name) => specification(`src/${name}.test.ts`)));
    unit.onTestRunEnd(
      [
        testModule("src/a.test.ts", "passed"),
        testModule("src/b.test.ts", "queued"),
        testModule("src/c.test.ts", "pending"),
      ],
      [{ message: "[vitest-pool]: Timeout starting forks runner." }],
      "failed",
    );
    expect(said.slice(1)).toEqual(["  src/b.test.ts", "  src/c.test.ts", "  src/d.test.ts"]);
    expect(said[0]).toMatch(/^unit tests: 3 test files of 4 did not run /);
    expect(fail).toHaveBeenCalledOnce();
  });

  it("names the files a run stopped on purpose left, and leaves its failing to Vitest", () => {
    const { unit, said, fail } = report();
    unit.onTestRunStart([specification("src/a.test.ts"), specification("src/b.test.ts")]);
    unit.onTestRunEnd([testModule("src/a.test.ts")], [], "interrupted");
    expect(said).toEqual(notRunLines(["src/b.test.ts"], 2, LOAD));
    expect(fail).not.toHaveBeenCalled();
  });

  it("says a test that ran out of its time as a stall, with the load as it was then", () => {
    // The load as the timeout was heard, then another by the run's end.
    const later = [9, 9, 9];
    const { unit, said } = report([LOAD, later]);
    unit.onTestRunStart([
      specification("src/timeline.test.tsx"),
      specification("src/queue.test.tsx"),
    ]);
    unit.onTestCaseResult(
      testCase("src/timeline.test.tsx", "reads > each 45 s", "failed", [TIMED_OUT], 1_016_760),
    );
    unit.onTestCaseResult(
      testCase("src/timeline.test.tsx", "reads > a closed task once", "failed", [
        { message: "expected [ …(62) ] to have a length of 1 but got 62" },
      ]),
    );
    expect(said).toEqual([]);
    unit.onTestRunEnd([testModule("src/timeline.test.tsx", "failed")], [], "failed");
    expect(said).toEqual([
      stall("src/timeline.test.tsx > reads > each 45 s", 5000, 1_016_760, LOAD),
      ...notRunLines(["src/queue.test.tsx"], 2, later),
    ]);
  });

  it("reads no stall into a test that failed otherwise or passed", () => {
    const { unit, said, fail } = report();
    unit.onTestRunStart([specification("src/a.test.ts")]);
    unit.onTestCaseResult(testCase("src/a.test.ts", "a > fails", "failed", [{ message: "no" }]));
    unit.onTestCaseResult(testCase("src/a.test.ts", "a > passes", "passed", [TIMED_OUT]));
    unit.onTestModuleEnd(testModule("src/a.test.ts", "failed", [{ message: "no" }]));
    unit.onTestRunEnd([testModule("src/a.test.ts", "failed")], [], "failed");
    expect(said).toEqual([]);
    expect(fail).not.toHaveBeenCalled();
  });

  it("says a hook that ran out of its time in a suite or a file, before the files not run", () => {
    const { unit, said } = report();
    unit.onTestRunStart([specification("src/a.test.ts"), specification("src/b.test.ts")]);
    const hook = { message: `Hook timed out in ${BACKSTOP_MS}ms.` };
    unit.onTestModuleEnd(
      testModule("src/a.test.ts", "failed", [hook], [suite("the feed", [hook]), suite("quiet")]),
    );
    unit.onTestRunEnd([testModule("src/a.test.ts", "failed")], [], "failed");
    expect(said).toEqual([
      stall("src/a.test.ts", BACKSTOP_MS, undefined, LOAD),
      stall("src/a.test.ts > the feed", BACKSTOP_MS, undefined, LOAD),
      ...notRunLines(["src/b.test.ts"], 2, LOAD),
    ]);
  });

  it("starts each run afresh, as a watched run starts again", () => {
    const { unit, said, fail } = report();
    unit.onTestRunStart([specification("src/a.test.ts"), specification("src/b.test.ts")]);
    unit.onTestCaseResult(testCase("src/a.test.ts", "a", "failed", [TIMED_OUT]));
    unit.onTestRunStart([specification("src/a.test.ts")]);
    unit.onTestRunEnd([testModule("src/a.test.ts")], [], "passed");
    expect(said).toEqual([]);
    expect(fail).not.toHaveBeenCalled();
  });

  it("names a file from the package's folder, and one outside it whole", () => {
    const { unit, said } = report([LOAD], `${ROOT}/`);
    unit.onTestRunStart([
      specification("src/a.test.ts"),
      { moduleId: "/elsewhere/b.test.ts" } as unknown as TestSpecification,
    ]);
    unit.onTestRunEnd([], [], "failed");
    expect(said.slice(1)).toEqual(["  src/a.test.ts", "  /elsewhere/b.test.ts"]);
  });

  it("says its lines through Vitest's logger and fails the run through Node's exit code", () => {
    const node = (
      globalThis as unknown as { process: { exitCode?: number | string | null | undefined } }
    ).process;
    const before = node.exitCode;
    const logged: string[] = [];
    const unit = new UnitRunReport();
    unit.onInit(vitestOf(ROOT, logged));
    unit.onTestRunStart([specification("src/a.test.ts")]);
    try {
      unit.onTestRunEnd([], [], "failed");
      expect(node.exitCode).toBe(1);
    } finally {
      node.exitCode = before;
    }
    expect(logged[0]).toMatch(
      /^unit tests: 1 test file of 1 did not run .*; load average \d+\.\d\d \d+\.\d\d \d+\.\d\d over 1, 5 and 15 minutes:$/,
    );
    expect(logged.slice(1)).toEqual(["  src/a.test.ts"]);
  });

  it("says nothing before Vitest gives it a logger, if no one gave it a voice", () => {
    const unit = new UnitRunReport({ load: () => LOAD, fail: () => {} });
    unit.onTestRunStart([specification("src/a.test.ts")]);
    expect(() => unit.onTestRunEnd([], [], "failed")).not.toThrow();
  });
});
