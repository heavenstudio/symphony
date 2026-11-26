import { describe, expect, it } from "vitest";
import GUARDS from "../../symphony-web-tests/tests/browser/timeline/revision-guards.json";
import { clockOf, dateOf, readTimeline } from "./timeline-layout";
import {
  historyKept,
  opened,
  QUIET_LEAST,
  type Quiet,
  quietStretches,
  SKIPPING,
  skipOn,
  skippedOf,
  switched,
} from "./timeline-quiet";
import type { History, TaskTimeline, TimelineMessage } from "./wire";

const HOUR = 3600;
/** Midnight UTC, a task's hours counted from it. */
const T = 1_794_355_200;
const at = (hours: number) => T + hours * HOUR;

/** A message at `hours`. */
function message(id: number, hours: number): TimelineMessage {
  return {
    id,
    at: at(hours),
    sender: "quiet-wren",
    recipient: "@orchestrator",
    kind: "msg",
    from: { state: "unknown", candidates: [] },
    to: { state: "unknown", candidates: [] },
    after_close: null,
  };
}

/** A whole history: its one source complete, with `messages` and no roles. */
function kept(messages: TimelineMessage[] = []): History {
  return {
    sources_complete: true,
    sources: [{ slug: "t", state: "complete", first_id: 1, last_id: 9, before: null }],
    roles: [],
    messages,
  };
}

/** A task made at 0 h, worked from half past to 2 h, done at 2 h and read at 12 h: its history
 * whole, nothing recorded after it closed but what `extra` adds. */
function task(extra: Partial<TaskTimeline> = {}): TaskTimeline {
  return {
    slug: "t",
    now: at(12),
    zone: [],
    created: at(0),
    closed: at(2),
    bounces: 0,
    stages: [
      { status: "ready", from: at(0), to: at(0.5) },
      { status: "in_progress", from: at(0.5), to: at(2) },
      { status: "done", from: at(2), to: null },
    ],
    claims: [{ by: "quiet-wren", from: at(0.5), to: at(2) }],
    rounds: [],
    runs: [],
    gates: [],
    history: kept(),
    ...extra,
  };
}

/** The stretches found, in hours from midnight. */
const hours = (found: Quiet[]) =>
  found.map((stretch) => [(stretch.from - T) / HOUR, (stretch.to - T) / HOUR, stretch.status]);

describe("a task's quiet stretches", () => {
  it("are the card's on its task: the closed stretch, never the time no run is kept of", () => {
    const guards = readTimeline(GUARDS);
    if (guards === null) throw new Error("the card's task is no timeline");
    const found = quietStretches(guards);
    expect(
      found.map((stretch) => [
        `${dateOf(guards, stretch.from)} ${clockOf(guards, stretch.from)}`,
        `${dateOf(guards, stretch.to)} ${clockOf(guards, stretch.to)}`,
        Math.round((stretch.to - stretch.from) / 60),
        stretch.status,
      ]),
    ).toEqual([["11-19 14:03", "11-20 00:57", 654, "done"]]);
  });

  it("are found only between two records, at least an hour apart, never after the last", () => {
    // Nothing after the close: the close is the last record, and the read at 12 h is none.
    expect(quietStretches(task())).toEqual([]);
    // A message at 9 h: the seven hours before it are quiet, the three after it are not.
    expect(hours(quietStretches(task({ history: kept([message(1, 9)]) })))).toEqual([
      [2, 9, "done"],
    ]);
    // Exactly an hour is quiet; a second less is not.
    expect(hours(quietStretches(task({ history: kept([message(1, 3)]) })))).toEqual([
      [2, 3, "done"],
    ]);
    const short = task({ history: kept([{ ...message(1, 3), at: at(3) - 1 }]) });
    expect(quietStretches(short)).toEqual([]);
    expect(QUIET_LEAST).toBe(HOUR);
  });

  it("are broken by every record: a run, a role, a round, a change, a message", () => {
    const foundWith = (extra: Partial<TaskTimeline>) =>
      hours(quietStretches(task({ history: kept([message(1, 9)]), ...extra })));
    // A run queued at 4 h that ended at 5 h, and one still running, queued at 7 h.
    expect(
      foundWith({
        runs: [
          {
            phase: "test",
            label: "t",
            queued: at(4),
            started: at(4.5),
            ended: at(5),
            result: "green",
          },
          {
            phase: "test",
            label: "t",
            queued: at(7),
            started: at(7),
            ended: null,
            result: "running",
          },
        ],
      }),
    ).toEqual([
      [2, 4, "done"],
      [5, 7, "done"],
    ]);
    // A reviewer's role held from half past 2 to 6 h, its source the history's.
    const reviewer = {
      role: "reviewer",
      by: "lunar-heron",
      source: "t",
      from: at(2.5),
      from_id: 2,
      to: at(6),
      to_id: 3,
      uncertain: false,
    };
    expect(foundWith({ history: { ...kept([message(1, 9)]), roles: [reviewer] } })).toEqual([
      [6, 9, "done"],
    ]);
    // An executor's role still held, to now: nothing after it is quiet.
    const open = { ...reviewer, role: "executor", to: null, to_id: null };
    expect(foundWith({ history: { ...kept([message(1, 9)]), roles: [open] } })).toEqual([]);
    // A round asked for at half past 3, taken at 4 h and ended at 5 h, and a gate's change at 7 h:
    // the ask is a record too.
    expect(
      foundWith({
        rounds: [
          { round: 1, from: at(3.5), taken: at(4), to: at(5), by: "x", verdict: "approved" },
        ],
        gates: [{ name: "tests", states: [{ state: "met", from: at(7), detail: "" }] }],
      }),
    ).toEqual([
      [2, 3.5, "done"],
      [5, 7, "done"],
      [7, 9, "done"],
    ]);
    // A round asked for at 4 h that nobody took: its ask alone is a record.
    expect(
      foundWith({
        rounds: [{ round: 1, from: at(4), taken: null, to: null, by: null, verdict: null }],
      }),
    ).toEqual([
      [2, 4, "done"],
      [4, 9, "done"],
    ]);
    // A place a custom workflow put it at, at 5 h.
    expect(
      foundWith({
        places: [
          {
            phase: "p",
            phase_label: "P",
            state: "s",
            state_label: "S",
            status: "done",
            from: at(5),
            by: "owner",
          },
        ],
      }),
    ).toEqual([
      [2, 5, "done"],
      [5, 9, "done"],
    ]);
  });

  it("are not broken by an orchestrator's hold, but by its start and its end", () => {
    const hold = {
      role: "orchestrator",
      by: "coral-badger",
      source: "t",
      from: at(0),
      from_id: 1,
      to: at(5),
      to_id: 2,
      uncertain: false,
    };
    const found = quietStretches(task({ history: { ...kept([message(1, 9)]), roles: [hold] } }));
    expect(hours(found)).toEqual([
      [2, 5, "done"],
      [5, 9, "done"],
    ]);
  });

  it("are measured only from the time runs are kept, and only with a whole history", () => {
    const late = task({ history: kept([message(1, 9)]) });
    // Runs kept from 6 h: the stretch begins there.
    expect(hours(quietStretches({ ...late, runs_from: at(6) }))).toEqual([[6, 9, "done"]]);
    // From 8.5 h: what is left is under an hour.
    expect(quietStretches({ ...late, runs_from: at(8.5) })).toEqual([]);
    // No history, a source left partial, or sources left unread: nothing is measured.
    expect(quietStretches({ ...late, history: null })).toEqual([]);
    const history = late.history as History;
    const partial = {
      ...history,
      sources: [
        { ...(history.sources[0] as History["sources"][number]), state: "partial" as const },
      ],
    };
    expect(quietStretches({ ...late, history: partial })).toEqual([]);
    expect(quietStretches({ ...late, history: { ...history, sources_complete: false } })).toEqual(
      [],
    );
    // A timeline from a server that sends no history at all.
    const { history: _none, ...unsent } = late;
    expect([historyKept(late), historyKept(unsent)]).toEqual([true, false]);
  });

  it("take records at one instant as one, and say the status held through them", () => {
    // Two messages at 4 h and a stage that ends there: one record, the stretches either side.
    const same = task({
      history: kept([message(1, 4), message(2, 4), message(3, 9)]),
    });
    expect(hours(quietStretches(same))).toEqual([
      [2, 4, "done"],
      [4, 9, "done"],
    ]);
    // A ready task's wait with no one on it, between its making and its first claim at 5 h: its
    // status is ready; an open task's time since its last record is still going.
    const waiting = task({
      now: at(9),
      closed: null,
      stages: [
        { status: "ready", from: at(0), to: at(5) },
        { status: "in_progress", from: at(5), to: null },
      ],
      claims: [{ by: "quiet-wren", from: at(5), to: at(6) }],
    });
    expect(hours(quietStretches(waiting))).toEqual([[0, 5, "ready"]]);
    // Before any stage, a status none says.
    const early = task({ history: kept([message(1, -3)]) });
    expect(hours(quietStretches(early))).toEqual([[-3, 0, ""]]);
  });
});

describe("which quiet stretches are skipped", () => {
  const quiet: Quiet[] = [
    { from: at(2), to: at(4), status: "done" },
    { from: at(5), to: at(9), status: "done" },
  ];
  const [first, second] = quiet as [Quiet, Quiet];

  it("opens with all of them skipped, a band pressed drawing its own on the true clock", () => {
    expect(skippedOf(quiet, SKIPPING)).toEqual(quiet);
    const one = opened(SKIPPING, first);
    expect(skippedOf(quiet, one)).toEqual([second]);
    // Pressed again, nothing changes.
    expect(opened(one, first)).toBe(one);
    expect(skipOn(quiet, one)).toBe(true);
    // Both pressed: none skipped, and the switch reads off.
    const both = opened(one, second);
    expect(skippedOf(quiet, both)).toEqual([]);
    expect(skipOn(quiet, both)).toBe(false);
  });

  it("switches every stretch to the true clock where any is skipped, else skips them all", () => {
    const off = switched(quiet, SKIPPING);
    expect(off).toEqual({ on: false, opened: [] });
    expect(skippedOf(quiet, off)).toEqual([]);
    expect(switched(quiet, off)).toEqual(SKIPPING);
    // Every band pressed reads off, and the switch skips them all again.
    expect(switched(quiet, opened(opened(SKIPPING, first), second))).toEqual(SKIPPING);
    // A task with none reads off.
    expect(skipOn([], SKIPPING)).toBe(false);
  });
});
