# Where decision cards live

A proposal for where the owner's decision cards are written: in any document, by default the one
each card decides about, as the consuming project's are. It names what makes that happen there
and what this repository's guidance says instead, then covers the words to change, what the
Symphony must do, what the decision register, `docs/DECISIONS.md`, becomes, how the documents
could be laid out by area and how the cards written so far would move.
[Its cards](#the-cards), at the end of this page, ask the two choices. Nothing in the
product changes with this proposal, and no card moves.

Every figure of this repository is of main at `8ccc91eb2`, and every figure of the consuming
project is of its main at `c4d2fc10f`, both read on 2026-11-21. A path is of this repository
unless it is said to be the consuming project's.

## Today, here

Every card is written in `docs/DECISIONS.md`.

| | |
| --- | --- |
| Cards | 88: 2 approved, 1 auto-approved, 11 rejected, 71 resolved, 3 dismissed |
| Size | 378 KB in 1,959 lines, about 95,000 tokens to read whole |
| Settled | the 75 cards in *Resolved* hold 322 KB, 85% of the file |
| Writes | 228 commits, merges aside, since it began on 2026-11-11 |
| Links into it | 133 links of the form `DECISIONS.md#SLUG` in 60 files, to 68 cards |
| What each card links | 72 an analysis (43 analyses, 298 KB), 10 a reference document (`TASKS.md` 4, `RELEASES.md` 4, `REVIEW.md`, `CHECKS.md`; 53 KB), 6 nothing (26 KB) |

An agent that checks the earlier verdicts before it posts a card reads the whole file. Every
branch that posts a card, and every answer the owner saves, writes the same file. And a decision
about releases is read on a page away from `RELEASES.md`, under a topic heading among 35.

## The consuming project

Its cards are spread over the documents they decide about: 841 cards in 38 documents, 128 of
them (15%) in its `docs/DECISIONS.md`.

| Its folder | Cards | In |
| --- | --- | --- |
| `docs/ships/` | 369 | a ships page per faction, `CAPITAL_SIGNATURES.md`, `SHIP_SHAPES.md` |
| `docs/factions/` | 175 | a page per faction |
| `docs/DECISIONS.md` | 128 | the register |
| `docs/combat/` | 56 | `GEAR.md`, `WEAPONS.md`, `DEFENCES.md`, `STATUS_EFFECTS.md`, `CAPABILITIES.md`, `BALANCE_ARENA.md` |
| `docs/art/` | 54 | `UI_DESIGN.md`, `UI_COMPONENTS.md`, `UI_TEMPLATES.md`, `HULL_MODELS.md`, `UI_PRINCIPLES.md` |
| `docs/engineering/` | 47 | `TESTING.md` 11, and 36 in seven analyses under `engineering/analysis/` |
| `docs/workflow/` | 6 | `MERGING.md` |
| `docs/analysis/` | 5 | two analyses |
| `docs/game/` | 1 | `VISION.md` |

A reference document there ends with its own register: `docs/combat/WEAPONS.md:203`,
`docs/engineering/TESTING.md:1275` and `docs/workflow/MERGING.md:679` each open a `## Register`
heading over a register region, after everything the document says about how things are.

### What makes it so there

No one sentence there says "write a card in the document it belongs to". Five things do it
together, all in the consuming project's checkout:

1. **Its index calls the register a roll-up, not the home.** `docs/README.md:5-6`: decisions
   waiting on the owner are in the register "every other page's cards roll up to". Its root
   guide never says where a card goes: `AGENTS.md:25-32` describes the folders and sends an
   agent to its review guide.
2. **Its review guide names a home for each kind of card.** `docs/workflow/REVIEW.md:72-74`:
   "Art lives on its faction's page", in the register of the faction page it belongs to. `:88`:
   each version of a hull's model is a card on the hull models page. `:104`: a changed golden
   frame is a card in the testing document. `:262-263`: a hull is reviewed on its faction's
   ships page. `:83-84`: the tidy pass covers every register under `docs/`, "not only" the
   register file.
3. **Its posting commands take the page.** `.agents/skills/generate-art/SKILL.md:91` posts a
   picture card with the ships page as its first argument; `.agents/skills/hull-models/SKILL.md:44-47`
   posts a model card on the hull models page; the golden card command writes the testing
   document. An agent that posts with them cannot put the card anywhere else. The faction
   pages, the ships pages and the hull models page hold 532 of the 841 cards.
4. **Its apply-review skill speaks of registers, in the plural.** `.agents/skills/apply-review/SKILL.md:36-38`
   has the tidy pass regroup "every register", the register file and each faction page;
   `:60-63` has a replacement card added "inside the register region", naming no file. So a
   document written with cards of its own, as its testing and merging documents were, keeps them
   and their replacements.
5. **Nothing forbids it.** Its rules hold `history-is-not-reference` (`RULES.md:223-228`) and no
   rule that names the register file as where the dated lines live.

Its documents are a folder per topic (`docs/README.md:8-20`, `AGENTS.md:25-29`): `game`,
`combat`, `factions`, `ships`, `stations`, `art`, `workflow`, `engineering`, `data`, `analysis`
and `history`, each with a `README.md` that says what it holds, and `engineering/` with an
`analysis/` of its own. A topic is small enough that a decision plainly belongs to one page, and
the owner's queue, which names a card's category by its first folder, sorts by those topics.

### What this repository says instead

| Where | What it says | Its effect |
| --- | --- | --- |
| `AGENTS.md:210-211` | "A decision the owner must make is a card in DECISIONS, not a question in chat." | names the one file |
| `RULES.md:396-401`, `docs/no-ledger-in-documents` | the owner's words and the dated history "live in the decision register (`docs/DECISIONS.md`, each card's dated lines)"; a document links its card as `[its card](DECISIONS.md#SLUG)` | names the one file, and its link form |
| `docs/README.md:66-67` | the register "holds Symphony's product and engineering decisions, one card each" | the home, where the consuming project's index says roll-up |
| `docs/workflow/REVIEW.md:172-173`, `:278`, `:282` | the register file "lists the proposals"; the link form again; the lint's exceptions are `docs/history/` and `docs/analysis/` | no section says where a card is written |
| `crates/symphony/shipped/skills/apply-review/SKILL.md:39-40` | "Read `docs/DECISIONS.md` in full" | every project's agent reads one file for the verdicts |
| `crates/symphony/shipped/skills/apply-review/rewrite-brief.md:5`, `:21`, `:27` | the owner reviews "as one-line decision cards in `docs/DECISIONS.md`" | every project's rewrite agent is told one file |
| `docs/workflow/CHECKS.md:285-286` | the records are the register file, `docs/analysis/` and `docs/history/` | a card's lines elsewhere are judged as a document's prose |

The work, create-task and triage skills and their `project.md` pages say nothing of where a
card is written: `create-task/SKILL.md:65` cites approved cards by document and heading, and
`triage/SKILL.md:16-24` checks earlier verdicts without naming a file. The shipped apply-review
skill already has the generic words, `:37` ("the project register and its per-topic pages") and
`:68-70` (a new card "inside the register region"), beside the two that name the one file.

This repository's documents are flat: 19 reference documents and the register at the top of
`docs/`, three product specs in `product/`, 48 analyses and 186 pictures in one
`analysis/` folder, and `history/`. `docs/README.md` sorts the references under three headings
(product use, operation and setup, development and contracts) that are no folders, so a card
in any reference document has the queue category `docs`
(`crates/symphony-api/src/lib.rs:430-439`).

## The default: a card in the document it decides about

**A card is written in the document that states, or will state, what its verdict changes**:
a reference document (`RELEASES.md` for how a release is cut), a product spec (a page's
behaviour), the analysis that proposes something not built yet. It sits in a register region at
that document's end, under a `## Decisions` heading, added where the document has none.

**The prose above the region says how things are; the region is that document's record.** This
is what the two rules about history ask (RULES.md `history-is-not-reference`,
`docs/no-ledger-in-documents`): no dated line or owner's word in what agents read as current. A
card's lines stay on the card, as today, and the card is at the end of the page it decided
rather than in another file. A reader after how things are stops at the region's first marker.

**`DECISIONS.md` holds only a card no one document holds**: a rule or a guide's wording (the
guides are outside `docs/`, which is all the board reads), or a choice across several documents.

A card is posted once and never moved, so a link to it stays good.

## What already works in any document

The Symphony reads a card in any document its entrypoints serve.

| Part | Where it reads cards |
| --- | --- |
| The board, `just symphony-status` and its `--json` (`crates/symphony/src/board.rs`) | every document the entrypoints show outside their history folders |
| The owner's queue | the same cards, each with the category of its first folder under `docs/` |
| A page's card and its verdict editor (`crates/symphony/src/edit.rs`, `set_decision`) | any page; a save writes the card's own file and regroups its region |
| `just symphony-status --tidy` | every document holding a register region |
| A branch's posted cards (`crates/symphony/src/branches.rs`) | the documents holding a region at the branch's HEAD; a card is matched by its slug across them |
| A task's wait on `decision:NAME` (`crates/symphony/src/decided.rs`) | the owner's lines under every card every page holds |
| A card named in bold, or after *Replaces:* (`crates/symphony/src/render/links.rs`) | the first live document holding that slug |
| The lint of slugs in prose (`crates/symphony/src/lint.rs`) | passes a slug inside a region in any document |
| An agent's reply (`just symphony-reply PATH SLUG`) and a project's tools ([TOOL_COMMANDS](../../engineering/TOOL_COMMANDS.md)) | the document given |

The consuming project's 841 cards in 38 documents run on exactly this.

### What the system would need

1. **One name, one card.** Nothing refuses one slug in two documents: they are two cards to the
   queue, while a wait on the name ends on either's verdict and a bold link opens the first.
   `just symphony-status --lint` and the docs index (`crates/symphony-docs/src/check.rs`) would
   refuse it, naming both. A project whose documents hold such a pair is red at the release that
   brings the check, so its task carries a `Consumer-Note`.
2. **Two checks that find the records by path would find them by region.** This repository's
   own check for the owner's words in a document (`crates/symphony/tests/hooks/owner_words.rs:18`)
   and the docs audit's rule for recipe names (`crates/symphony/src/docs_audit/recipes.rs:39`)
   pass over the register file by its name. A card at the end of `RELEASES.md` would be judged
   as that document's prose, so both would pass over a register region wherever it is.
3. **Triage's category.** `symphony triage stats` gives a card the category `design · DOCUMENT`
   where no plugin names one. It would fall back to the queue's category, the first folder.
4. **A list of every card**, under choice C below: the register's page drawing each card held
   elsewhere, one line each, and `just symphony-status --cards` printing the same for an agent.

Not needed: a command to post a card (an agent writes the two markers and the card, as the
consuming project's agents do in its reference documents), or reading a branch's card outside a
region.

## The words to change

In this repository:

- **`AGENTS.md:210-211`** becomes: "A decision the owner must make is a card in the document it
  decides about ([where a card is written](docs/workflow/REVIEW.md#where-a-card-is-written)), not a
  question in chat."
- **RULES.md `docs/no-ledger-in-documents`**, its second sentence, becomes: "The owner's exact
  words and the dated history of who decided what, and when, live on the cards, each in the
  register region that ends the document it decides about (`docs/DECISIONS.md` for a card no one
  document holds), and on task threads; a document's prose, above its region, says how things
  are, and where the reason matters links its card: `[its card](#SLUG)` on its own page,
  `[its card](FILE.md#SLUG)` on another." Its last sentences name the regions with
  `docs/history/` as the records that keep the words.
- **`docs/workflow/REVIEW.md`** gains a section, *Where a card is written*, after the table of markers:
  "A card is written in the document that states, or will state, what its verdict changes: a
  reference document, a product spec, or the analysis that proposes it. It sits in a register
  region at that document's end, under a `## Decisions` heading: add the two markers where the
  document has none. The prose above says how things are; the region is the document's record.
  `DECISIONS.md` holds only a card no one document holds: a rule, or a choice across several
  documents. A card is posted once and never moved. Before posting, look for earlier verdicts
  in every document: `just symphony-status --json` lists every card with its file." *Start with
  the decision register* says the register is the catch-all, and *Slugs in prose* takes the
  link forms above.
- **`docs/README.md:66-67`** becomes: "Decisions are cards at the end of the document each
  decides about; the [decision register](DECISIONS.md) holds those no one document holds.
  [Analysis](analysis/) holds proposals and evidence."
- **`docs/workflow/CHECKS.md:285-286`** names the records as every register region, `docs/analysis/` and
  `docs/history/`, once the two checks read them so.

In the shipped skills, for every project (`crates/symphony/shipped/skills/`, which `symphony
init` writes):

- **`apply-review/SKILL.md:39-40`**: "Read `docs/DECISIONS.md` in full" becomes "Read every card
  `--open` and `--json` name, in the document that holds it; the project's register holds only
  the cards no one document holds."
- **`apply-review/SKILL.md:68-70`**: a replacement card is added "inside the register region of
  the document it decides about, the rejected card's own unless it decides about another; add
  the two markers at the document's end where it has none".
- **`apply-review/rewrite-brief.md:5`**: "as one-line decision cards in the register regions of
  the project's documents, `docs/DECISIONS.md` among them", with `:21` and `:27` saying "a card"
  for the register file, and a rewrite leaving every register region as it is.

Nothing more is shipped: which kind of card goes on which page is a project's own, said in its
review guide, as the consuming project's is. The work, create-task and triage skills need no
change.

## What DECISIONS.md becomes

- **The catch-all, and nothing more.** It holds the cards no one document holds. The queue, the
  board and `just symphony-status --json` are the list of every decision, as in the consuming
  project.
- **The catch-all, its page listing every other card live.** The server draws its page with
  every card held elsewhere after its own, one line each (its mark, its name, its first sentence,
  a link), grouped as the register groups. Nothing is copied into a file, so the list is never
  out of step and never conflicts. `just symphony-status --cards` prints it for an agent, about
  90 lines in place of 1,959. A plain clone shows the cards in their documents and not the list.
- **A committed copy of one line per card** is ruled out, as a full copy is: every post and
  every answer would write it, so landings would conflict on it, for a list the app can draw.

## Folders by area

The consuming project's layout would be, here:

| Folder | Holds | Today |
| --- | --- | --- |
| `product/` | what each page is for and how it behaves | `product/`, as it is |
| `design/` | how the app looks: the header, the mark, a page while it loads, the timeline | the analyses of how it looks, under `analysis/` |
| `workflow/` | how work moves: review, the board, checks, releases, the records of runs | `REVIEW`, `TASKS`, `WORKFLOW`, `CHECKS`, `RELEASES`, `MACHINE_WORK`, `RUNS`, `AGENT_ACTIVITY` |
| `engineering/` | the contracts a project or a provider builds against | `PLUGIN_PROTOCOL`, `PLUGIN_CALLS`, `TOOL_COMMANDS`, `ARTIFACTS`, `EXECUTION_ADAPTER`, `EXECUTION_ATTEMPTS` |
| `operations/` | setting up and running it | `PROJECT_FILE`, `POSTGRESQL`, `GITHUB`, `GITHUB_SETUP`, `UPSTREAM_DOWNSTREAM` |
| `history/` | records | as it is |

Each area keeps its analyses in an `analysis/` folder of its own, as the consuming project's
`engineering/analysis/` does, and each folder has a `README.md` that says what it holds. The
owner's queue would then sort by product, design, workflow, engineering and operations, with no
change to it, where today it says `docs` or `analysis`.

What it costs:

- **Every reference document moved.** 1,092 mentions of the 19 reference documents by path, in
  355 files: 623 in Markdown, 374 in the module docs of 237 Rust files, 95 elsewhere. One
  throwaway script rewrites them, and the docs audit checks every link. A link written
  `[[NAME#Heading]]` finds its document by name and does not change.
- **The consuming project's links.** Its guides link this repository's documents on GitHub 34
  times, 20 of them `docs/workflow/REVIEW.md`. They break until its own task rewrites them, which the
  landing's `Consumer-Note` says.
- **Two constants.** The lint of slugs in prose and the recipes audit pass `analysis` only as the
  first folder under `docs/` (`crates/symphony/src/lint.rs:16`,
  `crates/symphony/src/docs_audit/recipes.rs:39`); they would pass it at any depth.
- **The analyses**, which move too: 254 mentions in 73 files, 42 of them in Rust. Moved alone,
  they are about a fifth of the whole.

## How the cards written so far move

- **Only new cards.** Nothing moves. The register stays at 378 KB of mostly settled cards, which
  agents still search, and grows only by the owner's lines on old ones.
- **Once, to the document each card links.** One documents-only task with a throwaway script,
  deleted once it has run. It lifts each card word for word into a region at the end of the
  document its link names: 72 into 43 analyses, 10 into `TASKS.md`, `RELEASES.md`, `REVIEW.md`
  and `CHECKS.md`. It rewrites the relative paths in the 28 cards that show pictures or link
  files, and the 133 links `DECISIONS.md#SLUG` in 60 files. The register keeps 6 cards and
  falls to about 26 KB. The docs audit and `status --lint` are its evidence.

  A settled card stays with the analysis that argues it rather than moving to the reference
  document its subject is in: its link is the one home a script can read, and its argument is
  there. A new card about that subject goes in the reference document.

  It runs when no moved card waits on an agent. A branch that changed a moved card meets a
  conflict in documents alone at its land, which its one merge of main settles (RULES.md
  `workflow/never-chase-main`). The branch scan matches a moved card by its slug already.

## The choices

**Where cards are written** (the first card):

- **A: the default, by words alone.** One task changes the words above. New cards go in their
  documents; the 88 stay in the register; nothing in the Symphony changes.
- **B: the default, the checks and the move.** A's task; one name, one card, with the two checks
  and triage's category; the 88 cards moved once. `DECISIONS.md` is the catch-all and nothing
  more: the queue and the board list every decision.
- **C: as B, and the register's page lists every card live**, with `--cards` for an agent.

B is recommended. It is how the consuming project runs 841 cards: the queue is the owner's list,
`--json` is an agent's, and the register is small. A leaves 378 KB that every agent still reads
and lets two documents take one name. C's list can follow B as a task of its own if the owner
misses one page of every decision; nothing in B has to be undone for it.

**Folders by area** (the second card):

- **A: as today.** References at the top of `docs/`, analyses in one folder.
- **B: a folder per area, every document moved once.** The table above, at its full cost.
- **C: the areas for analyses alone.** `docs/analysis/` becomes `product/analysis/`,
  `design/analysis/`, `workflow/analysis/`, `engineering/analysis/` and `operations/analysis/`;
  the 19 reference documents stay where every guide, module doc and the consuming project link
  them. A card in an analysis sorts by area in the queue; one in a reference document stays `docs`.

C is recommended. Most cards are argued in analyses, so most of the queue sorts by area, and 48
analyses with 186 pictures stop being one folder, for about a fifth of B's rewrites and none of
its breaks outside this repository. B is the same shape as the consuming project's, at five times
the rewrites and a break in the consuming project's guides.

## What it costs, in slices

1. **The words** (A, B and C): the guide, the rule, the review guide, the docs index and the two
   shipped apply-review pages. Documents and skills only.
2. **The checks** (B and C): one name, one card in `status --lint` and the docs index; the two
   checks reading records by region; triage's category. Each tested on a slug in two documents,
   in a document and a history record, and in two regions of one document.
3. **The move** (B and C): documents only, after slices 1 and 2 land.
4. **The register's list** (C): the page's list and `--cards`, with a picture of the page wide
   and on a phone.
5. **The folders** (the second card, B or C): one scripted move with the constants, after the
   card move so no card's link is rewritten twice.

## Tradeoffs

- **A reference document grows by its cards.** A card's lines average 4.3 KB. They are below
  everything the document says, behind a fold on its page, and an agent reading for how things
  are stops at the marker.
- **Two homes for one subject for a while.** After the move, `RELEASES.md` holds 4 cards about
  releases and the analyses that argued the others hold theirs. The queue and `--json` name each card's
  file.
- **A region's headings.** A save regroups a region under the register's own group headings, so
  a document's last headings are *Needs your decision* down to *Resolved*, under `## Decisions`.
- **No one page of every decision** under B, in the app or in a clone: the queue lists what is
  open, and a settled card is found on its document or by `--json`.

## The cards

<!-- register:start -->
## Waiting on an agent

### Decision cards

- [x] **docs-folders-by-area** — The documents under `docs/` would be sorted into folders by area, as the consuming project's are: product, design, workflow, engineering and operations. Your queue names each card by its first folder, so it would sort decisions by those areas, where today every card says `docs` or `analysis`. Which documents move? A: none; the layout stays as it is. B: every document, once: the 19 reference documents and the 48 analyses, each area with its own `analysis/` folder. C: the analyses alone, into `product/analysis/`, `design/analysis/` and so on; the reference documents stay where they are. Recommended: C. Approve and say A, B or C in the note, or comment. [[analysis/cards-in-their-documents#Folders by area]]
  [2026-11-21 10:21] agent — Posted for the board task `docs/decisions-anywhere-by-default`, on the second half of your rejection of **cards-in-their-documents**, that the consuming project's "docs have better structure between engineering, product, design etc." It goes with **cards-in-any-document** and can be answered apart from it.

    **Base, today.** 19 reference documents and the register at the top of `docs/`, three product specs in `product/`, and 48 analyses with 186 pictures in one `analysis/` folder. The docs index sorts the references under three headings that are no folders. The consuming project has a folder per topic, each with a page saying what it holds, and its engineering folder has an `analysis/` of its own.

    **The areas.**
    - `product/`: what each page is for and how it behaves.
    - `design/`: how the app looks: the header, the mark, a page while it loads, the timeline.
    - `workflow/`: how work moves: review, the board, checks, releases.
    - `engineering/`: the contracts a project builds against: plugins, tools, artifacts, execution.
    - `operations/`: setting it up and running it: the project file, PostgreSQL, GitHub.

    **What each costs.**
    - **A**: nothing. A card in an analysis says `analysis` in your queue, one in a reference document says `docs`.
    - **B**: 1,092 mentions of the reference documents by path, in 355 files, 237 of them Rust files whose comments name a document, and 254 more for the analyses. A throwaway script rewrites them and the docs audit checks every link. The consuming project's guides link these documents on GitHub 34 times; those break until its own task rewrites them.
    - **C**: the 254 mentions of analyses, in 73 files. Nothing outside this repository breaks.

    **Why C.** Most cards are argued in analyses, so most of your queue would sort by area, and one folder of 48 analyses becomes five, for about a fifth of B's rewrites. B gives the same shape as the consuming project at five times the cost and a break in its guides; it can follow C later, as the areas would already exist.

    **Earlier verdicts checked.** **product-spec-form** set the form of the specs in `product/`, which every option keeps. No card decides how the other documents are laid out.
  [2026-11-21 17:27] owner · approved — B
- [-] **cards-in-any-document** — Replaces: cards-in-their-documents. A decision card would be written, by default, in whichever document it decides about: `RELEASES.md` for a decision about releases, a product spec for how a page behaves, an analysis for something proposed and not built. The card sits at the end of that document, below everything it says about how things are. `docs/DECISIONS.md` would keep only the cards that belong to no one document. You would answer every card from the queue as today; opening one shows its document above it. How far should this go now? A: the words alone: the guides and the shipped skill say the default, new cards follow it, and the 88 cards stay in `DECISIONS.md`. B: the words, a check that no two documents use one card name, and the 88 cards moved once to the documents they link; `DECISIONS.md` keeps 6 cards and the queue is the list of every decision. C: as B, and the `DECISIONS.md` page also lists every card held elsewhere, drawn live. Recommended: B. Approve and say A, B or C in the note, or comment. [[analysis/cards-in-their-documents#The choices]]
  [2026-11-21 10:21] agent — Posted for the board task `docs/decisions-anywhere-by-default`, answering your rejection of **cards-in-their-documents**: "This isn't just about analysis docs. The idea is that decisions are made in any place, And this should be the default behavior." This card drops that card's limit to analyses, and its "never in a reference document".

    **What makes it so in the consuming project.** No single instruction, but five things together:
    - its docs index calls the register the page "every other page's cards roll up to", not their home;
    - its review guide names a home for each kind of card: art on its faction's page, a hull on its ships page, a model on the hull models page, a changed test picture in the testing document;
    - the commands its skills post those cards with take the page as an argument;
    - its apply-review skill tidies "every register" and adds a replacement card to "the register region", naming no file;
    - no rule there names the register file as the one place your words live.

    The result: 841 cards in 38 documents, 15% of them in its register. Its testing, merging and weapons documents each end with their own cards.

    **What this project says instead.** The root guide: a decision "is a card in DECISIONS". The rule on dated history: your words "live in the decision register (`docs/DECISIONS.md`)". The docs index: the register "holds" the decisions. The apply-review skill shipped to every project: "Read `docs/DECISIONS.md` in full". The analysis gives the file and line of each, in both projects, and the exact words that would replace them.

    **Base, today.** 88 cards in one file of 378 KB, about 95,000 tokens for an agent to read whole, 85% of it settled. 72 cards link an analysis, 10 link `TASKS.md`, `RELEASES.md`, `REVIEW.md` or `CHECKS.md`, 6 link nothing.

    **What works already.** The queue, a card's page and its verdict, your answer ending a task's wait, and a link to a card by its name all read a card in any document. The consuming project runs on exactly that.

    **What each costs.**
    - **A**: one task of words (the root guide, one rule, the review guide, the docs index, two pages of one shipped skill). No code, nothing moves. `DECISIONS.md` stays 378 KB and nothing stops two documents using one card name.
    - **B**: A's task; a check for one card name in two documents, with two of this project's own checks taught that a card's lines at the end of a document are a record; one move of 82 cards and the 133 links to them, documents only, by a throwaway script. `DECISIONS.md` falls to about 26 KB.
    - **C**: B, and the register's page drawing every other card, one line each, with the same list in the terminal for agents.

    **Why B.** It is how the consuming project works: the queue is your list, the register is small, and each decision is on the page it decided. A leaves the big file every agent still reads. C's list can follow as its own task if you miss one page of every decision; B leaves nothing to undo for it.

    **How the documents are laid out** is a card of its own, **docs-folders-by-area**.

    **Earlier verdicts checked.**
    - **guides-keep-owner-quotes** (B): your words live on the cards and the threads, not in what agents read as current. That stands: a card's lines stay on the card, at the end of its document, and the prose above it quotes nothing. It does not say which file holds a card.
    - **pictures-in-every-clone** (F): a card's pictures are committed beside its document. A moved card keeps its pictures where they are, its paths rewritten.
    - **contract-tool-route** (auto-approved A): a project's tools draw a card by its document and name, which works wherever the card is.

    None of them decides where a card is written.
  [2026-11-21 17:26] owner · rejected — Ideally, decisions are at the place where they “decide” not at the bottom

## Rejected and replaced

### Decision cards

- [-] **cards-in-their-documents** — Each decision card would be written at the end of the analysis that argues it, instead of in `docs/DECISIONS.md`, which today holds all 88 cards in 378 KB. You would still answer every card from the queue as today; opening one shows its analysis above it. What should `DECISIONS.md` become, and do the cards written so far move? A: only new cards go in their analyses; `DECISIONS.md` keeps all its cards and lists no new one. B: every card moves once to its analysis; `DECISIONS.md` keeps the 17 cards that have none, and its page lists every other card live, one line each, linking it. C: as B, but `DECISIONS.md` holds a committed one-line copy of every card, rewritten on each post and answer. Recommended: B. Approve and say A, B or C in the note, or comment. [[analysis/cards-in-their-documents#The choice]]
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
  [2026-11-21 09:39] owner · rejected — This isn't just about analysis docs. The idea is that decisions are made in any place, And this should be the default behavior. If you look at the star-foundry project, you can see how in there decisions are a lot more spread, docs have better structure between engineering, product, design etc. There's probably some skill or instructions in star foundry that better dictates this that is missing on this project.
  [2026-11-21 10:21] agent — Answered by **cards-in-any-document**: a card in any document is the default, reference documents included, and this card's limit to analyses is dropped. How the documents are laid out by area is **docs-folders-by-area**. What makes the consuming project's cards land in their own documents, and the words this project has in their place, are in the analysis above, by file and line.
<!-- register:end -->
