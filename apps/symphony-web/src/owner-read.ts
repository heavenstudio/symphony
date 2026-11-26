// Whose a card or a task is, as this client reads it where the read arrives: a task's with its page
// (task-read.ts), a document's cards' with their own read (owner.tsx). Its own module, apart from
// the line that draws it, so a task's page reads it without that line, a chunk it imports for a
// task its board names an owner of alone (task-page.tsx's `OwnerPart`), and tells whether its
// reader decides the thing ([`decides`]) as it draws the controls that do.

import { field } from "./shared";
import type { Owner } from "./wire";

const isText = (value: unknown): value is string => typeof value === "string";

/**
 * Whose a thing is, or none: as serde reads an `Owner`, its `hand_to` empty and `decides` false
 * when absent.
 */
export function readOwner(value: unknown): Owner | null {
  const names = field(value, "hand_to") ?? [];
  const decides = field(value, "decides") ?? false;
  if (
    !["name", "kind", "by", "said"].every((key) => isText(field(value, key))) ||
    !Array.isArray(names) ||
    !names.every(isText) ||
    typeof decides !== "boolean"
  ) {
    return null;
  }
  return {
    name: field(value, "name") as string,
    kind: field(value, "kind") as string,
    by: field(value, "by") as string,
    said: field(value, "said") as string,
    hand_to: names as string[],
    decides,
  };
}

/**
 * Whether the one reading decides a thing whose owner the server said is `owner`: always where it
 * said none, on a board that names nobody, as before there were groups.
 */
export function decides(owner: Owner | null | undefined): boolean {
  return owner === null || owner === undefined || owner.decides;
}
