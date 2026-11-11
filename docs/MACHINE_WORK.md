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

Each server scans its local repository's native records once per existing board
tick; HTTP pages and the feed share the cached result. The reader is an already
compiled `sf-slots` at the project's prepared `.target/bin/sf-slots` path. A direct
host process may supply an absolute `SF_MACHINE_WORK_READER` instead. The selected
path is retained; sampling never searches caches or invokes a build or source hash.
A missing/unsupported reader or invalid response leaves observations unknown.
Normal tool commands prepare the default link through the existing binary cache.

Configuring another machine's ID supplies advice, not telemetry. An unobserved
machine stays unknown. This feature adds no remote worker, reporting transport,
board persistence or scheduler.
