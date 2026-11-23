---
name: create-task
description: File a task on the board so an executor can work from the brief alone, without duplicating existing work or missing a required field. Trigger on "file a task", "create a task", "add this to the board", or /create-task.
---

# File a task on the board

Read the project's task conventions first. `just tasks create --help` says the arguments; this
page says how to prepare the brief. Not attached to the board yet, get a handle with
`just tasks register`, and pass it as `--as HANDLE`.

1. **Does this need a task at all?** A problem met once (a one-time migration, a cleanup, a
   repair of one machine's state) is solved by doing it: run the commands or a throwaway script,
   say what was run and what came of it, and stop. File a task for what will be built on, run
   again or must be reviewed, in the smallest form that solves the problem.
2. **Look before filing.** Run `just tasks list --all` and `just tasks search "WORDS"` with the
   work's own words. An open task that covers it gets a post (`just tasks say`), not a second
   task; a finished one is named in the new brief.
3. **Pick the slug**, `area/name` (`app`, `tests`, `tools`, `docs`, `skills`, …): it becomes the
   branch name, so keep it short and distinct. **Name its kind** with `--kind`, what its work is:
   `feature`, `fix`, `red`, `tests` or `docs`. The board never reads one from the slug, and
   Reports groups the landed tasks by it, a task with none under No kind.
4. **Priority and stage.** `--priority` is 0 (most urgent) to 4. `--stage` is `ready` (the
   default, work with no design step), `design` or `concept` (an owner-signed design document is
   still needed, which agents never sign off themselves) or `backlog` (a parked follow-up).
5. **Links.** `--depends SLUG` names a task that must exist already and be done first, `--link
   docs/X.md#Heading` the spec section it implements, `--parent SLUG` the larger task it is under.
6. **Write the brief** to a file outside the tree (never committed) and pass it with
   `--body-file`. Give the executor everything it needs without the thread:

   ```
   what and why: the measurement or evidence that motivated it
   spec: docs/X.md#Heading, and the approved cards it builds on ([x], [r], [a] only)
   pending: what must not be built yet
   touches: files and areas it changes, and tasks it must not collide with
   acceptance: observable behaviour and error cases
   evidence: the tests that cover the change and their green run, posted on the thread, and
     what the kind of change calls for (before and after screenshots of a changed page)
   gates: the focused checks the change needs, by the project's configured entrypoints (the
     tests closest to it and its lint, never the full suite), and its landing prerequisites
   rulings: owner answers that apply, quoted as already approved, never proposed again
   communication: milestones only (a tip ready for review, a block, a collision, a finding, a
     question), addressed --to @orchestrator, --to @reviewer or --to HANDLE
   ```

7. **File it**: `just tasks create SLUG "title" --kind KIND --priority N --stage STAGE --link …
   --depends … --parent … --body-file PATH --as HANDLE`. Its line says who orchestrates it, or that it waits
   for an orchestrator. Tell the owner the slug in one line. Starting an executor on it is a
   separate step, the task-orchestrator skill's.
