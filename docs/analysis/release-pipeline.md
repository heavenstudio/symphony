# The release pipeline, redesigned

Why the release run took a night and a morning to release nothing, what a pasted proposal for
pre-certified CI (Buildmark) has that the Symphony lacks and the reverse, and the options for a
run that is faster and steadier, a hosted run on GitHub among them. The ask and its words are on
the thread of the board task `docs/release-pipeline-redesign`. This page changes no check, run
step, budget, test or setting, and builds no attestation, signer or workflow: each option's
card in [DECISIONS](../DECISIONS.md) is answered, and what is built from them is a board task,
each named in [what follows](#what-follows).

"Read 2026-11-18" marks this machine that day: an Apple M3 Max laptop, 16 cores, shared with the
agents whose work the run judges. The run records are in the git common dir,
`symphony/release/runs/ID.json`, their logs under `symphony/release/logs/ID/`; each fact below
names its run by that id, and each fix by its board task.

## The result

- **The run already reports every red at once.** `just verify` runs each step whatever the ones
  before it did, and every lane runs with `--no-fail-fast`
  ([CHECKS](../CHECKS.md#the-release-run)). The rounds were not one failure at a time by the
  run's design: each round was red in *other* tests than the round before, most of them tests
  that pass alone.
- **Load, not defects, held both candidates.** From 2026-11-17 20:31 to 2026-11-18 10:53, eight
  red runs filed ten reds. Their fixes deleted 11 tests as load-dependent, after one or two runs
  alone each. Two rounds were red in nothing else (2026-11-17T2211-98626 and
  2026-11-18T0931-12491): each would have been a release had the run itself rerun its failures
  alone.
- **Three of the 11 were not load.** Three deleted store tests failed as `idle=1` where
  `idle=0` was expected. The next morning the same line was traced to the machine's own
  terminals read by the tests, a fault of the run's environment, and fixed without deleting
  anything (`tests/release-red-verify-tests-23`). The three were then read again and restored
  unchanged (`tests/idle-count-discards-restored`). "Passes alone" cannot tell load from
  environment, and the discard rule acts on it at once.
- **The candidate's bounds made it worse once.** The reruns bound dropped a candidate whose last
  round was red in two load-dependent tests and one check already fixed on main. The run of
  main's tip it made due took 17 more landings and three new deterministic reds with them, one of
  which (a size budget) then took 8 hours 46 minutes.
- **The pasted proposal describes what the Symphony already does, and assumes what broke
  here.** A task's recorded runs at its tip are its evidence, a land goes by the stamps standing
  on the tip, and no server runs anything again. What the proposal adds is a signature, a digest
  of the checks' environment and a verifier on another machine. What it assumes, that a branch
  green alone merges green, is the class of red the release run exists to find.
- **Decided, each on its card:** the run reruns each failure alone before it calls it red
  ([B](#b-the-run-reruns-each-failure-alone)), and the release's checks run on GitHub where the
  plugin is there ([F](#the-hosted-run-on-github)). Main is not judged after each landing
  ([C](#c-main-judged-in-smaller-steps)): a run is paced by how long it takes, more often only
  on GitHub and never on this machine. A task's own recorded run spares the release nothing
  ([E](#e-runs-accepted-in-place-of-running-again)). Steps and lanes carried by their inputs
  within a candidate ([A](#a-a-steps-inputs-narrower-than-the-tree)) is the release candidate's
  own pending part, in progress.

## The day's record

Times are local. Load is the one-minute, five-minute and fifteen-minute averages when the tests
step ended, on 16 processors, as each run's `steps.tsv` recorded them. "Wide" is the landings
since the commit the run before it judged.

| run | judged | wide | run took | load at the tests' end | red |
|---|---|---|---|---|---|
| 2026-11-17T1950-53603 | `8bc9d7da1e74`, a candidate's third rerun | | 37 min | 61, 85, 110 | none: `release-2026-11-17.3` |
| 2026-11-17T2031-63033 | main, `63861e0139ba` | 36 | 28 min | 28, 44, 68 | size check; 22 tests |
| 2026-11-17T2131-56140 | candidate, rerun 1 | | 29 min | 21, 48, 83 | 3 tests |
| 2026-11-17T2211-98626 | candidate, rerun 2 | | 26 min | 30, 53, 75 | 1 test |
| 2026-11-17T2249-30801 | candidate, rerun 3 | | 25 min | 11, 32, 45 | 2 tests; crates check. Dropped: three red reruns |
| 2026-11-17T2318-55030 | main, `53f30a300904` | 17 | 21 min | 14, 30, 38 | size check; 3 tests. Dropped at once (below) |
| 2026-11-17T2342-2053 | main, `5f185d0f54fe` | 2 | 21 min | 13, 50, 57 | size check; 4 tests |
| 2026-11-18T0825-74490 | candidate, rerun 1 | | 41 min | 124, 143, 111 | 1 test |
| 2026-11-18T0931-12491 | candidate, rerun 2 | | 54 min | 50, 68, 62 | 2 tests |
| 2026-11-18T1053-68975 | candidate, rerun 3 | | running when read | | 2 tests failed so far |

What each red turned out to be, from its task's thread:

| red task | run | filed to landed | what it was |
|---|---|---|---|
| `tests/release-red-verify-tests-18` | 2031 | 25 min | 20 of 22 failures deterministic: 15 browser tests whose fixtures predated a change of the board's page, one first-visit byte budget, two store tests, on both stores, counting a query a landing added. 2 deleted as load |
| `tests/release-red-verify-size-check-4` | 2031 | 30 min | the server 62,867 bytes past its budget; fixed in the code |
| `tests/release-red-verify-tests-19` | 2131 | 11 min | 1 deterministic, caused by the fix before it (a style sheet's copy not synced). 2 deleted as load |
| `tests/release-red-verify-tests-20` | 2211 | 11 min | 1 deleted as load |
| `tests/release-red-verify-tests-21` | 2249 | 27 min | 2 deleted as load, one of them a wall-clock budget |
| `tests/release-red-verify-crates-check-2` | 2249 | closed in 2 min | fixed on main already by another task, not in the candidate |
| `tests/release-red-verify-size-check-5` | 2318 | 8 h 46 min | two binaries past their budgets after 257 commits; twenty reductions, then the server's line raised. Its executor was lost overnight |
| `tests/release-red-verify-tests-22` | 2342 | 16 min | 2 deterministic: a list of the board's tables that a landing's new table made stale. 2 deleted as load |
| `tests/release-red-verify-tests-23` | 0825 | 25 min | the run's environment: the store tests read the machine's own terminals. Fixed in the runner; nothing deleted |
| `tests/release-red-verify-tests-24` | 0931 | 27 min | 2 deleted as load |

Read together:

- **A round costs 50 to 80 minutes, and the run is the smaller half only at night.** A run took
  21 to 54 minutes; a fix took 11 to 30 minutes from its filing to its land, the executor, the
  assessor and the reviewer included; and the rerun started within two minutes of that land every
  time. The one long wait was a fix, not a run.
- **The first run of a wide candidate finds the real reds; the reruns find load.** Run 2031, 36
  landings wide, found 20 deterministic failures. The five reruns found one deterministic
  failure (caused by a fix), one fault of the environment, one check fixed elsewhere and seven
  load-dependent tests.
- **The same environment fault was read as load three times.** `idle=1` for `idle=0` failed
  `mixed_dispatchers::an_older_dispatcher_between_newer_ones_repeats_no_line_and_widens_none`
  (run 2031),
  `quiet_callbacks::pull_updates::a_failed_unanswered_scan_delivers_the_rest_and_never_repeats_a_told_post`
  (2131) and
  `quiet_callbacks::an_unreviewed_task_returning_is_new_even_when_its_status_never_changes`
  (2342). Each passed alone once and was deleted. Run 0825 failed a fourth test on the same line,
  and its task found the cause: a terminal of another project open on this machine, named as a
  fixture's session is, read through the machine's `hc`. `tools/test-lanes.sh` now gives the
  lanes an `hc` that answers nothing. The thread of that fix does not name the three earlier
  tests; `tests/idle-count-discards-restored` read their logs, where each failure adds one
  `idle` line naming a session of that other project's board live over the run, and restored all
  three as they were deleted.
- **The laptop slept under a run.** Run 0931's record says it took 1,425 s; its logs span 09:32
  to 10:25. The system's power log has the machine asleep, lid closed on battery, for all but
  five minutes of 09:52 to 10:23, with the `board` and `rest` lanes still running. The record
  counts a clock that stops in sleep, and its tests step, timed by the shell, says 3,014 s. No
  failure of that run is the sleep's: the browser lane had ended at 09:49.
- **Agents were held back for a run.** During run 1053 executors were told to hold their builds
  and tests, then to stop their checks (the thread of `tests/release-red-verify-tests-24`, 11:03
  to 11:15), which [the rule](../../RULES.md) on load forbids an orchestrator to do for a run.
  The run's result had come to depend on it.

## The pasted proposal, claim by claim

Buildmark's proposal: a developer or agent runs the CI pipeline locally, a signed attestation
says it passed, and a GitHub App accepts the attestation in place of running the pipeline again.

| the proposal | the Symphony today | where it stands |
|---|---|---|
| The development loop and the verification loop are apart: push, wait for central CI, fail, push again | There is no central CI. An executor runs its checks in its worktree, and the board's recorder writes each build and test with the commit it ran at ([MACHINE_WORK](../MACHINE_WORK.md#recording-builds-and-tests), [RUNS](../RUNS.md#a-run-and-its-steps)) | The Symphony is already past this problem |
| Agents run checks locally and the server repeats them | Nothing is repeated: a reviewer takes a green recorded run at the tip as run and builds nothing it covers (RULES.md `workflow/no-second-build-in-review`, [CHECKS](../CHECKS.md#no-second-build-in-review)) | Has it, by rule and not by signature |
| The attestation applies to the exact commit or prospective merge | A stamp stands on the task's tip: the `tests` stamp reads the recorded runs at that commit, and an approve names it ([TASKS](../TASKS.md#needed-stamps)). Nothing is bound to the merge's tree | Half. See [the merge tree](#the-merge-tree) |
| It used the current CI workflow and configuration | The pipeline is in the tree: `tools/verify.sh`, `tools/test-lanes.sh`, the justfile, the toolchain pin, the pinned SurrealDB and Node. A commit's tree names them, so a green on a tree is a green of that pipeline. The stamp catalogue is read from main's committed `symphony.toml`, never from the branch | Has it for what is in the tree. Lacks it for what is not: Chrome's version, the machine's PostgreSQL, the system, and crates.io as the crates check reads it |
| All required jobs passed | A task's gate is deliberately not the whole pipeline: the tests closest to the change and the lint (RULES.md `workflow/evidence-is-the-gate`). Which stamps it needs is its assessor's, from the catalogue | Does not fit, on purpose. See [where it does not fit](#where-it-does-not-fit) |
| It was signed by an authorised developer, agent or device | A run is written by the board's recorder, not by the command it runs; a verdict by a session that never executed, orchestrated or filed the task. Neither is signed: every session is one user on one machine | Lacks it, and a signature would add nothing while the signer and the verifier are the same user on the same machine |
| A GitHub App verifies it and publishes the required checks | [The GitHub landing design](github-pr-landing.md#checks-github-requires) proposes exactly this shape: a snapshot of the assessment and each stamp's evidence at the PR's head, with digests of the catalogue and policy, published as required checks by a dedicated App, with no hosted job | Designed, not built. It publishes stamps, not a pipeline's result |
| If it is missing, stale or invalid, conventional CI runs | A missing stamp refuses the land, naming who gives it ([TASKS](../TASKS.md#the-default-landing)). Nothing runs in its place | Stricter: no fallback that runs later what was not run |
| Some jobs remain server-side | The full verification runs once, later, on main: the release run ([RELEASES](../RELEASES.md#the-release-run)) | Has the split, drawn elsewhere: a task's focused checks and the release's whole run, not portable jobs and the rest |
| Reuse a run already completed in place of running it again | The candidate's rerun carries a green: a step green on a commit whose tree is the tip's is not run again ([RELEASES](../RELEASES.md#the-release-candidate), *The rerun*). A consuming project's certificates are its own and no part of this repository ([TASKS](../TASKS.md)) | Has it, at the coarsest key there is: the whole tree |

### The merge tree

The proposal's own words carry its weak point: "assuming the target branch has not changed
incompatibly, integration returns to an ordinary merge". Main here gained 55 landings in four
and a half hours on 2026-11-17. A branch's runs are of its tip, which merged main once before review
(RULES.md `workflow/never-chase-main`); by its land main has moved, the merge's tree is a tree
nothing ran, and a run on the prospective merge is stale as soon as the next task lands. Binding
evidence to the merge tree needs either a queue or a run again at each land, and
[the land's card](../DECISIONS.md#land-tests-changed-packages) rejected both: nothing automatic
waits on a land.

Every deterministic red of the day was of this kind. The 16 browser fixtures, the two query
counts and the table list each passed at the tip of the task that wrote them and broke on a main
that another task had changed. No attestation of either branch would have been false, and none
would have caught it.

### What each has that the other lacks

The proposal has, and the Symphony lacks:

- **A digest of the environment.** A tree names the pipeline but not the browser, the database or
  the system it ran on. The day's `idle=1` reds were the environment's, and nothing in a run's
  record said what the environment was.
- **A signer.** Worth having only with the next.
- **A verifier outside the machine that ran it.** Today the machine that runs is the machine
  that believes. This starts to matter with [the hosted run](#the-hosted-run-on-github), where
  one machine runs and another tags, and there the platform supplies both: a workflow run names
  the commit and the workflow file it ran, and GitHub signs artifact attestations for it.

The Symphony has, and the proposal lacks:

- **An assessment of what a change needs.** The proposal's "required jobs" are one list for every
  change. Here an independent assessor reads the diff and names the stamps, so a document lands
  on none and a store change on its tests and a reviewer.
- **A reviewer who is not the author.** The proposal trusts whoever holds the key. A stamp here
  is given by a session that neither wrote nor requested the work.
- **A run for what branches do to each other,** with one owner for each red it finds.

### Where it does not fit

The proposal's saving is the server's second run of the same pipeline. The Symphony never had the
second run, so there is nothing to save at a task's gate, and the gate is not the pipeline. The
one place the whole pipeline runs is the release, and there the proposal's shape does fit, turned
around: a run on one machine, accepted on another in place of running again
([E](#e-runs-accepted-in-place-of-running-again)).

## What the release run is for

Every landing carries its stamps, so the release run is not a second look at any task. It finds
what no task's gate can, and the day shows four classes:

| class | the day's examples | found now | cheapest place to find it |
|---|---|---|---|
| **Combination**: two changes green apart, red together | 16 browser fixtures, two query counts, the table list (runs 2031, 2342) | up to 36 landings later, among 22 failures | a whole run of main sooner, which only a hosted runner affords: runs paced by a run's length ([C](#c-main-judged-in-smaller-steps)) |
| **A fix's own break** | the style sheet's copy, broken by the fix before it (run 2131) | one round later | the fix's own evidence: the test that failed takes 10 s, and [CHECKS](../CHECKS.md#evidence-by-kind-of-change) already names it as the closest test for a change to a copied file |
| **A budget across many landings** | the size budgets, 62,867 bytes after 36 landings and 331,430 after 257 commits (runs 2031, 2318) | when the sum crosses the line, with no landing to name | the same: a run of main sooner names fewer landings ([C](#c-main-judged-in-smaller-steps)) |
| **The run's environment** | the machine's terminals read by the store tests (runs 2031 to 0825); the laptop asleep (run 0931) | as a test's red, read as load | a runner that is nobody's desk ([D](#d-the-run-away-from-the-agents-load)), and a run that says what it ran on |
| **Load** | 11 tests deleted | as a round each | never as a red: a rerun alone in the same run ([B](#b-the-run-reruns-each-failure-alone)), and tests written on conditions and counts ([CHECKS](../CHECKS.md#time-in-tests)) |

The run's own product, a commit every check found green, is needed still: a consuming project
takes a release tag and never main ([RELEASES](../RELEASES.md)).

## The options

Each says what it costs, what it stops catching, and what it would have done to the day.

### A. A step's inputs narrower than the tree

A candidate's rerun carries a green only across commits of the same tree, so every fix reruns
every step ([RELEASES](../RELEASES.md#the-release-candidate), *The rerun*). Give each step, and
each lane of the tests, its inputs, and carry its green across any commit that leaves them alone:

- **A step's inputs** are the paths it reads and the paths of the check itself. The client
  check has them already: `tools/client-key.sh`, the hash the regenerator skips a tip by
  ([TASKS](../TASKS.md#the-regenerator)).
- **A lane's inputs** are derived, not written by hand: the packages its targets build with, from
  `cargo metadata`, their folders, the lock file and the toolchain pin, plus the paths its tests
  read outside those. A target that reads the whole tree (the hooks tests of `crates/symphony`
  read every tracked file) declares the whole tree and is never carried.
- **Its record** says, for each green carried, the commit and run it was green on and the digest
  of its inputs, and the release's notes name them, as they name a carried green now.

Cost: the derivation, a digest for each step and lane, and the rule that an input left out
carries a green the change broke. That is its one real risk, so it is bounded: a green is carried
only within one candidate, from a run of that candidate, for at most the candidate's 24 hours.

It stops catching: a break through a path no input names. The first run of every candidate is
still whole.

The day: the five steps before the tests reran whole in each of the six reruns, 207 to 395 s
each, about 30 minutes in all, for fixes that changed tests and one style sheet. By lanes,
`tests/release-red-verify-tests-20` changed the browser tests alone: its rerun would have been
the browser lane, about 12 minutes of a 22-minute tests step. It shortens a rerun by minutes. It
removes no round.

This is the [release candidate's design](release-candidate.md#the-trades) as approved, which
names the trade (the commit tagged is not itself run in full) and leaves it to the task that
builds the inputs. It is filed: `tools/release-steps-carry-by-inputs`. The tests are where a
rerun's time is, so this, with the one scheduler and the time per test
`tools/nextest-runs-the-tests` gave the step, is where a run not made again saves most
([E](#e-runs-accepted-in-place-of-running-again)).

### B. The run reruns each failure alone

Chosen on [its card](../DECISIONS.md#release-run-reruns-alone), and built
([RELEASES](../RELEASES.md#the-release-run), step 5, `crates/symphony-build/src/release/alone.rs`).
The run knew the command that reruns each failure alone, and wrote it into the red it filed; it
runs them itself: once the tests have ended, each test the run named failed is run again alone,
one at a time, in the same checkout and build (`tools/test-lanes.sh --alone`).

- **Fails alone:** red, as now, its task saying it failed twice.
- **Passes alone:** not a red of the release. The run says both results and the load at each,
  and files one task for the tests that did so, at p1 and not in the candidate's series: to find
  why, and then to rewrite the test on a condition or a count, fix the runner, or discard it by
  [the rule](../CHECKS.md#time-in-tests). The release is judged by the run alone.
- **A target that named no failed test** (a lane killed, a binary that died) is rerun whole, alone.
- **A bound:** 12 tests, the number a red already lists reruns for. More than that failed is a
  red without reruns: a break, not load.

Cost: the reruns' own time, seconds to a few minutes (the day's reruns alone took 0.5 s to 45 s
each without a build), and a rule changed: a test that fails in company and passes alone today
leaves the tree before the release; under B the release is tagged with it in the tree and its
task follows.

It stops catching: a real fault that shows only in company (an order or a shared resource), for
the length of its task. Today that test is deleted on the same evidence, so nothing is caught
less; and a test that fails in company through the environment is no longer deleted for it.

The day: runs 2211 and 0931 were red only in tests that passed alone. With B, 2211 releases
`ec74ff5713be` at about 22:40 on 2026-11-17, and the candidate is never dropped. Had that not
happened, 0931 releases `5696d2f2afa5` at about 10:00 on 2026-11-18 (10:30 with the sleep). Runs
2031, 2131 and 2342 stay red for their deterministic failures and file fewer tests. Rounds saved:
two of eight outright, and the two drops of 23:14 and 23:42 with them. No test is deleted on the
strength of one run alone.

The candidate's bounds follow from it: a rerun counts as red only when something in it fails
alone ([the bounds](#the-candidates-bounds)).

### C. Main judged in smaller steps

A run judges main once 20 tasks have landed, and nothing judges main while a candidate is open
([RELEASES](../RELEASES.md#the-cadence), *Main while it is open*). The day's two first runs were
36 and 55 landings wide: run 2031 found 20 deterministic failures together, and the size overrun
of run 2318 took 8 hours 46 minutes to trace through 257 commits.

**Proposed and refused** ([its card](../DECISIONS.md#release-main-judged-in-steps)): a round
after each land, in the background, of the steps and lanes the landed changes can reach. At 12
landings an hour that is a round running most of the time, and rounds without end.

**The direction instead.** How often main is judged follows how long a run takes, not how often
tasks land:

- **On this machine** the count of landings stays as it is. It is there so the machine is not
  always under a run's load, and no run is added beside it.
- **On GitHub**, where [the hosted run](#the-hosted-run-on-github) is on, a run of main's tip
  starts on a clock set from how long a run there takes, which the trial measured at 49 minutes
  (workflow run 44903375883). The example of a run of an hour started every half hour, two at
  once, is not built: one run at a time holds the release's lock, so the interval is the least
  time between two starts and a run longer than it is followed by the next. It is the whole run
  on one commit, never a partial round, a tip already judged starts none, and a run it starts
  verifies nothing on this machine ([RELEASES](../RELEASES.md#the-hosted-clock), which says what
  was built and what each run still costs this machine: a landing is the tick).
- **A red** of such a run is a red of main as any other, naming the landings since the last
  green run, fewer than today's because the runs are closer.

Whether a hosted run of main goes on while a candidate is open is not decided here:
[the approved choice](../DECISIONS.md#release-main-while-candidate-open) is that none does, and
changing it is a card of its own.

### D. The run away from the agents' load

- **A reserved share of this machine.** macOS has no way to reserve processors for a process
  group, and holding agents back for a run is what [the rule](../../RULES.md)
  (`tests/no-test-fails-on-load`) forbids. Run 1053 did it by hand all the same. Not proposed.
  What the run does to itself was `tools/nextest-runs-the-tests`'s, landed: eight lanes of six
  threads on 16 cores made the run its own load
  ([the slow tests](slow-tests.md#what-the-lanes-do-to-each-other)), and one scheduler now runs
  every test.
- **Another machine.** A second Mac running `just release` in a clone removes the agents' load
  and the sleeping lid. It costs the machine, and it needs what the hosted run needs: a way for
  a result made elsewhere to become the record and the tag here. It is the hosted design with a
  runner of one's own, and GitHub runs a self-hosted runner by the same workflow.
- **Hosted CI.** [Below](#the-hosted-run-on-github).

Under any of them a test that fails on a quiet runner fails on what the code did, so the 11
discards of the day would have been none or real. The environment still has to be said: the
record of a run names the runner, the system, Chrome's and PostgreSQL's versions
(`tools/release-steps-carry-by-inputs` carries it, as the digest's second half).

One part of it needs no card: a run on a laptop says when the machine slept under it, and counts
the time by the clock. Filed: `tools/release-run-says-when-the-machine-slept`.

### E. Runs accepted in place of running again

The pasted shape, placed where it fits. Three candidates for a green accepted without a run:

- **At a land, a run on the merge tree.** No: [the merge tree](#the-merge-tree). The stamps at
  the tip are what a land goes by, and the combination is the release run's.
- **At a release, a task's own recorded runs.** A task that ran `just lint-rs` green at a tip
  whose lint inputs are the candidate's could spare the release that step. It would spare the
  lint, the docs audit and the client check, one to four minutes of a run of 21 to 54; the tests
  step, the long one, is never a task's run. And it lets an executor's run stand for the
  release's. Refused ([its card](../DECISIONS.md#release-accepts-task-runs)): one to four minutes
  of 21 to 54 is no saving worth the trade.
- **At a release, the release run's own greens, wherever they ran.** A step green in a release
  run, here or hosted, on the same inputs, is accepted by the next: A within a candidate, and F
  across machines. This is the proposal's idea with its trust the right way round: the run that
  certifies is never the task's executor.

What a green accepted so must carry, beyond what a carried green carries now: the digest of its
inputs, and the environment it ran on. A signature and an outside verifier come with F, from the
platform, and are not built here.

A run not made again is worth having only where the time is, which is the tests. No task's run
is the tests step, so that saving comes from the release run's own greens: a lane whose inputs a
fix left alone is not run again within a candidate (A, `tools/release-steps-carry-by-inputs`),
and each test now has a time and a timeout of its own under one scheduler
(`tools/nextest-runs-the-tests`). Nothing further is proposed.

## The hosted run on GitHub

Once the GitHub plugin is in, a release's checks run on GitHub's CI, and a project without the
plugin keeps the local run exactly as it is. The design, approved on
[its card](../DECISIONS.md#release-run-on-github). Its workflow is in the tree, and `just
release` starts it where a project turns it on, which this repository has not
([RELEASES](../RELEASES.md#on-githubs-runners), which says what was built and what was left for
later: a rerun of named steps and the rerun of a failure alone stay on this machine):

**Optional by the plugin.** The release run verifies locally unless the project's GitHub
integration is installed, its repository identity verified and hosted verification turned on,
each off when unsaid, as [the landing design](github-pr-landing.md#mode-and-publication) turns
on its own capabilities. With it off, or the plugin absent, nothing below exists: `just release`
is today's, to the byte.

**What the workflow runs.** One workflow file in the tree, started by `workflow_dispatch` alone,
with the commit as its input: never on a push or a pull request, since a task's gate stays local
([the landing design](github-pr-landing.md#checks-github-requires)). It checks out that commit
and runs that commit's own `just verify`, a step or a lane to a job:

- one job each for the lint, the client check, the size check, the docs audit, the crates and
  semver checks, the hosted board's check and the tests, the jobs being the steps the commit's
  `tools/verify.sh` names;
- the tests one job: their one build is the build folder itself, since all their lanes but one run
  by Cargo over it to keep their fixtures shared, and a nextest archive carries only what nextest
  runs. Jobs of a few lanes each would take that folder from one build, or each build its own,
  which the trial's measure of the folder and of the build decides;
- every job runs whatever the others did, as the steps do now, and uploads its log and its line
  of `steps.tsv`.

The workflow is thin on purpose: what a step does is the commit's `tools/verify.sh`, so a tree
still names its pipeline. A candidate's tip is reached by its commit, which is on `origin/main`
from its fix's land; the branch `release-candidate` is still never pushed.

**The runner.** macOS, since the product's release is built and installed on macOS and the
board's recorder has a macOS path of its own. The repository is public, and GitHub's standard
runners are free and unlimited for public repositories: the macOS one has 3 cores and 7 GB, the
Linux one 4 and 16 GB
([GitHub's runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)).
Whether the suite passes on Linux is unknown (60 places in the code are Linux's own, 160
macOS's); a Linux job is a later addition, not the judge.

**The pinned tools.** Each comes as it does here, by the tree's own scripts: the toolchain by
`rust-toolchain.toml`, SurrealDB by `tools/surreal.sh` (checksum-pinned), Node by `tools/node.sh`,
the semver checker by `tools/semver.sh`. Chrome is the runner image's, named with
`SYMPHONY_CHROME`. PostgreSQL 18 is installed by the job with Homebrew, the line
[PROJECT_FILE](../PROJECT_FILE.md#board) gives. The job records the versions of the last two and
of the system, which no pin holds.

**Secrets.** None. The verification reads crates.io without a token, and the job's own token
reads the repository and nothing more. The crates.io token, the tag and every write stay on the
owner's machine. Actions pinned by commit, no third party's action given a secret, since there
is none to give.

**How its result becomes the record and the tag here.** `just release` is still the command and
still takes the lock:

1. It picks the candidate as now, and in place of running `just verify` in its own checkout it
   dispatches the workflow for that commit and waits, within the run's time.
2. It reads the workflow run through the plugin's credential and believes it only if the run is
   of the bound repository, of the workflow file main commits, of exactly the candidate's commit,
   and ended; a job cancelled or timed out is a step that did not run, never a green.
3. It downloads each job's log into `logs/ID/` and writes `runs/ID.json` as it does now, each job
   a step with its exit, its seconds, its log and the workflow run it came from. The reader that
   files reds reads those logs unchanged, since they are the same steps' logs.
4. Red: each red step is filed on this board as now, and the candidate is held as now. Green: it
   tags the commit here, pushes that one tag, and goes on to the publication and the refresh.

**What stays local:** the lock, the candidate and its record, the reds, the tag and its push, the
publication on crates.io, the install, the boards, `current` and the wait for the server
([RELEASES](../RELEASES.md#the-refresh)). The hosted run verifies and does nothing else.

**When GitHub does not answer.** A dispatch refused, a run that never starts, or a result that
cannot be read is not a red of the commit. The run says so and verifies locally, as a project
without the plugin does, and its record says which judged. This differs from the landing design,
which never falls back to a local merge: a merge made another way is another act, and a local
verification is the same checks.

**What a hosted run costs.** In money, nothing while the repository is public; were it private,
macOS minutes are $0.062 each and Linux $0.006 at 2026's rates. In minutes, an estimate that only
a trial settles: the six slowest binaries took about 20,000 thread-seconds measured under load
here, two to five times what they take alone ([the slow tests](slow-tests.md#the-result)), so
about two to three core-hours of tests, plus a cold build for each job that builds. On 3-core
runners, five at once (the free plan's macOS limit, to be confirmed by the trial), that is 250
to 450 runner-minutes and 45 to 90 minutes of wall for a whole run, against 21 to 54 minutes here
under load. A rerun by inputs (A) is a lane or two: 10 to 20 minutes. It is slower than this
machine when this machine is free and steadier than it ever is; and with 7 GB a job, the browser
and store lanes are the ones a trial must show fit.

**What it would have done to the day.** No test would have failed for the agents' load or the
machine's terminals, the lid would not have mattered, and nobody would have been told to stop
work for a run. The deterministic reds would have been found as they were.

**How often it runs.** As the local run does, by the count of landings, until the trial has
measured a run. From then a run of main's tip may follow a clock set from a run's length
([C](#c-main-judged-in-smaller-steps)), which this machine's load never allowed.

`just release` starting it uses the credential and repository binding of the GitHub plugin
(`tools/github-pr-delivery`), and waits for the trial: the workflow started by hand on one
commit, and measured.

## The candidate's bounds

Three bounds end a candidate ([RELEASES](../RELEASES.md#its-bounds)). How each behaved:

- **Three red reruns.** It fired once, at 23:14 on 2026-11-17, on a candidate whose third rerun
  (2249) was red in two tests that passed alone and in a crates check fixed on main and not in
  the candidate. The run of main's tip it made due (2318) was 17 landings wider and red in three
  new deterministic ways. It made the day worse: it took a candidate one honest round from a
  release and replaced it with a wider one. It is about to fire again on run 1053, for two more
  tests of the kind, onto a main 14 landings further.
- **A fix that merged main.** It fired at 23:42, on a fix transplanted onto main after the drop
  above: a consequence of the first, costing one more run of 21 minutes.
- **24 hours.** It did not fire. Its clock ran through 8 hours 46 minutes in which the one open
  red had no live executor, so it measures the night as much as the candidate.

The bounds exist so that a candidate that never converges ends. A round red only in tests that
pass alone is not a candidate failing to converge, and the count cannot tell. Under
[B](#b-the-run-reruns-each-failure-alone) such a round is not red at all, so the reruns bound
counts what it was meant to: with B, neither candidate of the day reaches it. So no bound's
number changes, and there is no card: the change is B's consequence, built with it.

What a drop falls onto is unchanged: nothing judges main beside a candidate
([C](#c-main-judged-in-smaller-steps)), so each drop is wider than the last.

## What follows

The cards, each answered in [DECISIONS](../DECISIONS.md):

- **release-run-reruns-alone**: B. The run reruns each failure alone.
- **release-run-on-github**: the hosted design above, as written.
- **release-main-judged-in-steps**: refused. A run is paced by its own length, more often only
  on GitHub ([C](#c-main-judged-in-smaller-steps)).
- **release-accepts-task-runs**: refused. The saving is in the tests, which the release run's
  own carried greens reach ([E](#e-runs-accepted-in-place-of-running-again)).

Tasks:

- `tools/release-steps-carry-by-inputs` (A, and the environment a green names): the approved
  release candidate's own pending part, in progress.
- `tools/nextest-runs-the-tests`: one scheduler, a time and a timeout for each test. Landed.
- `tools/release-run-says-when-the-machine-slept`: the record's clock and the sleep.
- `tests/idle-count-discards-restored`: the three store tests deleted on the `idle=1` line, read
  again now that its cause is fixed. It was the cause of each, and all three are back, unchanged.
- **To be filed by the orchestrator**, which the board then names: B's rerun inside the run, with
  the candidate's reruns bound counting only what fails alone; the hosted run's trial, after
  `tools/github-pr-delivery`; the hosted run in `just release`, after the trial; and the hosted
  run's clock, after both (`tools/release-hosted-clock`).

Neither a card nor a task:

- **A signer and an attestation format** (E): nothing verifies one until a second machine runs,
  and then the platform supplies it. Built with F or not at all.
- **A reserved share of this machine** (D): the rule on load forbids it and the system cannot
  enforce it.
- **A second machine of one's own** (D): the hosted design with another runner label. It is a
  purchase, and nothing here needs it decided before a hosted trial says what the free runners
  do.
- **The bounds' numbers**: B's consequence, above.
- **A round of main after each land** (C): refused.

## Verified and inferred

Verified, read 2026-11-18: each run's commit, cause, steps, seconds and load, from its record and
`steps.tsv`; each failed test, from the lanes' logs; the candidate's rounds, reds and advances,
from `candidate.json` and the release status; the sleep, from the system's power log against the
log files' times; each red's diagnosis, fix and times, from its task's thread; the repository's
visibility and the runners' sizes and price for public repositories, from GitHub.

Inferred:

- That the three `idle=1` discards share the cause found in `tests/release-red-verify-tests-23`:
  each log's extra `idle` line names a handle that the other project's board on this machine
  records as its executor over that run. The terminal itself is no longer listed by `hc`, so
  that it carried the name is inferred from those records.
- What B would have released and when: from which runs were red only in tests that passed alone,
  each on one or two runs alone under load. A rerun inside the run could have failed where the
  executor's passed.
- Every figure of the hosted run but the runners' sizes and prices: the minutes are computed from
  times measured under load here, the limit of five macOS jobs at once is recalled and not read,
  and whether Chrome on the image and 7 GB suffice is unproved. The trial is what settles them.
- Run 1053's result: it was running when read, with two tests failed.
