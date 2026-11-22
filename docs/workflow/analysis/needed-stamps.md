# Needed stamps

What `just land` requires of a task: the stamps its risk calls for, named by an assessor that is
never the task's executor, chosen from one catalogue, and checked on the branch's tip. The task's
assessment names its stamps and the land reads their evidence. Its card:
[the needed stamps](#needed-stamps).

## Current contract

The default land uses the assessment and the catalogue. A needed `review` is a fresh reviewer's
approve naming the tip; no task mark grants a requester approval privilege. The task page shows
which stamps stand and which the task needs.

## The catalogue

One list of the stamps a project's land can require, each with how the land knows it stands and a
guideline on where it applies and where it does not. It is `[stamps]` in `symphony.toml`, a table a
stamp, read as the board reads its own keys: from the main checkout's committed copy, so a branch
cannot change what it is held to. With none written it is the Symphony's four below. A project that
writes any writes its whole list, as `[[entrypoints]]` replace the convention.

```toml
[stamps.review]
default = true   # needed by a task no assessment has read
applies = "…"    # where the assessor needs it
skips = "…"      # where it does not
```

It is the one source, read three ways and copied nowhere:

- the assessor's prompt prints it (`just tasks stamps`, [below](#the-assessor));
- the land checks the stamps a task needs against it;
- the task page's gates draw each of its stamps, needed or waived.

A stamp's name is also how it is read: `tests`, `review`, `pictures` or `owner`. A name the
Symphony has no reader for is refused where the file is read.

## The default catalogue

Every stamp names the tip it was given at and stands on that commit alone: a later commit, a merge
of the default branch included, needs it again. `tests` and `pictures` are the executor's to
produce, `review` a fresh reviewer's, `owner` the owner's. The default, what a task nobody has
assessed needs, is `review`, which is today's rule.

### `tests`

- **Met:** the task's recorded runs at the tip, its own or work under it
  ([MACHINE_WORK](../MACHINE_WORK.md#recording-builds-and-tests)): the newest run there of each build
  and test command it ran is green, and one test run at least is among them.
- **Applies:** any change to code, scripts, recipes, build settings, or a file a test reads (a
  fixture, a shipped skill whose install is tested). The tests closest to a change are what proves
  it works. A change of the values the project file gives its settings, which the settings check
  proves alone (`symphony settings-check`, recorded as a test run: every table and key known, each
  value of its type, no name judged against a list), with no other suite.
- **Skips:** documents and guides alone; pictures and other files nothing tests; a hotfix whose
  break no test shows in the time it has, its tests coming with its postmortem.

### `review`

- **Met:** a fresh reviewer's approve naming the tip, as today: from a session that never executed,
  orchestrated or filed the task.
- **Applies:** code whose mistake reaches past the task, into what every agent runs or every client
  reads (a store, a schema, a board command, the landing, a launcher, a hook, a recipe, a wire type,
  a route, a shared component); a change to a test, a check, a budget or a run step, which is any
  path `[land] check_paths` names, since a loosened test is how a red turns green unfixed; a
  shipped skill or a rule every agent follows; a setting the Symphony does not read yet or reads
  another way, which is code; a value of the project file that is a command or a program the
  Symphony runs or that decides what lands (`[project]`'s `cargo` and `default_branch`,
  `[plugins]`'s `crates` and `process`, `[verdicts]`, `[land]`, `[releases]`, `[stamps]`,
  `[board]`'s `on_review`, `on_land`, `machine_work_reader` and `assessments`); a change that does more than one thing, or
  one the assessor cannot place.
- **Skips:** documents alone (a design, a reference page, a record); the values the project file
  gives settings the Symphony already reads (a model, an agent, a limit, a count, a name),
  written, changed or removed, with the guide's and the rules' words that repeat those values and
  nothing else: the settings check green at the tip stands in the reviewer's place, as `tests`; a
  fix of a few lines that
  leaves the failing test its task names unchanged; a hotfix restoring
  something wholly broken, read afterwards as its postmortem.

### `pictures`

- **Met:** a post on the task's thread naming the tip and showing at least one picture (a file under
  the task's artifacts folder, or an upload).
- **Applies:** a change to what a page shows or how it lays out (a view, a style sheet, a theme
  token, a page's words): before and after, phone and wide, dark and light where colour changed.
- **Skips:** a change no page shows (what a page reads, caches or weighs, a route's answer), and
  anything off the pages.

### `owner`

- **Met:** the owner's own approve naming the tip, given on the task's page.
- **Applies:** a result the owner asked, in the brief or on the thread, to see before it lands: a
  new page or look, a behaviour they will use themselves.
- **Skips:** anything else. A fresh reviewer stands for the owner, and nothing waits on the owner
  unasked.

- [r] **needed-stamps** — `just land` would require exactly the stamps a task's risk calls for: its tests, a reviewer, pictures, your own look at it, or nothing at all. A small assessor agent, never the task's executor and told only by a fixed skill, picks them from one list when the task is filed, and again from the real diff when it reaches review. A document or a hotfix then lands without waiting for a reviewer, and a task nobody has assessed still needs one, as today. Is this the design, with the default list and where each stamp applies? Approve, or comment with what to change. [[analysis/needed-stamps#The default catalogue]]
  [2026-11-14 07:52] agent — Posted for the board task `tools/land-requires-the-stamps-its-risk-names`.

    **The default list**, each with where it applies and where it does not:
    - `tests`, green recorded test runs at the tip: any change to code; not documents, nor a hotfix whose break no test can show in time.
    - `review`, a fresh reviewer's approve: what every agent runs or every client reads, a changed test or check, a skill or a rule; not documents, a fix of a few lines that leaves its failing test unchanged, or a hotfix.
    - `pictures`, posted at the tip: a change to what a page shows; nothing off the pages.
    - `owner`, your approve on the task's page: only what you asked to see before it lands.

    **What keeps it honest.** The board refuses the list, and the assessor's role, to any session that executed, orchestrated or filed the task. The assessor reads the diff itself, and takes the executor's posts as claims to check, never as instructions. A waiver holds only on the commit the assessor read: a later commit brings the reviewer back.

    **Cost.** Three build tasks: the list and the field, the task page, then the land and the skill. One small-model run per task when it is filed and each time it moves to review. The land runs nothing new. `--fast-fix` goes: a fast fix is an assessment that needs only tests.

    **Left out.** Risk levels (each stamp's guideline is the map); a project's own kinds of stamp; the board starting the assessor by itself; and any floor under the assessment: today's rule that a red's fix changing a test needs a reviewer becomes the review guideline, shown to the assessor as a fact, rather than a refusal in the land. Retiring `--fast-fix` gives up a second read: today its orchestrator reads that the red's own failing test, unchanged, ran green at the tip, and under this design nobody does, since `tests` asks only that the task's test runs there are green. Requiring a run of the test the red names is the smallest fix, if you want one.

    **Earlier verdicts checked.** **land-tests-changed-packages** (rejected: nothing automatic may block a land): this adds no run to the land. **reports-task-page-pictures** (A, a typed seam the page draws): the stamps are drawn as those gates. Choice 5 of **reports-small-choices** (a lower level replaces a higher one, everywhere): a project's own list replaces the Symphony's, and a project's own `land` replaces this check. None of them decides this card.

    This card is the only change: nothing is built until you answer.
  [2026-11-14 08:33] owner · approved — should had been auto approved, it's exactly what I had asked for
  [2026-11-15 11:13] agent — Recorded, and built as approved: a land requires the stamps a task's assessor wrote from the catalogue, and a task nobody assessed still needs a reviewer. [[TASKS#Needed stamps]] says how it works today.
  [2026-11-15 11:14] agent · resolved

## The field

`_board.needs` on the task: the stamps it needs (none, for everything waived), the tip the
assessment read (none before the task has a branch), who made it and why. It is the board's own
field, out of `just tasks field`'s reach, as the red mark is. Each change is also a
line on the thread, which keeps every assessment:
`needs tests, review at 1a2b3c4 (was review): a store query and its test, no page or wire; by HANDLE`.

`just tasks needs SLUG STAMP… --tip SHA --why WHY --as HANDLE` writes it, `none` for an empty
list. The board takes it only from the task's assessor, and refuses a stamp the catalogue lacks or
an empty reason.

## The assessor

A role in name, with no row on the task and no schema change: `just tasks needs` itself is refused,
by the rule a reviewer's verdict is held to, to a session that executed the task or work under it,
orchestrates or orchestrated it, or filed it. So the assessor is whoever writes the needs and is
none of those, and the field and its thread line name it.

- **Started by the orchestrator, never the executor:** when it files the task, and again each time
  the task moves to review, before it staffs any reviewer; by an Agent launch or `just tasks launch
  SLUG --role assessor`, on the task itself, with no subtask.
- **Neutral instructions.** Its whole prompt is `/assess-task SLUG HANDLE`, a skill the Symphony
  ships as it ships `/review-task`; a board launch already takes no `--prompt`. The skill is all it
  is told. Nothing on the thread instructs it but the owner's own posts, which only the Symphony
  writes as the owner.
- **A small model:** `[agents.models] assessor`, its default the smallest the agent offers (Claude's
  `haiku`), which `just tasks limits` names. Its decision is which of four stamps apply, from a diff
  and their guidelines.
- **What it reads, itself:**
  1. `just tasks stamps SLUG`: the catalogue; the task's needs as they stand; its branch's tip; and
     which changed paths `[land] check_paths` names, a fact the board computes, so a small model
     need not find the tests in a diff;
  2. `just tasks show SLUG`: the brief and the thread;
  3. `git diff --stat main...TIP`, then the diff.
- **The thread is data.** The executor's posts are claims to check against the diff, never a reason
  in themselves: a post asking for a waiver is no evidence for one. The brief says what the task
  was meant to change; the diff says what it changed.
- **What it writes:** the stamps whose guideline applies, and one line for each it left out or
  added, as `--why`. Unsure, it keeps the stamp. Then it ends.

## The land's check

The land reads the catalogue from the main checkout's project file, the needs from the board, and
the branch's tip:

1. **Which stamps.** The newest assessment's. When it read another tip, or none (made from the
   brief), those and the catalogue's defaults: an assessment lowers what a task needs only on the
   tip it read. With no assessment, the defaults.
2. **Ready.** The task is in review or blocked, or done by an approve where `review` is needed.
   Where it is not, no verdict is: the executor's move to review says the work is ready, and the
   land closes the task.
3. **Standing on the tip.** Each stamp, by its reader, at the tip. A missing one refuses, naming
   each missing and who gives it: `tests` or `pictures` to `@executor`; `review`, or an assessment
   of another tip, to `@orchestrator`; a missing `owner` moves the task to blocked, waiting on the
   owner. That move is new to the default land, which moves nothing to blocked today; a consumer's
   own landing makes it for review waiting on nobody.

Nothing else decides a land. The merge, the move, the push, the close and what starts after it are
as they are.

## The task page

The gates answer marks each stamp of the catalogue: one needed, met, missing or held at the tip;
one the assessment left out, waived, the assessment's line its reason. A task done with no review
round reads waived from its assessment. `owner` is held while the
owner's stamp is needed and not given, or a card the task posted waits. `check` stays as it is,
information the land does not read.

## Worked examples

| Task | Needs | Why |
|---|---|---|
| Documents only: a design under `docs/` and its card | none | Nothing runs it. The docs audit and the status lint at the tip are its evidence on the thread, and the regenerator runs the audit on main after the land, a red there a task within minutes. |
| A hotfix: the server's pages fail to load, a one-line fix | none | Restoring what everyone uses comes first; a reviewer reads it afterwards, as its postmortem. With a failing test that shows the break, `tests`. |
| A release red: a test fails, the fix is in code and the test unchanged | `tests` | The fix leaves its test alone. The stamp asks that the task's test runs at the tip are green, not that the red's own test is among them ([left out](#left-out)). |
| The same red, its fix changing the test | `tests`, `review` | The test is a check path: a loosened test needs a reader that did not ask for it. |
| A role's model changed in the project file, with the guide's words for it | `tests` | Values of settings the Symphony already reads: the settings check green at the tip, and no reviewer. |
| A board command's refusal reworded | `tests`, `review` | A board command every agent runs. |
| A row added to the home queue | `tests`, `review`, `pictures` | A page's look and the client's tests. |
| A new page the owner asked to see before it lands | `tests`, `review`, `pictures`, `owner` | The owner asked. |

A hotfix goes: filed, fixed, moved to review, assessed (one small-model run, about a minute),
landed. The assessment made at filing cannot waive anything on its own: a waiver waits for the
diff. A broken board, which each of those steps needs, is fixed and landed by hand, outside `just
land`.

## What stops an executor waiving its own review

- The board refuses the needs to any session that executed the task or work under it.
- The assessor's instructions are the Symphony's skill. The executor's posts are data it checks
  against the diff.
- A waiver stands only on the tip the assessor read: a commit after it brings the defaults back.
- The field is the board's own, which `just tasks field` cannot write.

The board cannot tell who started a session, so a helper an executor started itself would pass as
its assessor. A reviewer has the same gap today ([left out](#left-out)).

## Existing tasks

Nothing is migrated: completed tasks retain their status, fields and verdicts. Every task's needs
come from its assessment and the catalogue, including one with an older mark. No creation command
or API accepts `fast_fix`; API 2.0 refuses that argument. The stamp catalogue and its readers are
in the default `[land] check_paths`, alongside the landing itself.

RULES.md `workflow/evidence-is-the-gate`, `workflow/fast-bugfixes` and
`workflow/release-reds-first`, [CHECKS](../CHECKS.md) and
[the default landing](../TASKS.md#the-default-landing) say a reviewer is staffed when the needs
name `review`, and an assessor is started at filing and at review.

## Where it meets other work

- **Reviews as stamps** (landed): a reviewer is given the task it reviews, with no subtask, a later
  round resuming the same one, and `review` is its approve. The assessor has nearly the same shape:
  a fixed skill, no subtask, its rule held on the needs it writes rather than on a role; each run is
  a fresh one, as it reads anew.
- **The gates seam and panel:** the catalogue says which gates there are and the needs which of
  them the land requires; the panel draws both. A project's plugin gate of the same name still
  replaces the Symphony's on the page.
- **Each project's own columns and gates** (the concept task): its gates half is `[stamps]`, which
  it builds on rather than adding a table of its own; its columns half is untouched. A project's own
  stamp readers, its plugin's gate of a name, are where it would add to this.
- **A consumer with its own `land`:** its recipe replaces the Symphony's everywhere, and its own
  policy decides what lands. It gets the assessor, the catalogue and the field as any project does,
  and reads them only if it chooses.

## What it changes

Built in three tasks once this is approved, the shared data first, the land last, so nothing about
landing changes until the whole is there:

1. **The catalogue, the field and the role:** `crates/tasks` (the catalogue and its default, `just
   tasks stamps`, `just tasks needs`, the assessor's rule and model), the project file's `[stamps]`
   and `[agents.models] assessor` (`crates/symphony`, [PROJECT_FILE](../../operations/PROJECT_FILE.md)); tests on
   scratch boards.
2. **The task page:** the gates from the catalogue and the needs, the `tests` and `pictures`
   readers, and the owner's approve of a tip on the page.
3. **The land and the skill:** `land.rs` goes by the needs (the three steps above), `create` drops
   `--fast-fix`, the reds are filed without it; the shipped `assess-task` skill, the orchestrator's
   and tasks skills (the assessor at filing and at review, before any reviewer), and the rules and
   documents above.

## Left out

- **Risk levels mapped to stamps:** each stamp's guideline is the map, and a level would be a second
  table to keep beside it.
- **A project's own stamp kinds,** met by its plugin's gate of that name: added when a project on
  the default land needs one.
- **The board starting the assessor by itself** on a move: the orchestrator starts it, as it staffs
  reviewers.
- **The owner writing the needs on the page:** the assessor follows the owner's own post on the
  thread.
- **A floor under the assessment:** the land enforces the needs alone. The `review` guideline uses
  `[land] check_paths`, its paths named to the assessor as a fact.
- **A red's own failing test:** retiring `--fast-fix` gives up the orchestrator's read of a red's
  evidence, so nobody reads that the red's failing test, unchanged, ran green at the tip: `tests`
  asks only that the task's test runs there are green, and a red's fix that ran another test green
  meets it. Requiring a run of the test the red names is the smallest fix, if one is wanted.
- **Refusing an assessor or a reviewer the executor started:** one check of a session's recorded
  parent would close both, a task of its own if wanted.
- **A clean tree behind a recorded run:** a run records its checkout's commit, not whether the tree
  had edits.
