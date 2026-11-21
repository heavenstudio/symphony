# Brief: rewrite design documents to match the owner's verdicts

Work in the assigned project checkout, under its AGENTS.md, local rules and review conventions.

The owner reviews the design direction as one-line decision cards in `docs/DECISIONS.md`: `[x]`
approved, `[-]` rejected, `[?]` question, `[r]` resolved (approved, nothing left to answer),
`[ ]` not yet answered. Under each card is its dated history: `owner` lines are the owner's
words, `agent` lines are agent replies. A `[ ]` card is a proposal, never approved.

Documents the owner disagreed with are rewritten, not pointed at the register: rewrite the files
assigned to you so they agree with the verdicts and read as current direction to someone who
never saw the old version.

How to treat each verdict:

- **Approved**: binding direction, stated plainly. A note on an approved card refines it, and
  the document says the refined version.
- **Rejected with a note**: the note is the new direction; write the document to it in the
  owner's sense. Where the owner only said what is wrong, remove or reframe the rejected content.
- **Rejected with no note**: remove the rejected content, and say in one line it is undecided.
- **Question** and the agent reply under it: rewrite the passage so it answers the confusion in
  plain words; where it remains undecided, say so in one line.
- **Pending `[ ]` cards**: where your document needs something to replace rejected content and
  the owner's note did not supply it, state the pending card's proposal in one short line as
  **"Proposed, awaiting owner decision (`slug`)"**, never as decided, and invent no proposal
  beyond the pending cards.
- **Content the register never covered**: keep it only if it contradicts no verdict.

Writing rules:

- Keep direction and the reasons that matter; cut repetition, hedging, repeated caveats,
  commentary about the review process and process history.
- Plain words: no jargon unless defined in one line.
- Keep a heading `docs/DECISIONS.md` links to through `[[FILE#Heading]]` while its section
  exists; list any you rename or remove in your report. Do not edit `docs/DECISIONS.md`.
- Keep a relative link to a picture that still illustrates current direction. A picture a
  document shows is a file committed beside it under `docs/`, never linked from the ignored
  `artifacts/` folder (`just symphony-cli docs-audit --check` refuses one git does not track).
- Touch no file outside your assignment, and run only the checks the task and the project
  assign. Commit, delegate or land only within the assignment's authorization.

Report back: for each file, its old and new word count (`wc -w`), the verdicts you applied by
slug, any heading linked from the register that you renamed or removed, anything you
deliberately left out, and any contradiction between verdicts (state it; do not resolve it by
choosing).
