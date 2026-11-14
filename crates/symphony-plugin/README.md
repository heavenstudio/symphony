# symphony-plugin

What a Symphony plugin implements: a value implementing `Plugin`, whose seams take and return plain data (paths, text and the blocks in `vocabulary`), never a Symphony library type. A host binary compiles the plugin in, or the plugin answers from a process of its own: `serve(&PLUGIN)` answers the line protocol on its stdin and stdout. `error` is the block a host shows in a component's place for its problem, and `testing::Host` asks a plugin's components and card kinds as a host does, through the protocol's requests and answers, for the plugin's own tests. Every struct of `protocol` is `#[non_exhaustive]` and made by its constructors (a request's `new`, `CardData::of`, `PageData::of`), so a field a later minor version adds breaks no build.

Part of the plugin kit of Symphony, a document, review and task workspace: what a project's own code links to extend it, with `symphony-blocks` and `symphony-docs`. The three always carry one version, from 1.0.0: a release that breaks a name they export is a new major version, and any other change is a minor or a patch.

MIT licensed: the terms are in the LICENSE file beside this README.
