//! What `--help` and a usage error print: argparse's own text for the first command line's parser,
//! as it lays it out 78 columns wide (a pipe, where agents run it), taken from it once when the
//! command line moved to Rust. A flag added to a command adds itself here too.

mod launch;
mod panes;

use super::Env;

/// A command's help `text` as `--help` prints it in `env`: launch's then says what a bare launch
/// starts there ([`launch::shown`]).
pub fn shown(text: &str, env: &Env) -> String {
    if text == launch::HELP {
        launch::shown(env)
    } else {
        text.to_string()
    }
}

/// The usage line of `just tasks` itself.
pub const USAGE: &str = r#"usage: just tasks [-h] [--as ACTOR]
                  {callback,register,attach,list,search,stats,embed,next,create,priority,field,claim,reserve,release,show,parent,depends,done,role,handover,move,verdict,say,launch,restore,cleanup,clean,close,reap,staff,agents,who,status,inbox,pull,wait,leave,beat,export,finish,sweep} ...
"#;

/// `just tasks --help`.
pub const HELP: &str = r#"usage: just tasks [-h] [--as ACTOR]
                  {callback,register,attach,list,search,stats,embed,next,create,priority,field,claim,reserve,release,show,parent,depends,done,role,handover,move,verdict,say,launch,restore,cleanup,clean,close,reap,staff,agents,who,status,inbox,pull,wait,leave,beat,export,finish,sweep} ...

The task board from the terminal — `just tasks …`.

positional arguments:
  {callback,register,attach,list,search,stats,embed,next,create,priority,field,claim,reserve,release,show,parent,depends,done,role,handover,move,verdict,say,launch,restore,cleanup,clean,close,reap,staff,agents,who,status,inbox,pull,wait,leave,beat,export,finish,sweep}
    callback            register a native channel or --off for inbox fallback
    depends             change what a task waits on
    handover            hand a task's orchestrator role to another session
    clean               cleanup, then one idle sweep, at once
    close               close an agent's terminal, whatever it holds, and
                        record it with its restore line
    beat               a hook: read one Claude Code tool event on stdin and
                        beat for the subagent making it
    status              the board's health: failed dispatches and sweeps,
                        slow requests, write latency
    finish              close the panes of a finished task's sessions
    pull                print the whole PULL digest as ACTOR hears it
    wait                print what a callback would push ACTOR, once there is
                        any
    run                 run a build or test command, recorded for the board's
                        Build and Test columns (tasks run --phase build|test
                        -- COMMAND)
    migrate             move the board forward to this build's schema, or make
                        it; a live board whose main checkout holds no board
                        source moves only by this, run in that checkout

options:
  -h, --help            show this help message and exit
  --as ACTOR            act as this session handle (default: $TASKS_SESSION)
"#;

/// Each command's usage and `--help`, in the order the top usage names them.
pub const COMMANDS: &[(&str, &str, &str)] = &[
    (
        "callback",
        "usage: just tasks callback [ADDRESS] [--chat] [--if-free] [--off] [--session-url URL] [--as HANDLE]\n",
        "Register the session native callback: uds:/path or codex:UUID. With no address, detect the current Claude socket or Codex thread, refusing if neither exists. An address held by another registered handle is refused until it unregisters or leaves, including while idle. In-process subagents use inbox. --chat takes the chat recipient seat; --if-free only takes a vacant seat. --off unregisters and restores inbox fallback. --session-url URL gives the session the web address its Go to session control links to (https:// or http://; an empty one clears it); with it and no ADDRESS given or detected, only the session URL is set.\n",
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
                       [--mine] [--doubled] [--json]
"#,
        r#"usage: just tasks list [-h] [--as ACTOR]
                       [--status {backlog,concept,design,ready,in_progress,review,done,blocked,cancelled}]
                       [--mine] [--doubled] [--json]

options:
  -h, --help            show this help message and exit
  --as ACTOR
  --status {backlog,concept,design,ready,in_progress,review,done,blocked,cancelled}
  --mine
  --doubled             only tasks holding two or more orchestrators
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
                         [--body-file BODY_FILE] [--priority PRIORITY]
                         [--stage {backlog,concept,design,ready}] [--backlog]
                         [--parent PARENT] [--for ASSIGNEE] [--instruction]
                         [--depends DEPENDS] [--link LINK]
                         slug title
"#,
        r#"usage: just tasks create [-h] [--as ACTOR] [--body BODY]
                         [--body-file BODY_FILE] [--priority PRIORITY]
                         [--stage {backlog,concept,design,ready}] [--backlog]
                         [--parent PARENT] [--for ASSIGNEE] [--instruction]
                         [--depends DEPENDS] [--link LINK]
                         slug title

positional arguments:
  slug
  title

options:
  -h, --help            show this help message and exit
  --as ACTOR
  --body BODY
  --body-file BODY_FILE
  --priority PRIORITY
  --stage {backlog,concept,design,ready}
                        where it enters the pipeline
  --backlog             same as --stage backlog
  --parent PARENT
  --for ASSIGNEE        reserve it for this handle, with no lapse (nobody else
                        is offered it)
  --instruction         a subtask its executor closes with `done`; its slug is
                        never a branch that lands
  --depends DEPENDS
  --link LINK
"#,
    ),
    (
        "priority",
        "usage: just tasks priority [-h] [--as ACTOR] slug {0,1,2,3,4}\n",
        r#"usage: just tasks priority [-h] [--as ACTOR] slug {0,1,2,3,4}

Set an existing task's priority (0 is most urgent)

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

positional arguments:
  slug
  text

options:
  -h, --help  show this help message and exit
  --as ACTOR
  --to TO
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
                        [--background BACKGROUND]
"#,
        r#"usage: just tasks sweep [-h] [--as ACTOR] [--dry-run]
                        [--recent-hours RECENT_HOURS] [--task TASK]
                        [--background BACKGROUND]

options:
  -h, --help            show this help message and exit
  --as ACTOR
  --dry-run             say what would go, remove nothing
  --recent-hours RECENT_HOURS
                        keep a worktree touched within this many hours
                        (default: $SYMPHONY_SWEEP_RECENT_HOURS, else 2)
  --task TASK           a task just done or cancelled: its closed sessions'
                        worktrees go without the window (once per task)
  --background BACKGROUND
                        why work finishing started it: said first, and the
                        board's server is never started for it
"#,
    ),
];
