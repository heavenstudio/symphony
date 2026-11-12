# Rules

Standing rules for every agent working in this repository, beyond [AGENTS.md](AGENTS.md) and
the package guides. A consuming project's landing, stamps, certificates and gates are its own
([command ownership](AGENTS.md#command-ownership)), under its own rules; this repository's gate is
`workflow/evidence-is-the-gate`.

## How work is done

### `workflow/parent-feature-delivery`

A top-level task owns one feature branch and one delivery cycle; its
[subtasks](docs/TASKS.md#subtasks) are small slices of it, not releases. One agent carries the
feature unless another, on disjoint files, demonstrably shortens it. A slice commits into the
feature branch, runs only its focused checks and closes with
`just tasks done SLUG "integration commit and focused result"`; only the parent goes to review,
once, with its evidence (`workflow/evidence-is-the-gate`), keeping its review fixes. This holds
over a slice brief that asks for more.

### `workflow/evidence-is-the-gate`

A task's gate is evidence that its change works, not the suite. The owner, 2026-11-11: "Do they
need to run a full test suite? Why? If tests are so time consuming why are they being required to
run every time, instead of running tests more closely associated with the tasks at hand, leaving
a nighly run for a full verify?" and "So the executors need to 'prove that the work they did
work'. That is the concept of the stamp based gating. As long as evidence is good enough, a full
verification can be left to be done later". Every task, parent or slice, feature or fix, runs the
tests closest to what it changed and `just lint-rs` (and `just client-update` for a client
change), and posts the exact commands and results with the evidence its kind of change asks for,
such as before and after screenshots of a changed page. Its reviewer runs the same tests, never
the suite, judges whether the evidence is good enough for the change's risk, and its approve is
the task's stamp (the task `tools/review-as-stamps` records it on the task). A slice's evidence is
judged in its parent's review, and a fix whose recorded risk calls for no reviewer
(`workflow/fast-bugfixes`) is landed on its posted evidence by its orchestrator, with no stamp.
Higher risk asks for stronger evidence, never for the whole suite. No task runs `just verify`,
whatever a brief asks: it is the nightly's (`just nightly`, [RELEASES](docs/RELEASES.md)), on
main's tip, and a red night is filed as a task naming the commits since the last green one. A
night runs once `[releases] every_landings` tasks have landed (the cadence, started by the land
itself), and each night once the owner installs its schedule, so a break no task's focused tests
reach is found within that many landings. Evidence by kind of change, how to pick the tests and
worked examples: [CHECKS](docs/CHECKS.md).

### `workflow/fast-bugfixes`

A bug fix is recovery, not a feature cycle: one agent, the smallest regression checks that cover
the repair, no review or check subtasks, and a reviewer only when the fix's recorded risk calls
for one. Restore what is broken before unrelated failures; record what still fails as one
follow-up task, never claiming green or loosening a test. After recovery another agent makes a
bounded postmortem (root cause, the class of recurrence, a shared API or refactor that would
prevent it), reusing an existing analysis task; it blocks nothing.

### `workflow/shared-data-lands-first`

Data or an interface other tasks read (a wire type, a schema, a document's decided content)
reaches main in a small task of its own before the work that uses it; what it breaks on main
becomes a named follow-up task, not a fix inside it. Every branch builds on main alone: never
read or copy files from another branch or worktree (`just tasks create` refuses a brief naming
the worktree of a `--depends` task not yet done); wait, or ask on that task's thread. A chain's
next slice may start on the previous slice's approved tip by merging that branch.

### `workflow/blocked-work-followup`

Work that cannot go on is `blocked`, and before going idle its executor posts a `BLOCKED`
handoff to `@orchestrator` on the thread: the exact restriction, the next action, who can lift
it, and what independent work continues. The orchestrator records the blocker's owner, the
resume condition and a dated follow-up there and does that follow-up; at every slice completion,
handoff and PULL it checks its `blocked` and `in_progress` parents for a real next action. An
orchestrator adds no holds to work the owner already authorized, and a refused permission is
reported, never routed around, while independent authorized work goes on. A board that refuses a
command is such a refusal: report it and stop, and never build or run another binary to get
around it.

### `workflow/claude-first-agents`

Executors and reviewers are Claude on Opus ([AGENTS.md](AGENTS.md#this-repository-as-its-own-project)),
started as an in-process subagent (an Agent launch with `isolation: worktree`, its branch named
for the task's slug) or a headless `claude -p`; a terminal of its own (`just tasks launch SLUG`)
only when the work needs one. *A terminal keeps its memory until it is closed.*

### `reference-by-slug`

Name work by its slug, `area/slug` (`tools/memory-guard-hook`), the one its branch and commits
use; never a code like `M17`, which ends up shared by unrelated work.

## Tests

### `workflow/tests-after`

Changed behaviour gets tests, written with or after the code and run after it, and a branch
shows them and their green run. No test-first order, no red run before the code and no planted
fault or reverted change to watch a test fail, whatever a brief asks; a reviewer may still probe
the code with a case it suspects the code gets wrong.

### `tests/rerun-the-failure-alone`

A test that failed is run again by itself before anything is concluded from it or changed for it,
never the whole suite again. The owner, 2026-11-11: "test that fail should be ran themselves to
see if they pass. No need to run entire test suite each time." One that passes alone but failed in
the run is reported as load- or order-dependent, with both results.

### `tests/a-red-on-main-has-one-owner`

A test that fails on main, with the task's change taken out, is not the task's to fix or to wait
for. The owner, 2026-11-11: "tests that are failing in main should be handled by an orchestrator
as a separate task in order for individual tasks not to be blocked by it and have a single
dedicated owner to solve it." The agent that finds it proves it on main (the test alone, at main's
tip), tells its orchestrator on its thread with the command and output, and carries on: its own
review and landing name the red and do not wait on it. The orchestrator checks the board for a
task already holding that red, files one at p0 if there is none, and staffs it at once; a second
finder adds its evidence to that task and files nothing.

### `a-test-that-passes-either-way-is-not-a-test`

A test, check or meter has to be able to tell. A test that would pass with the change reverted,
judged by reading it, is replaced or deleted; a check that missed its fault or hung instead of
reporting is repaired, with a test of the case it missed and every case of that shape, never
narrowed. Quote a measurement only after showing its meter moves, more than once, with no other
agent loading the machine.

## Review and the owner

### `workflow/reviewers-not-the-owner`

Review is the verdict (`just tasks verdict SLUG approve|changes`) of a fresh reviewer that did
not write, execute or request the work. Once it approves, nothing is queued for the owner.

### `docs/no-self-approval`

Only the owner approves a design document or a card; an agent never marks its own output
approved, and answers a callout with a nested `[!resolved]` note under it
([REVIEW](docs/REVIEW.md#commenting-in-place)).

### `workflow/approval-goes-with-the-ask`

What the owner approves, in the terminal or in the Symphony, is approved: no new card, no
command for the owner to run, no waiting; an agent records it with the owner's words, and a brief
quotes the ask it serves. The approval covers what the ask obviously needs (a dependency, a crate,
a tool); a choice a benchmark can settle is tried, taken and carded after the fact so the owner
can switch, and a check's threshold is never an owner card. It never covers a result the owner
has not seen, such as a document's content or how a page looks.

### `workflow/owner-questions-visible`

A question only the owner can answer is written where it lasts before an agent waits on it: on
its task (`just tasks say SLUG "…" --to owner`) or as a card question
(`just symphony-cli reply PATH SLUG "…" --question`). A terminal or chat question is never the
only copy; a follow-up reuses it, an answer given in the terminal is recorded there, and silence
is not an answer.

### `review/auto-approve-what-is-decided`

A card is not put to the owner, or is auto-approved if already posted, when its answer is not a
real choice. The owner, 2026-11-11: "run some auto triage. Some decisions look like they should
had been auto approved". An orchestrator marks it `[a]` with a dated
`agent · auto-approved — <letter>: <basis>` line ([REVIEW](docs/REVIEW.md)) when the owner's own
words, quoted in the basis, already decide it, or when the card's own evidence leaves one option
standing (the others measured wrong, or against something the owner said). It stays the
owner's when it is a matter of taste or look (names, layout, pictures), a product direction with
live alternatives, money, or an act that cannot be undone (moving live state, deleting, a tag or
a release): those are recommended, never auto-approved. An auto-approved card stays in the
register for the owner to spot-check, and any verdict of theirs overrides it.

### `docs/cards-clear-and-concise`

A card holds what its judgement call needs and nothing more: short paragraphs, plain words, one
question. Detail the decision does not need goes in a linked file, and pictures go where the
thing judged is visual ([REVIEW](docs/REVIEW.md#writing-a-card)).

## Documents

### `history-is-not-reference`

Docs, guides, skills and code comments say how things are now, never what the code used to do,
and a reviewer refuses a line that does. Evidence and superseded numbers go to `docs/history/`,
which the Symphony passes over when it reads documents; a rejected approach keeps one line and a
pointer.

### `docs/one-topic-per-doc`

One topic per document, linked to the others, never merged into one file.

## Tools and the machine

### `commits/no-ai-authorship`

No commit, merge, tag, pull request, file header or licence line names Claude, Claude Code, Codex
or any other AI agent as author, co-author or generator: no `Co-Authored-By` trailer, no
"Generated with" line. The owner, 2026-11-11: "no claude code or codex co authorship anywhere".
The author and owner of this repository is Stefano Benatti, and its licence is MIT (`LICENSE`).
An agent's handle belongs on the board thread, never in the commit.

### `tools/no-new-python`

No Python is written or run here: not in the project's code, and not in an agent's own scratch
scripts or one-off edits. The repository tracks no Python file. Simple operations use the `just`
recipes and shell pipelines (`curl` with `jq` for JSON) rather than a language runtime; anything
more is Rust.

### `memory-lives-in-the-repo`

Nothing goes in an agent's own memory folder (`~/.claude/projects/…/memory/`): what must outlast
a session is a rule here, guidance in an AGENTS.md, a skill, a doc or a board task, committed
where every agent sees it. In a Claude Code session the memory guard (`symphony hook memory-guard`,
which `symphony init` installs in `.claude/settings.json`) refuses a write tool's path there, or in
Codex's `~/.codex/memories/`, and a shell command that names either folder and writes. It reads the
command as text: the write shapes it knows are listed in [AGENTS.md](AGENTS.md#agent-integration),
others pass, and so does a write through a link made before. A Codex session runs no Claude hook,
so nothing guards it, its own `~/.codex/memories/` included. `.claude/settings.json` also denies
Edit there.

### `tools/parallel-by-default`

Anything an agent runs works beside the same thing in another worktree: no fixed path two runs
both write, no fixed port or name, no lock (`just symphony --port 0`, a scratch board, a
temporary machine folder). State shared by design, such as the board in the git common dir, is
correct as it is, and only a truly single resource is serialised, under a lock named for it. The
git stash is shared too: commit to your branch instead.

### `delete-by-name`

`rm -f` names every file it removes, never a glob. An agent stops only a run it started, by the
PID it recorded (`cmd > log 2>&1 & PID=$!`, later `kill $PID`) or with TaskStop; never by name
or pattern (`pkill`, `killall`, `kill` fed by `pgrep`, `ps | grep` or `lsof`, a process group,
`kill 0`), which matches other agents' runs too.

### `tools/vendor-every-asset`

Every script, stylesheet and font a Symphony page loads is in this repository and committed;
nothing loads from a CDN or any outside URL.

## Keeping these rules

### `only-codify-what-was-infringed`

A rule or guard is added only for a mistake actually made, where it will keep paying off, and
never for what a check already catches; one that adds little is removed when spotted.
