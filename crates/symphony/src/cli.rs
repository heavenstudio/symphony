//! The `symphony` binary (`just symphony-cli`): `status` (`just symphony-status`), `reply`
//! (`just symphony-reply`), `triage`, `ask` (`just symphony-ask`), the commands
//! the project's plugins add, and `serve`, one line-oriented JSON entry to the register, the asks and
//! the schema check, one JSON line per call.

mod commands;
mod publish;

pub(crate) use publish::publish_client;

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};

use crate::asks::{self, AskError, AskStore};
use crate::board::{Doc, render, scan, tidy, to_json};
use crate::callouts::{Comment, parse_comments};
use crate::clock::now_stamp;
use crate::edit::{self, EditError};
use crate::lint;
use crate::plugins::Plugins;
use crate::register::{self, Decision, QUESTION, ReviewError, Who, card_end, parse_decisions};
use crate::text::{os_error, read_text, write_whole};
use crate::{schema, triage};

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
const REPLY_USAGE: &str = "usage: symphony reply [-h] [--question] [--now NOW] path slug text";
const REPLY_HELP: &str = "An agent's line under a card, appended the way the app appends the owner's.

positional arguments:
  path
  slug
  text

options:
  -h, --help  show this help message and exit
  --question  the line asks the owner something: the card stays open for the owner (`agent · question`)
  --now NOW   timestamp (default: local time)";

/// `symphony COMMAND ARGS…`: the whole command line without the program name. Its exit code. A
/// command the Symphony does not have is run by the first plugin this process holds that adds it.
pub fn main(
    args: &[String],
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    match args.first().map(String::as_str) {
        Some("status") => status_main(&args[1..], out, err),
        Some("reply") => reply_main(&args[1..], out, err),
        Some("serve") => serve_main(input, out),
        Some("docs-audit") => crate::docs_audit::main(&args[1..], out, err),
        Some("blocks") => crate::blocks::main(&args[1..], out, err),
        Some("publish-client") => publish::publish_client_main(&args[1..], err),
        Some("triage") => triage_main(&args[1..], out, err),
        Some("ask") => asks::main(&args[1..], &std::env::vars().collect(), out, err),
        Some("project") => crate::machine::command::main(&args[1..], out, err),
        Some("hook") => crate::hook::main(&args[1..], input, err),
        Some(other) => commands::plugin(other, &args[1..], out, err),
        None => commands::usage(err),
    }
}

pub use symphony_args::{Flag, Parsed, command_at, parse};

/// A refusal of the command line, as argparse words one: the usage, then the error. Exit 2.
pub fn refuse(err: &mut dyn Write, usage: &str, command: &str, problem: &str) -> i32 {
    let _ = writeln!(err, "{usage}\nsymphony {command}: error: {problem}");
    2
}

/// `symphony status [--root ROOT] [--json] [--open] [--tidy] [--lint] [--now STAMP]`.
pub fn status_main(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
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
        return tidy(&root, parsed.value("--now"), out, err);
    }
    if parsed.flag("--lint") {
        return lint::main(&root, out, err);
    }
    let docs = match scan(&root) {
        Ok(docs) => docs,
        Err(problem) => {
            let _ = writeln!(err, "✖ concept review board could not run: {problem}");
            return 2;
        }
    };
    let plugins = crate::plugins::Plugins::of(&root);
    let said = if parsed.flag("--json") {
        to_json(&plugins, &docs)
    } else {
        render(&plugins, &docs, parsed.flag("--open"))
    };
    let _ = writeln!(out, "{said}");
    0
}

/// `symphony reply PATH SLUG TEXT [--question] [--now STAMP]`: an agent's line under a card.
pub fn reply_main(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let flags = [
        Flag {
            name: "--question",
            value: false,
        },
        Flag {
            name: "--now",
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
    let file = Path::new(path);
    let written = read_text(file)
        .map_err(|e| os_error(&e, file))
        .and_then(|old| {
            edit::add_message(&old, slug, text, &now, Who::Agent, verdict).map_err(|e| e.0)
        })
        .and_then(|new| write_whole(file, &new).map_err(|e| os_error(&e, file)));
    if let Err(problem) = written {
        let _ = writeln!(err, "✖ {problem}");
        return 1;
    }
    let _ = writeln!(out, "replied on {slug} in {path}");
    0
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
            std::thread::sleep(PARENT_LOOK);
        }
        let _in_hand = IN_HAND.lock().unwrap_or_else(PoisonError::into_inner);
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
        let _in_hand = IN_HAND.lock().unwrap_or_else(PoisonError::into_inner);
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

/// Any JSON the request carries under `name`; null when it carries none.
fn value_arg<'a>(request: &'a Value, name: &str) -> &'a Value {
    request.get(name).unwrap_or(&Value::Null)
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
        "validate" => json!(
            schema::validate(value_arg(request, "instance"), value_arg(request, "schema"))
                .map_err(|e| bad(e.0))?
        ),
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
    let said = if command == "stats" {
        match triage::stats(&crate::plugins::Plugins::of(&root), &docs) {
            Ok(found) => triage::render_stats(&found),
            Err(problem) => {
                let _ = writeln!(err, "✖ {problem}");
                return 1;
            }
        }
    } else {
        let plugins = crate::plugins::Plugins::of(&root);
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
