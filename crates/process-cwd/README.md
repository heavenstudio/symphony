# process-cwd

The working directories of running processes, read through `lsof`, for sweeps that must leave a tree some process still stands in; and children whose pipes no other thread's child can hold open, started without a fork on macOS (elsewhere by std's spawn), with an async wait that kills the child if the wait is dropped; and `session`, the variables that tie a process to the session that started it and mark how it was asked for, which a child keeps none of once `sessionless` and `unmarked` have run on its command. Unix only.

One of the helpers of Symphony, a document, review and task workspace. Each helper has a version of its own, from 1.0.0: a release that breaks a name it exports is a new major version, and any other change is a minor or a patch.

A public Rust item a known consumer calls stays working beside its replacement for at least one release, without `#[deprecated]` because consumer lints deny warnings. Its removal is a separate task for a later release, with read-only evidence of consumer migrations and the consumer-API compile list updated.

MIT licensed: the terms are in the LICENSE file beside this README.
