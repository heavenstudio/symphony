---
name: apply-review
description: Apply the owner's review verdicts to the design docs. Use after the owner has reviewed in the Symphony (`just symphony`) — answers every open note in docs/DECISIONS.md, turns rejections into new proposal cards, and sends subagents to rewrite each document that disagrees with a verdict. Trigger on "apply my review", "I made decisions", "I left comments", or /apply-review.
---

# Apply the owner's review

The owner reviews in the web app (`just symphony`); verdicts land in the markdown
(convention: `docs/workflow/REVIEW.md`). This skill is the agent side of that loop. It
exists because the owner's standing instructions are:

- A note on ANY card, approvals included, is work to act on.
- Documents the owner disagreed with are **rewritten** to match, not given a
  banner pointing at the register.
- The rewriting is done by subagents as part of the review, one per disjoint
  group of files, and the documents come out **shorter**: the owner found them
  too long to review.

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
   (`DECISIONS.md` and each faction page, whose art cards sit under *art* topics).
   Then `just symphony-status --open` and
   `just symphony-status --json`. Read `docs/DECISIONS.md` in
   full. Note every owner note, including those on approved cards, and every
   owner callout in other documents.

2. **List every owner comment that has no agent reply after it, whatever the
   card's verdict.** Approvals with comments are the easy ones to miss: "approved
   — at least 21, could vary" is an instruction, not a sign-off. The board's
   `--open` list and each card's `unanswered` field in `--json` are exactly
   this list; the tidy pass will not resolve a card while it has one. Treat
   each comment on an approved card as a change request for the documents in
   step 4, not only as something to reply to.

3. **Answer every open item in the register** with one dated line appended
   under the card, `  [YYYY-MM-DD HH:MM] agent — <reply>` (continuation lines
   indented four spaces; see `docs/workflow/REVIEW.md`). Never edit the owner's words
   or marker, and never rewrite an earlier line: history is append-only. Plain words; explain jargon the owner asked about. A reply may use
   markdown links and images (the web app renders them), which is how to answer
   "show me side by side" with an existing picture under `artifacts/`.
   Keep any hidden `review:unseen-owner` comment: it prevents a reply that had not seen a stale owner save from closing that save.

4. **Turn rejections into proposals.** Where a rejection or note needs a
   replacement the owner did not fully write, add a new `[ ]` card in a new
   `###` topic inside the register region (between `register:start` and
   `register:end`), naming the card it replaces or extends. The tidy pass
   moves it into "Needs your decision". **Where the owner's note already states
   the direction, make no card**: apply it to the documents and say so in the
   reply on the original card. The owner called cards that only restate their
   own words pointless: a card must ask for a real choice (a proposal they did
   not write, missing detail, or pictures to judge). Where two verdicts conflict, add a card that
   asks, and do not choose. Do not propose beyond what the notes call for.

5. **Find the documents that disagree.** Start from the `[[FILE#Heading]]`
   links on rejected and noted cards, then grep `docs/` for the rejected
   concepts to find documents built on them (art plans, matrices, handoffs).
   JSON data files and `docs/analysis/` / review records are evidence: list
   them as disagreeing but do not rewrite them unless the owner asks.

6. **Rewrite with subagents, in parallel.** One `general-purpose` subagent per
   disjoint group of files (a big document alone; small related ones
   together), model `opus` unless the owner says otherwise. Each prompt: "Read
   the brief at `.agents/skills/apply-review/rewrite-brief.md` and follow it
   exactly", then the file list, the verdict slugs that touch those files, and
   the headings `DECISIONS.md` links to in them. Subagents do not edit
   `DECISIONS.md`, do not run `just verify`, and do not commit.

7. **Integrate.** From the reports: fix any `[[FILE#Heading]]` link in
   `DECISIONS.md` whose heading was renamed; check `just symphony-status --open`
   shows nothing but what is waiting on the owner; check word counts went down;
   skim each rewritten file for anything presented as decided that is only a
   pending proposal. Run `just symphony-status --tidy` so the register ends
   regrouped, then `just check` and `just docs-index` once; `just land` runs
   `just verify`.

8. **Report to the owner**: which cards got replies (call out comments on approved cards and what each changed), the new cards waiting on
   them (the highest-leverage first), each rewritten document with old and new
   length, contradictions found, and what still disagrees but was left alone
   (JSON, evidence records) and why.
