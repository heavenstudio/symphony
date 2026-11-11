# Machine build and test work

Task cards, task detail and agents share build/test phase labels. Queued means an
actual resource wait for known build/test work; running requires a successful
native spawn. Coding, review, setup, generic commands and slot reservations do
not become build/test execution. A resource phase does not change a task's lifecycle.

The machine panel shows each observed operation and its machine. Explicit native
completion retires that operation only. Missing, unreadable, abandoned or expired
evidence retains last-known work and uncertainty. Local native inventories are
always partial: even an empty one has **unknown occupancy**, never free capacity.
An expired phase is labelled stale. A broken browser feed marks retained evidence
disconnected. Producer phase timestamps are display information, never freshness.

The task pipeline places in-progress work in **Build** or **Test** when connected,
current evidence identifies its queued or running operation. Positive evidence in a
partial inventory can place a task while occupancy stays unknown. Running work wins
over queued work, then Test over Build. Expired, conflicting or disconnected evidence
keeps the task in its lifecycle column with the last observed phase chip.

Open **Advisory targets** to find a machine's installation ID. Configure optional
targets in the project's `symphony.toml`, using that ID as the table key:

```toml
[board.machine_wip."replace-with-machine-id"]
build = 2
test = 1
```

Each target applies to that machine alone. Missing configuration means no
recommendation; unavailable targets display as unknown, never zero. An explicit
zero is also advice. Orchestrators decide when to start work. These values do not change native
slot admission, Cargo room/lock controls, cancellation or verification certificates.

One opaque ID is persisted as `machine-id` in the existing Symphony machine folder
(`SYMPHONY_MACHINE_DIR` in private fixtures). Hostname is only a display label.
An unreadable or malformed existing ID leaves identity unknown and is not replaced.
Task attribution requires explicit, unambiguous local session context and is bound
once for an operation; later role changes do not reassign it.

Each server scans its local repository's records once per existing board tick;
HTTP pages and the feed share the cached result. A project's own reader wins: an
already compiled `sf-slots` at the project's prepared `.target/bin/sf-slots` path,
or an absolute `SF_MACHINE_WORK_READER` a direct host process supplies. The selected
path is retained; sampling never searches caches or invokes a build or source hash.
A missing/unsupported reader or invalid response leaves observations unknown.
Normal tool commands prepare the default link through the existing binary cache.
A project that names no reader of its own and has none prepared is read by the
built-in reader, from its run records.

## Run records

`tools/cargo.sh` records each build or test run it starts: `build`, `check`,
`clippy` and the client's `symphony-ui dist|clippy|size` are builds; `test` and
`nextest` are tests. So are `just test-rs`, `just lint-rs`, `just client-update`,
`just verify` and the build `tools/rs-bin.sh` makes before each run. The record is
one file in `sf-runs/` in the git common dir, named for the run: a first line,
written before Cargo starts, with its kind, the wrapper's pid, the checkout, its
branch and tip, the `TASKS_SESSION` handle, the command and, for a launcher's
build, the binary it builds; then an end line, `{"exit":N,"seconds":S}`, appended
when Cargo ends. The wrapper reads git's files itself and writes with shell
builtins: it starts no process for the record. Cargo runs as its child with its
arguments, streams, limits and status; a Cargo killed by a signal kills the wrapper
with it. A run inside a recorded run is not recorded again, and a run whose record
cannot be written is Cargo alone. Other commands are Cargo itself, as before.

The built-in reader lists `sf-runs/` at each tick. A run with no end line whose
wrapper is alive (the same process: the kernel dates its start no later than the
record) is going, one running operation of this machine. A run whose wrapper is
gone without an end was cut short; its record is removed. The reader's inventory
is partial (`incomplete`): it says what this project's runs are doing, never what
else occupies the machine. A project in which no run was ever recorded has no
`sf-runs/` and no inventory: the board draws **Build** and **Test** as one narrow
rail saying so once, until its first run.

A run is attributed once, when first seen: to its `TASKS_SESSION` handle, else the
one handle bound or launched in its checkout; then to the open task that handle
holds as executor or reviewer (an instruction subtask gives way to its held
parent; of several, the one the run's branch is named for), else to the open task
named for its branch. Nothing else is guessed. A reviewer's run therefore shows as
a chip on the card in **review**: only in-progress work moves to Build or Test, and
a card that left review while its reviewer tests would hide who holds it.

Each task keeps its last finished run (command, exit status, duration, tip) for a
week; older ones are removed as a newer one finishes. A launcher's build is never
kept, and a run no task claims is removed after a day.

Configuring another machine's ID supplies advice, not telemetry. An unobserved
machine stays unknown. This feature adds no remote worker, reporting transport,
board persistence or scheduler.
