# Build

Implement under the destination project's implementation, testing and documentation guidance.

- Changed behaviour gets meaningful tests, edge cases included. Fix an implementation defect
  before considering any change to an expectation or a limit.
- A test never fails because the machine is loaded: it bounds a cost by the CPU the work took or
  a count of it, waits on its condition with a wall-clock backstop only a stall reaches and that
  says the load average when it fails, and keeps elapsed time only where the behaviour's promise
  is elapsed time.
- Delegation, model choices and concurrent runs follow the task and project policy. Keep every
  child process owned through shutdown and reaping. Use private fixtures for tests, and follow
  the project's policy for shared services and live data.
- This skill requires no language wrapper, verification binary, certificate scheme or landing
  command, and no recipe exists because another project has it: use the project's configured
  entrypoints and review rules.

## Evidence is the gate

The gate of a task or a slice, a feature or a fix, is evidence that what it changed works, not
the full suite: the tests closest to the change and the project's lint, posted with their exact
commands and results and any relevant limitation, plus what the kind of change calls for (before
and after screenshots of a changed page, a measurement before and after). That report is what
review judges. Higher risk asks for stronger evidence, never for the whole suite. The full
verification runs on the project's main line on its own schedule, such as a release run or a
nightly, and a red run there is filed as a task of its own. The project's guidance names its
focused checks, its stamp and that schedule.

## Milestones

Post milestones only, with `just tasks say SLUG "text" --to @orchestrator --as HANDLE`: a tip
ready for review, a move to review or blocked, a collision, a finding, a verdict or a question.
Address action to the role or handle that must act. No acknowledgements, and a milestone on one
thread, never on both a subtask and its parent. Keep the board true. Messages, waiting and an
owner's stop: [connected.md](connected.md).

Next: [finish.md](finish.md).
