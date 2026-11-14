# symphony-server

Symphony's server: one per machine, serving each project its registry holds under `/<id>/`, the project's documents, its review queue and its task board, to Symphony's web client, which this package embeds, so an installed server needs no client build of its own. Symphony is a document, review and task workspace.

The product's commands install together:

    cargo install --locked --config profile.release.strip=true --config profile.release.codegen-units=1 symphony-app symphony-board symphony-server

`symphony-server --help` says how to run it. The two `--config` settings strip the symbol table and build each crate as one codegen unit, as Symphony's own release builds do: without the first each binary is a quarter to a third larger, and without the second 0.5 to 2.7 MB larger.

Its version, below 1.0, is the product's: `symphony-app`, `symphony-board`, `symphony-server`, `symphony-chrome` and `symphony-api` carry one version, require each other at exactly that version, and promise no name: any release may change one.

MIT licensed: the terms are in the LICENSE file beside this README.
