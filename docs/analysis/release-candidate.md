# A release candidate held still

How a red found by the full run gets fixed so that a release comes, and what the run and its
command are called. This is a design: nothing here is built. The choice is the owner's, on one card
in [DECISIONS](../DECISIONS.md).

The owner, 2026-11-12, in this order:

- "release candidate branch process? bugfix - rerun failures then release once green. Then update
  main with bugfixed changes. Open to suggestions on how"
- "nightly fixes should be prioritized, like bugfixes are since they block releases. Meaning express
  path line followed up by another run. Nightly can also use the stamp based approach for things
  that did not change so it's faster"
- "A test that fail when machine is under load should be rewritten to account for cpu wall clock
  time usage - not total time spent or be removed from nightly"
- “we can also move from calling this "nightly" to release candidate”
- "just release ? (would do tests and release, if fails then won't do release and agents has to
  fix?)"

The first and the last are proposals, not rulings: the owner asked for suggestions on how, and put
the command's name as a question. They are designed around here, and decided on the card.

- **The problem.** A run judges main's tip, and main gains several landings an hour. A red's fix
  lands on main with everything else that landed meanwhile, so the next run judges a different
  tree, and what those landings brought can be red in turn. A release may never come.
- **Recommended: A, the fix's own tip is the candidate.** The run holds the commit it judged. A
  red's fix is a branch started at that commit, never merging main, and lands on main by the
  ordinary `just land`. Its tip, the tree its reviewer approved, is then on main, and becomes the
  candidate. A rerun of the steps that were red, and of those whose inputs the fix changed,
  releases it. Main has every fix from its land; nothing is merged back; what a release is does not
  change. This is the orchestrator's first suggestion with one merge instead of two.
- **The command is `just release`**, the commit it holds while its reds are fixed is the release
  candidate, and a red there is a candidate red, filed as `tests/release-red-NAME`
  ([the vocabulary](#the-vocabulary)). The rename is the first implementation task, in one step,
  everything at once.

## Today

### What a run judges

`just nightly` (`crates/symphony/src/nightly.rs`, [RELEASES](../RELEASES.md#the-nightly)) fetches
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
- **`conciliate-star-foundry`, after 154 s.** Star Foundry's `gate-review` no longer compiles
  against the candidate: it calls `cli::board_client`, which the board's library no longer has.
  Its fix may be in Star Foundry rather than here.

Filed: `tests/nightly-red-verify` and `tests/nightly-red-conciliate-star-foundry`. In the hour the
run took, main gained 3 landings.

Two things follow under every option:

- **"Rerun failures" needs the run split into steps** that each run whatever an earlier one did.
  Today the failure is all of `just verify`, tests that never ran included. The split is the first
  part of the stamps task.
- **A red can be fixed outside this repository.** A conciliation red fixed in the consuming
  project changes no commit here; the next run takes the project's newer default branch.

## The options

Each option says how long a release takes after a red, what main and a consuming project's pin
hold meanwhile, what `just land`, the regenerator, the cadence, `--status` and the refresh must
change, and how it meets the owner's earlier rulings ([below](#the-rulings)). The git behaviour
each relies on was tried on a scratch repository ([the trials](#tried-on-a-scratch-repository)).

### A. The fix's own tip is the candidate (recommended)

1. **The candidate.** A run red in any check holds the commit it judged: a record in the run's
   store (the commit, when it was first judged, the round, each red with its task) and a local
   branch `release-candidate` at that commit, never pushed, which only the run moves. One
   candidate at a time.
2. **A red's fix** starts at `release-candidate` and never merges main. Before review it merges
   `release-candidate` again, which moves when another red's fix lands, as any branch merges main
   today. Its evidence is the red check run alone at its tip. It lands with `just land`, as any
   task: one `--no-ff` merge into main.
3. **The advance.** The land's `on_land` (today `nightly --due SLUG MERGE`) reads the merge's
   second parent, the tip the reviewer approved. When that tip descends from the candidate and
   every commit it holds beyond the candidate is the branch's own (none that main held before the
   land), the candidate moves to it. Tried: a tip that merged the candidate advances; one that
   merged main does not.
4. **The rerun.** A red is answered by a fix that advanced the candidate, or by its task closed
   with its fix made outside this repository (a consuming project's, for a conciliation red). The
   land that answers the last red starts the rerun of the candidate's tip through its `--due`; a
   red answered outside this repository is picked up by the next landing's `--due`, or by `just
   release --rerun`. That would be an orchestrator's retry, as `--refresh` is, once the owner
   approves this design: it reruns failures and releases once green, which is the owner's own
   proposal. The rerun runs:
   - each step that was red or did not run;
   - each step whose inputs the fixes changed (the stamps' key, comparing the tip with the commit
     first judged);
   - the conciliation.

   Every other step keeps the green it had on the first commit. Green: the tip is tagged, the tag
   pushed, the machine refreshed. Red: a new round, its reds filed as before.

   A red that passes when run alone is load- or order-dependent (RULES
   `tests/rerun-the-failure-alone`). By the owner's rule its fix is the test "rewritten to account
   for cpu wall clock time usage - not total time spent", or "removed from nightly", so it is
   answered by a fix like any other, never by a rerun that happens to pass.
5. **Bounds.** The candidate is dropped, and the next run judges main's tip at once, when:
   - three reruns came back red;
   - it is 24 hours old;
   - a red's fix landed without advancing it: it merged main, as a fix whose land conflicted with
     main must;
   - or a release on main passed it.

   The two numbers are the run's thresholds, `[releases]` settings, not the owner's to set.

What it changes:

- **`just land`:** nothing. The advance is read after the land, by `on_land`.
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
  the fix's own included (tried).
- **`--status`:** a candidate block: its tip and first commit, its age and round, each red with its
  task and state, and what the rerun waits for.
- **The refresh:** nothing.
- **Elsewhere:** CHECKS' rule to merge main again before review, and AGENTS' "a branch commits no
  distribution", each get the candidate fix's exception; the red task's text says both.

**How long after a red:** the fixes, then one rerun of the red steps and of those the fixes reach.
Main's pace does not enter: the tree judged changes only by the fixes, so a new red can come only
from a fix itself. Without stamps the rerun is the whole verification of the tip, and still
converges.

**Meanwhile:** main has each fix from its land. A project's pin keeps the release before; one moved
by hand to a commit of main gets each fix with main.

**Costs:**

- A fix is written on a tree hours old and lands on a newer main, so it can conflict there more
  often than a branch started at main. A conflict drops the candidate, and the fix lands as any
  branch does.
- Two reds' fixes are chained: the second merges the first before its review, and needs a fresh
  approve if the first lands during its review.
- Agents must base these branches on the candidate, not main: the red task says so, and a fix
  that merges main anyway costs only the candidate, never a wrong release.
- The release is a commit main holds as a merge's second parent, not on main's first-parent line.
  Today's rules read "on `origin/main`" as an ancestor of its tip (`is_ancestor`), so nothing
  refuses it.

### B. The fix on main first, then picked onto the candidate

The fix is an ordinary task, based on main, and lands there. The run then copies its commits onto
the candidate (a cherry-pick) and reruns.

- **How long after a red:** as A, when the pick applies. Tried, it conflicted in two of three
  cases: a fix to a line main added since the candidate, and a fix next to a line main changed. A
  conflict drops the candidate.
- **Meanwhile:** as A.
- **The release is a commit nobody reviewed and main lacks:** the pick (tried). So the rules that a
  release is on `origin/main` and only moves forward must change, or the run merges the candidate
  back into main (an empty merge, tried) and pushes main, where today it pushes only its tag.
- **The regenerator:** the fix landed with no distribution, so the run builds the candidate's and
  commits it there, another commit nobody reviewed.
- **The cadence** counts from a release on main's line (`since`, by `is_ancestor`); a release main
  lacks is passed over, so the count needs changing too.
- **`just land`:** nothing. **`--status`:** as A, plus each pick. **The refresh:** nothing.

### C. The fix on the candidate only, merged into main after the release

The owner's words read literally: "bugfix - rerun failures then release once green. Then update
main with bugfixed changes". The candidate is a task's branch started at the commit judged; each
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

What is being built now: a red's fix takes the express path and its land starts another run of
main's tip (`tools/nightly-reds-take-the-express-path`), and a run reuses the green of each step
whose inputs did not change (`perf/nightly-stamps-unchanged-checks`).

- **How long after a red:** the fix, then a run of main's tip: every step whose inputs any landing
  since the red changed. On a day like 2026-11-12 that is close to a full run, and each of those
  landings can bring a red of its own, so the wait starts again. It converges when main is quiet.
- **Meanwhile:** main has the fix at once; pins keep the release before.
- **Changes:** none to `just land`, the regenerator or the refresh; the cadence gets the follow-up
  trigger, and `--status` says "due: the last night's reds are fixed".

### The orchestrator's suggestion, two merges

The brief's starting point: a candidate branch, and each fix merged into it and into main in the
same act. Tried: merged into the candidate with `--no-ff`, the candidate's tip is a merge main does
not hold, so a release tagged there is off main, and main no longer descends from the newest
release until the candidate is merged back. That merge back changes nothing in main's tree, but it
is one more land, and the land would make two merges, one of them able to conflict on its own.
Merged into the candidate by fast-forward instead, the candidate's tip is the fix's tip itself:
that is A, and the land needs no change at all.

### Side by side

| | A | B | C | D |
|---|---|---|---|---|
| after a red, a release waits for | the fixes and one rerun | the fixes, the picks and one rerun | the fixes and one rerun | the fixes and a run of main's newest tip, again after each new red |
| main meanwhile | each fix from its land | each fix from its land | no fix until after the release | each fix from its land |
| the release is | the last fix's tip, on main, reviewed | a pick main lacks, unreviewed | the candidate's tip, which main lacks until merged back | main's tip |
| rules that change | none about a release | a release off main; moving forward | a release off main; moving forward | none |
| `just land` | unchanged | unchanged | unchanged | unchanged |
| the candidate's client | its fixes commit it | the run commits it | its branch commits it | the regenerator, as today |

### The rulings

| the owner's earlier ruling | A | B | C | D |
|---|---|---|---|---|
| No lock or queue on a land (2026-11-12, 06:42: "A solution like a lock or queue would directly bottleneck the pipeline which isn’t acceptable either.") | met: the land is unchanged | met | met | met |
| A release only after a full run that is green (2026-11-12, 06:42: "a nightly or more often run that then runs all tests and publishes when green is the only acceptable choice I can think of.") | met: every step is green on the tree released, by a rerun or by a stamp, which the owner allowed ("Nightly can also use the stamp based approach for things that did not change") | met | met | met |
| Releases built from full runs, urgent fixes out of cycle (2026-11-11: "New releases can be pushed for urgent bugfixes, but generally should be generated based on nightly builds that would do a full verification cycle and conciliation.") | met | met | met | met |
| Fixes to reds first (2026-11-12: "Meaning express path line followed up by another run.") | met: the express path, and the rerun is the run that follows | met | met | met, as written |

Two more are the repository's rules rather than the owner's words: nothing is force-pushed
(AGENTS: "never forces"), and only the run pushes a tag ([RELEASES](../RELEASES.md#who-runs-it)).
Every option meets both: nothing is rewritten, and B's and C's candidate commits reach `origin`
with the tag that names them (tried: a clone and a fetch of the tag alone both get a commit no
branch holds).

### What stands of the tasks in flight

| | A | B | C | D |
|---|---|---|---|---|
| `tools/nightly-reds-take-the-express-path`: the red's priority, mark, slot, PULL and text | stands | stands | stands | stands |
| the same: a fix's land starts another run of main's tip | replaced by the rerun | replaced by the rerun | replaced by the rerun | stands |
| `perf/nightly-stamps-unchanged-checks`: the run split into steps, each recorded | stands, and is what reruns only the failures | stands | stands | stands |
| the same: keys and reuse | stands: the keys compare the candidate's tip with its first commit | stands | stands | stands, and skips less on a busy day |
| the same: its card on how old a green may be | stands; a green carried to a rerun is at most the candidate's age | stands | stands | stands |
| `tools/nightly-and-regenerator-edges`, all seven | stand: items 1 and 2 are the first run's wait on main's tip | stand | stand | stand |

## Why A

- It is the only option in which the tree judged changes only by the fixes, so a release comes
  however fast main moves, and the tree released is a tree a reviewer approved.
- Main never waits for the release, and nothing is merged back: "update main with bugfixed
  changes" happens at each fix's land, earlier than the owner's "Then", which the owner left open
  ("Open to suggestions on how").
- No rule about what a release is changes, and `just land` does not change.
- It works before the stamps land (the rerun is then the whole verification of the tip) and gets
  faster with them.

Its one real cost is fixes written on an older tree. The candidate's 24-hour bound keeps that tree
at most a day old, and a fix that cannot land cleanly drops the candidate rather than holding a
release.

## The names

### The vocabulary

The command is `just release`, as the owner proposed: it verifies and releases only on green, and
on red releases nothing and files the reds for agents to fix. "Release candidate" is the commit it
holds while they are fixed.

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

### What keeps working during the change

Everything moves at once, as the owner chose for the last rename (2026-11-11, on Star Foundry's
names: "B - just remove the bandaid quickly. Break things then fix them ASAP"). Five things need
care:

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
the files to their new names. Three tasks in flight edit those lines, the express path's red slugs
above all. The rename is mechanical, so it is cheaper to make again on a newer main than to merge.
So:

1. `tools/nightly-reds-take-the-express-path`, its first part (the red's priority, mark, slot,
   PULL, lane and text), already reserved: lands under the old names.
2. **The rename**, once the card is answered, in one step while nothing else edits those files.
3. `tools/nightly-and-regenerator-edges` and the candidate (A) can go side by side: the edges are
   the first run's wait on main and the regenerator; the candidate is the run's start, its reds and
   the cadence.
4. `perf/nightly-stamps-unchanged-checks` after the candidate, since both change how the run's
   steps are run and recorded.

Their slugs keep the old word; their code uses the names on main when they merge.

## The implementation tasks

None is filed. Under A, in the order above:

1. **The rename** to `just release`, as the vocabulary says, with what keeps working. Evidence:
   `-p symphony --test release` (renamed), the board's red tests, `named_recipes`, the hooks'
   commands, docs checks.
2. **The candidate held** (A): the record and the local branch; the red task's text (start at the
   candidate, merge it and not main, commit the client when its inputs changed, what a conflict
   does); the advance on `--due`; the rerun (each red or unrun step, each step whose inputs
   changed, the conciliation; the whole verification while there are no steps); the bounds as
   `[releases]` settings; the cadence's hold; `--status`, `--drop`, `--rerun`; RELEASES, TASKS,
   CHECKS, AGENTS and the skills. Tests on scratch repositories and boards. It replaces the express
   path's second part, which is cancelled.
3. **The run split into steps and stamped:** the stamps task as filed, its keys comparing a
   candidate's tip with its first commit.
4. **The edges** as filed, beside 2.

Under B: the rename; the candidate's record and branch; the pick of each landed fix and its
conflict drop; the run's commit of the candidate's client; a release off main, and the rules on
moving forward and the cadence's count changed; then the stamps.

Under C: the rename; the candidate as a task whose branch starts at the commit judged, the reds its
slices; the rerun of its tip; a release off main and the same rule changes; the merge back as its
land, and a run of main refused until it lands; then the stamps.

Under D: the rename; the express path's second part as filed; the stamps.

## Tried on a scratch repository

Two scripts ran with git 2.54.0 in a scratch folder, each on a repository of its own; nothing was
pushed anywhere, and a fetch stands in for a push where a tag carries commits.

- **The first** made a main line with a candidate commit C and a later landing, then each option's
  fix: A's branch started at C and merged into main with `--no-ff`, keeping main's side of a
  generated folder as the land does; a second fix merging the first; one merging main; the
  orchestrator's candidate branch; B's cherry-picks; C's merge back after main changed the same
  line; a tag on a commit no branch holds, then cloned and fetched; and the rename's move of a
  locked worktree.
- **The second** copied this checkout's `nightly.rs` and `nightly/board.rs`, edited them on a
  branch, renamed them and the word on main, and merged main into the branch.

Their output, whole:

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

## Verified and inferred

- **Read in code**, at `bc43c169`: the run's candidate, its `--release` check and its tag
  (`nightly.rs`), the cadence's count and `since` (`nightly/cadence.rs`), the release notes
  (`nightly/notes.rs`), how reds are filed and closed (`nightly/board.rs`), the red series
  (`crates/tasks/src/red.rs`), the sweep's lock name (`crates/tasks/src/sweep/holder.rs`), the
  land and its generated paths (TASKS), the justfile and `symphony.toml`.
- **Measured:** the first run's record and logs (`.git/symphony/nightly/`); landings per hour and
  the packages each changed (`git log --first-parent --merges`, `git diff --name-only`) on
  `origin/main` at `c70a7515`; the packages' local dependencies (their `Cargo.toml`); that no
  schedule is installed (`~/Library/LaunchAgents`, `crontab -l`); the trials above.
- **Inferred:** that between two runs a few landings apart nearly every step's inputs change, from
  the share of landings in the board's and the symphony crates and their dependents; that A's
  fixes conflict with main more often than branches started at main, from their older base, not
  measured; how long a rerun takes, which depends on which steps a fix reaches, and is measured
  once the run is split into steps.
