---
name: review-task
description: Review one task in the board's review column, independently of its author: read its diff against its recorded evidence, run only what that leaves open and record a verdict naming the tip read. A reviewer's whole prompt is `/review-task SLUG HANDLE` (Codex `$review-task SLUG HANDLE`), never /work. Trigger on /review-task SLUG or $review-task SLUG.
---
<!-- symphony-stamp: 69648f43060d8201 files=SKILL.md -->

# /review-task SLUG HANDLE

Judge one task in review, independently of whoever wrote, executed or requested it. The
destination project's review rules come first: its reviewer families and models (`just tasks
limits` names the reviewer's), its stamps, any checklist it provides for the changed languages or
domains, and any further verification it requires. This skill supplies the board's mechanics; it
assumes no certificate or landing system.

Those rules are the project's guide (its `AGENTS.md`), the rules it names and the guide of each
package the change touches, and `project.md` beside this file where the project has one: its own
standing instructions for a reviewer. Read them before step 3.

1. Reuse HANDLE from the prompt; with none, register with `just tasks register --kind reviewer`.
   Pass it on every board command as `just tasks … --as HANDLE`, the handle last. A session with
   its own channel (a terminal of its own) registers it: `just tasks callback --as HANDLE`.
2. SLUG is the task to judge, itself, never a subtask: take the reviewer role on it, unless you
   hold it already: `just tasks role SLUG reviewer --as HANDLE`. Refused the role, stop and say why on
   the thread `--to @orchestrator`; never pass `--beside` yourself.
3. Read `just tasks show SLUG --as HANDLE`, its thread with every earlier review on it, and the
   whole diff of the tip it names against the default branch.
4. Read its recorded builds and tests at that tip, beside the thread's evidence:
   `just tasks runs SLUG --as HANDLE` (`--tip SHA` for another commit) prints each command it ran
   there with its newest result, the `tests` stamp's reading, the project's `[verdicts] check`
   with its run there or none, and its runs at other commits. A run recorded green at the tip is
   run: never build or run it again, and never the full suite. Build or run only what the record
   does not cover and reading cannot settle: a doubt you name, with the smallest target that
   answers it; a mutation, to see that a test can fail; a run the executor left out. Green is the
   record's to say, never the executor's word: where nothing recorded a run the evidence names,
   ask for it in a changes, or in a project that records none rerun the focused tests yourself. A
   log or scratch file you write in your scratchpad is named by your handle: every in-process
   agent of one orchestrator shares that folder, and two writing one name read each other's
   output.
5. Judge, naming what is missing in a changes:
   - whether the change is correct and does what the brief asks, its documents included;
   - whether its tests cover the edge cases, and whether a test or a check was loosened to pass;
   - whether the evidence is good enough for the change's risk: higher risk asks for stronger
     evidence, never for the whole suite;
   - whether a test it adds could fail on a loaded machine (a wall-clock bound where a CPU bound,
     a count or a wait on its condition belongs): that is a changes;
   - whether the change is bigger than what it solves (a new mechanism where an existing one
     serves, more files, options or pages than the ask needs, a committed tool for a problem met
     once): name the smaller form, which may be "once and done", as a finding;
   - each gate decision on it (`GATES` in `just tasks stamps SLUG --as HANDLE`, a `gate …` line
     on the thread, recorded by its orchestrator alone): for a raise, that the cut came first,
     that the commit raising the limit is its own with an account that is true (what grew, what
     was taken back, the number measured), and that the number the tip holds is the one decided;
     for a waiver, that it names the tip you read. A limit raised in the diff that no raise names
     is a changes: the gate is its orchestrator's to give way, never its executor's.

   That the default branch moved since the tip the evidence names is no finding: never send a
   branch back only for that. Ask for a merge only by naming what the default branch gained that
   breaks the change or that the change breaks.
6. Record the verdict on SLUG, a round of its review: `just tasks verdict SLUG approve
   "FULL_SHA: why" --as HANDLE`, naming the whole tip you read (the approve is the task's stamp:
   SLUG becomes `done`, and the default landing lands that commit and no other), or
   `just tasks verdict SLUG changes "what and where" --as HANDLE`. The board refuses the verdict
   of a session that ever executed SLUG or a slice of it (a subtask without `--instruction`).
   Never judge a task you executed, and never fix the branch yourself: a defect is a changes.
   The verdict's text lists what you checked, what you ran with its result, each gate decision
   at the tip (a raise you judged, a waiver you saw), and each defect with its file and line and
   why it matters. Approve only what you would land; a nit is a follow-up
   named in the text, never a reason for a changes.
7. After a changes, or a land that sent SLUG back to review because your approve does not name
   its tip, you stay its reviewer: the next round is yours. Resumed with a new tip, read the
   thread since your verdict and judge that tip from step 2.

An in-process reviewer reads `just tasks inbox --as HANDLE` between steps; an owner's `MOVE`
ending `it moves no further: stop work on it` means stop. Post the verdict, never an
acknowledgement, and say in one line what you judged and the verdict.
