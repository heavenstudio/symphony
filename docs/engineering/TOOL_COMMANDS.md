# Commands for a project's tools

A project's art and landing tools call the installed `symphony` rather than link its library
([the command route](analysis/consumer-contract.md#contract-tool-route)). They run with `--root ROOT`, or with the
project root as their current directory. Card rendering, status and triage start the project's
selected plugins through `[plugins.process]`, just as the server does.

The outputs below are contracts: field names, types and meanings, the documented text layout,
and exit codes stay supported. JSON readers accept extra fields; text readers use the layouts
below. JSON object order and whitespace are not significant. Typed blocks use the plugin kit's
block vocabulary. There is no separate command protocol version; these commands ship with the
product release. The contract tests are `crates/symphony/tests/tool_commands.rs`.

## Draw one card

`symphony render-card PATH SLUG [--root ROOT]` reads one card from a document route, such as
`docs/DECISIONS.md`. The route must be served by an entrypoint; an excluded, hidden or outside
file cannot be read. A missing or duplicate slug is an error. Only the named card is drawn.

Success prints one JSON object followed by a newline:

```json
{"path":"docs/DECISIONS.md","slug":"shape","line":3,"body":{"blocks":[]},"thread":{"blocks":[]}}
```

`line` is the card's one-based source line. `body.blocks` holds the card as the native page
draws it, with its thread detached; `thread.blocks` holds that thread. The arrays in this
envelope example are empty to show their positions. The card block's `line_offset` is relative
to this one card, so add `line` to obtain its source line. Components, pictures, links and
history use the page's rendering and asset resolution. A tool tests those typed results rather
than depending on HTML markup.

## Check a branch for a landing

`symphony landing-checks BRANCH [PATH...] [--root ROOT] [--base REF]` checks a branch using
`REF`'s committed `symphony.toml`, with `main` as the default reference. Resolve a configured
default branch by passing it as `--base`; the checkout's uncommitted settings and the branch
being judged cannot widen what the base admits. A missing or unreadable project file uses the
plain project's document convention and no generators, as the landing's library caller does.

Success prints one JSON object followed by a newline, with these fields:

| Field | Type | Meaning |
| --- | --- | --- |
| `base` | string | Resolved base commit. |
| `tip` | string | Resolved branch commit. |
| `regenerates` | boolean | The branch has a verdict record and its non-document changes are exactly what the base's verdict generators make from its documents. |
| `documents` | object of booleans | Repository file paths changed since the merge base, plus each requested `PATH`, classified using the base's entrypoints. Renames classify both paths. |
| `owner_lines_left` | array of strings | Document file paths with uncommitted added owner lines in the checkout at `ROOT`, as its current entrypoints show them. |
| `write_lock` | string | Absolute path of the OS advisory lock shared with document saves. Reading this answer neither creates nor acquires it. |

`regenerates` runs only the base's `[verdicts] generators`, in a temporary private worktree,
then removes that worktree. A missing record, generator failure, unexpected output or different
bytes yields `false`. It runs no plugin generator. It does not publish a verdict or land a
branch. Paths are UTF-8; a changed path that is not UTF-8 is an error. A failed Git read or invalid
revision is an error, with no success JSON on stdout.

## Status and triage

`symphony status --json` prints the board with this structure, including empty arrays and null
values. `line` is one-based; `open` counts cards and comments waiting on an agent. A decision's
`open`, `group` and thread are read by the project's card kinds.

```json
{"docs":[{"path":"docs/DECISIONS.md","decisions":[{"line":3,"slug":"shape","state":"pending","text":"Choose a shape.","note":["A note"],"open":true,"group":"agent","topic":"General","decided_at":null,"entries":[{"ts":null,"who":"owner","verdict":null,"text":"A note"}]}],"comments":[{"line":8,"kind":"comment","title":"A note","body":[],"resolution":null,"open":true}]}],"open":2}
```

`symphony status --open` prints open items in file and source-line order. Each first line is
`PATH:LINE`, two spaces, the kind padded to eight characters, one space, then the title; cards'
titles start with `**SLUG**`. Nonempty body lines follow with four spaces of indentation. With
none open it prints `no open comments`. Without `--open` or `--json`, status first prints
decision counts by state and group, then the open-comment section. `status --tidy` regroups and
resolves the project's registers; `--now` supplies its timestamp. This write follows the CLI's
existing write confinement. `status --lint` is the document lint, a separate check.

`symphony triage stats` prints first-pass approval rates by category, then first-post and redraw
rates, approvals with a note, picture and design letters, and answers with a question. A rate
has `N/D` left-aligned in seven characters, a space, then `P%` right-aligned in four characters;
with no decisions the percentage is `-`. The note and question lines collapse that padding to
single spaces. Categories and letters sort by name.

`symphony triage queue N` prints up to N items, three lines each: the rank padded to two
characters plus `. SLUG`, two spaces and the document route; four spaces and the reason; four
spaces and the card URL. The URL uses the project's recorded listening port, otherwise 8765.
An empty queue prints `nothing waiting on the owner`. These text layouts and status's JSON
shape are held by the contract tests.

## Exit codes

`render-card` and `landing-checks` print help and exit 0 for `-h` or `--help`, exit 2 for invalid command-line
arguments, and exit 1 for a read, configuration or rendering error. Diagnostics go to stderr;
failures print no success object. Status exits 2 if its board cannot be read. Triage exits 1
when its board or statistics cannot be read, and 2 for invalid arguments. Success exits 0.

## The document write lock

A landing tool's final clean-check and main-checkout swap take the same exclusive OS advisory
file lock as the server's document writes. Its identity is
`<absolute git common dir>/symphony/server/verdict-writes.lock`, across the main checkout and
all linked worktrees. Obtain the absolute common directory with
`git rev-parse --path-format=absolute --git-common-dir`; `landing-checks` returns the complete
path. Create its parent folder if needed, open the file for append with create enabled, acquire
the exclusive advisory file lock, and keep the descriptor open through validation and swap.
Release it by closing the descriptor. Do not unlink or replace the lock file: that changes the
identity other processes hold. On macOS and Linux this is the `flock(2)` exclusive lock used by
Rust's `File::lock`; a create-exclusive sentinel file is not the same lock. Candidate generation
and verification run before acquiring it. A folder outside a Git checkout has no such lock.

This names the lock's existing identity and scope, so a consumer can take it itself without
linking the library. Other library uses migrate under their own contracts; these commands do
not remove those APIs.
