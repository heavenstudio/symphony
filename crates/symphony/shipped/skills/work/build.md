# Build

Implement under the project's implementation, testing and documentation guidance, through its
configured entrypoints: no recipe exists because another project has it.

- Changed behaviour gets meaningful tests, edge cases included. Fix an implementation defect
  before considering any change to an expectation or a limit.
- A test never fails because the machine is loaded: it bounds a cost by the CPU the work took or
  a count of it, waits on its condition with a wall-clock backstop only a stall reaches, and
  keeps elapsed time only where the behaviour's promise is elapsed time.
- Keep every child process owned through shutdown and reaping. Use private fixtures for tests,
  and follow the project's policy for shared services and live data.
- A log or scratch file you write in your scratchpad is named by your handle: every in-process
  agent of one orchestrator shares that folder.

## Evidence is the gate

The gate of a task or a slice, a feature or a fix, is evidence that what it changed works, not
the full suite: the tests closest to the change and the project's lint, posted with their exact
commands and results and any limitation, plus what the kind of change calls for (before and
after screenshots of a changed page, a measurement before and after). That report is what review
judges. Higher risk asks for stronger evidence, never for the whole suite, which runs on the
project's own schedule.

## Milestones

Post milestones only, with `just tasks say SLUG "text" --to @orchestrator --as HANDLE`: a tip
ready for review, a block, a collision, a finding or a question, addressed to the role or handle
that must act. No acknowledgements, and a milestone on one thread, never on both a subtask and
its parent. Messages and waiting: [connected.md](connected.md).

Next: [finish.md](finish.md).
