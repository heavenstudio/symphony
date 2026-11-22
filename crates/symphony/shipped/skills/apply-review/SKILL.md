---
name: apply-review
description: Apply the owner's review verdicts to the design docs. Use after the owner has reviewed in the Symphony (`just symphony`) — answers every open note on a card, turns rejections into new proposal cards, and rewrites documents that disagree with a verdict under the project's workflow. Trigger on "apply my review", "I made decisions", "I left comments", or /apply-review.
---

# Apply the owner's review

The owner reviews in the Symphony. Read the destination project's review
conventions and locate its decision register before acting; paths below use the
usual docs/DECISIONS.md convention. Follow local permissions, staffing, model,
validation and landing rules. Only the owner decides the design direction.

Act on every unanswered note, including notes attached to approvals. Rewrite
documents that disagree with an explicit direction so they read as current guidance.
Keep replies and the owner's history in the register. Use delegation only when
the task and project authorize it; this skill prescribes no agent family or model.

## Hear the owner live

When the owner is reviewing while you work, connect as the `chat-doc` skill
describes: register the session's own native channel with
`just tasks callback --chat`. Each owner save queues a callback for the chat
recipient: a verdict note, card comment, open callout or ask. Answer each
event as it arrives, the same way step 3 answers a note. In-process subagents
use their board inbox through their parent, never the inherited native socket.

The owner can also **chat** on a card (💬 Send in the app): a dated owner line
with no verdict, delivered as an owner comment callback. Answer it like a
conversation, with one agent line under the card; the app shows the reply
within seconds and raises a banner for it. A chat message is not a verdict:
do not treat it as approval, and change documents only when it asks for that.

## Steps

1. **Tidy, then read the verdicts.** `just symphony-status --tidy` first: it marks
   plain approvals resolved (dated) and regroups every register owner-first
   (the project register and its per-topic pages).
   Then `just symphony-status --open` and
   `just symphony-status --json`, which names each card's file and line. A
   card sits at the place in its document where the thing it decides is said,
   so read each open card in its document, with the prose around it; the
   project's register holds only the cards no document says the subject of.
   Note every owner note, including those on approved cards, and every
   owner callout in other documents.

2. **List every owner comment that has no agent reply after it, whatever the
   card's verdict.** Approvals with comments are the easy ones to miss: "approved
   — at least 21, could vary" is an instruction, not a sign-off. The board's
   `--open` list and each card's `unanswered` field in `--json` are exactly
   this list; the tidy pass will not resolve a card while it has one. Treat
   each comment on an approved card as a change request for the documents in
   step 4, not only as something to reply to.

3. **Answer every open item** with one dated line appended
   under the card, `  [YYYY-MM-DD HH:MM] agent — <reply>` (continuation lines
   indented four spaces). Never edit the owner's words
   or marker, and never rewrite an earlier line: history is append-only. Plain words; explain jargon the owner asked about. A reply may use
   markdown links and images (the web app renders them), which is how to answer
   "show me side by side" with an existing picture. A card's picture is a file
   committed beside its document under `docs/`, so a clone shows it: copy one
   from the ignored `artifacts/` folder there and commit it, never link into
   `artifacts/` or an owner's upload. `just symphony-cli docs-audit --check`
   refuses a card picture git does not track.
   Keep any hidden `review:unseen-owner` comment: it prevents a reply that had not seen a stale owner save from closing that save.
   Write each line with `just symphony-reply PATH SLUG "<reply>" --as <handle>`,
   and resolve an owner callout with `just symphony-reply PATH L<line> "<answer>"
   --as <handle>` (`<line>` from `PATH:LINE` in `--open`): the owner's ask on the
   card or comment then closes credited to your session, where a line written by
   hand closes it credited to no one.

4. **Turn rejections into proposals.** Where a rejection or note needs a
   replacement the owner did not fully write, add a new `[ ]` card at the place
   it decides: in the document that says the thing, directly under the
   paragraph or heading that says it, its line at the left margin with a blank
   line above and below, never in a section at the document's end. That is the
   rejected card's own place unless it decides something said elsewhere. On a
   page that is a register of cards it goes inside the register region (between
   `register:start` and `register:end`), which the tidy pass regroups. Name the
   card it replaces or extends, and give it a name no other card has. **Where the owner's note already states
   the direction, make no card**: apply it to the documents and say so in the
   reply on the original card. The owner called cards that only restate their
   own words pointless: a card must ask for a real choice (a proposal they did
   not write, missing detail, or pictures to judge). Where two verdicts conflict, add a card that
   asks, and do not choose. Do not propose beyond what the notes call for.

   **Never reopen an approved card with a question on it.** A consequence of
   the approved choice, whatever follows from it while the option chosen stays
   the one the card described (a correction, a name it implied, a cost or a
   trade it did not state), is a plain agent line on that card, quoting the
   owner's words it rests on and answering any owner note still open there; the
   card stays resolved. Only a change that would make the option chosen another
   than the one described is a new card of its own that links the approved one.
   **Before adding any card, check the owner's earlier verdicts.** A card the
   owner approved decides a later card only when the choice it approved already
   answers what the later card asks, and each of the later card's other options
   would change that choice. A recommendation that leaves alone something the
   approved card never addressed decides nothing, and that card is asked. An
   auto-approved card, or a line an agent recorded as answered, is not the
   owner's approval and decides no further card. A decided card is not asked
   but recorded as answered on the card the owner approved, quoting their words
   and naming the card they are on. A card that is asked opens with what changes
   for the owner in one or two plain sentences, before any option.

5. **Find the documents that disagree.** Start from the `[[FILE#Heading]]`
   links on rejected and noted cards, then grep `docs/` for the rejected
   concepts to find documents built on them (plans, matrices, handoffs).
   JSON data files and `docs/analysis/` / review records are evidence: list
   them as disagreeing but do not rewrite them unless the owner asks.

6. **Rewrite the affected documents.** Use the bundled rewrite-brief.md for the
   content rules, together with the destination project's instructions. If the
   project permits delegation, assign disjoint file groups with their relevant
   verdicts and linked headings. Follow its launch, model and concurrency policy.
   Keep card updates with the integrating session so replies remain ordered.
   Give each assignment an explicit scope, validation and commit boundary.

7. **Integrate.** From the reports: fix any `[[FILE#Heading]]` link on a
   card, in whichever document holds it, whose heading was renamed; check `just symphony-status --open`
   shows nothing but what is waiting on the owner; check word counts went down;
   skim each rewritten file for anything presented as decided that is only a
   pending proposal. Run `just symphony-status --tidy` so the register ends
   regrouped. Run the destination project's required document and link checks,
   then follow its review and landing process. Do not assume a particular gate,
   certificate, build recipe or landing command.

8. **Report to the owner**: which cards got replies (call out comments on approved cards and what each changed), the new cards waiting on
   them (the highest-leverage first), each rewritten document with old and new
   length, contradictions found, and what still disagrees but was left alone
   (JSON, evidence records) and why.
