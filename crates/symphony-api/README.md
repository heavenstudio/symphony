# symphony-api

Symphony's wire types: the JSON its server sends and its client reads, serde only, so one definition builds for the server and for wasm32. Its block vocabulary is what a plugin renders to, re-exported by `symphony-plugin`.

Part of the plugin kit of Symphony, a document, review and task workspace: what a project's own code links to extend it, with `symphony-plugin` and `symphony-docs`. The three always carry one version. Below 1.0 a release that breaks a name they export moves the minor version (0.1 to 0.2), as Cargo reads a 0.x version; the kit moves to 1.0.0 once its blocks have left the app's wire types in `symphony-api`, and from then such a release is a new major version.

MIT licensed: the terms are in the LICENSE file beside this README.
