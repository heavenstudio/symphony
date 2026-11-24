import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  appended,
  bytesWord,
  carried,
  cpu,
  duration,
  earlierLogUrl,
  ended,
  extended,
  FILTERS_KEY,
  FiltersView,
  filterQuery,
  followList,
  followNow,
  followRun,
  followUpdates,
  freshness,
  isRunLine,
  LIST_MS,
  type ListState,
  ListView,
  listUrl,
  MACHINE_WORK_PATH,
  merged,
  NO_FILTERS,
  NOW_MS,
  type Now,
  NowView,
  nextLogUrl,
  parseFilters,
  pause,
  prepended,
  RunsPage,
  type RunTold,
  RunView,
  type RunViewProps,
  RunWaiting,
  readEarlier,
  readNow,
  readOlder,
  readRunLog,
  readRunPage,
  readRunsPage,
  readSchemaUpdates,
  refreshed,
  result,
  runHref,
  runningIds,
  type ShownLog,
  shortCommit,
  shownRun,
  stepBars,
  type Told,
  took,
  UpdatesView,
  updateRows,
  updateTimes,
  type Wait,
  when,
  wholeLines,
} from "./runs";
import { aMachine, anOperation, asking, aWork, memoryStorage, reply, server } from "./testing";
import {
  RUN_LOG_MOST,
  RUN_LOG_TAIL,
  type RunLine,
  type RunLog,
  type RunPage,
  SCHEMA_UPDATES_PATH,
  type SchemaUpdate,
  type SchemaUpdates,
  type UpdateTime,
} from "./wire";

/** Sunday 2026-11-15, 00:00 UTC. */
const TODAY = 1_794_700_800;

/** The run id `n`, 32 hex digits as the recorder writes one. */
const id = (n: number) => n.toString(16).padStart(32, "0");

/** A run's line as the server answers it: a test run of `sim/a` on its branch, started, no end. */
function aLine(n: number, fields: Partial<RunLine> = {}): RunLine {
  return {
    schema: 1,
    run: id(n),
    parent: null,
    project: null,
    machine: null,
    step: null,
    pipeline: "test",
    label: "just test-rs",
    trigger: "worker-a",
    task: "sim/a",
    handle: "worker-a",
    branch: "sim/a",
    commit: "0123456789abcdef0123456789abcdef01234567",
    tree: null,
    dirty: false,
    rerun_of: null,
    queued: TODAY + n,
    started: TODAY + n,
    ended: null,
    end: null,
    exit: null,
    signal: null,
    cpu_s: null,
    ...fields,
  };
}

/** `line` ended `end` after `seconds`. */
const endedLine = (line: RunLine, end: RunLine["end"], seconds: number): RunLine => ({
  ...line,
  ended: (line.started ?? line.queued) + seconds,
  end,
  exit: end === "green" ? 0 : 1,
});

const listOf = (runs: RunLine[], fields: Partial<ListState> = {}): ListState => ({
  runs,
  more: false,
  pipelines: ["test"],
  unread: [],
  ...fields,
});

const aLog = (from: number, to: number, size: number, text: string): RunLog => ({
  run: id(1),
  log: "logs/2026-11-15/x.log",
  size,
  from,
  to,
  text,
});

/** A wait that records each pause and ends `page` after `pauses` of them. */
function waits(page: AbortController, pauses: number): { waited: number[]; wait: Wait } {
  const waited: number[] = [];
  return {
    waited,
    wait: async (ms, signal) => {
      expect(signal).toBe(page.signal);
      waited.push(ms);
      if (waited.length >= pauses) page.abort();
    },
  };
}

/** What a follower told, in order. */
function telling<T>(): { told: unknown[]; check: Told<T> } {
  const told: unknown[] = [];
  return {
    told,
    check: {
      answered: (read) => told.push(["answered", read]),
      failed: (why, missing) => told.push(["failed", why, missing]),
    },
  };
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

describe("a run's line", () => {
  it("says how it ended, else that it runs while the machine's work has it, else started", () => {
    const line = aLine(1);
    expect(result(line, new Set())).toBe("started");
    expect(result(line, new Set([id(1)]))).toBe("running");
    expect(result(endedLine(line, "red", 3), new Set([id(1)]))).toBe("red");
  });

  it("is read only with every field the page draws, each of its type", () => {
    const line = aLine(1, {
      cpu_s: 2,
      cpu_partial: true,
      log: "logs/x.log",
      log_bytes: 4,
      suites: [{ name: "a", passed: 1, failed: 0, ignored: 0, s: 1 }],
      failed: [{ suite: null, name: "t", line: "boom" }],
    });
    expect(isRunLine(line)).toBe(true);
    expect(isRunLine(aLine(2))).toBe(true);
    expect(isRunLine({ ...line, end: "blue" })).toBe(false);
    expect(isRunLine({ ...line, queued: "now" })).toBe(false);
    expect(isRunLine({ ...line, queued: Number.NaN })).toBe(false);
    expect(isRunLine({ ...line, task: 3 })).toBe(false);
    expect(isRunLine({ ...line, dirty: "yes" })).toBe(false);
    expect(isRunLine({ ...line, cpu_partial: 1 })).toBe(false);
    expect(isRunLine({ ...line, suites: [{ name: "a" }] })).toBe(false);
    const crashed = { name: "a", passed: 0, failed: 0, ignored: 0, s: 1, error: true };
    expect(isRunLine({ ...line, suites: [crashed] })).toBe(true);
    expect(isRunLine({ ...line, suites: [{ ...crashed, error: "yes" }] })).toBe(false);
    expect(isRunLine({ ...line, failed: "t" })).toBe(false);
    expect(isRunLine({ ...line, failed: [{ name: "t" }] })).toBe(false);
    const { label: _, ...unlabelled } = line;
    expect(isRunLine(unlabelled)).toBe(false);
    expect(isRunLine(null)).toBe(false);
  });

  it("takes its time from its start to its end, or to now while it runs", () => {
    expect(took(aLine(1, { started: null }), TODAY)).toBeNull();
    expect(took(endedLine(aLine(1), "green", 90), TODAY + 1_000)).toBe(90);
    expect(took(aLine(0), TODAY + 30)).toBe(30);
    expect(took(aLine(10), TODAY)).toBe(0);
  });

  it("says its CPU, at least where the recorder knows it is short", () => {
    expect(cpu(aLine(1))).toBeNull();
    expect(cpu(aLine(1, { cpu_s: 125 }))).toBe("2 min 5 s of CPU");
    expect(cpu(aLine(1, { cpu_s: 12, cpu_partial: true }))).toBe("at least 12 s of CPU");
  });

  it("names its commit short", () => {
    expect(shortCommit(null)).toBeNull();
    expect(shortCommit("0123456789abcdef")).toBe("0123456");
  });
});

describe("the words", () => {
  it("say a duration in its largest units", () => {
    expect(duration(-4)).toBe("0 s");
    expect(duration(42.4)).toBe("42 s");
    expect(duration(59.6)).toBe("1 min");
    expect(duration(200)).toBe("3 min 20 s");
    expect(duration(540)).toBe("9 min");
    expect(duration(1_440)).toBe("24 min");
    expect(duration(3_900)).toBe("1 h 05 min");
    expect(duration(36_000)).toBe("10 h 00 min");
  });

  it("say a time in UTC, its day and month by name", () => {
    expect(when(TODAY)).toBe("Sun 15 Nov, 00:00 UTC");
    expect(when(TODAY + 2 * 3_600 + 47 * 60 + 59)).toBe("Sun 15 Nov, 02:47 UTC");
  });

  it("say a size in bytes, kilobytes or megabytes", () => {
    expect(bytesWord(812)).toBe("812 B");
    expect(bytesWord(4_200)).toBe("4.2 KB");
    expect(bytesWord(1_300_000)).toBe("1.3 MB");
  });

  it("say a machine's freshness, an unknown word as not known", () => {
    expect(freshness("fresh")).toBe("up to date");
    expect(freshness("disconnected")).toBe("not heard from");
    expect(freshness("unknown")).toBe("not known");
    expect(freshness("anything")).toBe("not known");
  });
});

describe("the answers", () => {
  it("read a page of runs, refusing one without what the page draws", () => {
    const page = { runs: [aLine(1)], more: true, pipelines: ["test"], unread: ["x: why"] };
    expect(readRunsPage(page)).toEqual(page);
    expect(readRunsPage({ ...page, runs: [{ run: id(1) }] })).toBeNull();
    expect(readRunsPage({ ...page, more: "yes" })).toBeNull();
    expect(readRunsPage({ ...page, pipelines: [1] })).toBeNull();
    expect(readRunsPage({ ...page, unread: null })).toBeNull();
    expect(readRunsPage([])).toBeNull();
  });

  it("read a run and its steps", () => {
    const page = { run: aLine(1), steps: [aLine(2, { parent: id(1) })], unread: [] };
    expect(readRunPage(page)).toEqual(page);
    expect(readRunPage({ ...page, run: null })).toBeNull();
    expect(readRunPage({ ...page, steps: [1] })).toBeNull();
    expect(readRunPage({ ...page, steps: {} })).toBeNull();
    expect(readRunPage({ ...page, unread: "x" })).toBeNull();
  });

  it("read part of a log, its names optional", () => {
    expect(readRunLog(aLog(0, 3, 3, "abc"))).toEqual(aLog(0, 3, 3, "abc"));
    expect(readRunLog({ size: 3, from: 0, to: 3, text: "abc" })).toEqual({
      run: "",
      log: "",
      size: 3,
      from: 0,
      to: 3,
      text: "abc",
    });
    expect(readRunLog({ size: 3, from: 0, text: "abc" })).toBeNull();
    expect(readRunLog({ size: 3, from: 0, to: 3, text: null })).toBeNull();
  });

  it("read what runs now: each machine's word and each operation, a machine's or none's", () => {
    const work = aWork(
      [
        aMachine(
          "desk",
          [
            anOperation("desk", id(1), "test", "running", { phase_since: TODAY }),
            anOperation("desk", null, "build", "queued", { task: null, handle: null }),
          ],
          { age_ms: 1_500 },
        ),
        aMachine("laptop", [], { age_ms: null, freshness: "stale" }),
      ],
      { unassigned: [anOperation(null, id(3), "command", "queued")] },
    );
    const now = readNow(work);
    expect(now?.machines).toEqual([
      { label: "desk", freshness: "fresh", ageMs: 1_500 },
      { label: "laptop", freshness: "stale", ageMs: null },
    ]);
    expect(now?.operations).toEqual([
      {
        machine: "desk",
        id: id(1),
        state: "running",
        recipe: "fixture",
        task: "sim/a",
        handle: "worker-a",
        since: TODAY,
      },
      {
        machine: "desk",
        id: null,
        state: "queued",
        recipe: "fixture",
        task: null,
        handle: null,
        since: null,
      },
      {
        machine: "",
        id: id(3),
        state: "queued",
        recipe: "fixture",
        task: "sim/a",
        handle: "worker-a",
        since: null,
      },
    ]);
    expect([...runningIds(now)]).toEqual([id(1)]);
    expect(runningIds(null).size).toBe(0);
  });

  it("refuse machine work without what the page draws", () => {
    const one = (operation: unknown, fields: Record<string, unknown> = {}) =>
      readNow({
        machines: [{ ...aMachine("desk", []), operations: [operation], ...fields }],
        unassigned: [],
      });
    const operation = anOperation("desk", id(1), "test", "running");
    expect(one(operation)).not.toBeNull();
    expect(one({ ...operation, state: "done" })).toBeNull();
    expect(one({ ...operation, recipe: null })).toBeNull();
    expect(one({ ...operation, phase_since: "then" })).toBeNull();
    expect(one({ ...operation, task: 4 })).toBeNull();
    expect(one(operation, { label: null })).toBeNull();
    expect(one(operation, { age_ms: "1" })).toBeNull();
    expect(one(operation, { operations: null })).toBeNull();
    expect(readNow({ machines: [], unassigned: [{ state: "running" }] })).toBeNull();
    expect(readNow({ machines: [] })).toBeNull();
    expect(readNow(null)).toBeNull();
  });
});

describe("the filters", () => {
  it("are a query, the empty ones left out, and read back from one", () => {
    const filters = { pipeline: "test", end: "red", task: " sim/a ", branch: "" };
    expect(filterQuery(filters)).toBe("pipeline=test&end=red&task=sim%2Fa");
    expect(parseFilters(filterQuery(filters))).toEqual({ ...filters, task: "sim/a" });
    expect(filterQuery(NO_FILTERS)).toBe("");
    expect(parseFilters("")).toEqual(NO_FILTERS);
  });

  it("ask for the list, its next page before a time", () => {
    expect(listUrl(NO_FILTERS)).toBe("/api/runs");
    expect(listUrl({ ...NO_FILTERS, end: "started" })).toBe("/api/runs?end=started");
    expect(listUrl({ ...NO_FILTERS, branch: "main" }, 12.5)).toBe(
      "/api/runs?branch=main&before=12.5",
    );
    expect(listUrl(NO_FILTERS, TODAY)).toBe(`/api/runs?before=${TODAY}`);
  });

  it("name a run's page by the address the shell routes, and read it back", () => {
    expect(runHref(id(7))).toBe(`/runs?run=${id(7)}`);
    expect(shownRun(`?run=${id(7)}`)).toBe(id(7));
    expect(shownRun("?run=")).toBeNull();
    expect(shownRun("")).toBeNull();
  });
});

describe("the list", () => {
  it("takes its first page again over the older runs a reader loaded, each once", () => {
    const fresh = [aLine(30), aLine(20)];
    expect(
      merged(fresh, [aLine(25), aLine(20), aLine(10), aLine(5)]).map((line) => line.queued),
    ).toEqual([TODAY + 30, TODAY + 20, TODAY + 10, TODAY + 5]);
    expect(merged([], [aLine(1)])).toEqual([]);
    // A run asked for in the same second as the page's last, which the page cut through, stays.
    const tied = aLine(21, { queued: TODAY + 20 });
    expect(merged(fresh, [aLine(30), aLine(20), tied, aLine(10)]).map((line) => line.run)).toEqual([
      id(30),
      id(20),
      id(21),
      id(10),
    ]);
  });

  it("keeps Load more and what was not read while older runs stay, else takes the page's", () => {
    const page = { runs: [aLine(30), aLine(20)], more: true, pipelines: ["test"], unread: ["b"] };
    expect(refreshed(null, page)).toEqual(listOf(page.runs, { more: true, unread: ["b"] }));
    const loaded = listOf([aLine(30), aLine(20), aLine(10)], {
      pipelines: ["lint"],
      unread: ["a"],
    });
    expect(refreshed(loaded, page)).toEqual(
      listOf([aLine(30), aLine(20), aLine(10)], {
        pipelines: ["lint", "test"],
        unread: ["a", "b"],
      }),
    );
    expect(refreshed(listOf([aLine(20)], { more: false }), page).more).toBe(true);
  });

  it("takes the page below its last run after it, a run it already shows once", () => {
    const shown = listOf([aLine(30), aLine(20)], { more: true, unread: ["a"] });
    const older = {
      runs: [aLine(20), aLine(10)],
      more: false,
      pipelines: ["build"],
      unread: ["a", "c"],
    };
    expect(extended(shown, older)).toEqual(
      listOf([aLine(30), aLine(20), aLine(10)], {
        more: false,
        pipelines: ["build", "test"],
        unread: ["a", "c"],
      }),
    );
  });
});

describe("a run's steps", () => {
  it("are bars on the run's span, each as deep as it ran and at least a sliver", () => {
    const run = endedLine(aLine(0), "red", 100);
    const build = endedLine(
      aLine(1, { parent: id(0), step: "build", started: TODAY }),
      "green",
      40,
    );
    const unit = endedLine(
      aLine(2, { parent: id(1), step: "unit", started: TODAY + 10 }),
      "red",
      20,
    );
    const never = aLine(3, {
      parent: id(0),
      step: "docs",
      started: null,
      queued: TODAY + 50,
      end: "interrupted",
    });
    const bars = stepBars(run, [build, unit, never], TODAY + 500);
    expect(bars.map((bar) => [bar.line.step, bar.depth, bar.left, bar.width])).toEqual([
      ["build", 1, 0, 40],
      ["unit", 2, 10, 20],
      ["docs", 1, 50, 0.5],
    ]);
  });

  it("run to now while the run goes on, and a run of no time is a span of a second", () => {
    const run = aLine(0);
    const going = aLine(5, { parent: id(0), step: "test", started: TODAY + 5 });
    expect(stepBars(run, [going], TODAY + 10).map((bar) => [bar.left, bar.width])).toEqual([
      [50, 50],
    ]);
    const instant = endedLine(aLine(0), "green", 0);
    const orphan = endedLine(aLine(1, { parent: id(9), started: TODAY }), "green", 0);
    expect(stepBars(instant, [orphan], TODAY + 100).map((bar) => [bar.depth, bar.width])).toEqual([
      [1, 0.5],
    ]);
  });

  it("have ended once the run and every step of it have", () => {
    const run = endedLine(aLine(0), "red", 10);
    const step = aLine(1, { parent: id(0) });
    expect(ended({ run, steps: [step], unread: [] })).toBe(false);
    expect(ended({ run, steps: [endedLine(step, "red", 2)], unread: [] })).toBe(true);
    expect(ended({ run: aLine(0), steps: [], unread: [] })).toBe(false);
  });

  it("carry their failed tests and suites keyed by the line and the place in it", () => {
    const one = aLine(1, {
      failed: [
        { suite: "a", name: "t", line: "x" },
        { suite: "a", name: "t", line: "y" },
      ],
    });
    const two = aLine(2, { failed: null });
    const three = aLine(3, { failed: [{ suite: null, name: "t", line: "z" }] });
    expect(
      carried([one, two, three], (line) => line.failed).map(([key, test]) => [key, test.line]),
    ).toEqual([
      [`${id(1)}/0`, "x"],
      [`${id(1)}/1`, "y"],
      [`${id(3)}/0`, "z"],
    ]);
  });
});

describe("a run's log", () => {
  const shown: ShownLog = { size: 100, from: 40, to: 100, text: "tail" };

  it("is asked for from its tail, then on from what is shown, then before it", () => {
    expect(nextLogUrl(id(1), null)).toBe(`/api/run-log?run=${id(1)}&tail=${RUN_LOG_TAIL}`);
    expect(nextLogUrl(id(1), shown)).toBe(`/api/run-log?run=${id(1)}&from=100`);
    expect(earlierLogUrl(id(1), shown)).toBe(`/api/run-log?run=${id(1)}&from=0&to=40`);
    const far = { ...shown, from: RUN_LOG_MOST + 10 };
    expect(earlierLogUrl(id(1), far)).toBe(
      `/api/run-log?run=${id(1)}&from=10&to=${RUN_LOG_MOST + 10}`,
    );
  });

  it("takes on what was written since, and is what was read once it was written again", () => {
    expect(appended(null, aLog(40, 100, 100, "tail"))).toEqual(aLog(40, 100, 100, "tail"));
    expect(appended(shown, aLog(100, 120, 120, " more"))).toEqual({
      size: 120,
      from: 40,
      to: 120,
      text: "tail more",
    });
    expect(appended(shown, aLog(0, 30, 30, "new"))).toEqual(aLog(0, 30, 30, "new"));
    expect(appended(shown, aLog(90, 120, 120, "gap"))).toEqual(aLog(90, 120, 120, "gap"));
  });

  it("draws whole lines: a tail less the line its first byte may cut, a whole log as it is", () => {
    expect(wholeLines({ size: 20, from: 4, to: 20, text: "es/crate)\nnext\nlast" })).toBe(
      "next\nlast",
    );
    expect(wholeLines({ size: 20, from: 4, to: 20, text: "one long line" })).toBe("one long line");
    expect(wholeLines({ size: 9, from: 0, to: 9, text: "a\nb" })).toBe("a\nb");
  });

  it("puts the part just before it in front, and no other", () => {
    expect(prepended(shown, aLog(0, 40, 100, "head "))).toEqual({
      size: 100,
      from: 0,
      to: 100,
      text: "head tail",
    });
    expect(prepended(shown, aLog(0, 30, 100, "other"))).toBe(shown);
  });
});

describe("the wait between reads", () => {
  /** A page whose visibility a test sets, and says changed. */
  function aPage(hidden: boolean) {
    const events = new EventTarget();
    const page = {
      hidden,
      addEventListener: events.addEventListener.bind(events),
      removeEventListener: events.removeEventListener.bind(events),
      shows() {
        page.hidden = false;
        events.dispatchEvent(new Event("visibilitychange"));
      },
    };
    vi.stubGlobal("document", page);
    return page;
  }

  it("is its time while the page is shown, then on only once a hidden page shows", async () => {
    vi.useFakeTimers();
    const page = aPage(false);
    const reading = new AbortController();
    const over: string[] = [];
    const shown = pause(NOW_MS, reading.signal).then(() => over.push("shown"));
    await vi.advanceTimersByTimeAsync(NOW_MS - 1);
    expect(over).toEqual([]);
    await vi.advanceTimersByTimeAsync(1);
    await shown;
    // Hidden, no time is enough: it goes on as the page shows again.
    page.hidden = true;
    const hidden = pause(NOW_MS, reading.signal).then(() => over.push("hidden"));
    await vi.advanceTimersByTimeAsync(NOW_MS * 100);
    expect(over).toEqual(["shown"]);
    page.shows();
    await hidden;
    expect(over).toEqual(["shown", "hidden"]);
  });

  it("ends at once with its signal, hidden or not", async () => {
    vi.useFakeTimers();
    aPage(true);
    const leaving = new AbortController();
    const waiting = pause(NOW_MS, leaving.signal);
    await vi.advanceTimersByTimeAsync(NOW_MS);
    leaving.abort();
    await waiting;
    await pause(NOW_MS, leaving.signal);
  });
});

describe("the reads", () => {
  it("read what runs now at once and on its timer, saying a failure and going on", async () => {
    const asked = server();
    const page = new AbortController();
    const { waited, wait } = waits(page, 3);
    const { told, check } = telling<Now>();
    const done = followNow(page.signal, check, wait);
    await asking(asked, 1);
    expect(asked[0]?.url).toBe(MACHINE_WORK_PATH);
    expect(asked[0]?.signal).not.toBeNull();
    asked[0]?.answer(reply(200, aWork([])));
    await asking(asked, 2);
    asked[1]?.answer(reply(503, { error: "the board is down" }));
    await asking(asked, 3);
    asked[2]?.answer(reply(200, { machines: "none" }));
    await done;
    expect(told).toEqual([
      ["answered", { machines: [], operations: [] }],
      ["failed", "the board is down (the server answered 503). Trying again…", false],
      ["failed", "The machine work's answer is not machine work. Trying again…", false],
    ]);
    expect(waited).toEqual([NOW_MS, NOW_MS, NOW_MS]);
  });

  it("read the list's first page naming the last tag, until a 404, said as missing and not tried again", async () => {
    const asked = server();
    const page = new AbortController();
    const { waited, wait } = waits(page, 4);
    const { told, check } = telling<unknown>();
    const filters = { ...NO_FILTERS, pipeline: "test" };
    const done = followList(filters, page.signal, check, wait);
    const list = { runs: [aLine(1)], more: false, pipelines: ["test"], unread: [] };
    await asking(asked, 1);
    expect(asked[0]?.url).toBe("/api/runs?pipeline=test");
    asked[0]?.answer(reply(200, list, "t1"));
    await asking(asked, 2);
    asked[1]?.answer(new Response(null, { status: 304 }));
    await asking(asked, 3);
    asked[2]?.answer(reply(200, { runs: "none" }, "t2"));
    await asking(asked, 4);
    asked[3]?.answer(reply(404, { error: "this project keeps no runs where this server reads" }));
    // The server has no such thing: the read ends there, with no wait and no read after it.
    await done;
    expect(told).toEqual([
      ["answered", list],
      ["answered", null],
      ["failed", "The runs' answer is not a page of runs. Trying again…", false],
      [
        "failed",
        "this project keeps no runs where this server reads (the server answered 404).",
        true,
      ],
    ]);
    expect(waited).toEqual([LIST_MS, LIST_MS, LIST_MS]);
    expect(asked).toHaveLength(4);
    expect(page.signal.aborted).toBe(false);
  });

  it("read what runs now no more once the server has no such thing", async () => {
    const asked = server();
    const page = new AbortController();
    const { waited, wait } = waits(page, 2);
    const { told, check } = telling<Now>();
    const done = followNow(page.signal, check, wait);
    await asking(asked, 1);
    asked[0]?.answer(reply(404, { error: "no machine work here" }));
    await done;
    expect(told).toEqual([["failed", "no machine work here (the server answered 404).", true]]);
    expect([waited, asked.length, page.signal.aborted]).toEqual([[], 1, false]);
  });

  it("stop telling anything once the page that asked is gone", async () => {
    const asked = server();
    const page = new AbortController();
    const { told, check } = telling<unknown>();
    const list = followList(NO_FILTERS, page.signal, check, async () => {});
    const now = followNow(page.signal, check, async () => {});
    await asking(asked, 2);
    page.abort();
    expect(asked.every((one) => one.signal?.aborted)).toBe(true);
    asked[0]?.answer(reply(200, { runs: [], more: false, pipelines: [], unread: [] }));
    asked[1]?.answer(reply(503, {}));
    await Promise.all([list, now]);
    expect(told).toEqual([]);
  });

  it("read an older page and the log before what is shown, under the signal given", async () => {
    const asked = server();
    const mount = new AbortController();
    const older = readOlder({ ...NO_FILTERS, task: "sim/a" }, TODAY, mount.signal);
    await asking(asked, 1);
    expect(asked[0]?.url).toBe(`/api/runs?task=sim%2Fa&before=${TODAY}`);
    expect(asked[0]?.signal).not.toBeNull();
    asked[0]?.answer(reply(200, { runs: [], more: false, pipelines: [], unread: [] }));
    expect(await older).toEqual({ runs: [], more: false, pipelines: [], unread: [] });
    const bad = readOlder(NO_FILTERS, TODAY, mount.signal);
    await asking(asked, 2);
    asked[1]?.answer(reply(200, {}));
    await expect(bad).rejects.toThrow("not a page of runs");
    const log = { size: 100, from: 40, to: 100, text: "tail" };
    const earlier = readEarlier(id(1), log, mount.signal);
    await asking(asked, 3);
    expect(asked[2]?.url).toBe(`/api/run-log?run=${id(1)}&from=0&to=40`);
    asked[2]?.answer(reply(200, aLog(0, 40, 100, "head ")));
    expect(await earlier).toEqual(aLog(0, 40, 100, "head "));
    const unreadable = readEarlier(id(1), log, mount.signal);
    await asking(asked, 4);
    asked[3]?.answer(reply(200, { text: 1 }));
    await expect(unreadable).rejects.toThrow("not a log");
  });
});

describe("a run's read", () => {
  /** A run's read, what it told and what it waited, the log it shows kept as the page keeps it. */
  function follow(pauses: number) {
    const asked = server();
    const page = new AbortController();
    const { waited, wait } = waits(page, pauses);
    const told: unknown[] = [];
    let log: ShownLog | null = null;
    const check: RunTold = {
      answered: (read) => told.push(["answered", read === null ? null : read.run.run]),
      failed: (why, missing) => told.push(["failed", why, missing]),
      log: (next) => {
        log = next;
        told.push(["log", next.text]);
      },
      logFailed: (why) => told.push(["log failed", why]),
      logMissing: () => told.push(["log missing"]),
    };
    const done = followRun(id(1), () => log, page.signal, check, wait);
    return { asked, told, waited, done };
  }
  const running: RunPage = { run: aLine(1, { log: "logs/x.log" }), steps: [], unread: [] };
  const over: RunPage = { ...running, run: endedLine(running.run, "green", 5) };

  it("reads the log on while the run goes on, and stops once it has ended and its log is read", async () => {
    const { asked, told, waited, done } = follow(10);
    await asking(asked, 1);
    expect(asked[0]?.url).toBe(`/api/run?run=${id(1)}`);
    asked[0]?.answer(reply(200, running, "r1"));
    await asking(asked, 2);
    expect(asked[1]?.url).toBe(`/api/run-log?run=${id(1)}&tail=${RUN_LOG_TAIL}`);
    asked[1]?.answer(reply(200, aLog(0, 3, 3, "one")));
    await asking(asked, 3);
    asked[2]?.answer(new Response(null, { status: 304 }));
    await asking(asked, 4);
    // A 304 of a run that goes on still reads its log on from what is shown.
    expect(asked[3]?.url).toBe(`/api/run-log?run=${id(1)}&from=3`);
    asked[3]?.answer(reply(503, {}));
    await asking(asked, 5);
    asked[4]?.answer(reply(200, over, "r2"));
    await asking(asked, 6);
    expect(asked[5]?.url).toBe(`/api/run-log?run=${id(1)}&from=3`);
    asked[5]?.answer(reply(200, aLog(3, 7, 7, " two")));
    await done;
    expect(told).toEqual([
      ["answered", id(1)],
      ["log", "one"],
      ["answered", null],
      ["log failed", "The server answered 503. Trying again…"],
      ["answered", id(1)],
      ["log", "one two"],
    ]);
    expect(waited).toEqual([NOW_MS, NOW_MS]);
  });

  it("reads again a run that ended while its log's last read failed, and a log written again from its tail", async () => {
    const { asked, told, done } = follow(10);
    await asking(asked, 1);
    asked[0]?.answer(reply(200, running, "r1"));
    await asking(asked, 2);
    asked[1]?.answer(reply(200, aLog(0, 9, 9, "long line")));
    await asking(asked, 3);
    asked[2]?.answer(reply(200, over, "r2"));
    await asking(asked, 4);
    asked[3]?.answer(reply(200, aLog(4, 4, 4, "")));
    await asking(asked, 5);
    expect(asked[4]?.url).toBe(`/api/run-log?run=${id(1)}&tail=${RUN_LOG_TAIL}`);
    asked[4]?.answer(reply(200, { text: 3 }));
    await asking(asked, 6);
    asked[5]?.answer(new Response(null, { status: 304 }));
    await asking(asked, 7);
    // Each read asks on from what is shown, and from the tail once that is past the log's end.
    expect(asked[6]?.url).toBe(`/api/run-log?run=${id(1)}&from=9`);
    asked[6]?.answer(reply(200, aLog(4, 4, 4, "")));
    await asking(asked, 8);
    expect(asked[7]?.url).toBe(`/api/run-log?run=${id(1)}&tail=${RUN_LOG_TAIL}`);
    asked[7]?.answer(reply(200, aLog(0, 4, 4, "anew")));
    await done;
    expect(told.slice(2)).toEqual([
      ["answered", id(1)],
      ["log failed", "The log's answer is not a log. Trying again…"],
      ["answered", null],
      ["log", "anew"],
    ]);
  });

  it("keeps the run it read when a later answer is no run, reading its log on and stopping once it is read", async () => {
    const { asked, told, waited, done } = follow(5);
    await asking(asked, 1);
    asked[0]?.answer(reply(200, over, "r1"));
    await asking(asked, 2);
    asked[1]?.answer(reply(503, {}));
    await asking(asked, 3);
    expect([asked[2]?.url, asked[2]?.named]).toEqual([`/api/run?run=${id(1)}`, "r1"]);
    asked[2]?.answer(reply(200, { run: null }, "r2"));
    await asking(asked, 4);
    // The run read before still stands: its log is read, and the read stops once it is.
    expect(asked[3]?.url).toBe(`/api/run-log?run=${id(1)}&tail=${RUN_LOG_TAIL}`);
    asked[3]?.answer(reply(200, aLog(0, 3, 3, "one")));
    await done;
    expect(told).toEqual([
      ["answered", id(1)],
      ["log failed", "The server answered 503. Trying again…"],
      ["failed", "The run's answer is not a run. Trying again…", false],
      ["log", "one"],
    ]);
    expect(waited).toEqual([NOW_MS]);
  });

  it("stops on a run the server has none of, and reads no log of a run that kept none", async () => {
    const gone = follow(5);
    await asking(gone.asked, 1);
    gone.asked[0]?.answer(reply(404, { error: "no run x in the last 14 days' files" }));
    await gone.done;
    expect(gone.told).toEqual([
      ["failed", "no run x in the last 14 days' files (the server answered 404).", true],
    ]);
    const quiet = follow(5);
    await asking(quiet.asked, 1);
    quiet.asked[0]?.answer(reply(200, { ...over, run: { ...over.run, log: null } }, "r"));
    await quiet.done;
    expect(quiet.told).toEqual([["answered", id(1)]]);
    expect(quiet.asked).toHaveLength(1);
  });

  it("stops on an ended run whose log the server keeps no more, and reads on a running one's", async () => {
    const gone = follow(5);
    await asking(gone.asked, 1);
    gone.asked[0]?.answer(reply(200, over, "r"));
    await asking(gone.asked, 2);
    gone.asked[1]?.answer(reply(404, { error: "no log of the run" }));
    await gone.done;
    expect(gone.told).toEqual([["answered", id(1)], ["log missing"]]);
    expect(gone.asked).toHaveLength(2);
    expect(gone.waited).toEqual([]);
    // A running run's log not written yet is read again on the timer.
    const early = follow(1);
    await asking(early.asked, 1);
    early.asked[0]?.answer(reply(200, running, "r"));
    await asking(early.asked, 2);
    early.asked[1]?.answer(reply(404, { error: "no log of the run" }));
    await early.done;
    expect(early.told).toEqual([["answered", id(1)], ["log missing"]]);
    expect(early.waited).toEqual([NOW_MS]);
  });

  it("goes on after a failed read, and stops at once once the page is gone", async () => {
    const failing = follow(1);
    await asking(failing.asked, 1);
    failing.asked[0]?.answer(reply(200, { run: null }));
    await failing.done;
    expect(failing.told).toEqual([
      ["failed", "The run's answer is not a run. Trying again…", false],
    ]);
    const leaving = follow(5);
    await asking(leaving.asked, 1);
    leaving.asked[0]?.answer(reply(200, running, "r"));
    await asking(leaving.asked, 2);
    leaving.asked[1]?.answer(reply(200, aLog(0, 3, 3, "one")));
    await asking(leaving.asked, 3);
    expect(leaving.waited).toHaveLength(1);
  });
});

describe("the views", () => {
  it("say what runs now: each machine, the running first, and nothing when nothing runs", () => {
    const now: Now = {
      machines: [{ label: "desk", freshness: "stale", ageMs: 90_000 }],
      operations: [
        {
          machine: "desk",
          id: null,
          state: "queued",
          recipe: "lint",
          task: null,
          handle: null,
          since: null,
        },
        {
          machine: "desk",
          id: id(1),
          state: "running",
          recipe: "test",
          task: "sim/a",
          handle: "w",
          since: TODAY,
        },
      ],
    };
    const html = renderToStaticMarkup(<NowView now={now} error={null} at={TODAY + 75} />);
    expect(html).toContain("<strong>desk</strong> · late · last word 1 min 30 s ago");
    expect(html.indexOf('data-state="running"')).toBeLessThan(html.indexOf('data-state="queued"'));
    expect(html).toContain("sim/a · w");
    expect(html).toContain("1 min 15 s");
    const quiet = renderToStaticMarkup(
      <NowView now={{ machines: [], operations: [] }} error={null} at={TODAY} />,
    );
    expect(quiet).toContain("Nothing runs or waits for this project now.");
    const failing = renderToStaticMarkup(<NowView now={null} error="Gone." at={TODAY} />);
    expect(failing).toContain("What runs now could not be read: Gone.");
    expect(failing).not.toContain("Reading what runs now");
    expect(renderToStaticMarkup(<NowView now={null} error={null} at={TODAY} />)).toContain(
      "Reading what runs now…",
    );
  });

  it("draw the list a line a run, linked to its page, with Load more and the files not read", () => {
    const red = endedLine(
      aLine(1, { failed: [{ suite: null, name: "a", line: "x" }], task: null, commit: null }),
      "red",
      65,
    );
    const list = listOf([aLine(2), red], { more: true, unread: ["day: why"] });
    const html = renderToStaticMarkup(
      <ListView
        list={list}
        error={null}
        running={new Set([id(2)])}
        at={TODAY + 12}
        loadingMore={false}
        onMore={() => {}}
      />,
    );
    expect(html).toContain(`href="/runs?run=${id(2)}"`);
    expect(html).toContain('data-result="running"');
    expect(html).toContain('data-result="red"');
    expect(html).toContain("1 min 5 s");
    expect(html).toContain("sim/a · 0123456");
    expect(html).toContain("1 failed");
    expect(html).toContain("1 day file not read: their runs are missing");
    expect(html).toContain("Load more");
    const loading = renderToStaticMarkup(
      <ListView
        list={list}
        error={null}
        running={new Set()}
        at={TODAY}
        loadingMore
        onMore={() => {}}
      />,
    );
    expect(loading).toContain("Loading…");
    const two = renderToStaticMarkup(
      <ListView
        list={listOf([], { unread: ["a", "b"] })}
        error={null}
        running={new Set()}
        at={TODAY}
        loadingMore={false}
        onMore={() => {}}
      />,
    );
    expect(two).toContain("2 day files not read");
    expect(two).toContain("No run in the last 14 days matches.");
    expect(two).not.toContain("Load more");
  });

  it("draw ghosts while the list is first read, and nothing of it once its read failed", () => {
    const props = { running: new Set<string>(), at: TODAY, loadingMore: false, onMore: () => {} };
    expect(renderToStaticMarkup(<ListView list={null} error={null} {...props} />)).toContain(
      "skeleton",
    );
    expect(renderToStaticMarkup(<ListView list={null} error="x" {...props} />)).toBe("");
  });

  it("offer the pipelines read, a chosen one not among them too, and Clear once one is chosen", () => {
    const none = renderToStaticMarkup(
      <FiltersView filters={NO_FILTERS} pipelines={["lint", "test"]} onAsk={() => {}} />,
    );
    expect(none).toContain('<option value="lint">lint</option>');
    expect(none).toContain('<option value="started">not ended</option>');
    expect(none).toContain('<option value="timed_out">timed out</option>');
    expect(none).not.toContain("Clear");
    const chosen = renderToStaticMarkup(
      <FiltersView
        filters={{ ...NO_FILTERS, pipeline: "release", task: "sim/a" }}
        pipelines={["test"]}
        onAsk={() => {}}
      />,
    );
    expect(chosen).toContain('<option value="release" selected="">release</option>');
    expect(chosen).toContain('value="sim/a"');
    expect(chosen).toContain("Clear");
  });
});

describe("a run's page", () => {
  const steps = [
    endedLine(aLine(2, { parent: id(1), step: "build", started: TODAY }), "green", 30),
    endedLine(
      aLine(3, {
        parent: id(1),
        step: "unit",
        started: TODAY + 30,
        failed: [
          { suite: "symphony", name: "a::one", line: "assertion failed: left == right" },
          { suite: null, name: "b::two", line: "panicked at src/b.rs:3" },
        ],
        suites: [{ name: "symphony", passed: 10, failed: 2, ignored: 1, s: 12 }],
      }),
      "red",
      30,
    ),
  ];
  const run = endedLine(
    aLine(1, {
      queued: TODAY,
      started: TODAY,
      log: "logs/2026-11-15/x.log",
      cpu_s: 50,
      dirty: true,
      rerun_of: id(9),
      handle: "lead",
    }),
    "red",
    60,
  );
  const view = (fields: Partial<RunViewProps> = {}) =>
    renderToStaticMarkup(
      <RunView
        page={{ run, steps, unread: [] }}
        log={{ size: 2_000_000, from: 1_900_000, to: 2_000_000, text: "the tail" }}
        logError={null}
        logMissing={false}
        running={new Set()}
        at={TODAY + 500}
        loadingEarlier={false}
        onEarlier={() => {}}
        {...fields}
      />,
    );

  it("is its ghost, busy, until its first answer, and once that read failed says so, not busy", () => {
    const loading = renderToStaticMarkup(<RunWaiting error={null} onRetry={() => {}} />);
    expect(loading).toMatch(/^<div class="runs run-page" data-ready="loading" aria-busy="true">/);
    expect(loading).toContain("Loading the run…");
    // A failed page is drawn, not busy: `symphony shot` pictures what it says.
    const failed = renderToStaticMarkup(<RunWaiting error="Gone." onRetry={() => {}} />);
    expect(failed).toMatch(/^<div class="runs run-page" data-ready="failed" aria-busy="false">/);
    expect(failed).toContain("The run could not be read");
    expect(failed).toContain("Gone.");
    expect(failed).not.toContain("Loading the run…");
  });

  it("says how it went, its facts, its steps as bars, the tests it failed and its suites", () => {
    const html = view();
    expect(html).toMatch(/^<div class="runs run-page" data-ready="run" data-result="red">/);
    expect(html).toContain('href="/runs"');
    expect(html).toContain("0123456, with changes not committed");
    expect(html).toContain("worker-a for lead");
    expect(html).toContain("Sun 15 Nov, 00:00 UTC");
    expect(html).toContain("<dt>Took</dt><dd>1 min</dd>");
    expect(html).toContain("50 s of CPU");
    expect(html).toContain("<dt>Exit</dt><dd>1</dd>");
    expect(html).toContain(`href="/runs?run=${id(9)}"`);
    expect(html).toContain('style="left:0.00%;width:50.00%"');
    expect(html).toContain('style="left:50.00%;width:50.00%"');
    expect(html).toContain("2 failed tests");
    expect(html).toContain('<code>a::one</code><span class="run-suite"> in symphony</span>');
    expect(html).toContain("panicked at src/b.rs:3");
    expect(html).toContain(
      '<th scope="row">symphony</th><td data-label="Passed">10</td><td data-label="Failed">2</td>' +
        '<td data-label="Ignored">1</td><td data-label="Time">12 s</td>',
    );
    expect(html).toContain("The last 100.0 KB of 2.0 MB");
    expect(html).toContain("Show earlier output");
    expect(html).toContain("the tail");
  });

  it("is a step's page with the way back to its run, running while the machine runs it", () => {
    const step = aLine(5, { parent: id(1), step: "unit", signal: 15, log: null });
    const html = view({ page: { run: step, steps: [], unread: [] }, running: new Set([id(5)]) });
    expect(html).toContain('data-result="running"');
    expect(html).toContain(`href="/runs?run=${id(1)}"`);
    expect(html).toContain("The run it is a step of");
    expect(html).toContain("<dt>Running for</dt><dd>8 min 15 s</dd>");
    expect(html).toContain("<dt>Exit</dt><dd>signal 15</dd>");
    // With no end and nothing running it, it says when it started.
    expect(view({ page: { run: step, steps: [], unread: [] } })).toContain(
      "<dt>Started</dt><dd>8 min 15 s ago, no end written</dd>",
    );
    expect(html).not.toContain("Steps");
    expect(html).not.toContain("failed test");
    expect(html).toContain("No log was kept");
  });

  it("says where its log stands: being read, not read, empty, whole, or reading earlier", () => {
    expect(view({ log: null })).toContain("Reading the log…");
    expect(view({ log: null, logError: "Gone." })).toContain("The log could not be read: Gone.");
    const kept = view({ log: null, logMissing: true });
    expect(kept).toContain('<p class="runs-quiet">This run&#x27;s log is no longer kept.</p>');
    expect(kept).not.toContain('role="alert"');
    const going = { run: aLine(1, { log: "logs/x.log" }), steps: [], unread: [] };
    expect(view({ page: going, log: null, logMissing: true })).toContain(
      "Its log is not written yet.",
    );
    expect(view({ log: { size: 0, from: 0, to: 0, text: "" } })).toContain(
      "Nothing written to the log yet.",
    );
    const whole = view({ log: { size: 812, from: 0, to: 812, text: "all" } });
    expect(whole).toContain("The whole log, 812 B");
    expect(whole).not.toContain("Show earlier output");
    expect(view({ loadingEarlier: true })).toContain("Loading…");
  });

  it("takes the suites of its own line over its steps'", () => {
    const own = { ...run, suites: [{ name: "own", passed: 1, failed: 0, ignored: 0, s: 1 }] };
    const html = view({ page: { run: own, steps, unread: [] } });
    expect(html).toContain('<th scope="row">own</th>');
    expect(html).not.toContain('<th scope="row">symphony</th>');
  });

  it("says a binary that ended with no result line ended so, never as a pass", () => {
    const crashed = { name: "cli", passed: 3, failed: 0, ignored: 0, s: 4, error: true };
    const html = view({ page: { run: { ...run, suites: [crashed] }, steps, unread: [] } });
    expect(html).toContain(
      '<th scope="row">cli<span class="run-suite-error"> ended with no result</span></th>',
    );
    expect(view()).not.toContain("ended with no result");
  });
});

describe("the page", () => {
  it("opens on the list with this browser's filters, what runs now being read", () => {
    vi.stubGlobal("localStorage", memoryStorage());
    vi.stubGlobal("location", { search: "" });
    localStorage.setItem(FILTERS_KEY, "pipeline=lint");
    const html = renderToStaticMarkup(<RunsPage />);
    expect(html).toMatch(
      /^<main class="runs-page"><div class="runs" data-ready="loading" aria-busy="true">/,
    );
    expect(html).toContain("Reading what runs now…");
    expect(html).toContain("Reading the schema updates…");
    expect(html).toContain("Loading the runs…");
    expect(html).toContain('<option value="lint" selected="">lint</option>');
  });

  it("opens on the run its address names", () => {
    vi.stubGlobal("location", { search: `?run=${id(4)}` });
    const html = renderToStaticMarkup(<RunsPage />);
    expect(html).toContain('class="runs run-page" data-ready="loading"');
    expect(html).toContain("Loading the run…");
  });
});

/** 2026-11-19, 00:00 UTC. */
const DAY = 1_795_046_400;

/** The record of a board's first store at its schema, as the ledger holds its words. */
const SEEN = "schema-seen.log: the first store that saw the board at this schema";

/** The record of a release's install: no colon, its words its name. */
const MANIFEST = "release-2026-11-19's manifest, written by its install";

/** The record of the server's first write at the schema. */
const HANDED = "the server's hand-off: its reload's line in server.log";

/** A time at `at` from the record `words`, named by its words before their first colon. */
const stamp = (at: number, words: string): UpdateTime => ({
  at,
  source: words.split(":")[0] ?? words,
  words,
});

/** The `alpha` board's update to schema 23: reached 08:05:09, its release installed 09:34:29 and
 * served 12:51:19 on 2026-11-19, as the server says it. */
function aServed(fields: Partial<SchemaUpdate> = {}): SchemaUpdate {
  return {
    id: "alpha@23",
    project: "alpha",
    schema: 23,
    generation: 1,
    state: "served",
    reached: stamp(DAY + 29_109, SEEN),
    available: stamp(DAY + 34_469, MANIFEST),
    served: stamp(DAY + 46_279, HANDED),
    total: 17_170,
    total_said: "4 h 46 min",
    after_available: 11_810,
    after_available_said: "3 h 16 min",
    failures: 0,
    failure: null,
    task: null,
    ...fields,
  };
}

/** The `beta` board's update to schema 24, in its second generation, open and failed twice. */
function aFailing(fields: Partial<SchemaUpdate> = {}): SchemaUpdate {
  return aServed({
    id: "beta@24.2",
    project: "beta",
    schema: 24,
    generation: 2,
    state: "open",
    reached: stamp(DAY + 50_000, SEEN),
    available: stamp(DAY + 50_100, MANIFEST),
    served: null,
    total: null,
    total_said: null,
    after_available: null,
    after_available_said: null,
    failures: 2,
    failure: "the server still reads the board at 23",
    task: "/symphony/task/tests/release-red-update-beta-24-g2",
    ...fields,
  });
}

describe("the schema updates", () => {
  const answer = (updates: SchemaUpdate[], unread: string | null = null): SchemaUpdates => ({
    updates,
    unread,
  });

  it("read the fields the section draws, each of its type, and refuse an answer without them", () => {
    const read = answer([aServed(), aFailing()]);
    expect(readSchemaUpdates(read)).toEqual(read);
    expect(readSchemaUpdates({ updates: [] })).toEqual(answer([]));
    expect(readSchemaUpdates({ updates: [], unread: "/m/updates.json: no" })).toEqual(
      answer([], "/m/updates.json: no"),
    );
    for (const [what, value] of [
      ["no updates", { unread: null }],
      ["a state it does not know", answer([aServed({ state: "gone" as SchemaUpdate["state"] })])],
      [
        "a time with no record's words",
        answer([aServed({ reached: { at: 1, source: "x" } as SchemaUpdate["reached"] })]),
      ],
      ["a wait not said", answer([aServed({ total_said: 4 as unknown as string })])],
      ["a task that is no text", answer([aFailing({ task: 1 as unknown as string })])],
      ["no count of failures", answer([aServed({ failures: null as unknown as number })])],
      ["an unread that is no text", { updates: [], unread: 1 }],
    ] as const) {
      expect(readSchemaUpdates(value), what).toBeNull();
    }
  });

  it("are read at once and on the list's timer naming the last tag, until a 404", async () => {
    const asked = server();
    const page = new AbortController();
    const { waited, wait } = waits(page, 4);
    const { told, check } = telling<SchemaUpdates>();
    const done = followUpdates(page.signal, check, wait);
    const read = answer([aServed()]);
    await asking(asked, 1);
    expect(asked[0]?.url).toBe(SCHEMA_UPDATES_PATH);
    asked[0]?.answer(reply(200, read, "u1"));
    await asking(asked, 2);
    asked[1]?.answer(new Response(null, { status: 304 }));
    await asking(asked, 3);
    asked[2]?.answer(reply(200, { updates: "none" }, "u2"));
    await asking(asked, 4);
    asked[3]?.answer(reply(404, { error: "no such route" }));
    await done;
    expect(told).toEqual([
      ["answered", read],
      ["answered", null],
      ["failed", "The schema updates' answer is not schema updates. Trying again…", false],
      ["failed", "no such route (the server answered 404).", true],
    ]);
    expect(waited).toEqual([LIST_MS, LIST_MS, LIST_MS]);
  });

  it("are told nothing once the page that asked is gone", async () => {
    const asked = server();
    for (const late of [reply(200, answer([aServed()])), reply(503, {})]) {
      const page = new AbortController();
      const { told, check } = telling<SchemaUpdates>();
      const at = asked.length;
      const done = followUpdates(page.signal, check, async () => {});
      await asking(asked, at + 1);
      page.abort();
      expect(asked[at]?.signal?.aborted).toBe(true);
      asked[at]?.answer(late);
      await done;
      expect(told).toEqual([]);
    }
  });

  it("list the open ones first, each part newest first as the server sent it", () => {
    const older = aServed({ id: "alpha@22", schema: 22 });
    const open = aFailing();
    const left = aServed({ id: "gamma@23", project: "gamma", state: "left", served: null });
    expect(updateRows([aServed(), older, open, left]).map((update) => update.id)).toEqual([
      "beta@24.2",
      "alpha@23",
      "alpha@22",
      "gamma@23",
    ]);
  });

  it("say each time in UTC, one on the day of the one before by its hour, with its words", () => {
    expect(updateTimes(aServed())).toEqual([
      {
        name: "reached",
        said: "reached Thu 19 Nov, 08:05 UTC",
        words: SEEN,
      },
      {
        name: "release installed",
        said: "release installed 09:34 UTC",
        words: MANIFEST,
      },
      { name: "served", said: "served 12:51 UTC", words: HANDED },
    ]);
    const nextDay = aServed({
      served: stamp(DAY + 86_400 + 60, "this refresh found the server writing it"),
    });
    expect(updateTimes(nextDay)[2]?.said).toBe("served Fri 20 Nov, 00:01 UTC");
    expect(updateTimes(aServed({ reached: null, served: null }))).toEqual([
      {
        name: "release installed",
        said: "release installed Thu 19 Nov, 09:34 UTC",
        words: MANIFEST,
      },
    ]);
  });

  it("draw a served one with its wait, the part after its release and each time's record on hover", () => {
    const html = renderToStaticMarkup(<UpdatesView updates={answer([aServed()])} error={null} />);
    expect(html).toContain('<h2 id="runs-updates-title">Schema updates</h2>');
    expect(html).toContain('data-update="alpha@23" data-state="served"');
    expect(html).toContain(
      '<span class="run-result" data-result="served"><span class="run-mark" aria-hidden="true">✓</span><span class="run-word">served</span></span>',
    );
    expect(html).toContain('<span class="run-label">alpha board · schema 23</span>');
    expect(html).toContain(">4 h 46 min</span>");
    expect(html).toContain('<span class="run-who">3 h 16 min after its release</span>');
    expect(html).toContain(
      '<span title="schema-seen.log: the first store that saw the board at this schema">reached Thu 19 Nov, 08:05 UTC</span>',
    );
    expect(html).toContain(
      '<span title="the server&#x27;s hand-off: its reload&#x27;s line in server.log"> · served 12:51 UTC</span>',
    );
    // Where the times come from is not said under them: each time's record is its title.
    expect(html).not.toContain("Times from");
    expect(html).not.toContain("run-note");
  });

  it("draw the failures of one no longer open, and a task with no failure said, each as it is", () => {
    const once = aServed({ failures: 1, failure: "the server read 22" });
    const html = renderToStaticMarkup(
      <UpdatesView
        updates={answer([once, aFailing({ id: "beta", failures: 0, failure: null })])}
        error={null}
      />,
    );
    expect(html).toContain(
      '<span class="run-note">1 failed refresh, the last: the server read 22</span>',
    );
    expect(html).toContain(
      '<span class="run-note">task <a href="/symphony/task/tests/release-red-update-beta-24-g2">tests/release-red-update-beta-24-g2</a></span>',
    );
    const unnamed = renderToStaticMarkup(
      <UpdatesView updates={answer([aServed({ failures: 3, failure: null })])} error={null} />,
    );
    expect(unnamed).toContain('<span class="run-note">3 failed refreshes</span>');
    const elsewhere = renderToStaticMarkup(
      <UpdatesView updates={answer([aFailing({ task: "https://h/task/x" })])} error={null} />,
    );
    expect(elsewhere, "only a path of the app is linked").not.toContain("<a ");
  });

  it("draw an open one first, waiting, with its failures and its task's page as the server names it", async () => {
    vi.resetModules();
    vi.stubGlobal("document", {
      getElementById: () => ({ textContent: '{"name":"","register":"","base":"/alpha"}' }),
    });
    const { UpdatesView: View } = await import("./runs");
    const html = renderToStaticMarkup(
      <View updates={answer([aServed(), aFailing()])} error={null} />,
    );
    expect(html.indexOf('data-update="beta@24.2"')).toBeLessThan(
      html.indexOf('data-update="alpha@23"'),
    );
    expect(html).toContain(
      '<span class="run-result" data-result="waiting"><span class="run-mark" aria-hidden="true">◷</span><span class="run-word">waiting</span></span>',
    );
    expect(html).toContain('<span class="run-label">beta board · schema 24, generation 2</span>');
    expect(html).toContain("2 failed refreshes, the last: the server still reads the board at 23");
    expect(html).toContain(
      '<a href="/symphony/task/tests/release-red-update-beta-24-g2">tests/release-red-update-beta-24-g2</a>',
    );
    expect(html).not.toContain('href="/alpha/symphony/');
  });

  it("say none is recorded, a ledger not read, a failed read and the first read under way", () => {
    const view = (updates: SchemaUpdates | null, error: string | null = null) =>
      renderToStaticMarkup(<UpdatesView updates={updates} error={error} />);
    expect(view(answer([]))).toContain('<p class="runs-quiet">No schema update recorded yet.</p>');
    const unread = view(answer([], "/m/updates.json: format 2 is a later build's"));
    expect(unread).toContain(
      "The schema updates could not be read: /m/updates.json: format 2 is a later build&#x27;s",
    );
    expect(unread).not.toContain("No schema update recorded yet.");
    const failed = view(null, "No answer. Trying again…");
    expect(failed).toContain("The schema updates could not be read: No answer. Trying again…");
    expect(failed).not.toContain("Reading the schema updates…");
    expect(view(null)).toContain("Reading the schema updates…");
  });
});
