# Setting up GitHub

Symphony runs without GitHub. `symphony-github` is an optional program that connects a
project's board to one repository on GitHub.com, and it does four things, each set up on its own;
the first two are a project's, the last two serve the Symphony's own releases:

- **The issue exchange.** Open issues carrying a label you choose become proposals on the board,
  and tasks you opt in are published as issues. It needs one token and no GitHub App:
  [its steps](#the-issue-exchange).
- **Delivery by pull request.** A task's move to review opens a pull request for it, writes its
  evidence there as checks and turns on GitHub's auto-merge, and GitHub squashes it once the
  evidence stands, in place of a land merging and pushing the default branch itself. It needs one
  GitHub App installed on the repository, with no administration, and one ruleset on its default
  branch, verified by someone who administers the repository:
  [its steps](#delivery-by-pull-request).
- **GitHub Releases.** A release run publishes a GitHub Release for each tag it pushes, its text
  the release's notes. It needs the exchange's one token with contents read and write, and is
  the release run's alone, so it serves the Symphony's own repository:
  [its steps](#github-releases).
- **A release verified on GitHub's runners.** A release run has its commit's full verification
  run on GitHub's macOS runners in place of the machine it runs on. It needs the same token with
  actions read and write, and is the release run's alone:
  [its steps](#a-release-verified-on-githubs-runners).

This page is the setup, in order, from nothing to a check that passes. How each part behaves,
every setting and every refusal are in [GITHUB](GITHUB.md).

## What works today, and what waits

- It is run by hand. Nothing runs the exchange on a schedule, from a webhook or from a page: a
  person runs `preview`, then `reconcile`. A land runs the delivery itself.
- One repository on GitHub.com. Another host, GitHub Enterprise among them, is refused.
- The program makes and renews the GitHub App's installation token itself, at the use that
  needs it, from the App's key ([step 6](#delivery-by-pull-request)). Nothing makes one between
  uses, and a token file with no App's key named keeps whatever you write in it.
- `pull_requests = "track"` is accepted and does nothing in this build.
- An organization's own rulesets count only where GitHub shows the repository's administrator
  their bypass lists through the repository, which was not tried: put the ruleset of
  [step 7](#delivery-by-pull-request) on the repository.

## Before you start

- A project set up as [the README](../README.md#on-your-own-project) says: a main checkout in
  git, the board made, and its `tasks` command working.
- `git` and `curl` on `PATH`: the program reads its settings with git and reaches GitHub with
  curl. For delivery, `openssl` as well, which signs the App's request for its token (macOS has
  one; most Linux systems too), and the `gh` CLI where the repository's administrator verifies
  the ruleset (step 8): no land runs it.
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
names, else the one beside it, else the one on `PATH`. A land runs it by the name `[land]
delivery` gives, and so does the dispatch a move to review starts: named bare, it must be on the
`PATH` of every session that moves a task or gives a verdict, and named by its path
(`.target/bin/symphony-github`) it need not be.

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

**Read this first.** Once the ruleset of step 7 is active, GitHub takes no push to the default
branch, from anyone: it changes only by a pull request whose `symphony/eligible` the App wrote
green. Every land, the regenerator's work and the app's document saves are delivered that way
once `[land] delivery` is set ([TASKS](TASKS.md#a-land-delivered-by-a-pull-request)); anything
else that pushes there, a person's own `git push` among them, is refused. Steps 1 to 5 change
nothing a land does. The settings of step 6 land by the ordinary merge and push, so they come
before the ruleset: from then on every land is delivered by a pull request, and refused, pushing
nothing, until steps 7 and 8 are done.

1. **One GitHub App** in the organization (its settings, Developer settings, GitHub Apps, New
   GitHub App). It takes a name and a homepage URL, any address; untick the webhook's Active box,
   set these repository permissions and no others (GitHub adds Metadata, read), and let it be
   installed only on this account. Its number, the App ID on its settings page, goes in
   `app_id`.

   | Repository permission | For |
   | --- | --- |
   | Checks: Read and write | the evidence, written as checks |
   | Contents: Read and write | the pull request's merge, and a release where `releases = "publish"` |
   | Pull requests: Read and write | the pull request, opened, marked ready, armed for auto-merge, merged and closed |
   | Issues: Read and write | only where `close_issues` is set or the issue exchange is on |
   | Actions: Read and write | only where `verification = "hosted"`: the workflow's start and the reads of its run |

   Never Administration: the App must not be able to change the rules it is held to. Who
   bypasses them is read by a person instead, in step 8.
2. **Install the App** on the one repository: Install App, then "Only select repositories". Its
   installation's number is the last part of the address of the installation's settings page.
3. **A private key** (Generate a private key, on the App's settings page), kept outside every
   checkout and readable by its owner alone. Whoever holds the key can act as the App. GitHub
   downloads it as a `.pem` file:

   ```sh
   mkdir -p ~/.config/symphony-github
   (umask 077 && cp ~/Downloads/NAME.DATE.private-key.pem ~/.config/symphony-github/app.pem)
   ```

   then delete the download.
4. **Nothing to make by hand.** The program makes the App's installation token from its key
   ([tokens it makes](GITHUB.md#tokens-it-makes)): asked for the one repository and the least its
   calls need, written to its token file, and made again before it expires. The token file need
   not be there yet. With the issue exchange on as well, the same token serves both.
5. **The repository's merge settings** (Settings, General, Pull Requests): squash merging
   allowed, merge commits and rebase merging not, and "Allow auto-merge" ticked, which `arm`
   turns on for a pull request.
6. **The settings, landed while the default branch still takes a push.** `symphony-github.toml`,
   at the root:

   ```toml
   enabled = true
   repository = "OWNER/NAME"
   pull_requests = "publish"
   token_file = "/Users/YOU/.config/symphony-github/token"
   app_id = 1000001
   installation_id = 2000001
   key_file = "/Users/YOU/.config/symphony-github/app.pem"
   ```

   `installation_id` is the number of step 2, `key_file` the key of step 3. Naming them hands the
   token file to the program, which writes it from then on. Left out, the program makes no token,
   and the file must hold a current one made otherwise.

   and in `symphony.toml` ([PROJECT_FILE](PROJECT_FILE.md#land)):

   ```toml
   [land]
   delivery = ["symphony-github", "deliver"]
   ```

   with `check = false` in that table where `[verdicts] check` is set: a land delivers nothing
   past a check it would have run on its own merge, and the pull request's required check takes
   its place. The land of this task merges and pushes as before, as a land reads
   `[land] delivery` from the default branch before it merges. Every land after it is delivered
   by a pull request, and until steps 7 and 8 are done it is refused, pushing nothing, with each
   thing missing named.
7. **One ruleset** on the default branch (the repository's Settings, Rules, Rulesets), with
   enforcement Active, the default branch as its target and its bypass list empty: restrict
   deletions; block force pushes; require a pull request before merging, with Squash its one
   allowed merge method; require status checks to pass, adding `symphony/eligible` with the App as
   its source, and "Require branches to be up to date before merging" off. No other ruleset may
   restrict updates of the branch.

   Other Apps installed on the repository are worth a look: one that holds Administration can
   edit this ruleset.
8. **Verify it**, signed in to the gh CLI as someone who administers the repository (`gh auth
   status` says who), in a checkout of the project:

   ```sh
   symphony-github rulesets --verify
   ```

   It reads the ruleset in full, its bypass list among them, which GitHub shows the App never,
   and says each thing missing; where nothing is, it keeps who bypasses each ruleset, and every
   land from then on takes that while the ruleset has not changed
   ([who bypasses the rules](GITHUB.md#who-bypasses-the-rules)). Any change to a ruleset, its
   bypass list or its enforcement, stops every delivery until it is verified again.

   On the Symphony's own repository, whose release run is the one there is, each release run can
   verify it again: commit `rulesets = ["symphony-github", "rulesets", "--verify"]` to the
   `[releases]` table of `symphony.toml` ([RELEASES](RELEASES.md#who-bypasses-the-rules-again)).
   The run starts that program by name, so it is on the `PATH` of whoever starts the release run,
   whose gh is signed in as above. A run where the policy does not hold is red, and files it.

A repository that imports an upstream ([delivering an import](GITHUB.md#delivering-an-import))
differs in three places: its merge settings allow merge commits beside squash; the ruleset
names Merge beside Squash, and no ruleset requires a linear history; and the settings add
`upstream_id`, the upstream's repository number, to `symphony-github.toml` and `[land.upstream]`
to `symphony.toml` ([PROJECT_FILE](PROJECT_FILE.md#landupstream)). With a private upstream,
leave out `installation_id` and `key_file`: a token the program makes reads the
one repository, so `token_file` stays kept by hand, with a token that reads the upstream too.

## GitHub Releases

The release run ([RELEASES](RELEASES.md#a-github-release)) is the Symphony's own repository's,
so these steps are for that repository.

1. **The token.** `token_file`'s token, as [the exchange's step 1](#the-issue-exchange) keeps it,
   with contents read and write on the repository: GitHub takes a release's creation as a write
   of the repository's contents.
2. **The adapter's word.** Commit `releases = "publish"` to `symphony-github.toml` on the default
   branch, beside `enabled = true`, `repository` and `token_file`.
3. **The run's command.** Commit `github = ["symphony-github", "release"]` to the `[releases]`
   table of `symphony.toml` ([PROJECT_FILE](PROJECT_FILE.md#releases)). The run starts that
   program by name, so it is on the `PATH` of whoever starts the release run.
4. **The releases tagged before.** The release run's `--github` asks for each release tagged in
   the checkout, oldest first, and checks, tags and pushes nothing; one GitHub holds already is
   left as it is.

`symphony-github status` then says `releases: publish`, and the release run's `--status` counts
the releases published and names each one pending with why.

## A release verified on GitHub's runners

The release run ([RELEASES](RELEASES.md#on-githubs-runners)) is the Symphony's own repository's,
so these steps are for that repository.

1. **The workflow.** The default branch commits `.github/workflows/release-verify.yml`, and the
   repository's settings allow Actions to run it.
2. **The token.** `token_file`'s token with actions read and write on the repository, which
   starts the workflow and reads its runs, logs and artifacts. Where the program makes the token,
   the App is given that permission (step 1 of [delivery](#delivery-by-pull-request)) and whoever
   owns the installation accepts it; the next token is asked for it.
3. **The adapter's word.** Commit `verification = "hosted"` to `symphony-github.toml` on the
   default branch, beside `enabled = true`, `repository` and `token_file`.
4. **The run's command.** Commit `hosted = ["symphony-github", "verify"]` to the `[releases]`
   table of `symphony.toml` ([PROJECT_FILE](PROJECT_FILE.md#releases)). The run starts that
   program by name, so it is on the `PATH` of whoever starts the release run, as `unzip` is,
   which opens each job's artifact.

`symphony-github status` then says `verification: hosted`, and each release run says which
judged it: `verify verified on GitHub (RUN)`, or `verify verified here, not on GitHub: WHY`.

## Checking it

**The settings.** `symphony-github status` says what the committed settings allow, reading
nothing remote and writing nothing:

```text
enabled by the committed settings
repository: OWNER/NAME
issues: import
pull requests: off
releases: off
token file: /Users/YOU/.config/symphony-github/token
nothing is kept: /PATH/TO/PROJECT/.git/symphony/integrations/github is not there
```

Its first line is the one to read: one that starts `turned off:` is a step missed
([below](#when-it-does-not-work)). With delivery set up it says `pull requests: publish` and
names the App's number as well.

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

**Delivery.** `symphony-github rulesets` says, as step 8 does, each ruleset and who bypasses it,
read with gh, and keeps nothing without `--verify`:

```text
OWNER/NAME (id 123456789), main: 1 ruleset(s) in effect, read with gh
  ruleset 4567890 "Symphony delivery" (active), last changed 2026-11-20T05:05:05.939+01:00: bypassed by nobody
holds; `--verify` keeps it, which a delivery takes while each ruleset is unchanged
```

`symphony-github preflight` reads the default branch's protection as a land does before it
pushes, with the App's token alone, and writes nothing anywhere:

```text
OWNER/NAME (id 123456789, bound to nothing here yet), main: checks written and merged by App 1000001, who else bypasses the rules as verified
holds: a delivery would go on (policy 0123abc)
```

It exits 0 on `holds` alone, and has made the token it needed. The next task moved to review is
then dispatched: its pull request opened with its checks and its auto-merge armed. GitHub
squashes it once `symphony/eligible` is green, which takes an assessment at its head (`just tasks
needs SLUG STAMP… --tip HEAD`) and every stamp that names standing there. A task nobody assessed
waits in review with its checks pending, and nothing merges it. A task that reached done
before the settings landed is published and armed by `just land SLUG --as HANDLE`.

**The token.** `symphony-github tokens` says whether it is current, reading nothing remote and
writing nothing, and never prints it:

```text
token_file /Users/YOU/.config/symphony-github/token: current until 2026-11-19T18:04:05Z (in 47 min)
```

An expired or absent one is not a fault where the App makes it: the next land makes it.

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
| `does not hold: a delivery would be refused`, then a `missing:` line for each thing (`preflight`, `rulesets`) | Each line names the rule or setting to put right in steps 5 to 8: `no rule in effect for main requires a pull request` is the ruleset missing or not active; `who bypasses ruleset N is not known (it was never verified …)` or `(it changed at … after it was verified …)` is step 8 to run again; `ruleset N is bypassed by …` is an actor to take off its bypass list; `symphony/eligible is required of main from another source` is the check's source to set to the App; `a rule holds main's pull requests up to date` is the box to untick; `a rule restricts updates of main` is a ruleset to delete or take that rule out of. |
| `its bypass list is not shown: gh is not signed in as someone who administers the repository` (`rulesets`) | `gh auth login` as an administrator of the repository, and run it again. |
| A land refused: `[land] delivery is set and so is the land's merge check` | Add `check = false` to `[land]`, as step 6 says. |
| A land refused: `the protection of main does not hold a delivery to its evidence` | As `preflight` says, which lists each thing missing. |
| A land stopped on `GitHub refused the credential (401)` | A token was refused, or expired in a file whose App's key is not named: name the keys of step 6, or write a current token in it, and land again. Where GitHub revoked a token the program made before it expires, delete its file: the next land makes a new one. |
| `key_file PATH is readable by others; chmod 600 it` | `chmod 600 PATH`. A key not there, or inside the repository, is said the same way. |
| `… checks_token_file is gone: one GitHub App …` (or another `checks_`, `landing_` or `policy_` key) | The settings of three Apps: write the one App's `app_id`, `installation_id` and `key_file` (step 6), and land them. |
| `openssl is not on PATH: … so install it; nothing was sent` | Install openssl, which signs the App's request for its token. |
| `openssl could not sign with key_file PATH: write the App's private key there as GitHub gave it, a PEM file with no passphrase` | The file is not the key GitHub gave: generate a key again (step 3) and copy it there. |
| `making token_file's token as App N: GitHub refused the App's JWT (401: …)` | GitHub's words follow the status. The key is not App N's, `app_id` is not that App's number (check both on the App's settings page), or this machine's clock is more than a minute ahead of GitHub's or nine minutes behind it (a message naming `iat` or `exp` says so): set the clock right. |
| `… GitHub found no installation N of App M (404)` | `installation_id` is not the installation's number (step 2), or the App is not installed. |
| `… GitHub refused a token for OWNER/NAME with … (422: …)` | GitHub's words follow the status: give the App the permissions named (step 1) and install it on that repository (step 2). |
| `making …'s token as App N: GitHub answered 5XX` (`unavailable`) | GitHub did not answer: the file is as it was, and the next land asks again. |

Every other answer, what pauses the exchange and how it recovers, is in
[what pauses or suspends](GITHUB.md#what-pauses-or-suspends) and
[delivering a land](GITHUB.md#delivering-a-land).
