// What a task's head draws of its state wherever it is drawn: its status in words, and its builds'
// and tests as the machines report them, a chip each. A task's page draws them in its head, and the
// task's card (taskcard.tsx, the board's and the agents page's) on the card: apart from the card, so
// a task's page loads none of the card's code.

import type { MachineWork } from "./wire";
import { type Phase, phaseLabel, shownPhases } from "./work";

/** A status as the board says it: `in_progress` reads "in progress". */
export function statusWords(status: string): string {
  return status.replaceAll("_", " ");
}

/**
 * The builds and tests of `slug`'s task, a chip each: live while a fresh machine runs it, else
 * stale, each saying how sure it is; nothing for a task with none.
 */
export function WorkPhases({
  work,
  slug,
  connected,
}: {
  work: MachineWork;
  slug: string;
  connected: boolean;
}) {
  return <PhaseChips rows={shownPhases(work, slug, connected)} />;
}

/** Builds and tests as a card, an agent or a machine shows them: a chip each, or nothing. */
export function PhaseChips({ rows }: { rows: Phase[] }) {
  if (rows.length === 0) return null;
  return (
    // A group, so the label is read: a plain box's is not.
    // biome-ignore lint/a11y/useSemanticElements: a row of chips, laid out as the card's others are, not a form's fieldset
    <div className="work-phases chips" role="group" aria-label="Build and test work">
      {rows.map((row, at) => (
        <span
          // biome-ignore lint/suspicious/noArrayIndexKey: a phase is its place in the machines' report
          key={at}
          className={
            row.freshness === "fresh" && row.operation.state === "running"
              ? "chip work-phase health-live"
              : "chip work-phase health-stale"
          }
          data-work-kind={row.operation.kind === "build" ? "build" : "test"}
          title={`${row.operation.recipe} · ${row.operation.resource} · ${row.operation.reason}`}
        >
          {phaseLabel(row)}
        </span>
      ))}
    </div>
  );
}
