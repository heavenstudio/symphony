# The commands

Which command to reach for, and where each is explained. Every command prints its own arguments
and options, and that help is the reference: this page names the commands and repeats none of it.

```sh
just tasks help              # every board command, by group
just tasks help COMMAND      # one board command
just symphony-cli help       # every document command
just symphony-cli COMMAND --help
just symphony --help         # the server
```

## The recipes

`symphony init` writes these eight recipes in `symphony.just`, which the project's `justfile`
imports. Each runs an installed command; a recipe of the same name in the project's own
`justfile` replaces it.

| Recipe | Runs | For |
| --- | --- | --- |
| `just tasks …` | `tasks` | the task board |
| `just land SLUG` | `tasks land` | landing a task's approved branch ([the default landing](../workflow/TASKS.md#the-default-landing)) |
| `just agent-send HANDLE TEXT` | `agent-send` | a message to another agent's session |
| `just symphony` | `symphony-server` | the app |
| `just symphony-cli …` | `symphony` | the document commands |
| `just symphony-status` | `symphony status` | the cards and comments in the terminal |
| `just symphony-reply PATH SLUG TEXT` | `symphony reply` | an agent's line under a card or a comment |
| `just symphony-ask …` | `symphony ask` | the questions asked in the app |

A recipe runs the project's own build of a command when it has one linked at `.target/bin/`, else
the release installed on the machine, else the command on `PATH`.

## The app

| Command | What it does |
| --- | --- |
| `just symphony` | starts the machine's one server when it is not running, registers this project and prints its address |
| `just symphony --open` | the same, and opens the address in the browser |
| `just symphony --if-up` | prints the address when the server runs, and starts nothing when it does not (exit 3) |
| `just symphony --stop` | stops the machine's server |
| `just symphony --port N` | a private server for this checkout, in the foreground; `--port 0` takes any free port |
| `just symphony --port N --root DIR` | a private server for another folder |
| `just symphony --port N --host ADDRESS` | the address a private server listens on, repeatable; the default is every interface, each request judged by its client's address |
| `just symphony-cli project add [DIR]` | registers a project with the machine's server without starting it |

The machine's server answers this machine and confirmed peers of its tailnet. How it is put
together is [the server guide](../../apps/symphony-server/README.md).

## Documents and review

| Command | What it does | More |
| --- | --- | --- |
| `just symphony-status` | counts the cards by where they wait, and the open comments | [REVIEW](../workflow/REVIEW.md#two-commands) |
| `just symphony-status --open` | only what waits on someone; `--json` gives it as data | |
| `just symphony-status --tidy` | marks plainly approved cards resolved, regroups each register, archives answered comments | [REVIEW](../workflow/REVIEW.md#groups) |
| `just symphony-status --lint` | fails on a card's name in a document's prose and on what a plugin's rules refuse | [REVIEW](../workflow/REVIEW.md#slugs-in-prose) |
| `just symphony-reply PATH SLUG TEXT` | an agent's dated line under a card; `--question` asks you back | [REVIEW](../workflow/REVIEW.md#card-history) |
| `just symphony-ask list --open` | the questions you asked in the app that still wait; `show ID` and `answer ID TEXT --as HANDLE` read and answer one | [TASKS](../workflow/TASKS.md#asks) |
| `just symphony-cli docs-audit --check` | fails when a document does not render, a waiting card cannot be answered or a picture is not committed | [REVIEW](../workflow/REVIEW.md#writing-a-card) |
| `just symphony-cli settings-check` | checks the project file's shape | [the project file](PROJECT_FILE.md#the-settings-check) |
| `just symphony-cli model ROLE MODEL` | sets the model a role's agents run on | [the project file](PROJECT_FILE.md#agentsmodels) |
| `just symphony-cli triage stats` | your verdicts counted; `triage queue N` the cards you see first | [tool commands](../engineering/TOOL_COMMANDS.md) |
| `just symphony-cli release-notes` | what the project changes to take each release after its own | [RELEASES](../workflow/RELEASES.md#taking-a-release) |
| `just symphony-cli init` | installs or refreshes the skills, recipes and hooks | [getting started](GETTING_STARTED.md#2-set-the-project-up) |

`render-card` and `landing-checks` are for a project's own tools, which read their JSON
([tool commands](../engineering/TOOL_COMMANDS.md)); `serve` answers such requests on its standard input, one a
line.

### Pictures of pages

`just symphony-cli shot URL --out FILE.png` opens a page in a private headless Chrome, waits until
it has drawn and saves a picture. It is how an agent sees a page and shows you one: the before
and after of a change it made to a page. Steps run in the order given before the capture
(`--click CSS`, `--type CSS=TEXT`, `--key NAME`, `--hover CSS`, `--eval JS`, `--wait-for CSS`,
`--shot FILE.png` for a picture mid-way), and `--phone`, `--size WxH`, `--full-page` and
`--scheme dark` set the view. `--report` prints the console errors, failed requests and sideways
overflow it saw. It needs Chrome installed, or named by `SYMPHONY_CHROME`.

`just symphony-cli attach FILE…` stores files by their content in the main checkout's uploads
and prints the line a task's thread shows them by, so a picture kept as a task's evidence
outlives its worktree; `shot --attach` does both.

## The board

`just tasks help` groups the commands as below. `--as HANDLE` names the session that acts, on
every command.

| To | Use |
| --- | --- |
| join the board | `register` prints a handle; `callback` has messages reach the session; `leave` gives its work back |
| see the work | `list` (the open tasks), `show SLUG` (one task: roles, brief, thread), `search WORDS`, `next` (what to take) |
| see who is there | `who` (sessions), `agents` (agents and their terminals), `status` (the board's health), `limits` |
| file work | `create SLUG TITLE --body-file FILE`, then `priority`, `parent`, `depends` |
| do a task | `claim SLUG`, `say SLUG TEXT --to @orchestrator`, `move SLUG review`, `done SLUG TEXT` for a subtask |
| let one go | `release SLUG`, or `move SLUG blocked --on … --why …` when it cannot go on |
| review one | `role SLUG reviewer`, `runs SLUG` (what its builds and tests recorded), `verdict SLUG approve` or `changes` |
| land one | `just land SLUG`; `stamps SLUG` says what its landing still needs |
| read your messages | `inbox`, `pull` (an orchestrator's digest), `asks` (what you wait on an agent for) |
| run agents in terminals | `launch SLUG`, `restore SLUG`, `close`, `cleanup`, `finish SLUG` (these need Herdr) |
| tidy up | `sweep` removes the worktrees of landed branches |
| look after the board | `migrate`, `snapshot`, `export`, `database`, `recovery`, `schema` ([operating the board](BOARD_OPERATIONS.md)) |

[TASKS](../workflow/TASKS.md) explains each: [tasks and roles](../workflow/TASKS.md#tasks-and-roles),
[what a task waits on](../workflow/TASKS.md#what-a-task-waits-on), [the default landing](../workflow/TASKS.md#the-default-landing),
[the stamps a landing needs](../workflow/TASKS.md#needed-stamps), [subtasks](../workflow/TASKS.md#subtasks),
[limits and models](../workflow/TASKS.md#limits-and-models), [talking on a task](../workflow/TASKS.md#talking-on-a-task).

## GitHub

`symphony-github` is a program of its own, built apart; its commands are in
[GITHUB](GITHUB.md#the-commands) and its setup in [setting up GitHub](GITHUB_SETUP.md).

## Environment

A project sets none of these to run. Each changes where something is found.

| Variable | What it names |
| --- | --- |
| `SYMPHONY_SURREAL` | a SurrealDB binary to run the board on, in place of the one `init` fetches |
| `SYMPHONY_CHROME` | the Chrome `shot` drives, in place of the installed one |
| `SYMPHONY_WT_DIR` | the folder a launched agent's worktree is made under, in place of `<project>-wt/` beside the checkout |
| `SYMPHONY_MACHINE_DIR` | the machine folder (the server's registry and the installed release), which tests point at a temporary one |
| `TASKS_SESSION` | the handle `--as` defaults to |
