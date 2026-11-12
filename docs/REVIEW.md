# Concept review

The standalone workspace provides the generic board and document commands.
Verification, certificates and the landing/stamp recipes in the workflow examples
are supplied by a consuming project; no project gate executable or standalone
landing backend is bundled here. See the workspace [command ownership](../AGENTS.md#command-ownership).

How design and concept work gets validated before more of it is made. The
owner reads in a local web app and every verdict is written into the markdown
itself; agents read the verdicts back with one command. Nothing leaves the
repository, and git history is the audit trail.


Commands shown use the consuming project's recipes and board connection.

## Two commands

```
just symphony           # the Symphony at http://127.0.0.1:8765/, one for every worktree (`just symphony --stop` stops it)
just symphony-status    # the board in the terminal: decisions, open comments
```

`just symphony-status --open` prints only the items still waiting on someone,
and `--json` gives the same board as data.

## The web app

The board is the front page: open items first, then the design docs and how
many decisions wait in them. A document page renders the markdown; hovering any block shows a
**+ comment** button, and clicking a decision card opens its verdict buttons
and a note box. The app serves any file under the project, so a concept
page's images, SVGs and galleries under `artifacts/` show inline, and once
in-game captures exist under `artifacts/concept-renders/` they show the same
way. It writes verdicts under `docs/` (and the owner's approval to land an owner-tier branch, in the landing store),
and keeps queue-view preferences under `artifacts/review-order/`. It listens on every interface but answers
only this machine and tailnet peers `tailscale whois` confirms (403 otherwise). The markdown holds the
owner's answers; view preferences never become verdicts.

### Queue categories

The owner queue mixes document categories in round-robin order, oldest first inside each
category. The home preview follows the same order. A category is the first folder under
`docs/` (such as `design` or `engineering`); root-level documents use `docs`, and branch
proposals use their document's folder. This changes order, never a card's computed group.
The full queue's category chips show counts that add up to the section total. **All** is
the default; selecting a chip filters the rows and remembers that filter on this device.
If a saved category has no cards, the queue shows All.

### Later

**Later**, beside a card's verdict buttons and in the queue, puts it last in its document category
and opens the next card. The marker, owner lines and *Waiting on you* count stay unchanged.
A small *postponed* mark offers **Undo Later**. Its ordinary place returns when the owner has opened
the rest of its computed owner group, or after three days (72 hours). Answering or deleting a card clears its entry;
a new owner line clears it even when the card stays in the same group.

The server stores this per-owner order in its root's `artifacts/review-order/postponed.json`, shared by
the phone and laptop and retained across restarts. Main and branch decisions share that order and the
next card; their document writes still go to the selected worktree. `POST /api/postpone` takes `path`, `slug` and an `action` of
`later`, `undo` or `seen`; a visit records progress without rescanning documents or refreshing an unchanged
queue. A corrupt or unreadable preference file is preserved and logged, and reads serve the mixed order
without postponements.

### One app, every worktree

**The owner runs one app, the primary checkout's, and every card waiting on them is in it.** A card an agent
posted on a branch (committed to its worktree's `docs/DECISIONS.md` or any register) is not in main's file until
the branch lands, so the app also reads the worktrees of live branches (`symphony::branches`): each card the
branch's commits added or changed, and that differs from main's, is an ordinary decision in the same ranked
queue, board counts and navigation as main's cards, with a small tag linking its task. A card identical to
main's, or one main changed after the branch forked, is skipped.

- **Posted, never drafted.** A branch's cards are read from its HEAD commit, never from the worktree's
  files: a card or a line the agent has not committed yet reaches no queue, count, navigation or banner.
  Committing it posts it. A posted card asking the owner whose pictures are not on disk in its worktree
  waits until they are, and `just symphony-cli docs-audit --check` refuses it ([below](#writing-a-card)).
- **Live branches only.** A branch merged into main, or whose task the board says is done or cancelled
  (the task named like the branch, else every task that records it as its executor's branch), posts
  nothing.
- **One card.** A card main holds and a branch posted is one card wherever the owner reads it: the queue,
  main's page and the branch's page all show both copies' lines in time order, so the owner's line and the
  agent's answer show wherever either was written. Its mark is the one the latest verdict line set (the
  owner's, or an agent's resolved, auto-approved or dismissed line), main's on a tie. Once main closes a
  card (resolved, or rejected and answered) after every line of a branch's copy, that copy adds nothing.

The card's page and every page its links reach are served from that worktree, under
`/@<branch>/docs/…` (a `/` in the branch name is written `~`). The owner's verdict is written **into that
worktree's file**, uncommitted, where the agent's `just symphony-status --open` reads it exactly as before; it is
never committed onto main (the branch carries it when the agent commits and lands). Until then the owner's own
lines there count at once, laid on the posted card; on a card the branch did not post (its copy is the one it
forked with, or main's), and in a finished branch's worktree, they join main's copy of the card, on the same
terms. No second app is needed,
and a per-worktree app is never something the owner is pointed at. The document shows added, removed
and changed blocks in place against main, with the previous content available beside a changed block
and a line linking the task those differences belong to. Identical documents have no comparison marks.
Only blocks changed since the branch's merge base and still different from main are marked.
Cards match by slug; prose paragraphs match by text. If main both rewrites a paragraph and
inserts nearby text, that alignment is best effort: an insertion can be shown as the previous
paragraph. Revised cards replace their
main copy in the queue; distinct proposals from different branches each remain a decision,
including after another proposal is answered. Identical stacked copies still appear once,
and the document counts once. The header board link, decision counts
and previous/next navigation keep the same global view while a branch document is open.

The scan is kept, never run per request: a watcher refreshes it when a worktree's HEAD log or a register file
moves (and every minute, for a register file only a branch has), and an owner's write refreshes it at once.
About 160 worktrees take ~3 s to scan on a busy machine; a check that nothing moved is stat calls.

**A worktree with an owner line no commit has is never removed.** The worktree sweep (`just tasks sweep`) and
`land`'s cleanup keep it and say so (`symphony::landing::owner_lines_left`, and the sweep's own reading of it);
commit the verdict first.

## Start with the decision register

The direction documents are far too long to review as prose. `DECISIONS.md`
lists the proposals they rest on, one line each, as a checklist:

```markdown
- [x] **lamp-count-141** — 141 original lamps ... [[LAMP_CATALOGUE#Size distribution]]
  [2026-09-24] owner · approved — at least 21. Number could vary.
  [2026-09-24] agent — Recorded as a floor.
```

| marker | meaning |
|---|---|
| `[ ]` | not answered yet |
| `[x]` | approved |
| `[-]` | rejected; the project's triage command (`reject --answered-by`) sets it where the owner rejected the card in the terminal |
| `[?]` | a question |
| `[r]` | resolved: approved with nothing left to answer (set by an agent's tidy pass, never by hand) |
| `[a]` | auto-approved: set only by the project's triage command (`auto-approve`), with a dated `agent · auto-approved — <letter>: <basis>` line; counts as approved, and any owner verdict overrides it |
| `[~]` | dismissed: the owner says the card is not needed (⊘ Dismiss), or a newer card names it in its *Replaces:* while it still asks the owner, pending or answered `[?]` ("superseded by …"; set by `--tidy` and on every post in the app), or the project's triage command (`dismiss --answered-by`) where the owner said so in the terminal; nothing to act on |

### Card history

Keep the hidden `review:unseen-owner` comment under a card: it records that an agent reply had not seen a stale owner save, so that save stays open until a later reply answers it.

**Every event is a dated line under the card**, appended and never rewritten:
`[YYYY-MM-DD HH:MM] owner · <verdict> — <comment>` when the owner answers, and
`[YYYY-MM-DD HH:MM] agent — <reply>` when an agent does. Deeper-indented lines
continue the entry above. The web app writes the owner's lines; the history
is how anyone can tell when something was decided and whether it changed.
Agent lines may carry markdown links and images; owner lines are shown as
plain text.

**An agent that needs the owner to answer** writes its line as
`[YYYY-MM-DD HH:MM] agent · question — <the question>` (`just symphony-reply PATH
SLUG "text" --question`). A plain `agent —` line is an answer and lets an
approval resolve, even one ending in `?`; a question never does, until a later
plain agent line withdraws it. A card whose last entry is a question
is not resolvable, is not tidied, and sits in *Needs your decision* whatever
its mark (a resolved or dismissed card stays closed). The owner's comment on it
sends the card back to *Waiting on an agent*; a plain re-approval resolves it.

### Groups

**Where a card sits** is computed from its marker and its last line, and the
register regroups itself (inside its `register:start`/`register:end` markers)
every time the owner answers. Counts are never written into the file; the page
and the board count the cards live:

| group | a card is here when |
|---|---|
| Needs your decision | it is new: not answered yet (`[ ]`) |
| Auto-approved: spot-check | it is `[a]`: approve, reject or comment on it like any card; a comment moves it to *Waiting on an agent* |
| Answered: approve again or comment | the owner approved it or asked a question with a comment, and an agent has answered; a plain re-approval resolves it |
| Waiting on an agent | the owner had the last word and it asks for something: any comment, a rejection or a question |
| Rejected and replaced | it is rejected and an agent has answered, usually with a replacement card |
| Resolved | it is `[r]`, or it is approved and the owner's last line is a plain approval with no comment |

A plain approval, first time or after an agent's answer, is **resolvable**;
the next review pass marks it `[r]` with a dated `agent · resolved` line
(`just symphony-status --tidy`). Any new comment from the owner reopens a card,
resolved or not. Agents never change the owner's marker or words; a rejected
card stays rejected, and its replacement is a new card. Only lines with a
**bold slug** are cards. Read the prose only where a one-line proposal is not
enough; the link at the end of each card goes to the section that argues it.

### Settled cards

**Settled cards fold away.** Resolved, dismissed and rejected cards show as one
line in the app; click to see the card and its full history. `--tidy` also
resolves an approval once an agent has answered its note, and moves every
answered `> [!comment]` into `docs/history/review-ledger.md`, word for word, so
pages keep only what is still open. Nothing is deleted: the cards stay in their
register and the comments in the ledger.

### Slugs in prose

**Documents say what the design is, not how it was decided.** A card slug in a
document's prose ("approved in *some-card*") is history, and history belongs
on the card. `just symphony-status --lint` fails on a slug outside a register or
code block, naming its file and line, and a project's checks can run it. The
exceptions are `docs/history/` and `docs/analysis/`, which are records. A
project's own checks may also refuse the verdict words themselves (approved,
rejected, superseded, dismissed) outside the register of the pages they name: such
a page shows the chosen pictures and traits, never a list of what else was approved.

An agent registers with `just tasks callback --chat --as HANDLE`. Each ask,
follow-up, card message, verdict note and callout you save is queued by that
save and delivered to the recipient's native Claude inbox socket or Codex
thread. The app names the recipient and says when nobody is registered; saved
messages wait in an outbox. With no chat seat or `--if-free` fallback, chat goes
to a live orchestrator with a native callback, choosing the lexicographically
smallest handle. A normal `just tasks callback --as HANDLE` registers that channel;
`--chat --if-free` opts into the higher-priority fallback without displacing a seat.
No agent watches files or polls for messages.

### Chat

**Chat and notifications.** On an open card, type a message and press
💬 Send (or ⌘/Ctrl+Enter) to talk about it without giving a verdict; the
thread updates by itself when an agent answers. Every page shows a banner
when an agent replies or adds a card anywhere, with a link to it.

### Cards with options

**Cards with options.** When a card offers A and B, approve it and say which
in the note ("B"); reject only when neither will do. Agents write such cards
ending "Approve and say A or B in the note, or comment."

### Writing a card

**All the judgement call needs, clear and concise.** The owner, 2026-10-06:
"The card should encompass "all relevant information needed to make a judgment
call in a clear and concise manner"".

- It reads well: short paragraphs, bullets, plain words, no jargon, and one
  clear question. Not a wall of text.
- Low-level detail the decision does not need (benchmark method, rigs, per-run
  numbers, gate thresholds) goes in a linked log or analysis file.
- Pictures where the thing judged is visual (pictures, UI, models, renders), shown
  rather than described; a gameplay mechanic is fine as well-written text. Every
  picture is on disk before the card is committed: `just symphony-cli docs-audit
  --check` refuses an open card whose picture does not load (any picture of a
  card the branch wrote, a tracked one of any other), and the app holds a branch's
  card asking the owner back until its pictures are there.
- A results or comparison card says how each option looks and how well it
  performs, one plain line each, such as "all three cost the same on the GPU,
  under 0.02 ms apart".

(`RULES.md` `docs/cards-clear-and-concise`.)

## Versions and waiting decisions

A document has no status: it is always open to review, and what the owner has
yet to answer is its undecided cards. Every document page and the docs index
say **N decisions waiting** (the same count the next/previous buttons walk).
Where a document stands in time is its git history: the page header shows when
it last changed and the commit, and the commit links to the document's history,
a list of each commit that touched it (date, subject), each opening that
commit's diff of the document in the app.

Only the owner approves a card (`RULES.md` `docs/no-self-approval`). Approval
covers the card as it stood: an agent that materially changes an approved card
says why in a `[!question]`.

## Commenting in place

Anywhere in a document, next to the thing it is about, as a callout. The web
app's **+ comment** button has one text box, whose first line becomes the
title, and writes exactly this after the block you were on:

```markdown
> [!comment] 141 lamps
> Far too many to validate. Show me the 30 that matter first.

> [!question] Do the stations need five size bands?
> Three would do if the spans stay this far apart.

> [!decision] Drop the brass finish from the lamp matrix.
```

The first line is the tag and a short title; the `>` lines under it are the
body. `comment`, `question` and `decision` are open work until an agent nests
a resolution under them. Older documents also hold `like` (what to keep, no
answer needed) and `dislike` (open, like `comment`); the app no longer offers
them. A resolution looks like this:

```markdown
> [!decision] Drop the brass finish from the lamp matrix.
> > [!resolved] Removed the row and its five fittings; the
> > matrix is now 20 cells. Evidence kept in docs/history/finishes.md.
```

The owner's words are never edited or deleted. The resolution goes under them.

## Asking

**Ask** sits beside **Comment**. A comment is written into the document and
stays until an agent resolves it; an ask is a chat. You ask about a document,
a heading or a card, a listening agent (`/chat-doc`) answers, and the answer
shows beside your question. Reply to go on; the thread opens again. The app
says whether anyone is listening before you ask.

Nothing of an ask is written to `docs/` or committed: asks live in
`artifacts/asks/` and are deleted a day after their last message. When an
answer should change a document, say so as a comment or on a card.

**The `+` button** (bottom right of every page, made for a phone) opens a
sheet with two modes. *Chat* is an ask: on a document page it hangs on that
document and heading; from any other page (board, queue, tasks) it lands on
`docs/README.md` and names the page it came from. The answer shows in the
sheet. *Add task* files a task on the board as the owner, in `concept`,
priority 2 unless you pick one, slug `inbox/<title>` unless you give one, with
the page linked. The draft is kept in this browser, so a
reload loses nothing. While a task's outcome is uncertain, the sheet shows
**Checking save…** and keeps the draft locked until it confirms the task or
shows a refusal. After a reload, open the sheet and use **Check save** to
resolve that same submission. A refusal keeps the text editable for another
try; a confirmed save clears it and links to the task. The server retains
pending task text until confirmation, then keeps only a small receipt.
Routes: `POST /api/quick-chat`, `POST /api/quick-task`.

## Attaching files

Every box the owner writes in takes files: the `+` sheet's chat and new task,
a document's ask and follow-up, a card's message, verdict note or comment, and
a task's message. Pick them with **Attach** (a phone offers its camera and
photo library there), paste a screenshot into the box, or drop files on it.
Each file is sent as soon as it is added and shows as a chip, with × to take
it off; the post then carries one line per file after the owner's words: a
picture `![name](/artifacts/uploads/<sha256>.png)`, any other file a link
`[name](/artifacts/uploads/<sha256>.pdf)`. The posted item keeps showing the
file: a picture drawn at its real size (tap it for the viewer), a document as
its link. A comment needs words, its first line being its title; every other
post may be files alone. An agent finds each file at that path in the main
checkout, and a chat callback names the full paths.

- **What is taken.** PNG, JPEG, GIF and WebP pictures; PDF, plain text
  (`.txt`, `.md`, `.csv`, `.json`, `.log`), Word, Excel and PowerPoint files;
  at most 10 MB each and 10 to a post. The kind is read from the file's own
  bytes, never from its name alone or the browser's word: SVG, HTML, scripts
  and everything else are refused (`415`), under a text file's name too (text
  that opens with a tag or a `#!` line, or holds a tag such as `<script>` or
  `<svg>`), an empty file `400`, a larger one `413`, and none of them is
  stored.
- **Where they live.** `artifacts/uploads/` of the checkout the shared server
  serves, the main checkout, named by the SHA-256 of their bytes: the same
  file sent twice is stored once, and a file posted from a branch page shows
  on every branch. The folder is gitignored, like every image in `artifacts/`.
- **How they are served.** `GET /artifacts/uploads/<sha256>.<ext>`, also under
  `/@<tree>/`, answers a stored name only, with its fixed type, `nosniff` and
  a script-free sandboxing `Content-Security-Policy`; pictures and text open
  in the browser, PDF and Office files download. Nothing is ever served as
  HTML or script. `POST /api/upload?name=<file name>` takes the raw bytes
  and keeps the Host and Origin checks of every other save.

## On a phone

The owner reviews on a phone over a mobile link, so the app is built to cost little there
(`symphony::images` and `apps/symphony-ui`; measured with Chrome at phone size).

- **A picture never moves the page.** Every local PNG the app draws carries its real `width` and
  `height`, so its space is reserved before it arrives. Nothing is unhidden or reshaped after the first
  paint: the contents list starts shut on a narrow screen.
- **Pictures are drawn from small copies.** A PNG over 120 KB is shown from a cached JPEG at
  `/thumb/<width>/<token>/<path>.jpg`, made once into `artifacts/docs-thumbs/` (a cache the app
  writes, named by the picture's path, time and size, so it is never stale) with the system's `sips`;
  where there is none the original is drawn. The viewer opens the original. Pictures load as they near
  the screen, the first of a page first.
- **Text is gzipped, pictures revalidate** (`ETag`, `304`), models load as they near the screen.

## Limits, honestly

- **JSON data is not reviewable here.** The app and the board only read
  markdown, so the data in `docs/data/` is reviewed through the page that
  shows it: a plugin draws a record of it as a component of a page, one card per
  record or a live table, and the cards on that page are where it is reviewed.
- **Images live in `artifacts/`, which is gitignored.** The app shows them on
  this machine and nowhere else. Anything approved should be copied into a
  tracked place as part of resolving the approval.
- **The board sees `docs/` only**, and skips `docs/history/`.

## HTML mockups in cards

A card line `![Flight HUD A](/ui/mockups/hud-a.html)` (an image whose target is an `.html` file, absolute or relative
to the page) shows the mockup live, not a picture of it: a lazy, sandboxed `<iframe>` at the mockup's native
1280x800, scaled to the card's width (`tools/static/mockups.js`) so it stays crisp on a phone. Tapping it or the
*full size* link opens the page in a new tab. A plain link to an `.html` stays a link; a PNG keeps the picture path.
The card's comparison row (Base, A, B, C above its lines) shows an `.html` candidate the same way.

- **Sandbox.** The frame is `sandbox="allow-scripts"`: scripts run for animation, with no same-origin access to the
  app, no top navigation, no forms or popups. The server also sends `Content-Security-Policy: sandbox allow-scripts`
  with every raw file it serves from the project (not only `.html`: an `.svg` a mockup navigates to would otherwise run as
  the app), so a mockup opened full size has the same reach as the one in a card. `/api/*` also refuses `Origin: null`,
  which is what a sandboxed page sends.
- **Assets.** The page's CSS, JS, SVG and images are served like any file under the project (`artifacts/`, or the tracked
  mockup folder under `docs/`: `tokens.css` plus one component file per element); nothing outside the repo is.
- **Source.** Compose a screen from one shared CSS file of tokens and one file per component, assembled per page; a
  PNG is only a snapshot for a thumbnail. Tracked sources beat `artifacts/`, which is gitignored.

The SPA renders media in `apps/symphony-ui/src/media.rs`; its browser adapter is
`media_live.rs`, and the native server confines raw media to the selected
checkout in `apps/symphony-server/src/media_files.rs`. Browser coverage returns after
the Rust migration, including attempts to read, write or navigate the parent
from a mockup.

## Cards link each other

In the app, a card slug named in a card (after *Replaces:*, or in bold in an agent line such as "Answered by **x**") links to that card, in any doc.
