# Rules

Standing rules for every agent working in this repository, beyond [AGENTS.md](AGENTS.md) and
the package guides: each is the rule, and the document it names holds how and why. A consuming
project's landing, stamps, certificates and gates are its own
([command ownership](AGENTS.md#command-ownership)); this repository's gate is
`workflow/evidence-is-the-gate`.

## How work is done

### `workflow/parent-feature-delivery`

A top-level task owns one feature branch and one delivery cycle; its
[subtasks](docs/workflow/TASKS.md#subtasks) are slices of it, carried by one agent unless another, on
disjoint files, demonstrably shortens it. A slice commits into the feature branch, runs only its
focused checks and closes with `just tasks done SLUG "integration commit and focused result"`;
only the parent goes to review, once. This holds over a slice brief that asks for more.

### `workflow/evidence-is-the-gate`

A task's gate is evidence that its change works, not the suite. Every task, parent or slice,
feature or fix, runs the tests closest to what it changed and `just lint-rs` (the client built
for a client change, its distribution never committed) and posts the exact commands and results
with what its kind of change asks for, such as before and after screenshots of a changed page
([CHECKS](docs/workflow/CHECKS.md#the-gate-is-evidence)). What it needs to land is its assessment's: an
assessor that neither filed, orchestrates nor executed it writes the stamps it needs
(`just tasks stamps`, [TASKS](docs/workflow/TASKS.md#needed-stamps)) when it is filed and each time it
moves to review, before any reviewer. Higher risk asks for stronger evidence, never for the
suite: no task runs `just verify`, whatever a brief asks. It is the release run's
([RELEASES](docs/workflow/RELEASES.md#the-release-run)).

### `workflow/never-chase-main`

A task merges main into its branch once, when its work is complete and it is about to ask for
review, and runs its closest tests again at that tip. From then on it never merges main because
main moved, and a reviewer never sends a branch back only for that: it merges again only when
its land reports a conflict, or a reviewer names what main gained that breaks the change or that
the change breaks ([CHECKS](docs/workflow/CHECKS.md#after-main-moves)). A figure a guide quotes is
measured once, at the review tip. A release candidate's fix merges `release-candidate` in main's
place (`workflow/release-reds-first`).

### `workflow/no-second-build-in-review`

What a task built and ran while it was made is not built or run again to review it: its recorded
runs at its tip are its evidence, and a reviewer takes a green one as run. It builds or runs only
what the record does not cover and reading cannot settle (a doubt it names, with the smallest
target that answers it; a mutation; a run the executor left out), and asks in a changes for a run
the evidence claims and no record shows ([CHECKS](docs/workflow/CHECKS.md#no-second-build-in-review)).
What changes do to each other on main is the release run's to find, never a review's.

### `workflow/fast-bugfixes`

A bug fix is recovery, not a feature cycle: one agent, the smallest regression checks that cover
the repair, no review or check subtasks, and a reviewer only when its assessment needs one.
Restore what is broken before unrelated failures, and record what still fails as one follow-up
task, never claiming green or loosening a test. After recovery another agent makes a bounded
postmortem (the root cause, how it would recur, what would prevent it); it blocks nothing.

### `workflow/simplest-thing-that-solves-it`

The smallest thing that solves the problem in front of us. A problem met once is solved once: an
agent runs the commands or a throwaway script, says on the thread what it ran and what came of
it, and stops, with no committed tool, tests, review round or task of its own. Code is committed
only for what will run again, and guarded only against what has happened or plainly will; a
script that has done its one job is deleted, never hardened. Whoever files, starts or reviews
work refuses the larger form.

### `workflow/question-the-task-first`

Before any code, the executor that claims a task reads its brief against the tree and the board
as they are now and answers on the thread: is it still needed, and is the brief's way the
simplest that solves it. A no to either goes to the orchestrator with the smaller alternative,
which may be cancelling it, and the brief is not built as written meanwhile.

### `workflow/release-reds-first`

A red the release run files (`tests/release-red-*`, p0) blocks every release: an orchestrator
staffs it before anything else, and one agent fixes it as a bug fix, starting from the failing
tests or step its text names, rerun alone. It is fixed in the code. A test or check is never
loosened to turn it green (an assertion widened, a timeout or a budget raised, a test ignored or
moved out of the release run): a fix that must change one says on its thread why the code was
not the thing to fix, and a fresh reviewer judges that. A test red on load is discarded in the
fix instead (`tests/no-test-fails-on-load`). A budget or a cap, a red's or any task's, gives way
only by its orchestrator's decision recorded on the task (`just tasks gate`,
[TASKS](docs/workflow/TASKS.md#gate-decisions)), once the cut came first. While a run holds a release
candidate, a red's fix starts at the local branch `release-candidate`, never at main, merges
that branch and never main before review (once the candidate advanced to a delivered head it
is rebased onto it instead, never merging it), and commits the client its own sources make
when it changes what the client is built from. Its task's text says so, and the rest (a red
covered by another landing or fixed outside this repository) is
[RELEASES](docs/workflow/RELEASES.md#the-release-candidate).

### `workflow/shared-data-lands-first`

Data or an interface other tasks read (a wire type, a schema, a document's decided content)
reaches main in a small task of its own before the work that uses it; what it breaks on main is
a named follow-up task, not a fix inside it. Every branch builds on main alone, but a release
candidate's red fix: never read or copy files from another branch or worktree; wait, or ask on
that task's thread. A chain's next slice may start on the previous slice's approved tip by
merging that branch.

### `workflow/blocked-work-followup`

Work that cannot go on is `blocked`, and before going idle its executor posts a `BLOCKED` handoff
to `@orchestrator`: the exact restriction, the next action, who can lift it, and what independent
work continues. The orchestrator records the blocker's owner, the resume condition and a dated
follow-up there, does it, and checks its `blocked` and `in_progress` parents for a real next
action at every slice completion, handoff and PULL. It adds no holds to work the owner already
authorized. A refused permission, a board's refusal of a command included, is reported, never
routed around: never build or run another binary to get past it.

### `workflow/no-slowing-for-usage-limits`

No agent slows down for a usage, credit or rate limit of the account it runs on, seen or forecast:
it never holds back, defers or narrows filing, staffing, reviews, follow-ups or the number of
tasks in progress for one. Work goes on at the pace the board and `[limits]` set until a limit
actually stops a session, which is resumed or its task handed on as any stopped session is
(`STALE` and `LOST` in [the tasks skill](.agents/skills/tasks/SKILL.md#events)). Switching to usage
credits to go on is the owner's choice, never an agent's.

### `workflow/claude-first-agents`

Executors and reviewers are Claude on the model `just tasks limits` names for their role
([AGENTS.md](AGENTS.md#this-repository-as-its-own-project)), started as an in-process subagent
(an Agent launch with `isolation: worktree`, its branch named for the task's slug) or a headless
`claude -p`; a terminal of its own (`just tasks launch SLUG`) only when the work needs one.

### `reference-by-slug`

Name work by its slug, `area/slug` (`tools/memory-guard-hook`), the one its branch and commits
use; never a code like `M17`.

## Code

### `code/consumer-api-two-releases`

A public Rust item a consumer calls keeps working in the release that brings its replacement,
without `#[deprecated]`: a consumer's lint denies warnings. A separate task removes it only in a
later release, with read-only evidence that consumers have migrated, updating the consumer-API
compile guards with it. The procedure is [CHECKS](docs/workflow/CHECKS.md#consumer-api-changes), from
[its card](docs/workflow/analysis/release-candidate.md#consumer-api-break-release).

### `code/no-timed-waits`

No code sleeps or waits by a number: a wait is for its event (a channel's message, a process's
exit, a socket's answer, a page's event, the reader's press). A number drives only a debounce, a
throttle, a retry with backoff, a deadline that bounds a wait for an event, a clock's tick, or a
check for an event its watcher can drop, each through a named helper: in Rust
`test_time::wait::until` in a test and `apps/symphony-server/src/backstop.rs` in the server,
with the others `git grep 'code/no-timed-waits'` lists; in the client `src/timing.ts` and, in
its unit tests, `src/testing.ts`. A deadline on the wait
itself (`recv_timeout`, `tokio::time::timeout`) needs no helper, unless nothing ends the wait
but a stop: that is a sleep the lint cannot see, and a reviewer refuses it.

`just lint-rs` refuses a new timer by this rule's name (`clippy.toml`,
`apps/symphony-web/biome.json`). A site kept is the baseline, marked
`no-timed-waits baseline: WHY` in its linter's own suppression, WHY the event it should wait
for. The baseline only shrinks: a new suppression is a new timer, which a reviewer refuses.
Shell scripts are read in review alone.

## Tests

### `workflow/tests-after`

Changed behaviour gets tests, written with or after the code and run after it, and a branch
shows them and their green run. No test-first order, no red run before the code and no planted
fault or reverted change to watch a test fail, whatever a brief asks; a reviewer may still probe
the code with a case it suspects the code gets wrong.

### `tests/rerun-the-failure-alone`

A test that failed is run again by itself before anything is concluded from it or changed for it;
the whole suite is never run again for it. One that passes alone but failed in the run is
reported as load- or order-dependent, with both results.

### `tests/a-red-on-main-has-one-owner`

A test that fails on main, with the task's change taken out, is not the task's to fix or to wait
for. The agent that finds it proves it on main (the test alone, at main's tip), tells its
orchestrator on its thread with the command and output, and carries on: its own review and
landing name the red and do not wait on it. The orchestrator files a p0 task for it unless one
holds that red already, and staffs it at once; a second finder adds its evidence there.

### `tests/no-test-fails-on-load`

A test passes or fails on what the code did, never on how long a loaded machine took to let it. A
bound on a cost is a bound on the CPU the work took or on a count of the work. A wait is a wait
on its condition, its only time a wall-clock backstop a stall alone reaches. Elapsed time stays
only where the product's promise is elapsed time, its clock injected where it can be. The helper
is `crates/test-time` ([CHECKS](docs/workflow/CHECKS.md#time-in-tests)). No budget, allowance or
tolerance is widened to pass, and nobody lowers the machine's load, or holds other work back, to
help a run pass.

A test that can be none of these is proposed on its task's thread, with the promise it guards;
it leaves `just verify` only on the owner's word, and runs until then. A red test that fails on
load (a wall-clock bound, a sleep-ordered step, a timeout under load) is discarded with no card
to the owner: its red's own fix removes it, with one line on the red's thread naming the test,
the evidence it is load and what it proved. A test that fails alone on what its code did is
fixed in the code.

### `a-test-that-passes-either-way-is-not-a-test`

A test that would pass with the change reverted, judged by reading it, is replaced or deleted. A
check that missed its fault or hung instead of reporting is repaired, with a test of the case it
missed and every case of that shape, never narrowed. Quote a measurement only after showing its
meter moves, more than once, with no other agent loading the machine.

## Review and the owner

### `workflow/reviewers-not-the-owner`

Review is the verdict (`just tasks verdict SLUG approve|changes`) of a fresh reviewer that did
not write, execute or request the work. Once it approves, nothing is queued for the owner.

### `docs/no-self-approval`

Only the owner approves a design document or a card; an agent never marks its own output
approved, and answers a callout with a nested `[!resolved]` note under it
([REVIEW](docs/workflow/REVIEW.md#commenting-in-place)).

### `workflow/approval-goes-with-the-ask`

What the owner approves, in the terminal or in the Symphony, is approved: no new card, no
command for the owner to run, no waiting; an agent records it with the owner's words, and a brief
quotes the ask it serves. It covers what the ask obviously needs (a dependency, a crate, a tool);
a choice a benchmark can settle is tried, taken and carded after the fact, and a check's
threshold is never an owner card. It never covers a result the owner has not seen, such as a
document's content or how a page looks.

### `workflow/owner-questions-visible`

A question only the owner can answer is written where it lasts before an agent waits on it: on
its task (`just tasks say SLUG "…" --to owner`) or as a card question
(`just symphony-cli reply PATH SLUG "…" --question`). A terminal or chat question is never the
only copy; a follow-up reuses it, an answer given in the terminal is recorded there, and silence
is not an answer.

### `review/auto-approve-what-is-decided`

A card whose answer is no real choice is not put to the owner, and one already posted is marked
`[a]` with a dated `agent · auto-approved — <letter>: <basis>` line: where the owner's own words,
quoted in the basis, already decide it (`review/decided-is-not-asked`), or its own evidence
leaves one option standing. Taste or look, a product direction with live alternatives, money and
an act that cannot be undone stay the owner's: recommended, never auto-approved. An
auto-approved card stays in the register, and any verdict of the owner's overrides it
([REVIEW](docs/workflow/REVIEW.md)).

### `review/decided-is-not-asked`

An approved card is never reopened by a question on it, and nothing an earlier verdict decides is
put to the owner. What follows from an approved choice while the option chosen stays the one the
card described, a cost or a trade it did not state included, is a plain agent line on that card
quoting the owner's words; only a change that would make the option another is a new card, which
links the approved one. Before any card goes to the owner its agent checks the owner's earlier
verdicts: an approved card decides a later one only when the choice it approved already answers
what the later card asks and each of the later card's other options would change that choice.
Only the owner's own words decide, never an auto-approved card, an agent's recorded answer or a
question of the owner's ([REVIEW](docs/workflow/REVIEW.md#before-a-card-goes-to-the-owner)).

### `docs/cards-clear-and-concise`

A card holds what its judgement call needs and nothing more: short paragraphs, plain words, one
question. It opens with what changes for the owner, in one or two plain sentences, before any
option, for someone who has not read the internals. Detail goes in a linked file, and pictures
where the thing judged is visual ([REVIEW](docs/workflow/REVIEW.md#writing-a-card)).

### `docs/one-decision-per-card`

A card asks one decision. A detail that carries choices of its own is a card of its own that
links the first.

## Documents

### `history-is-not-reference`

Docs, guides, skills and code comments say how things are now, never what the code used to do,
and a reviewer refuses a line that does. Evidence and superseded numbers go to `docs/history/`;
a rejected approach keeps one line and a pointer.

### `docs/no-ledger-in-documents`

A document describes intent, behaviour and options, and a guide states each rule and its reason
in plain words. The owner's exact words and the dated history of who decided what live on the
cards, each card's dated lines, and on task threads. A card sits at the place in its document
where the thing it decides is said, never in a section at the document's end, and in the
decision register (`docs/DECISIONS.md`) only when no document says it
([REVIEW](docs/workflow/REVIEW.md#where-a-card-is-written)). The prose around a card says how things are,
and where the reason matters it links the card ([REVIEW](docs/workflow/REVIEW.md#slugs-in-prose)).
`crates/symphony/tests/hooks/owner_words.rs` fails on the shapes it knows outside a card's own
lines, and a reviewer reads for the others.

### `docs/one-topic-per-doc`

One topic per document, linked to the others, never merged into one file.

## Tools and the machine

### `commits/no-ai-authorship`

No commit, merge, tag, pull request, file header, licence line or anything else names Claude,
Claude Code, Codex or any other AI agent as author, co-author or generator: no `Co-Authored-By`
trailer, no "Generated with" line. The author and owner of this repository is Stefano Benatti,
and its licence is MIT (`LICENSE`). An agent's handle belongs on the board thread, never in the
commit.

### `tools/no-new-python`

The repository tracks no Python file, and nothing it ships, builds, tests or checks runs Python:
simple operations are `just` recipes and shell pipelines (`curl` with `jq`), and anything more is
Rust. An agent's throwaway script may be Python, never committed.

### `memory-lives-in-the-repo`

Nothing goes in an agent's own memory folder (`~/.claude/projects/…/memory/`,
`~/.codex/memories/`): what must outlast a session is a rule here, guidance in an AGENTS.md, a
skill, a doc or a board task, committed where every agent sees it. In a Claude Code session the
memory guard refuses a write there, and it reads a shell command as text, so one that names the
folder beside a write (a commit message, a board post) is refused too: say "the memory folder",
or put the text in a file. What it reads and misses is its module doc
(`crates/symphony/src/hook/memory_guard.rs`); nothing guards a Codex session.

### `tools/parallel-by-default`

Anything an agent runs works beside the same thing in another worktree: no fixed path two runs
both write, no fixed port or name, no lock (`just symphony --port 0`, a scratch board, a
temporary machine folder). State shared by design, such as the board in the git common dir, is
correct as it is, and only a truly single resource is serialised, under a lock named for it.
The git stash is shared too: commit to your branch instead.

### `tools/the-machine-is-shared`

Many agents share this machine and its disk. A long command is started in the background and
waited for, never started again while it still runs. No agent copies a build folder, and an
agent removes the `build/` output of its worktree when its work there is done.

### `tools/no-sweep-on-live-state`

A sweep or a migration an agent runs, to try a change or to see what it does, runs against a
scratch repository and a scratch board, never the real worktrees or the live board.

### `tools/a-consumers-checkout-is-read-only`

A consuming project's checkout on this machine is read-only to an agent working here: nothing in
it is changed but what a release's refresh moves there, an act the owner's word names, such as a
pin's commit made once by hand ([its card](docs/workflow/RELEASES.md#consumer-manifest-line)),
and the filing, staffing and work of a task on that project's own board
([the orchestrator's page](.agents/skills/task-orchestrator/project.md)).

### `commits/only-the-land-pushes`

Nothing is pushed but a task's branch, by the dispatch of its land, for the pull request that
delivers it, and a release's tag, by the release run: nothing else of a task. Main is pushed by
nobody, the owner included, while this repository sets `[land] delivery`: it moves only by
GitHub's squash of a task's pull request, and its ruleset refuses any other update
([TASKS](docs/workflow/TASKS.md#a-land-delivered-by-a-pull-request)). A consuming project's branch is
pushed only on the owner's word naming that push.

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
