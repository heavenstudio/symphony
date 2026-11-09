# Brief: rewrite design documents to match the owner's verdicts

Work in the assigned project checkout. Read its AGENTS.md, local rules and review conventions first.

The owner reviews the design direction as one-line decision cards in `docs/DECISIONS.md`: `[x]` approved, `[-]` rejected, `[?]` question, `[r]` resolved (approved, nothing left to answer), `[ ]` not yet answered. Under each card is its dated history: `[date] owner · <verdict> — <comment>` lines are the owner's words; `[date] agent — …` lines are agent replies. Cards marked `[ ]` are proposals, often written by an agent from the owner's comments; they are not approved.

The owner's standing instruction: **documents the owner disagreed with must be REWRITTEN, not pointed at the register.** Your job is to rewrite the files assigned to you so they agree with the verdicts, and read as current direction to someone who never saw the old version.

How to treat each verdict:
- **Approved** (`[x]`): binding direction. Keep it, stated plainly. If the owner left a note on an approved card, the note refines it and the document must say the refined version.
- **Rejected** with an owner note: the owner's note IS the new direction; write the document to it in the owner's sense. Where the owner only said what is wrong, remove or reframe the rejected content.
- **Rejected with no note**: remove the rejected content; say in one line it is undecided.
- **Question** (`[?]`) and the agent reply under it: rewrite the passage so it answers the confusion in plain words; where it remains undecided, say so in one line.
- **Pending `[ ]` cards**: proposals awaiting the owner. Where your document needs something to replace rejected content and the owner's note did not supply it, state the pending card's proposal as **"Proposed, awaiting owner decision (`slug`)"** in one short line. Never present it as decided. Do not invent further proposals beyond the pending cards.
- **Content the register never covered**: keep it only if it does not contradict any verdict.

Writing rules:
- The owner found these documents far too long to review. **Make them substantially shorter** — aim for well under half the current length. Keep direction and the reasons that matter; cut hedging, repeated caveats ("this is not approved…"), meta-commentary about review process, and process history. Per `history-is-not-reference`, a rejected approach keeps at most one line; do not create history files unless something is genuinely worth keeping, and then put it under `docs/history/` with a one-line pointer.
- Plain words. Avoid jargon such as "grammar", "departure", "cell", "lineage allocation" unless defined in one line.
- Documents have no status or frontmatter; add none. Only the owner approves a card.
- Keep headings that `docs/DECISIONS.md` links to via `[[FILE#Heading]]` if the section still exists; if you rename or remove a linked heading, list it in your report so the links can be fixed. Do NOT edit `docs/DECISIONS.md` yourself.
- Keep valid relative links to images under `artifacts/` when the images still illustrate current direction; drop links to superseded studies unless labelled as such in one line.
- Do not touch files outside your assignment. Other agents are rewriting other docs in parallel.
- Do not run `just verify` (the full fast plan runs on the tip at the move to review, and on main after the landing). You may run `just symphony-status` and `just symphony-status --open` to check you did not create malformed callouts. Do not write `[!like]`/`[!dislike]`/`[!question]`/`[!decision]` callouts: those are the owner's.
- Do not commit.

Report back: for each file, old and new word count (`wc -w`), the verdicts you applied (by slug), any heading linked from DECISIONS.md that you renamed/removed, anything you deliberately left out, and any contradiction between verdicts you found (state it; don't resolve it by choosing).
