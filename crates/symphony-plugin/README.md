# symphony-plugin

What a Symphony plugin implements: a value implementing `Plugin`, whose seams take and return plain data (paths, text and the blocks in `vocabulary`), never a Symphony library type. A host binary compiles the plugin in, or the plugin answers from a process of its own: `serve(&PLUGIN)` answers the line protocol on its stdin and stdout.

Part of [Symphony](https://github.com/heavenstudio/symphony)'s plugin kit, with `symphony-api` and `symphony-docs`, the three at one version. At 0.1.0 the kit promises no name yet: it takes 1.0.0 once its block vocabulary has left the app's wire types in `symphony-api` ([RELEASES](https://github.com/heavenstudio/symphony/blob/main/docs/RELEASES.md#packages-on-cratesio)).

MIT licensed: see [LICENSE](LICENSE).
