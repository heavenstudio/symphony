# symphony-board

Symphony's task board: tasks in a tree, the sessions and agents that claim, review and land them, their threads and verdicts, and the builds and tests each records, kept in a local SurrealDB server the board starts in a repository's git folder. It installs the `tasks` command (every board command), `agent-send` (a message to another agent's session), `tasks-copy` (a whole board copied into SurrealDB and back out) and `tasks-reclaim` (the store rebuilt from an export of itself).

Symphony is a document, review and task workspace. The product's commands install together:

    cargo install --locked --config profile.release.strip=true symphony-app symphony-board symphony-server

The `--config` line strips the symbol table, as Symphony's own release builds do; without it each binary is a quarter to a third larger.

Its version, below 1.0, is the product's: `symphony-app`, `symphony-board`, `symphony-server`, `symphony-chrome` and `symphony-api` carry one version, require each other at exactly that version, and promise no name: any release may change one.

MIT licensed: the terms are in the LICENSE file beside this README.
