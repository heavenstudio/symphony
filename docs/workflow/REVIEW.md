# Concept review

The standalone workspace provides the generic board and document commands.
Verification, certificates and the landing/stamp recipes in the workflow examples
are supplied by a consuming project; no project gate executable or standalone
landing backend is bundled here. See the workspace [command ownership](../../AGENTS.md#command-ownership).

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

The product references describe [reading a document](../product/DOCUMENT.md) and
[using a card's verdict editor](../product/CARDS.md). The app serves any file under the project, so a concept
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
  Committing it posts it. The branch's page draws such a card as a draft ("draft, not posted" where its
  posted time goes). The owner can ask about a draft, reject it or write under it, but not approve or
  dismiss it: its agent may still change it, and a verdict that closes a card never covers what the
  owner has not seen, so the save says it is a draft not posted yet until its branch commits it. The
  same holds for a posted card while its worktree's copy says anything the committed one does not
  besides the owner's own lines (another text, an agent's line): the save says it has changes its
  branch has not posted until its agent commits them. A
  posted card asking the owner whose pictures, in what the branch wrote on it (its
  text or its lines, not main's), are not on disk in its worktree waits until they are: the server
  checks those files on a timer and posts the card when they land. `just symphony-cli docs-audit
  --check` refuses it ([below](#writing-a-card)).
- **Until a branch lands.** A branch merged into main posts nothing. A branch whose task the board
  says is done or cancelled
  (every task that names the branch: the task named like it, and the tasks that record it as their
  executor's branch) keeps its posted revisions of main's proposal text until it lands, as a live
  branch does, including rewrites of options or body content under an unchanged heading. A plain
  appended reply keeps the existing proposal. The page and the queue draw the same revised card,
  and an answer saved from either belongs to that branch. Its other cards keep only what the
  owner said there and the answers under it (below).
- **One card.** A card main holds and a branch posted is one card wherever the owner reads it: the queue,
  main's page and the branch's page all show both copies' lines in time order, so the owner's line and the
  agent's answer show wherever either was written. Its mark is that of the copy whose latest verdict line
  (the owner's, or an agent's resolved, auto-approved or dismissed line) is the later. Lines carry
  minutes, so of the lines of one minute the last a copy holding them wrote decides: the owner's verdict
  laid on a posted card in the minute of its last commit, or one a branch wrote below main's line it
  holds. Between a branch's copy and main's, two lines of one minute that no copy holds together go to
  main's. Once main closes a
  card (resolved, or rejected and answered) after every line of a branch's copy, that copy adds nothing.
  A branch's page draws main's card for every card the branch did not post, and for one main closed
  after the branch's copy, so a stale copy shows main's resolution. A card a page draws in another
  group than its file's copy sits in (an answer given there and saved on main) is drawn in that
  group, the register regrouped as a save would regroup it, each block still standing for the
  file's own lines, where a save or a comment from the page lands.
- **The owner's word never disappears.** A line the owner saved in any worktree counts, whether the
  branch is live or finished and whether the card is posted or a draft. On a card a live branch posted
  or a finished branch's posted revision of main's proposal text,
  it is laid on the branch's copy, which is one card with main's by the rule above. Otherwise (a card the
  branch did not post, a draft, a finished branch's thread) it is laid on main's copy of the card when
  main has it, else it stands as the card of the branch's page, waiting on an agent. Laid on main's copy,
  it goes after main's lines of its minute, so there a line of the owner's and one of main's in one
  minute that no copy holds together go to the owner's. That is a choice, the opposite of the one-card
  tie, so that a tie never drops the owner's word: for a card the branch did not post, its page draws
  main's card, and the owner wrote after reading main's lines. On a live branch's draft it counts until
  an agent answers it, the answer being a draft too until the branch posts it. In a
  finished or merged branch's worktree the owner's lines and every agent's line after the first of them,
  committed or not, stay laid on main's card (or as the branch page's card) until main holds those lines
  or closes the card after them, so an answered question is the owner's to read again like any answer.
- **Mid-merge.** While a merge (or a rebase, a cherry-pick) has stopped on a branch's document and its
  file holds conflict markers, the branch's page shows the branch's last committed copy under a note
  that says the worktree is merging, never the markers. An answer on a card there is saved on the
  branch's side of the conflict, for whoever finishes the merge to keep; a comment waits for the merge.
  A card whose lines cross a conflict marker cannot be answered until the merge is done. While the
  merge lasts, a card's lines are those of the branch's committed copy and what was saved on the
  branch's side since: a line neither the committed file nor the other side holds. A line of the
  other side's that git left outside a conflict, after a line two cards share, never counts as the
  branch card's.

The card's page and every page its links reach are served from that worktree, under
`/@<branch>/docs/…` (a `/` in the branch name is written `~`). The owner's verdict on a card the branch posted,
or on its draft, is written **into that worktree's file**, uncommitted, where the agent's
`just symphony-status --open` reads it exactly as before; the branch carries it once the agent commits it, and
a land refuses while a worktree at a commit it brings holds an owner's answer no commit has
([TASKS](TASKS.md#the-default-landing)). Until then
the owner's own lines there count at once, laid on the posted card. A line given on a branch's page is main's
exactly when that page draws main's card: main holds the card, and the branch did not post it (its copy is the
one it forked with, or main's), the branch is in main already (landed), or main closed its copy after every line
of the page's. A card the branch posted that main has not closed since stays its own. A page whose worktree is
gone is judged on its branch while the branch is kept, as the page was drawn from it, and only with neither left
is a line on a card main holds main's. A line that is main's is saved in main's
file and committed there like a line given on main's page, so `just symphony-status` counts it at once; a line
that is the branch's from a gone worktree's page, or on a card main lacks, has nowhere to go and is refused.
While git cannot list the worktrees, or read the branch of a gone one, nothing says where the card is: a branch
page's line is refused, never taken to main. A line left in a worktree
whose branch reached main another way (merged by hand, or saved while it landed) still joins main's copy of the
card on the terms above, or stands as the branch page's card when main has none. No second app is needed,
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
`land`'s cleanup keep it and say so: the board reads the owner's dated lines under a card, in the documents of the
project's entrypoint folders, that the worktree's HEAD does not hold, and keeps a worktree it cannot read as well;
commit the verdict first. A consuming project's own gate asks the Symphony with
`symphony::landing::owner_lines_left`, which names the documents holding such a line.

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

**A note is drawn as a document's body is**, the owner's and an agent's alike:
a single line break is a line break and a blank line a paragraph gap; bold,
italics, inline code, links, bulleted and numbered lists, tables, fenced code,
pictures, `.html` mockups and the files a plugin draws, `[[wiki links]]`, a
card named in bold, and `[[name:argument]]` components all show. Raw HTML does
not: `Vec<String>` or `<b>` in a note is the words typed, on the lines typed.
Three things are the register's own structure and never come out of a note: a
`#` line, in a list item or a quote too (`- # Intro`, `> ## Q`), or a line
underlined with `=` or `-`, stays as typed and is never a heading; a
`- [ ] **slug**` line is a list item, never a card, since a card starts at
column 0; and a review callout (`> [!comment]` and the other review kinds)
cannot be written in one, because the document's comment reader would take it
for a comment on the page and archiving it would cut lines out of the card.
For the same reader a note's code must be closed (a fence left open would make
the rest of the file code, later comments among it) and cannot open on its
first line, which the file keeps on the entry's own line; such a note is
refused when it is saved. A note's lines keep their words but not their
indentation, so a nested list or indented code inside one comes out flat.

**An agent that needs the owner to answer** writes its line as
`[YYYY-MM-DD HH:MM] agent · question — <the question>` (`just symphony-reply PATH
SLUG "text" --question`). A plain `agent —` line is an answer and lets an
approval resolve, even one ending in `?`; a question never does, until a later
plain agent line withdraws it. A card whose last entry is a question
is not resolvable, is not tidied, and sits in *Needs your decision* whatever
its mark (a resolved or dismissed card stays closed). The owner's comment on it
sends the card back to *Waiting on an agent*; a plain re-approval resolves it.
A question never goes on a card the owner approved
([below](#versions-and-waiting-decisions)), nor on a card whose answer an
earlier verdict already gives ([before a card goes to the
owner](#before-a-card-goes-to-the-owner)).

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

A card waiting on an agent leaves that group by an agent's plain line under it, and
by nothing else: a task filed for its work is named in that line. Until then its
row in the queue says the ask the board holds for it ([TASKS](TASKS.md)), as
`just tasks asks` does: how long it has waited, late past ten minutes, and who
holds it, nobody said as that, so an ask no one took reads as late and not as work
under way.

A plain approval, first time or after an agent's answer, is **resolvable**;
the next review pass marks it `[r]` with a dated `agent · resolved` line
(`just symphony-status --tidy`). Any new comment from the owner reopens a card,
resolved or not. Agents never change the owner's marker or words; a rejected
card stays rejected, and its replacement is a new card. Only lines with a
**bold slug** are cards. Read the prose only where a one-line proposal is not
enough; the link at the end of each card goes to the section that argues it.

### Settled cards

[Settled cards](../product/CARDS.md#settled-cards) keep their full history behind a fold.
`--tidy` also resolves an approval once an agent has answered its note, and moves every
answered `> [!comment]` into `docs/history/review-ledger.md`, word for word, so
pages keep only what is still open. Nothing is deleted: the cards stay in their
register and the comments in the ledger.

### Slugs in prose

**Documents say what the design is, not how it was decided.** A card slug in a
document's prose ("approved in *some-card*") is history, and history belongs
on the card. So do the owner's exact words and who decided what, and when: a
document states intent, behaviour and options, and where the reason matters it
links the card, `[its card](../DECISIONS.md#SLUG)`, which the lint passes
(`RULES.md` `docs/no-ledger-in-documents`). `just symphony-status --lint` fails
on a slug outside a register or code block, naming its file and line, and a
project's checks can run it. The
exceptions are a `history/` and an `analysis/` folder at any depth under the
documents (`docs/history/`, `docs/workflow/analysis/`), which are records. A
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

### Before a card goes to the owner

**Nothing an earlier verdict decides is asked.** What a card the owner approved
already decides, its consequences included, is never put to them again
(`RULES.md` `review/decided-is-not-asked`).

- Before a card is put to the owner, its agent checks the owner's earlier
  verdicts. A card the owner approved decides a later card only when the choice
  it approved already answers what the later card asks, and each of the later
  card's other options would change that choice. A recommendation that leaves
  alone something the approved card never addressed decides nothing, and that
  card is asked. An auto-approved card, or a line an agent recorded as answered,
  is not the owner's approval and decides no further card.
- A decided card is not asked. The agent records it as answered: a plain agent
  line on the card the owner approved, quoting the owner's words and naming the
  card they are on, and answering any owner note still open there, since any
  agent line closes that note. That card stays in *Resolved*, where the owner
  can still reopen it with a comment.
- A card already posted that an earlier verdict decides is auto-approved
  (`[a]`), its basis quoting those words and naming their card (`RULES.md`
  `review/auto-approve-what-is-decided`).
- Only the owner's own words decide. A question of theirs, or a recommendation
  they have not answered, decides nothing.
- A design review that asks for a new owner card says which earlier verdict it
  checked and why that verdict does not decide it.

### Writing a card

**All the judgement call needs, clear and concise.** A card holds all the
information its judgement call needs, and nothing more (`RULES.md`
`docs/cards-clear-and-concise`).

- It opens with what changes for the owner, in one or two plain sentences,
  before any option, written for someone who has not read the internals.
- It reads well: short paragraphs, bullets, plain words, no jargon, and one
  clear question. Not a wall of text.
- It asks one decision. A detail that carries choices of its own, its own A, B
  or C, is a card of its own, linking the first (`RULES.md`
  `docs/one-decision-per-card`).
- Low-level detail the decision does not need (benchmark method, rigs, per-run
  numbers, gate thresholds) goes in a linked log or analysis file.
- Pictures where the thing judged is visual (pictures, UI, models, renders), shown
  rather than described; a gameplay mechanic is fine as well-written text. A
  card's picture is a file committed beside its document under `docs/`, so every
  clone and GitHub show it: never a path into the ignored artifacts folder, and
  never an owner's upload, which is for a thread. Every picture is on disk and
  added to git before the card is committed: `just symphony-cli docs-audit
  --check` refuses an open card whose picture does not load, and any card's
  picture that is not a file git tracks, judged on what the checkout wrote (the
  card's text and lines its fork from main did not hold, so an answer to main's
  card is never refused for main's pictures, nor a verdict's branch for an
  agent's). On main itself, which has no fork of its own, it only warns of a
  picture git does not track, on disk or not. A document's own pictures are
  judged the same way, line by line: a branch's line showing one git does not
  track is refused, the owner's callouts aside. The audit reads every form a
  picture takes: `![alt](path "title")`, a reference `![alt][label]` to its
  `[label]: path`, raw `<img src="path">`, and this server's own URL
  (`http://127.0.0.1:8765/<project>/…`) for a file of the project. The app holds a
  branch's card asking the owner back until its pictures are there.
- A results or comparison card says how each option looks and how well it
  performs, one plain line each, such as "all three cost the same on the GPU,
  under 0.02 ms apart".

(`RULES.md` `docs/cards-clear-and-concise`.)

## Versions and waiting decisions

A document has no status: it is always open to review, and what the owner has
yet to answer is its undecided cards. The document reference describes
[versions and branch comparisons](../product/DOCUMENT.md#versions-and-branch-pages).

Only the owner approves a card (`RULES.md` `docs/no-self-approval`), and an
approved card stays closed: no agent reopens it with a question on it
(`RULES.md` `review/decided-is-not-asked`). A later change to what it approved
is one of two kinds, never both:

- **A consequence of the choice**: whatever follows from it while the option
  chosen stays the one the card described: a correction, a name it implied, a
  cost or a trade it did not state (even one another option would not have
  carried). It is a plain agent line on the card saying what changed, quoting
  the owner's words it rests on and answering any owner note still open there.
  The card stays resolved, and the owner can reopen it with a comment.
- **A material change**: only one that would make the option chosen another
  than the one the card described. It is a new card of its own, linking the
  approved one.

A question the approved choice does not answer is neither: it is a new card of
its own, put to the owner only if no earlier verdict decides it ([before a card
goes to the owner](#before-a-card-goes-to-the-owner)).

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

Anywhere but inside a decision register, which holds cards only: a callout
between its markers would stop every later verdict on its cards, so a comment
left on a register's group or topic heading is refused, the save saying to put
it on the card itself (its **Verdict / message**) or on a block outside the
register.

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
- **A verdict's uploads are committed with it.** When the server commits what
  the owner saved on main, a card's line or note or a document's callout, each
  upload that line names goes into the same commit, at the path its link names
  (`artifacts/uploads/<sha256>.<ext>`, whatever the project's artifacts folder
  is called), so a clone and GitHub show the owner's words as written. It
  commits only the upload itself: a regular file in the store, of at most the
  upload cap, whose SHA-256 is its name. Anything else under that name (a link,
  other bytes, a larger file) is left out with a note, and the verdict is saved
  without it. Uploads on an ask, a thread message or a new task are working
  evidence and stay in the store alone. The docs audit warns, "not in a clone",
  of a line naming an upload git does not track.
- **How they are served.** `GET /artifacts/uploads/<sha256>.<ext>`, also under
  `/@<tree>/`, answers a stored name only, with its fixed type, `nosniff` and
  a script-free sandboxing `Content-Security-Policy`; pictures and text open
  in the browser, PDF and Office files download. Nothing is ever served as
  HTML or script. `POST /api/upload?name=<file name>` takes the raw bytes
  and keeps the Host and Origin checks of every other save.

## On a phone

The product references show the [document](../product/DOCUMENT.md#on-a-phone) and
[card editor](../product/CARDS.md#on-a-phone) on a phone. The app limits the cost of pictures over a
mobile link (`symphony::images` and `apps/symphony-web`).

- **A picture reserves its space.** Every local PNG the app draws carries its real `width` and
  `height`, so its space is reserved before it arrives.
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
- **Working evidence lives in `artifacts/`, which is gitignored.** The app
  shows it on this machine and nowhere else, and the docs audit warns of a
  document's link there that this machine or a clone lacks. What a document or
  a card shows is committed beside it, and the audit refuses a picture a branch
  shows that git does not track ([Writing a card](#writing-a-card)); the
  uploads a verdict names are committed with the verdict
  ([Attaching files](#attaching-files)).
- **The board sees `docs/` only**, and skips `docs/history/`.

## HTML mockups in cards

A card line `![Flight HUD A](/ui/mockups/hud-a.html)` (an image whose target is an `.html` file,
absolute or relative to the page) embeds a mockup. A plain link to an `.html` stays a link; a PNG
keeps the picture path. The document reference describes
[pictures and mockups in the reader](../product/DOCUMENT.md#pictures-and-mockups).
The frame uses the mockup's native 1280x800 size; the card's comparison row (Base, A, B, C above
its lines) uses the same embedding for an `.html` candidate.

- **Sandbox.** The frame is `sandbox="allow-scripts"`: scripts run for animation, with no same-origin access to the
  app, no top navigation, no forms or popups. The server also sends `Content-Security-Policy: sandbox allow-scripts`
  with every raw file it serves from the project (not only `.html`: an `.svg` a mockup navigates to would otherwise run as
  the app), so a mockup opened full size has the same reach as the one in a card. `/api/*` also refuses `Origin: null`,
  which is what a sandboxed page sends.
- **Assets.** The page's CSS, JS, SVG and images are served like any file under the project (`artifacts/`, or the tracked
  mockup folder under `docs/`: `tokens.css` plus one component file per element); nothing outside the repo is.
- **Source.** Compose a screen from one shared CSS file of tokens and one file per component, assembled per page; a
  PNG is only a snapshot for a thumbnail. Tracked sources beat `artifacts/`, which is gitignored.

The client renders media in `apps/symphony-web/src/media.tsx`; its layer over a drawn page is
`media-layer.ts`, and the native server confines raw media to the selected
checkout in `apps/symphony-server/src/media_files.rs`. Browser coverage returns after
the Rust migration, including attempts to read, write or navigate the parent
from a mockup.

## Cards link each other

In the app, a card slug named in a card (after *Replaces:*, or in bold in an agent line such as "Answered by **x**") links to that card, in any doc.
