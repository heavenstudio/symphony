---
name: work
description: Do one task from the board by its description alone, a stage at a time: start (claim it, then question it), build, and finish by the step the description names. An executor's whole prompt is `/work SLUG HANDLE` (Codex `$work SLUG HANDLE`); a reviewer's is the review-task skill's. Trigger on /work SLUG or $work SLUG.
---
<!-- symphony-stamp: 976449b2fcb71c21 files=SKILL.md,start.md,build.md,connected.md,finish.md,rare-endings.md -->

# /work SLUG HANDLE

Do the one task the prompt names, by its board description: its scope, its acceptance and the
step it finishes by. The destination project's instructions come first (its tools, permissions,
staffing, review, verification and landing); this skill supplies the board's mechanics, not a
build or approval policy.

Those instructions are the project's guide (its `AGENTS.md`), the rules it names and the guide of
each package the task touches, and `project.md` beside this file where the project has one: its
own standing instructions for an executor. Read them before the stage page of step 3.

1. Reuse HANDLE from the prompt, the task's reservation or `TASKS_SESSION`; with none, register
   with `just tasks register --kind worker`.
2. Pass it on every board command as `just tasks … --as HANDLE`, the handle last, Claude and
   Codex alike: it binds the heartbeat, and no allow rule passes a command with an env prefix
   (`TASKS_SESSION=HANDLE just tasks …` stops at a prompt). Check the active command permissions
   as well.
3. Read `just tasks show SLUG --as HANDLE`, then only the page of the stage you are at, beside
   this file. Each page names the next.
   - Not claimed by you yet: [start.md](start.md).
   - Claimed and questioned: [build.md](build.md).
   - Built, or back from a verdict or a land: [finish.md](finish.md).

A task moves by its workflow's edges, each named here by its operation: `claim`, `block`,
`request_review`, `complete_subtask` and `close_covered`. Where the board's own workflow holds
the task, each is the command its page gives. Where a custom workflow holds it, a command that
cannot take the edge for you refuses it as `explicit_action_required`: then `just tasks workflow
actions SLUG --as HANDLE` names each edge that leaves where it stands, with its label, destination and what stops you now (`--json` adds its operation), and
`just tasks workflow apply SLUG EDGE --revision N --as HANDLE` takes one, N the workflow revision
those actions named, an input the edge needs given as `--input NAME=TEXT`. One refused as
`stale_task` moved since you read it: read its actions again.
