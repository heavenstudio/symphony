// The name a person's line on a card carries (docs/product/analysis/users-and-identity.md, "How a
// person's name is written"): `[ts] ana · approved — A` reads as a thread entry of a person's
// (`who: "owner"`), whose first block is a paragraph of one span classed `entry-by` holding the
// name, as the server draws it (`crates/symphony/src/render/cards.rs`), since the kit's entry has
// no field for one. The owner's and an agent's entries carry no name but their class.

import type { Block, ThreadEntry } from "./wire";

/** The class of the span that holds a person's name. */
const BY = "entry-by";

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
