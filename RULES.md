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
tests closest to what it changed and `just lint-rs` (and the client built for a client change,
its distribution never committed: the regenerator a land starts makes main's), and posts the
exact commands and results with the evidence its kind of change asks for,
such as before and after screenshots of a changed page. Its reviewer runs the same tests, never
the suite, judges whether the evidence is good enough for the change's risk, and its approve is
the task's stamp (the task `tools/review-as-stamps` records it on the task). A slice's evidence is
judged in its parent's review, and a fix whose recorded risk calls for no reviewer
(`workflow/fast-bugfixes`) is landed on its posted evidence by its orchestrator, with no stamp.
Higher risk asks for stronger evidence, never for the whole suite. No task runs `just verify`,
whatever a brief asks: it is the release run's (`just release`, [RELEASES](docs/RELEASES.md)), on
main's tip, and a red release run is filed as a task naming the commits since the last green one.
A run starts once `[releases] every_landings` tasks have landed (the cadence, started by the land
itself), and each day once the owner installs its schedule, so a break no task's focused tests
reach is found within that many landings. Evidence by kind of change, how to pick the tests and
worked examples: [CHECKS](docs/CHECKS.md).

### `workflow/fast-bugfixes`

A bug fix is recovery, not a feature cycle: one agent, the smallest regression checks that cover
the repair, no review or check subtasks, and a reviewer only when the fix's recorded risk calls
for one. Restore what is broken before unrelated failures; record what still fails as one
follow-up task, never claiming green or loosening a test. After recovery another agent makes a
bounded postmortem (root cause, the class of recurrence, a shared API or refactor that would
prevent it), reusing an existing analysis task; it blocks nothing.

### `workflow/release-reds-first`

A red the release run files blocks every release, so it is fixed first, the way a bug fix is. The
owner, 2026-11-12: "also nightly fixes should be prioritized, like bugfixes are since they block
releases. Meaning express path line followed up by another run". A candidate red
(`tests/release-red-*`) is filed at p0 and marked red, so it takes no orchestrator slot,
`just tasks next` offers it before other work of its priority, and the PULL digest lists it among
ready work at once, by priority and slug, and first of everything (`red_unstaffed`) once nobody
executes it 10 minutes after it was filed. An orchestrator staffs it before anything else, and one
agent fixes it with the smallest checks that cover the repair, starting from the failing tests or
the failing step rerun alone as its text names them. A red is fixed in the code, and a test red on
load is discarded instead (`tests/no-test-fails-on-load`). A test or check is never loosened to
turn it green (an assertion widened, a timeout or a budget raised, a test ignored or moved out of
the release run): a fix that must change one says on its thread why the code was not the thing to
fix, and a fresh reviewer judges that. It is filed as a fast fix
(`workflow/fast-bugfixes`) under a bound the land enforces: its orchestrator's approve lands a fix
that changes none of the project's tests and checks (`[land] check_paths`,
[TASKS](docs/TASKS.md#the-default-landing)), the failing tests unchanged and green at its tip, and
a fix that changes a test, a check, a budget or a run step lands only on a fresh reviewer's
approve. A red run holds its commit as the release candidate, so the release is that commit and
its reds' fixes alone however fast main moves ([RELEASES](docs/RELEASES.md#the-release-candidate)):
its red's fix starts at the local branch `release-candidate`, never at main, merges that branch
again before review and never merges main before its approve, and commits the client its own
sources make when it changes what the client is built from; its task's text says so. Its own land
moves the candidate, and the land that answers the last red starts the candidate's rerun. A red
whose fix landed with another red's task is closed as covered by that landing, which answers it
once that land advanced the candidate; closing one as covered by any other landing drops the
candidate, its fix being on main and not in it. A red fixed outside this repository is answered by
cancelling its task, saying where.

### `workflow/shared-data-lands-first`

Data or an interface other tasks read (a wire type, a schema, a document's decided content)
reaches main in a small task of its own before the work that uses it; what it breaks on main
becomes a named follow-up task, not a fix inside it. Every branch builds on main alone, but a
release candidate's red fix, which builds on `release-candidate` (`workflow/release-reds-first`):
never read or copy files from another branch or worktree (`just tasks create` refuses a brief
naming the worktree of a `--depends` task not yet done); wait, or ask on that task's thread. A
chain's next slice may start on the previous slice's approved tip by merging that branch.

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

Executors and reviewers are Claude on the model `just tasks limits` names for their role, from
`[agents.models]`, Opus until it names another ([AGENTS.md](AGENTS.md#this-repository-as-its-own-project)),
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

### `tests/no-test-fails-on-load`

A test passes or fails on what the code did, never on how long a loaded machine took to let it.
The owner, 2026-11-12: "A test that fail when machine is under load should be rewritten to account
for cpu wall clock time usage - not total time spent or be removed from nightly". A bound on a cost
is a bound on the CPU the work took, or on a count of the work. A wait is a wait on its condition:
its only time is a wall-clock backstop that only a stall reaches, and its failure says it was a
stall and gives the load average. Elapsed time stays only where the product's promise is elapsed
time, with its clock injected where it can be. A test that can be none of these is proposed on
its task's thread, with the promise it guards and why no CPU bound, count, wait or injected clock
can stand in for it; the orchestrator puts it to the owner, and it leaves `just verify` only on the
owner's word. Until then it stays and runs. A test the owner lets go on such a proposal moves to a
recipe a quiet machine runs, which still runs it, and none leaves before that recipe exists;
docs/CHECKS.md lists each with its promise and the owner's word. No budget or tolerance is widened
to pass. The workspace's one helper is `crates/test-time` (`cpu`, `wait::until`); how to use it:
[CHECKS](docs/CHECKS.md#time-in-tests).

A test that fails a release run or its rerun because of load is discarded. The owner, 2026-11-13:
"A rerun should not fail or pass based on machine load. If it does, discard the tests and move on".
It is load when the test passes alone, or when its failure is a wall-clock bound, a sleep-ordered
step or a timeout under load. The red's own fix removes it from the tree, with one line on the
red's thread naming the test, the evidence it is load (its runs alone and the failing line) and
what it proved. The owner's word is the approval: no card is put to them to discard such a test.
If what it proved matters, a follow-up task may bring it back written on a condition, a count, CPU
time or an injected clock, but the release does not wait for that. No rerun is spent on it, no
rewrite is waited for, and nobody lowers the machine's load to help a run pass. A test that fails
alone on what its code did, or fails the same way without load, is not this case and is fixed in
the code.

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
standing (the others measured wrong, or against something the owner said). A card the owner
approved is such words for a later card only when the choice it approved already answers what the
later card asks, and each of the later card's other options would change that choice: the basis
quotes the verdict and names its card (`review/decided-is-not-asked`). It stays the
owner's when it is a matter of taste or look (names, layout, pictures), a product direction with
live alternatives, money, or an act that cannot be undone (moving live state, deleting, a tag or
a release): those are recommended, never auto-approved. An auto-approved card stays in the
register for the owner to spot-check, and any verdict of theirs overrides it.

### `review/decided-is-not-asked`

An approved card is never reopened by a question on it, and nothing an earlier verdict decides is
put to the owner. The owner, 2026-11-12, approving cards a design had added after they approved
its first: "A. this should had been auto approve/triaged, its basically what was decided in the
other card." and "A. should also had been auto approved."; then of the first card, which an
agent's question on it had put back in front of them: "this should had been a seaparate card no
the same one. I also don't really get why those similar questions about things that were already
been approved are being asked. They are consequences of the previous decision and should be
auto-approved". A consequence of an approved choice is whatever follows from it while the option
chosen stays the one the card described, a cost or a trade the card did not state included: it is
a plain agent line on that card, quoting the owner's words it rests on and answering any owner note
still open there, and the card stays resolved, where the owner can reopen it with a comment. A
material change is only one that would make the option chosen another than the one described: it
is a new card that links the approved one. A question the approved choice does not answer is a new
card too, checked first as every card is. Before any card goes to the owner its agent checks the
owner's earlier verdicts. A card the owner approved decides a later card only when the choice it
approved already answers what the later card asks, and each of the later card's other options
would change that choice. A recommendation that leaves alone something the approved card never
addressed decides nothing, and that card is asked. An auto-approved card, or a line an agent
recorded as answered, is not the owner's approval and decides no further card. A decided card is
recorded as answered on the card the owner approved, never asked; one already posted is
auto-approved (`review/auto-approve-what-is-decided`). A design review that asks for a new owner
card says which earlier verdict it checked and why that does not decide it. Only the owner's own
words decide; a question of theirs decides nothing
([REVIEW](docs/REVIEW.md#before-a-card-goes-to-the-owner)).

### `docs/cards-clear-and-concise`

A card holds what its judgement call needs and nothing more: short paragraphs, plain words, one
question. It opens with what changes for the owner, in one or two plain sentences, before any
option, written for someone who has not read the internals. The owner, 2026-11-12, on a card that
did not: "I don't fully understand this issue. So pick whatever choice makes sense to you." Detail
the decision does not need goes in a linked file, and pictures go where the thing judged is visual
([REVIEW](docs/REVIEW.md#writing-a-card)).

### `docs/one-decision-per-card`

A card asks one decision. A detail that carries choices of its own, its own A, B or C, is a card
of its own that links the first, so no verdict has to decide several things at once
([REVIEW](docs/REVIEW.md#writing-a-card)).

## Documents

### `history-is-not-reference`

Docs, guides, skills and code comments say how things are now, never what the code used to do,
and a reviewer refuses a line that does. Evidence and superseded numbers go to `docs/history/`,
which the Symphony passes over when it reads documents; a rejected approach keeps one line and a
pointer.

### `docs/no-ledger-in-documents`

A document under `docs/` describes intent, behaviour and options. The owner's exact words and the
dated history of who decided what, and when, live in the decision register (`docs/DECISIONS.md`,
each card's dated lines) and on task threads; a document whose reason matters links its card,
`[its card](DECISIONS.md#SLUG)` ([REVIEW](docs/REVIEW.md#slugs-in-prose)). The register and
`docs/history/` are the records that keep them.

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

The repository tracks no Python file, and nothing it ships, builds, tests or checks runs Python:
its simple operations are `just` recipes and shell pipelines (`curl` with `jq` for JSON), and
anything more is Rust. An agent's throwaway code may be Python: a scratch script, a one-off edit or
count, never committed and never a step of a recipe, a test or a check. The owner, 2026-11-13:
"By the way, usage of python is acceptable for throw away code."

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
