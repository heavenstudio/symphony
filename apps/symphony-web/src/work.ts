// The machines' build and test work (`/api/machine-work`, the feed's `machine-work`): the stage it
// places a task in on the board, and the phases a task's card shows of it. Uncertain evidence never
// reads as free capacity, and a lost feed never as current work.
//
// `taskStage`, `placesAlike`, `canPlace`, `conflicts` and `isFor` carry crates/symphony-api's own
// (task_stage.rs, machine_work.rs), held to the Rust by the cases it writes (wire.cases.json,
// work.test.ts). The phases carry the Dioxus client's (its `machine_work.rs`, gone with its task
// page), held by the browser tests; a machine's occupancy and the work a lost feed shows are the agents page's
// (agents-live.ts).

import { field } from "./shared";
import type {
  MachineObservation,
  MachineWork,
  Task,
  WorkFreshness,
  WorkKind,
  WorkOperation,
  WorkState,
} from "./wire";

/** The board's pipeline, in its order: a lifecycle status, or a machine's Build or Test. */
export const PIPELINE = [
  "concept",
  "design",
  "ready",
  "in_progress",
  "build",
  "test",
  "review",
  "done",
] as const;

/** The work of no machine: what the board knows before it reads any. */
export const NO_WORK: MachineWork = {
  freshness: "unknown",
  machines: [],
  unassigned: [],
  guidance: {},
};

const FRESHNESS: WorkFreshness[] = [
  "unknown",
  "fresh",
  "stale",
  "disconnected",
  "incomplete",
  "conflict",
];
const KINDS: WorkKind[] = ["build", "test", "command", "setup", "reservation"];
const STATES: WorkState[] = ["queued", "running"];

const isText = (value: unknown): value is string => typeof value === "string";
const isOptionalText = (value: unknown) => value === undefined || value === null || isText(value);

function isOperation(value: unknown): value is WorkOperation {
  const under = field(value, "under");
  return (
    ["machine_id", "operation_id", "task"].every((key) => isOptionalText(field(value, key))) &&
    ["recipe", "resource", "reason"].every((key) => isText(field(value, key))) &&
    KINDS.includes(field(value, "kind") as WorkKind) &&
    STATES.includes(field(value, "state") as WorkState) &&
    (under === undefined || (Array.isArray(under) && under.every(isText)))
  );
}

const isOperations = (value: unknown): value is WorkOperation[] =>
  Array.isArray(value) && value.every(isOperation);

function isMachine(value: unknown): value is MachineObservation {
  return (
    isText(field(value, "machine_id")) &&
    isText(field(value, "label")) &&
    FRESHNESS.includes(field(value, "freshness") as WorkFreshness) &&
    typeof field(value, "complete") === "boolean" &&
    isOperations(field(value, "operations"))
  );
}

/**
 * The machines' work as this client reads it, or none for a value without what the board places
 * and draws: each machine's identity, freshness and operations, and the operations no machine
 * vouches for. A freshness missing reads as unknown, as serde's default does.
 */
export function readWork(value: unknown): MachineWork | null {
  const freshness = field(value, "freshness") ?? "unknown";
  const machines = field(value, "machines");
  return FRESHNESS.includes(freshness as WorkFreshness) &&
    Array.isArray(machines) &&
    machines.every(isMachine) &&
    isOperations(field(value, "unassigned"))
    ? ({
        ...(value as MachineWork),
        freshness: freshness as WorkFreshness,
        guidance: (field(value, "guidance") as MachineWork["guidance"] | undefined) ?? {},
      } as MachineWork)
    : null;
}

/** The operation is `slug`'s work: its own task's, or a task's under `slug`. */
export function isFor(operation: WorkOperation, slug: string): boolean {
  return operation.task === slug || (operation.under ?? []).includes(slug);
}

/** Whether two reports of an operation say the same, every field of it alike. */
function same(a: WorkOperation, b: WorkOperation): boolean {
  const optional = (value: unknown) => value ?? null;
  return (
    (
      [
        "machine_id",
        "operation_id",
        "parent_id",
        "handle",
        "task",
        "checkout",
        "recipe",
        "kind",
        "state",
        "resource",
        "reason",
        "phase_since",
      ] as const
    ).every((key) => optional(a[key]) === optional(b[key])) &&
    JSON.stringify(a.under ?? []) === JSON.stringify(b.under ?? [])
  );
}

/**
 * Whether another report of `operation` (the same machine and id) says otherwise, among every
 * machine's.
 */
export function conflicts(work: MachineWork, operation: WorkOperation): boolean {
  return work.machines.some((machine) =>
    machine.operations.some(
      (other) =>
        (other.machine_id ?? null) === (operation.machine_id ?? null) &&
        (other.operation_id ?? null) === (operation.operation_id ?? null) &&
        !same(other, operation),
    ),
  );
}

/**
 * Whether `operation`, as `machine` reports it, can place a task in Build or Test: current
 * evidence on a fresh or partial machine, recorded by that machine, with an operation id, of a
 * build or a test, which no other report of the same operation contradicts.
 */
export function canPlace(
  work: MachineWork,
  machine: MachineObservation,
  operation: WorkOperation,
): boolean {
  return (
    (machine.freshness === "fresh" || machine.freshness === "incomplete") &&
    operation.machine_id === machine.machine_id &&
    !(operation.operation_id === null || operation.operation_id === undefined) &&
    operation.operation_id !== "" &&
    (operation.kind === "build" || operation.kind === "test") &&
    !conflicts(work, operation)
  );
}

/**
 * The stage `task` stands in on the board: its lifecycle status, or, for a task in progress while
 * the feed is open, Build or Test when a machine's current work for it says so. Running work wins
 * over queued work, then Test over Build.
 */
export function taskStage(
  work: MachineWork,
  task: Pick<Task, "slug" | "status">,
  connected: boolean,
): string {
  if (!connected || task.status !== "in_progress") return task.status;
  let stage = task.status;
  let priority = 0;
  for (const machine of work.machines) {
    for (const operation of machine.operations) {
      if (!isFor(operation, task.slug) || !canPlace(work, machine, operation)) continue;
      // Queued Build < queued Test < running Build < running Test.
      const candidate =
        1 + 2 * Number(operation.state === "running") + Number(operation.kind === "test");
      if (candidate > priority) {
        priority = candidate;
        stage = operation.kind === "test" ? "test" : "build";
      }
    }
  }
  return stage;
}

/**
 * Whether `other` puts every task in the stage `work` puts it in: the same operations place tasks,
 * in the same order, each for the same task and the tasks it is under, of the same kind, running
 * or queued alike. A heartbeat moves nothing on a board, so it reads its counts and columns again
 * only when this is false.
 */
export function placesAlike(work: MachineWork, other: MachineWork): boolean {
  const placing = (one: MachineWork) =>
    JSON.stringify(
      one.machines.flatMap((machine) =>
        machine.operations
          .filter((operation) => canPlace(one, machine, operation))
          .map((operation) => [
            operation.task ?? null,
            operation.under ?? [],
            operation.kind === "test",
            operation.state === "running",
          ]),
      ),
    );
  return placing(work) === placing(other);
}

/**
 * Whether two reports of the machines' work draw alike: the same but for what the page never shows
 * of a machine, its producer and its report's sequence and age, which its heartbeat changes each
 * second.
 */
export function drawsAlike(work: MachineWork, other: MachineWork): boolean {
  const shown = (one: MachineWork) =>
    JSON.stringify({
      ...one,
      machines: one.machines.map(({ producer_id, sequence, age_ms, ...machine }) => machine),
    });
  return work === other || shown(work) === shown(other);
}

/** One build or test a card shows: its operation, the machine it is on, and how sure that is. */
export interface Phase {
  operation: WorkOperation;
  machine: string;
  freshness: WorkFreshness;
  /** Whether the machine that reports it vouches for it, its own id on it. */
  proven: boolean;
}

/** How a phase's freshness reads. */
const FRESHNESS_WORDS: Record<WorkFreshness, string> = {
  fresh: "fresh",
  stale: "stale",
  disconnected: "disconnected",
  incomplete: "partial inventory",
  conflict: "conflicting evidence",
  unknown: "unknown",
};

export function freshnessWord(freshness: WorkFreshness): string {
  return FRESHNESS_WORDS[freshness];
}

/**
 * Whose builds and tests a view shows: a task's (its own and its subtasks'), an agent's, a
 * machine's, or every operation no machine vouches for (`unassigned`).
 */
export type Scope = { task: string } | { agent: string } | { machine: string } | "unassigned";

/** Whether `operation` is `scope`'s. */
function inScope(scope: Scope, operation: WorkOperation): boolean {
  if (scope === "unassigned") return true;
  if ("task" in scope) return isFor(operation, scope.task);
  if ("agent" in scope) return operation.handle === scope.agent;
  return operation.machine_id === scope.machine;
}

/**
 * The builds and tests of `slug`'s task, its own and its subtasks', in the order the machines
 * report them: one per operation a machine vouches for (a second report of it marks it
 * conflicting when it says otherwise), each it does not vouch for apart.
 */
export function phases(work: MachineWork, slug: string): Phase[] {
  return scopedPhases(work, { task: slug });
}

/**
 * The builds and tests of `scope`, as `phases` finds a task's; the unassigned scope's are only the
 * operations no machine vouches for.
 */
export function scopedPhases(work: MachineWork, scope: Scope): Phase[] {
  const found: Phase[] = [];
  const append = (
    operation: WorkOperation,
    machine: string,
    freshness: WorkFreshness,
    proven: boolean,
  ) => {
    if (!inScope(scope, operation) || (operation.kind !== "build" && operation.kind !== "test")) {
      return;
    }
    if (proven && operation.operation_id !== null && operation.operation_id !== undefined) {
      const previous = found.find(
        (one) =>
          one.proven &&
          (one.operation.machine_id ?? null) === (operation.machine_id ?? null) &&
          one.operation.operation_id === operation.operation_id,
      );
      if (previous) {
        if (!same(previous.operation, operation)) previous.freshness = "conflict";
        return;
      }
    }
    found.push({ operation, machine, freshness, proven });
  };
  for (const machine of scope === "unassigned" ? [] : work.machines) {
    for (const operation of machine.operations) {
      if (operation.machine_id !== machine.machine_id) {
        append(operation, "Machine unknown", "conflict", false);
      } else {
        const observed =
          !machine.complete && machine.freshness === "fresh" ? "incomplete" : machine.freshness;
        append(operation, machine.label, observed, true);
      }
    }
  }
  for (const operation of work.unassigned) append(operation, "Machine unknown", "unknown", false);
  return found;
}

/** What a phase's chip says. */
export function phaseLabel(phase: Phase): string {
  const kind = phase.operation.kind === "build" ? "Build" : "Test";
  const state = phase.operation.state;
  return phase.freshness === "fresh"
    ? `${kind} ${state} · ${phase.machine}`
    : `Observed ${kind} ${state} · ${phase.machine} · ${freshnessWord(phase.freshness)}`;
}

/** The phases `slug`'s card shows: a machine's own read as disconnected while the feed is. */
export function shownPhases(work: MachineWork, slug: string, connected: boolean): Phase[] {
  return shownScoped(work, { task: slug }, connected);
}

/** The phases `scope` shows, as `shownPhases` shows a task's. */
export function shownScoped(work: MachineWork, scope: Scope, connected: boolean): Phase[] {
  const rows = scopedPhases(work, scope);
  return connected
    ? rows
    : rows.map((row) => (row.proven ? { ...row, freshness: "disconnected" } : row));
}

/** Whether two lists of phases draw the same chips: each the same words, look and title. */
export function samePhases(a: Phase[], b: Phase[]): boolean {
  const face = (phase: Phase) => {
    const { kind, state, recipe, resource, reason } = phase.operation;
    return [phaseLabel(phase), phase.freshness, kind, state, recipe, resource, reason].join("\n");
  };
  return a.length === b.length && a.every((phase, at) => face(phase) === face(b[at] as Phase));
}
