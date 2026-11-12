//! What `--help`, `help [COMMAND]` and a usage error print. The top help lists every command once,
//! with one line on what it does ([`GROUPS`]); a command's own help is its usage, what it does and
//! its arguments, laid out 78 columns wide (a pipe, where agents run it), in argparse's words, which
//! `tools/tasks.py`'s parser printed. A flag or a command added adds itself here too.

mod launch;
mod panes;

use super::Env;

/// What `--help` prints in `env`: the top help for no command, else `command`'s own help, which
/// says what the command does first when its text does not ([`summary`]); launch's then says what
/// a bare launch starts there ([`launch::shown`]).
pub fn shown(command: Option<&str>, env: &Env) -> String {
    let Some(name) = command else {
        return top();
    };
    if name == super::recorder::COMMAND {
        return super::recorder::help();
    }
    if name == super::migrate::COMMAND {
        return super::migrate::HELP.to_string();
    }
    let help = COMMANDS
        .iter()
        .find(|(n, _, _)| *n == name)
        .map_or("", |(_, _, help)| *help);
    let help = if help == launch::HELP {
        launch::shown(env)
    } else {
        help.to_string()
    };
    described(&help, summary(name))
}

/// `help` with `said` after its usage block when what follows that block is only its arguments.
fn described(help: &str, said: Option<&str>) -> String {
    let Some((usage, rest)) = help.split_once("\n\n") else {
        return match said {
            Some(said) => format!("{help}\n{said}\n"),
            None => help.to_string(),
        };
    };
    match said {
        Some(said) if rest.starts_with("positional arguments:") || rest.starts_with("options:") => {
            format!("{usage}\n\n{said}.\n\n{rest}")
        }
        _ => help.to_string(),
    }
}

/// The usage line of `just tasks` itself, which a usage error before any command starts with.
pub const USAGE: &str = "usage: just tasks [-h] [--as ACTOR] COMMAND ...\n";

/// Every command `just tasks` runs, by what it is for, each with one line on what it does: what the
/// top help lists, in this order. `run` and `help` are read before the board's argument table.
pub const GROUPS: &[(&str, &[(&str, &str)])] = &[
    (
        "Sessions",
        &[
            (
                "register",
                "join the board as a new session and print its handle",
            ),
            (
                "callback",
                "register your native channel, or --off to read your inbox",
            ),
            ("inbox", "print the messages addressed to you, once each"),
            ("pull", "print the whole PULL digest as you hear it now"),
            (
                "wait",
                "wait for what a callback would push a session with no channel",
            ),
            (
                "leave",
                "leave the board: your roles go, work in progress is ready again",
            ),
            ("who", "every session: its kind, health, branch and roles"),
            (
                "agents",
                "every agent: its state, health, task, role, branch and terminal",
            ),
            (
                "attach",
                "register and watch the board from this terminal (a monitor)",
            ),
            (
                "beat",
                "a hook: beat for the subagent making a Claude Code tool call",
            ),
        ],
    ),
    (
        "Reading the board",
        &[
            (
                "list",
                "the open tasks as a tree (--all adds done and cancelled)",
            ),
            (
                "show",
                "one task: its roles, stages, subtasks, brief and thread",
            ),
            (
                "search",
                "rank tasks by words in their titles, briefs and threads",
            ),
            (
                "next",
                "the task to take next: your reservation, else the most urgent",
            ),
            (
                "stats",
                "where the pipeline's time goes, over the last days",
            ),
            (
                "status",
                "the board's health: failed dispatches, slow requests, writes",
            ),
            (
                "asks",
                "what the owner waits on an agent for, and who holds each",
            ),
            ("export", "write a markdown snapshot of the board"),
            (
                "embed",
                "catch up the local embeddings `search --mode vector` reads",
            ),
        ],
    ),
    (
        "Filing work",
        &[
            ("create", "file a task: SLUG (area/short-name) and TITLE"),
            ("priority", "set a task's priority, 0 (most urgent) to 4"),
            ("parent", "put a task under another, or --none"),
            ("depends", "change what a task waits on"),
            ("field", "set a plugin's board field on a task"),
        ],
    ),
    (
        "Doing work",
        &[
            ("claim", "take a task as its executor"),
            (
                "reserve",
                "hold a task for an hour, so nobody else claims it",
            ),
            ("release", "let your roles on a task go, or one --role"),
            ("role", "join a task as orchestrator or reviewer"),
            (
                "handover",
                "hand a task's orchestrator role to another session",
            ),
            ("move", "move a task to another status"),
            (
                "say",
                "post on a task's thread, --to owner, @orchestrator or HANDLE",
            ),
            (
                "verdict",
                "as its reviewer: approve (done) or changes (back to work)",
            ),
            ("done", "close an instruction subtask with what came of it"),
            (
                "land",
                "land an approved branch and push it; `just land SLUG` runs it",
            ),
        ],
    ),
    (
        "Agents' terminals",
        &[
            (
                "launch",
                "start an agent on a task in a terminal of its own",
            ),
            (
                "restore",
                "bring a closed agent's conversation back in a terminal",
            ),
            ("close", "close one agent's terminal, whatever it holds"),
            ("cleanup", "close the terminals nobody needs any more"),
            ("clean", "cleanup, then one idle sweep, at once"),
            (
                "finish",
                "close the terminals of a finished task's sessions",
            ),
            (
                "reap",
                "the idle sweep: close terminals whose agents ended or sat idle",
            ),
            (
                "staff",
                "one staffing pass over ready work (tools/task_staff.json)",
            ),
            (
                "sweep",
                "remove worktrees merged into main that nobody needs",
            ),
        ],
    ),
    (
        "The board, builds and help",
        &[
            (
                "migrate",
                "move the board forward to this build's schema, or make it",
            ),
            (
                "run",
                "run a build or test, recorded for the Build and Test columns",
            ),
            ("help", "this list, or a command's own help: help COMMAND"),
        ],
    ),
];

/// The commands read before the board's argument table ([`super::args::COMMANDS`]), each with its
/// own parser and help, which [`shown`] answers `help COMMAND` with: the tests hold the dispatch
/// and the help to them.
#[cfg(test)]
pub const BEFORE_TABLE: [&str; 2] = [super::recorder::COMMAND, super::migrate::COMMAND];

/// What `command` does, in one line.
pub fn summary(command: &str) -> Option<&'static str> {
    GROUPS
        .iter()
        .flat_map(|(_, commands)| commands.iter())
        .find(|(name, _)| *name == command)
        .map(|(_, said)| *said)
}

/// Every command name, in the top help's order.
pub fn names() -> impl Iterator<Item = &'static str> {
    GROUPS
        .iter()
        .flat_map(|(_, commands)| commands.iter().map(|(name, _)| *name))
}

/// `just tasks --help`, `just tasks help`: every command, once, with what it does.
pub fn top() -> String {
    let width = names().map(str::len).max().unwrap_or(0);
    let mut text = format!(
        "{USAGE}\nThe task board from the terminal. `just tasks help COMMAND`, or\n\
         `just tasks COMMAND --help`, shows a command's arguments and options.\n"
    );
    for (group, commands) in GROUPS {
        text.push_str(&format!("\n{group}:\n"));
        for (name, said) in *commands {
            text.push_str(&format!("  {name:<width$}  {said}\n"));
        }
    }
    text.push_str(
        "\noptions:\n  -h, --help       show this help message and exit\n  --as ACTOR       act as \
         this session handle, before or after the command\n                   (default: \
         $TASKS_SESSION)\n  --database NAME  the scratch board NAME, not the project's; it comes \
         first,\n                   before every other word, and sweeps no repository\n",
    );
    text
}

/// Each command's usage and `--help`, in the order the top usage names them.
pub const COMMANDS: &[(&str, &str, &str)] = &[
    (
        "callback",
        r#"usage: just tasks callback [-h] [--as ACTOR] [--chat] [--if-free] [--off]
                           [--session-url SESSION_URL]
                           [address]
"#,
        r#"usage: just tasks callback [-h] [--as ACTOR] [--chat] [--if-free] [--off]
                           [--session-url SESSION_URL]
                           [address]

Register your session's native channel, so what is addressed to you starts a
turn directly. With no address it detects this Claude Code session's socket or
Codex thread, and refuses when there is neither. An address another registered
handle holds is refused until that one unregisters or leaves, idle or not. An
in-process subagent has no channel of its own: it reads `just tasks inbox`.

positional arguments:
  address               uds:/absolute/socket or codex:THREAD-UUID (default:
                        detected)

options:
  -h, --help            show this help message and exit
  --as ACTOR
  --chat                take the seat that hears the owner's chat
  --if-free             with --chat: take the seat only when nobody holds it,
                        else stand next in line for it
  --off                 unregister: what is addressed to you waits in your
                        inbox
  --session-url SESSION_URL
                        the web address Go to session links to: https:// or
                        http://, an empty one clearing it; with it and no
                        address given or detected, only that is set
"#,
    ),
    (
        "register",
        r#"usage: just tasks register [-h] [--as ACTOR]
                           [--kind {orchestrator,worker,reviewer,chat,app}]
                           [--parent PARENT] [--affinity AFFINITY]
                           [--model MODEL] [--session-url SESSION_URL]
"#,
        r#"usage: just tasks register [-h] [--as ACTOR]
                           [--kind {orchestrator,worker,reviewer,chat,app}]
                           [--parent PARENT] [--affinity AFFINITY]
                           [--model MODEL] [--session-url SESSION_URL]

options:
  -h, --help            show this help message and exit
  --as ACTOR
  --kind {orchestrator,worker,reviewer,chat,app}
  --parent PARENT
  --affinity AFFINITY   comma-separated areas, e.g. sim,view
  --model MODEL
  --session-url SESSION_URL
                        the web address Go to session links to: https:// or
                        http://
"#,
    ),
    (
        "attach",
        r#"usage: just tasks attach [-h] [--as ACTOR]
                         [--kind {orchestrator,worker,reviewer,chat,app}]
                         [--parent PARENT] [--affinity AFFINITY]
                         [--model MODEL] [--resume RESUME]
                         [--interval INTERVAL] [--beat BEAT]
"#,
        r#"usage: just tasks attach [-h] [--as ACTOR]
                         [--kind {orchestrator,worker,reviewer,chat,app}]
                         [--parent PARENT] [--affinity AFFINITY]
                         [--model MODEL] [--resume RESUME]
                         [--interval INTERVAL] [--beat BEAT]

options:
  -h, --help            show this help message and exit
  --as ACTOR
  --kind {orchestrator,worker,reviewer,chat,app}
  --parent PARENT
  --affinity AFFINITY   comma-separated areas, e.g. sim,view
  --model MODEL
  --resume RESUME       take an existing handle back (a re-armed monitor)
  --interval INTERVAL
  --beat BEAT
"#,
    ),
    (
        "list",
        r#"usage: just tasks list [-h] [--as ACTOR]
                       [--status {backlog,concept,design,ready,in_progress,review,done,blocked,cancelled}]
                       [--mine] [--doubled] [--all] [--json]
"#,
        r#"usage: just tasks list [-h] [--as ACTOR]
                       [--status {backlog,concept,design,ready,in_progress,review,done,blocked,cancelled}]
                       [--mine] [--doubled] [--all] [--json]

The board's open tasks as a tree, each subtask under its parent: done and
cancelled ones too with --all, or with --status naming either.

options:
  -h, --help            show this help message and exit
  --as ACTOR
  --status {backlog,concept,design,ready,in_progress,review,done,blocked,cancelled}
                        only tasks in this status
  --mine                only tasks you hold a role on
  --doubled             only tasks holding two or more orchestrators
  --all                 done and cancelled tasks too
  --json
"#,
    ),
    (
        "search",
        r#"usage: just tasks search [-h] [--as ACTOR]
                         [--status {backlog,concept,design,ready,in_progress,review,done,blocked,cancelled}]
                         [--limit N] [--json] [--mode {keyword,vector,hybrid,exact}] query
"#,
        r#"usage: just tasks search [-h] [--as ACTOR]
                         [--status {backlog,concept,design,ready,in_progress,review,done,blocked,cancelled}]
                         [--limit N] [--json] [--mode {keyword,vector,hybrid,exact}] query

Rank full-text matches in task titles, bodies, slugs and thread messages.

positional arguments:
  query                 at most 1024 UTF-8 bytes and 32 terms (word/number runs
                        and punctuation/symbols, after lowercasing); longer queries
                        are refused with exit 1

options:
  -h, --help            show this help message and exit
  --as ACTOR
  --status {backlog,concept,design,ready,in_progress,review,done,blocked,cancelled}
                        filter by task status before limiting results
  --limit N             positive maximum number of results (default: 20)
  --json                list-compatible task records with score, snippet and snippet_source
  --mode {keyword,vector,hybrid,exact}
                        keyword is the default; vector uses local embeddings, hybrid fuses
                        both rankings, exact checks vector recall; the shared server catches up
"#,
    ),
    (
        "stats",
        "usage: just tasks stats [-h] [--as ACTOR] [--days N] [--json]\n",
        r#"usage: just tasks stats [-h] [--as ACTOR] [--days N] [--json]

Where the pipeline's time goes, over the top-level tasks made in the window:
tasks by status; for the done ones, lead time and the time in each status
(median, p75, p90); how often review sent work back; the ten open tasks
longest in their status; blocked and unstaffed ready work. Done per day
counts every top-level task closed in the window, whenever it was made.

options:
  -h, --help  show this help message and exit
  --as ACTOR
  --days N    how many days back, 1 to 3650 (default: 14)
  --json      the same figures, every duration in seconds
"#,
    ),
    (
        "embed",
        "usage: just tasks embed [-h] [--as ACTOR] [--task SLUG] [--batch-size N] [--threads N] [--maintenance]\n",
        "usage: just tasks embed [-h] [--as ACTOR] [--task SLUG] [--batch-size N] [--threads N] [--maintenance]\n\nCatch up local BGE embeddings for missing or changed title/brief and thread chunks.\nDownloads checksum-pinned model artifacts when changed sources need them. No board text is sent out.\nThe shared Symphony server runs maintenance automatically; use this command for repair.\n\noptions:\n  -h, --help       show this help message and exit\n  --as ACTOR\n  --task SLUG      catch up just this task (default: the whole board)\n  --batch-size N   CPU inference batch size, 1 to 64 (default: 8)\n  --threads N      inference threads, 1 to 64 (default: 1)\n  --maintenance   cache-only server pass; no agent session required\n",
    ),
    (
        "next",
        r#"usage: just tasks next [-h] [--as ACTOR]
"#,
        r#"usage: just tasks next [-h] [--as ACTOR]

options:
  -h, --help  show this help message and exit
  --as ACTOR
"#,
    ),
    (
        "create",
        r#"usage: just tasks create [-h] [--as ACTOR] [--body BODY]
                         [--body-file BODY_FILE] [--priority {0,1,2,3,4}]
                         [--stage {backlog,concept,design,ready}] [--backlog]
                         [--parent PARENT] [--for ASSIGNEE] [--instruction]
                         [--depends DEPENDS] [--link LINK]
                         [--fast-fix FAST_FIX]
                         slug title
"#,
        r#"usage: just tasks create [-h] [--as ACTOR] [--body BODY]
                         [--body-file BODY_FILE] [--priority {0,1,2,3,4}]
                         [--stage {backlog,concept,design,ready}] [--backlog]
                         [--parent PARENT] [--for ASSIGNEE] [--instruction]
                         [--depends DEPENDS] [--link LINK]
                         [--fast-fix FAST_FIX]
                         slug title

positional arguments:
  slug                  area/short-name, also the branch's name: lowercase
                        letters, digits and dashes, such as tools/cli-help
  title

options:
  -h, --help            show this help message and exit
  --as ACTOR
  --body BODY           the brief
  --body-file BODY_FILE
                        read the brief from this file, or stdin for -
  --priority {0,1,2,3,4}
                        0 is the most urgent (default: 2); p0 to p4 read the
                        same
  --stage {backlog,concept,design,ready}
                        where it enters the pipeline (default: ready)
  --backlog             same as --stage backlog
  --parent PARENT       file it as a subtask of this task
  --for ASSIGNEE        reserve it for this handle, with no lapse (nobody else
                        is offered it)
  --instruction         a subtask its executor closes with `done`; its slug is
                        never a branch that lands
  --depends DEPENDS     a task that must be done first (repeatable)
  --link LINK           a document it implements, docs/X.md#Heading
                        (repeatable)
  --fast-fix FAST_FIX   a bug fix whose recorded risk, FAST_FIX, calls for no
                        reviewer: its orchestrator approves it (RULES.md
                        workflow/fast-bugfixes)
"#,
    ),
    (
        "priority",
        "usage: just tasks priority [-h] [--as ACTOR] slug {0,1,2,3,4}\n",
        r#"usage: just tasks priority [-h] [--as ACTOR] slug {0,1,2,3,4}

Set an existing task's priority (0 is most urgent); p0 to p4 read the same.

positional arguments:
  slug
  {0,1,2,3,4}

options:
  -h, --help    show this help message and exit
  --as ACTOR
"#,
    ),
    (
        "field",
        "usage: just tasks field [-h] [--as ACTOR] slug name value\n",
        r#"usage: just tasks field [-h] [--as ACTOR] slug name value

Set a plugin's board field on a task; the Symphony server validates it first

positional arguments:
  slug
  name        the field, or PLUGIN.FIELD
  value

options:
  -h, --help  show this help message and exit
  --as ACTOR
"#,
    ),
    (
        "claim",
        r#"usage: just tasks claim [-h] [--as ACTOR] slug
"#,
        r#"usage: just tasks claim [-h] [--as ACTOR] slug

positional arguments:
  slug

options:
  -h, --help  show this help message and exit
  --as ACTOR
"#,
    ),
    (
        "reserve",
        r#"usage: just tasks reserve [-h] [--as ACTOR] slug
"#,
        r#"usage: just tasks reserve [-h] [--as ACTOR] slug

positional arguments:
  slug

options:
  -h, --help  show this help message and exit
  --as ACTOR
"#,
    ),
    (
        "release",
        r#"usage: just tasks release [-h] [--as ACTOR]
                          [--role {orchestrator,executor,reviewer,reserved}]
                          slug
"#,
        r#"usage: just tasks release [-h] [--as ACTOR]
                          [--role {orchestrator,executor,reviewer,reserved}]
                          slug

positional arguments:
  slug

options:
  -h, --help            show this help message and exit
  --as ACTOR
  --role {orchestrator,executor,reviewer,reserved}
"#,
    ),
    (
        "show",
        r#"usage: just tasks show [-h] [--as ACTOR] [--json] slug
"#,
        r#"usage: just tasks show [-h] [--as ACTOR] [--json] slug

positional arguments:
  slug

options:
  -h, --help  show this help message and exit
  --as ACTOR
  --json
"#,
    ),
    (
        "parent",
        r#"usage: just tasks parent [-h] [--as ACTOR] [--none] slug [parent]
"#,
        r#"usage: just tasks parent [-h] [--as ACTOR] [--none] slug [parent]

positional arguments:
  slug
  parent

options:
  -h, --help  show this help message and exit
  --as ACTOR
  --none      take it out from under its parent
"#,
    ),
    (
        "depends",
        r#"usage: just tasks depends [-h] [--as ACTOR] [--add ADD] [--remove REMOVE]
                          [--replace OLD NEW]
                          slug
"#,
        r#"usage: just tasks depends [-h] [--as ACTOR] [--add ADD] [--remove REMOVE]
                          [--replace OLD NEW]
                          slug

Change what a task waits on, on its thread. A task a cancelled prerequisite
blocked goes back to the status its block line recorded once nothing it waits
on is cancelled.

positional arguments:
  slug

options:
  -h, --help         show this help message and exit
  --as ACTOR
  --add ADD          wait on this task too (repeatable)
  --remove REMOVE    stop waiting on this task (repeatable)
  --replace OLD NEW  wait on NEW instead of OLD (repeatable)
"#,
    ),
    (
        "done",
        r#"usage: just tasks done [-h] [--as ACTOR] slug note
"#,
        r#"usage: just tasks done [-h] [--as ACTOR] slug note

positional arguments:
  slug
  note

options:
  -h, --help  show this help message and exit
  --as ACTOR
"#,
    ),
    (
        "role",
        r#"usage: just tasks role [-h] [--as ACTOR] slug {orchestrator,reviewer}
"#,
        r#"usage: just tasks role [-h] [--as ACTOR] slug {orchestrator,reviewer}

positional arguments:
  slug
  {orchestrator,reviewer}

options:
  -h, --help            show this help message and exit
  --as ACTOR
"#,
    ),
    (
        "handover",
        r#"usage: just tasks handover [-h] [--as ACTOR] slug to
"#,
        r#"usage: just tasks handover [-h] [--as ACTOR] slug to

Move the orchestrator role on a task you orchestrate to another session, in one
step: the thread names the new orchestrator, who hears one ROLE event; you made
the move and are not told it.

positional arguments:
  slug
  to          the handle of the new orchestrator

options:
  -h, --help  show this help message and exit
  --as ACTOR
"#,
    ),
    (
        "move",
        r#"usage: just tasks move [-h] [--as ACTOR]
                       slug
                       {backlog,concept,design,ready,in_progress,review,done,blocked,cancelled}
"#,
        r#"usage: just tasks move [-h] [--as ACTOR]
                       slug
                       {backlog,concept,design,ready,in_progress,review,done,blocked,cancelled}

positional arguments:
  slug
  {backlog,concept,design,ready,in_progress,review,done,blocked,cancelled}

options:
  -h, --help            show this help message and exit
  --as ACTOR
"#,
    ),
    (
        "verdict",
        r#"usage: just tasks verdict [-h] [--as ACTOR] slug {approve,changes} [text]
"#,
        r#"usage: just tasks verdict [-h] [--as ACTOR] slug {approve,changes} [text]

positional arguments:
  slug
  {approve,changes}
  text

options:
  -h, --help         show this help message and exit
  --as ACTOR
"#,
    ),
    (
        "say",
        r#"usage: just tasks say [-h] [--as ACTOR] [--to TO] slug text
"#,
        r#"usage: just tasks say [-h] [--as ACTOR] [--to TO] slug text

Post a line on a task's thread. Only an addressed line is pushed to anyone; an
unaddressed one stays on the thread, for `just tasks show SLUG` and the app.

positional arguments:
  slug
  text

options:
  -h, --help  show this help message and exit
  --as ACTOR
  --to TO     owner, a role on the task (@orchestrator, @executor, @reviewer,
              @reserved), or a session's HANDLE
"#,
    ),
    ("launch", launch::USAGE, launch::HELP),
    ("restore", panes::RESTORE.0, panes::RESTORE.1),
    ("cleanup", panes::CLEANUP.0, panes::CLEANUP.1),
    ("clean", panes::CLEAN.0, panes::CLEAN.1),
    ("close", panes::CLOSE.0, panes::CLOSE.1),
    ("reap", panes::REAP.0, panes::REAP.1),
    (
        "staff",
        r#"usage: just tasks staff [-h] [--as ACTOR] [--dry-run]
"#,
        r#"usage: just tasks staff [-h] [--as ACTOR] [--dry-run]

options:
  -h, --help  show this help message and exit
  --as ACTOR
  --dry-run   preview staffing even when disabled; print only, change nothing
"#,
    ),
    (
        "agents",
        r#"usage: just tasks agents [-h] [--as ACTOR] [--json] [--all]
"#,
        r#"usage: just tasks agents [-h] [--as ACTOR] [--json] [--all]

options:
  -h, --help  show this help message and exit
  --as ACTOR
  --json
  --all       include sessions that have left
"#,
    ),
    (
        "who",
        r#"usage: just tasks who [-h] [--as ACTOR] [--all] [--json]
"#,
        r#"usage: just tasks who [-h] [--as ACTOR] [--all] [--json]

options:
  -h, --help  show this help message and exit
  --as ACTOR
  --all       include sessions that have left
  --json
"#,
    ),
    (
        "status",
        r#"usage: just tasks status [-h] [--as ACTOR] [--json]
"#,
        r#"usage: just tasks status [-h] [--as ACTOR] [--json]

the board's health over the last hour, as the Symphony server serving it last
recorded it in sf-tasks/health.json: its callback dispatch passes and idle
sweeps, ok and failed, the slowest board request and the line that sent it,
and write latency; read without asking the board

options:
  -h, --help  show this help message and exit
  --as ACTOR
  --json      the record as /api/health answers it, the file and the warnings
"#,
    ),
    (
        "asks",
        "usage: just tasks asks [-h] [--as ACTOR] [--all] [--json] [close ID WHY]\n",
        r#"usage: just tasks asks [-h] [--as ACTOR] [--all] [--json] [close ID WHY]

What the owner said that waits on an agent's answer: a verdict with a note or
a rejection, a message on a card, a comment, a chat, a message on a task. Each
is an ask, never a task: open, oldest first, with who holds it and for how
long, late past ten minutes; or answered. `close ID WHY` closes one by hand,
as needing no answer.

positional arguments:
  close ID WHY  close ask ID, saying why it needs no answer (needs --as)

options:
  -h, --help    show this help message and exit
  --as ACTOR
  --all         the asks answered in the last day too, the newest first
  --json        the asks as the board holds them
"#,
    ),
    (
        "inbox",
        r#"usage: just tasks inbox [-h] [--as ACTOR]
"#,
        r#"usage: just tasks inbox [-h] [--as ACTOR]

options:
  -h, --help  show this help message and exit
  --as ACTOR
"#,
    ),
    (
        "pull",
        r#"usage: just tasks pull [-h] [--as ACTOR]
"#,
        r#"usage: just tasks pull [-h] [--as ACTOR]

Print the whole PULL digest as ACTOR hears it now; a callback carries only what
changed since the last PULL it received.

options:
  -h, --help  show this help message and exit
  --as ACTOR
"#,
    ),
    (
        "wait",
        r#"usage: just tasks wait [-h] [--as ACTOR] [--timeout TIMEOUT]
"#,
        r#"usage: just tasks wait [-h] [--as ACTOR] [--timeout TIMEOUT]

Print what a callback would push ACTOR, a session with no channel, once there
is any: what changed since its last read, said once. Run it as a background
command, and again when it exits; it beats for ACTOR while it waits.

options:
  -h, --help         show this help message and exit
  --as ACTOR
  --timeout TIMEOUT  give up after this many seconds, saying nothing changed
                     (0: look once); default: wait until something does
"#,
    ),
    (
        "leave",
        r#"usage: just tasks leave [-h] [--as ACTOR]
"#,
        r#"usage: just tasks leave [-h] [--as ACTOR]

options:
  -h, --help  show this help message and exit
  --as ACTOR
"#,
    ),
    (
        "beat",
        r#"usage: just tasks beat [-h] [--as ACTOR]
"#,
        r#"usage: just tasks beat [-h] [--as ACTOR]

options:
  -h, --help  show this help message and exit
  --as ACTOR
"#,
    ),
    (
        "export",
        r#"usage: just tasks export [-h] [--as ACTOR] [--out OUT]
"#,
        r#"usage: just tasks export [-h] [--as ACTOR] [--out OUT]

options:
  -h, --help  show this help message and exit
  --as ACTOR
  --out OUT
"#,
    ),
    (
        "finish",
        r#"usage: just tasks finish [-h] [--as ACTOR] slug
"#,
        r#"usage: just tasks finish [-h] [--as ACTOR] slug

positional arguments:
  slug

options:
  -h, --help  show this help message and exit
  --as ACTOR
"#,
    ),
    (
        "sweep",
        r#"usage: just tasks sweep [-h] [--as ACTOR] [--dry-run]
                        [--recent-hours RECENT_HOURS] [--task TASK]
"#,
        r#"usage: just tasks sweep [-h] [--as ACTOR] [--dry-run]
                        [--recent-hours RECENT_HOURS] [--task TASK]

Remove the worktrees nobody needs any more: merged into main, holding nothing
that did not land, untouched lately and with no live session in them. Work
finishing starts it by itself.

options:
  -h, --help            show this help message and exit
  --as ACTOR
  --dry-run             say what would go, remove nothing
  --recent-hours RECENT_HOURS
                        keep a worktree touched within this many hours
                        (default: $SF_SWEEP_RECENT_HOURS, else 2)
  --task TASK           a task just done or cancelled: its closed sessions'
                        worktrees go without the window (once per task)
"#,
    ),
    (
        "land",
        r#"usage: just tasks land [-h] [--as ACTOR] slug
"#,
        r#"usage: just tasks land [-h] [--as ACTOR] slug

Land the task's branch, named as its slug, on the default branch: the approve
of a fresh reviewer (or of its orchestrator, on a fast fix) must be its last
verdict and cover the branch's tip; the branch is merged (--no-ff) and
checked ([verdicts] check, after [land] generators) in a worktree of its own;
main moves there, is pushed to origin (unless [land] push = false), and the
task closes with its `landed` line. It runs only as `just land SLUG`, which
asks first, and never where the project's own `land` recipe replaces it.

positional arguments:
  slug

options:
  -h, --help  show this help message and exit
  --as ACTOR
"#,
    ),
];
