// A task's kind, as the board keeps it among the task's own fields (`fields._board.kind`, crates/
// tasks's `kind`): one word of a short list, chosen when the task is filed or given to it after. A
// task may have none. Its own module, so the task page reads it without the Reports page's charts.

import { field } from "./shared";

/** The kinds a task may be given, in the order a list names them (crates/tasks's `kind::KINDS`). */
export const KINDS = ["feature", "fix", "red", "tests", "docs"] as const;

/** Whether `kind` is one of KINDS. */
export function isKind(kind: unknown): kind is (typeof KINDS)[number] {
  return typeof kind === "string" && (KINDS as readonly string[]).includes(kind);
}

/** The kind a task's board fields name (`fields._board.kind`); none for none or any other word. */
export function taskKind(fields: unknown): string | null {
  const kind = field(field(fields, "_board"), "kind");
  return isKind(kind) ? kind : null;
}
