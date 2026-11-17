# symphony-args

Command-line arguments with stable option matching and refusals: the parsing Symphony's commands share, which names the command a mistyped word was nearest to.

One of the helpers of Symphony, a document, review and task workspace. Each helper has a version of its own, from 1.0.0: a release that breaks a name it exports is a new major version, and any other change is a minor or a patch.

A public Rust item a known consumer calls stays working beside its replacement for at least one release, without `#[deprecated]` because consumer lints deny warnings. Its removal is a separate task for a later release, with read-only evidence of consumer migrations and the consumer-API compile list updated.

MIT licensed: the terms are in the LICENSE file beside this README.
