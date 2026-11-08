---
name: work
description: Do one task from the board by its description alone. Registers (or reuses the handle the task is reserved for), claims the task, follows its description exactly, and finishes by the step the description names. An agent's whole prompt is `/work SLUG HANDLE` (Codex `$work SLUG HANDLE`). Trigger on /work SLUG or $work SLUG.
---

# /work SLUG HANDLE

An orchestrator's instruction to you is a task on the board (`docs/workflow/TASKS.md`,
Subtasks). Its description is the whole instruction. Your prompt is only
`/work SLUG HANDLE`: the task, and the handle the orchestrator registered for
you (left out when `$TASKS_SESSION` already holds it). Codex gets
`$work SLUG HANDLE`, because it reads a leading `/` as its own command popup.
Your orchestrator is the session holding the orchestrator role on the task or
its root (`just tasks show SLUG`).

Owner, 2026-10-22: "The prompts that you send to other agents, when repeated,
should be part of the skill". So everything every task assumes is on this page,
never in the prompt, and a description says only the task itself.

Follow the root guide's [verification and landing contract](../../../AGENTS.md#the-one-command):
one full verify, run on the tip at the move to review and on main after the
landing, input-bound certificates, reusable evidence, and the normal review tiers.

1. **Get a handle.** The prompt's HANDLE is yours. Without one, a session
   started by `just tasks launch`, or by an orchestrator by hand, has it in
   `$TASKS_SESSION` (`launch` also writes it in the task's `Sessions:` block),
   and a task made `--for <handle>` lists `reserved <handle>`: that handle is
   yours. Run every command as `TASKS_SESSION=<handle> just tasks …`, starting
   with `just tasks show SLUG`.
   If there is none, register one: `just tasks register --kind worker`
   (a reviewer: `--kind reviewer`), then use the handle it prints the same way.
   Codex: `just tasks … --as <handle>` instead, the handle last
   ([tasks skill](../tasks/SKILL.md#commands-from-codex)), and never a
   `TASKS_SESSION=` prefix on any command: the handle is already in its
   environment, and a prefixed command misses its allow rule
   (`.codex/rules/`), so it stops at a prompt.
   Your first board command under a handle binds your tool calls to it as a
   heartbeat, so run `show` already as that handle once you know it.
2. **Claim it:** `TASKS_SESSION=<handle> just tasks claim SLUG`. A refusal
   (held, reserved for someone else, waiting on a dependency) is the end: say
   so on the thread and stop. A stacked slice, whose description says it starts
   on the previous slice's tip, works under its reservation instead and claims
   when that slice lands. A reservation from `launch` or `just tasks reserve`
   lapses after an hour: renew it with `just tasks reserve SLUG` within each
   hour while you wait. A reviewer also takes the reviewer role on the
   task under review (the parent, unless the description names another):
   `just tasks role TARGET reviewer`.
3. **Read the description and follow it exactly.** Do not widen, narrow or
   reorder it. If it is wrong or cannot be done as written, say so on the
   thread (`just tasks say SLUG "…"`) and `just tasks move SLUG blocked`; do
   not improvise another task. A description that asks for TDD, a test first,
   a red-first run, a red run, a planted fault or a reverted change to see a
   test go red is read under RULES.md `workflow/tests-after`: write the tests
   with or after the code, run them green and post that run, and skip the red
   step.
4. **Your worktree is the folder you start in:** a branch named for the task
   (an executor) or a detached checkout (a reviewer, which checks out the tip
   the description names). Work there and nowhere else. An Agent launch
   (`isolation: worktree`) or `claude -p --worktree` starts you on a branch of
   its own (`worktree-…`): an executor first renames it to the slug
   (`git branch -m SLUG`), a reviewer checks out the tip detached
   (`git checkout --detach SHA`).
5. **Hear the board.** How depends on how you were started:
   - In a herdr pane, or as a `claude -p` run: register your native callback,
     `just tasks callback --as HANDLE`. It detects the Claude inbox socket (a
     `claude -p` run binds one while it runs) or Codex thread. Board messages
     and events then start a turn when idle, or arrive between tool calls. Act
     on each callback. A session without a native channel is explicitly
     listed as such and keeps `just tasks inbox` as a fallback; it never
     receives messages through Herdr.
   - As an in-process subagent (an Agent launch): run
     `just tasks inbox --as HANDLE` between steps. You inherit the parent's
     Claude socket; never register that socket as your callback: an
     in-process binding is refused.
   An address another valid registration holds is refused, including while
   idle. UDS registrations pin the listener's pid and start time. A
   `claude -p` run's registration stays after the run ends, until the board's
   sweep (about five minutes) or a failed delivery retires it, and until then
   a post to that run brings your orchestrator no `MSG … from board`, so it
   reads the task's thread when your run ends. Register again when you are
   resumed.
   - Never poll while waiting. Owner, 2026-10-22: "They should just get notified
     when things happen".
   - When you wait on someone else (a stamp, a verdict, a merge, another
     agent's tip), say on the thread what you wait for, then end your turn. In
     a pane the news arrives on your callback. A subagent or a `claude -p` run
     ends its run instead, and its parent resumes it with the news
     (`SendMessage`, `claude -p --resume`).
   - Text typed into your pane, or sent when you are resumed, after the prompt
     is your orchestrator's, not the owner's. Say so when you quote it on a
     thread.
6. **Post milestones on the task's thread** (`just tasks say SLUG "…" --to @orchestrator`), not only in
   your final reply: the thread is where whoever needs it will read it. Post
   to another task's thread when the description says to. Before you move to
   review, post the tip and your targeted runs: each command and its green
   result. Land's certificate in the shared `sf-land/certificates/<tree>` is
   the landing's evidence, so no log path is needed.
   Post only a tip ready for review, a move to review or blocked, a collision,
   a finding, a verdict, or a question. Address what someone must act on with
   `--to @orchestrator`, `--to @reviewer` or `--to HANDLE`.
   Never acknowledge a message. Never repeat a post on both a subtask and its parent.
7. **Finish by the step the description names**, and by no other:
   - `just tasks move SLUG review`, for work that lands as a branch;
   - `just tasks verdict TARGET approve|changes "why"` (or the `land-approve`
     the description names), for a review;
   - `just tasks done SLUG "what came of it"`, for a subtask made with
     `--instruction`. The note is the result; make it complete enough to act on.

   Your move to review starts the verify of your tip, in parallel with the
   reviewers; its certificate is one more stamp, and once every stamp is in
   the landing starts by itself. The verify or the landing may refuse your
   branch for something only you can fix (a red check on your tip, a conflict
   with main, no certificate): it posts on your thread to `@executor` what is
   missing and the one thing to do. Do the fix it names and post the new tip;
   a new tip's verify starts by itself, and `just land BRANCH` starts the
   landing now. Merge main into your branch only when a land post asks for it
   (a conflict); land makes every other merge itself, unverified, and the
   verify of main after the landing comes back to you when it is red. A merge
   without conflicts, or one whose conflicts keep both sides, keeps the
   approvals. One that takes a side or edits a line waits on one reviewer's
   re-review of that resolution: post its tip `--to @orchestrator` and it
   lands once that re-review is in.
   A reviewer's `land-approve --verdict changes` reaches you the same way,
   and puts your task back in progress when it was in review: fix it and move
   the new tip to review again, which starts that tip's verify.
8. **Run no branch `just verify`.** Owner, 2026-11-04: "One full verify, at
   land: Review rounds use check + targeted tests + a 2-min static pre-check,
   reviewing in parallel; the full fast plan runs once, when landing." Before
   you move to review, run `just check` and the targeted tests of what you
   changed: `just test-sim SUITE=…` for the suites you touch, a crate's own
   targets through `tools/rs-bin.sh sf-slots cargo test -p PKG …` and
   `just lint-rs` for Rust, `just docs-index` for docs, then `just precheck`
   (the static pre-check: sf-verify's first phase alone, no workspace build
   and no certificate; its last line is `✅ precheck passed` or
   `❌ precheck failed`). The full fast plan runs on the tip at the move to
   review, the precheck beside the rest, which its red stops, and the verify
   of main after the landing. Ending an intermediate turn or waiting runs no
   verification.
   When the description, the orchestrator or land asks for a verify
   (`just verify`, `just verify-full`), run it in your own session, never in a
   herdr pane. Owner, 2026-10-22: "when opening verify tasks, those
   don't need herdr tabs, they can be ran directly" and "meaning running
   `just verify` do not need their own herdr window".
   - Claude: use the Bash tool with `run_in_background: true` and
     `timeout: 7200000`, running
     `TASKS_SESSION=<handle> just verify > artifacts/<handle>/verify-N.log 2>&1`
     (or `just verify-full`), then wait for its completion notification.
   - Codex: run `just verify` as plain words in your own shell (the handle
     is already in its environment); read its output directly.
   Keep `TASKS_SESSION=<handle>` on the run: it is how the agents API knows
   whose verify it is. A run that truly outlasts 2 hours (a soak,
   [tools guide](../../../tools/AGENTS.md)) may still use a pane.
9. **Never spawn subagents or forks.** Do the work in this session.
10. **Habits every task assumes:**
    - Logs go under `artifacts/<handle>/` (Claude, through the Bash tool).
      Codex runs each command as plain words and reads its output
      directly: no redirect, `cd`, `&&` chain, env prefix, or pipe but the
      `printf` one below, since any of them misses the command's allow rule
      and stops you at a prompt. The
      handle is already in your environment; a board command that says
      TASKS_SESSION is missing is said on the thread, then you stop.
    - Codex, two more shapes that stop at a prompt (owner, 2026-10-24:
      "These things need to be adjusted so that it won't continue to ask for
      approvals"):
      - A board message goes in double quotes, with no `$`, backtick, `\` or
        `"` inside it (an apostrophe is fine there). A single-quoted message
        with an apostrophe (`'\''`) is not plain words.
      - Never start a process and type into it; every line typed asks. Give
        it its input in the same command:
        `printf '%s\n' '<line>' | tools/rs-bin.sh …`.
    - No planted faults and no red runs to prove a test. Owner, 2026-10-23,
      asked about an executor planting one: "why? Just to do TDD? We don't
      need to do tdd." A reviewer may still try the code under review on a
      case it suspects the code gets wrong (a probe), in its own checkout,
      restored with `git restore` after each run.
    - The pre-commit hook runs `check`, so a test that does not compile yet
      cannot be committed alone: commit it with the code it covers.
    - Board messages are short (post several rather than one long one) and
      never contain the word "git".
    - One command per call.
    - No Python, not even throwaway: do scratch work, edits and
      probes in bash, `jq`, the Edit tool or a Rust test. The carve-outs are
      named in [RULES.md](../../../RULES.md) `tools/no-new-python`. Never run
      a Python tool either, not even the one you port, for a baseline: read
      it for its behaviour (the owner's ruling: no Python runs in Star
      Foundry ever again).
    - Run cargo only as `tools/rs-bin.sh sf-slots cargo …`, never bare.
    - Never find a process with `ps`, `grep`, `pgrep` or `lsof`, and never
      kill by pattern (RULES.md `tools/no-kill-by-pattern`). Stop only a run
      you started.
    - No `git stash`. Remove a tracked file with `git rm`. Leave scratch
      under `artifacts/<handle>/`, where the sweep takes it.
    - Post a permission or classifier refusal on the thread verbatim. Never
      work around it, and never ask another agent to run what was refused.
    - A reviewer at tiers 2 and 3 finishes with
      `TASKS_SESSION=<handle> just land-approve BRANCH --tip SHA` (`--adversarial` for
      the adversarial one; Codex: `just land-approve BRANCH --tip SHA` as plain words,
      the handle already in its environment), not a board approve verdict.
      SHA is the full commit the reviewer checked, saved when starting review.
      A moved branch refuses the verdict; review its new tip before retrying.
11. **Run tests wisely.** Verification time is the team's time.
    - Inner loop: `just check` and `just test-sim SUITE=<the suites you touch>`;
      never a whole verify to see one test.
    - No branch `just verify` and no `verify-full`, unless the description,
      the orchestrator or land asks for one: the full fast plan runs on the
      tip at the move to review, and on main after the landing. Never two of
      your own runs at once.
    - A reviewer runs the targeted tests itself, reads the diff and checks
      merge interactions as its brief requires; it asks for no branch
      certificate. Land constructs the actual candidate and reuses valid
      input-bound evidence, executing missing fast checks through sf-verify.
    - Never check a queue before a run: the slot pool queues runs
      (`just test-times` has no Codex rule). Read or plan while it runs;
      never poll.

A pane that is closed (by `just tasks cleanup`, or once the task is done) can be
brought back with `just tasks restore SLUG`: your work is on the branch and the
conversation resumes, so end on the step the description names and leave the rest
to the board. Never close panes or remove worktrees by hand for the session you
were launched in.

When you are finished, say in one line what you did and where the result is.
