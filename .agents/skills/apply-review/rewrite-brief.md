# Brief: rewrite design documents to match the owner's verdicts

Work in the assigned project checkout. Read its AGENTS.md, local rules and review conventions first.

The owner reviews the design direction as one-line decision cards, each at the place in its document where the thing it decides is said (the register, `docs/DECISIONS.md` by convention, holds those with no such place): `[x]` approved, `[-]` rejected, `[?]` question, `[r]` resolved (approved, nothing left to answer), `[ ]` not yet answered. Under each card is its dated history: `[date] owner · <verdict> — <comment>` lines are the owner's words; `[date] agent — …` lines are agent replies. Cards marked `[ ]` are proposals, often written by an agent from the owner's comments; they are not approved.

The owner's standing instruction: **documents the owner disagreed with must be REWRITTEN, not pointed at the register.** Your job is to rewrite the files assigned to you so they agree with the verdicts, and read as current direction to someone who never saw the old version.

How to treat each verdict:
- **Approved** (`[x]`): binding direction. Keep it, stated plainly. If the owner left a note on an approved card, the note refines it and the document must say the refined version.
- **Rejected** with an owner note: the owner's note IS the new direction; write the document to it in the owner's sense. Where the owner only said what is wrong, remove or reframe the rejected content.
- **Rejected with no note**: remove the rejected content; say in one line it is undecided.
- **Question** (`[?]`) and the agent reply under it: rewrite the passage so it answers the confusion in plain words; where it remains undecided, say so in one line.
- **Pending `[ ]` cards**: proposals awaiting the owner. Where your document needs something to replace rejected content and the owner's note did not supply it, state the pending card's proposal as **"Proposed, awaiting owner decision (`slug`)"** in one short line. Never present it as decided. Do not invent further proposals beyond the pending cards.
- **Content the register never covered**: keep it only if it does not contradict any verdict.

Writing rules:
- **Remove repetition and keep the length appropriate to the project's audience.** Keep direction and the reasons that matter; cut hedging, repeated caveats ("this is not approved…"), meta-commentary about review process, and process history. Follow the project's policy for current guidance and historical evidence; keep process history out of current instructions.
- Plain words. Avoid jargon such as "grammar", "departure", "cell", "lineage allocation" unless defined in one line.
- Follow the project's document metadata conventions. Only the owner approves a card.
- Keep headings that a card links to via `[[FILE#Heading]]` if the section still exists; if you rename or remove a linked heading, list it in your report so the links can be fixed. Do NOT edit a card yourself: a card's line and the indented lines under it stay exactly as they are and where they are, in your files too, with the passage each decides kept above it.
- Keep valid relative links to images when the images still illustrate current direction; drop links to superseded studies unless labelled as such in one line. A picture a document shows is a file committed beside it under `docs/`, so a clone shows it: one linked from the ignored `artifacts/` folder is copied there and committed, never linked in place. `just symphony-cli docs-audit --check` refuses a picture your lines show that git does not track.
- Do not touch files outside your assignment. Other agents are rewriting other docs in parallel.
- Run only the checks assigned by the task and destination project. Preserve other sessions' active commands and follow local shared-service rules. Do not invent a verifier or landing command. Owner callouts remain the owner's; replies follow the project's conventions.
- Commit, delegate or land only within the assignment's authorization and project workflow.

Report back: for each file, old and new word count (`wc -w`), the verdicts you applied (by slug), any heading linked from a card that you renamed/removed, anything you deliberately left out, and any contradiction between verdicts you found (state it; don't resolve it by choosing).
