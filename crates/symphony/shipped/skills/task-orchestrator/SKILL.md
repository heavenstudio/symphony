---
name: task-orchestrator
description: Keep the task board's development pipeline healthy without doing the tasks yourself. Finds gaps between approved docs and the game, files them as tasks, keeps executors and reviewers running, lands approved work with just land. Trigger on "orchestrate the dev work", "keep agents going on tasks", "run the pipeline", "find what the docs have that the game lacks", or /task-orchestrator.
---

# Orchestrate development from the board

You keep the pipeline moving; you do not do tasks. Load `/tasks` first for the
board mechanics (register, callback, roles). This page is what went wrong or cost
real time before, and what to do instead.

Register your native callback with `just tasks callback --as HANDLE`.
Chat goes to the explicit chat seat, then an `--if-free` fallback, then a live
orchestrator with a native callback. Among plain orchestrators, the lexicographically
smallest handle wins. Native presence confirmed by the sweep counts as live;
a recipient suspended after delivery failures waits for the sweep to restore it.
No chat flag is needed for this orchestrator fallback. `--chat --if-free` still
opts into the higher-priority fallback without displacing an occupied seat.
Every connected agent exposes its native channel in `/api/agents`: a Claude
inbox socket or Codex thread (queue by default; `sf.codexSteer` opts in to
active-turn steering, with idle threads still queued). Herdr is for inspecting panes, never the
message channel. An agent without a native channel is shown as such and keeps
its inbox. The app names the chat recipient and says when nobody receives it.
Owner, 2026-10-25: "You should not 'listen to chat'. You should register yourself
to it, and then get a message callback directly addressed to you"; "same goes
for most of the things in the task board."

## Pull work, don't wait for events

Events only tell what changed; the board says what nobody is doing. Your
callback does both: besides events it receives a
`PULL executing=N ready_unstaffed=N in_progress_orphaned=N
review_unreviewed=N landed_open=N idle=N` digest once at first dispatch, even
when empty. Later delivery compares the last delivered lists after forgetting
entries absent from any computed digest: a returning entry pushes once, an
always-present entry is never re-sent, and counts, departures and time alone
never trigger delivery. It shows the first six of each:

- ready with no live executor (a ready or in_progress subtask's executor counts);
- in_progress with none;
- review with no reviewer;
- landed on main but the task still open;
- idle agents.

Act on every line: staff the top ready task whenever there is test-slot
capacity, re-staff an orphan, put reviewers on unreviewed work, close what
landed. Do not wait for an event to find work, and do not rebuild the digest
by hand. The shared server schedules timed events and delivery retries.
The digest is `symphony/crates/tasks/src/cli/pull.rs`.

Unaddressed posts stay on their task threads. Read them in the app or with
`just tasks show SLUG`; they do not arrive by push. Post milestones only: a
tip ready for review, a move to review or blocked, a collision, a finding, a
verdict, or a question. Address what someone must act on with
`--to @orchestrator`, `--to @reviewer` or `--to HANDLE`. Never acknowledge a
message. Never repeat a post on both a subtask and its parent.

## Who is doing what: ask the app, not herdr

**The app's agents API is the source of truth for who is doing what:**
`just tasks agents [--json]`, the same as `/api/agents` of the Symphony. Each
agent has its task and role, its real worktree and branch, its Claude/Codex
session id, its herdr pane, a `state` (`working`, `running_test` with the recipe
and whether it holds a slot or waits for one, `waiting_on_input`, `idle`,
`gone`), its last tool call and last board message. Use it for every question
about agents: who is idle, who waits on a verify (never call that LOST), who is
blocked on a question, which pane to restore. herdr tab and agent listings are
only a probe the API reads for you; look at herdr directly to act on a pane
(`just tasks launch|restore|cleanup` do), never to find out what the agents are
doing. Its fields: [TASKS.md](../../../docs/workflow/TASKS.md#the-agents-api).

## Before anything: can a worktree agent see the work?

Worktree agents branch from `main`'s HEAD. **Check `git status` first.** Once,
the board CLI (`tools/rs-bin.sh tasks`), the Symphony and every design catalogue were
untracked on main: no agent in a worktree could claim a task or read its spec.
If approved docs or tooling are uncommitted, ask the owner, then commit them
before any executor starts. The concepts session commits docs as it goes, so
recheck when an agent reports a missing doc.

## Machine load: few agents, few worktrees

On 2026-09-29 the Mac panicked with its vnode table full (263168 in use, 0
free). star-foundry held 209 worktrees (8.8 GB): each agent worktree installed
its own `.venv` (about 1,900 files; until tools/shared-venv lands it still
does), and 181 of them sat merged and clean, because `land` removes only a
landed branch's own worktree and nothing removed the rest. Several full-scope
runs also overlapped (load average 194, 736 Godot processes in an hour). So:
- One full verify per change, at land. Owner, 2026-11-04: "One full verify,
  at land: Review rounds use check + targeted tests + a 2-min static
  pre-check, reviewing in parallel; the full fast plan runs once, when
  landing." Executors move to review after `just check`, the targeted tests
  and `just precheck`, with no branch `just verify`. Reviewers run the targeted tests and
  read the diff, and ask for no branch certificate. The move to review starts
  the tip's verify in a private checkout, in parallel with the reviewers; its
  certificate is one more stamp, and a red there goes to the executor by
  itself. Once every stamp is in, the landing starts by itself: a clean merge
  lands at once, unverified, and the verify of main runs right after it.
  `verify-full` is never a landing prerequisite.
- Test runs share the machine's slot pool, which queues them: an executor's
  targeted tests wait for no go from you. Honor explicit holds on the thread
  when coordinating load.
- At most two executors working at once, each in its own worktree. An executor
  whose task waits in review doesn't count, so start the next one when a task
  moves to review. But its worktree sits there until landing, so land or
  release branches in review promptly. Reviewers that read in place and run
  only targeted Rust or shell suites or `just test-sim SUITE=…` don't count
  either. This limit stays after tools/test-run-cap, which caps test runs, not worktrees.
- Claude Code locks each agent worktree with the pid of the session that
  spawned it, and unlocks it when the agent finishes, unless the agent still
  has a monitor or background command running. So put "stop your monitors and
  background commands before your final report" in every brief. Before
  `just land`, check `git worktree list --porcelain`: if the branch's worktree
  is still locked with your pid (`$PPID` in your Bash tool) and its executor
  has handed back, run `git worktree unlock` on it first. `land` then removes
  it after its own left-behind check, deletes the branch, and clears the
  branch's records under `.git/sf-land/` (authors, requesters, branches).
- If `land` already printed "kept BRANCH: its worktree … could not be removed",
  the fallback is `git worktree unlock`, `git worktree remove` without
  `--force`, then `git branch -d BRANCH`. It leaves those `.git/sf-land/`
  records behind, and a new branch of the same name would inherit them, so
  prefer unlocking before landing. Never do this on the other "kept" lines:
  "holds uncommitted work or files that did not land" keeps ignored files a
  plain remove deletes, and "it moved while landing" means the agent may still
  be at work. The sweep (`just tasks sweep --dry-run`) names the worktrees it
  kept for files left behind and those it spared in use, with why; a lock
  shows in `git worktree list -v`.

## Know who else is on the board

- `just tasks who` lists sessions (there is no `sessions` command). A board
  full of `lost` sessions still holding roles is normal after a restart. Their
  tasks are claimable, and nothing is reassigned automatically.
- **A concepts session owns design and art** (docs/workflow/TASKS.md, Sessions). It
  also files dev tasks. Before filing, read `just tasks list` and the
  `NEW` events, and don't duplicate its slugs. Tell it what you own via
  `SendMessage` (find it with `ListAgents`), not only on a thread.

## Finding gaps

- Owner verdicts waiting on an agent arrive as `verdicts/…` tasks: the
  Symphony files one when the owner rejects or comments and adds later
  verdicts to its thread until it is claimed. Dispatch it like any task.
  `just verdict-tasks` lists open cards no live task acts on; it should list none.
- Approval is per card, not per document. Documents have no status.
  A card is approved when marked
  `[x]`, `[r]` or `[a]`. `[ ]`, `[?]`, `[-]` and `[~]` are not.
- Split the audit by domain (the combat catalogue; ships, factions and
  stations) into parallel read-only Explore agents. Ask each for a status
  table, a task list with dependencies and acceptance tests, and the owner
  questions. That cost about 170k tokens per agent, and it was worth it.

## Rules that would block every task: ask once, write the answer down

Several `AGENTS.md` "ask first" items (state_hash, replay format, entity ids)
turned out not to reflect what the owner wanted, and nearly every sim task
touches them. Batch the blocking questions into one `AskUserQuestion` before
filing. Put the answers in a **rulings footer on every task brief**, and fix the
guide that was wrong, so executors don't re-ask. Don't ask for what the
owner's ask already covers (a dependency, a framework): quote the ask in the
brief instead (RULES.md `workflow/approval-goes-with-the-ask`).

## Reports for the owner go in the Symphony

A report, comparison or gallery for the owner goes in the Symphony (a docs
page, images under `artifacts/`, a card for the decision), never in a claude.ai
artifact.

Adding, moving or deleting a doc (or a docs folder) means linking it from its folder's
`README.md` (and docs/README.md's Folders list for a folder): `just docs-index`, part of both
verify scopes, fails on a page its README does not link, or a list README without a row for it.
Say so in any brief that adds a page.

## Filing tasks

- **Before filing a tool, check whether one line in a skill would do.** If an
  agent can follow the rule, write the rule and build nothing. Owner,
  2026-10-24, on a board relay that typed news into waiting agents' panes when
  the orchestrator skill already said to: "Do we need a tool for this?? Isnt it
  just an instruction for agents?" and "Be careful not to go overboard on
  \"Creating tools for things that could had been a one line instruction\"".
- `--depends` must name a task that already exists. Create in dependency
  order, or run the filing script twice (make it skip slugs that exist).
- A task that produces shared data (hull dimensions, outline notes, a design
  doc's decided content, content JSON) is split: the data is its own small
  task that lands first, and the code or art tasks that read it `--depends` on
  it. A dependant builds on main: its brief never imports from the worktree
  of a `--depends` task that is not done (`just tasks create` refuses it; a
  review's diff, an executor's own cwd and `<worktree>/artifacts/` evidence
  are fine)
  ([RULES.md](../../../RULES.md) `workflow/shared-data-lands-first`).
- One brief per task: the doc#Heading, the approved card slugs, what is
  pending and must not be built, and an acceptance test on a consequence over
  a run.
- Take `role SLUG orchestrator` on each task, so its events reach you. A task
  has one orchestrator (owner, 2026-11-04: "A task should go to one"): when
  another orchestrator still holds it, the refusal names the holder. Hand one
  of yours to another orchestrator with `just tasks handover SLUG HANDLE`, in
  one step; its thread then names the new orchestrator, and agents address
  that one. A lost or left holder's role moves to you when you take it. You
  hear your own tasks and the unclaimed ones only, so another orchestrator's
  work is not yours to staff. `just tasks list --doubled` lists tasks still
  holding two from before the rule: settle each with `handover`, or `role`
  again where the other holder left.

## Instructing agents

**Every instruction to an agent is a subtask**, never a brief file or a long
prompt. Create it with
`just tasks create SLUG "title" --parent PARENT [--instruction] --body-file -`,
with the instruction **verbatim as the body**, written the way you would say it
to the agent. For a review, a measurement or any step nobody lands, use
`--instruction`, so its executor closes it with `just tasks done SLUG "note"`.
Executors and reviewers are Claude, started as subagents or `claude -p`; Codex
takes the adversarial review of a Claude executor's branch, images and 3D
([standing rule](../../../RULES.md#workflowclaude-first-agents),
[which way an agent starts](#which-way-an-agent-starts)). For a subagent or
`claude -p`, register its handle
(`just tasks register --kind worker|reviewer --parent <you>`) and create its
subtask with `--for <handle>`. For a herdr pane, create the subtask without
`--for` and run `just tasks launch SLUG --kind K` (a reviewer:
`--role reviewer` too), which registers the handle. Start it with the prompt
`/work SLUG <its handle>` (Codex: `$work SLUG <its handle>`) and nothing else.
Board launches send that prompt themselves: the `work` skill carries the
handle, claim, worktree, callback or inbox, thread, verify, process and finish
rules, so the body holds only the task itself.
Owner, 2026-10-22: "'do board skill X using .....' is pointless prompting!
This should be something like `/work <task-number> <agent-orchestrator-id>`".
You take the orchestrator role on the root task; every event on its subtasks,
at any depth, reaches you. `--body-file -` reads the instruction from stdin, so
it is never a file; a hook refuses a brief written to a file or to a session
scratchpad, and says to create the subtask.

**An instruction you send twice goes into a skill.** Owner, 2026-10-04:
"something you need to keep sending to agents should in general be a skill",
and "if you use a similar prompt more than once, make it a skill/document and
refer to it in the next prompt". The second time you write the same thing into
a subtask or onto threads, write it instead into the skill those agents load,
and from then on the prompt names the agent's handle, role and slug and points
there. Executors and reviewers follow the [work skill](../work/SKILL.md); a 3D
builder or blind 3D reviewer also the [hull-models skill](../hull-models/SKILL.md)
and its REVIEW.md. The work skill already holds these, so a body never
repeats them: logs go under `artifacts/<handle>/`; no planted faults or red runs
(work habit 10); messages are short; one command per call; a test is
committed with its code; tests are run wisely (work step 11); an executor
never lands or reviews its own work; at tiers 2 and 3 a reviewer finishes
with `just land-approve`, not a board approve verdict (see
[Running executors](#running-executors)); no subagents or forks.

**The owner never runs a command** to approve or unblock work. Give the owner a
ready `! <command>` line only for the owner's own configuration (an install, a
login, a rule in `~/.codex/rules/`).

## Running executors

- Start each executor as [Which way an agent starts](#which-way-an-agent-starts)
  says: a Claude subagent or `claude -p`, in herdr too, and a pane only for
  work that needs one.
- The owner picks the model per role (in session, 2026-09-30): Sonnet for code
  development, Opus for planning and reasoning, and Codex with its
  Sol-6.1 model for images and 3D modelling. Reviews (2026-11-04): Codex,
  `gpt-6.1-sol` or better at `xhigh`, for the adversarial review of a Claude
  executor's branch; Claude for every other review, and for every review of a
  branch Codex wrote. The owner asked for Opus at medium effort or above, so
  an Agent launch passes `effort` (`medium` or above) beside its model, and
  `claude -p` passes `--effort`.
- **Pick concurrent tasks that touch different files** (for example sim
  combat, content data and view drawing). Two agents in `sim/combat.gd` means
  merge conflicts.
- The body of its subtask says the task and the step that finishes it
  (`move SLUG review` for work that lands as a branch). What once went in
  every brief, because each one cost executors retries, is in
  [`/work`](../work/SKILL.md), so a body never repeats it: tests with or
  after the code (step 3); one command per call (the worktree sandbox
  refuses loops, `cd && …`, heredocs and `$(( ))`); a test committed with its
  code (the pre-commit hook runs `check`); logs under `artifacts/<handle>/`
  (two agents once wrote the same scratchpad `verify.log`, and a green run
  read as red); tests run wisely (step 11); no landing or reviewing its own
  work. A rule it lacks that you would send twice goes there, not into the
  body.
- **Start the next executor when one moves to `review`,** not when its report
  arrives. Three executors finished within minutes of each other and the
  pipeline sat at zero. Keep the next two unblocked, non-overlapping tasks
  picked out ahead of time.
- **Stack the next slice of a chain early.** Owner, 2026-11-04: "Stack next
  slice early: Start the next slice (e.g. tools/studio/plugin now) on the
  previous slice's approved tip under a reservation, instead of waiting for it
  to land." When a slice is approved, launch the next slice's executor. Its
  description says to start on the previous slice's tip (merge that branch
  first), work under its reservation (a `--for` one does not lapse; one
  `launch` or `just tasks reserve` made lapses after an hour, so renew it
  with `just tasks reserve SLUG` within each hour), claim when the previous
  slice lands, and merge the
  previous slice's later changes as they come. Relay each new tip of the
  previous slice, and its landing, to that executor with `--to HANDLE`. Shared
  data still lands first ([RULES.md](../../../RULES.md)
  `workflow/shared-data-lands-first`).
- A foundation task in review (one that a whole tier depends on) is the
  bottleneck. When the executors can't start anything else, add a **second
  reviewer** for the smaller reviews, so the first can finish the foundation.
- Tell reviewers: a verdict message containing the word "git" is refused by
  the worktree sandbox, so rephrase it. The permission classifier also
  refuses a reviewer editing a golden to test a claim ("Security Test
  Removal"). Golden reconciliation is the executor's, on its branch.
- Two branches that each re-record the same golden (`NEAR_FIELD_CHAIN` in
  `tests/fidelity_test.gd`) conflict at merge. Land one; land reports the
  second's conflict to its executor, which merges main and re-records on its
  branch. That is an edit beyond the merge, so it goes back to its reviewers,
  and land verifies the resulting tree.
- Reviewers often can't plant a mutation to prove a test fails without the
  feature: the permission classifier refused it three times (reverting a
  guard, moving a start sector, editing a hull's sensor). Executors post the
  tests that cover the change and their green run on the thread. A reviewer
  checks each new or changed test after the fact, by reading it or with a
  probe, that it would fail with the change reverted, and that the green run
  names it.
- A branch green on its own can go red once merged: two branches each added
  tests to one file (`tests/ui_front_test.gd`) and together passed the
  600-line lint limit. This is another reason for the trial merge. When
  several executors extend one test file, tell them up front to add their
  own suite.
- **Tell reviewers to trial-merge main** (in a detached checkout, then abort)
  when main has moved since the branch last merged it. Two branches that each
  registered a kind in `SimStatus.KINDS` merged "cleanly" into code that
  silently dropped one kind and renumbered the other. Only the trial merge
  caught it.
- **An executor merges main into its branch only when a land post asks for
  it** (a conflict, or a red or uncertified merge with main); land makes every
  other merge itself. A stacked slice merges the previous slice's branch, not
  main. Owner, 2026-11-04: "No re-review on clean merge: A main merge
  without conflicts keeps the approvals; conflicted merges still get
  re-reviewed." A clean merge, or one whose conflicts keep both sides, needs
  no new review: land carries the approvals and posts the merge that kept
  them. One that takes a side or edits a line waits on one re-review: send
  one reviewer, plain or adversarial, to read only its remerge diff and record
  `just land-approve BRANCH --tip SHA --remerge`, which restores every
  approval ([MERGING](../../../docs/workflow/MERGING.md#stamps)).
- A pre-existing gate failure gets fixed on every branch that hits it: four
  branches each patched the same `justfile` line. When the first executor
  reports one, fix it on main (or ask the session that owns it) and tell the
  other executors to leave it.
- Read each executor's report for **pre-existing gate failures** it found
  (for example, the pack shipping `docs/*.json`) and file them as tasks right
  away. They block every build-based check later.
- A reviewer is a separate registration and records its own verdict. The
  stamp that completes the set (the last `land-approve`, a `land-stamps` edit,
  the tip's certificate) starts the landing by itself, detached; `just land
  BRANCH` from any session starts it now. Land validates the stamps and the
  tip's certificate, merges a clean merge without verifying it and moves main
  by compare-and-swap, replanning once when main moved.
  It posts the landing on the task thread and pushes main to `origin`; never
  merge or push by hand. Start what the landing unblocks. A refusal only the
  executor can fix (a conflict with main, a red check of the tip's verify, no
  valid certificate, a task of the branch not in review) goes to the executor's
  pane by itself, once per tree and reason, so
  do not relay it; when land says the executor is not live, run
  `just tasks restore SLUG`. Handle any other refusal from its stated cause;
  never substitute a manual move of main.
- **You set which stamps a task's landing needs**
  ([MERGING](../../../docs/workflow/MERGING.md#stamps)). The default is the
  risk tier's set; `just land-stamps BRANCH` shows it. When a stamp makes no
  sense for the work (a dependency the owner's approved card already asked
  for), waive it with the reason, `just land-stamps BRANCH --waive dependency
  --why '<why>'`; `--require NAME` adds one, `hold:NAME` holds the landing
  until you waive it. The edit is posted on the task thread for the owner.
  It needs the orchestrator role on the branch's own task (or its review
  task), as relaying the owner's approval does, never a reviewer's role there.
  A waiver covers only the patch it was recorded at: waive again after the
  branch changes. The owner stamp moves only with the owner's verbatim words
  (`--owner-said`), and the certificate never. A task in review waiting only
  on stamps nobody is producing reads `blocked` until one arrives.
- **Read the verify record and certificate, not just its exit.** Every required
  fast ID needs valid green evidence or a proven empty native inventory. No
  sim or render TESTS line belongs to the fast plan. A complete nightly needs
  every exhaustive gate and its coverage recorded before it counts as green.
- **Do not land onto checked-out main while an exhaustive run reads it.**
  Let that run finish before changing its checkout. Land handles the candidate's
  required checks and publication through its own captured inputs.
- **Approved branches land as their stamps complete, in any order.** Nothing
  queues them: each landing moves main by compare-and-swap and replans once
  when main moved. Up to three branches held the same ordinal at a time: when
  one lands, land reports each later one's conflict with it (the tape,
  `NEAR_FIELD_CHAIN`), and its executor merges main, takes the next ordinal
  and re-records, an edit beyond the merge that goes back to its reviewers.
  Tell every branch still in flight on its thread each time main's ordinal
  moves.
- **A tier 0 docs-only branch lands at once.** Owner, 2026-11-04: "Docs-only
  lands skip queue: Tier 0 docs-only branches land outside the code land
  queue." When `just merge-risk BRANCH` puts a branch that changes only
  documentation at tier 0, it lands as soon as it is ready: it waits behind no
  code branch and for no machine load. It needs no reviewer: its certificate
  comes from the tip's verify the move to review started, which lands it and
  closes its task.
- **Shared numbering collides.** Every sim branch bumps the rules ordinal, and
  two gear branches picked the same item `code`. Allocate data codes on the
  thread before executors pick them. Don't pre-allocate ordinals: branches
  land out of order (31 landed before 29 and 30), and every branch landing
  after another state change must re-record its tape and `NEAR_FIELD_CHAIN`
  anyway. Rule: take main's ordinal + 1 when land's conflict post has you
  merge main; a second state-changing branch merges main before it lands.
- **Brief 3D hull reviewers with the hull-models review procedure**
  ([REVIEW.md](../hull-models/REVIEW.md)). Critics and tier-3 reviewers cleared
  hulls the owner then rejected on sight (EMP-BB02, COA-F03, SYN-CR01) because
  they sampled, judged outline only, read the builder's captions first and
  trusted IoU against stretched art. Each reviewer's subtask says: review every
  hull in the batch, from `just hull-review ID [GLB]`'s composite; answer the
  ten checklist items for each hull before reading the thread; any item clearly
  off blocks; read `hull-models/taste.md` first; make the composite from the
  exact GLB on the tip under review and quote its header's GLB sha256 in the
  verdict (a changed GLB needs a new verdict). One reviewer per batch is the
  fresh-eyes judge: its subtask body holds only the composite paths and the
  checklist, never the task thread, the builder's posts or a link to them, and
  it is told not to read the thread. Relay every owner verdict on a 3D hull to
  the modeller and have it added to `taste.md`.
- **UI and art image tasks get a fresh visual reviewer before any card is
  posted,** using the ui-design skill's [REVIEW.md](../ui-design/REVIEW.md)
  (UI) and the art rules (pictures). The owner rejected a whole UI batch on
  sight, 2026-10-06: "Was this reviewed before being presented?". The
  reviewer's subtask holds only the snapshots, their `.html` and the
  checklist, never the designer's prompts or thread.
- **Watch for circular tests.** A test rewritten to read its expected value
  from the function under test (`ring0 == ring_radius(...)`) plus lower bounds
  passes any over-correction. Reviewers caught this twice. Ask for the value
  pinned from constants; the reviewer reads it and judges whether an
  over-correction would pass.
- A branch whose owner questions are still open can land with written
  defaults. Don't hold the pipeline for them, but do relay them.
- Resumed reviewers pile up context: one reached about 225k tokens after five
  reviews. Start a fresh reviewer for a big diff rather than feeding it to a
  full one.
- Long `just tasks say` messages are refused by the sandbox. Tell agents to
  post several short ones.
- **A golden FRAME re-bless** (render) needs a reason tied to an approved
  rule, stated in the commit and checked by the reviewer pixel by pixel.
  Rounding-only changes go to the owner.
- **The owner approves a re-bless in words; the executor runs `just bless`.**
  The owner never runs commands. Check the diff yourself, then show the owner
  the exact diff (diff PNG, `frame.diff.png`) and ask for approval in words. An
  approval the owner gives in the Symphony, or in the orchestrator's terminal
  posted verbatim on the task thread (as with `land-approve --owner-said`),
  counts. Approving the change that moved the frame does not approve the
  frame: the owner must have seen that diff (RULES.md
  `workflow/approval-goes-with-the-ask`). The executor runs `just bless "…"`
  and quotes those words in the commit. An agent's own say-so without the
  owner's quoted words does not count. Keep other work moving while you wait.
- A resumed agent (`SendMessage` to its agent id) keeps its context. Prefer
  that to a fresh agent for review changes and follow-up reviews.
- **Check `just merge-risk BRANCH` before briefing the reviewer**, not after the
  verdict. Tier 2 and 3 need two reviewers (one `--adversarial` at tier 3),
  each recording `just land-approve BRANCH`, and `just land` counts them only
  on a task that is still live. A reviewer's board `verdict … approve` moves
  the task to done, so the one approval that closes it strands the second:
  `land` then says "no task". It cost a landing cycle (view/ram-look went
  done, then review/view/ram-look went done, then review-2 was needed). So on
  tier 2 and 3, brief both reviewers to finish with `land-approve` and no board
  approve verdict (`land-approve --verdict changes` posts its note to the
  executor's pane and sends a task in review back to in progress, as a board
  `changes` verdict does; you hear it as a VERDICT), and land with `just land`.
- **An owner-tier change an owner-approved card or ask already asked for** (a
  dependency, a framework, a workspace) is relayed by the lander with
  `land-approve --owner-said '<verbatim quote>'` quoting the owner; never ask again.
- **Any session lands.** The last stamp starts the landing; `just land
  BRANCH` from any session starts it now. `just land-request BRANCH` files the
  review task for a branch that has none of its own.
- **A moved main replans once.** The tip's certificate stands whatever main
  does: land merges again and swaps once more; a second move says retry, and
  `just land BRANCH` retries. The merge is never verified: the verify of main
  follows each landing, and its red goes back to the executors of what landed
  since the last green (`tests/main-red-<tip>`).
- **Keep verify logs under `artifacts/` (ignored).** Writing a tracked or
  untracked source into a captured checkout invalidates its input audit.
- **State-changing branches in review together re-review each other.** Each
  bumps the rules ordinal and re-records the tape, so whichever lands first
  forces the others to merge, re-record and collect both tier 3 approvals
  again on the new patch. Review and land them in ordinal order, and start the
  next one's reviewers only once the one ahead has landed, unless the queue
  would otherwise sit idle.

## Instructions to agents

Owner, 2026-10-02: every instruction to another agent is a board subtask,
`just tasks create PARENT/NAME "title" --parent PARENT --body ...`, whose description
is the instruction verbatim. Never a brief file in `artifacts/`, `/tmp` or a
session scratchpad; `artifacts/orchestrator/` must not exist. A throwaway
export for testing goes under `artifacts/exports/<handle>/`.

## Which way an agent starts

Owner, 2026-11-04: "and also be using more subagents or claude -p to not have
such a high memory usage". A herdr pane keeps its agent's memory until it is
closed, so a Claude executor or reviewer starts in one of the first two ways
below, and a pane holds only work that needs one
([standing rule](../../../RULES.md#workflowclaude-first-agents)). Each gets
its subtask and its prompt as [Instructing agents](#instructing-agents) says.

1. **A subagent**, an Agent launch: the default for a Claude orchestrator. An
   executor or a reviewer takes `isolation: worktree`. That worktree's branch
   is its own (`worktree-agent-…`): an executor renames it to the slug and a
   reviewer checks out the tip it reviews, detached ([work](../work/SKILL.md)
   step 4). It runs inside your process and ends with its work, but also with
   your session: one that stops goes `LOST` and is resumed or handed on
   ([Subagent health](#subagent-health)).
2. **`claude -p`**: for an orchestrator without the Agent tool (a Codex one,
   a script). Start it from the main checkout as a background command
   (Claude: the Bash tool with `run_in_background: true`):
   `claude -p "/work SLUG HANDLE" --worktree NAME --session-id UUID --model M --effort E --permission-mode MODE --output-format json`.
   `--worktree` makes `.claude/worktrees/NAME` (NAME: the slug with `/` as
   `-`) on a branch of its own, as an Agent launch does. `UUID` is a fresh
   `uuidgen`: `claude -p --resume UUID "<the news>"`, from the main checkout
   and with the same `--permission-mode`, resumes it in its worktree. Nobody
   answers its permission prompts, so give it the mode your own session runs
   under (auto); it posts a refusal on its thread verbatim. In that mode board
   messages are delivered. A mode that bypasses permission prompts holds them,
   and a `-p` run drops held ones after five minutes (the Claude Code docs'
   cross-session messaging page, "Non-interactive sessions"), so a board
   worker never runs in such a mode. The run's JSON result is its report.
3. **A herdr pane**, `just tasks launch SLUG --kind K`
   ([Spawning in herdr](#spawning-in-herdr)), only when the work needs one:
   - a Codex agent: the adversarial review of a Claude executor's branch,
     image generation, 3D modelling (`--kind codex`);
   - an agent the owner asked to watch or answer in its pane;
   - work that must outlive your session, or a long game run such as a soak,
     which goes in a pane, never a session's background command
     ([tools guide](../../../tools/AGENTS.md#soaking-the-game-for-leaks)).

How the board knows and reaches each one:

- **Its handle.** You register it for 1 and 2 and pass it in the prompt;
  launch registers it for 3. Its first board command run as that handle
  (`TASKS_SESSION=<handle> just tasks show SLUG`) binds its tool calls to it
  as a heartbeat: a subagent by its agent id, a `claude -p` run or a pane by
  its session id ([Sessions](../../../symphony/docs/TASKS.md#sessions)).
- **Messages to it.** A pane registers its callback
  (`just tasks callback --as HANDLE`) and board news arrives between its tool
  calls; so does a `claude -p` run while it runs (it binds an inbox socket of
  its own). Its registration stays after the run ends, until the board's
  sweep (about five minutes) or a failed delivery retires it, and until then
  a post to that run brings you no `MSG … from board`. So when a `claude -p`
  run ends, read the task's thread before you resume it with the news. A
  subagent inherits your
  socket and registers none: it reads `just tasks inbox --as HANDLE` between
  steps.
- **Messages from it.** Its milestones reach you on the thread through your own
  callback (`--to @orchestrator`). Its report is the Agent result (1), the
  background run's output (2), or its last post on the thread (3).
- **News it waits on.** A subagent, or a `claude -p` run that has ended, has no
  channel: a post to it waits in its inbox. The board checks a post to
  `@executor` only when it is made: one made while the agent still reads live
  (up to 90 s after its last tool call) or while its callback is registered
  brings you no `MSG … from board`, then or later. So news meant for an agent
  that has ended, or may have, goes to you (`--to @orchestrator`), and you
  resume the agent with it: `SendMessage` to its agent id, or
  `claude -p --resume UUID "<the news>"`.
- **A tier 1 fresh reviewer** is a Claude subagent or `claude -p`, with no
  worktree and no handle, given only the text between the markers that
  `just land BRANCH` prints (as its prompt, or on the stdin of `claude -p`). You,
  never the branch's writer, record its answer:
  `just land-approve BRANCH --tip SHA --by subagent:<its agent id, or the claude -p session id> --verdict approve|changes --note "<its lines>"`.

## Spawning in herdr

A pane is for [work that needs one](#which-way-an-agent-starts): a Codex agent,
an agent the owner watches, work that must outlive your session. In-process
subagents end with their orchestrator's session: about 13 executors and
reviewers died with an orchestrator's terminal tab on 2026-10-02. A pane needs
`test "$HERDR_ENV" = 1`. Syntax is the installed binary's (`herdr --help`, the
[herdr skill](../herdr/SKILL.md)).

1. **One command starts a board agent:** create the subtask first (no `--for`),
   then
   `just tasks launch SLUG --kind codex|claude [--model M] [--role executor|reviewer] [--branch B | --detached] [--tab STREAM] [--prompt TEXT]`.
   Always pass `--kind`: without it launch starts Codex. The adversarial
   reviewer of a Claude executor's branch is
   `just tasks launch SLUG --role reviewer --kind codex` (`gpt-6.1-sol` at
   `xhigh`).
   It registers a handle as your child, makes the worktree under
   `star-foundry-wt/` (an executor: a branch named for the slug; a reviewer: a
   detached checkout of main, a worktree of its own), reserves the task for the
   executor (a reviewer takes the reviewer role), puts a pane in the herdr tab of
   its stream (`code` for an executor, `review` for a reviewer; at most 4 panes a
   tab, `code-2` past that, per the
   [herdr skill](../herdr/SKILL.md#how-this-project-uses-herdr)), starts
   Codex (`gpt-6.1-sol`, `xhigh`) by default, or Claude when
   selected, for an executor or a reviewer (retrying `agent_pane_busy`), sends `$work SLUG HANDLE` (Claude:
   `/work`; then `--prompt`'s text, if given), and
   records on the task its handle, role, agent session id, worktree, branch,
   pane and start time. `just tasks show SLUG` lists them under `Sessions:`.
   Works for subtasks: a regular executor on `X/do` defaults to branch `X/do`;
   an instruction subtask (`X/r2`) or reviewer defaults to detached main.
   `--branch B` selects a branch. A branch checked out in another worktree is
   refused, naming that worktree; to continue there, use `--worktree` with that
   path. `--detached` gives a checkout of main for read-only work where no work
   lands. A reviewer launched on an instruction
   review subtask works it as its executor (`/work` claims it) and holds the
   reviewer role on the task it reviews, its parent. `--worktree DIR` uses one that
   exists (made absolute; its branch is what it has checked out). A stream with no
   tab gets one. Reviewers keep a worktree of their own.
2. **Restore** a closed session where it stopped, resuming its conversation:
   `just tasks restore SLUG [HANDLE]` (every closed session of the task, or the
   one named). Every pane closure leaves a `restore: just tasks restore SLUG H`
   line on the task's thread, so nothing is written by hand.
3. **Clean up** with `just tasks cleanup [--dry-run]`: it closes herdr panes whose
   agent is idle with nothing running under it and whose tasks are done or
   cancelled, or that hold nothing live, and idle shell panes; never a pane
   running `verify`, a test run, `land` or `just symphony`, never yours. Run it every
   round instead of sweeping herdr's tab and agent listings by hand. When a task becomes `done`
   or `cancelled` (a verdict, `done`, `move`, a landing) its own sessions are
   cleaned the same way, and a sweep starts by itself in which its worktrees
   go if merged and clean (`just tasks sweep`'s rules; `sf-tasks/sweep.log` in
   the git common dir says what it did); the record and restore line stay.
4. **The instruction is a subtask** (see [Instructions to agents](#instructions-to-agents)),
   and the prompt is just to work it (`launch` sends `$work SLUG HANDLE` to
   Codex, `/work SLUG HANDLE` to Claude). Launch sets the handle in its environment
   and uses the owner's reviewer model (owner, 2026-10-22: "make sure reviews
   are triggered with sol 6.1 or better at xhigh"). Codex reads a
   leading `/work` as its slash-command popup (Enter opened `/worktree`,
   2026-10-05), and `$work` names the skill instead. Text you type into its
   pane later is yours, never the owner's, and the work skill says so.
5. **A herdr agent is a full session,** so the subtask description says only
   the task itself (tests with or after the code, `check`, the targeted tests
   and `precheck`, the step it finishes by, no self-review, and for a stacked slice the
   previous slice's branch): the handle, claim, inbox, thread, merge and
   no-subagents rules are in `/work`. After the initial launch prompt, reach
   it through the thread and its registered native callback. Herdr only starts
   and inspects the pane; it is never the message channel.
6. **Watch** with `just tasks agents` (and the PULL digest); `herdr agent read <handle>`
   only to see a screen. Results arrive on the board thread.
   **Answer a Codex approval prompt yourself when it is clearly justified,
   needed for the agent's task and obvious** (an edit in its own worktree, a
   skill line repointed at the ported command, a build or check its task
   needs). Owner, 2026-10-24: "you don't need to ask me about those \"codex
   asked for\" prompts. That was a clearly justified, needed and obvious
   approval". Bring the owner only a prompt that changes a rule, deletes or
   overwrites what is not the agent's own, or reaches outside its task. Read
   the whole prompt first (`ctrl+a` in its pane shows every file of a patch).
7. **Close only panes you created,** as soon as that agent's work has
   landed or been handed on, and the tab when it empties: `just tasks cleanup`
   does it, and a finished task does it for its own sessions. An agent that
   still holds a live role (a reviewer waiting for a new tip) is closed with
   `just tasks close SLUG HANDLE`, never a bare herdr or hc call: it records
   the closure and its restore line on the thread, and is refused for your own
   pane, one the owner may be looking at, and one with work under it (a
   verify, any test run, a land, `just symphony`, a shell): wait for the work,
   or ask the agent to stop it. A Codex session closed before its thread id
   arrived says so on its thread: launch the task again rather than restore.

## Subagent health

A subagent, or a `claude -p` run, has no monitor. A hook
(`sf-hook tasks-beat`) beats for it on every tool call once its first
board command (`TASKS_SESSION=H just tasks …` or `--as H`) has bound it, and a
foreground `verify` counts as busy for its whole timeout. `LOST` fires only
for a child still holding claimable work (an executor on unfinished work, a
reviewer on a task still in review), and names only that work. A child holding
nothing reads `left` quietly.

Check each `LOST`. If the agent's own task notification (a subagent) or its
background run's end (`claude -p`) says it stopped, resume it with `SendMessage`
or `claude -p --resume UUID`, or hand the task on, and say which on the
thread. If it is still running, its callback (a pane, a running `claude -p`)
or its inbox reaches it between tool calls.

**A waiting agent never polls.** It ends its turn, and you relay the news it
waits on when it arrives: `just tasks say SLUG "<one line>" --to HANDLE` delivers
through that session's registered channel. A subagent, or a `claude -p` run
that has ended, has none: resume it with the news
([which way an agent starts](#which-way-an-agent-starts)). Owner, 2026-10-22:
"They should just get notified when things happen". Ending an intermediate turn
or waiting runs no verification. Executors move to review after `just check`,
the targeted tests and `just precheck`; the full fast plan runs on the tip at
the move to review, and on main after the landing.

## Keeping yourself alive

- Callback registration does not expire. Tool calls keep the session live;
  ending a turn does not require a polling loop or a monitor. On a native
  channel failure, use the retained inbox and repair the registration.
