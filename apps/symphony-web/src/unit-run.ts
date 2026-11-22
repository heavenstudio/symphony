// The unit run's own report, beside Vitest's default (vite.config.ts names both). It says two
// things Vitest's summary does not.
//
// - Each test file the run asked for and did not run or finish. A file whose worker never starts
//   has no test module, so the summary leaves it out of its count. Vitest's error for it names the
//   file, unless the pool's own wait ran out first ("Timeout starting forks runner."). This report
//   names every such file, gives the load average and fails the run.
// - Each test or hook that ran out of its time, which under the run's wall-clock backstop is a
//   stall, with the load average as the timeout was heard (docs/workflow/CHECKS.md, "Time in tests").
//
// It reads a run as unsharded, as every run here is: Vitest reports a sharded run's every file
// before it keeps its shard's, so under `--shard` this report would name the other shards' files
// and fail the run.
//
// No kind imports this module, so no build carries it. Vitest's main process runs it, and Vitest
// runs its tests.

import type {
  Reporter,
  SerializedError,
  TestCase,
  TestModule,
  TestRunEndReason,
  TestSpecification,
  Vitest,
} from "vitest/node";

/** How long a test or a hook may take, in ms: a backstop only a stall reaches, the least that
 * docs/workflow/CHECKS.md ("Time in tests") gives one. Vitest's own default is 5 s per test, which load
 * alone has reached. */
export const BACKSTOP_MS = 120_000;

/** Vitest's words for a test or a hook that ran out of its time, and that time. */
const TIMED_OUT = /^(?:Test|Hook) timed out in (\d+)ms/;

/** The time Vitest gave a test or a hook that ran out of it, in ms, if `message` says so. */
export function timedOut(message: string): number | null {
  const found = TIMED_OUT.exec(message);
  return found ? Number(found[1]) : null;
}

/** The machine's load over 1, 5 and 15 minutes, as a stall gives it. */
export function loadAverage(load: readonly number[]): string {
  return `load average ${load.map((one) => one.toFixed(2)).join(" ")} over 1, 5 and 15 minutes`;
}

/** A duration in ms, in seconds. */
function seconds(ms: number): string {
  return `${Number((ms / 1000).toFixed(1))} s`;
}

/** What a test or hook that ran out of its time `ms` says, after it took `took` ms. */
export function stall(what: string, ms: number, took: number | undefined, load: readonly number[]) {
  const after = took === undefined ? "" : ` after ${seconds(took)}`;
  return `${what}: stalled: the unit run's wall-clock backstop of ${seconds(ms)} ran out${after}; ${loadAverage(load)}`;
}

/** The files of `asked` none of `finished` names, each once, in the order asked. */
export function notRun(asked: readonly string[], finished: Iterable<string>): string[] {
  const done = new Set(finished);
  return [...new Set(asked)].filter((file) => !done.has(file));
}

/** What a run says of `files` it asked for and did not run, of `of` it asked for. */
export function notRunLines(files: readonly string[], of: number, load: readonly number[]) {
  const these = files.length === 1 ? "1 test file" : `${files.length} test files`;
  return [
    `unit tests: ${these} of ${of} did not run or did not finish, so none of their tests is counted (no worker started them, or theirs ended first); ${loadAverage(load)}:`,
    ...files.map((file) => `  ${file}`),
  ];
}

/** What of Node's process the report uses. The package carries no Node types, so they are here. */
interface NodeProcess {
  exitCode?: number | string | null | undefined;
  getBuiltinModule(id: "node:os"): { loadavg(): number[] };
}

/** What the report reads and writes outside itself, which a test gives it. */
export interface Io {
  /** The machine's load average over 1, 5 and 15 minutes. */
  load(): readonly number[];
  /** Fails the run. */
  fail(): void;
  /** Says a line. Vitest's logger is used when nothing else is given. */
  print?: (line: string) => void;
}

function node(): NodeProcess {
  return (globalThis as unknown as { process: NodeProcess }).process;
}

const NODE: Io = {
  load: () => node().getBuiltinModule("node:os").loadavg(),
  fail: () => {
    node().exitCode = 1;
  },
};

/** A module's state once it has ended. "queued" and "pending" are a module's that has not. */
const ENDED = new Set(["passed", "failed", "skipped"]);

export class UnitRunReport implements Reporter {
  readonly #io: Io;
  #print: (line: string) => void;
  #root = "";
  #asked: string[] = [];
  #stalls: string[] = [];

  constructor(io: Io = NODE) {
    this.#io = io;
    this.#print = io.print ?? (() => {});
  }

  onInit(vitest: Vitest): void {
    this.#root = vitest.config.root;
    if (!this.#io.print) this.#print = (line) => vitest.logger.error(line);
  }

  onTestRunStart(specifications: ReadonlyArray<TestSpecification>): void {
    this.#asked = specifications.map((specification) => specification.moduleId);
    this.#stalls = [];
  }

  onTestCaseResult(testCase: TestCase): void {
    const result = testCase.result();
    if (result.state !== "failed") return;
    const what = `${this.#name(testCase.module.moduleId)} > ${testCase.fullName}`;
    this.#timeouts(what, result.errors, testCase.diagnostic()?.duration);
  }

  onTestModuleEnd(testModule: TestModule): void {
    const file = this.#name(testModule.moduleId);
    this.#timeouts(file, testModule.errors(), undefined);
    for (const suite of testModule.children.allSuites()) {
      this.#timeouts(`${file} > ${suite.fullName}`, suite.errors(), undefined);
    }
  }

  onTestRunEnd(
    testModules: ReadonlyArray<TestModule>,
    _unhandledErrors: ReadonlyArray<SerializedError>,
    reason: TestRunEndReason,
  ): void {
    for (const line of this.#stalls) this.#print(line);
    const finished = testModules
      .filter((testModule) => ENDED.has(testModule.state()))
      .map((testModule) => testModule.moduleId);
    const lost = notRun(this.#asked, finished);
    if (lost.length === 0) return;
    const files = lost.map((file) => this.#name(file));
    for (const line of notRunLines(files, new Set(this.#asked).size, this.#io.load())) {
      this.#print(line);
    }
    // A run stopped on purpose leaves files unrun, and Vitest says so itself.
    if (reason !== "interrupted") this.#io.fail();
  }

  /** Keeps a stall for each of `errors` that is a timeout, with the load as it was then. */
  #timeouts(what: string, errors: readonly { message: string }[], took: number | undefined) {
    for (const error of errors) {
      const ms = timedOut(error.message);
      if (ms !== null) this.#stalls.push(stall(what, ms, took, this.#io.load()));
    }
  }

  /** A test file's path from the package's folder. */
  #name(file: string): string {
    const root = this.#root.endsWith("/") ? this.#root : `${this.#root}/`;
    return this.#root && file.startsWith(root) ? file.slice(root.length) : file;
  }
}
