// The name a person's line on a card carries (docs/product/analysis/users-and-identity.md, "How a
// person's name is written"): `[ts] ana · approved — A` reads as a thread entry of a person's
// (`who: "owner"`), whose first block is a paragraph of one span classed `entry-by` holding the
// name, as the server draws it (`crates/symphony/src/render/cards.rs`), since the kit's entry has
// no field for one. The owner's and an agent's entries carry no name but their class.
//
// A card entry's writer is drawn by the rule every writer a page shows is drawn by
// (`project.ts`'s `drawnName`): the person at the machine, once they gave themselves a name, by how
// they are shown, under `owner` as under their short name.

import { type Drawn, drawnName } from "./project";
import type { Block, PersonName, ThreadEntry } from "./wire";

/** The class of the span that holds a person's name. */
const BY = "entry-by";

/** The word the person at the machine's lines carry until they give themselves a name. */
const OWNER = "owner";

/** `entry`'s name, when its first block is a person's name, and the blocks that follow it; none
 * and every block otherwise. */
export function byline(entry: ThreadEntry): [string | null, Block[]] {
  const [first, ...rest] = entry.blocks;
  const span = first?.type === "paragraph" && first.spans.length === 1 ? first.spans[0] : undefined;
  const text = span?.type === "styled" && span.presentation.class === BY ? span.spans : [];
  return text.length === 1 && text[0]?.type === "text"
    ? [text[0].text, rest]
    : [null, entry.blocks];
}

/**
 * `entry`'s writer as its thread draws them (`drawnName`), and the blocks that are its words: a
 * person's name it carries, else, for a person's line that carries none (the owner's), the person
 * at the machine once they gave themselves a name; none for an agent's, and for the owner's while
 * the person at the machine gave none.
 */
export function entryWriter(
  entry: ThreadEntry,
  machine: PersonName | null | undefined,
): [Drawn | null, Block[]] {
  const [name, blocks] = byline(entry);
  const writer = name ?? (entry.who === "owner" && machine != null ? OWNER : null);
  return [writer === null ? null : drawnName(writer, machine), blocks];
}
