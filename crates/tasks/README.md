# symphony-board

Symphony's task board: tasks in a tree, the sessions and agents that claim, review and land them, their threads and verdicts, and the builds and tests each records, kept in a local SurrealDB server the board starts in a repository's git folder. It installs the `tasks` command (every board command), `agent-send` (a message to another agent's session), `tasks-copy` (a whole board copied into its database and back out) and `tasks-reclaim` (the store rebuilt from an export of itself).

That server is SurrealDB's own binary, at the version the board pins, which this package does not install: `symphony init` ends by printing the line that fetches it into the repository's git folder and checks its SHA-256, and `SYMPHONY_SURREAL` names a copy of it elsewhere.

Symphony is a document, review and task workspace. The product's commands install together:

    cargo install --locked --config profile.release.strip=true --config profile.release.codegen-units=1 symphony-app symphony-board symphony-server

The two `--config` settings strip the symbol table and build each crate as one codegen unit, as Symphony's own release builds do: without the first each binary is a quarter to a third larger, and without the second 0.5 to 2.7 MB larger.

Its version, below 1.0, is the product's: `symphony-app`, `symphony-board`, `symphony-server`, `symphony-chrome` and `symphony-api` carry one version, require each other at exactly that version, and promise no name: any release may change one.

MIT licensed: the terms are in the LICENSE file beside this README.
