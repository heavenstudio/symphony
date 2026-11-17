# symphony-app

Symphony's library and its `symphony` command. Symphony is a document, review and task workspace: an owner reviews a project's documents, answers its decision cards and asks in a web app, and agents take the work from a task board. This package reads and writes the cards and callouts in a project's `docs/`, its project file (`symphony.toml`) and the plugins it selects; its `symphony` command answers `status`, `reply`, `ask`, `init` (the agent skills and recipes a project uses), `shot` (a page in a private headless Chrome) and the rest its `--help` lists.

The product's commands install together:

    cargo install --locked --config profile.release.strip=true --config profile.release.codegen-units=1 symphony-app symphony-board symphony-server

`symphony-app` installs `symphony`, `symphony-board` the board's `tasks` and `agent-send`, and `symphony-server` the server. The two `--config` settings strip the symbol table and build each crate as one codegen unit, as Symphony's own release builds do: without the first each binary is a quarter to a third larger, and without the second 0.5 to 2.7 MB larger.

The library keeps the name `symphony`: a dependent names it `symphony = { package = "symphony-app", … }`, and its code says `symphony::`.

Its version, below 1.0, is the product's: `symphony-app`, `symphony-board`, `symphony-server`, `symphony-chrome` and `symphony-api` carry one version and require each other at exactly that version, without a general API stability promise. Known consumer calls follow the two-release rule below. What a project's own code links to extend Symphony is the plugin kit, `symphony-plugin`, `symphony-blocks` and `symphony-docs`.

A public Rust item a known consumer calls stays working beside its replacement for at least one release, without `#[deprecated]` because consumer lints deny warnings. Its removal is a separate task for a later release, with read-only evidence of consumer migrations and the consumer-API compile list updated.

MIT licensed: the terms are in the LICENSE file beside this README.
