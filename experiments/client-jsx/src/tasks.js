// What the Dioxus client takes from the Rust wire crate (`symphony_api`) and the JSX side has to
// keep in step by hand: the board's stage of a task, the pipeline's order and the slug order the
// server pages a column by.

export const PIPELINE = ["concept", "design", "ready", "in_progress", "build", "test", "review", "done"];
export const OFFLINE = ["backlog", "blocked", "cancelled"];

/** `TaskStage::task_stage`: the build or test the machine's fresh work places an in-progress task in. */
export function taskStage(work, task, connected) {
  if (!connected || task.status !== "in_progress") return task.status;
  let stage = task.status;
  let priority = 0;
  const machines = work?.machines ?? [];
  const conflicts = (operation) =>
    machines.some((machine) =>
      machine.operations.some(
        (other) =>
          other.machine_id === operation.machine_id &&
          other.operation_id === operation.operation_id &&
          JSON.stringify(other) !== JSON.stringify(operation),
      ),
    );
  for (const machine of machines) {
    if (machine.freshness !== "fresh" && machine.freshness !== "incomplete") continue;
    for (const operation of machine.operations) {
      const forTask = operation.task === task.slug || (operation.under ?? []).includes(task.slug);
      if (
        operation.machine_id !== machine.machine_id ||
        !forTask ||
        !operation.operation_id ||
        (operation.kind !== "build" && operation.kind !== "test") ||
        conflicts(operation)
      )
        continue;
      const candidate = 1 + 2 * (operation.state === "running" ? 1 : 0) + (operation.kind === "test" ? 1 : 0);
      if (candidate > priority) {
        priority = candidate;
        stage = operation.kind === "test" ? "test" : "build";
      }
    }
  }
  return stage;
}

const isDigit = (c) => c >= 48 && c <= 57;

/** `slug_order`: byte order, but a run of digits compares as its number (`ready-3` < `ready-30`). */
export function slugOrder(a, b) {
  let i = 0;
  let j = 0;
  while (true) {
    if (i >= a.length && j >= b.length) return a < b ? -1 : a > b ? 1 : 0;
    if (i >= a.length) return -1;
    if (j >= b.length) return 1;
    const x = a.charCodeAt(i);
    const y = b.charCodeAt(j);
    if (isDigit(x) && isDigit(y)) {
      let ei = i;
      while (ei < a.length && isDigit(a.charCodeAt(ei))) ei++;
      let ej = j;
      while (ej < b.length && isDigit(b.charCodeAt(ej))) ej++;
      const left = a.slice(i, ei).replace(/^0+/, "");
      const right = b.slice(j, ej).replace(/^0+/, "");
      if (left.length !== right.length) return left.length - right.length;
      if (left !== right) return left < right ? -1 : 1;
      i = ei;
      j = ej;
    } else if (x !== y) {
      return x - y;
    } else {
      i++;
      j++;
    }
  }
}

export const byPriority = (a, b) => a.priority - b.priority || slugOrder(a.slug, b.slug);
export const byRecent = (a, b) => b.updated - a.updated || slugOrder(a.slug, b.slug);

/** `tasks::merge`: a `task-fields` change merged into its task. */
export function mergeFields(task, fields) {
  const next = { ...task };
  for (const name of ["title", "body", "status", "links", "depends", "roles", "priority", "updated", "instruction"]) {
    if (fields[name] !== undefined && fields[name] !== null) next[name] = fields[name];
  }
  if (fields.parent !== undefined && fields.parent !== null) next.parent = fields.parent === "" ? null : fields.parent;
  return next;
}

export const statusWords = (status) => status.replaceAll("_", " ");
