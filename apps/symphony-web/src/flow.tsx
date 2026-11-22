// A custom workflow on the task board (docs/workflow/WORKFLOW.md, "The app"): a chunk of its own, which the
// board loads only once its listing says a workflow is active (`waiting.workflow`, its activation
// generation), so a board its own writers keep fetches none of it. It reads the active definition
// (`/api/workflow`) and gives the board its lanes (the phases that are columns, by their order,
// then the process overlays and any status the board's own writers still hold tasks in), its
// phone tabs (every phase by its phone order), the names its IDs read as (the board's `NAMES`),
// and the strip of the workflow's states, each a wait facet (`state:ID`) that lists its open tasks
// across their phases, with a button for each phase that is no column. It imports nothing of the
// board's own chunk, which the board hands it: a module both imported would be split into a file
// of its own, which every board's first visit would fetch. Its style sheet it puts in the page
// itself, as the timeline does: a chunk with a sheet of its own would bring Vite's preload helper
// into the board's first visit.

import type { ReactNode } from "react";
import STYLE from "./flow.css?inline";
import { scoped } from "./project";
import { getJson } from "./request";
import { field, list } from "./shared";
import { tryAgain } from "./state";
import { type WaitingBoard, WORKFLOW_PATH } from "./wire";

/** The id of the flow's style element, put in the page once. */
const STYLE_ID = "flow-style";

/** Put the flow's style sheet in the page once, before the project's theme, whose rules win. */
export function putStyle(): void {
  if (document.getElementById(STYLE_ID)) return;
  const element = document.createElement("style");
  element.id = STYLE_ID;
  element.textContent = STYLE;
  document.head.insertBefore(element, document.head.querySelector("[data-symphony-theme]"));
}

/** A column's key for a phase: this, then the phase's ID, as the server keys it. */
export const PHASE = "phase:";
/** A wait facet's for a state: this, then the state's ID. */
export const STATE = "state:";

/** A phase of the definition, as the board draws it. */
export interface Phase {
  id: string;
  label: string;
  order: number;
  phone_order: number;
  column: boolean;
}

/** A state of the definition: its ID and label. */
export interface Named {
  id: string;
  label: string;
}

/** The active definition as the board reads it: its phases, states and the pairs it allows. */
export interface Definition {
  phases: Phase[];
  states: Named[];
  pairs: [string, string][];
}

/** What the strip of a workflow's states is drawn from and acts on. */
export interface StripProps {
  waiting: WaitingBoard | null;
  /** The wait facet chosen: a state's is `state:ID`. */
  wait: string;
  /** Choose a facet, or the one chosen again to leave it. */
  choose: (facet: string) => void;
  /** Open a phase that is no column as a list of its own, or "" to close it. */
  open: (stage: string) => void;
  opened: string;
  phone: boolean;
}

/** A custom workflow as the board draws it. */
export interface Flow {
  /** The desk's lanes and the phone's tabs, under the listing `waiting`. */
  lanes: (waiting: WaitingBoard | null) => [string[], string[]];
  strip: (props: StripProps) => ReactNode;
}

const isText = (value: unknown): value is string => typeof value === "string";
const isNumber = (value: unknown): value is number => typeof value === "number";
const isId = (value: unknown) => isText(value) && /^[a-z][a-z0-9_-]*$/.test(value);

/**
 * The definition a board's workflow read (`/api/workflow`) holds, as the board draws it, or none:
 * for a read with no workflow (`undefined`), and for one whose phases, states or positions are not
 * what a board draws (`null`).
 */
export function readDefinition(value: unknown): Definition | null | undefined {
  const workflow = field(value, "workflow");
  if (workflow === null) return undefined;
  const definition = field(workflow, "definition");
  const phases = field(definition, "phases");
  const states = field(definition, "states");
  const positions = field(definition, "positions");
  const named = (one: unknown) => isId(field(one, "id")) && isText(field(one, "label"));
  if (
    !Array.isArray(phases) ||
    !Array.isArray(states) ||
    !Array.isArray(positions) ||
    !phases.every(
      (one) =>
        named(one) &&
        isNumber(field(one, "order")) &&
        isNumber(field(one, "phone_order")) &&
        typeof field(one, "column") === "boolean",
    ) ||
    !states.every(named) ||
    !positions.every(
      (one) => isId(field(field(one, "at"), "phase")) && isId(field(field(one, "at"), "state")),
    )
  ) {
    return null;
  }
  return {
    phases: phases as Phase[],
    states: (states as Named[]).map(({ id, label }) => ({ id, label })),
    pairs: positions.map((one) => [
      field(field(one, "at"), "phase") as string,
      field(field(one, "at"), "state") as string,
    ]),
  };
}

/** The lanes and tabs of the board its own writers keep: its pipeline's and the phone's. */
export type Lanes = [readonly string[], readonly string[]];

/**
 * Fill `names` (the board's `NAMES`) with what `definition`'s IDs read as: each phase's column
 * key its label, each state's facet its label, and each pair a task can be at its phase's and
 * state's, as its card names its place. What an older definition named goes.
 */
export function name(definition: Definition, names: Map<string, string>): void {
  names.clear();
  const label = (named: Named[], id: string) => named.find((one) => one.id === id)?.label ?? id;
  for (const phase of definition.phases) names.set(`${PHASE}${phase.id}`, phase.label);
  for (const state of definition.states) names.set(`${STATE}${state.id}`, state.label);
  for (const [phase, state] of definition.pairs) {
    names.set(
      `${phase}/${state}`,
      `${label(definition.phases, phase)} · ${label(definition.states, state)}`,
    );
  }
}

/**
 * The desk's lanes and the phone's tabs of a board `definition` holds, under the listing `waiting`.
 * The lanes: the phases that are columns by their order, a phase the listing names that the
 * definition does not (a newer activation's, read next) by its ID, Build and Test, then each
 * status the board's own writers hold a task in, in the pipeline's order (`own`'s first). The
 * tabs: every phase by its phone order, the phase the definition does not name, then those
 * statuses in the phone's (`own`'s second).
 */
export function lanes(
  definition: Definition,
  waiting: WaitingBoard | null,
  own: Lanes,
): [string[], string[]] {
  const listed = (waiting?.columns ?? [])
    .filter((column) => column.ordinary + column.blocked > 0)
    .map((column) => column.status);
  const keys = (phases: Phase[]) => phases.map((phase) => `${PHASE}${phase.id}`);
  const unknown = listed.filter(
    (key) => key.startsWith(PHASE) && !keys(definition.phases).includes(key),
  );
  const kept = (order: readonly string[]) =>
    order.filter((status) => status !== "build" && status !== "test" && listed.includes(status));
  const by = (key: "order" | "phone_order") =>
    [...definition.phases].sort((a, b) => a[key] - b[key] || a.id.localeCompare(b.id));
  return [
    [
      ...keys(by("order").filter((phase) => phase.column)),
      ...unknown,
      "build",
      "test",
      ...kept(own[0]),
    ],
    [...keys(by("phone_order")), ...unknown, ...kept(own[1])],
  ];
}

/** The open tasks of each state the listing counts, by its ID; none for a count it cannot read. */
export function stateCounts(waiting: WaitingBoard | null): Map<string, number> {
  return new Map(
    list(waiting?.states).filter(
      (pair): pair is [string, number] =>
        Array.isArray(pair) && isText(pair[0]) && isNumber(pair[1]),
    ),
  );
}

/**
 * The strip of a board `definition` holds: a button for each phase that is no column (on a desk:
 * a phone has a tab for each), opening it as a list of its own as Backlog opens, and a chip for
 * each state with open tasks, or chosen, choosing it as a wait facet.
 */
export function Strip({
  definition,
  waiting,
  wait,
  choose,
  open,
  opened,
  phone,
}: StripProps & { definition: Definition }) {
  const counts = stateCounts(waiting);
  const columns = waiting?.columns ?? [];
  const shown = definition.states.filter(
    (state) => (counts.get(state.id) ?? 0) > 0 || wait === `${STATE}${state.id}`,
  );
  const off = phone ? [] : definition.phases.filter((phase) => !phase.column);
  if (shown.length === 0 && off.length === 0) return null;
  return (
    <nav className="wait-strip state-strip scroll-row" aria-label="Filter tasks by their state">
      {off.map((phase) => {
        const key = `${PHASE}${phase.id}`;
        const column = columns.find((one) => one.status === key);
        return (
          <button
            key={key}
            type="button"
            className="phase-open"
            data-open-stage={key}
            aria-expanded={opened === key}
            onClick={() => open(opened === key ? "" : key)}
          >
            {phase.label} <b>{(column?.ordinary ?? 0) + (column?.blocked ?? 0)}</b>
          </button>
        );
      })}
      {shown.map((state) => (
        <button
          key={state.id}
          type="button"
          className="wait-chip k-state"
          data-state={state.id}
          aria-pressed={wait === `${STATE}${state.id}`}
          onClick={() => choose(`${STATE}${state.id}`)}
        >
          {state.label} <b>{counts.get(state.id) ?? 0}</b>
        </button>
      ))}
    </nav>
  );
}

/** The board's workflow as `definition` gives it, over the board's `own` lanes and tabs. */
export function flowOf(definition: Definition, own: Lanes): Flow {
  return {
    lanes: (waiting) => lanes(definition, waiting, own),
    strip: (props) => <Strip definition={definition} {...props} />,
  };
}

/** The wait before each read again (`tryAgain`'s). */
type Wait = Parameters<typeof tryAgain>[3];

/**
 * The active definition of the board under `scope`, read again until it answers or `signal` ends:
 * none when the read says none is active, or is not one a board draws, or `signal` ended first.
 */
async function activeDefinition(
  scope: string,
  signal: AbortSignal,
  wait?: Wait,
): Promise<Definition | null> {
  const read: { definition?: Definition | null | undefined } = {};
  const ask = async () => {
    try {
      read.definition = readDefinition(await getJson(scoped(scope, WORKFLOW_PATH), signal));
    } catch {
      // Tried again on the feed's waits until it answers.
    }
  };
  await ask();
  await tryAgain(() => !("definition" in read), ask, signal, wait);
  return signal.aborted ? null : (read.definition ?? null);
}

/**
 * Read the active definition of the board under `scope` and name its IDs in `names`: the board's
 * workflow over its `own` lanes and tabs, or none when the read says none is active or is not one
 * a board draws, which leaves the board as its own writers draw it.
 */
export async function readFlow(
  scope: string,
  signal: AbortSignal,
  names: Map<string, string>,
  own: Lanes,
  wait?: Wait,
): Promise<Flow | null> {
  const definition = await activeDefinition(scope, signal, wait);
  if (definition === null) return null;
  putStyle();
  name(definition, names);
  return flowOf(definition, own);
}

/**
 * The names the IDs of the active definition of the board under `scope` read as (as [`name`] gives
 * them), for a page that names a task's place and draws none of the board's: none when no
 * definition a board draws is active.
 */
export async function readNames(
  scope: string,
  signal: AbortSignal,
  wait?: Wait,
): Promise<Map<string, string> | null> {
  const definition = await activeDefinition(scope, signal, wait);
  if (definition === null) return null;
  const names = new Map<string, string>();
  name(definition, names);
  return names;
}
