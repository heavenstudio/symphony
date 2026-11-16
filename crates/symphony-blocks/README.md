# symphony-blocks

The blocks a Symphony plugin renders to and the data its seams return, as the server sends them and the client draws them: document content as data (`Block`, `Span` and what they hold, never rendered HTML), media and their viewer, a board field's value, a task's gate, a home region, a card's pictures, a navigation entry and a board action's offer. Serde only, so one definition builds for a server and a plugin's process, and is written out as the client's TypeScript.

Part of the plugin kit of Symphony, a document, review and task workspace: what a project's own code links to extend it, with `symphony-plugin` and `symphony-docs`. The three always carry one version, from 1.0.0: a release that breaks a name they export is a new major version, and any other change is a minor or a patch. A new kind of block, text span or media, or a new word one carries, is a minor: those lists are `#[non_exhaustive]`, so a match on one ends with `_`.

MIT licensed: the terms are in the LICENSE file beside this README.
