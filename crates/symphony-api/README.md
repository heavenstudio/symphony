# symphony-api

Symphony's wire types: the JSON its server sends and its client reads, serde only, so one definition is the server's and, generated as TypeScript, the client's. The blocks a plugin renders to come from `symphony-blocks`, re-exported here under the same names.

Its version, below 1.0, is the product's: `symphony-app`, `symphony-board`, `symphony-server`, `symphony-chrome` and `symphony-api` carry one version and require each other at exactly that version, without a general API stability promise. Known consumer calls follow the two-release rule below. What a project's own code links to extend Symphony is the plugin kit, `symphony-plugin`, `symphony-blocks` and `symphony-docs`.

A public Rust item a known consumer calls stays working beside its replacement for at least one release, without `#[deprecated]` because consumer lints deny warnings. Its removal is a separate task for a later release, with read-only evidence of consumer migrations and the consumer-API compile list updated.

MIT licensed: the terms are in the LICENSE file beside this README.
