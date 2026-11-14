# symphony-plugin

What a Symphony plugin implements: a value implementing `Plugin`, whose seams take and return plain data (paths, text and the blocks in `vocabulary`), never a Symphony library type. A host binary compiles the plugin in, or the plugin answers from a process of its own: `serve(&PLUGIN)` answers the line protocol on its stdin and stdout.

Part of the plugin kit of Symphony, a document, review and task workspace: what a project's own code links to extend it, with `symphony-api` and `symphony-docs`. The three always carry one version. Below 1.0 a release that breaks a name they export moves the minor version (0.1 to 0.2), as Cargo reads a 0.x version; the kit moves to 1.0.0 once its blocks have left the app's wire types in `symphony-api`, and from then such a release is a new major version.

MIT licensed: the terms are in the LICENSE file beside this README.
