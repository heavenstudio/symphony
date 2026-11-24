// Whose a card or a task is (docs/workflow/TASKS.md, "Which group a thing belongs to"): its group
// or person and what gave it, as the server said it to the one reading (`Owner`), and for one who
// may hand it on, whom to. A task's comes with its page (`TaskPage.owner`); a document's cards'
// with a read of their own (`CARD_OWNERS_PATH`), read again as the document moves. A board that
// names nobody says none, and nothing is drawn. The select is drawn only where the server named
// someone to hand it to, and a thing's deciding controls (a card's verdict, a task's priority,
// cancel and approve) only where it says the reader decides it (`decides`, [`decides`]): a control
// that could not act is left out.

import { createContext, useState } from "react";
import { useMount } from "./mount";
import { said } from "./request";
import { SelectField } from "./select";
import { field, STALE, useStore } from "./shared";
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

/** A document's cards' owners by slug, or none: as serde reads a `CardOwners`. */
export function readCardOwners(value: unknown): Record<string, Owner> | null {
  const cards = field(value, "cards");
  if (typeof cards !== "object" || cards === null || Array.isArray(cards)) return null;
  const read: Record<string, Owner> = {};
  for (const [slug, owner] of Object.entries(cards)) {
    const one = readOwner(owner);
    if (one === null) return null;
    read[slug] = one;
  }
  return read;
}

/** The owners of the document page's cards by slug, and how a card is handed on from it. */
export interface CardOwnersValue {
  owners: Record<string, Owner>;
  hand: (slug: string, to: string) => Promise<unknown>;
}

/** The document page's cards' owners: none where the page read none. */
export const CardOwnersContext = createContext<CardOwnersValue | null>(null);

/** How a kind of owner is labelled: a group, else a person (the person at the machine too). */
export function ownerLabel(owner: Owner): string {
  return owner.kind === "group" ? "group" : "person";
}

/** What handing a thing to `to` by `hand` says once it is over: that it was, or its refusal. */
export async function handOver(
  to: string,
  hand: (to: string) => Promise<unknown>,
): Promise<string> {
  try {
    await hand(to);
    return `Handed to ${to}`;
  } catch (error) {
    return said(error);
  }
}

export interface OwnerLineProps {
  owner: Owner;
  /** The select's id, one per page. */
  id: string;
  /** Hands the thing to the name chosen: its refusal is said beside it. */
  hand: (to: string) => Promise<unknown>;
}

/**
 * Whose a thing is and what gave it, then for one who may hand it on, a select of whom to. A
 * hand-over waits while the page may show what is out of date.
 */
export function OwnerLine({ owner, id, hand }: OwnerLineProps) {
  const mount = useMount();
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState("");
  const stale = useStore(STALE);
  const names = owner.hand_to ?? [];
  const choose = async (to: string) => {
    if (to === "" || busy || STALE.get()) return;
    setBusy(true);
    setResult("");
    const answer = await handOver(to, hand);
    if (mount.aborted) return;
    setResult(answer);
    setBusy(false);
  };
  return (
    <span className="owner" data-by={owner.by}>
      <span className="owner-name">{`${ownerLabel(owner)}: ${owner.name}`}</span>
      <small className="owner-by">{owner.said}</small>
      {names.length > 0 && (
        <SelectField
          id={id}
          label="Hand to"
          value=""
          disabled={busy || stale}
          onChange={(to) => void choose(to)}
          options={[["", "Hand to…"], ...names.map((name): [string, string] => [name, name])]}
        />
      )}
      {result !== "" && <span role="status">{result}</span>}
    </span>
  );
}
