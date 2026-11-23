//! The `symphony` binary (`just symphony-cli`): `status` (`just symphony-status`), `reply`
//! (`just symphony-reply`), `triage`, `ask` (`just symphony-ask`), `verdict-asks`, the commands
//! the project's plugins add, and `serve`, one line-oriented JSON entry to the register and the asks,
//! one JSON line per call.

mod commands;
mod model;
mod publish;
mod tool_commands;

/// A built client published over the one served, keeping the builds published before it: what
/// `symphony-build publish-client` (crates/symphony-build) runs.
pub use publish::publish_keeping;
pub(crate) use publish::{BUILDS, publish_client};

use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};
use symphony_board::lock::Recovered as _;

use crate::asks::{self, AskError, AskStore};
use crate::board::{Doc, render, scan, tidy, to_json};
use crate::callouts::{Comment, parse_comments};
use crate::clock::now_stamp;
use crate::edit::{self, EditError};
use crate::lint;
use crate::plugins::Plugins;
use crate::register::{self, Decision, QUESTION, ReviewError, Who, card_end, parse_decisions};
use crate::text::{os_error, read_text, write_whole};
use crate::triage;

const STATUS_USAGE: &str =
    "usage: symphony status [-h] [--root ROOT] [--json] [--open] [--tidy] [--lint] [--now NOW]";
const STATUS_HELP: &str = "The concept review board — `just symphony-status`.

options:
  -h, --help   show this help message and exit
  --root ROOT  project root holding docs/
  --json       print the board as JSON
  --open       print only the open comments
  --tidy       mark plain approvals resolved and regroup every register under docs/, owner-first
  --lint       fail when a document's prose names a card slug
  --now NOW    timestamp for --tidy (default: local time, YYYY-MM-DD HH:MM)";
const REPLY_USAGE: &str =
    "usage: symphony reply [-h] [--question] [--now NOW] [--as HANDLE] path slug text";
const REPLY_HELP: &str = "An agent's line under a card, appended the way the app appends the owner's, or the
answer that resolves a comment. In a checkout on the default branch of a project that delivers
by pull request it writes no file: it is kept as a save, shown on the page at once and delivered
by a pull request.

positional arguments:
  path          the document, by its path from here
  slug          the card's slug, or L and the line a comment's callout opens on (L12)
  text          the reply; a comment's answer is one line (`> > [!resolved] text`)

options:
  -h, --help    show this help message and exit
  --question    the line asks the owner something: the card stays open for the owner (`agent · question`)
  --now NOW     timestamp (default: local time)
  --as HANDLE   the session replying (default: TASKS_SESSION), credited on the owner's ask it answers";

/// `symphony COMMAND ARGS…`: the whole command line without the program name. Its exit code. A
/// command the Symphony does not have is run by the first plugin this process holds that adds it.
/// A host binary that calls this runs `symphony_chrome::leash_main()` first, as `bin/symphony.rs`
/// does: `shot` leashes its Chrome by running the program again, and without it a shot fails
/// before Chrome is ready, its leash exited.
pub fn main(
    args: &[String],
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    main_in(args, &process_env(), input, out, err)
}

/// [`main`], reading what `status`, `reply` and `ask` take from their environment out of `env`
/// in place of this process's: the marker a recipe an agent runs unasked sets
/// ([`symphony_board::confine::MARKER`]), which confines where each writes, the session `reply`
/// credits (`TASKS_SESSION`) and the ask store `ASKS_DB` names. A caller whose own environment is
/// not the command's, a test run under a recipe's or a land's, says each of them it depends on.
pub fn main_in(
    args: &[String],
    env: &BTreeMap<String, String>,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    if version(args, "symphony", env!("CARGO_PKG_VERSION"), out) {
        return 0;
    }
    let command = args.first().map(String::as_str);
    let symphony = &commands::SYMPHONY;
    if let Some(help) = command.and_then(|name| commands::plain_help(symphony, name, &args[1..])) {
        let _ = write!(out, "{help}");
        return 0;
    }
    match command {
        Some("--help" | "-h" | "help") => commands::help(symphony, &args[1..], out, err),
        Some("status") => status_in(&args[1..], env, out, err),
        Some("reply") => reply_in(&args[1..], env, out, err),
        Some("serve") => serve_main(input, out),
        Some("docs-audit") => crate::docs_audit::main(&args[1..], out, err),
        Some("settings-check") => lint::settings(&args[1..], out, err),
        Some("model") => model::main(&args[1..], env, out, err),
        Some("blocks") => crate::blocks::main(&args[1..], out, err),
        Some("triage") => triage_main(&args[1..], out, err),
        Some(name @ ("render-card" | "landing-checks")) => {
            tool_commands::main(name, &args[1..], out, err)
        }
        Some("verdict-asks") => verdict_asks_main(&args[1..], out, err),
        Some("ask") => asks::main(&args[1..], env, out, err),
        Some("project") => crate::machine::command::main(&args[1..], out, err),
        Some("hook") => crate::hook::main(&args[1..], input, err),
        Some(other) => commands::plugin(other, &args[1..], out, err),
        None => commands::usage(symphony, err),
    }
}

/// The command line's reading, a refusal of it as argparse words one, and the answer to
/// `--version`: the helper `symphony-args`, which a plugin's commands and the server use too.
pub use symphony_args::{Flag, Parsed, command_at, parse, refuse, version};

/// This process's environment, each variable whose name and value are text: what [`main`] reads.
fn process_env() -> BTreeMap<String, String> {
    symphony_board::cli::Env::of_process().vars
}

/// `symphony status [--root ROOT] [--json] [--open] [--tidy] [--lint] [--now STAMP]`.
pub fn status_main(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    status_in(args, &process_env(), out, err)
}

/// [`status_main`] in the environment `env` ([`main_in`]).
fn status_in(
    args: &[String],
    env: &BTreeMap<String, String>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let flags = [
        Flag {
            name: "--root",
            value: true,
        },
        Flag {
            name: "--json",
            value: false,
        },
        Flag {
            name: "--open",
            value: false,
        },
        Flag {
            name: "--tidy",
            value: false,
        },
        Flag {
            name: "--lint",
            value: false,
        },
        Flag {
            name: "--now",
            value: true,
        },
    ];
    let parsed = match parse(args, &flags) {
        Ok(parsed) if parsed.help => {
            let _ = writeln!(out, "{STATUS_USAGE}\n\n{STATUS_HELP}");
            return 0;
        }
        Ok(parsed) if !parsed.positionals.is_empty() => {
            return refuse(
                err,
                STATUS_USAGE,
                "status",
                &format!("unrecognized arguments: {}", parsed.positionals.join(" ")),
            );
        }
        Ok(parsed) => parsed,
        Err(problem) => return refuse(err, STATUS_USAGE, "status", &problem),
    };
    let root = PathBuf::from(parsed.value("--root").unwrap_or("."));
    if parsed.flag("--tidy") {
        // It rewrites the registers under ROOT: through `just symphony-status`, the checkout's.
        if let Err(problem) = unasked_inside(&root, env) {
            let _ = writeln!(err, "✖ {problem}");
            return 1;
        }
        return tidy(&root, parsed.value("--now"), out, err);
    }
    if parsed.flag("--lint") {
        return lint::main(&root, out, err);
    }
    // As the page shows it: a save still waiting for delivery by pull request is in it.
    let docs = match crate::board::scan_shown(&root) {
        Ok(docs) => docs,
        Err(problem) => {
            let _ = writeln!(err, "✖ concept review board could not run: {problem}");
            return 2;
        }
    };
    let plugins = crate::plugins::Plugins::of(&root).reading(&root, &docs);
    let said = if parsed.flag("--json") {
        to_json(&plugins, &docs)
    } else {
        render(&plugins, &docs, parsed.flag("--open"))
    };
    let _ = writeln!(out, "{said}");
    0
}

/// `symphony reply PATH SLUG TEXT [--question] [--now STAMP] [--as HANDLE]`: an agent's line under
/// a card, or with the slug `L<line>` the answer that resolves the comment whose callout opens on
/// that line; and, in a checkout, who wrote it, in the ask store ([`crate::asks::Reply`]), for the
/// owner's ask it answers. In a checkout on the default branch of a project that delivers by pull
/// request it writes no file: the line is written on the page as the saves still waiting leave
/// it, and kept as a save of its own ([`crate::commit::pending::keep`]), which the page shows at
/// once and a pull request delivers.
pub fn reply_main(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    reply_in(args, &process_env(), out, err)
}

/// [`reply_main`] in the environment `env` ([`main_in`]).
fn reply_in(
    args: &[String],
    env: &BTreeMap<String, String>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let flags = [
        Flag {
            name: "--question",
            value: false,
        },
        Flag {
            name: "--now",
            value: true,
        },
        Flag {
            name: "--as",
            value: true,
        },
    ];
    let parsed = match parse(args, &flags) {
        Ok(parsed) if parsed.help => {
            let _ = writeln!(out, "{REPLY_USAGE}\n\n{REPLY_HELP}");
            return 0;
        }
        Ok(parsed) => parsed,
        Err(problem) => return refuse(err, REPLY_USAGE, "reply", &problem),
    };
    let [path, slug, text] = parsed.positionals.as_slice() else {
        let problem = if parsed.positionals.len() > 3 {
            format!(
                "unrecognized arguments: {}",
                parsed.positionals[3..].join(" ")
            )
        } else {
            let missing = ["path", "slug", "text"][parsed.positionals.len()..].join(", ");
            format!("the following arguments are required: {missing}")
        };
        return refuse(err, REPLY_USAGE, "reply", &problem);
    };
    let now = parsed
        .value("--now")
        .filter(|now| !now.is_empty())
        .map_or_else(now_stamp, str::to_string);
    let verdict = parsed.flag("--question").then_some(QUESTION);
    let comment = comment_line(slug);
    if comment.is_some() && verdict.is_some() {
        let _ = writeln!(
            err,
            "✖ --question is for a card: an answer on a comment resolves it, so ask the owner on a card"
        );
        return 1;
    }
    let file = Path::new(path);
    if let Err(problem) = unasked_inside(file, env) {
        let _ = writeln!(err, "✖ {problem}");
        return 1;
    }
    // In a checkout on the default branch of a project that delivers by pull request the reply
    // writes no file: it is read as the page shows the saves still waiting, and kept as a save
    // of its own (`commit::pending::keep`), under the lock the server's saves take.
    let delivered = delivered_from(file);
    let _held = match &delivered {
        Some((top, _)) => match crate::commit::write_lock(top) {
            Ok(held) => held.map(crate::commit::Held),
            Err(problem) => {
                let _ = writeln!(err, "✖ {problem}");
                return 1;
            }
        },
        None => None,
    };
    let written = read_text(file)
        .map(|old| match &delivered {
            Some((top, rel)) => crate::commit::pending::shown(top, rel, old),
            None => old,
        })
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => format!(
                "no document {path}: reply names the document by its path from here, such as \
                 docs/DECISIONS.md"
            ),
            _ => os_error(&e, file),
        })
        .and_then(|old| {
            match comment {
                Some(line) => edit::resolve_comment(&old, line, text)
                    .map(|(new, title)| (new, "comment", title))
                    .map_err(|e| e.0),
                None => edit::add_message(&old, slug, text, &now, Who::Agent, verdict)
                    .map(|new| (new, "card", slug.to_string()))
                    .map_err(|e| e.0),
            }
            .map(|made| (old, made))
        });
    let (old, (new, kind, target)) = match written {
        Ok(written) => written,
        Err(problem) => {
            let _ = writeln!(err, "✖ {problem}");
            return 1;
        }
    };
    // Who answered is in the ask store before the answer is in the file, so the scan that reads
    // the answer finds who gave it.
    let by = parsed
        .value("--as")
        .map(str::to_string)
        .or_else(|| env.get("TASKS_SESSION").cloned())
        .filter(|by| !by.trim().is_empty());
    if let Some(by) = &by
        && let Err(problem) = record_reply(file, kind, &target, by.trim(), env)
    {
        let _ = writeln!(
            err,
            "warning: who replied is not recorded ({problem}): the owner's ask on it closes \
             credited to no one"
        );
    }
    if let Some((top, rel)) = delivered {
        let edit = crate::commit::Edit {
            rel,
            base: old,
            new,
        };
        return match crate::commit::pending::keep(&top, edit, by.as_deref().unwrap_or_default()) {
            Ok(number) => {
                let _ = writeln!(
                    out,
                    "replied on {slug} in {path}, kept as save {number}: this project delivers \
                     by pull request, so the page shows it now and a pull request delivers it"
                );
                0
            }
            Err(problem) => {
                let _ = writeln!(err, "✖ the reply is not kept: {problem}");
                1
            }
        };
    }
    if let Err(e) = write_whole(file, &new) {
        let _ = writeln!(err, "✖ {}", os_error(&e, file));
        return 1;
    }
    let _ = writeln!(out, "replied on {slug} in {path}");
    0
}

/// The checkout that holds `file` and the file's path from it, when that checkout is on the
/// default branch of a project that delivers by pull request (`[land] delivery`): there the host
/// alone writes that branch, so a reply is kept for delivery ([`crate::commit::pending::keep`])
/// and no file is written, as an owner's save writes none. None for a file in any other place.
fn delivered_from(file: &Path) -> Option<(PathBuf, String)> {
    let file = std::fs::canonicalize(file).ok()?;
    let top = crate::branches::git(
        file.parent()?,
        &["rev-parse", "--path-format=absolute", "--show-toplevel"],
    )?;
    let top = std::fs::canonicalize(top.trim()).ok()?;
    if !(crate::commit::on_main(&top) && crate::commit::delivers(&top)) {
        return None;
    }
    let rel = crate::board::relative(&file, &top);
    Some((top, rel))
}

/// Whether `path`, a place this process is told to write, may be written: anywhere for the binary
/// run by itself, and under the marker the recipes an agent runs unasked set
/// (`just symphony-reply`, `just symphony-ask`, `just symphony-status`), as `env` holds it, only in
/// the checkout this process runs in or, from a linked worktree, the main checkout's artifacts
/// folder, never a git folder ([`symphony_board::confine`]).
pub(crate) fn unasked_inside(path: &Path, env: &BTreeMap<String, String>) -> Result<(), String> {
    if env
        .get(symphony_board::confine::MARKER)
        .is_none_or(String::is_empty)
    {
        return Ok(());
    }
    let here = std::env::current_dir().map_err(|e| format!("no folder to run in: {e}"))?;
    symphony_board::confine::inside(&here, path).map(drop)
}

/// The line a comment slug names: `L` and the 1-based line its callout opens on.
fn comment_line(slug: &str) -> Option<usize> {
    slug.strip_prefix('L')
        .filter(|line| !line.is_empty() && line.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|line| line.parse().ok())
        .filter(|line| *line > 0)
}

/// Who answered on `target` (a card's slug, a comment's title; `kind` `card` or `comment`) of the
/// document `file`, in the ask store its checkout's repository shares, for the owner's ask on it
/// ([`crate::owner_asks`]), `ASKS_DB` in `env` when it names one. A document in no git checkout
/// is no project's, and no ask is on it.
fn record_reply(
    file: &Path,
    kind: &str,
    target: &str,
    by: &str,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    let Some((top, tree, path)) = crate::owner_asks::reply_place(file) else {
        return Ok(());
    };
    let store = AskStore::open(&asks::default_path(
        &top,
        env.get("ASKS_DB").map(String::as_str),
    ))
    .map_err(|e| e.to_string())?;
    store
        .record_reply(&asks::Reply {
            tree,
            path,
            kind: kind.into(),
            target: target.into(),
            by: by.into(),
            at: 0.0,
        })
        .map_err(|e| e.to_string())
}

/// Held while `serve` answers a request, so [`end_with_parent`] never ends the process mid-write.
static IN_HAND: Mutex<()> = Mutex::new(());

/// How often [`end_with_parent`] looks at the parent.
const PARENT_LOOK: Duration = Duration::from_millis(250);

/// A `symphony serve` ends with the process that started it, even while its input stays open (a child
/// that parent forked holds the pipe too, and its end of input never comes): a thread looks at the
/// parent every [`PARENT_LOOK`] and, once it is another (the first one gone and this process handed to
/// init or a subreaper), ends this process after the request in hand. A server whose parent is init
/// from the start (its starter already gone) has nothing to watch; its end of input ends it.
pub fn end_with_parent() {
    let parent = std::os::unix::process::parent_id();
    if parent == 1 {
        return;
    }
    std::thread::spawn(move || {
        while std::os::unix::process::parent_id() == parent {
            #[expect(
                clippy::disallowed_methods,
                reason = "no-timed-waits baseline: looks again whether its parent has ended, where a pipe from the parent closing is the event"
            )]
            std::thread::sleep(PARENT_LOOK);
        }
        let _in_hand = IN_HAND.lock().recover();
        std::process::exit(0);
    });
}

/// `symphony serve`: one JSON request per line in, one JSON answer per line out, until the input ends.
/// A request is `{"fn": NAME, …its arguments}`; an answer is `{"ok": VALUE}` or
/// `{"error": KIND, "message": TEXT}`, KIND `review`, `edit`, `ask`, `store` or `value`. A request's `cwd` is the
/// caller's directory at the time: this process moves there first, so a relative path reads, and is
/// named, as it would in the caller.
pub fn serve_main(input: &mut dyn BufRead, out: &mut dyn Write) -> i32 {
    let mut line = String::new();
    loop {
        line.clear();
        match input.read_line(&mut line) {
            Ok(0) => return 0,
            Ok(_) => {}
            Err(_) => return 2,
        }
        let _in_hand = IN_HAND.lock().recover();
        let answer = match serde_json::from_str::<Value>(&line) {
            Ok(request) => match request.get("cwd").and_then(Value::as_str) {
                Some(dir) if std::env::set_current_dir(dir).is_err() => {
                    json!({"error": "value", "message": format!("no directory {dir:?} to work from")})
                }
                _ => call(&request),
            },
            Err(problem) => {
                json!({"error": "value", "message": format!("not a JSON request: {problem}")})
            }
        };
        if writeln!(out, "{answer}")
            .and_then(|()| out.flush())
            .is_err()
        {
            return 0; // whoever asked is gone
        }
    }
}

/// Why a call failed: `review`, `edit` and `ask` are the library's refusals, `store` an ask store that
/// does not open or read, `value` a request that does not read.
struct Failure {
    kind: &'static str,
    message: String,
}

impl From<ReviewError> for Failure {
    fn from(problem: ReviewError) -> Failure {
        Failure {
            kind: "review",
            message: problem.0,
        }
    }
}

impl From<AskError> for Failure {
    fn from(problem: AskError) -> Failure {
        match problem {
            AskError::Refused(message) => Failure {
                kind: "ask",
                message,
            },
            AskError::Store(message) => Failure {
                kind: "store",
                message,
            },
        }
    }
}

impl From<EditError> for Failure {
    fn from(problem: EditError) -> Failure {
        Failure {
            kind: "edit",
            message: problem.0,
        }
    }
}

fn bad(message: String) -> Failure {
    Failure {
        kind: "value",
        message,
    }
}

/// What `serve` answers a `validate` request: an unknown function, as any name it lacks, and where a
/// schema check is.
const VALIDATE_REFUSED: &str = "unknown function \"validate\": serve has no schema check; link \
                                symphony_schema::validate (symphony::schema) instead";

/// A card as `serve` answers it: its fields and everything read off it.
#[derive(Serialize)]
struct CardData<'a> {
    line: usize,
    slug: &'a str,
    state: &'static str,
    text: &'a str,
    note: &'a [String],
    topic: &'a str,
    raw: &'a [String],
    entries: Vec<Value>,
    reply: Option<String>,
    posted_at: Option<String>,
    decided_at: Option<String>,
    unanswered: Vec<String>,
    open: bool,
    asks_owner: bool,
    resolvable: bool,
    group: &'static str,
}

fn card_data<'a>(d: &'a Decision, plugins: &Plugins) -> CardData<'a> {
    let thread = d.thread(plugins);
    CardData {
        line: d.line,
        slug: &d.slug,
        state: d.state.as_str(),
        text: &d.text,
        note: &d.note,
        topic: &d.topic,
        raw: &d.raw,
        entries: thread
            .entries
            .into_iter()
            .map(|e| json!({"ts": e.ts, "who": e.who.as_str(), "verdict": e.verdict, "text": e.text}))
            .collect(),
        reply: thread.reply,
        posted_at: thread.posted_at,
        decided_at: thread.decided_at,
        unanswered: thread.unanswered,
        open: thread.open,
        asks_owner: thread.asks_owner,
        resolvable: thread.resolvable,
        group: thread.group.key(),
    }
}

fn comment_data(c: &Comment) -> Value {
    json!({"line": c.line, "kind": c.kind, "title": c.title, "body": c.body, "resolution": c.resolution,
           "raw": c.raw, "open": c.open()})
}

fn doc_data(doc: &Doc, plugins: &Plugins) -> Value {
    json!({
        "path": doc.path,
        "comments": doc.comments.iter().map(comment_data).collect::<Vec<_>>(),
        "decisions": doc.decisions.iter().map(|d| card_data(d, plugins)).collect::<Vec<_>>(),
    })
}

fn text_arg<'a>(request: &'a Value, name: &str) -> Result<&'a str, Failure> {
    request
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| bad(format!("{name} must be a string")))
}

fn optional_text<'a>(request: &'a Value, name: &str) -> Result<Option<&'a str>, Failure> {
    match request.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text)),
        Some(_) => Err(bad(format!("{name} must be a string or null"))),
    }
}

fn lines_arg<'a>(request: &'a Value, name: &str) -> Result<Vec<&'a str>, Failure> {
    let list = request
        .get(name)
        .and_then(Value::as_array)
        .ok_or_else(|| bad(format!("{name} must be a list")))?;
    list.iter()
        .map(|line| {
            line.as_str()
                .ok_or_else(|| bad(format!("{name} must hold strings")))
        })
        .collect()
}

fn int_arg(request: &Value, name: &str) -> Result<i64, Failure> {
    request
        .get(name)
        .and_then(Value::as_i64)
        .ok_or_else(|| bad(format!("{name} must be an integer")))
}

/// One request's answer, as `serve` writes it.
pub fn call(request: &Value) -> Value {
    match dispatch(request) {
        Ok(value) => json!({"ok": value}),
        Err(failure) => json!({"error": failure.kind, "message": failure.message}),
    }
}

fn dispatch(request: &Value) -> Result<Value, Failure> {
    let name = text_arg(request, "fn")?;
    let text = || text_arg(request, "text");
    let now = || text_arg(request, "now");
    // Cards are read by the card kinds of the project at the request's `root`; none without one.
    let plugins = request
        .get("root")
        .and_then(Value::as_str)
        .map_or_else(Plugins::default, |root| Plugins::of(Path::new(root)));
    Ok(match name {
        "parse_decisions" => json!(
            parse_decisions(&lines_arg(request, "lines")?)
                .iter()
                .map(|d| card_data(d, &plugins))
                .collect::<Vec<_>>()
        ),
        "parse_comments" => json!(
            parse_comments(&lines_arg(request, "lines")?)
                .iter()
                .map(comment_data)
                .collect::<Vec<_>>()
        ),
        "card_end" => {
            let lines = lines_arg(request, "lines")?;
            let index = usize::try_from(int_arg(request, "index")?)
                .map_err(|_| bad("index must not be negative".into()))?;
            json!(card_end(&lines, index))
        }
        "sort_register" => json!(register::sort_register(text()?, &plugins)?),
        "resolve" => json!(register::resolve(text()?, now()?, &plugins)),
        "supersede" => json!(register::supersede(text()?, now()?)),
        "check_nothing_lost" => {
            register::check_nothing_lost(
                text_arg(request, "before")?,
                text_arg(request, "after")?,
                text_arg(request, "doing")?,
            )?;
            Value::Null
        }
        "scan" => json!(
            scan(Path::new(text_arg(request, "root")?))?
                .iter()
                .map(|doc| doc_data(doc, &plugins))
                .collect::<Vec<_>>()
        ),
        "dashboard" => {
            let snapshot =
                crate::dashboard::snapshot(Path::new(text_arg(request, "root")?), &[], "")?;
            json!({"board":snapshot.board, "queue":snapshot.queue, "pending":snapshot.pending, "activity":snapshot.activity})
        }
        "board_plain" => json!(crate::dashboard::plain(text()?)),
        "doc_versions" => json!(crate::versions::versions(
            Path::new(text_arg(request, "root")?),
            text_arg(request, "path")?,
            request
                .get("limit")
                .and_then(Value::as_u64)
                .and_then(|n| usize::try_from(n).ok())
                .unwrap_or(0)
        )),
        "version_subject" => json!(crate::versions::subject(
            Path::new(text_arg(request, "root")?),
            text_arg(request, "sha")?
        )),
        "version_diff" => json!(
            crate::versions::diff(
                Path::new(text_arg(request, "root")?),
                text_arg(request, "path")?,
                text_arg(request, "sha")?
            )
            .map(|diff| diff.diff)
        ),
        "board_words" => {
            let mut lines = vec![format!("- [ ] **board-card** — {}", text()?)];
            lines.extend(lines_arg(request, "lines")?.into_iter().map(str::to_string));
            let cards = parse_decisions(&lines);
            let card = cards
                .first()
                .ok_or_else(|| bad("board_words needs a card".into()))?;
            json!({"title":crate::dashboard::card_title(&plugins, card), "summary":crate::dashboard::card_summary(&plugins, card), "rest":crate::dashboard::card_rest(&plugins, card)})
        }
        "set_decision" => json!(edit::set_decision(
            text()?,
            text_arg(request, "slug")?,
            text_arg(request, "state")?,
            text_arg(request, "note")?,
            now()?,
            &plugins,
        )?),
        "is_repeat_decision" => {
            let (slug, state, note) = (
                text_arg(request, "slug")?,
                text_arg(request, "state")?,
                text_arg(request, "note")?,
            );
            json!(
                edit::is_repeat_decision(text()?, slug, state, note, now()?)
                    .map_err(|e| bad(e.0))?
            )
        }
        "add_message" => {
            let who = match optional_text(request, "who")?.unwrap_or("owner") {
                "owner" => Who::Owner,
                "agent" => Who::Agent,
                other => return Err(bad(format!("who must be owner or agent, not {other:?}"))),
            };
            let (slug, message) = (text_arg(request, "slug")?, text_arg(request, "message")?);
            json!(edit::add_message(
                text()?,
                slug,
                message,
                now()?,
                who,
                optional_text(request, "verdict")?
            )?)
        }
        "insert_comment" => json!(edit::insert_comment(
            text()?,
            int_arg(request, "after_line")?,
            text_arg(request, "kind")?,
            text_arg(request, "title")?,
            text_arg(request, "body")?
        )?),
        "split_message" => json!(edit::split_message(text_arg(request, "message")?)?),
        "section_text" => json!(edit::section_text(text()?, text_arg(request, "anchor")?)),
        "now_stamp" => json!(now_stamp()),
        // A schema check is a library call (`symphony::schema::validate`), never a request: the
        // validator would add a fifth to this binary (tests/links.rs). The refusal is any unknown
        // name's, saying where the check is.
        "validate" => return Err(bad(VALIDATE_REFUSED.into())),
        "expand_components" => json!(
            crate::render::Render::new(text_arg(request, "root")?)
                .expand_components(text()?, text_arg(request, "doc_path")?)
        ),
        "component" => json!(
            crate::render::Render::new(text_arg(request, "root")?).component(
                text_arg(request, "doc_path")?,
                text_arg(request, "name")?,
                text_arg(request, "arg")?,
                optional_text(request, "option")?
            )
        ),
        name if name.starts_with("asks_") => asks_call(name, request)?,
        other => return Err(bad(format!("unknown function {other:?}"))),
    })
}

/// The asks `serve` answers, each on the store at `db`.
fn asks_call(name: &str, request: &Value) -> Result<Value, Failure> {
    let store = || AskStore::open(Path::new(text_arg(request, "db")?)).map_err(Failure::from);
    let or_empty = |name: &str| optional_text(request, name).map(Option::unwrap_or_default);
    Ok(match name {
        "asks_ask" => json!(store()?.ask(
            text_arg(request, "path")?,
            text_arg(request, "question")?,
            or_empty("anchor")?,
            or_empty("quote")?
        )?),
        "asks_reply" => {
            store()?.reply(int_arg(request, "id")?, text_arg(request, "text")?)?;
            Value::Null
        }
        "asks_answer" => {
            let by = optional_text(request, "by")?.unwrap_or("agent");
            store()?.answer(int_arg(request, "id")?, text_arg(request, "text")?, by)?;
            Value::Null
        }
        "asks_thread" => json!(store()?.thread(int_arg(request, "id")?)?),
        "asks_all" => json!(store()?.all()?),
        "asks_for_doc" => json!(store()?.for_doc(text_arg(request, "path")?)?),
        "asks_questions_since" => json!(store()?.questions_since(int_arg(request, "seq")?)?),
        "asks_watch_line" => json!(asks::watch_line(
            int_arg(request, "ask")?,
            text_arg(request, "path")?,
            or_empty("anchor")?,
            text_arg(request, "text")?
        )),
        "asks_default_path" => json!(
            asks::default_path(
                Path::new(text_arg(request, "root")?),
                optional_text(request, "asks_db")?
            )
            .to_string_lossy()
        ),
        other => return Err(bad(format!("unknown function {other:?}"))),
    })
}

const TRIAGE_USAGE: &str = "usage: symphony triage [-h] [--root ROOT] {stats,queue} [n]";
const TRIAGE_HELP: &str =
    "Triage the owner's review queue: measure their verdicts, rank what only they can decide.

  stats      the owner's first-pass approval rate per category, how often an approval still
             carries a note, the letters they pick and how often they answer with a question
  queue N    the N pending cards the owner should see first: design cards by how much
             they gate, then their questions an agent has answered, then the cards only the
             owner signs off, then pictures nobody could decide, each with a link that scrolls
             Symphony to the card

options:
  -h, --help   show this help message and exit
  --root ROOT  project root holding docs/";

/// `symphony triage stats` and `symphony triage queue N [--root ROOT]`.
pub fn triage_main(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let flags = [Flag {
        name: "--root",
        value: true,
    }];
    let parsed = match parse(args, &flags) {
        Ok(parsed) if parsed.help => {
            let _ = writeln!(out, "{TRIAGE_USAGE}\n\n{TRIAGE_HELP}");
            return 0;
        }
        Ok(parsed) => parsed,
        Err(problem) => return refuse(err, TRIAGE_USAGE, "triage", &problem),
    };
    let root = PathBuf::from(parsed.value("--root").unwrap_or("."));
    let (command, rest) = match parsed.positionals.split_first() {
        Some((command, rest)) if command == "stats" || command == "queue" => (command, rest),
        Some((other, _)) => {
            let problem = format!(
                "argument command: invalid choice: {other:?} (choose from 'stats', 'queue')"
            );
            return refuse(err, TRIAGE_USAGE, "triage", &problem);
        }
        None => {
            return refuse(
                err,
                TRIAGE_USAGE,
                "triage",
                "the following arguments are required: command",
            );
        }
    };
    let count = match (command.as_str(), rest) {
        ("stats", []) => 0,
        ("queue", [n]) => match n.parse::<usize>() {
            Ok(n) => n,
            Err(_) => {
                let problem = format!("argument n: invalid int value: {n:?}");
                return refuse(err, TRIAGE_USAGE, "triage queue", &problem);
            }
        },
        ("queue", []) => {
            return refuse(
                err,
                TRIAGE_USAGE,
                "triage queue",
                "the following arguments are required: n",
            );
        }
        (_, extra) => {
            let problem = format!("unrecognized arguments: {}", extra.join(" "));
            return refuse(err, TRIAGE_USAGE, "triage", &problem);
        }
    };
    let docs = match scan(&root) {
        Ok(docs) => docs,
        Err(problem) => {
            let _ = writeln!(err, "✖ {problem}");
            return 1;
        }
    };
    let plugins = crate::plugins::Plugins::of(&root).reading(&root, &docs);
    let said = if command == "stats" {
        match triage::stats(&plugins, &docs) {
            Ok(found) => triage::render_stats(&found),
            Err(problem) => {
                let _ = writeln!(err, "✖ {problem}");
                return 1;
            }
        }
    } else {
        triage::render_queue(&triage::queue(
            &plugins,
            &docs,
            count,
            &triage::app_url(&root),
        ))
    };
    let _ = writeln!(out, "{said}");
    0
}

const TO_ASKS_USAGE: &str = "usage: symphony verdict-asks [-h] [--root ROOT]";
const TO_ASKS_HELP: &str =
    "Turn the open verdict tasks the Symphony filed before asks into asks, once:
each card a verdicts/ task lists that still waits on an agent becomes an ask
keeping the owner's time, and the task is cancelled with a line naming them;
one an executor holds keeps its holder. Run it once in the project's main
checkout, after `just tasks migrate` moved the board to this build's schema.
Run again, it turns nothing more. A server start never does this.

options:
  -h, --help   show this help message and exit
  --root ROOT  the project's main checkout (default: here)";

/// `symphony verdict-asks [--root ROOT]`: [`crate::verdicts::turn`] on the board the
/// environment reaches.
pub fn verdict_asks_main(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let flags = [Flag {
        name: "--root",
        value: true,
    }];
    let parsed = match parse(args, &flags) {
        Ok(parsed) if parsed.help => {
            let _ = writeln!(out, "{TO_ASKS_USAGE}\n\n{TO_ASKS_HELP}");
            return 0;
        }
        Ok(parsed) if !parsed.positionals.is_empty() => {
            let problem = format!("unrecognized arguments: {}", parsed.positionals.join(" "));
            return refuse(err, TO_ASKS_USAGE, "verdict-asks", &problem);
        }
        Ok(parsed) => parsed,
        Err(problem) => return refuse(err, TO_ASKS_USAGE, "verdict-asks", &problem),
    };
    let root = PathBuf::from(parsed.value("--root").unwrap_or("."));
    match crate::verdicts::turn(&root, crate::children::board_env()) {
        Ok(done) if done.is_empty() => {
            let _ = writeln!(
                out,
                "no open verdict task to turn into asks: nothing changed"
            );
            0
        }
        Ok(done) => {
            for line in done {
                let _ = writeln!(out, "{line}");
            }
            0
        }
        Err(problem) => {
            let _ = writeln!(err, "✖ {problem}");
            2
        }
    }
}
