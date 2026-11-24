// Whose a card or a task is, as this client reads it where the read arrives: a task's with its page
// (task-read.ts), a document's cards' with their own read (owner.tsx). Its own module, apart from
// the line that draws it, so a task's page reads it without that line, a chunk it imports for a
// task its board names an owner of alone (task-page.tsx's `OwnerPart`).

import { field } from "./shared";
import type { Owner } from "./wire";

const isText = (value: unknown): value is string => typeof value === "string";

/** Whose a thing is, or none: as serde reads an `Owner`, its `hand_to` empty when absent. */
export function readOwner(value: unknown): Owner | null {
  const names = field(value, "hand_to") ?? [];
  if (
    !["name", "kind", "by", "said"].every((key) => isText(field(value, key))) ||
    !Array.isArray(names) ||
    !names.every(isText)
  ) {
    return null;
  }
  return {
    name: field(value, "name") as string,
    kind: field(value, "kind") as string,
    by: field(value, "by") as string,
    said: field(value, "said") as string,
    hand_to: names as string[],
  };
}
