# Setting up GitHub

Symphony runs without GitHub. `symphony-github` is an optional program that connects a
project's board to one repository on GitHub.com, and it does two things, each set up on its own:

- **The issue exchange.** Open issues carrying a label you choose become proposals on the board,
  and tasks you opt in are published as issues. It needs one token and no GitHub App:
  [its steps](#the-issue-exchange).
- **Delivery by pull request.** `just land` opens a pull request for the task, writes its
  evidence there as checks, and has GitHub squash it, in place of merging and pushing the default
  branch itself. It needs three GitHub Apps installed on the repository and two rulesets on its
  default branch: [its steps](#delivery-by-pull-request).

This page is the setup, in order, from nothing to a check that passes. How each part behaves,
every setting and every refusal are in [GITHUB](GITHUB.md).

## What works today, and what waits

- It is run by hand. Nothing runs the exchange on a schedule, from a webhook or from a page: a
  person runs `preview`, then `reconcile`. A land runs the delivery itself.
- One repository on GitHub.com. Another host, GitHub Enterprise among them, is refused.
- Nothing makes or renews a GitHub App's token. Delivery reads three installation tokens from
  files, GitHub expires each within the hour, and a delivery stops as `unavailable` until the
  file holds a current one. Keeping them current is yours, with whatever you run for it.
- `pull_requests = "track"` is accepted and does nothing in this build.
- An organization's own rulesets count only where GitHub shows the policy reader their bypass
  lists, which was not tried: put the rulesets of [step 7](#delivery-by-pull-request) on the
  repository.

## Before you start

- A project set up as [the README](../README.md#on-your-own-project) says: a main checkout in
  git, the board made, and its `tasks` command working.
- `git` and `curl` on `PATH`: the program reads its settings with git and reaches GitHub with
  curl, and needs neither `gh` nor anything else.
- The repository on GitHub.com, named `OWNER/NAME`. For delivery, it is the main checkout's
  `origin`, and your own `git push` to it works: the land pushes the task's branch with it.
- Rust, to build the program: the toolchain this repository's `rust-toolchain.toml` names.

## Building the program

`symphony-github` is not on crates.io and no release installs it, so it is built from this
repository's source, best at the release the project's `tasks` comes from, as it reaches the
board through `tasks api`. In a clone of this repository:

```sh
cargo install --locked --path crates/symphony-github
```

That puts it in Cargo's `bin` folder (`~/.cargo/bin`), beside the `tasks` that
`cargo install symphony-board` put there. It runs the `tasks` its `--tasks PROGRAM` option
names, else the one beside it, else the one on `PATH`. A land runs it by name, so for delivery it
must be on the `PATH` of whoever runs `just land`.

`symphony-github --version` prints its version, and `symphony-github --help` its commands. Run
every command below in the project's main checkout.

## The issue exchange

1. **A token.** On GitHub, a fine-grained personal access token (Settings, Developer settings,
   Personal access tokens, Fine-grained tokens). Its repository access is "Only select
   repositories" with the one repository, and its one permission is Issues: Read-only for
   `issues = "import"`, Read and write for `"export"` or `"exchange"`. GitHub adds Metadata
   (read). An organization can require its approval before the token reaches its repositories.
2. **The token file**, outside every checkout of the project and its git folder, and readable by
   you alone:

   ```sh
   mkdir -p ~/.config/symphony-github
   (umask 077 && cat > ~/.config/symphony-github/token)
   ```

   Paste the token, then Enter and Ctrl-D. `ls -l ~/.config/symphony-github/token` shows
   `-rw-------`.
3. **The settings**: `symphony-github.toml` at the root of the project.

   ```toml
   enabled = true
   repository = "OWNER/NAME"
   issues = "import"
   token_file = "/Users/YOU/.config/symphony-github/token"
   import_labels = ["symphony"]
   ```

   `token_file` is the file's absolute path: a `~` is not expanded. The path is committed, so
   every machine that runs the program keeps its token there. `issues = "import"` brings
   issues in, `"export"` publishes the tasks you opt in, `"exchange"` does both; one that imports
   needs `import_labels`. `import_comments`, `export_label` and `close_issues` are in
   [the table](GITHUB.md#turning-it-on).
4. **Commit it on the default branch** (`[project] default_branch` in `symphony.toml`, `main`
   unless it says another). The program reads the file from that branch's commit, never from a
   working tree: a file only on disk, on another branch or in a linked worktree turns nothing on.
   On a project that lands through its board, that is a task whose branch adds the file, landed
   with `just land`.
5. **A handle** for the board's writes: `just tasks register` prints one, say `calm-swift`.

Then [check it](#checking-it). From then on, each run is `symphony-github preview`, which shows
exactly what would be written, then the `reconcile` line it prints. A task is published by
`symphony-github publish SLUG --title TITLE --body-file FILE` before the preview, its public text
kept apart from its brief ([the commands](GITHUB.md#the-commands)).

## Delivery by pull request

Someone who administers the organization and the repository does these steps on GitHub, in this
order. Symphony does none of them, and no agent does them without that person's word.

**Read this first.** Once the rulesets of step 7 are active, GitHub takes no push to the default
branch, from anyone: it changes only by a pull request the landing App merges. Every land, the
regenerator's work and the app's document saves are delivered that way once `[land] delivery` is
set ([TASKS](TASKS.md#a-land-delivered-by-a-pull-request)); anything else that pushes there,
a person's own `git push` among them, is refused. Steps 1 to 5 change nothing a land does. The
settings of step 6 land by the ordinary merge and push, so they come before the rulesets: from
then on every land is delivered by a pull request, and refused, pushing nothing, until step 7 is
done.

1. **Three GitHub Apps** in the organization (its settings, Developer settings, GitHub Apps, New
   GitHub App). Each takes a name and a homepage URL, any address; untick the webhook's Active
   box, set these repository permissions and no others (GitHub adds Metadata, read), and let it
   be installed only on this account. Each App's number is the App ID on its settings page.

   | App | Repository permissions | Its number goes in |
   | --- | --- | --- |
   | the check publisher | Checks: Read and write | `checks_app_id` |
   | the landing App | Contents: Read and write; Pull requests: Read and write; Issues: Read and write where `close_issues` is set or the issue exchange is on | `landing_app_id` |
   | the policy reader | Administration: Read and write | `policy_app_id` |

   The policy reader's permission could change or remove the rules of step 7. It is asked
   because GitHub shows a ruleset's bypass list to nothing less
   ([the rights each read needs](GITHUB.md#the-protection-a-delivery-needs)), and the program
   sends its token with that one read alone.
2. **Install each App** on the one repository: Install App, then "Only select repositories". Its
   installation's number is the last part of the address of the installation's settings page.
3. **A private key for each App** (Generate a private key, on its settings page), kept outside
   every checkout and readable by its owner alone. Whoever holds a key can act as its App.
4. **A current installation token of each App** in a file of its own, held as
   [the token file](#the-issue-exchange) is. GitHub makes one from the App's key: a JSON Web
   Token signed with it, sent to `POST /app/installations/INSTALLATION_ID/access_tokens`
   ([GitHub's guide](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-an-installation-access-token-for-a-github-app)).
   Each expires within the hour, and whatever you run for it writes each file again before then.
   With the issue exchange on as well, `token_file` is the landing App's token for both.
5. **The repository's merge settings** (Settings, General, Pull Requests): squash merging
   allowed, merge commits and rebase merging not, auto-merge off.
6. **The settings, landed while the default branch still takes a push.** `symphony-github.toml`,
   at the root:

   ```toml
   enabled = true
   repository = "OWNER/NAME"
   pull_requests = "publish"
   token_file = "/Users/YOU/.config/symphony-github/landing-token"
   landing_app_id = 1000001
   checks_token_file = "/Users/YOU/.config/symphony-github/checks-token"
   checks_app_id = 1000002
   policy_token_file = "/Users/YOU/.config/symphony-github/policy-token"
   policy_app_id = 1000003
   ```

   and in `symphony.toml` ([PROJECT_FILE](PROJECT_FILE.md#land)):

   ```toml
   [land]
   delivery = ["symphony-github", "deliver"]
   ```

   with `check = false` in that table where `[verdicts] check` is set: a land delivers nothing
   past a check it would have run on its own merge, and the pull request's required check takes
   its place. The land of this task merges and pushes as before, as a land reads
   `[land] delivery` from the default branch before it merges. Every land after it is delivered
   by a pull request, and until step 7 is done it is refused, pushing nothing, with each thing
   missing named.
7. **Two rulesets** on the default branch (the repository's Settings, Rules, Rulesets), both
   with enforcement Active and the default branch as their target:
   - **the requirements**, its bypass list empty: restrict deletions; block force pushes;
     require a pull request before merging, with Squash its one allowed merge method; require
     status checks to pass, adding `symphony/eligible` with the check publisher as its source,
     and "Require branches to be up to date before merging" off.
   - **the update restriction**, its one rule restrict updates, with the landing App as its
     only bypass actor.

   Other Apps installed on the repository are worth a look: one that holds Administration can
   edit these rules.

A repository that imports an upstream ([delivering an import](GITHUB.md#delivering-an-import))
differs in three places: its merge settings allow merge commits beside squash; the requirements
ruleset names Merge beside Squash, and no ruleset requires a linear history; and the settings add
`upstream_id`, the upstream's repository number, to `symphony-github.toml` and `[land.upstream]`
to `symphony.toml` ([PROJECT_FILE](PROJECT_FILE.md#landupstream)).

## Checking it

**The settings.** `symphony-github status` says what the committed settings allow, reading
nothing remote and writing nothing:

```text
enabled by the committed settings
repository: OWNER/NAME
issues: import
pull requests: off
token file: /Users/YOU/.config/symphony-github/token
nothing is kept: /PATH/TO/PROJECT/.git/symphony/integrations/github is not there
```

Its first line is the one to read: one that starts `turned off:` is a step missed
([below](#when-it-does-not-work)). With delivery set up it says `pull requests: publish` and
names the policy token's file as well.

**The exchange.** `symphony-github preview` reads GitHub and the board and writes nothing. The
first one binds the program to the repository by its number, then lists each labelled issue it
would import, with its text:

```text
1. bind this state to the repository OWNER/NAME (id 123456789)
2. import issue #7 as the backlog proposal github/issue-123456789-7
    local title: …
Nothing was written. Apply exactly this with:
  symphony-github reconcile --preview DIGEST --as HANDLE
```

Run that last line with your handle. The exchange works once `symphony-github status` says
`bound to OWNER/NAME (id 123456789)` with no `paused:` line, and a second `preview` ends
`Nothing to reconcile with OWNER/NAME.` while nobody changed an issue or a task in between.

**Delivery.** `symphony-github preflight` reads the default branch's protection as a land does
before it pushes, and writes nothing anywhere:

```text
OWNER/NAME (id 123456789, bound to nothing here yet), main: checks from App 1000002, merged by App 1000001, bypass lists read by App 1000003
holds: a delivery would go on (policy 0123abc)
```

It exits 0 on `holds` alone. The next `just land SLUG --as HANDLE` then publishes the task as a
draft pull request with its checks, and has it squashed.

## When it does not work

A refusal is one line on standard error that starts `symphony-github:`, left out below. A
`turned off:` line is said on standard output instead, and a `preview` turned off exits 0, as
reading nothing is not an error: read its first line.

| What it says | What to do |
| --- | --- |
| `turned off: the committed settings do not enable it` | The file is not on the default branch, or says no `enabled = true`. `git show main:symphony-github.toml`, with the default branch's name for `main`, prints what the program reads. |
| `turned off: its settings do not read: symphony-github.toml: …` | A key misspelt, a value of another type, or a key another needs; the rest of the line names it (`unknown field`, `enabled needs token_file`, `issues = "import" needs import_labels`). |
| `… token_file is an absolute path outside the repository` | `token_file` is relative or starts with `~`: write the whole path. |
| `token_file PATH: No such file or directory (os error 2)` | The file is not where the settings say. |
| `token_file PATH is readable by others; chmod 600 it` | `chmod 600 PATH`. |
| `token_file PATH is inside the repository; keep it outside` | Move it out of every checkout and the git folder, and name the new path. |
| `token_file PATH: the token file is empty` | Write the token in it. |
| `reading the repository: GitHub refused the credential (401)` | The token is mistyped, revoked or expired; an App's installation token lasts under an hour. The exchange pauses with everything kept until a preview succeeds. |
| `reading the repository: missing or not accessible (404)` | `repository` is misspelt, or the token was not given that repository (its repository access, an organization's approval). |
| `GitHub answered 403 to …` | The token lacks the permission that request needs: Issues, Read and write to publish or edit an issue. |
| `reading the repository: curl is not on PATH: …` | Install curl. |
| `tasks api integration_state: tasks did not start: …` | No `tasks` beside the program or on `PATH`: name it with `--tasks PROGRAM`. |
| `PATH is in no git repository` | Run it in the project's checkout. |
| `turned off: its settings do not read: the repository has no branch main` | The default branch has another name: `[project] default_branch` in `symphony.toml` says which. |
| `turned off: the committed settings do not say enabled = true and pull_requests = "publish"` (`preflight`) | The delivery's settings of step 6 are not landed yet. |
| `does not hold: a delivery would be refused`, then a `missing:` line for each thing (`preflight`) | Each line names the rule or setting to put right in steps 5 to 7: `no rule in effect for main requires a pull request` is the requirements ruleset missing or not active; `no rule restricts updates of main to the landing App` is the update restriction missing; `who bypasses ruleset N is not known` is the policy reader not installed or without Administration; `ruleset N is bypassed by …` is an actor to take off the requirements' bypass list; `symphony/eligible is required of main from another source` is the check's source to set to the check publisher; `a rule holds main's pull requests up to date` is the box to untick; `auto-merge is allowed` is the merge setting to turn off. |
| A land refused: `[land] delivery is set and so is the land's merge check` | Add `check = false` to `[land]`, as step 6 says. |
| A land refused: `the protection of main does not hold a delivery to its evidence` | As `preflight` says, which lists each thing missing. |
| A land stopped on `GitHub refused the credential (401)` | A token expired or was refused: write a current one in each file and land again. |

Every other answer, what pauses the exchange and how it recovers, is in
[what pauses or suspends](GITHUB.md#what-pauses-or-suspends) and
[delivering a land](GITHUB.md#delivering-a-land).
