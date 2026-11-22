# Updating a project's Symphony

How a consuming project takes a newer Symphony, as decided on the
[update-command card](#symphony-update-command), and the evidence it rests on. Each fact about the
code names the file and function it was read in. "Read 2026-11-11" marks the state of this
machine that day.

## Releases

**Decided: B**, since other projects will use Symphony: releases are generally made from nightly
builds that run a full verification cycle and the conciliation, and an urgent fix can be released
out of cycle.

- Projects follow Symphony's release tags, never main.
- A nightly on main's tip, in its own worktree, runs the full verification and the conciliation.
  When both are green it tags that commit as the next release and pushes the tag. Each red becomes
  a task on Symphony's board, and no release is cut that night.
- An urgent fix is released out of cycle: the same checks on the fix's commit, at once, then a
  tag, without waiting for the night.
- A tag is dated and sorts in release order: `release-2026-11-12`, and `release-2026-11-12.2` for
  a second release that day. Its message names the board schema and pin, and says when either
  changed since the last release.
- A project's `Cargo.toml` names the tag (`tag = "release-…"` in place of `branch = "main"`), and
  its `Cargo.lock` binds the tag's commit.

**Full verification** is `just verify` in a clean checkout, plus the nightly-tier targets the fast
plan leaves out: the board's `perf` target and `tests/child_guard` (`crates/tasks/AGENTS.md`).
**Conciliation**, as read here until the owner says otherwise: build and test each project the
machine serves with its Symphony moved to the candidate, in a scratch copy, never its checkout;
and list what changed since the last release.

## What happens today

- **The pin.** The consuming project names ten Symphony crates in its `Cargo.toml` on `branch = "main"`,
  and its `Cargo.lock` holds the commit. Read 2026-11-11: `0e97e33`, 13 commits behind main.
- **The client** is compiled into the server from the pinned commit
  (`apps/symphony-server/build.rs`, `generate`), and a consuming host always serves that copy
  (`apps/symphony-server/src/client.rs`, `select`). It moves with the pin.
- **The server.** The consuming project's `just symphony`, run on main, opens a server that answers
  (`--if-up`) and builds only when none does (its `justfile`, the `symphony` recipe). A start
  replaces a running server only when that server lacks a plugin, or its pin or board schema is
  older (`machine/host.rs`, `judge` and `Build::lacks`). An ordinary fix changes neither, so it
  reaches the server only through `just symphony --stop` and a new start.
- **The board command.** The consuming project's `just tasks` runs the build of its committed main (its
  `justfile`, the `tasks` recipe), so it moves once the update lands.
- **Skills and the heartbeat hook** are compiled into the `symphony` binary
  (`init.rs`, `Shipped::builtin`) and written only when someone runs `symphony init`; nothing else
  calls it (`cli/commands.rs`, `plugin`). A file without the Symphony's stamp is the project's own
  and is never written (`install_skills`, `ensure_beat`). Read 2026-11-11: all seven of the
  consuming project's skill copies and its heartbeat hook carry no stamp, and every skill differs
  from the shipped one, so `init` would leave them as they are.
- **Behind.** Nothing says so. A server reports its pin and board schema, not its commit
  (`symphony-api`, `Front`). The pin is 1 and moves only with the wire contract or the plugin
  seams (`machine.rs`, `PIN`). The board schema went from 9 to 14 between 2026-11-02 and
  2026-11-10 (`crates/tasks/src/schema.rs`, `VERSION`). Symphony has no tags yet.

## The command, step by step

`just symphony-update` is a one-line recipe over `symphony update`, a new command of the
`symphony` binary. It runs twice: once to make a branch, once on main after the branch lands.
Steps marked *project* stay the project's (AGENTS.md, command ownership); the rest are Symphony's.

On a branch:

1. **Find.** Read the Symphony source and tag from the project's `Cargo.toml` and `Cargo.lock`,
   and list that source's release tags with `git ls-remote --tags`. Stop when the project is on
   the newest. `--to TAG` names another release.
2. **Branch** (*project*: where). Make `symphony-update/TAG` in a new worktree from the default
   branch, in the folder `[agents] worktrees` names.
3. **Move the pin.** Set the tag in the Symphony lines of `Cargo.toml` and run
   `cargo update -p symphony`. Refuse when any line of another package moved, and show it.
4. **Build** (*project*: how). Build the project's `symphony` binary at the new release with the
   project's own build command (its `[project] cargo`), or plain `cargo build --locked` when it
   names none.
5. **Init.** Run that new binary's `init`: stamped skills, Codex rules and hook brought up to
   date; the project's own copies left as they are and listed.
6. **Commit** the manifest, the lock and the init changes together. The message names the old
   and new tags, the release's notes, and whether the board schema or pin moved.
7. **Check** (*project*). Run `[verdicts] check` (for example `just verify`), the command that
   already checks a generated branch (`project.rs`, `Verdicts`).
8. **Land** (*project*). Stop and print `[verdicts] land` with the branch (for example
   `just land`); `--land` runs it. The project's gates and approval decide.

On main, after landing:

9. **Restart.** Only in the main checkout of the project that hosts the shared server. The host
   must compile every registered project's plugins, and a project with none, like Symphony's own,
   never hosts (`machine/host.rs`, `judge`). Read 2026-11-11, the host is the consuming project's build
   (the machine folder's `server.json`). Build the server, then stop and start it. A project that
   does not host is told which one does.
10. **Relink** `.target/bin/symphony`, which the heartbeat hook runs (AGENTS.md, agent
    integration).

- [r] **symphony-update-command** — A project takes a newer Symphony with one command, `just symphony-update`, which `symphony init` installs. The project's own check, land command and approval still decide whether it lands. Which commit should the command take? A: main's newest. B: the newest release, which Symphony would tag. C: no command, a written checklist instead. Recommended: A. Approve and say A, B or C in the note, or comment. [[analysis/symphony-update#The command, step by step]]
  [2026-11-11 12:37] agent — Posted for the board task `product/symphony-update`.

    **What the command does.** On a new branch it moves only the Symphony commit in Cargo.lock, builds, brings the shipped skills and hook up to date, and runs the project's check. The project's land command and approval land it. Run again on main afterwards, it restarts the shared server. It never lands, pushes or moves another dependency by itself, and it asks before a restart that changes the board schema.

    **A, follow main.** Each run takes main's newest commit. Main moved 9 to 23 times a day this past week, so a project is shown as behind only when main's board schema or pin is ahead of it, or its commit is over a week old. Cost: a bad main commit reaches the project if the project's own check misses it; one revert undoes it.

    **B, follow releases.** Symphony tags a release for a fix, a schema change or once a week, and the command takes the newest tag. Cost: a release step on Symphony's side for every fix, and a fix waits for its tag. There are no tags today.

    **C, a checklist.** The same steps by hand. Cost: today's hand work every time, and nothing tells a project it is behind.

    **Why A.** The consuming project is the only project, its own `just verify` checks every update before it lands, and a revert is the rollback. B pays off once a project off this machine uses Symphony.

    Four smaller questions are at the end of [[analysis/symphony-update#Open questions]]; answer any of them in the note.
  [2026-11-11 12:43] owner · approved — B. There will be other projects using it. New releases can be pushed for urgent bugfixes, but generally should be generated based on nightly builds that would do a full verification cycle and conciliation.
  [2026-11-11 12:53] agent — Recorded as B: projects follow Symphony's release tags, never main. A nightly on main's tip runs the full verification and the conciliation, and tags a release only when both are green. An urgent fix is released out of cycle after the same checks. The pin-only move, the project's own landing, the schema and restart handling, and rollback (now to the previous release tag) stay as proposed. [[analysis/symphony-update#Releases]] now states this. Filed on the board: tools/release-nightly (the releases and the nightly), tools/symphony-update-command (the command and the stamped recipe init installs) and tools/symphony-behind-signal (the line on the page, in tasks status and at start). What conciliation should cover is asked on tools/release-nightly's thread; until you answer it reads as building each project the machine serves against the candidate.
  [2026-11-11 12:53] agent · resolved

## What one run updates

Everything comes from the one pin, so one command covers it:

- The Rust crates: the pin itself (step 3).
- The skills, Codex rules and heartbeat hook: stamped copies only (step 5).
- The embedded client: with the server build (step 9).
- The server binary: step 9.
- The board command: by itself, once the lock lands.

## Knowing a project is behind

- **Behind:** a newer release tag exists than the one the project names.
- **Must update:** a newer release changes the board schema or the pin, which the shared server
  and the other projects on the machine follow.
- One line says so on the project's page, in `just tasks status`, and when `just symphony`
  starts. `just symphony-update --check` prints it and changes nothing.

Listing the tags reaches the network, so the look runs at most once a day and keeps its answer in
the machine folder; a page never waits on it. The hard cases already refuse and name the project
that must update (`Lack::said`, the schema `Refusal`).

## When the board schema changes

What the code does:

- A build writes only a board at its own schema. A newer writer moves an older board forward when
  it opens it; a reader never does. An older build is refused before it writes. A reader one
  version behind still reads (`schema.rs`, `decide`; `store/opening.rs`, `Store::opening`).
- A project's live board moves forward only by a build its main checkout says is its own
  (`live.rs`, `may_move`): in Symphony's checkout, which holds the schema source, any command of
  a build of that very source; in a project whose checkout holds a pin, only `just tasks migrate`
  run in its main checkout.
- The server opens every board it shows to read (`taskview/connect.rs`, `connect_in`).

What follows:

- Symphony's own `just tasks` runs this checkout's build (`justfile`, `tasks`), so Symphony's
  board moves as soon as main raises the schema, before any release carries it. Until a release
  with that schema is out and the hosting project takes it, the shared server reads Symphony's
  board but cannot write to it.
- A project's board moves at its `just tasks migrate` in its main checkout after the update
  lands, so the update runs it and the restart should follow at once: until then the server
  cannot write to that board.
- A newer server never moves a board it reads. A project still on an older release reads and
  writes its own board until it updates; a newer server reads it only once it has.

What the command does when an update carries one:

- Says so in the branch's commit and in its summary.
- After landing, before the restart: lists the registered projects at an older schema
  (`projects.json`), writes a dump of the board (`tasks-copy --board --out`), and asks.

## Rollback

- `just symphony-update --to TAG`, with the previous release the update's message names, runs
  the same steps toward it. Reverting the update's commit through the project's landing does the
  same. Then restart from main.
- Across a schema change there is no way back down: the older build is refused writes on the
  moved board. Take a newer release with the fix, or restore the dump with `tasks-copy`, losing
  the board's writes since the update. The command refuses `--to` a release with an older schema
  than the board's unless told to restore.

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

- Take a commit that is not a release tag.
- Land, push or merge anything; the nightly pushes only its new tag.
- Tag a commit a check found red, or move or delete a tag.
- Move any dependency other than Symphony's.
- Write over a project's own files, its `justfile` included.
- Restart the shared server from a worktree, or across a schema change, without a yes.
- Move a board's schema back.

## Not chosen

- Following main: each run would take main's newest commit. Not chosen; see the card.
- A written checklist with no command. Not chosen; see the card.

## Open questions

1. What does the nightly's conciliation cover? The reading under [Releases](#releases) is a
   proposal.
2. Should the command run the project's land command by default? Until answered it stops at a
   ready branch, and `--land` runs it.
3. The consuming project's skills and hook are its own copies. Should it take the stamped ones, so
   updates reach them, or keep its own, which the update then never touches?
4. When a restart would move another registered project's board, should it refuse until that
   project updates? Until answered it asks.

## Verified and inferred

- **Read in code:** each fact above that names a file and function.
- **Measured:** commit counts and the schema's history from this repository's `git log`; the
  consuming project's lock, skills and hook read in place; `import?` precedence tried in a scratch
  folder.
- **Not tried:** a newer server moving another project's board, read in code and not exercised.
