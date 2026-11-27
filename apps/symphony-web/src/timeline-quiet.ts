// A task's quiet stretches, with no DOM: where the task's records hold nothing for at least
// QUIET_LEAST and are kept across it, so the wide timeline skips it as a band
// (docs/design/analysis/timeline-navigation.md, "Quiet stretches", under the owner's B), and which of
// them the page skips now (`Skipping`).
//
// A stretch is quiet where no build or test is queued or running, no executor or reviewer holds the
// task, and nothing happens at any instant in it: no message, and no change of stage, role, review
// round, gate or place. An orchestrator's hold does not count against quiet (an orchestrator holds
// many tasks at once), though its start and its end are changes. A stretch is found only between
// two records, never after the last: a task's end is a record, and on an open task the time since
// the last one is still going.
//
// It is measured only where every record the rule reads is kept across it: the recorder's runs from
// `runs_from`, where the stretch is cut to begin, and a complete role and address history (every
// source read whole). Anywhere else the same emptiness is unknown, and nothing is found there.

import type { TaskTimeline } from "./wire";

/** The least a stretch lasts to be quiet, in seconds: an hour. */
export const QUIET_LEAST = 3600;

/** A quiet stretch: from its last record before to its first after, and the status the task held
 * through it, which its words say ("done", "ready"…; none before the task was made). */
export interface Quiet {
  from: number;
  to: number;
  status: string;
}

/** Whether the task's role and address history is kept whole: read, and every source of it read
 * complete. */
export function historyKept(timeline: TaskTimeline): boolean {
  const history = timeline.history;
  return (
    history?.sources_complete === true &&
    history.sources.every((source) => source.state === "complete")
  );
}

/** The status the task held at `at`: the stage it was in then. */
function statusAt(timeline: TaskTimeline, at: number): string {
  const stage = timeline.stages.find(
    (stage) => stage.from <= at && at < (stage.to ?? Number.POSITIVE_INFINITY),
  );
  return stage?.status ?? "";
}

/** The quiet stretches of `timeline`, oldest first; none where its history is not kept whole. */
export function quietStretches(timeline: TaskTimeline): Quiet[] {
  const history = timeline.history;
  if (!historyKept(timeline) || !history) return [];
  const now = timeline.now;
  // What is not quiet: each record's hours, and each instant as an hour of no length.
  const records: [number, number][] = [];
  const held = (from: number, to: number | null | undefined) => {
    records.push([from, Math.max(from, to ?? now)]);
  };
  const instant = (at: number | null | undefined) => {
    if (at !== null && at !== undefined) records.push([at, at]);
  };
  for (const run of timeline.runs) held(run.queued, run.ended);
  for (const claim of timeline.claims) held(claim.from, claim.to);
  for (const round of timeline.rounds) {
    instant(round.from);
    instant(round.taken);
    instant(round.to);
    if (round.taken !== null && round.taken !== undefined) held(round.taken, round.to);
  }
  for (const role of history.roles) {
    if (role.role !== "orchestrator") held(role.from, role.to);
    instant(role.from);
    instant(role.to);
  }
  for (const stage of timeline.stages) {
    instant(stage.from);
    instant(stage.to);
  }
  for (const message of history.messages) instant(message.at);
  for (const gate of timeline.gates) for (const state of gate.states) instant(state.from);
  for (const place of timeline.places ?? []) instant(place.from);
  records.sort((a, b) => a[0] - b[0]);
  const kept = timeline.runs_from ?? Number.NEGATIVE_INFINITY;
  const found: Quiet[] = [];
  let reach: number | null = null;
  for (const [from, to] of records) {
    if (reach !== null) {
      const start = Math.max(reach, kept);
      if (from - start >= QUIET_LEAST) {
        found.push({ from: start, to: from, status: statusAt(timeline, start) });
      }
    }
    reach = reach === null ? to : Math.max(reach, to);
  }
  return found;
}

/** Which quiet stretches the page skips: all of them while `on`, but those `opened` (by their
 * start), each drawn on the true clock since its band was pressed. */
export interface Skipping {
  on: boolean;
  opened: readonly number[];
}

/** How each visit opens: every quiet stretch skipped (the owner's B). */
export const SKIPPING: Skipping = { on: true, opened: [] };

/** The stretches of `quiet` that `skipping` skips. */
export function skippedOf(quiet: readonly Quiet[], skipping: Skipping): Quiet[] {
  return skipping.on ? quiet.filter((stretch) => !skipping.opened.includes(stretch.from)) : [];
}

/** Whether the switch reads on: some stretch is skipped. */
export function skipOn(quiet: readonly Quiet[], skipping: Skipping): boolean {
  return skippedOf(quiet, skipping).length > 0;
}

/** The skipping a press of the switch leaves: every stretch drawn on the true clock where any was
 * skipped, else every stretch skipped. */
export function switched(quiet: readonly Quiet[], skipping: Skipping): Skipping {
  return { on: !skipOn(quiet, skipping), opened: [] };
}

/** The skipping a press of `stretch`'s band leaves: that stretch on the true clock, the rest as
 * they were. */
export function opened(skipping: Skipping, stretch: Quiet): Skipping {
  return skipping.opened.includes(stretch.from)
    ? skipping
    : { on: skipping.on, opened: [...skipping.opened, stretch.from] };
}
