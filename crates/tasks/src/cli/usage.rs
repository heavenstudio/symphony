//! What `--help`, `help [COMMAND]` and a usage error print. The top help lists every command once,
//! with one line on what it does ([`GROUPS`]); a command's own help is its usage, what it does and
//! its arguments, laid out 78 columns wide (a pipe, where agents run it), in argparse's words, which
//! the first command line's parser printed. A flag or a command added adds itself here too.

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
    if name == super::database::COMMAND {
        return super::database::HELP.to_string();
    }
    if name == super::recovery::COMMAND {
        return super::recovery::HELP.to_string();
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
                "limits",
                "the limits and models in force here, and where each came from",
            ),
            (
                "settings",
                "the preferences the board keeps: list, set KEY VALUE, remove KEY",
            ),
            (
                "stamps",
                "the stamps a land can require, and what a task needs of them",
            ),
            (
                "runs",
                "what the recorder kept of a task at its tip, for its review",
            ),
            (
                "asks",
                "what the owner waits on an agent for, and who holds each",
            ),
            ("export", "write a markdown snapshot of the board"),
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
            (
                "move",
                "take the board's own edge to a status, --reason saying why",
            ),
            (
                "workflow",
                "a custom workflow: show it, a task's actions, apply, recover",
            ),
            (
                "say",
                "post on a task's thread, --to owner, @orchestrator or HANDLE",
            ),
            (
                "verdict",
                "as its reviewer: a review round's approve or changes edge",
            ),
            (
                "needs",
                "as its assessor: the stamps a task needs, at a tip, and why",
            ),
            (
                "gate",
                "as its orchestrator: raise a limit, or waive a stamp at one tip",
            ),
            (
                "done",
                "close a subtask, or a task covered by another's landing",
            ),
            (
                "land",
                "land an approved branch and push it; `just land SLUG` runs it",
            ),
            (
                "regenerate",
                "make [land] generators' outputs of main after a land; --status",
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
                "staff ready work once, if tools/task_staff.json enables it",
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
                "database",
                "hold the board's database stopped for maintenance, or release it",
            ),
            (
                "recovery",
                "the board's recovery binding; revoke it, or bind the board anew",
            ),
            (
                "schema",
                "the board's schema and the highest it was seen at; --accept",
            ),
            (
                "snapshot",
                "clone the board's store while no server runs on it; --list",
            ),
            (
                "run",
                "record a build or test, by the binary: `just tasks` refuses it",
            ),
            ("help", "this list, or a command's own help: help COMMAND"),
        ],
    ),
];

/// The commands read before the board's argument table ([`super::args::COMMANDS`]), each with its
/// own parser and help, which [`shown`] answers `help COMMAND` with: the tests hold the dispatch
/// and the help to them.
#[cfg(test)]
pub const BEFORE_TABLE: [&str; 4] = [
    super::recorder::COMMAND,
    super::migrate::COMMAND,
    super::database::COMMAND,
    super::recovery::COMMAND,
];

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
        "\noptions:\n  -h, --help       show this help message and exit\n  --version        say \
         the version and exit\n  --as ACTOR       act as \
         this session handle, before or after the command\n                   (default: \
         $TASKS_SESSION)\n  --database NAME  the scratch board NAME, not the project's; it comes \
         first,\n                   before every other word, and sweeps no repository\n\
         \nenvironment:\n  TASKS_SESSION    the session handle --as defaults to\n  TASKS_DATABASE   \
         the board's database, as --database NAME names it\n  TASKS_SERVER     the board's \
         server, HOST:PORT, in place of the one the\n                   repository's common dir \
         records\n",
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

The owner's chat goes to the session holding the seat (--chat). While nobody
holds it, it goes to a fallback: the one next in line (--chat --if-free), else
the live orchestrator with a callback first by handle, so a plain callback is
enough for an orchestrator to hear chat. The CHAT line names who hears it, and
says fallback and why when that session holds no seat.

positional arguments:
  address               uds:/absolute/socket or codex:THREAD-UUID (default:
                        detected); through just tasks, a codex: one only
                        this session's own (CODEX_THREAD_ID)

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
  --json                the task records, each with in_status_since: when it
                        took the status it holds
"#,
    ),
    (
        "search",
        r#"usage: just tasks search [-h] [--as ACTOR]
                         [--status {backlog,concept,design,ready,in_progress,review,done,blocked,cancelled}]
                         [--limit N] [--json] query
"#,
        r#"usage: just tasks search [-h] [--as ACTOR]
                         [--status {backlog,concept,design,ready,in_progress,review,done,blocked,cancelled}]
                         [--limit N] [--json] query

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
counts each top-level task done now once, on the day of its last close in
this machine's time zone, whenever it was made: one approved again after its
landing sent it back counts once, one cancelled or reopened since counts
none. The Reports page's Tasks done counts the same tasks, by UTC day.
Where tasks of the window were filed on an arm (`create --arm`), each arm's
tasks, how many are done, their median lead and review time, and how often
review sent the arm's work back.

options:
  -h, --help  show this help message and exit
  --as ACTOR
  --days N    how many days back, 1 to 3650 (default: 14)
  --json      the same figures, every duration in seconds
"#,
    ),
    (
        "next",
        r#"usage: just tasks next [-h] [--as ACTOR]
"#,
        r#"usage: just tasks next [-h] [--as ACTOR]

The task to take next: your reservation, else the most urgent ready one you
may take. Past the project's [limits] machine_agents it warns on stderr, and
still names the task: you run already.

options:
  -h, --help  show this help message and exit
  --as ACTOR
"#,
    ),
    (
        "workflow",
        r#"usage: just tasks workflow [-h] [--as ACTOR] [--revision N] [--key KEY] [--input NAME=TEXT] [--accept SLUG=OUTPUT] [--json] ACTION [SLUG] [EDGE]
"#,
        r#"usage: just tasks workflow [-h] [--as ACTOR] [--revision N] [--key KEY] [--input NAME=TEXT] [--accept SLUG=OUTPUT] [--json] ACTION [SLUG] [EDGE]

A custom workflow the board activated from the project's default branch
(docs/workflow/WORKFLOW.md). `show` prints its revision, positions and edges, or says
none is active. `actions SLUG` prints where the task stands, its workflow
revision, and each edge that leaves it with what stops you taking it now: a
preview, never a permit, and the accepted outputs it gives and takes. `apply
SLUG EDGE --revision N` takes the edge as you, through the board's one
admission, from the revision you read: a task moved since is refused as
stale_task, so read its actions again. An edge depending on accepted outputs
takes the one you name of each task it depends on (--accept): one superseded
since is refused as stale_input. `recover
SLUG HOLDER` asks the board's recovery for a holder whose session is lost or
left: its position's lost-holder action lets its roles go, or keeps them for
a person, and stays or moves to the pair it names. A task a custom workflow
holds moves and is staffed by its edges alone: move, claim, role, reserve,
handover and the verdicts refuse it as explicit_action_required, and release
and leave let go by its position's recovery.

positional arguments:
  ACTION                show, actions, apply or recover
  SLUG                  the task (actions, apply, recover)
  EDGE                  the edge to take (apply), or the lost holder (recover)

options:
  -h, --help            show this help message and exit
  --as ACTOR
  --revision N          the task's workflow revision you read (apply)
  --key KEY             the request's key: the same key and request answer the
                        first result, another request under it is refused
                        (default SLUG@REVISION:EDGE)
  --input NAME=TEXT     an input the edge declares, again for each
  --accept SLUG=OUTPUT  the accepted output of a task it depends on that the
                        edge takes (apply), again for each
  --json                the answer as JSON
"#,
    ),
    (
        "limits",
        r#"usage: just tasks limits [-h] [--as ACTOR]
"#,
        r#"usage: just tasks limits [-h] [--as ACTOR]

The limits a command here is held to and the model each role runs on, read
from the main checkout's symphony.toml as it is now, each with where it came
from: its table, key and line, or the default. [limits] holds
orchestrator_slots (the tasks in progress an orchestrator holds, work in review
and red work not counted; no cap unwritten; [board] orchestrator_slots is read
where [limits] names none), low_volume (4) and machine_agents (no limit unwritten,
then this machine's count of agents); [board] holds the pipeline alarm's
thresholds; [agents.models] names the model of the orchestrator, executor,
reviewer and assessor (the smallest its agent offers unwritten: Claude's
haiku). A key of [limits] this build does not know is listed, left out. An
edit to the file holds from the next command, and on a running server from
its next read. symphony.local.toml beside it, which git does not version,
is read after it: a key of [agents] launch, [agents.models] or [limits] it
writes holds in the committed one's place, named with that file and its line.
The board's store (just tasks settings) is read between the two: a setting it
holds is named as the store's, with the committed value it stands over.

options:
  -h, --help  show this help message and exit
  --as ACTOR
"#,
    ),
    (
        "settings",
        r#"usage: just tasks settings [-h] [--as ACTOR] [--json] ACTION [KEY] [VALUE]
"#,
        r#"usage: just tasks settings [-h] [--as ACTOR] [--json] ACTION [KEY] [VALUE]

The preferences the board keeps for its project, read after the committed
symphony.toml and before symphony.local.toml. `list` prints each entry with
every change made to it: the value before and after, when, and who wrote it.
`set KEY VALUE` stores a value, in force from the next read of the settings.
`remove KEY` removes the entry, so the files' value holds again; its record
stays. A key is the setting's table and key as the project file names them,
joined by dots: limits.low_volume, agents.launch.default, agents.launch.model,
agents.launch.effort, agents.models.reviewer, agents.panes. The store holds
those preferences alone: a setting that holds a command or decides what lands
is refused by name, as symphony.local.toml refuses it, and a value is judged
as the project file's is (a limit a whole number in its range, a launch agent
one the board starts). A refusal names the key and why, and writes nothing.
just tasks limits names the store for each value it gives.

positional arguments:
  ACTION      list, set or remove
  KEY         the setting (set, remove)
  VALUE       the value to store (set)

options:
  -h, --help  show this help message and exit
  --as ACTOR  who the change is recorded as made by (set, remove)
  --json      the entries as JSON (list)
"#,
    ),
    (
        "create",
        r#"usage: just tasks create [-h] [--as ACTOR] [--body BODY]
                         [--body-file BODY_FILE] [--priority {0,1,2,3,4}]
                         [--stage {backlog,concept,design,ready}] [--backlog]
                         [--parent PARENT] [--for ASSIGNEE] [--instruction]
                         [--depends DEPENDS] [--link LINK] [--no-role]
                         [--arm ARM]
                         slug title
"#,
        r#"usage: just tasks create [-h] [--as ACTOR] [--body BODY]
                         [--body-file BODY_FILE] [--priority {0,1,2,3,4}]
                         [--stage {backlog,concept,design,ready}] [--backlog]
                         [--parent PARENT] [--for ASSIGNEE] [--instruction]
                         [--depends DEPENDS] [--link LINK] [--no-role]
                         [--arm ARM]
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
                        is offered it); with --no-role, the live orchestrator
                        that takes it
  --instruction         a subtask its executor closes with `done`; its slug is
                        never a branch that lands
  --depends DEPENDS     a task that must be done first (repeatable)
  --link LINK           a document it implements, docs/X.md#Heading
                        (repeatable)
  --no-role             file it for the orchestrator --for names, not for the
                        filer: else an orchestrator filing it takes the
                        role, and the board gives any other filer's task an
                        orchestrator, unless one holds the task it is filed
                        under
  --arm ARM             the side of a process experiment it is filed on, one
                        lowercase word such as control: `show` prints it and
                        `stats` gives each arm's figures; nothing changes it
                        after
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
        "stamps",
        "usage: just tasks stamps [-h] [--as ACTOR] [slug]\n",
        r#"usage: just tasks stamps [-h] [--as ACTOR] [slug]

The stamps a land can require, from the main checkout's committed
symphony.toml ([stamps], else the Symphony's own four), each with whether a
task no assessment has read needs it, how the land knows it stands, where it
applies and where it does not. With a slug, then the task's needs as they
stand, its gate decisions, its branch's tip and the paths [land] check_paths
names that the branch changes: what an assessor reads first.

positional arguments:
  slug

options:
  -h, --help  show this help message and exit
  --as ACTOR
"#,
    ),
    (
        "runs",
        "usage: just tasks runs [-h] [--as ACTOR] [--tip TIP] slug\n",
        r#"usage: just tasks runs [-h] [--as ACTOR] [--tip TIP] slug

What the recorder kept of a task at its branch's tip in the main checkout, or
at the commit --tip names there, which its reviewer reads in place of building
again: each build and test command the task and the work under it ran there,
with its newest run's result, when, how long and who, and how many runs of it
came before there; the tests stamp as its reader reads it; the project's
[verdicts] check with its newest run there, or none; and how many of the
task's runs are at other commits, none of them the tip's.

positional arguments:
  slug

options:
  -h, --help  show this help message and exit
  --as ACTOR
  --tip TIP   the commit to read at, resolved in the main checkout; left out,
              the task's branch's tip
"#,
    ),
    (
        "needs",
        r#"usage: just tasks needs [-h] [--as ACTOR] [--tip TIP] [--import IMPORT]
                        --why WHY
                        slug stamps [stamps ...]
"#,
        r#"usage: just tasks needs [-h] [--as ACTOR] [--tip TIP] [--import IMPORT]
                        --why WHY
                        slug stamps [stamps ...]

As the task's assessor, write the stamps it needs, from the catalogue `just
tasks stamps` lists, with the tip read and why: the board's own field, and a
line on its thread saying them and what it needed before. Refused to a session
that executed the task or work under it, orchestrates or orchestrated it, or
filed it, and on an empty reason or a stamp the catalogue lacks.

A head that merges a commit of the project's upstream ([land.upstream]) is an
import: with --import, the assessment names that commit, resolved to the whole
commit, beside the head --tip names, and a land delivered by a pull request
merges that head by a merge commit, keeping the upstream's history. An import
named at no tip is refused; one named at another tip than the head delivers
nothing, and a head of upstream commits whose assessment names none is never
squashed.

positional arguments:
  slug
  stamps      the stamps it needs, by name; none alone for none

options:
  -h, --help  show this help message and exit
  --as ACTOR
  --tip TIP   the commit the assessment read, resolved to the whole commit;
              left out, the assessment read the brief alone
  --import IMPORT
              the upstream commit the head imports, resolved to the whole
              commit; only with --tip
  --why WHY   one line for each stamp it left out or added
"#,
    ),
    (
        "gate",
        r#"usage: just tasks gate [-h] [--as ACTOR] [--raise RAISE | --waive]
                       [--tip TIP] --why WHY
                       slug gate
"#,
        r#"usage: just tasks gate [-h] [--as ACTOR] [--raise RAISE | --waive]
                       [--tip TIP] --why WHY
                       slug gate

As the task's orchestrator, give one of its quality gates way by a decision the
board records: the board's own field, and a line on its thread its assessor,
its reviewer and its land read. Refused to a session that executed the task or
work under it, reviews it or gave it a verdict, or does not orchestrate it; the
owner's word in chat is recorded by the orchestrator, its reason naming it.

A raise names a limit as PATH:NAME, the one line of PATH that sets NAME to a
number (tools/size-budget.toml:symphony), in a file [land] check_paths names,
so the change keeps a fresh reviewer. Its number now is read from the default
branch; RAISE is the number it goes to, which the task commits in that file on
its branch, its message the account, and the land reads it there.

A waiver names a stamp the catalogue marks waivable (`just tasks stamps`), for
the tip --tip names, else the branch's tip: the land does not require that
stamp at that commit alone. tests, review and owner are never waived.

positional arguments:
  slug
  gate         PATH:NAME for a raise, a stamp for a waiver

options:
  -h, --help   show this help message and exit
  --as ACTOR
  --raise RAISE
               the number the limit goes to, above the default branch's
  --waive      waive the stamp at one tip
  --tip TIP    the commit a waiver is for, resolved to the whole commit;
               left out, the task's branch's tip
  --why WHY    what was measured, and why the gate gives way
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
        r#"usage: just tasks done [-h] [--as ACTOR] [--covered-by OTHER] slug note
"#,
        r#"usage: just tasks done [-h] [--as ACTOR] [--covered-by OTHER] slug note

Close a subtask, by its executor, with what came of it. With --covered-by, close
a task whose work landed with another task's branch, by its executor or
orchestrator, with no reviewer's verdict: OTHER is done and landed, its merge
on the default branch. It refuses while the task has work the default branch
lacks where the board records its executors working: commits on the branch
named as its slug, on a launch's branch or on an in-process executor's (as
tasks agents shows it), at the HEAD of such a worktree, detached too, or
changes there no commit has. It does not read the main checkout's changes, an
executor's earlier checkouts, where one whose session left worked, stashes or
other branches: check those. The thread names OTHER, the commit and who closed
it; the close is no approve, so no landing goes by it.

positional arguments:
  slug
  note                what came of it

options:
  -h, --help          show this help message and exit
  --as ACTOR
  --covered-by OTHER  close it as covered by OTHER's landing
"#,
    ),
    (
        "role",
        r#"usage: just tasks role [-h] [--as ACTOR] [--beside WHY]
                       slug {orchestrator,reviewer}
"#,
        r#"usage: just tasks role [-h] [--as ACTOR] [--beside WHY]
                       slug {orchestrator,reviewer}

positional arguments:
  slug
  {orchestrator,reviewer}

options:
  -h, --help            show this help message and exit
  --as ACTOR
  --beside WHY          a reviewer's: why it joins beside one still there
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
        r#"usage: just tasks move [-h] [--as ACTOR] [--reason REASON] [--on THING]
                       [--why WHY]
                       slug
                       {backlog,concept,design,ready,in_progress,review,done,blocked,cancelled}
"#,
        r#"usage: just tasks move [-h] [--as ACTOR] [--reason REASON] [--on THING]
                       [--why WHY]
                       slug
                       {backlog,concept,design,ready,in_progress,review,done,blocked,cancelled}

Take one of the board's own workflow's edges: the one from where the task
stands to the status named, as your role on it allows. A task a custom workflow
holds moves by its edges alone: a move refuses it as explicit_action_required,
and `just tasks workflow actions SLUG` names the edges you can take.

positional arguments:
  slug
  {backlog,concept,design,ready,in_progress,review,done,blocked,cancelled}

options:
  -h, --help            show this help message and exit
  --as ACTOR
  --reason REASON       why it moves (a cancel's, say), posted on its thread
                        after the move's line, in the same write
  --on THING            a move to blocked's: what it waits on, decision:NAME
                        (a card of the owner's), check:NAME, task:SLUG or
                        parked; refused without it. A task blocked already
                        takes it in place of what it named
  --why WHY             why, in a few words (at most 120 characters), kept
                        beside what it waits on
"#,
    ),
    (
        "verdict",
        r#"usage: just tasks verdict [-h] [--as ACTOR] slug {approve,changes} [text]
"#,
        r#"usage: just tasks verdict [-h] [--as ACTOR] slug {approve,changes} [text]

Record a round of a task's review as its reviewer, by one of the board's own
review edges: approve, which completes the task and is the stamp its landing
goes by, the text naming the whole tip read and why, or changes, which returns
it to its executor's work, the text naming what and where. A task a custom
workflow holds takes its review edges with `just tasks workflow apply`: a
verdict refuses it as explicit_action_required.

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
recorded it in symphony/tasks/health.json: its callback dispatch passes and idle
sweeps, ok and failed, the slowest board request and the line that sent it,
and write latency; read without asking the board

options:
  -h, --help  show this help message and exit
  --as ACTOR
  --json      the record as /api/health answers it, the file and the warnings
"#,
    ),
    (
        "schema",
        r#"usage: just tasks schema [-h] [--as ACTOR] [--accept VERSION]
"#,
        r#"usage: just tasks schema [-h] [--as ACTOR] [--accept VERSION]

the board's schema, and the highest it was seen at, which
symphony/tasks/schema-seen.log in the git common dir records beside the store,
outside the database. A live board that reads below it went back: its store
lost what was written since, and every command that opens it refuses it,
`tasks migrate` included (exit 1 here). The owner, and only the owner,
accepts a board older on purpose (a fresh store, a restore they chose) with
--accept, naming the schema it holds: docs/operations/BOARD_OPERATIONS.md, A board that
went back, says how to recover one first.

options:
  -h, --help         show this help message and exit
  --as ACTOR
  --accept VERSION   record VERSION (none: no board) as the board's, refused
                     unless the board holds exactly that
"#,
    ),
    (
        "snapshot",
        r#"usage: just tasks snapshot [-h] [--as ACTOR] [--list]
"#,
        r#"usage: just tasks snapshot [-h] [--as ACTOR] [--list]

take a snapshot of the board's store now: a copy-on-write clone of
symphony/tasks/data in symphony/tasks/snapshots/, named by its UTC time. Taken
only while no process serves the store, under the lock every start of its
server takes and holding the store's own LOCK, never of files a server is
writing: refused while any server has the store open. The launcher takes one
before each start of the server too, outside a scratch place, and on every
board before a replay of its log, which never runs without it, and
tasks-reclaim one of the old store before its own servers start on it. A
volume that cannot clone takes none, but before a replay or a reclaim a plain
copy. One whose write-ahead log holds writes not yet in its tables (taken
after an unclean end, before a replay, or of a store that went back) is kept
until the owner removes it (rm -rf its folder); of the others the newest 3 are
kept. Each grows towards the whole store's size as the store moves on, and the
room a reclaim frees comes back only once the snapshots of the old store are
removed: docs/operations/BOARD_OPERATIONS.md, The store and its server.

options:
  -h, --help  show this help message and exit
  --as ACTOR
  --list      the snapshots kept, the newest first, and where each is
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
                        (default: $SYMPHONY_SWEEP_RECENT_HOURS, else 2)
  --task TASK           a task just done or cancelled: its closed sessions'
                        worktrees go without the window (once per task)
"#,
    ),
    (
        "land",
        r#"usage: just tasks land [-h] [--as ACTOR] [--dispatch] [--watch] [slug]
"#,
        r#"usage: just tasks land [-h] [--as ACTOR] [--dispatch] [--watch] [slug]

Land the task's branch, named as its slug, on the default branch once every
stamp it needs stands on the branch's tip: its assessment's on the tip it read,
else the catalogue's defaults beside them (`just tasks stamps SLUG` says which).
Where review is needed, the approve of a fresh reviewer must be its last
verdict and cover the tip; where it is not, the task lands from review or blocked. A missing stamp
refuses, naming who gives it. The branch is merged (--no-ff) in a
worktree of its own, main's side of each generated path kept, and checked by
[verdicts] check unless [land] check = false; main moves there, is pushed to
origin (unless [land] push = false), the task closes with its `landed` line,
and the regenerator starts. It runs only as `just land SLUG`, whose recipe
sets the marker it needs, and never where the project's own `land` recipe
replaces it. Under [land] delivery the host merges by a pull request instead:
the task's move to review dispatches it, and the watcher closes it once merged.

positional arguments:
  slug

options:
  -h, --help  show this help message and exit
  --as ACTOR
  --dispatch  publish the task's head and its evidence to its pull request and
              arm the host's merge of it, whatever the evidence says; a move to
              review starts it detached under [land] delivery
  --watch     read every task armed at the host each [land] watch_seconds,
              closing what merged, until none is armed; a dispatch starts it
"#,
    ),
    (
        "regenerate",
        r#"usage: just tasks regenerate [-h] [--as ACTOR] [--status] [--wait]
"#,
        r#"usage: just tasks regenerate [-h] [--as ACTOR] [--status] [--wait]

Make what [land] generators make of main's newest tip, in a worktree of its
own, and land it as one commit; one regenerator at a time, which makes main's
newest tip again once it ends while main moved. A land starts it, detached,
unless one runs; a red round files tests/main-red-regenerate. It runs only as
a land starts it.

options:
  -h, --help  show this help message and exit
  --as ACTOR
  --status    say whether the generated outputs are current on main, and
              exit 1 when they are not; run nothing
  --wait      as --status, after waiting while the regenerator makes them: a
              pin to a commit of main moves once it exits 0
"#,
    ),
];
