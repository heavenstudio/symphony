# symphony-api

Symphony's wire types: the JSON its server sends and its client reads, serde only, so one definition builds for the server and for wasm32. The blocks a plugin renders to come from `symphony-blocks`, re-exported here under the same names.

One feature, off by default, removes data: `skip-board-fields` reads a `Task` without its plugins' board fields (`Task::fields` reads as empty, so a task written back carries none) and so builds no reader for their values. Symphony's browser client turns it on in its wasm32 dependencies, as it draws no field; a crate that reads a task's fields leaves it off.

Its version, below 1.0, is the product's: `symphony-app`, `symphony-board`, `symphony-server`, `symphony-chrome` and `symphony-api` carry one version, require each other at exactly that version, and promise no name: any release may change one. What a project's own code links to extend Symphony is the plugin kit, `symphony-plugin`, `symphony-blocks` and `symphony-docs`.

MIT licensed: the terms are in the LICENSE file beside this README.
