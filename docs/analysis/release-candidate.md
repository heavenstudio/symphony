# A release candidate held still

How a red found by the full run gets fixed so that a release comes, and what the run and its
command are called. This is a design: built are the refresh's rule that a pin moves only
forward, which any candidate needs (A, below), the rename to `just release`
([below](#the-names)) and A's held candidate (`crates/symphony/src/release/candidate.rs`,
[RELEASES](../RELEASES.md#the-release-candidate)). A is chosen on its card in
[DECISIONS](../DECISIONS.md). This page is A as its review corrected it; the corrections follow
from A and stand with it, auto-approved on its card, and the three choices A leaves open are cards
of their own there.

The asks, in the order the board records them (the express-path task holds the first; its scope
note says the release candidate question came next):

- The run's reds are fixed first, as bug fixes are, since they block releases: the express path,
  followed by another run. The run can use the stamps for what did not change, so it is faster.
- A release candidate branch: fix, rerun the failures, release once green, then bring the fixes to
  main. How is open to suggestions.
- A test that fails when the machine is loaded is rewritten to count the CPU time it used rather
  than the total time spent, or leaves the run.
- The run could be called the release candidate rather than the nightly.
- A command named `release`, which would test and release, releasing nothing on a failure, which
  agents then fix.

The second and the last are proposals, not rulings: the owner asked for suggestions on how, and put
the command's name as a question. They are designed around here.

- **The problem.** A run judges main's tip, and main gains several landings an hour. A red's fix
  lands on main with everything else that landed meanwhile, so the next run judges a different
  tree, and what those landings brought can be red in turn. A release may never come.
- **Chosen: A, the fix's own tip is the candidate.** The run holds the commit it judged. A red's
  fix is the red's own task, on a branch started at that commit that never merges main before its
  approve, and it lands on main by the ordinary `just land`. On that task's land, the newest commit
  of its branch a reviewer approved that holds only the candidate and the fix becomes the
  candidate: it is on main from then on. A rerun of the steps that were red or did not run, and of
  those whose inputs the fixes changed, releases it. Main has every fix from its land; nothing is
  merged back; what a release is does not change.
- **Two trades.** While a candidate is open, main's tip gets no full run, and the release lacks
  what landed since the candidate. Once stamps land, the commit tagged is not itself run in full
  ([the trades](#the-trades)).
- **Three choices A leaves open, each on a card:** whether main's tip is run beside an open
  candidate ([below](#main-while-a-candidate-is-open)); what "release candidate" names
  ([the names](#the-names)); and how a deliberate break of a consumer's API is released
  ([below](#a-deliberate-break-of-a-consumers-api)).
- **The command is `just release`**, the commit it holds while its reds are fixed is the release
  candidate, and a red there is a candidate red, filed as `tests/release-red-NAME`
  ([the vocabulary](#the-vocabulary)). The rename is one step, everything at once, after the run
  learns to name every red step.

## Today

### What a run judges

`just nightly` (`crates/symphony/src/nightly.rs`, [RELEASES](../RELEASES.md#the-release-run)) fetches
`origin/main`, waits for the regenerator to make main's client distribution current, and judges
that tip in its own checkout: `just verify` as one check, then each consuming project built and
tested against it (`conciliate-ID`). Green, it tags the tip `release-YYYY-MM-DD`, pushes that one
tag and refreshes the machine. Red, each red check is a task `tests/nightly-red-NAME`, filed at p0
and marked red so it takes no orchestrator slot; a later run green in that check closes it, when
the later run's commit holds the commit the red was found on.

A run starts every 20 landings (`[releases] every_landings`, started by a land's `[board]
on_land`), counted since the newest release or the last run that checked something, and once a
night when the owner installs the schedule. No schedule is installed (no `symphony.nightly.*` in
`~/Library/LaunchAgents` and no crontab line, 2026-11-12).

### Why a release may never come

Each run judges main's newest tip. By 18:33 on 2026-11-12 (main at `c70a7515`), 56 tasks had
landed that day, 2 to 8 an hour; 38 of them changed `crates/tasks` or `crates/symphony`, and 29
changed `apps/`. The board's crate is a dependency of `symphony` and of the server, and
`symphony` of the server, so a change in either reaches most of the workspace's test targets. Only
4 of the 56 changed `docs/` alone.

So the run after a fix judges the fix and every landing since the red, and each of those can bring
a red of its own. Stamps (`perf/nightly-stamps-unchanged-checks`) skip a step whose inputs did not
change, but between two runs a few landings apart nearly every step's inputs did.

![Today: each run judges main's newest tip](release-candidate/today.svg)

### The first run, tonight

The first run started at 17:28 on 2026-11-12, from the cadence's first count (96 landings since
the line's start), on `3d95edfa`. It ended red at 18:27, in two checks:

- **`verify`, after 3,325 s.** Its size check found `symphony` at 19,719,136 bytes against a
  budget of 19,200,000, and `symphony-server` less its client at 20,341,566 against 20,100,000.
  `just verify` stops at its first red, so the docs audit and every test target never ran.
- **The consuming project's `conciliate-ID` step, after 154 s.** Its `gate-review` no longer
  compiles against the candidate: it calls `cli::board_client`, which the board's library no longer
  has. Behind it, hidden by the first, a second break: a field added to the public
  `land::Generator`, which the consuming project's tests build by its four fields. Both are being
  put back on this side, on the red's own task.

Filed: `tests/nightly-red-verify` and the consuming project's `tests/nightly-red-conciliate-ID`. In
the hour the run took, main gained 3 landings.

What it taught, under every option:

- **A run must name every red step, not stop at the first.** An hour's run said nothing of the
  tests, and each fix would cost another whole run to find the next red. `tools/verify-runs-every-step`
  (in progress) makes `just verify` run every step and name each red. Rerunning the failures then has
  steps to rerun; the stamps task records them and reuses the green of those whose inputs did not
  change.
- **A conciliation red is fixed on this side.** A release's refresh moves a consumer's pin in one
  commit that may change only `Cargo.toml` and `Cargo.lock` ([RELEASES](../RELEASES.md#the-refresh)),
  and the conciliation builds the consumer's default branch as committed. So the fix keeps what
  the consumer calls; one made in the consumer instead would have to build against both its
  current pin and the candidate. A deliberate break of a consumer's API has no way to a release
  today ([below](#a-deliberate-break-of-a-consumers-api)). A test here that compiles what the
  consumers call is being added with tonight's fix, as its orchestrator reports (not yet on that
  task's thread at 19:17).
- **A red is staffed first.** The express path's lane for reds (the red's priority, mark, slot,
  PULL and text) is in review at `5f67069b`.

## The options

Each option says how long a release takes after a red, what main and a consuming project's pin
hold meanwhile, what `just land`, the regenerator, the cadence, `--status` and the refresh must
change, and how it meets the owner's earlier rulings ([below](#the-rulings)). The git behaviour
each relies on was tried on a scratch repository ([the trials](#tried-on-a-scratch-repository)).
The owner chose A.

### A. The fix's own tip is the candidate (chosen)

1. **The candidate.** A run red in any check holds the commit it judged: a record in the run's
   store (the commit, when it was first judged, the round, each red with its task) and a local
   branch `release-candidate` at that commit, never pushed, which only the run moves. One
   candidate at a time.
2. **A red's fix** is the red's own task; its slices merge into its branch, as for any parent. The
   branch starts at `release-candidate` and never merges main before it is approved. Before review
   it merges `release-candidate` again, which moves when another red's fix lands, as any branch
   merges main today. Its evidence is the red check run alone at its tip. It lands with `just land`,
   as any task: one `--no-ff` merge into main. A land that conflicts goes back to its executor,
   who merges main and asks for a fresh review, as for any task
   ([TASKS](../TASKS.md#the-default-landing)).
3. **The advance.** The land's `on_land` (today `nightly --due SLUG MERGE`) moves the candidate
   only when SLUG is a red's task in the candidate's record. It takes the newest commit of the
   landed branch (an ancestor of the merge's second parent) that an approve on that task named and
   that passes two tests: it descends from the candidate, and every commit it holds beyond the
   candidate is the branch's own, none that main held before the land.
   - For a land that went straight in, that commit is the tip landed.
   - For one that conflicted, it is the tip approved before the executor merged main. It holds the
     candidate and the fix alone, and it is on main once the merged tip lands.
   - Tried: a task forked at the candidate passes the two tests, which is why only a red's own task
     moves it. The tip approved before a conflict's merge passes them, and the merged tip does not.
   - The run releases only a commit `origin/main` holds (`nightly.rs` fetches it and refuses any
     other), so the advance waits for the land's push: a land whose push failed moves nothing until
     its retry has pushed, and the next `--due` or `--rerun` then reads it.
4. **The rerun.** A red is answered by its task's land advancing the candidate, or by its task
   closed with its fix made outside this repository. The land that answers the last red starts the
   rerun of the candidate's tip through its `--due`; a red answered otherwise is picked up by the
   next landing's `--due`, or by `just release --rerun`, an orchestrator's retry as `--refresh` is.
   That is the owner's own proposal: rerun failures, release once green. The rerun runs:
   - each step that was red or did not run;
   - each step whose inputs the fixes changed (the stamps' key, comparing the tip with the commit
     first judged);
   - the conciliation.

   Every other step keeps the green it had on the first commit ([the trades](#the-trades)). Green:
   the tip is tagged, the tag pushed, the machine refreshed. Red: a new round, its reds filed as
   before.

   A red that passes when run alone is load- or order-dependent (RULES
   `tests/rerun-the-failure-alone`). By the rule on load (RULES `tests/no-test-fails-on-load`) its
   fix is the test rewritten to count the CPU time it used rather than the total time spent, or
   taken out of the run, so it is answered by a fix like any other, never by a rerun that happens
   to pass.
5. **Bounds.** The candidate is dropped, and the next run judges main's tip at once, when:
   - three reruns came back red;
   - it is 24 hours old;
   - a red's task landed with no approved commit that passes the advance, as when its executor
     merged main before any approve;
   - or a release on main passed it.

   The two numbers are the run's thresholds, `[releases]` settings, not the owner's to set. Before
   stamps, a rerun is a whole `just verify`. The first run's stopped at its size check after
   3,325 s without reaching a test, so a whole one takes longer, not yet measured, and three rounds
   may not fit in 24 hours.

What it changes:

- **`just land`:** nothing. The advance is read after the land, by `on_land`, from the board's
  record of the task's approves.
- **The regenerator:** nothing on main. The candidate's first commit has a current distribution,
  as the run waits for the regenerator today. A fix that changes what the client is built from
  commits the distribution its own sources make (`just client-update`): the one kind of branch
  that does, since no regenerator builds the candidate. The land keeps main's side of it, as for
  every branch ([TASKS](../TASKS.md#the-default-landing), emulated in the trials), so main is
  unchanged. The rerun's client check proves it. It costs that executor a client build: the 17
  client checks lands ran on the night of 2026-11-11 took 7 to 20 minutes each, median 11, as the
  register records.
- **The cadence:** no run of main's tip while a candidate is open; a drop makes one due at once.
  After the release the count runs from it as today: the landings on main that the release lacks,
  the fix's own included (tried). On a busy day that count is past 20 when the release is cut, so
  the next landing starts a run of main's tip.
- **`--status`:** a candidate block: its tip and first commit, its age and round, each red with its
  task and state, and what the rerun waits for.
- **The release notes:** they list main's first-parent commits since the release before
  (`nightly/notes.rs`). A chained candidate's first-parent line shows the second fix and its merge
  of the first, and drops the first fix's own subject (tried). A candidate release's notes list
  each red's task and the commit it advanced to, from the record.
- **The refresh: a pin moves only forward.** Built ahead of the rest
  ([RELEASES](../RELEASES.md#the-refresh), `nightly/refresh.rs`): a refresh moves a consumer's pin
  only to a release that descends from it. A pin moved by hand to a commit of main while a
  candidate is open is neither ahead of the release nor behind: moving it to the release would drop
  what it had since the candidate, a newer board schema among them (tried). Such a pin is left
  where it is and named in the refresh's report and in `--status`; the next release that descends
  from it moves it.
- **Elsewhere:** CHECKS' rule to merge main again before review, and AGENTS' "a branch commits no
  distribution", each get the candidate fix's exception; the red task's text says both.

**How long after a red:** the fixes, then one rerun of the red steps and of those the fixes reach.
Main's pace does not enter while each red's fix has an approve on a commit from before any merge
of main: the tree judged changes only by the fixes, so a new red can come only from a fix itself.
Without stamps the rerun is the whole verification of the tip, and still converges.

**Meanwhile:** main has each fix from its land. A project's pin keeps the release before. One moved
by hand to a commit of main gets each fix with main, and the refresh leaves it until a release
descends from it.

**Costs:**

- A fix is written on a tree up to a day old and lands on a newer main, so its land can conflict
  more often than a branch started at main (inferred, not measured). A conflict costs a merge of
  main and a fresh review, as for any branch; the candidate keeps the commit approved before it.
- Two reds' fixes are chained: the second merges the first before its review, and needs a fresh
  approve if the first lands during its review.
- Agents must base these branches on the candidate, not main: the red task says so, and a fix that
  merges main before any approve costs only the candidate, never a wrong release.
- The release is a commit main holds as a merge's second parent, not on main's first-parent line.
  Today's rules read "on `origin/main`" as an ancestor of its tip (`is_ancestor`), so nothing
  refuses it.

#### The trades

- **Main's tip gets no full run while a candidate is open.** Up to the 24-hour bound, main's newest
  landings are judged by their own evidence and by the regenerator's checks on each round (the
  client check, and the docs audit), not by the full run, and a break they bring is found by the
  first run after the release. The release is the candidate and its fixes, without what landed
  since the candidate.
- **Once stamps land, the commit tagged is not run in full.** The rerun runs the steps that were
  red, that did not run and that a fix reaches; every other step keeps its green from the
  candidate's first commit, whose inputs for that step are the same. That is the stamps
  used for what did not change, so that the run is faster, set against the earlier rule that a run
  tests everything and releases only when green. It is the stamps' trade under
  every option, D's runs of main included. How old a carried green may be is the stamps task's own
  card to put; a candidate's is at most its 24 hours. Before stamps, the rerun is the whole
  verification.

#### Main while a candidate is open

The first trade is the owner's to weigh, on a card:

- **A, as designed:** no run of main's tip while a candidate is open. The first landing after the
  release starts one when 20 or more have landed since the candidate, which a busy day reaches.
- **B:** a run of main's tip right after each candidate's release, whatever the count. One more
  full run per candidate on a quiet day; on a busy day the same as A.
- **C:** runs of main's tip at the cadence while the candidate is open. As the run stands they take
  turns with the candidate's reruns: only one run holds `run.lock` at a time, and every run uses
  the one checkout ([RELEASES](../RELEASES.md), *One run at a time*). A run of main holding the
  lock when a rerun is due delays the release by up to a whole run (the first run took an hour
  and never reached the tests) and spends the 24-hour bound; and the reds of two trees share one
  red series, main's fixed after the release. Running the two at once would need that rule lifted
  and a second checkout.

Recommended: A. The 24-hour bound caps how long main goes without a full run, the regenerator
checks every round, and a busy day's count makes the next run start as the release is cut.

- [r] **release-main-while-candidate-open** — While a release candidate is open, the run judges nothing on main's tip, up to its 24-hour bound. Main's newest landings are checked only by their own evidence and by the regenerator's client check and docs audit each round; a break they bring is found by the first run after the release, and the release lacks what landed since the candidate. Should main's tip also get a run? A: no, as designed; the first landing after the release starts one when 20 or more have landed since the candidate, which a busy day reaches. B: a run of main's tip right after each candidate's release, whatever the count: one more full run per candidate on a quiet day. C: runs of main's tip at the cadence while the candidate is open, taking turns with its reruns, since only one run holds the lock at a time: a run of main holding it when a rerun is due delays the release by up to a whole run (the first run took an hour and never reached the tests) and spends the 24-hour bound, and the reds of two trees share one series. Recommended: A. Approve and say A, B or C in the note, or comment. [[analysis/release-candidate#Main while a candidate is open]]
  [2026-11-12 19:22] agent — Posted for the board task `design/release-candidate-branch`, from the review of the release candidate design you approved.
  [2026-11-12 20:24] owner · approved — A. this should had been auto approve/triaged, its basically what was decided in the other card.
  [2026-11-12 21:14] agent — A it is, and you are right that it should not have been asked: the release-candidate process A you approved at 19:05 already holds the run on the candidate, so main's tip gets no full run while a candidate is open, and B or C would have changed that. Recorded for the board task `tools/release-candidate-held`, which builds it: the first landing after a release starts a run of main's tip once 20 or more have landed since the candidate.
  [2026-11-12 21:15] agent · resolved

### B. The fix on main first, then picked onto the candidate

The fix is an ordinary task, based on main, and lands there. The run then copies its commits onto
the candidate (a cherry-pick) and reruns.

- **How long after a red:** as A, when the pick applies. A pick conflicts where the fix's lines or
  the lines beside them changed on main since the candidate, as A's land does: tried, the same fix
  conflicts as A's land and as B's pick, so conflicts do not set B apart. A failed pick drops the
  candidate, since no approved commit holds the fix on the candidate's base.
- **Meanwhile:** as A.
- **The release is a commit nobody reviewed and main lacks:** the pick (tried). So the rules that a
  release is on `origin/main` and only moves forward must change, or the run merges the candidate
  back into main (an empty merge, tried) and pushes main, where today it pushes only its tag.
- **The regenerator:** the fix landed with no distribution, so the run builds the candidate's and
  commits it there, another commit nobody reviewed.
- **The cadence** counts from a release on main's line (`since`, by `is_ancestor`); a release main
  lacks is passed over, so the count needs changing too.
- **`just land`:** nothing. **`--status`:** as A, plus each pick. **The refresh:** as A.

### C. The fix on the candidate only, merged into main after the release

The ask read literally: fix, rerun the failures, release once green, then bring the fixes to
main. The candidate is a task's branch started at the commit judged; each
red's fix is a slice committed on it; the rerun judges its tip; green tags it; then the branch
lands on main.

- **How long after a red:** as A.
- **Meanwhile main lacks every fix** until the merge back: each agent on main meets the same red,
  and a pin moved by hand to main's commits lacks it too.
- **The release is a commit main lacks** until the merge back, so the same rules as B change, and a
  run of main's tip is refused (not a descendant of the newest release) until the merge back
  lands.
- **The merge back can conflict, after the release is cut** (tried): the release then holds a fix
  that main still lacks, until an executor resolves it.
- **The regenerator:** the candidate branch commits its own distribution, as A's fixes do.
- **`just land`:** nothing; the merge back is the candidate task's land. **The cadence:** as A,
  while the merge back waits. **`--status`, the refresh:** as A.

### D. No candidate: each run judges main's newest tip

What was being built: a red's fix takes the express path and its land starts another run of
main's tip (`tools/nightly-reds-take-the-express-path`), and a run reuses the green of each step
whose inputs did not change (`perf/nightly-stamps-unchanged-checks`).

- **How long after a red:** the fix, then a run of main's tip: every step whose inputs any landing
  since the red changed. On a day like 2026-11-12 that is close to a full run. A release comes when
  such a run finds no new red, which is less likely the busier main is: the first run, judging 96
  landings, found 2.
- **Meanwhile:** main has the fix at once; pins keep the release before.
- **Changes:** none to `just land`, the regenerator or the refresh; the cadence gets the follow-up
  trigger, and `--status` says "due: the last night's reds are fixed".

### The orchestrator's suggestion, two merges

The brief's starting point: a candidate branch, and each fix merged into it and into main in the
same act. Tried: merged into the candidate with `--no-ff`, the candidate's tip is a merge main does
not hold, so a release tagged there is off main, and main no longer descends from the newest
release until the candidate is merged back. Merged into the candidate by fast-forward instead, the
candidate's tip is the fix's tip itself: that is A, and the land needs no change at all.

The `--no-ff` shape has one thing A lacks: two fixes made in parallel are merged into the
candidate side by side, with no chain and no fresh approve (tried). Its price is a release whose
tree, the two fixes together, no reviewer approved and main lacks until the candidate is merged
back, a merge that changes nothing in main's tree once both fixes have landed there (tried). A
trades that for its chain: the second fix merges the first, and is approved again if the first
lands during its review.

### Side by side

| | A | B | C | D |
|---|---|---|---|---|
| after a red, a release waits for | the fixes and one rerun | the fixes, the picks and one rerun | the fixes and one rerun | the fixes and a run of main's newest tip, again after each new red |
| main meanwhile | each fix from its land | each fix from its land | no fix until after the release | each fix from its land |
| main's tip run in full meanwhile | no, up to 24 hours | no | no | yes |
| the release is | the newest approved commit of the last red's fix, on main | a pick main lacks, unreviewed | the candidate's tip, which main lacks until merged back | main's tip |
| the commit tagged run in full | before stamps; after, the steps red, unrun or reached by a fix | as A | as A | before stamps; after, the steps whose inputs changed |
| rules that change | none about a release; the refresh moves pins only forward | a release off main; moving forward; the refresh | a release off main; moving forward; the refresh | none |
| `just land` | unchanged | unchanged | unchanged | unchanged |
| the candidate's client | its fixes commit it | the run commits it | its branch commits it | the regenerator, as today |

### The rulings

| the owner's earlier ruling | A | B | C | D |
|---|---|---|---|---|
| No lock or queue on a land, which would bottleneck the pipeline | met: the land is unchanged | met | met | met |
| A release only after a full run that is green, nightly or more often, which tests everything and releases only when green | met as the stamps allow for what did not change: each step red, unrun or reached by a fix runs on the tree released, and every other keeps the green of a commit whose inputs for it are the same; before stamps, every step runs on it | as A | as A | as A, against the last run |
| Releases built from full runs of verification and conciliation, urgent fixes out of cycle | met | met | met | met |
| Fixes to reds first, on the express path, another run following | met: the express path for the fix, and the candidate's rerun is the run that follows it | met | met | met, as written |

Two more are the repository's rules rather than the owner's rulings: nothing is force-pushed
(AGENTS: "never forces"), and only the run pushes a tag ([RELEASES](../RELEASES.md#who-runs-it)).
Every option meets both: nothing is rewritten, and B's and C's candidate commits reach `origin`
with the tag that names them (tried: a clone and a fetch of the tag alone both get a commit no
branch holds).

### What stands of the tasks in flight

| | A | B | C | D |
|---|---|---|---|---|
| `tools/nightly-reds-take-the-express-path`, its first part (the red's priority, mark, slot, PULL and text), in review at `5f67069b` | stands | stands | stands | stands |
| the same, its second part: a fix's land starts another run of main's tip | replaced by the rerun | replaced by the rerun | replaced by the rerun | stands |
| `tools/verify-runs-every-step`, in progress: every step runs, each red named | stands, and lands before the rename | stands | stands | stands |
| `perf/nightly-stamps-unchanged-checks`: the run split into steps, each recorded | stands, and is what reruns only the failures | stands | stands | stands |
| the same: keys and reuse | stands: the keys compare the candidate's tip with its first commit | stands | stands | stands, and skips less on a busy day |
| the same: its card on how old a green may be | stands; a green carried to a rerun is at most the candidate's age | stands | stands | stands |
| `tools/nightly-and-regenerator-edges`, all seven | stand: items 1 and 2 are the first run's wait on main's tip | stand | stand | stand |
| the consuming project's `tests/nightly-red-conciliate-ID`: its calls put back | stands | stands | stands | stands |

- [r] **release-candidate-process** — The full run judges main's tip, and main gains 2 to 8 landings an hour. So a red's fix reaches the next run with everything that landed since, and that run can be red again: a release may never come. Your words, 2026-11-12: "release candidate branch process? bugfix - rerun failures then release once green. Then update main with bugfixed changes. Open to suggestions on how". How is a red fixed and the release cut? A: the run holds the commit it judged; each fix starts there, never merges main, and lands on main as any task does; the fix's tip, on main from then on, becomes the candidate, and a rerun of what was red releases it. B: the fix lands on main first, and the run copies it onto the candidate (a cherry-pick). C: the fix lands on the candidate only, and the candidate is merged into main after the release. D: no candidate; each run judges main's newest tip, reusing the green of each step whose inputs did not change. Recommended: A. Approve and say A, B, C or D in the note, or comment. [[analysis/release-candidate#The options]]
  [2026-11-12 18:45] agent — Base: today, each run judges main's newest tip. ![Today: each run judges main's newest tip](release-candidate/today.svg)
  [2026-11-12 18:45] agent — Posted for the board task `design/release-candidate-branch`. A, the recommendation, drawn: ![A: the fix's own tip is the release candidate](release-candidate/proposed.svg)

    **Tonight's first run**, 17:28 to 18:27, was red twice. `just verify` stopped at its size check, two binaries past their budgets, so none of its tests ran. And the consuming project no longer builds against it. Main gained 3 landings meanwhile. Under every option, "rerun failures" first needs the run split into steps that each run on their own: the first part of the stamps task.

    **A, the fix's own tip.** Main has each fix from its land, nothing is merged back, and the tree released is the tree a reviewer approved. `just land` does not change, and neither does what a release is. Cost: a fix is written on a tree up to a day old, so it conflicts with main more often; a conflict drops the candidate, and the next run judges main's tip. A second red's fix merges the first before its review. A fix that changes the client commits its own build of it.

    **B, picked onto the candidate.** Cost: the pick conflicted in two of three trials. The release is a commit nobody reviewed and main lacks, so what a release is must change, and the run builds and commits the candidate's client itself.

    **C, merged back after the release.** Your words read literally. Cost: main lacks every fix until the release, so every agent on main meets the same red. The release is a commit main lacks until the merge back, which can conflict after the release is cut (tried).

    **D, no candidate.** What is being built now. Cost: today 38 of 56 landings changed the board's or the symphony crate, so the run after a fix reruns nearly everything, and each landing since can bring a red of its own. A release comes only when main is quiet.

    **Why A.** It is the one option in which the tree judged changes only by the fixes, so a release comes however fast main moves. It updates main earlier than your "Then", at each fix's land. It works before the stamps land, and is faster with them. A red that passes when run alone is fixed as you said, rewritten or taken out of the run, never rerun until it passes.

    **Names, unless your note says otherwise.** `just release` runs it, as you proposed; today's `just nightly` answers one line pointing to it until the first release it cuts. The commit it holds is the release candidate, and a red there is a candidate red, filed as `tests/release-red-NAME`. `--release COMMIT` becomes `--commit COMMIT`. Everything moves in one step, the first task, as you chose for the last rename ("B - just remove the bandaid quickly"). The whole vocabulary: [[analysis/release-candidate#The vocabulary]]

    Approve and say A, B, C or D in the note, or comment. [[analysis/release-candidate#Why A]]
  [2026-11-12 19:05] owner · approved — A
  [2026-11-12 19:22] agent · question — A it is. Its review on the board task found four things to correct in what you approved, and two trades the card did not state. The design now says each, and the three choices they leave open are cards of their own: **release-main-while-candidate-open**, **release-candidate-name** and **consumer-api-break-release**.

    **Corrected:**
    - **Only a red's own fix moves the candidate.** As written, any task started at the candidate's commit would have moved it, and main sits at that commit for minutes (tried). It now moves only on the land of a red's own task.
    - **A conflict no longer drops the candidate.** When a fix's land conflicts, its executor merges main and asks for a fresh review, as for any task. The commit approved before that merge is on main once the fix lands, and holds only the candidate and the fix, so it becomes the candidate (tried).
    - **The refresh moves a pin only forward.** A pin moved by hand to a commit of main while a candidate is open is neither before nor after the release, and moving it would drop what it had, a newer board schema among them (tried). Such a pin is left and named until a release descends from it.
    - **B's "two of three trials" was not a rate:** the same fix conflicts the same way under A (tried); B's other costs stand. And D's release comes when a run of main's tip finds no new red, not only when main is quiet.

    **The two trades A makes:**
    - **Main's tip gets no full run while a candidate is open**, up to the 24-hour bound. A break landed meanwhile is found by the first run after the release, and the release lacks what landed since the candidate. The regenerator still checks main each round. Whether main gets a run too: **release-main-while-candidate-open**.
    - **Once stamps land, the commit tagged is not run in full.** The steps that were red, did not run or that a fix reaches run on it; the rest keep their green from the candidate's first commit, whose inputs for them are the same. That is your "Nightly can also use the stamp based approach for things that did not change so it's faster", and it holds under every option, D included. Before stamps, the rerun is the whole verification.

    **Your words the card did not quote**, 2026-11-12:
    - "also nightly fixes should be prioritized, like bugfixes are since they block releases. Meaning express path line followed up by another run. Nightly can also use the stamp based approach for things that did not change so it's faster": the express path stands for a red's fix, and the candidate's rerun is the run that follows it. The express path's own "a fix's land starts another run of main's tip" is dropped.
    - "we can also move from calling this "nightly" to release candidate" and "just release ? (would do tests and release, if fails then won't do release and agents has to fix?)": the card's names gave "release candidate" to the commit held, not to the run, without saying so: **release-candidate-name**.

    Tonight's first run also showed that a deliberate break of a consumer's API has no way to a release: **consumer-api-break-release**.

    Since these change what you approved: do the corrections stand? Approve again to keep A with them, or comment. [[analysis/release-candidate#The trades]]
  [2026-11-12 19:38] agent · question — One more change under your A, the same correction to D: the Base picture's last label, "and so on, until main is quiet", now reads "and so on, until a run finds no new red". Approve again to keep A with the corrections, or comment.
  [2026-11-12 20:35] agent — Your A stands, with the corrections and the two trades above: they follow from it and are not new choices, so they are auto-approved as you say, and both questions above are withdrawn. Nothing on this card waits on you. Your words, 2026-11-12: "release-candidate-proccess is weird. the card was approved, then another question to decide came, but the decision remains in approved? this should had been a seaparate card no the same one. I also don't really get why those similar questions about things that were already been approved are being asked. They are consequences of the previous decision and should be auto-approved"
  [2026-11-12 20:35] agent · resolved

## Why A

- It is the only option in which the tree judged changes only by the fixes, so a release comes
  however fast main moves, while each red's fix has an approve on a commit from before any merge of
  main; and the tree released is a tree a reviewer approved.
- Main never waits for the release, and nothing is merged back: main gets the fixes at each
  fix's land, earlier than the ask's after the release, which left how open.
- No rule about what a release is changes, and `just land` does not change. The refresh gains the
  one rule any candidate needs: a pin moves only forward.
- It works before the stamps land (the rerun is then the whole verification of the tip) and gets
  faster with them.

Its costs are fixes written on an older tree, which conflict more often and then cost a merge and a
fresh review, and main's tip going without a full run while a candidate is open. The 24-hour bound
caps both.

## A deliberate break of a consumer's API

Today it has no way to a release, under any option. The conciliation builds each consumer's
default branch as committed against the candidate, and the refresh moves its pin in one commit
that may change only `Cargo.toml` and `Cargo.lock`. So a change here that removes or reshapes a
public item a consumer uses keeps the conciliation red until the consumer's code changes, and that
code cannot change on its default branch until its pin moves. Tonight's two breaks were accidents,
and are being put back; a deliberate one needs a way. The owner's, on a card:

- **A, two releases.** The change adds the new item and keeps the old one working beside it, with
  no `#[deprecated]` mark: the consuming project's `just lint-rs` runs clippy with `-D warnings`
  (its justfile and workspace lints), so the mark would turn its own lint red once its pin moves.
  The release carries both; the refresh moves the pin as today; the consumer moves to the new item
  through a task of its own. A later task here removes the old item once no consumer's default
  branch calls it, which that run's conciliation proves. Nothing in the run or the refresh
  changes, and no consumer's main breaks. The cost: the old item is kept for a release or more, and
  the break takes two tasks here and one in each consumer.
- **B, one release, the consumer's move riding with it.** The consumer prepares its side on a
  branch of its own (say `symphony/next`), which the conciliation builds in place of its default
  branch when it exists. Green, the release is cut, and the refresh makes the pin's move on that
  branch; holding more than Cargo files, it cannot fast-forward the consumer's main, so its owner
  lands it through the consumer's own landing, as a refused refresh is landed today. The run, the
  conciliation and the refresh each change, and the consumer's main waits on a person.

Recommended: A. Either way, a test here that compiles what the consumers call makes a break show at
its own task's tip, before any run, so a break is deliberate or caught.

- [r] **consumer-api-break-release** — A release reaches a consumer by a pin move that may change only `Cargo.toml` and `Cargo.lock`, and is cut only when the consumer's default branch, as committed, builds and tests against it. So a change here that removes or reshapes a public item a consumer uses can never be released: the consumer's code cannot change until its pin moves. Tonight's first run found one such break in the consuming project, and its fix found a second behind it; both were accidents, now being put back. How is a deliberate one released? A: in two releases: the change keeps the old item working beside the new (no `#[deprecated]` mark, which the consuming project's lint would turn red), the consumer moves over through a task of its own, and a later task here removes the old item, which that run's conciliation proves. B: in one: the consumer prepares its side on a branch the conciliation builds in place of its default branch, and after the release its owner lands the pin's move with it through the consumer's own landing. Recommended: A: nothing in the run or the refresh changes, and no consumer's main breaks or waits on a person; its cost is the old item kept for a release or more. Approve and say A or B in the note, or comment. [[analysis/release-candidate#A deliberate break of a consumer's API]]
  [2026-11-12 19:22] agent — Posted for the board task `design/release-candidate-branch`, from the review of the release candidate design you approved.
  [2026-11-12 20:26] owner · approved — I don't fully understand this issue. So pick whatever choice makes sense to you.
  [2026-11-12 21:14] agent — Chosen as A by the orchestrator under your words, "pick whatever choice makes sense to you", and filed as the board task `tools/consumer-api-break-in-two-releases`. In plain words: when Symphony changes something a project's code uses, the old version keeps working until the release after its replacement is out, so a project is never broken by taking a release.
  [2026-11-12 21:15] agent · resolved

## The names

### The vocabulary

The command is `just release`, as the owner proposed: it verifies and releases only on green, and
on red releases nothing and files the reds for agents to fix. "Release candidate" is the commit it
holds while they are fixed.

The ask to call this the release candidate rather than the nightly, read literally, names the
run. This design gives the words to the commit the run holds, and calls the run a release
run, which the command already names. Which one they name is the owner's, on a card; the table is
the default, and every other name in it stands unless the owner says otherwise.

| today | proposed |
|---|---|
| `just nightly`, `symphony nightly` | `just release`, `symphony release` |
| a night, a nightly run | a release run: one run of `just release`, first or rerun |
| the candidate (main's tip a night judges) | the release candidate: the commit a run judged, held while its reds are fixed |
| none | a candidate rerun: a release run of the candidate's tip after its fixes landed |
| a night's red | a candidate red: a check red on the candidate |
| `tests/nightly-red-NAME`, `-run`, `-refresh-ID` | `tests/release-red-NAME`, `tests/release-red-run`, `tests/release-red-refresh-ID` |
| `--release COMMIT` | `--commit COMMIT`, out of cycle |
| none | `--drop`, the candidate dropped; `--rerun`, the candidate rerun now |
| `--due`, `--refresh`, `--status`, `--schedule`, `--at`, `--no-push`, `--timeout`, `--worktree`, `--check`, `--remote`, `--branch` | the same |
| output and status lines `nightly: …`, "the last night", "overdue" past 36 hours | `release: …`, "the last run", the same 36 hours |
| `crates/symphony/src/nightly.rs` and `nightly/`; `crates/symphony/tests/nightly/` | `crates/symphony/src/release.rs` and `release/`; `crates/symphony/tests/release/` |
| `crates/symphony/src/cli/commands/release.rs` (the client's release inputs) | `release_inputs.rs`, so one module is called release |
| `../symphony-wt/nightly`, `nightly-conciliate/ID`, `nightly-refresh/ID` | `../symphony-wt/release`, `release-conciliate/ID`, `release-refresh/ID` |
| their lock reasons and marks: "the Symphony nightly's checkout", `symphony-nightly`, `symphony-nightly-copy` | "the Symphony release run's checkout", `symphony-release`, `symphony-release-copy` |
| the store `.git/symphony/nightly/` | `.git/symphony/release/`, with the candidate's record |
| a run record's `kind` `nightly`; the recorder's label `nightly NAME`; the Runs pipeline and trigger `nightly` | `release`; `release NAME`; `release` |
| the schedule `symphony.nightly.ID` | `symphony.release.ID` |
| `SYMPHONY_NIGHTLY_NOW`, `SYMPHONY_NIGHTLY_RELOAD_SECONDS`, `SYMPHONY_NIGHTLY_DRIVER` | `SYMPHONY_RELEASE_…` |
| `[board] on_land = [".target/bin/symphony", "nightly", "--due"]` | `[…, "release", "--due"]` |
| RELEASES' and CHECKS' "The nightly", and "the nightly's" across AGENTS, RULES, the skills and the docs | "The release run" |

Kept as they are: the tags `release-YYYY-MM-DD`, the `[releases]` table, "out of cycle", the
cadence, the refresh and the conciliation. `just release` is free today: the justfile has no
`release` recipe and `symphony` no `release` command. The word is taken by `symphony
release-inputs` (the files the client's release build reads), which keeps its name, by Cargo's
`--release` profile, and by `--release COMMIT`, which becomes `--commit`.

- [r] **release-candidate-name** — Your words, 2026-11-12: "we can also move from calling this "nightly" to release candidate", then "just release ? (would do tests and release, if fails then won't do release and agents has to fix?)". Read literally, "release candidate" names the run. The design you approved gives it to the commit the run holds while its reds are fixed, and calls the run a release run, which `just release` starts; its card listed those names without pointing out the difference. What does "release candidate" name? A: the commit held: a candidate red, a candidate rerun, and the run is a release run. B: the run: a release candidate run, its reds release candidate reds, and the commit held is just the candidate. Either way the rest stands unless you comment: `just release`; `just nightly` answering one line pointing to it until the first release it cuts; `--commit COMMIT` for `--release COMMIT`; reds filed as `tests/release-red-NAME`. Recommended: A, since a release candidate is the build that may become the release, and the command already names the run. Approve and say A or B in the note, or comment. [[analysis/release-candidate#The vocabulary]]
  [2026-11-12 19:22] agent — Posted for the board task `design/release-candidate-branch`, from the review of the release candidate design you approved.
  [2026-11-12 20:25] owner · approved — A. should also had been auto approved.
  [2026-11-12 21:14] agent — A it is, and you are right that it should not have been asked: the process card you approved already called the commit the run holds the release candidate. Recorded for the board task `tools/release-rename`: "release candidate" names that held commit, a red there is a candidate red and its rerun a candidate rerun, and the run itself is a release run, started by `just release`.
  [2026-11-12 21:15] agent · resolved

### What keeps working during the change

Everything moves at once, as for the last rename, of the consuming project's names: quickly,
whatever breaks fixed as soon as possible. Five
things need care:

- **The old recipe.** `just nightly` answers one line, "`just nightly` is now `just release`", and
  exits 2, running nothing, until the first release `just release` cuts; then it goes.
  Recommended, since briefs and open reds on the board say `just nightly`. Its cost: the word stays
  in the justfile for a while.
- **The land's `on_land`.** It names the build the main checkout links, which knows no `release`
  until it is linked again (`tools/rs-bin.sh --link symphony` in the main checkout, the lander's
  next step). Until then a land shows that build's refusal and starts no run; the count is read
  from git each time, so no landing is lost.
- **Open reds.** The board's red series keeps `tests/nightly-red-` beside `tests/release-red-`
  while a red of the old name is open, and the run reads both, so the next green still closes it.
  The old prefix goes once none is open.
- **What is on disk.** The first run of the new build, holding the old store's lock, renames
  `.git/symphony/nightly/` to `.git/symphony/release/`, and moves its checkout: unlock, `git
  worktree move`, lock with the new reason, marked again. Tried: git refuses to move a locked
  worktree, and after an unlock the move keeps its untracked `.target/`, so no build is lost. Not
  tried: the lock held across the store's rename.
- **Records.** Run records and the Runs page's day files written before keep `nightly`, and the
  readers take it as `release`. No schedule is installed, so none moves.

### The order of the rename and the tasks in flight

Tried on copies of `nightly.rs` and `nightly/board.rs`: after a rename of the files and of the word,
a branch's edit of a line naming the word conflicts, and every other edit merges, git following
the files to their new names. The tasks in flight edit those lines, the express path's red slugs
and the verify steps' red filing above all. The rename is mechanical, so it is cheaper to make
again on a newer main than to merge. So:

1. `tools/nightly-reds-take-the-express-path`, its first part, in review at `5f67069b`: lands
   under the old names.
2. `tools/verify-runs-every-step`, in progress: lands under the old names. It edits `_verify` and
   the red filing in `nightly*.rs`, the files the rename moves.
3. **The refresh moving pins only forward**, any time before the candidate: it touches
   `nightly/refresh.rs` alone.
4. **The rename**, once the names card is answered, in one step while nothing else edits those
   files.
5. The candidate (A) and `tools/nightly-and-regenerator-edges`, side by side: the edges are the
   first run's wait on main and the regenerator; the candidate is the run's start, its reds and the
   cadence.
6. `perf/nightly-stamps-unchanged-checks` after the candidate, since both change how the run's
   steps are run and recorded.

The two other cards' work follows their answers: main while a candidate is open is a line in the
candidate's cadence under A or B, a second run under C; a deliberate consumer-API break is a rule
in CHECKS and AGENTS under A, and changes to the conciliation and the refresh under B.

Their slugs keep the old word; their code uses the names on main when they merge.

## The implementation tasks

The first three are built (`tools/refresh-moves-pins-forward`, `tools/release-rename`,
`tools/release-candidate-held`); none other is filed. The rename keeps `on_land` naming `nightly --due`, which the new build runs as `release
--due`, since a build older than it refuses `release` ([RELEASES](../RELEASES.md#the-rename)). In
the order above:

1. **The refresh moves a pin only forward.** A consumer whose pin neither holds the release nor is
   held by it is left, named in the step's output, its record and `--status`, and moved by the
   next release that descends from it. Evidence: `-p symphony --test nightly` refresh cases on
   scratch consumers (a pin behind, at, ahead of and beside the release).
2. **The rename** to `just release`, as the vocabulary says once its card is answered, with what
   keeps working. Evidence: `-p symphony --test release` (renamed), the board's red tests,
   `named_recipes`, the hooks' commands, docs checks.
3. **The candidate held** (A): the record and the local branch; the red task's text (start at the
   candidate, merge it and not main before the approve, commit the client when its inputs
   changed, what a conflict does); the advance on `--due`, only on a red's own task, taking the
   newest approved commit that passes, once `origin/main` holds the land; the rerun (each red or
   unrun step, each step whose inputs changed, the conciliation; the whole verification while
   there are no steps); the bounds as `[releases]` settings; the cadence's hold; the release notes
   from the record; `--status`, `--drop`, `--rerun`; RELEASES, TASKS, CHECKS, AGENTS and the
   skills. Tests on scratch repositories and boards. It replaces the express path's second part,
   which is cancelled.
4. **The run split into steps and stamped:** the stamps task as filed, its keys comparing a
   candidate's tip with its first commit.
5. **The edges** as filed, beside 3.
6. **What the two other cards decide**, once answered.

## Tried on a scratch repository

Three scripts ran with git 2.54.0 in a scratch folder, each on repositories of its own; nothing was
pushed anywhere, and a fetch stands in for a push where a tag carries commits.

- **The first** made a main line with a candidate commit C and a later landing, then each option's
  fix: A's branch started at C and merged into main with `--no-ff`, keeping main's side of a
  generated folder as the land does; a second fix merging the first; one merging main; the
  orchestrator's candidate branch; B's cherry-picks; C's merge back after main changed the same
  line; a tag on a commit no branch holds, then cloned and fetched; and the rename's move of a
  locked worktree.
- **The second** copied this checkout's `nightly.rs` and `nightly/board.rs`, edited them on a
  branch, renamed them and the word on main, and merged main into the branch.
- **The third** made the cases A's review raised: a task forked at the candidate landing before the
  red's fix; a fix whose land conflicts, merged with main and landed, and the same fix as B's pick;
  a pin moved to main while the candidate is open; a chained candidate's release notes; and two
  fixes merged into a candidate branch with `--no-ff`.

The first two's output, whole:

```text
== A. The fix's branch starts at the candidate and lands on main by an ordinary --no-ff merge
Automatic merge went well; stopped before committing as requested
the fix's tip is on main (an ancestor of its tip): yes
the fix's tip's tree is still the tree reviewed: yes
the fix's tip holds the landing it was not based on ('other'): no
main's generated file after the land: gen-of-C; the fix's tip's: gen-of-fix1
main's second parent of the land merge is the fix's tip: yes
a later main descends from the release (it only moves forward): yes
landings the cadence counts after the release (first-parent merges release-1..main):
   Merge branch 'fix1'
   Merge branch 'other'
the release notes' first-parent commits release-0..release-1:
   fix1
   C

== A, two reds: the second fix merges the candidate's tip (the first fix) before review
without that merge, the second fix holds the first: no
after it: holds the first: yes
commits it holds that the candidate's tip does not (rev-list F2M ^F1):
   Merge commit '8d219e86c094129c2a0b383f91c6ed986894143d' into fix2
   fix2
does it hold main's later landing 'other'?: no
its land on main: clean
on main: yes; a.txt line 2 on main: a2-fixed; c.txt on main: c1 c2-fixed
the second fix: beyond the candidate 2, of which main lacked before its land 2: advances: yes
a fix that merged main: beyond the candidate 5, of which main lacked 2: advances: no

== The orchestrator's two merges: a candidate branch at C, the fix merged into it with --no-ff as into main
the candidate's tip is on main, so main's tip descends from a release tagged there: no
merging the candidate back into main changes main's tree: no
merged by fast-forward instead, the candidate's tip is the fix's tip itself: yes

== B. The fix made on main first, then cherry-picked onto the candidate
cherry-pick onto C of a fix to a file untouched since C: clean
the picked commit is the reviewed commit: no
the picked commit is on main: no
merging it back into main changes main's tree: no
cherry-pick of a fix next to a line main changed after C: CONFLICT
cherry-pick of a fix to a line main added after C: CONFLICT

== C. The fix on the candidate only, merged back after the release
main holds the fix meanwhile: no
the merge back, after main changed the same line: CONFLICT

== A tag alone carries its commit: a fresh clone and a fetch of the tag get a commit no branch holds
on any branch here: no
a plain clone has it: yes
a fetch of that one tag has it: yes

== The rename: a locked worktree with a build folder moved to a new name
move while locked: refused
after unlock, move, lock: worktree wt/release locked the Symphony release run's checkout (docs/RELEASES.md)
its untracked build folder came with it: built
git version: git version 2.54.0 (Apple Git-157)

== The rename merged under a branch's edits (copies of nightly.rs and nightly/board.rs)
lines the rename changed: 81 in src/{nightly.rs => release.rs}
lines the rename changed: 75 in src/{nightly => release}/board.rs
the task merging main: CONFLICT in src/release.rs src/release/board.rs
files after the merge: src/release.rs src/release/board.rs
the task's added line kept: 1
conflict markers in release.rs: 1
```

The B cases above were built to touch lines main changed, so their two conflicts in three are not
a rate; the third script shows the same fix conflicting under A too. The third's output, whole:

```text
== The advance read from git alone lets in any task forked at the candidate
feature-u, forked at C, lands: clean
the git rule takes feature-u as the candidate: yes
the red's fix, from C, lands: clean
it then advances a candidate moved to feature-u: no
read only on the land of the red's own task, the candidate is still C, and the fix advances it: yes

== A fix whose land conflicts: the tip approved before it merged main
a landing after C changes the line the fix will: clean
the approved fix's land: CONFLICT
after it merged main, its land: clean
the approved tip F is on main: yes
F passes the advance: yes
F's tree is C's and the fix's alone (files C..F): a.txt
the merged tip F' passes the advance: no
the same fix made on main and picked onto C (B): CONFLICT

== The refresh: a pin moved by hand to a commit of main while the candidate is open
the pin P lacks the release F, and F lacks P
the refresh's own test, the release an ancestor of the pin (takes it already): no
a forward move, the pin an ancestor of the release: no
what a move from P to F would drop (rev-list P ^F):
   Merge branch 'meanwhile'
   meanwhile
schema.txt at P: s1 s2; at F: s1
a pin moved to main after the fix's land takes F already: yes

== The release notes of a chained candidate
first-parent subjects release-0..F2, as the notes read them:
   Merge branch 'tests/release-red-a' into tests/release-red-b
   the second red's fix
every commit's subject release-0..F2:
   Merge branch 'tests/release-red-a' into tests/release-red-b
   the second red's fix
   the first red's fix

== Parallel fixes merged into a candidate branch with --no-ff, the other shape of the two merges
f2 holds f1 (a chain, a fresh approve): no
the candidate's tip is a tip a reviewer approved: no
f1 lands on main: clean; f2 lands on main: clean
the candidate's tip is on main: no
merging the candidate back into main changes main's tree: no
```

## Verified and inferred

- **Read in code**, at `bc43c169` and again at `79915265`: the run's candidate, its `--release`
  check against the fetched `origin/main` and its tag (`nightly.rs`), the cadence's count and
  `since` (`nightly/cadence.rs`), the release notes' first-parent walk (`nightly/notes.rs`), the
  refresh's test that a consumer already takes the release (`nightly/refresh.rs`), how reds are
  filed and closed (`nightly/board.rs`), the red series (`crates/tasks/src/red.rs`), the sweep's
  lock name (`crates/tasks/src/sweep/holder.rs`), the land and its generated paths (TASKS), the
  refresh's Cargo-only commit (RELEASES), the justfile and `symphony.toml`; and, read only, the
  consuming project's `just lint-rs` (clippy with `-D warnings`).
- **Measured:** the first run's record and logs (`.git/symphony/nightly/`); landings per hour and
  the packages each changed (`git log --first-parent --merges`, `git diff --name-only`) on
  `origin/main` at `c70a7515`; the packages' local dependencies (their `Cargo.toml`); that no
  schedule is installed (`~/Library/LaunchAgents`, `crontab -l`); the trials above.
- **Read on the board:** the first run's two reds and their causes, on the conciliation red's
  thread; the express path's state; `tools/verify-runs-every-step`'s brief.
- **Inferred:** that between two runs a few landings apart nearly every step's inputs change, from
  the share of landings in the board's and the symphony crates and their dependents; that A's
  fixes conflict with main more often than branches started at main, from their older base, not
  measured; how long a whole verify and a rerun take, which is measured once every step runs.
- **Reported, not read:** the compile test of what the consumers call, added with tonight's
  conciliation fix, as that task's orchestrator reports.
