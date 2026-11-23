// A task as the board, the agents page and a task's page read it: the guards of the reads that
// bring tasks (a listing, a task the feed adds or changes), the order the board lists them in, and
// how a change the feed sends lands in a task already drawn.
//
// `slugOrder` and `isTask` carry crates/symphony-api's own (`slug_order`, `Task::is_task` with its
// `kind`), held to the Rust by the cases it writes (wire.cases.json, task.test.ts).

import { field } from "./shared";
import type {
  ItemChange,
  Orchestrator,
  Role,
  Task,
  TaskChange,
  TaskFields,
  TaskListing,
  WaitsOn,
} from "./wire";

const isText = (value: unknown): value is string => typeof value === "string";
const isNumber = (value: unknown): value is number => typeof value === "number";
const isReviewCount = (value: unknown): value is number =>
  isNumber(value) && Number.isInteger(value) && value >= 0 && value <= 0xffffffff;
const isTexts = (value: unknown): value is string[] => Array.isArray(value) && value.every(isText);
/** As serde reads an `Option`: absent and `null` are none. */
const absent = (value: unknown) => value === undefined || value === null;
/** A task's place in a custom workflow, as its card places and names it: its phase and state. */
const isPlace = (value: unknown) => isText(field(value, "phase")) && isText(field(value, "state"));

function isRole(value: unknown): value is Role {
  return ["handle", "role", "health"].every((key) => isText(field(value, key)));
}

const isRoles = (value: unknown): value is Role[] => Array.isArray(value) && value.every(isRole);

/** What a task waits on, as the server says it: its kind, who, why, and since when. */
function isWaitsOn(value: unknown): value is WaitsOn {
  return (
    ["kind", "who", "why"].every((key) => isText(field(value, key))) &&
    isNumber(field(value, "since"))
  );
}

function isOrchestrator(value: unknown): value is Orchestrator {
  return ["handle", "health"].every((key) => isText(field(value, key)));
}

/**
 * A task as this client reads it, or none for a value without what a card draws: its slug, title,
 * status, priority, parent, when it was updated, its links and its roles. The rest (its brief, its
 * plugins' board fields) is the server's to send or leave out.
 */
export function readTask(value: unknown): Task | null {
  return ["slug", "title", "status"].every((key) => isText(field(value, key))) &&
    isNumber(field(value, "priority")) &&
    isNumber(field(value, "updated")) &&
    (absent(field(value, "parent")) || isText(field(value, "parent"))) &&
    isTexts(field(value, "links")) &&
    isRoles(field(value, "roles")) &&
    (absent(field(value, "review_rounds")) || isReviewCount(field(value, "review_rounds"))) &&
    (absent(field(value, "waits_on")) || isWaitsOn(field(value, "waits_on"))) &&
    (absent(field(value, "from")) || isText(field(value, "from"))) &&
    (absent(field(value, "workflow")) || isPlace(field(value, "workflow")))
    ? { ...(value as Task), parent: (field(value, "parent") as string | undefined) ?? null }
    : null;
}

/** Counts by status, as the server lists them: `[status, count]` pairs. */
function isCounts(value: unknown): value is [string, number][] {
  return (
    Array.isArray(value) &&
    value.every((pair) => Array.isArray(pair) && isText(pair[0]) && isNumber(pair[1]))
  );
}

/**
 * A listing of the task board (`/api/task-board`) as this client reads it, or none: its tasks,
 * each as `readTask` reads it, and what a missing list of statuses, sign-offs, counts or
 * orchestrators, or a missing count of tasks waiting to start, reads as (none), as serde's
 * defaults do.
 */
export function readListing(value: unknown): TaskListing | null {
  const tasks = field(value, "tasks");
  if (!Array.isArray(tasks)) return null;
  const read = tasks.map(readTask);
  if (read.some((task) => task === null)) return null;
  const listed = (key: string, guard: (one: unknown) => boolean) => {
    const found = field(value, key);
    return found === undefined || guard(found);
  };
  if (
    !(
      absent(field(value, "waiting")) ||
      (isCounts(field(field(value, "waiting"), "kinds")) &&
        Array.isArray(field(field(value, "waiting"), "columns")) &&
        (field(field(value, "waiting"), "columns") as unknown[]).every(
          (column) =>
            isText(field(column, "status")) &&
            isNumber(field(column, "ordinary")) &&
            isNumber(field(column, "blocked")) &&
            (absent(field(column, "oldest")) || isNumber(field(column, "oldest"))),
        ))
    ) ||
    !listed("statuses", isTexts) ||
    !listed("signoff", isTexts) ||
    !listed("counts", isCounts) ||
    !listed("lifecycle_counts", isCounts) ||
    !listed("orchestrators", (one) => Array.isArray(one) && one.every(isOrchestrator)) ||
    !listed("unstarted", isNumber) ||
    !(absent(field(value, "next")) || isText(field(value, "next")))
  ) {
    return null;
  }
  return {
    ...(value as TaskListing),
    tasks: read as Task[],
    statuses: (field(value, "statuses") as string[] | undefined) ?? [],
    signoff: (field(value, "signoff") as string[] | undefined) ?? [],
    counts: (field(value, "counts") as [string, number][] | undefined) ?? [],
    lifecycle_counts: (field(value, "lifecycle_counts") as [string, number][] | undefined) ?? [],
    next: (field(value, "next") as string | undefined) ?? null,
    orchestrators: (field(value, "orchestrators") as Orchestrator[] | undefined) ?? [],
    unstarted: (field(value, "unstarted") as number | undefined) ?? 0,
  };
}

/** The fields of a task the feed says changed, each of its type, or none for one that is not. */
const FIELDS: [keyof TaskFields, (value: unknown) => boolean][] = [
  ["title", isText],
  ["body", isText],
  ["status", isText],
  ["priority", isNumber],
  ["parent", isText],
  ["updated", isNumber],
  ["links", isTexts],
  ["depends", isTexts],
  ["roles", isRoles],
  ["instruction", (value) => typeof value === "boolean"],
  ["review_rounds", isReviewCount],
  ["waits_on", isWaitsOn],
  ["from", isText],
  ["workflow", isPlace],
];

/** A task's changed fields as the feed's `task-fields` sends them, or none for what is not. */
export function readTaskChange(value: unknown): TaskChange | null {
  const fields = field(value, "fields");
  if (!isText(field(value, "key")) || typeof fields !== "object" || fields === null) return null;
  for (const [name, guard] of FIELDS) {
    const one = field(fields, name);
    if (!absent(one) && !guard(one)) return null;
  }
  return value as TaskChange;
}

/** A task the feed's `task` region adds, replaces or removes, or none for what is not. */
export function readItemChange(value: unknown): ItemChange<Task> | null {
  const task = field(value, "value");
  const removed = field(value, "removed");
  if (!isText(field(value, "key")) || !(removed === undefined || typeof removed === "boolean")) {
    return null;
  }
  if (absent(task)) return { key: field(value, "key") as string, removed: removed === true };
  const read = readTask(task);
  return read === null
    ? null
    : { key: field(value, "key") as string, value: read, removed: removed === true };
}

/** Whether `task` is a deliverable task rather than a slice of one: it has no parent. */
export function isTask(task: Pick<Task, "parent">): boolean {
  return task.parent === null || task.parent === undefined;
}

const utf8 = new TextEncoder();

/** `a` against `b`, byte by byte, as Rust compares two strings: -1, 0 or 1. */
function bytes(a: Uint8Array, b: Uint8Array): number {
  const shorter = Math.min(a.length, b.length);
  for (let at = 0; at < shorter; at += 1) {
    const one = a[at] as number;
    const other = b[at] as number;
    if (one !== other) return one < other ? -1 : 1;
  }
  return Math.sign(a.length - b.length);
}

/** `a` against `b` as Rust orders two strings, by their UTF-8 bytes: -1, 0 or 1. */
export function textOrder(a: string, b: string): number {
  return bytes(utf8.encode(a), utf8.encode(b));
}

const isDigit = (byte: number | undefined) => byte !== undefined && byte >= 0x30 && byte <= 0x39;

/** Where the run of digits starting at `at` in `text` ends. */
function digitsEnd(text: Uint8Array, at: number): number {
  let end = at;
  while (isDigit(text[end])) end += 1;
  return end;
}

/** Where the run of digits from `at` to `end` starts once its leading zeros are left out. */
function zerosEnd(text: Uint8Array, at: number, end: number): number {
  let start = at;
  while (start < end && text[start] === 0x30) start += 1;
  return start;
}

/**
 * The order slugs are listed in: as text, but a run of digits by its number, so `ready-3` comes
 * before `ready-30`; slugs whose numbers are equal but spelled apart keep their order as text, so
 * only equal slugs are equal. -1, 0 or 1, as `a` comes before, with or after `b`.
 */
export function slugOrder(a: string, b: string): number {
  const left = utf8.encode(a);
  const right = utf8.encode(b);
  let i = 0;
  let j = 0;
  for (;;) {
    const one = left[i];
    const other = right[j];
    if (one === undefined && other === undefined) return bytes(left, right);
    if (one === undefined) return -1;
    if (other === undefined) return 1;
    if (isDigit(one) && isDigit(other)) {
      const ends = digitsEnd(left, i);
      const otherEnds = digitsEnd(right, j);
      const value = left.subarray(zerosEnd(left, i, ends), ends);
      const otherValue = right.subarray(zerosEnd(right, j, otherEnds), otherEnds);
      const order = Math.sign(value.length - otherValue.length) || bytes(value, otherValue);
      if (order !== 0) return order;
      i = ends;
      j = otherEnds;
    } else if (one !== other) {
      return one < other ? -1 : 1;
    } else {
      i += 1;
      j += 1;
    }
  }
}

/** Open work: the most urgent first, then by slug, as the server pages a column. */
export function byPriority(a: Task, b: Task): number {
  return Math.sign(a.priority - b.priority) || slugOrder(a.slug, b.slug);
}

/** Finished work: the most recently updated first, then by slug as open work is. */
export function byRecent(a: Task, b: Task): number {
  return Math.sign(b.updated - a.updated) || slugOrder(a.slug, b.slug);
}

/** `task` with `fields` changed, the feed's `task-fields`: a parent of none (`""`) clears it. */
export function merge(task: Task, fields: TaskFields): Task {
  const next = { ...task };
  for (const name of [
    "title",
    "body",
    "status",
    "links",
    "depends",
    "roles",
    "priority",
    "updated",
    "instruction",
    "review_rounds",
    "workflow",
  ] as const) {
    const value = fields[name];
    if (value !== undefined && value !== null) (next as Record<string, unknown>)[name] = value;
  }
  if (fields.parent !== undefined && fields.parent !== null) {
    next.parent = fields.parent === "" ? null : fields.parent;
  }
  // What it waits on of no kind, and a `from` of no status, clear each.
  if (fields.waits_on !== undefined && fields.waits_on !== null) {
    next.waits_on = fields.waits_on.kind === "" ? null : fields.waits_on;
  }
  if (fields.from !== undefined && fields.from !== null) {
    next.from = fields.from === "" ? null : fields.from;
  }
  return next;
}

/**
 * `tasks` with each role `handle` holds saying `health`: the feed sends a holder's health once, as
 * its session (none, a session gone, is `lost`). Whether any changed, and the tasks.
 */
export function holderHealth(
  tasks: Task[],
  handle: string,
  health: string | null,
): [boolean, Task[]] {
  const said = health ?? "lost";
  let changed = false;
  const next = tasks.map((task) => {
    if (!task.roles.some((role) => role.handle === handle && role.health !== said)) return task;
    changed = true;
    return {
      ...task,
      roles: task.roles.map((role) =>
        role.handle === handle && role.health !== said ? { ...role, health: said } : role,
      ),
    };
  });
  return [changed, next];
}
