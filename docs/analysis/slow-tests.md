# The slow tests, reviewed

What the slowest test binaries of the release run spend their time on, what each slow test or
group proves that no other does, and whether it is kept, made faster, merged or removed. The ask
and its words are on the thread of the board task `tests/slow-tests-reviewed`. This page changes
no test. Each batch of changes it
proposes is a board task named in [the ranking](#the-ranking), and each fact about a test names
the test, so a reader can check it.

"Measured 2026-11-18" marks this machine that day: an Apple M3 Max, 16 cores, shared with other
agents' builds and tests, at commit `284fafb70`.

## The result

The release run's tests step took 1,640 s in its last green run (2026-11-17T1950-53603). Six
binaries were reviewed: `sweep`, `perf`, `browser`, the board's `server`, `release` and
`standalone`, 1,078 tests of them.

- **Most of the time is contention, not work.** `perf` alone takes 385 s; in the release run it
  took 910 s. The one standalone compile takes 99 s alone and 478 s in the run. One sweep test
  takes 306 s alone and 988 s beside its neighbours. The run starts eight lanes at once, each with
  six test threads (`tools/cargo.sh` sets `RUST_TEST_THREADS` to 6), on 16 cores, and these tests'
  work is mostly child processes: git, `lsof`, `ps`, the `tasks` binary, SurrealDB servers, Chrome.
  A process a test no longer starts is saved twice, once for itself and once for every test beside
  it. [The lanes](#what-the-lanes-do-to-each-other) says what follows for the task that gives
  the run one scheduler.
- **Few tests are worthless.** Of 1,078 tests, 11 are proposed for removal, each with the test
  that already proves the same thing named beside it, and about 25 for a merge into another.
  The suite's slowness is not redundancy.
- **A handful of tests carry a lane.** One sweep test (988 s of a 1,180 s binary), four `perf`
  phases, one compile, and two reclaim tests (512 s of the server binary's 2,666 thread-seconds)
  are where a change of a few lines pays.
- **The proposals save about 3,300 thread-seconds of 20,000 measured**, a sixth, before what less
  contention gives back, which is not estimated. [The ranking](#the-ranking) lists them by
  seconds for their risk. They shorten the `sweep` lane from about 1,290 s to under 1,000 s, and
  `perf` and `rest` by 150 to 300 s each.
- **They do not shorten the step by themselves.** The `board` lane (1,282 s: `cli` 423 s, `store`
  322 s, `server` 285 s, `launch` 166 s, one after another) is the other long lane, and only its
  `server` part is reviewed here. [What was not reviewed](#what-was-not-reviewed) names the rest.

## How it was measured

Every workspace test binary was built once (`tools/cargo.sh test --workspace --locked --no-run`),
then the six binaries ran at once, each as the release run's lanes run it: six test threads, the
build's own binaries named to the tests (`SYMPHONY_TEST_BINARIES`, `SYMPHONY_TEST_TASKS_BINARY`)
and the client built before (`SYMPHONY_UI_DIST`). Each run went through `tools/record.sh`, as the
task's own.

A time per test is libtest's own, `--report-time`, asked of the test process (the pinned toolchain
takes it with `RUSTC_BOOTSTRAP=1` in the test binary's environment alone, set by a Cargo runner, so
nothing was built differently). It was preferred to cargo-nextest, which is installed (0.9.152)
and gives a time per test too, since nextest runs each test in a process of its own: `sweep`,
`perf`, `server`, `release` and `browser` each keep something made once per process (a template
repository, a SurrealDB server, a generated board, the client's gzips), which every test would
make again and be charged for. `standalone` keeps nothing, and was timed by nextest.

| binary | tests | wall, this run | wall, release run | thread-seconds | load, start to end |
|---|---|---|---|---|---|
| `sweep` | 377 | 1,180 s | 1,290 s | 5,828 | 45 to 29 (peak above 200) |
| `perf` | 52 | 1,179 s | 910 s | serial, see below | 45 to 28 |
| `browser` | 424 | 853 s | 726 s | 4,866 | 45 to 189 |
| `server` | 129 | 479 s | 285 s | 2,666 | 45 to 67 |
| `release` | 88 | 754 s | 416 s | 4,481 | 45 to 224 |
| `standalone` | 8 | 977 s | 478 s | 977 | 45 to 121 |

The load is the one-minute average. It was 45 when the six started, from other agents' work, and
the six raised it above 200 themselves: this machine is never quiet, and the release run meets the
same. A thread-second is one test thread busy for a second; a binary's wall is near its
thread-seconds over six.

Three runs were then made alone, at a load of 10 to 25, to see inside the heaviest tests: the two
300-worktree sweep tests, `perf` whole, and the standalone compile, each with its own progress
lines. Their figures are given where they are used.

What the figures cannot say:

- A time under load is not a test's cost. Tests that do nothing took 9 to 27 s where they ran
  beside the 300-worktree tests (`finished::a_task_not_finished_hurries_no_worktree` runs no
  sweep: 12.2 s). An estimate of a saving below is the measured time of what would go, and is
  within a factor of two.
- `browser`'s list holds 411 of its 424 tests: 13 lines were lost to Chrome's shutdown output
  written across them. None of the 13 is among the slow modules' sums.
- `perf`'s tests hold one board in turn, so a test's reported time is mostly its wait for the
  board. [Its section](#perf) gives each phase's own time from the run made alone.

The six lists, a time and a name a line, slowest first, are attached to the task
`tests/slow-tests-reviewed` on the board.

## What the lanes do to each other

`tools/test-lanes.sh` starts at most half the processors' count of lanes, eight on this machine,
and every lane is a `cargo test` with six threads. Forty-eight test threads, and the processes
each starts, share 16 cores. The three binaries measured alone ran 2.4 to 4.8 times faster than in
the release run.

`tools/nextest-runs-the-tests` gives the run one scheduler over every binary, and is the open
task that reshapes the lanes; `tools/test-lanes-split-slow-lanes`, which would have rebalanced
the hand-made lanes, was cancelled in its favour. What this review adds for it:

- A lane's length is not a property of its tests alone. A long lane split into two that run side
  by side adds load to every other; the three ratios above are what that costs, and why the
  rebalance alone would not have shortened the step by what it moved.
- One scheduler over every binary, sized to the cores, is where the contention ends. Under
  nextest the per-process fixtures named above are made once per test, not once per binary, so
  `sweep`'s template (six git commands) is cheap, and `perf`'s board (57 s alone) and the store
  tests' server are not: those binaries need a fixture that outlives the test's process, or a
  group that runs them in one.
- `perf` is the lane load hurts most, since it waits for its server to go quiet before and after
  every command it bills. It is the one to run when little else does.

No task is filed for this: the nextest task is open, and the note is on its thread.

## sweep

`crates/tasks/tests/sweep/`: 377 tests, 5,828 thread-seconds. Every test copies a template
repository and takes a database of its own on the binary's one in-memory SurrealDB server; that
costs 0.3 to 0.5 s where the machine lets it (`rules::a_sweep_with_nothing_to_do_says_so`, 0.49 s).
Nothing here sleeps: the time is git processes, about 50 for one land and 15 to 25 for each
worktree a sweep removes.

| module | tests | thread-seconds | slowest |
|---|---|---|---|
| `clean_cache` | 4 | 1,394 | 988 |
| `cached` | 9 | 890 | 305 |
| `land` and its eleven modules | 157 | 1,883 | 43 |
| `cli` | 19 | 450 | 60 |
| `build_output` | 25 | 285 | 31 |
| `finished` | 18 | 255 | 41 |
| `rules` | 85 | 204 | 24 |
| `causes` | 16 | 124 | 17 |
| `kept` | 5 | 108 | 39 |
| `clean_races`, `policy_cache` | 4 | 141 | 50 |
| `triggers`, `report`, `on_review`, `sessionless`, `common` | 35 | 94 | 8 |

### The 300-worktree tests

| test | time in the run | alone |
|---|---|---|
| `clean_cache::previously_empty_300_trees_reuse_complete_proofs_and_known_blockers_skip_whole_pass_status` | 988 s | 306 s |
| `cached::a_second_pass_over_300_unchanged_worktrees_runs_no_status` | 305 s | 26 s |

**What they prove.** That a pass over many worktrees reuses each saved verdict and runs no
`git status` it does not need, counted in git's own trace, never by a time. The first is the only
proof that 300 clean worktrees with saved empty proofs reach the removal guard with no lookup, and
that known blockers (a HEAD writer's lock, a held removal lock) skip the pass's status whole; the
second, that 300 kept worktrees are inspected once and never again while unchanged.

**Where the time goes.** In the first, two in-process sweeps took 140 s and 161 s alone, and its
four command-line passes 0.6 s each. The second sweep runs no lookup at all and still takes half a
second a worktree: for every worktree it would remove, `remove` (`crates/tasks/src/sweep.rs`) lists
every worktree twice (`git worktree list`, before and after the status) and reads the board twice,
so a pass over n removable worktrees costs n squared. In the second test nothing is removed, and
300 worktrees cost 23 s cold and 1.1 s warm.

**Disposition: make faster.** The first test holds one of the binary's six threads for 84% of its
run, and loads every test that runs beside it (`cached`, `cli`, `finished`, `kept` and the first
`land` tests, by libtest's alphabetical order). Two ways, for its task to choose between by
measuring:

- The sweep lists the worktrees once per removal, not twice, or reads one worktree's own entry
  where it reads all: the re-read after the status is what protects a tree, and it needs that
  tree's entry, the paths nested in it and the board's facts, not 300 entries parsed twice. This
  is a change to the product, which sweeps a real repository of 300 finished worktrees in five
  minutes today for the same reason. It keeps the test as it is.
- Or the two in-process passes run over fewer worktrees and the command-line passes keep 300.
  What the counts prove (`lookups` 300 then 0, `guards` 300) they prove at any number; 300 is a
  size, not a threshold any code reads. Sixty worktrees cost a twenty-fifth.

Expected: about 900 thread-seconds, and the lane's neighbours freed.

### The cache and policy loops

`cached` (the other eight tests, 585 s), `clean_cache` (the other three, 405 s), `policy_cache`
(68 s) and `clean_races` (73 s): each proves one way a saved verdict must be dropped (a changed
ignore file in each of seven places git reads one, a tracked edit of each of seven kinds, five
kinds of configuration the sweep cannot prove) by running the `tasks` binary two to four times a
case, a repository a case.

**Disposition: keep.** A case is a repository-wide git setting, so cases cannot share a
repository, and each is the only proof of its invalidation. Their times are their neighbours':
`cached::changed_ignore_policies_release_output_after_a_proven_cache_hit` (230 s) is seven cases
of three command-line sweeps over one worktree, each well under a second alone. No task is filed;
they are timed again once the 300-worktree test no longer runs beside them.

### The land tests

157 tests, 1,883 thread-seconds, 12 s each. A land is about 40 git processes, one `sh` for the
check and a dozen board writes; 136 of the tests call the land in the test's process, and the
rest run the binary because a signal, a detached sweep or the regenerator is their subject. The
fifteen slowest are each the only test of a refusal or a path, and stay.

Remove, each proved by the test named:

| test | time | proved by |
|---|---|---|
| `land::the_land_sweeps_the_tasks_finished_worktrees` | 16 s | `land::the_lands_sweep_says_in_its_log_the_evidence_set_aside_past_its_cap`: the same land and detached sweep, with a cap and an artifact more. Its one line the other lacks (`landed sim/jump on main at`) moves there. |
| `land::approval::the_same_reviewers_approve_after_its_own_changes_lands_the_task` | 12 s | `land::the_command_line_lands_and_refuses_with_one_line`, whose last steps are the same reviewer's `changes`, the move back to review, its approve and the land. |

Merge, each pair one repository in one state:

| into | folds in | the one test asserts | saves |
|---|---|---|---|
| `land::a_conflict_fixed_on_the_branch_and_approved_again_lands` | `land::a_conflict_is_refused_naming_its_files_and_never_resolved` | at its first refusal, the exact line, who is told and that no check ran; then the fix and the land | 16 s |
| `land::a_land_from_a_linked_worktree_lands_on_the_main_checkout` | `land::an_approval_naming_no_commit_lands_by_time_and_says_so_on_the_thread` | the by-time line on the thread, the main checkout at main, the merge's second parent the tip | 15 s |
| `land::a_check_named_by_a_path_runs_the_merges_own_script_and_one_missing_is_refused` | `land::the_check_runs_without_the_callers_repository_variables` | the script also writes its top level and `GIT_DIR`, the land run with both set in its environment | 8 s |
| `land::a_landing_line_names_the_tokens_its_task_took_by_role_and_round_and_their_cost` | `land::a_land_pushes_main_alone_to_origin` | a tag made before the land; the remote holds `refs/heads/main` alone, at the merge | 16 s |
| `land::signals::a_land_stopped_after_main_moved_is_finished_by_a_run_again_pushing_what_it_checked` | `land::signals::a_full_sweep_after_a_land_stopped_past_main_keeps_its_branch_and_the_land_finishes` | after the hang-up: the record, then a full sweep keeping the branch, then the finishing land | 12 s |

Make faster:

- **A landing made by hand where the land is not the subject** (50 s). Eight tests of
  `land::covered` run a whole land only to leave a merge on main and its `landed` line, which
  `land::covered::a_landing_whose_commit_main_does_not_hold_covers_nothing` already makes with one
  merge and one store call in 4 s. `land::covered::its_executor_closes_it_from_the_command_line_naming_the_merge_on_main`
  keeps the real land.
- **A rule tested where it is decided** (40 s). Seven refusals of `land::carried` (a commit beyond
  the merge, a first parent that is not the approved commit, a merge made on main's commit, of a
  commit main does not hold, changing code beside its documents, keeping code main deleted, editing
  a document that did not conflict) differ in one line of `land::covered`'s answer, which is
  public: each is asserted on a repository alone, with no board, approval or check.
  `land::carried::a_merge_that_settles_code_goes_back_to_its_reviewer_naming_it` keeps what the
  land does with a refusal.
- **One case where two differ in nothing the product reads** (26 s):
  `land::approval::a_legacy_mark_never_lands_on_its_orchestrators_approve` (two repositories that
  differ in a mark's text no code reads) and
  `land::carried::a_conflict_in_the_rules_or_a_skill_is_the_reviewers_still` (two paths one unit
  test classes alike).
- **`land::a_landing_starts_the_projects_on_land_command_with_its_slug_and_merge`** (36 s, 10 to
  15 s saved): its first land proves a scratch board starts nothing, which the unit test
  `on_land::tests::only_the_repositorys_own_board_starts_the_command_unless_told` proves; and it
  starts a real regenerator nothing reads, where a stand-in script does.
- **The lands' process-level twin**: one of two iterations of
  `land::the_lands_sweep_removes_its_in_process_executors_worktree_and_says_what_it_freed` (11 s)
  is `causes::the_lands_pass_removes_its_subagents_worktree_and_names_the_bytes` through the
  binary; the iteration with a session that left has no twin and stays.

Keep, with their waits: `land::limits::a_land_waits_for_another_past_its_timeout_naming_it_and_lands_once_it_ends`
(2.5 s) and `land::signals::a_terminate_to_a_land_waiting_for_another_ends_its_wait_and_lands_nothing`
(1.5 s) show a wait that has no timeout, which no event can show. Three waits are the product's
own and could take a clock or a setting read only when set: the 5 s between a check's TERM and
its KILL (`land::limits::a_check_past_the_timeout_is_ended_whole_and_refused`, 15 s), a 3 s
regenerator timeout (`land::regenerate::a_check_past_its_time_is_filed_as_late_not_as_failed`) and
the 2 s between looks of `regenerate --status --wait`. Ten seconds in all; they ride with the task.

Expected: about 250 thread-seconds.

### The command line, and what finishes

`cli` (19 tests, 450 s), `finished` (18, 255 s), `kept` (5, 108 s), `build_output` (25, 285 s),
`rules` (85, 204 s), `causes` (16, 124 s). The command-line tests cost 20 to 60 s each in the run
and a tenth of it alone: they are the only ones here that ask the real `lsof`, about 30 times in
all, and they run beside the 300-worktree tests. Each is the only proof that an argument or an
environment variable reaches the sweep; what the sweep then decides is proved a layer below.

Remove:

| test | time | proved by |
|---|---|---|
| `rules::the_sweep_takes_the_scan_it_is_given` | 1 s | `rules::a_process_working_inside_it_keeps_it`: the same injected scan, the same assertion; a detached worktree where the other has a branch. |
| `cli::a_dry_run_says_what_it_would_and_starts_no_sweep` | 25 s | `rules::a_dry_run_removes_nothing_and_says_what_it_would` for what a dry run says and keeps, once its stamp assertions move to the merged command-line test below. |

Merge:

| into one test | from | it asserts | saves |
|---|---|---|---|
| a dry run then a real one, on one detached worktree | `cli::the_command_line_asks_lsof_itself`, `cli::the_command_line_dry_run_says_every_worktree_and_what_it_would_free`, the green half of `cli::a_sweep_work_finishing_started_says_when_and_why_first` | the dry run's lines and no stamp; then `--background`'s first line, the removal, the exact summary and the stamp | 50 s |
| `cli::the_tasks_named_let_their_closed_sessions_worktrees_go_at_once` | `cli::an_explicit_task_sweep_visits_only_its_worktrees_even_during_a_full_pass` | a full pass keeps all three; the named tasks' two go while a pass is held, the third stays | 28 s |
| the window, by flag and by variable | `cli::the_window_is_a_flag`, `cli::the_window_is_an_environment_variable` | one worktree an hour idle goes at the flag's half hour, a second at the variable's | 12 s |
| a window that is no number, refused | `cli::a_window_that_is_not_a_number_of_hours_is_refused`, the red half of `cli::a_sweep_work_finishing_started_says_when_and_why_first` | each refused before any pass, in one repository | 13 s |
| `rules::a_sweep_with_nothing_to_do_says_so` | `cli::a_sweep_with_nothing_to_do_says_so` | the exact summary line, which the first merged test prints through the command line | 13 s |
| `finished::a_finished_worktrees_output_goes_with_it` | `finished::a_worktree_with_no_task_lets_its_output_go` | a done, a cancelled and an unassigned worktree, each with output, gone in one sweep | 25 s |
| `finished::the_closed_sessions_worktree_of_a_finished_task_goes_without_the_window` | `finished::a_worktree_of_another_task_keeps_its_window` | with no task named both stay; with one, its worktree goes and the other's stays | 18 s |
| `kept::a_worktrees_artifacts_are_kept_and_its_build_output_is_not` | `kept::under_its_cap_the_sweep_keeps_what_it_sets_aside_and_warns_of_nothing` | the same sweep: what is kept, no warning, the record | 10 s |

Make faster, by one repository where a test builds one a case and the cases do not meet: in
`finished`, `artifacts_of_finished_and_unassigned_worktrees_are_set_aside_and_the_worktree_goes`
(41 s, three repositories), `an_unmerged_or_dirty_worktree_of_a_finished_task_stays` and
`a_finished_worktree_still_keeps_a_change_or_an_untracked_file`; in `build_output`,
`a_task_before_review_keeps_the_build_output` (five) and
`a_branch_with_no_commits_ahead_of_main_loses_it_once_its_task_is_done` (a worktree removed that
nothing asserts); in `rules`, `a_branch_a_live_task_holds_keeps_it`,
`each_thing_it_reads_alone_keeps_it`, `activity_after_the_first_check_keeps_each_activity_marker`,
`a_session_or_launch_registered_during_the_sweep_keeps_its_tree`,
`a_new_or_replaced_lock_is_never_unlocked_by_the_old_snapshot` and
`a_detached_agents_ref_stays_while_its_session_holds_the_worktree` (nine). And
`cli::a_busy_trigger_returns_without_opening_the_board_connection` (32 s) reaches a full `lsof`
scan twice only because its kept worktree holds an untracked file; an unmerged commit keeps it
before the scan, with the same assertions. About 95 s.

Keep, as the only test of its rule: `cli::inherited_repository_and_index_variables_do_not_redirect_sweep_operations`
(53 s) and `cli::busy_trees_with_escaped_names_are_kept_in_real_and_dry_sweeps` (41 s), which need
a child's environment; `cli::the_worktree_running_the_sweep_keeps_itself`; both `kept::past_its_cap_`
tests (39 s, 33 s), whose sequence of sweeps is their subject;
`rules::replacement_worktree_locks_survive_every_dead_lock_release_interval` (24 s, fifteen
repositories whose hooks count calls within one sweep); and every test of `triggers`, `report`,
`on_review`, `sessionless` and the rest of `rules`, `causes` and `build_output`, one repository
and one or two sweeps each.

Expected: about 270 thread-seconds as measured, 30 to 40 s of it on a quiet machine.

## perf

`crates/tasks/tests/perf/`: 52 tests, of which ten use the generated board (50,000 messages,
2,600 sessions, 100 recipients), made once and held by one test at a time, since a timing must
never share the server. So the binary is serial whatever its threads, and each test's reported
time is its turn in the queue. Alone, the binary took 385 s:

| phase | alone | what it is |
|---|---|---|
| the board made | 57 s | the copy (19 s), every recipient's first look (24 s) |
| `cleanup::static_board_exit_waits_for_its_server_even_after_a_poisoned_worker` | 55 s, beside the others | a second process that makes the whole board again |
| `plans::every_query_a_write_a_recipient_or_a_tick_sends_is_planned_on_an_index` | 137 s | 21 commands through the recording relay, then 109 statements planned |
| `plans::every_query_the_schemas_events_run_is_planned_on_an_index` | under 1 s | 3 statements planned |
| `reports::the_roll_up_reads_the_board_once_an_hour_within_a_bound` | 5 s | |
| `sessions::a_read_that_shows_tasks_returns_the_sessions_on_them_and_a_whole_read_every_session` | 25 s | |
| `timings::say_claim_inbox_and_the_api_answer_within_their_bounds_on_a_board_at_scale` | 135 s | about 45 commands, each billed |
| `unanswered::the_digests_unanswered_lookup_reads_the_open_threads_alone_on_a_board_at_scale` | 2 s | |
| `words::` both tests | 26 s | every message deleted, 22 s of the server's CPU |
| `copied::the_generated_board_is_copied_whole_onto_a_store_in_the_runs_folder` | with the board | reads what the copy left |

In the release run the same took 910 s, and 1,179 s here. Every billed command waits for the
board's server to read quiet for half a second before it and after it, and a loaded machine
stretches each wait.

**What it proves.** That no query on a per-write, per-recipient or per-tick path scans a table,
and that each command's CPU stays within a stated bound at the live board's scale: the suite an
incident of 2026-11-07 made (`crates/tasks/tests/perf/main.rs`). Nothing else proves either, and
every test above is the only one of its claim.

**Dispositions.** Keep all ten. Make two faster:

- `cleanup::static_board_exit_waits_for_its_server_even_after_a_poisoned_worker` proves that the
  process's exit stops the board's server even after a worker panicked holding it. That is true
  of a board of any size; its worker makes the full one. A worker told to make a board of a few
  rows proves the same, and the main board's copy no longer runs beside a second copy. 55 s of
  work alone, 299 thread-seconds in the run.
- The plan check bills each of its 21 commands (`Board::run`), waiting twice for quiet around
  each, and reads no bill: it asserts plans, never a cost. Run unbilled, its commands wait for
  nothing. Not measured apart; 21 commands at one to several seconds of waiting each.

`timings` is what the waits are for, and stays as it is. Expected: 300 thread-seconds, and 60 to
150 s of the lane's wall in the run.

## The board's server

`crates/tasks/tests/server/`: 129 tests, 2,666 thread-seconds, on real SurrealDB servers over
stores on disk. A start cost 7 to 9 s in the run, a clean stop a few more; a reclaim is three
starts, three stops and three export or import clients a database. Every fixed wait in the binary
together is under 15 s: the time is starts, stops and writes.

| module | tests | thread-seconds | slowest |
|---|---|---|---|
| `reclaim` | 14 | 1,401 | 303 |
| `snapshot` | 7 | 371 | 72 |
| `home` | 28 | 222 | 26 |
| `hold` | 12 | 204 | 46 |
| `left_replay` | 7 | 173 | 30 |
| `copy`, `postgres` | 6 | 142 | 56 |
| the rest | 55 | 153 | 16 |

### reclaim

| test | time | disposition |
|---|---|---|
| `reclaim::a_row_rewritten_many_times_leaves_the_store_the_size_of_what_it_holds` | 303 s | faster |
| `reclaim::a_store_that_grew_under_the_value_log_is_rebuilt_with_every_database_task_and_thread` | 209 s | faster |
| `reclaim::databases_whose_names_differ_only_in_case_each_keep_their_own_tasks_through_a_reclaim` | 142 s | keep; takes a merge |
| `reclaim::tasks_reclaim_in_a_repository_rebuilds_the_board_in_its_common_dir` | 121 s | keep; takes a merge |
| `reclaim::a_board_s_asks_held_answered_and_closed_come_through_a_reclaim_and_the_next_whole` | 115 s | keep: its second reclaim is the point |
| `reclaim::tasks_reclaim_with_a_folder_rebuilds_that_board_and_says_what_it_did` | 99 s | merge |
| `reclaim::a_replacement_whose_record_fails_is_ended_and_no_task_written_after_the_rollback_is_lost` | 84 s | keep: the only rollback |
| `reclaim::a_reclaim_with_no_server_running_keeps_the_store_and_its_log_before_its_own_server_starts` | 78 s | keep |
| `reclaim::a_reclaim_stopped_between_its_moves_is_never_served_empty_and_finished_by_the_next` | 70 s | merge |
| four refusals (a store that cannot be kept, a name the board cannot say, no database of the board's, a board below its record) | 42 to 48 s each | keep: each its own state, served again |

- **The first** proves the board's value-log threshold and a small memtable keep a rewritten row's
  store bounded, with no reclaim. Its time is 2,048 writes of a 64 KiB row, one after another,
  exactly the 128 MiB its assertion asks (four times a bound that is sixteen memtables of 2 MiB).
  A memtable of 1 MiB halves the writes and keeps every ratio the test asserts, since each bound
  is a multiple of it; whether the store's fixed overhead stays under the smaller bound is one
  run. About 140 s. The writes are not batched into one request: one transaction would write one
  version and prove nothing.
- **The second** is the only proof a reclaim frees space and keeps every database, thread and
  search. 576 rewrites; the count its assertion needs follows from the reclaimed store's size,
  which its own output prints, and its last 64 rewrites repeat what the first test proves. 40 to
  65 s.
- **Merge** `a_reclaim_stopped_between_its_moves…` into `databases_whose_names_differ_only_in_case…`:
  the three databases' board is stopped with its store moved aside, a start is refused naming it
  and makes no empty store, then the reclaim finishes and each database is read. 60 s.
- **Merge** `tasks_reclaim_with_a_folder…` into `tasks_reclaim_in_a_repository…`, the one whole
  run of the command, which takes its line's assertions; which board the arguments name, and the
  line, become unit tests beside `reclaim_main`. 95 s. The folder test also runs with the real
  checkout as its working directory, so an argument read wrongly would stop that checkout's board.

### The rest

Remove `home::a_killed_server_is_started_again_on_the_same_board` (26 s):
`snapshot::a_snapshot_whose_log_holds_writes_is_never_pruned_and_the_others_are_kept_by_count`
kills a board's server, starts it again and counts the rows it replayed, and
`hold::a_log_written_under_a_larger_memtable_is_replayed_whole_and_survives_a_kill_after` does the
same with more of them.

Make faster:

- **One written-and-killed store a binary, cloned a test** (50 to 80 s): the seven `left_replay`
  tests and two of `hold` (`a_log_written_under_a_larger_memtable…`,
  `a_store_that_went_back_keeps_its_unapplied_segment…`) each start a server, write 2,000 or 5,000
  rows and kill it, to have a log to replay.
- **A stand-in where the server's own answer is not the subject** (30 to 45 s): seven `home` tests
  about what `Home` takes for its server (another version's, other data's, a garbled address file,
  one never recorded) start a real SurrealDB where `home_fixture.rs`'s `healthy_server` answers as
  one, as its own doc recommends.
- **A store at rest made once** (45 to 80 s, the least sure): four `snapshot` tests start, fill and
  stop a server to have a store to clone. Their 52 to 72 s with one to four starts each is above
  what their work predicts, and is timed again first.
- **`fixture_script::tests::`** (16 s): eight tests compiled into this binary and into `store`,
  run in one.

Keep: `hold`'s waits (a closing never killed, a start that does not come up, a wait that says
what it waits on) are their tests' subjects, one second each;
`copy::a_quarter_of_the_live_board_copies_whole_on_its_engine` (56 s) is the copy's cost on the
real engine; `postgres` and the rest are a start or less each.

Expected: about 550 thread-seconds.

## release

`crates/symphony-build/tests/release/`: 88 tests, 4,481 thread-seconds, 51 s each, and almost
none under 25 s. The fixture is not the cost (a repository, a bare remote and a database: under
3 s). A run of `symphony-build release` is: about 25 git processes, a full `lsof` of the machine
after each check (0.3 s alone, with 1,200 processes; more under load), and four to nine `tasks api`
processes, each opening the board, with a detached dispatcher after every write. The 88 tests
make about 250 runs. The same green night took 13 s in one test and 64 s in another, by what ran
beside it.

| module | tests | thread-seconds | slowest |
|---|---|---|---|
| `refresh` | 8 | 1,389 | 215 |
| `candidate` | 14 | 591 | 80 |
| `reds` | 5 | 506 | 187 |
| `releases` | 10 | 421 | 58 |
| `cadence` | 7 | 328 | 57 |
| `stops` | 12 | 259 | 36 |
| `regenerated`, `install`, `publish` | 12 | 583 | 90 |
| the rest | 20 | 404 | 51 |

**`refresh`** (103 to 215 s each): each test starts one to four real SurrealDB servers on disk,
fifteen in all, and each run is a night and a refresh of the same size. Each is the only proof of
its hold: a failed move leaves `current`, every snapshot comes before any move, a lagging writer
holds every board. **Keep all eight.** Their servers live until the binary exits; a test that
stops its own when it ends takes them out of every later `lsof` and off the machine.

Merge:

| into | folds in | the one test asserts | saves |
|---|---|---|---|
| `reds::a_red_check_leaves_no_tag_and_files_exactly_one_red_task_a_green_rerun_closes` | `reds::a_release_of_a_commit_before_a_red_leaves_that_red_open` | between the red's update and its fix, a commit before the red released out of cycle: tagged, the red left open | 112 s |
| `install::a_green_release_is_built_once_in_the_runs_checkout_and_installed_on_the_machine` | `install::on_the_tests_clock_with_no_stand_in_nothing_is_built_or_installed` | a last refresh with no stand-in builds and installs nothing | 50 s |
| `places::an_agents_worktree_behind_a_link_or_named_outright_is_never_reset` | `a_folder_in_the_checkouts_place_is_never_taken_over_and_the_board_hears_of_it` | after the link, a plain folder in the checkout's place: the record, the red updated, the green closing it | 20 s |
| `releases::a_second_release_that_day_is_numbered_and_the_next_day_starts_again` | `releases::a_green_tip_is_tagged_as_the_days_release_and_only_the_tag_reaches_the_remote` | its first night also proves only the tag reached the remote | 22 s |
| `stops::a_process_left_in_the_checkout_stops_the_run_before_it_checks_anything` | `stops::a_process_holding_a_file_inside_the_checkout_open_stops_the_run_too` | a second stop, by a file held open, updates the same red | 11 s |

The first merge is checked against the red's asserted body before it is made: an extra landing
may change the changes it lists.

Remove, each proved twice, by a unit test of the rule and a whole-run test of its wiring:

| test | time | proved by |
|---|---|---|
| `one_run_at_a_time` | 47 s | `stops::a_lock_held_by_no_run_or_past_its_time_is_stuck_and_the_board_hears_who_holds_it` (the same refusal, no record), `stops::a_lock_held_while_a_killed_runs_check_runs_on_is_refused_as_that_checks_run` (a refusal files no red) |
| `candidate::a_land_that_merged_main_after_its_approve_advances_to_the_commit_approved_before` | 44 s | the unit test of the same name in `candidate_tests.rs`, and `candidate::a_red_run_holds_its_commit_until_its_reds_own_land_answers_it_and_the_rerun_releases_it` for the advance, rerun and release |
| `candidate::a_reds_land_with_no_approved_commit_that_passes_drops_it_and_main_is_run_at_once` | 43 s | `a_land_with_no_approved_commit_that_passes_drops_the_candidate` in `candidate_tests.rs`, and `candidate::the_hours_bound_drops_it_and_main_is_run_at_once` for a drop running main in the same run |
| `candidate::a_red_closed_as_covered_by_a_landing_that_did_not_advance_it_drops_it_and_main_is_run` | 38 s | its unit test in `candidate_tests.rs`, and `candidate::a_red_closed_as_covered_by_another_reds_land_is_answered_by_it_and_the_rerun_releases` for a covered close read from a real board |

These four are the review's least sure removals: each loses one reading from a real board that
its pair makes on a board of the test's making. Their task's reviewer decides each.

Make faster, every assertion kept:

- **The dispatch in the write.** The fixture does not set `TASKS_DISPATCH=inline`, so every board
  write of every run starts a detached `tasks dispatch` that opens the board for nobody. The
  board's own tests set it.
- **The refresh tests' servers stopped by their tests**, as above.

Neither is measured apart; both are processes not started, in the binary that suffers most from
them. Keep: `regenerated`'s three 3 s holds and `stops::a_remote_that_fails_or_hangs…`'s 8 s are
waits whose length is the subject, and every other test of `cadence`, `regenerated`, `steps`,
`recorder`, `consumers`, `renamed` and `publish` is the only whole-run proof of its wiring.

Expected: 215 thread-seconds from the merges, 170 from the removals, and the two fixture changes
beside them.

One change is the product's, and no task is filed for it here: a run asks the board for each red
by two `tasks api` processes a key (`crates/symphony-build/src/release/board.rs`), four on a green
night. One process answering all of a run's reads would take a real release run's board reads from
four to one as well. It is a change to `tasks api`, a contract, and waits for a time of one
`tasks api` call under load to say whether it is worth one.

## browser

`apps/symphony-web-tests/tests/browser/`: 424 tests, 4,866 thread-seconds, 12 s each, the most
even of the six: no test or module carries it.

**A browser a test.** About 394 tests start a Chrome of their own, many several (a fresh profile
is how a test gets a first visit), and the `shot` tests' 95 runs of `symphony shot` each start
one inside the command: 550 to 650 starts a run, 1 to 1.7 s each
(`the_ask_poll_hold_keeps_a_retry_left_waiting_and_lets_a_cleared_one_go`, a start, a `data:` page
and a stop: 1.7 s). That is 12 to 20% of the binary. The stand-in server is a thread and costs
nothing.

One Chrome for the whole binary, a context a test, would save most of it, and is **not
proposed**: every wait in these tests is given the CPU of the browser's whole process group as its
allowance (`crates/chrome-devtools`), so tests sharing a browser would be charged each other's
work, and a wait would end early under load, the failure `tests/no-test-fails-on-load` exists to
end. A browser shared within one test has no such cost, and is proposed below.

| module | tests | thread-seconds | slowest |
|---|---|---|---|
| `shot` | 50 | 640 | 50 |
| `layout_shift` | 10 | 491 | 105 |
| `polish` | 28 | 341 | 28 |
| the binary's own top-level tests | 26 | 322 | 66 |
| `projects` | 10 | 281 | 38 |
| `look_parity` | 17 | 250 | 33 |
| `owner_answer` | 6 | 216 | 57 |
| `document_landing` | 6 | 194 | 70 |
| `restored` | 22 | 173 | 24 |
| `notice_layout` | 6 | 170 | 60 |
| `landmarks` | 4 | 168 | 71 |
| forty more modules | 226 | 1,620 | 46 |

Remove:

| test | time | proved by |
|---|---|---|
| `a_first_visit_and_a_return_visit_fit_their_budgets` | 15 s | `entry_pages_fit_the_first_visit_budget_and_deferred_kinds_are_reported`, whose `/` case holds the same two byte caps on a fresh profile, waits for every request and serves the shell with its preloads as the real server does |
| `owner_answer::an_owner_verdict_shows_and_regroups_on_its_page_and_a_second_tab_without_a_reload` | 26 s | `owner_answer::an_owner_verdict_given_beside_an_open_excerpt_shows_and_regroups` and `owner_answer::a_draft_left_on_another_card_does_not_stop_the_answer_regrouping`, which run the same helper on the same page and assert all it does, with more open |

Make faster, by the product's own clock injected, which the rule allows:

- **`document_landing`** (194 s, about 80 saved):
  `a_reader_of_prose_stays_on_it_whatever_draws_beside_it` (70 s) and
  `a_reader_of_a_card_stays_on_it_whatever_draws_beside_it` (68 s) each make a dozen landings that
  wait out the client's 3 s settle deadline while reads are held, 28 times in the module. The
  deadline's timer is held and fired once the landing is in, as `hold_ask_polls` already holds the
  ask's retry.
- **`owner_answer`** (40 to 50 s): `a_draft_resumed_after_a_reload_keeps_the_words_its_tap_does_not_hold`
  (57 s) and `a_retry_after_two_unanswered_sendings_keeps_the_words_the_server_did_not_write` wait
  out the client's 15 s answer deadline twice each. `window.symphonyAnswerMs` shortens it, as
  `polish` and `skeleton` already do, while the writes are held.
  `an_answer_whose_reply_comes_too_late_shows_on_its_own_tab_from_the_feed` (32 s) is changed only
  if the order of its feed event and its deadline holds under load.
- **A window in which nothing must happen** (20 to 40 s):
  `panel_open::a_page_warms_on_a_fast_link_and_never_when_it_saves_data_or_is_on_2g` (33 s) and
  `cache_ahead::nothing_is_read_ahead_when_the_browser_saves_data_or_its_link_is_3g_or_slower`
  (33 s) wait two seconds or more a case to see no read. The page schedules its warm-up by an idle
  callback: captured and run by the test, its absence is asserted with no wait.
- **`a_stream_that_falls_silent_is_lost_and_healed_while_one_that_pings_holds`** (30 s, 15 saved):
  the silence it waits out is two and a half of the board's ping interval, which its stand-in can
  serve smaller, never under two seconds; that the bound is 12.5 s at the longest interval becomes
  a unit test.
- **`shot`**: three clicks on one page of labels in one run of the command (12 s), from
  `shot::a_click_on_a_checkbox_its_own_label_covers_lands_on_the_label_and_checks_it`,
  `…under_a_custom_element_no_control_takes…` and `…under_a_track_deep_in_plain_spans…`; and
  `shot::two_shots_at_once_each_have_a_profile_and_a_port_of_their_own` holds a route both pages
  ask for where it pauses four seconds.

Make faster, by a context where a test starts a browser for a cold profile (60 to 120 s): a small
addition to the driver (`Target.createBrowserContext`) lets one test's cold visits share its one
Chrome, its waits still charged its own work alone. About 65 starts:
`layout_shift::a_task_of_another_shape_moves_less_than_its_first_visits_limits` (105 s, six
browsers), `layout_shift::a_task_holds_still_while_its_thread_loads` (94 s),
`entry_pages_fit_the_first_visit_budget_and_deferred_kinds_are_reported` (66 s, ten), both
`preload` tests (46 s each, eight each),
`landmarks::a_task_draws_its_head_where_its_ghost_did_and_on_the_next_visit_every_part` (71 s) and
`notice_layout::on_a_phone_the_pages_without_a_composer_keep_their_notices_on_screen` (60 s). A
context keeps its cache and storage in memory, so each test that reads `fromDiskCache` or the
connection limit is checked as it moves.

**`projects`** (281 s): every test starts a board, a real server and two or three registered
projects. Three that write nothing to them (`a_draft_and_a_theme_one_project_keeps_never_appear_in_another`,
`the_top_left_switch_and_picker_work_by_keyboard_on_a_compact_nested_page`,
`every_header_and_home_link_lands_in_a_project_whose_guide_is_docs_review`) share one made once;
15 to 40 s, not measured.

Keep: `layout_shift` and `landmarks` wait 1.5 s of stillness after every load, which is the meter
that catches a late mover; `phone`'s 2.5 s is the product's promise in elapsed time; the other
eight `layout_shift` tests, three `landmarks` tests, `polish`, `restored`, `look_parity` and the
forty smaller modules are a page and an assertion each, read against their nearest neighbours
(`polish::the_tasks_board_keeps_its_height_as_rows_load` against `layout_shift`,
`restored::reconnect_pills…` against `notice_layout`) and found to assert different things. Not
every test of `polish` and `restored` was read.

**The shot refusals: an open question, no task.** About 32 runs of `symphony shot` are refusals
(a target covered, clipped, hidden, never still), each waiting out the command's 5 s, which is
what a refusal is. One browser asking the same probe of the same pages would prove each verdict
in a fraction, with one whole-command refusal kept for each message: about 175 s. It would lose
the proof, target by target, that the command's scroll-and-settle loop does not change the
verdict. That trade is the owner's, and nothing is filed until it is made.

Expected: about 240 thread-seconds from the clocks, removals and merges, 90 from contexts, 25 from
the shared projects.

## standalone

`apps/symphony-server/tests/standalone/`: 8 tests, of which one is the binary:
`actual_standalone_workspace_compiles_its_server_without_a_containing_project`, 478 s in the
release run, 977 s here, 99 s alone.

**What it proves.** That this workspace, copied out of any containing project with a hostile
parent's tools first on `PATH`, builds its server with its own Cargo wrapper and runs it. Nothing
else compiles the workspace from outside.

**Where the time goes**, alone: copying the tree 10 s, `cargo clean` 1.5 s, `cargo check` of the
server 32 s, `cargo build` of the same 46 s, the recipe's run 4 s. Both compiles run at the
wrapper's two jobs, beside seven other lanes.

**Disposition: make faster.**

- The `check` compiles every workspace crate the `build` then compiles again, and proves nothing
  the build does not: a crate that builds has checked. A third of the test.
- The copy takes `apps/symphony-web/node_modules` (164 MB) and the checkout's artifacts folder,
  where the test itself leaves an 85 MB proof a run; the server's build reads neither.

Expected: about 310 thread-seconds as measured, 150 s of the `rest` lane in the release run.

## The ranking

By thread-seconds saved for the risk taken. Risk is what a mistake would cost: low where
assertions move unchanged or a fixture shrinks, medium where a test's layer or a product seam
changes, high where a proof is traded.

| | proposal | saves | risk | task |
|---|---|---|---|---|
| 1 | The sweep's 300-worktree removal pass | 900 | low to medium | `tests/sweep-300-worktrees-faster` |
| 2 | The standalone test compiles once and copies what it builds from | 310 | low | `tests/standalone-compiles-once` |
| 3 | `perf`'s cleanup worker on a small board, its plan check unbilled | 300 | low | `tests/perf-cleanup-and-plans-faster` |
| 4 | The board's server: reclaim's writes, two merges, shared stores, stand-ins | 550 | medium | `tests/server-reclaim-faster` |
| 5 | `release`: dispatch in the write, servers stopped, five merges, four removals | 385 and what the fixture gives | low to medium | `tests/release-runs-fewer-processes` |
| 6 | The sweep's command line and finished tests merged | 270 | low to medium | `tests/sweep-cli-finished-merged` |
| 7 | The land tests: two removals, five merges, a landing by hand, a rule a layer down | 250 | medium | `tests/sweep-land-merged` |
| 8 | `browser`: injected clocks, two removals, the label clicks | 240 | medium | `tests/browser-waits-by-clock` |
| 9 | `browser`: a context where a test starts a browser for a cold profile; `projects` shared | 115 | medium to high | `tests/browser-contexts-in-a-test` |
| | **filed, in all** | **about 3,320** | | |
| | The shot refusals asked of one browser | 175 | high: a proof traded | none: the owner's question, above |
| | One `tasks api` process for a release run's reads | unmeasured | medium: a contract | none: waits for a time of one call |
| | One browser a binary | 550 to 900 | blocked by the waits' CPU accounting | none |

About 3,320 thread-seconds of the 20,000 measured, with the figures of the run made under load;
on a quieter machine each is smaller and so is what it is taken from. Per lane, as the release
run's walls:

- `sweep`: 1,290 s to about 970 s (tasks 1, 6 and 7 are a quarter of its thread-seconds), and
  less if the 300-worktree test was slowing its neighbours as measured.
- `perf`: 910 s to 760 to 850 s.
- `rest`, which runs `standalone` (478 s) and `release` (416 s): 150 s and about 40 s less.
- `browser`: 726 s to about 670 s.
- `board`, which runs `server` (285 s) among five: about 60 s less.

Every task changes tests, so each needs a fresh reviewer, and its brief says so. Each names the
behaviours it must still prove, by test, and gives its binary's time before and after, alone and
at the same load.

## What was not reviewed

- **`cli`, `store`, `launch`, `launcher` and `child_guard`** of the board (997 s of the `board`
  lane, 2,380 tests): `cli` took 423 s for 667 tests and `store` 322 s for 1,349, each test cheap and the sum
  long. They were not among the six named, and the `board` lane stays the step's longest until
  they are read or one scheduler runs them beside the rest.
- **`after_worktrees`** (168 s, 61 tests), and `after_commit` and `after_publication`, which the
  run's log shows run again a test a process (4 to 24 s each) beside their whole runs.
- **Every test of `polish`, `restored` and `look_parity`** in `browser`, read by their neighbours
  and their slowest, not one by one.
- **The load itself**: what each lane costs every other was measured for three binaries by running
  them alone, and not modelled.
