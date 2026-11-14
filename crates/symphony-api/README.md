# symphony-api

Symphony's wire types: the JSON its server sends and its client reads, serde only, so one definition builds for the server and for wasm32. The blocks a plugin renders to come from `symphony-blocks`, re-exported here under the same names.

Its version, below 1.0, moves with Symphony's server and client and promises no name: any release may change one. What a project's own code links to extend Symphony is the plugin kit, `symphony-plugin`, `symphony-blocks` and `symphony-docs`.

MIT licensed: the terms are in the LICENSE file beside this README.
