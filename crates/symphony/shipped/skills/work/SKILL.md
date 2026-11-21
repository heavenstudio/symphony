---
name: work
description: Do one task from the board by its description alone, a stage at a time: start (claim it, then question it), build, and finish by the step the description names. An executor's whole prompt is `/work SLUG HANDLE` (Codex `$work SLUG HANDLE`); a reviewer's is the review-task skill's. Trigger on /work SLUG or $work SLUG.
---

# /work SLUG HANDLE

Do the one task the prompt names, by its board description: its scope, its acceptance and the
step it finishes by. The project's instructions come first: its guide (`AGENTS.md`), the rules it
names, the guide of each package the task touches, and `project.md` beside this file where the
project has one, its standing instructions for an executor. This skill supplies the board's
mechanics, not a build or approval policy.

1. Reuse HANDLE from the prompt, the task's reservation or `TASKS_SESSION`; with none, register
   with `just tasks register --kind worker`. Pass it on every board command as
   `just tasks … --as HANDLE`, the handle last: it binds the heartbeat, and a command with an env
   prefix (`TASKS_SESSION=HANDLE just tasks …`) stops at a prompt.
2. Read `just tasks show SLUG --as HANDLE`, then only the page of the stage you are at, beside
   this file. Each page names the next.
   - Not claimed by you yet: [start.md](start.md).
   - Claimed and questioned: [build.md](build.md).
   - Built, or back from a verdict or a land: [finish.md](finish.md).

The pages' commands take the board's own edges: `claim`, `block`, `request_review`,
`complete_subtask` and `close_covered`. Where a custom workflow holds the task, a command that
cannot take the edge for you refuses it as `explicit_action_required`: then
`just tasks workflow actions SLUG --as HANDLE` names each edge that leaves where it stands and
what stops you now (`--json` adds its operation), and `just tasks workflow apply SLUG EDGE
--revision N --as HANDLE` takes one, N the revision those actions named, an input the edge needs
given as `--input NAME=TEXT`. One refused as `stale_task` moved since: read its actions again.
