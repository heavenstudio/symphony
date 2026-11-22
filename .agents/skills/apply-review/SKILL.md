---
name: apply-review
description: Apply the owner's review verdicts to the design docs. Use after the owner has reviewed in the Symphony (`just symphony`) — answers every open note on a card, turns rejections into new proposal cards, and rewrites documents that disagree with a verdict under the project's workflow. Trigger on "apply my review", "I made decisions", "I left comments", or /apply-review.
---
<!-- symphony-stamp: 4380d5b2aa78f174 files=SKILL.md,rewrite-brief.md -->

# Apply the owner's review

The owner reviews in the Symphony, and only the owner decides the design direction. Read the
project's review conventions and locate its decision register first; paths below use the usual
`docs/DECISIONS.md`. Follow local permissions, staffing, validation and landing rules.

Act on every unanswered note, those attached to approvals included, and rewrite documents that
disagree with an explicit direction so they read as current guidance. While the owner is
reviewing live, connect as the chat-doc skill describes and answer each event as it arrives, the
way step 3 answers a note; a chat message (💬 Send) is conversation, never a verdict.

1. **Tidy, then read the verdicts.** `just symphony-status --tidy` marks plain approvals resolved
   and regroups the register. Then `just symphony-status --open` and `--json`, which names each
   card's file and line. A card sits at the place in its document where the thing it decides is
   said, so read each open card in its document, with the prose around it; the register holds
   only the cards no document says the subject of. Note every owner note, those on approved
   cards too, and every owner callout in other documents.
2. **List every owner comment with no agent reply after it, whatever the card's verdict**: the
   `--open` list and each card's `unanswered` field in `--json`. An approval with a comment is an
   instruction, not a sign-off: treat it as a change request for the documents in step 5.
3. **Answer every open item** with one dated line under the card, written with
   `just symphony-reply PATH SLUG "<reply>" --as HANDLE`; resolve an owner callout with
   `just symphony-reply PATH L<line> "<answer>" --as HANDLE` (`<line>` from `PATH:LINE` in
   `--open`). History is append-only: never edit the owner's words or marker, an earlier line, or
   a hidden `review:unseen-owner` comment. Plain words; explain jargon the owner asked about. A
   reply may use markdown links and pictures; a card's picture is a file committed beside its
   document under `docs/`, never a link into the ignored `artifacts/` folder or an owner's upload
   (`just symphony-cli docs-audit --check` refuses one git does not track).
4. **Turn rejections into proposals, and only real choices into cards.**
   - Where a rejection or a note needs a replacement the owner did not fully write, add a new
     `[ ]` card at the place it decides: in the document that says the thing, directly under the
     paragraph or heading that says it, its line at the left margin with a blank line above and
     below, never in a section at the document's end. That is the rejected card's own place
     unless it decides something said elsewhere; on a page that is a register of cards it goes
     inside the register region (between `register:start` and `register:end`). Name the card it
     replaces or extends, and give it a name no other card has.
   - Where the owner's note already states the direction, make no card: apply it to the
     documents and say so in the reply. A card must ask for a real choice (a proposal they did
     not write, missing detail, pictures to judge). Where two verdicts conflict, add a card that
     asks, and do not choose. Do not propose beyond what the notes call for.
   - Never reopen an approved card with a question on it. What follows from the approved choice
     while the option chosen stays the one the card described (a correction, a name it implied,
     a cost or a trade it did not state) is a plain agent line on that card, quoting the owner's
     words it rests on; the card stays resolved. Only a change that would make the option chosen
     another is a new card, which links the approved one.
   - Before adding any card, check the owner's earlier verdicts. A card the owner approved
     decides a later card only when the choice it approved already answers what the later card
     asks, and each of the later card's other options would change that choice. An auto-approved
     card, or a line an agent recorded as answered, is not the owner's approval and decides
     nothing. A decided card is not asked: it is recorded as answered on the card the owner
     approved, quoting their words.
   - A card that is asked opens with what changes for the owner in one or two plain sentences,
     before any option.
5. **Find the documents that disagree.** Start from the `[[FILE#Heading]]` links on rejected and
   noted cards, then search `docs/` for the rejected concepts. Data files, review records and
   `docs/analysis/` are evidence: list them as disagreeing, and do not rewrite them unless the
   owner asks.
6. **Rewrite the affected documents** by [rewrite-brief.md](rewrite-brief.md) and the project's
   instructions. Where the project permits delegation, assign disjoint file groups with their
   verdicts, each with an explicit scope, validation and commit boundary; card updates stay
   with the integrating session so replies remain ordered.
7. **Integrate.** Fix any `[[FILE#Heading]]` link on a card, in whichever document holds it,
   whose heading was renamed; check `just symphony-status --open` shows nothing but what waits
   on the owner and that word counts went down; skim each rewritten file for anything presented
   as decided that is only a pending proposal. Run `just symphony-status --tidy`, then the
   project's document and link checks, and follow its review and landing process.
8. **Report to the owner**: which cards got replies (comments on approved cards called out, with
   what each changed), the new cards waiting on them, each rewritten document with its old and
   new length, contradictions found, and what still disagrees but was left alone, and why.
