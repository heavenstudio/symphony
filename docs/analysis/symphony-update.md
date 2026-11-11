# Updating a project's Symphony

The evidence and the proposed steps behind the update-command card in
[DECISIONS](../DECISIONS.md). Each fact about the code names the file and function it was read
in. "Read 2026-11-11" marks the state of this machine that day.

## What happens today

- **The pin.** Star Foundry names ten Symphony crates in its `Cargo.toml` on `branch = "main"`,
  and its `Cargo.lock` holds the commit. Read 2026-11-11: `0e97e33`, 13 commits behind main.
- **The client** is compiled into the server from the pinned commit
  (`apps/symphony-server/build.rs`, `generate`), and a consuming host always serves that copy
  (`apps/symphony-server/src/client.rs`, `select`). It moves with the pin.
- **The server.** Star Foundry's `just symphony`, run on main, opens a server that answers
  (`--if-up`) and builds only when none does (its `justfile`, the `symphony` recipe). A start
  replaces a running server only when that server lacks a plugin, or its pin or board schema is
  older (`machine/host.rs`, `judge` and `Build::lacks`). An ordinary fix changes neither, so it
  reaches the server only through `just symphony --stop` and a new start.
- **The board command.** Star Foundry's `just tasks` runs the build of its committed main (its
  `justfile`, the `tasks` recipe), so it moves once the update lands.
- **Skills and the heartbeat hook** are compiled into the `symphony` binary
  (`init.rs`, `Shipped::builtin`) and written only when someone runs `symphony init`; nothing else
  calls it (`cli/commands.rs`, `plugin`). A file without the Symphony's stamp is the project's own
  and is never written (`install_skills`, `ensure_beat`). Read 2026-11-11: all seven of Star
  Foundry's skill copies and its heartbeat hook carry no stamp, and every skill differs from the
  shipped one, so `init` would leave them as they are.
- **Behind.** Nothing says so. A server reports its pin and board schema, not its commit
  (`symphony-api`, `Front`). The pin is 1 and moves only with the wire contract or the plugin
  seams (`machine.rs`, `PIN`). The board schema went from 9 to 14 between 2026-11-02 and
  2026-11-10 (`crates/tasks/src/schema.rs`, `VERSION`). Symphony has no tags. Main moved 9 to 23
  times a day (first-parent commits) from 2026-11-04 to 2026-11-11. The board has no `status`
  command (`tasks --help`).

## The command, step by step

`just symphony-update` is a one-line recipe over `symphony update`, a new command of the
`symphony` binary. It runs twice: once to make a branch, once on main after the branch lands.
Steps marked *project* stay the project's (AGENTS.md, command ownership); the rest are Symphony's.

On a branch:

1. **Find.** Read the Symphony source and commit from the project's `Cargo.lock`: its
   `source = "git+URL?branch=main#SHA"` line names both. Ask that URL for its newest commit with
   `git ls-remote`. Stop when they match. A Symphony fix reaches a project only once it is pushed.
2. **Branch** (*project*: where). Make `symphony-update/SHORT` in a new worktree from the default
   branch, in the folder `[agents] worktrees` names.
3. **Move the pin.** `cargo update -p symphony --precise SHA`. Refuse when any line of another
   package moved, and show it.
4. **Build** (*project*: how). Build the project's `symphony` binary at the new commit with the
   project's own build command (Star Foundry's slot wrapper), or plain `cargo build --locked`
   when it names none.
5. **Init.** Run that new binary's `init`: stamped skills, Codex rules and hook brought up to
   date; the project's own copies left as they are and listed.
6. **Commit** the lock and the init changes together. The message names the old and new
   commits, Symphony's merges between them, and whether the board schema or pin moved.
7. **Check** (*project*). Run `[verdicts] check` (Star Foundry: `just verify`), the command that
   already checks a generated branch (`project.rs`, `Verdicts`).
8. **Land** (*project*). Hand the branch to `[verdicts] land` (Star Foundry: `just land`), or
   stop and print that command. The project's gates and approval decide.

On main, after landing:

9. **Restart.** Only in the main checkout of the project that hosts the shared server. The host
   must compile every registered project's plugins, and a project with none, like Symphony's own,
   never hosts (`machine/host.rs`, `judge`). Read 2026-11-11, the host is Star Foundry's build
   (the machine folder's `server.json`). Build the server, then stop and start it. A project that
   does not host is told which one does.
10. **Relink** `.target/bin/symphony`, which the heartbeat hook runs (AGENTS.md, agent
    integration).

## What one run updates

Everything comes from the one pin, so one command covers it:

- The Rust crates: the pin itself (step 3).
- The skills, Codex rules and heartbeat hook: stamped copies only (step 5).
- The embedded client: with the server build (step 9).
- The server binary: step 9.
- The board command: by itself, once the lock lands.

## Knowing a project is behind

Main moves too often for "a newer commit exists" to mean anything. Under A:

- **Must update:** main's board schema or pin is ahead of the project's. The page says so in
  one line, and `just symphony` prints it on start.
- **Should update:** the pinned commit is over a week old. Same places, quieter.
- Otherwise nothing is shown; `just symphony-update --check` prints how far behind, changing
  nothing.

The look reaches the network, so it runs at most once a day and keeps its answer in the machine
folder. The hard cases already refuse and name the project that must update (`Lack::said`, the
schema `Refusal`). Under B, behind simply means a newer tag exists.

## When the board schema changes

What the code does:

- A build writes only a board at its own schema. A newer build moves an older board forward when
  it opens it, even only to read. An older build is refused before it writes. A reader one
  version behind still reads (`schema.rs`, `decide`; `store/opening.rs`, `Store::opening`).
- The server opens every board it shows to read (`taskview/connect.rs`, `connect_in`).

What follows:

- When Symphony's main raises the schema, Symphony's own board moves the next time its
  `just tasks` runs. The shared server, an older build, still reads that board but cannot write
  to it until the hosting project updates and restarts.
- A project's board moves at its first `just tasks` after the update lands, so its restart
  should follow the landing at once: until then the server cannot write to that board.
- A newer server moves every registered project's board it reads. A project still pinned older
  then has its own `just tasks` refused until it updates too. A schema change is machine-wide.

What the command does when an update carries one:

- Says so in the branch's commit and in its summary.
- After landing, before the restart: lists the registered projects at an older schema
  (`projects.json`), writes a dump of the board (`tasks-copy --board --out`), and asks.

## Rollback

- `just symphony-update --to OLD`, with the commit the update's message names, runs the same
  steps toward the old commit. Reverting the update's commit through the project's landing does
  the same. Then restart from main.
- Across a schema change there is no way back down: the older build is refused writes on the
  moved board. Fix forward on Symphony's main, or restore the dump with `tasks-copy`, losing the
  board's writes since the update. The command refuses `--to` a commit with an older schema than
  the board's unless told to restore.

## Installing the recipe

- `symphony init` writes a stamped `symphony.just` holding the one recipe, the way it writes the
  Codex rules. It never edits the project's `justfile`; when the line is missing it prints the one
  line to add, `import? 'symphony.just'`.
- A project recipe of the same name is the project's own. `just` refuses the two together unless
  the justfile sets `allow-duplicate-recipes`, and then the project's wins (tried with just 1.58).
- Without just: run `symphony update` from the project's `symphony` binary.
- Without Cargo: there is no pin to move. The command refreshes the stamped files and names the
  project that hosts the server.

## Never unattended

- Land, push or merge anything.
- Move any dependency other than Symphony's.
- Write over a project's own files, its `justfile` included.
- Restart the shared server from a worktree, or across a schema change, without a yes.
- Move a board's schema back.

## Open questions

1. Should the command run the project's land command itself, or stop at a ready branch?
2. Is a week the right age for "should update"?
3. Star Foundry's skills and hook are its own copies. Should it take the stamped ones, so
   updates reach them, or keep its own, which the update then never touches?
4. When a restart would move another registered project's board, should it refuse until that
   project updates, or ask?

## Verified and inferred

- **Read in code:** each fact above that names a file and function.
- **Measured:** commit counts and the schema's history from this repository's `git log`; Star
  Foundry's lock, skills and hook read in place; `import?` precedence tried in a scratch folder.
- **Not tried:** whether `cargo update -p symphony --precise` alone keeps every other lock line
  in place; a newer server moving another project's board, read in code and not exercised.
