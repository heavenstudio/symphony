# Where decision cards live

A proposal for where the owner's decision cards are written and what the decision register,
`docs/DECISIONS.md`, becomes. It covers which documents may hold a card, what the register's page
shows, what changes in instructions alone and what in the Symphony, and how the cards written so
far would move. [Its card](#the-card), at the end of this page, asks the one
choice. The card is itself written the way this page proposes: in the analysis that argues it,
inside a register region. Nothing in the product changes with this proposal, and no card moves.

Every figure is of main at `497797177`, read on 2026-11-20 and 2026-11-21.

## Today

Every card is written in `docs/DECISIONS.md`, as the root guide asks ("a decision the owner must
make is a card in DECISIONS").

| | |
| --- | --- |
| Cards | 88: 2 approved, 1 auto-approved, 11 rejected, 71 resolved, 3 dismissed |
| Size | 378 KB in 1,959 lines, about 95,000 tokens to read whole |
| Settled | the 75 cards in *Resolved* hold 322 KB, 85% of the file |
| Growth | 173 KB on 2026-11-13, 259 KB on 11-15, 315 KB on 11-17, 373 KB on 11-19 |
| Writes | 228 commits since the file began on 2026-11-11; 10 of the 41 branches not merged into main change it now |
| Links into it | 138 links of the form `DECISIONS.md#SLUG` in 62 files, to 69 cards |

Where each card's argument is, by the link it ends with:

| Its link names | Cards | Bytes |
| --- | --- | --- |
| an analysis under `docs/analysis/` | 71, in 43 analyses: 29 with one card, 14 with two to seven | 297 KB |
| a reference document (`TASKS.md` 4, `RELEASES.md` 4, `REVIEW.md`, `CHECKS.md`, another card) | 11 | 80 KB, with the 6 below |
| nothing | 6 | |

A card's bulk is its dated lines: the agent's posts (a base, pictures, what each option costs,
the earlier verdicts checked) and the owner's answers, 4.3 KB a card on average. The busiest
analysis, `reports-and-monitoring.md`, argues 7 cards that hold 27 KB.

What this costs:

- **Reading.** An agent that reads the register whole spends about 95,000 tokens, most of them on
  settled cards that have nothing to do with its task. The rule that nothing an earlier verdict
  decides is asked again ([REVIEW](../REVIEW.md#before-a-card-goes-to-the-owner)) sends every agent
  that posts a card through it to find the verdicts that bear on its own.
- **Writing.** Every concept task's branch adds its card to the same file, and every owner answer
  on main rewrites it. Two branches posting cards meet in one file, and a land with a conflict
  there takes a merge of main.
- **Finding.** A card sits in a topic heading of the register, away from the analysis that argues
  it; the analysis links its card back across folders.

## What already reads a card anywhere

The Symphony already reads a card in any document its entrypoints serve. The register file is
special in two small ways only.

| Part | Where it reads cards |
| --- | --- |
| The board, `just symphony-status` and its `--json` (`crates/symphony/src/board.rs`, `documents`) | every document the entrypoints show outside their history folders |
| The owner's queue | the same cards; a card's category is the first folder under `docs/` (`symphony_api::queue_category`): `analysis` for a card in an analysis, `docs` for one in `DECISIONS.md` |
| A page's card and its verdict editor | any page; an owner's save writes the card's own file and regroups it only where a register region holds it (`crates/symphony/src/edit.rs`, `set_decision`) |
| `just symphony-status --tidy` | every document holding a register region, the project's register first (`board.rs`, `register_files`) |
| A branch's posted cards (`crates/symphony/src/branches.rs`) | the documents holding `<!-- register:start -->` at the branch's HEAD (`registers`, one `git grep -l`); a card is matched by its slug across them, so a card moved from one register to another is the same card |
| A task's wait on `decision:NAME` (`crates/symphony/src/decided.rs`, `said`) | the owner's lines under every card every page holds, main's and each branch's, by slug |
| A card named in bold, or after *Replaces:* (`crates/symphony/src/render/links.rs`, `card_docs`) | the first live document holding that slug |
| The owner stamp from a card (the board task `tools/owner-stamp-from-card`, in progress) | a card in any project document the branch posted |
| The lint of slugs in prose (`just symphony-status --lint`) | refuses a slug outside a register region, but passes `docs/analysis/` and `docs/history/` |
| A project's tools (`symphony render-card PATH SLUG`, [TOOL_COMMANDS](../TOOL_COMMANDS.md)) | the document route given |

The project's `[project] register` setting ([PROJECT_FILE](../PROJECT_FILE.md)) decides only the
home page's *Decision register* link and the order `--tidy` takes registers in.

So a card written in an analysis, inside a register region, reaches the owner's queue from its
branch, shows on the branch's page with its verdict editor, ends a `decision:` wait when answered,
and is linked by its name, exactly as a card in `DECISIONS.md` does. This page's card is one.

### What does not work yet

1. **One name, two cards.** `sort_register` refuses a slug twice inside one region, and nothing
   refuses it across documents. Two documents holding one slug are two cards to the queue, while a
   wait on the name ends on either one's verdict, a bold link opens the first, and the branch scan
   reads both as one card.
2. **A card outside a region** is read on main but never posted from a branch: the owner does not
   see it until its branch lands.
3. **The register's page** lists only the cards its own file holds. A card elsewhere is in the
   queue and on its own page, but on no list of every decision.
4. **Triage's categories.** `symphony triage stats` gives each card the category
   `design · DOCUMENT` where no plugin names one, so today every card is in one category; spread
   over 43 analyses, the statistics fall into 43 categories of one to seven cards.
5. **A region's headings.** A save regroups a region under the register's own headings, `## Needs
   your decision` down to `## Resolved`, with a `### topic` under each. In an analysis these are
   the last headings of the page. This page ends that way.

## Where a card is written

**In the analysis that argues it**, inside a register region at the end of that analysis, under a
heading of its own. The analysis holds the evidence the card links today; the card's dated lines
are a record of how its question was decided, and `docs/analysis/` is a folder of records, which
the lint of slugs in prose already passes.

**Never in a reference document or a guide.** `TASKS.md`, `RELEASES.md`, a package's guide and
the rules say how things are now, and carry neither the owner's words nor who decided what and
when (RULES.md `docs/no-ledger-in-documents`, `history-is-not-reference`). A card's history in
one of them would be the ledger those rules keep out. A reference document links its card, as it
does today.

**In `DECISIONS.md` when it has no analysis.** A card about a rule, a setting or a small choice
whose card holds all it needs (11 of today's cards link a reference document, 6 nothing) is not
worth an analysis of its own (RULES.md `docs/cards-clear-and-concise`). The register is its home,
the catch-all.

A card moves once at most: a card posted where it belongs is never moved again, so a link to it
stays good.

## What DECISIONS.md becomes

1. **Unchanged.** It keeps every card it holds and is the home of cards with no analysis. A card
   written elsewhere is on no list.
2. **The catch-all, its page listing every other card live.** The file holds only the cards with
   no analysis. The server draws its page with every card held elsewhere after its own, one line
   each (its mark, its name, its first sentence, a link to it), grouped as the register groups
   (*Needs your decision* down to *Resolved*), each line holding the card's name as its anchor.
   Nothing is copied into a file, so the list is never out of step with a card and never
   conflicts. `just symphony-status --cards` prints the same list for an agent, about 90 lines
   in place of 1,959. A plain clone or GitHub shows the cards in their analyses and the catch-all,
   not the list.
3. **A committed copy.** The file holds a line per card in every document, rewritten by every post,
   every owner save and `--tidy`, above the catch-all's region. GitHub and a clone show the list.
   Each owner save writes two files; every branch that posts or answers a card changes the copy,
   so landings conflict on it and the land must write it again; and the copy's lines (`- **slug**`,
   no mark) need an exception from the lint of slugs in prose and from the region's rule that it
   holds cards only.

A full copy of each card, its dated lines included, is ruled out. Two copies of one slug are two
cards to the queue, so the owner would see each twice and could answer either, and the readers in
the table above would each pick one copy by its own rule.

## Instructions and the system

**Instructions, under every option but where it says.** What tells an agent where to write a card:

- the root `AGENTS.md` ("a decision the owner must make is a card in DECISIONS");
- RULES.md `docs/no-ledger-in-documents`, which names `docs/DECISIONS.md` as where the dated
  history lives, and the link form `[its card](DECISIONS.md#SLUG)` it gives;
- `docs/REVIEW.md`, *Start with the decision register* and *One app, every worktree*;
- `docs/README.md`, *Design and history*;
- the shipped skills under `crates/symphony/shipped/skills/`, which every project's
  `symphony init` writes: `apply-review` and its rewrite brief tell an agent to read
  `docs/DECISIONS.md`, and `create-task`'s brief form cites the approved cards a task builds.

They say: write a card in a register region at the end of the analysis that argues it; a card
with no analysis goes in the register; never in a reference document; link a card at its home
(`[its card](analysis/NAME.md#SLUG)`); look for earlier verdicts across every document
(`just symphony-status --cards` under B, `--json` or a search of `docs/` otherwise).

**The system, under B and C:**

- **One name, one card.** `just symphony-status --lint`, and the docs index's list of cards
  (`crates/symphony-docs/src/check.rs`), refuse a slug two live documents hold, naming both. A
  consuming project whose documents already hold such a pair is red at the release that brings the
  check, so its task carries a `Consumer-Note`.
- **Triage's category** falls back to the queue's category, the document's first folder, where no
  plugin names one.
- **The list**: under B, the register page's live list and `--cards`; under C, the generator, its
  call from every save and post, and the land writing the copy again on a conflict.

Not needed under any option: reading a branch's cards outside a region. The instruction asks for
the region, the branch scan stays one `git grep -l` per worktree, and about 160 worktrees take
about 3 s to scan already ([REVIEW](../REVIEW.md#one-app-every-worktree)).

## How the cards written so far move

- **Only new cards.** Nothing moves. The register stays at 378 KB of mostly settled cards, which
  agents still search, and grows by the catch-all's cards and the owner's lines on old ones.
- **By area,** as the next task in an area touches its cards. No task of its own, but for a long
  time an area's cards are in two places, each such task's diff carries a move it did not need, and
  nobody can say when the move is done.
- **Once.** One documents-only task with a throwaway script, deleted once it has run: it lifts
  each of the 71 cards, word for word, into a region at the end of the analysis its link names;
  rewrites the relative paths in the 28 cards that show pictures or link files; and rewrites the
  138 links `DECISIONS.md#SLUG` in 62 files to the card's home. The docs audit and
  `status --lint` are its evidence. The register falls from 378 KB to about 80 KB, its 17 cards
  with no analysis.

  It runs when no moved card waits on an agent and few live branches change the register. A branch
  that changed a moved card meets a conflict in documents alone at its land, which its one merge of
  main settles (RULES.md `workflow/never-chase-main`), its lines put on the card in its new home.
  The branch scan already matches a moved card by its slug, and the owner's lines saved in a
  finished branch's worktree are laid on main's copy wherever main now holds it.

## The choice

- **A: new cards in their analyses, by instructions alone.** One task changes the words above;
  nothing in the Symphony changes and no card moves. `DECISIONS.md` keeps its 88 cards and the
  cards with no analysis, and lists no card written elsewhere. Nothing stops two documents using one
  name.
- **B: every card in its analysis, the register a live list and the catch-all.** The instruction
  task; the name check and triage's category; the register's live list and `--cards`; one move of
  71 cards and 138 links. The register keeps the cards with no analysis.
- **C: as B, with a committed copy.** B's tasks, with the generator in place of the live list. The
  list shows in a plain clone and on GitHub.

B is recommended. It puts each decision with the analysis it belongs to, keeps the register as
the catch-all for cards with no home, and gives the owner one list of every decision and an agent
a list of 90 lines, with nothing copied that can drift or conflict. A leaves the 378 KB in place
and the list of decisions incomplete. C's copy is a second place to keep in step, conflicting at
landings, for a list the app can draw from the cards themselves.

## What it costs, in slices

1. **The instructions** (A, B and C): the guides, the rules' link form, the review guide, the
   docs index's page and the shipped skills above. Documents and skills only.
2. **One name, one card** (B and C): the check in `status --lint` and the docs index, with
   triage's category, each tested on a slug in two documents, one in a document and a history
   record, and one in two regions of one document.
3. **The register's list** (B): the page's list and `--cards`, tested on cards in several
   documents and on a branch's card, with a picture of the register page wide and on a phone.
   Under C, the generator instead: tested on a post, a save, a tidy and a conflict.
4. **The move** (B and C): documents only, after slices 1 and 2 land, so the moved cards are
   checked by the name check and agents already write new cards at their homes.

## Tradeoffs

- **Two places to look.** A card is in its analysis or in the register. The register's list (B)
  and the queue name every card's home, and the name check keeps one card to a name.
- **Analyses grow.** A moved analysis gains its cards' lines: 4.3 KB a card on average, 27 KB for
  `reports-and-monitoring.md`. An agent reading an analysis reads its card's history with it, which
  is the history of the choice the analysis argues.
- **Headings.** An analysis ends with the register's group heading and a topic, as this page does.
- **A clone.** Under B a clone shows each card in its document, and the list only in the app.

## The card

<!-- register:start -->
## Needs your decision

### Decision cards

- [ ] **cards-in-their-documents** — Each decision card would be written at the end of the analysis that argues it, instead of in `docs/DECISIONS.md`, which today holds all 88 cards in 378 KB. You would still answer every card from the queue as today; opening one shows its analysis above it. What should `DECISIONS.md` become, and do the cards written so far move? A: only new cards go in their analyses; `DECISIONS.md` keeps all its cards and lists no new one. B: every card moves once to its analysis; `DECISIONS.md` keeps the 17 cards that have none, and its page lists every other card live, one line each, linking it. C: as B, but `DECISIONS.md` holds a committed one-line copy of every card, rewritten on each post and answer. Recommended: B. Approve and say A, B or C in the note, or comment. [[analysis/cards-in-their-documents#The choice]]
  [2026-11-21 13:20] agent — Posted for the board task `docs/decisions-live-with-their-documents`, on your ask of 2026-11-20: "we should not be hoarding all decisions in this file. The ideal scenario is that decisions are hopefully added or connected to the particular files that they belong at. [...] Decisions.md could be a sort of catch all that just has a copy of a decision made in another place?". This card is written the proposed way, at the end of its analysis, and reaches your queue as any card does.

    **Base, today.** Every card is in `DECISIONS.md`: 88 cards, 378 KB, about 95,000 tokens to read whole, 85% of it settled cards. It more than doubled in six days (173 KB on 11-13) and took 228 commits in ten; 10 of the 41 unmerged branches change it now. 71 cards link an analysis, 43 analyses in all; 17 link none.

    **What works anywhere already.** The queue, a card's page and its verdict, your answer ending a task's wait on a card, and a link to a card by its name read a card in any document. A card in an analysis needs only the register's markers around it to be seen from its branch before it lands, as this one is.

    **What each costs.**
    - **A**: one task of instructions (the guides, the review guide, two shipped skills); no code, nothing moves. `DECISIONS.md` stays 378 KB, its page shows no new card, and nothing stops two documents using one card name.
    - **B**: A's task; a check that a card's name is used once across documents; the register's page drawing every card held elsewhere, and the same list in the terminal for agents (about 90 lines); one move of 71 cards and the 138 links to them, documents only, by a throwaway script, at a quiet hour. `DECISIONS.md` falls to about 80 KB.
    - **C**: as B, with a generator run by every post, answer and tidy in place of the live list, and a conflict on the copy at landings, which the land must write again. Its gain over B: the list shows in a plain clone and on GitHub.

    **Why B.** Each decision sits with the analysis it belongs to, the register stays the catch-all for cards with no home, and you get one list of every decision without a copy that can drift or conflict. A leaves the 378 KB in place. C's copy is a second place to keep in step, for a list the app can draw.

    **Never in a reference document.** Under every option a card is not written in a document like `TASKS.md`: those say how things are now and carry no dated lines of yours, so they link their card, as today.

    **Earlier verdicts checked.**
    - **guides-keep-owner-quotes** (B): your words and who decided what live on the cards and the threads, not in what agents read as current. Every option keeps that, which is why none writes a card into a reference document. It does not say which file holds a card.
    - **pictures-in-every-clone** (F): a card's pictures are committed beside its document. A moved card keeps its pictures where they are, its paths rewritten.
    - **contract-tool-route** (auto-approved A): a project's tools draw a card by its document and name, which works wherever the card is.

    None of them decides where a card is written.
<!-- register:end -->
