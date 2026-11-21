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

A task's gate is evidence that its change works, not the suite. The suite takes too long to run
for every task, and the tests most closely tied to a change are the ones that tell whether it
works: its executor proves that the work it did works, and once that evidence is good enough the
full verification is left to a later, scheduled run. Every task, parent or slice, feature or fix,
runs the tests closest to what it changed and `just lint-rs` (and the client built for a client
change, its distribution never committed: the regenerator a land starts makes main's), and posts
the exact commands and results with the evidence its kind of change asks for,
such as before and after screenshots of a changed page. What it needs to land is its
assessment's: an assessor, which neither filed, orchestrates nor executed it, reads its diff when
it is filed and each time it moves to review, before any reviewer, and writes the stamps it needs
from the project's catalogue (`just tasks stamps`: its tests green at the tip, a fresh reviewer's
approve, pictures, the owner's approve; [TASKS](docs/TASKS.md#needed-stamps)), a reviewer's approve
alone with none written. Where it needs review, its reviewer reads the task's recorded runs at
the tip and builds or runs nothing they cover (`workflow/no-second-build-in-review`), judges whether the evidence is good enough for the change's risk, and its approve, a verdict on
the task itself and never a review subtask, is that stamp. The land lands the task once every
stamp it needs stands on its tip, and refuses naming who gives a missing one. A slice's evidence is
judged in its parent's review. Higher risk asks for stronger evidence, never for the whole suite. No task runs `just verify`,
whatever a brief asks: it is the release run's (`just release`, [RELEASES](docs/RELEASES.md)), on
main's tip, and a red release run is filed as a task naming the commits since the last green one.
A run starts once `[releases] every_landings` tasks have landed (the cadence, started by the land
itself), and each day once the owner installs its schedule, so a break no task's focused tests
reach is found within that many landings. Evidence by kind of change, how to pick the tests and
worked examples: [CHECKS](docs/CHECKS.md).

### `workflow/never-chase-main`

A task merges main into its branch once: when its work is complete and it is about to ask for
review, so the review is of the change on main as it is. It runs its closest tests again at that
tip, posts the evidence naming it and moves to review
([after main moves](docs/CHECKS.md#after-main-moves)). From then on it never merges main because
main moved. Main always moves: a task that fetches main, builds and measures finds main moved
again by the time that ends, a loop with no end, and each merge after an approve asks for a fresh
one, but the one merge of main that settles a land's conflict where every conflicted path is a
document (a `*.md` file that is no skill, guide or rules) and changes nothing else: its approve
stands over that merge, and it lands on its tests green at the merged tip
([the default landing](docs/TASKS.md#the-default-landing)). It lands once its stamps stand on
its tip and it merges without conflict. It merges main
again only when its land reports a conflict, or when a reviewer asks for it by naming what main
gained that breaks the change or that the change breaks. A reviewer never sends a branch back
only because main moved. A figure a guide quotes (a page's first-visit bytes, a size) is measured
once, at the review tip, never again for each new main. A release candidate's fix merges
`release-candidate` in main's place (`workflow/release-reds-first`).

### `workflow/no-second-build-in-review`

What a task built and ran while it was made is not built or run again to review it. Its
executor's builds and tests are recorded with the commit they ran at
([MACHINE_WORK](docs/MACHINE_WORK.md#recording-builds-and-tests)), and the recorded runs at its
tip are its evidence: a stamp stands on evidence of work already done, a build, a test run, a
review, so that a task is quick to land. A reviewer takes a green recorded run at the tip as
run. It reads the diff against the record, judges whether those runs cover the change, and
builds or runs only what the record does not cover and reading cannot settle: a doubt it names,
with the smallest target that answers it; a mutation, to see that a test can fail; a run the
executor left out. A run the evidence claims and no record at the tip shows is the executor's
to make, asked for in a changes. What changes do to each other once they are on main is the
release run's to find, on the candidate it holds ([RELEASES](docs/RELEASES.md)), never a
review's.

### `workflow/fast-bugfixes`

A bug fix is recovery, not a feature cycle: one agent, the smallest regression checks that cover
the repair, no review or check subtasks, and a reviewer only when its assessment needs one
(`workflow/evidence-is-the-gate`). Restore what is broken before unrelated failures; record what still fails as one
follow-up task, never claiming green or loosening a test. After recovery another agent makes a
bounded postmortem (root cause, the class of recurrence, a shared API or refactor that would
prevent it), reusing an existing analysis task; it blocks nothing.

### `workflow/simplest-thing-that-solves-it`

Keep it simple: the smallest thing that solves the problem in front of us. A problem met once is
solved once: an agent runs the commands or a throwaway script, says on the thread what it ran and
what came of it, and that is the end. It gets no committed tool, no tests, no review round and no
task of its own. Code is committed only for what will be run again, and guarded only against what
has happened or plainly will; a script that has done its one job is deleted, never hardened. An
orchestrator filing work, an executor starting it and a reviewer judging it each refuse the larger
form: "this is once and done" is a finding, and the reply is to do it and close the task.

### `workflow/question-the-task-first`

A task is questioned when it starts, before any code. The executor that claims it reads the brief
against the tree and the board as they are now and answers two things on the thread: is this
still needed (already done, overtaken by another change, about a state that no longer exists, a
case nobody has), and is the way the brief asks for the simplest that solves it. A no to either
goes to the orchestrator with the smaller alternative, which may be "cancel", and the executor
does not build the brief as written while it waits. A brief is an instruction, not proof the work
is worth doing.

### `workflow/release-reds-first`

A red the release run files blocks every release, so it is fixed first, the way a bug fix is: its
fix takes the express path, and another run follows it. A candidate red
(`tests/release-red-*`) is filed at p0 and marked red, so it takes no orchestrator slot,
`just tasks next` offers it before other work of its priority, and the PULL digest lists it among
ready work at once, by priority and slug, and first of everything (`red_unstaffed`) once nobody
executes it 10 minutes after it was filed. An orchestrator staffs it before anything else, and one
agent fixes it with the smallest checks that cover the repair, starting from the failing tests or
the failing step rerun alone as its text names them. A test the run reran alone that passed is no
red, and has a p1 task of its run's instead; a red is fixed in the code, and a test red on load all
the same (it failed alone too, or was not rerun) is discarded in its fix instead
(`tests/no-test-fails-on-load`). A test or check is never loosened to
turn it green (an assertion widened, a timeout or a budget raised, a test ignored or moved out of
the release run): a fix that must change one says on its thread why the code was not the thing to
fix, and a fresh reviewer judges that. A budget or a cap that gives way, a red's or any task's,
gives way by its orchestrator's decision recorded on the task (`just tasks gate`,
[TASKS](docs/TASKS.md#gate-decisions)), once the cut came first, never by its executor's or its
reviewer's word, and lands at the number decided, or lowered below the default branch's. Its
assessor reads what its fix needs at review (`workflow/evidence-is-the-gate`): a fix that
changes none of the project's tests and checks
(`[land] check_paths`, [TASKS](docs/TASKS.md#the-default-landing)) needs its tests, the failing
tests unchanged and green at its tip, and one that changes a test, a check, a budget or a run step
needs a fresh reviewer's approve as well. A red run holds its commit as the release candidate, so the release is that commit and
its reds' fixes alone however fast main moves ([RELEASES](docs/RELEASES.md#the-release-candidate)):
its red's fix starts at the local branch `release-candidate`, never at main, merges that branch
again before review (but where a pull request delivers the land and the candidate advanced to a
delivered head, which no delivery merges: there it never merges `release-candidate`, and one not
yet published or approved is rebased onto it instead) and never merges main before its land,
and commits the client its own
sources make when it changes what the client is built from; its task's text says so. Where a pull
request delivers the land, such a fix of the client is approved naming that commit whatever its
assessment names, then merges main once, keeping main's side of every generated path, and that
merge is approved again and delivered, the candidate taking the commit approved before it
([RELEASES](docs/RELEASES.md#the-release-candidate)). Its own land
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
`[agents] launch` and `[agents.models]` ([AGENTS.md](AGENTS.md#this-repository-as-its-own-project)),
started as an in-process subagent (an Agent launch with `isolation: worktree`, its branch named
for the task's slug) or a headless `claude -p`; a terminal of its own (`just tasks launch SLUG`)
only when the work needs one. *A terminal keeps its memory until it is closed.*

### `reference-by-slug`

Name work by its slug, `area/slug` (`tools/memory-guard-hook`), the one its branch and commits
use; never a code like `M17`, which ends up shared by unrelated work.

## Code

### `code/consumer-api-two-releases`

A public Rust item a consumer calls keeps working in the release that brings its replacement,
without `#[deprecated]`: a consumer's lint denies warnings. A separate task may remove it only in
a later release, with concrete read-only evidence that consumers have migrated, updating the
consumer-API compile guards with it. Review checks a removal or signature change against those
guards; kit and helper major-version requirements still apply. The procedure and evidence are in
[CHECKS](docs/CHECKS.md#consumer-api-changes), from
[its card](docs/DECISIONS.md#consumer-api-break-release), alongside the current
[release contract](docs/RELEASES.md#the-release-run).

### `code/no-timed-waits`

No code sleeps or waits by a number: a wait is for its event (a channel's message, a process's
exit, a socket's answer, a page's `pageshow`, a `MutationObserver`'s change, the reader's press).
A number drives only a debounce, a throttle, a retry with exponential backoff, a deadline that
bounds a wait for an event, a clock's tick, or a check for an event its watcher can drop, and each
goes through one named helper: in Rust `test_time::wait::until` for a test's wait,
`backstop::debounce` for a server's watcher and its `Backstop`'s check each `PERIOD` (10 s), which
reads what an event dropped under load would have said (`apps/symphony-server/src/backstop.rs`),
the board store's retry (`crates/tasks/src/store/mod.rs`), `reload/retry.rs`'s `backoff` and
the GitHub program's `deliver::rest` between two reads of the checks it wrote
(`crates/symphony-github/src/deliver.rs`) and the delivery watcher's `dispatch::rest` between two
rounds of reading whether the host merged, which no event tells this machine
(`crates/tasks/src/land/dispatch.rs`); in the React client `src/timing.ts`'s `retryWait` (the
next of a `Backoff`'s waits), `Debounce` (whose rests take each other's place), `deadline` and
`tick` (to the next minute), with `Backoff` and `tryAgain` (`src/state.ts`), and in its unit
tests `src/testing.ts`'s `until` for a condition and `turn()` for a turn of the event loop. A
deadline on the wait itself (`recv_timeout`, `Condvar::wait_timeout`, `park_timeout`,
`tokio::time::timeout`, a socket's read timeout) is the event's own bound and needs no helper,
unless nothing else ends it but a stop, which makes it a sleep the lint cannot see and a reviewer
refuses: a server's loop is woken by a message from its stop, as from everything else it waits for
(`backstop::Waker`).

`just lint-rs` refuses a new one with a line naming this rule: clippy's `disallowed-methods`
(`clippy.toml`, which `crates/tasks/clippy.toml` starts with) for `std::thread::sleep`, tokio's
`sleep`, `sleep_until`, `interval` and `interval_at`, and Biome for the client's `setTimeout`,
`setInterval` and `src/timing.ts`'s plain `sleep` (`apps/symphony-web/biome.json`). Each site kept
is the baseline, marked by its linter's own suppression, where WHY names the event it should wait
for: `#[expect(clippy::disallowed_methods, reason = "no-timed-waits baseline: WHY")]` for Rust,
`// biome-ignore …: no-timed-waits baseline: WHY` for the client. The baseline only shrinks: a site
converted leaves its suppression unfulfilled or unused, which fails `just lint-rs` until it is
removed, and a new suppression is a new timer, which a reviewer refuses.
`git grep 'no-timed-waits baseline'` lists it. The lint reads every Rust target, the board crate's
tests too, which start their children through `crates/tasks/tests/support/children.rs`, whose
constructors alone carry the expectation the board's spawn rule asks for. It does not read the shell
scripts: those are read in review.

## Tests

### `workflow/tests-after`

Changed behaviour gets tests, written with or after the code and run after it, and a branch
shows them and their green run. No test-first order, no red run before the code and no planted
fault or reverted change to watch a test fail, whatever a brief asks; a reviewer may still probe
the code with a case it suspects the code gets wrong.

### `tests/rerun-the-failure-alone`

A test that failed is run again by itself, to see whether it passes, before anything is concluded
from it or changed for it; the whole suite is never run again for it. One that passes alone but
failed in the run is reported as load- or order-dependent, with both results.

### `tests/a-red-on-main-has-one-owner`

A test that fails on main, with the task's change taken out, is not the task's to fix or to wait
for: an orchestrator handles it as a task of its own, so that no other task is blocked by it and
one dedicated owner solves it. The agent that finds it proves it on main (the test alone, at main's
tip), tells its orchestrator on its thread with the command and output, and carries on: its own
review and landing name the red and do not wait on it. The orchestrator checks the board for a
task already holding that red, files one at p0 if there is none, and staffs it at once; a second
finder adds its evidence to that task and files nothing.

### `tests/no-test-fails-on-load`

A test passes or fails on what the code did, never on how long a loaded machine took to let it: a
test that fails when the machine is loaded is rewritten to count the CPU time it used rather than
the total time spent, or it leaves the release run, as below. A bound on a cost
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

The release run reruns each test that failed in it alone, one at a time, in the same checkout and
over the same build, up to 12 (docs/RELEASES.md, "The release run", step 5). One that passes alone
is no red and holds no release: the run files one p1 task for all such tests of it, which finds
why, then fixes the code or the runner, rewrites the test on a condition, a count, CPU time or an
injected clock, or discards it with one line on its thread. One that fails again is red, its task
saying it failed twice; a retry of nextest's never counts as a pass. A red test that failed alone
too, or was not rerun, and fails on load all the same (its failure is a wall-clock bound, a
sleep-ordered step or a timeout under load) is discarded, and the release moves on: a rerun neither
fails nor passes on the machine's load. The red's own fix removes it from the tree, with one line on
the red's thread naming the test, the evidence it is load (its runs alone and the failing line) and
what it proved. The owner has approved this for every such test already: no card is put to them to
discard one.
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
real choice: an orchestrator triages the register for such cards, so the owner is not asked what
is already decided. It marks one `[a]` with a dated
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
put to the owner: what an approved card decided, and what follows from it, is never asked again,
a card already posted for it is auto-approved, and a new question is a card of its own, never one
put back on the approved card. A consequence of an approved choice is whatever follows from it
while the option chosen stays the one the card described, a cost or a trade the card did not state included: it is
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
option, written for someone who has not read the internals: a card the owner cannot follow loses
their judgement, which is what it was for, and they may hand the choice back rather than make it.
Detail the decision does not need goes in a linked file, and pictures go where the thing judged is
visual ([REVIEW](docs/REVIEW.md#writing-a-card)).

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

A document under `docs/` describes intent, behaviour and options, and a guide (`AGENTS.md`,
`RULES.md`, a package's `AGENTS.md` or `README.md`) states each rule and its reason in plain words.
The owner's exact words and the dated history of who decided what, and when, live in the decision
register (`docs/DECISIONS.md`, each card's dated lines) and on task threads; a document whose
reason matters links its card, `[its card](DECISIONS.md#SLUG)`
([REVIEW](docs/REVIEW.md#slugs-in-prose)). The register and `docs/history/` are the records that
keep them: the words the guides and documents once quoted are in
[owner-words](docs/history/owner-words.md). `crates/symphony/tests/hooks/owner_words.rs` fails on
the owner's words in a guide or a document in the shapes the guides gave them: the owner, the
owner's, or the owner's word or rule followed by a date, or a straight quote mark right after
`owner's` or `the owner:`. Any other shape, a curly quote or an attribution in other words,
passes it, and a reviewer reads for that.

### `docs/one-topic-per-doc`

One topic per document, linked to the others, never merged into one file.

## Tools and the machine

### `commits/no-ai-authorship`

No commit, merge, tag, pull request, file header, licence line or anything else names Claude,
Claude Code, Codex or any other AI agent as author, co-author or generator: no `Co-Authored-By`
trailer, no "Generated with" line. The author and owner of this repository is Stefano Benatti,
and its licence is MIT (`LICENSE`). An agent's handle belongs on the board thread, never in the commit.

### `tools/no-new-python`

The repository tracks no Python file, and nothing it ships, builds, tests or checks runs Python:
its simple operations are `just` recipes and shell pipelines (`curl` with `jq` for JSON), and
anything more is Rust. An agent's throwaway code may be Python: a scratch script, a one-off edit or
count, never committed and never a step of a recipe, a test or a check.

### `memory-lives-in-the-repo`

Nothing goes in an agent's own memory folder (`~/.claude/projects/…/memory/`): what must outlast
a session is a rule here, guidance in an AGENTS.md, a skill, a doc or a board task, committed
where every agent sees it. In a Claude Code session the memory guard (`symphony hook memory-guard`,
which `symphony init` installs in `.claude/settings.json`) refuses a write tool's path there, or in
Codex's `~/.codex/memories/`, and a shell command that names either folder and writes. It reads the
command as text: the write shapes it knows, and those it misses, are its module doc's
(`crates/symphony/src/hook/memory_guard.rs`), others pass, and so does a write through a link made
before. A Codex session runs no Claude hook, so nothing guards it, its own `~/.codex/memories/`
included. `.claude/settings.json` also denies Edit there.

### `tools/parallel-by-default`

Anything an agent runs works beside the same thing in another worktree: no fixed path two runs
both write, no fixed port or name, no lock (`just symphony --port 0`, a scratch board, a
temporary machine folder). State shared by design, such as the board in the git common dir, is
correct as it is, and only a truly single resource is serialised, under a lock named for it. The
git stash is shared too: commit to your branch instead.

### `tools/the-machine-is-shared`

Many agents share this machine, and its disk is limited. A long command is started in the
background and waited for, never started again while it still runs: a second start doubles the
load and finishes later. No agent copies a build folder, and an agent removes the `build/` output
of its worktree when its work there is done.

### `tools/no-sweep-on-live-state`

A sweep or a migration an agent runs, to try a change or to see what it does, runs against a
scratch repository and a scratch board, never the real worktrees or the live board.

### `tools/a-consumers-checkout-is-read-only`

A consuming project's checkout on this machine is read-only to an agent working here: it is read,
and nothing in it is changed but what a release's refresh moves there, which is the release run's,
and an act there the owner's word names, which is that word's, such as a pin's commit made once
by hand ([its card](docs/DECISIONS.md#consumer-manifest-line)).

### `commits/only-the-land-pushes`

Nothing is pushed but a task's branch, by the dispatch of its land, for the pull request that
delivers it, and a release's tag, by the release run
([AGENTS.md](AGENTS.md#this-repository-as-its-own-project)): nothing else of a task. Main is
pushed by nobody, the owner included, while this repository sets `[land] delivery`: it moves only
by GitHub's squash of a task's pull request, and its ruleset refuses any other update
([TASKS](docs/TASKS.md#a-land-delivered-by-a-pull-request)). A consuming project's branch is
pushed only on the owner's word naming that push; a release's refresh makes none and moves no pin
([the refresh](docs/RELEASES.md#the-refresh)).

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
