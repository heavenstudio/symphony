//! Each command of `just tasks`, as the board's first, Python, command line ran it: what it asks of
//! the board and the lines it prints.

use super::agents::{describe, table};
use super::args::Parsed;
use super::attach::{Watch, watch};
use super::text::{SNAPSHOT_TITLE, columns, event, export_markdown, message_line, roles_text};
use super::{Clock, Env, probes, shape};
use crate::health::RESERVE_SECONDS;
use crate::stages;
use crate::sweep::{self, git::Listed, trigger};
use crate::sys;
use crate::{Error, NewSession, NewTask, Session, Store, Task};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;

/// Where `export` writes when no `--out` is given.
const EXPORT_PATH: &str = "docs/history/tasks.md";
/// How often an orchestrator's monitor prints the PULL digest, unless `TASKS_PULL_SECONDS` says.
const PULL_SECONDS: f64 = 600.0;

mod asks;
mod finish;
mod gate;
mod limits;
mod list;
mod panes;
mod runs;
mod settings;
mod stamps;
mod stats;
mod wait;
mod workflow;

/// Why a command stopped short.
pub enum Stop {
    /// No `--as` and no `TASKS_SESSION`: exit 2.
    NoSession,
    /// Refused, nothing written: exit 1.
    Refused(String),
    /// The board could not be reached or did not answer, or priority input was refused: exit 2.
    Board(String),
}

impl From<Error> for Stop {
    fn from(error: Error) -> Stop {
        match error {
            Error::Refused(why) => Stop::Refused(why),
            Error::Db(_) => Stop::Board(error.to_string()),
        }
    }
}

/// What a command reads besides the board: who acts, where it runs, its stdin, where it prints, and
/// the clock its waits keep.
pub struct Context<'a> {
    pub parsed: &'a Parsed,
    pub env: &'a Env,
    pub input: &'a mut dyn Read,
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
    pub clock: &'a Clock,
    /// The `tasks` binary this runs as, none in a caller's process: what records the review
    /// command a move starts.
    pub program: Option<&'a std::path::Path>,
}

impl Context<'_> {
    /// `--as`, else `TASKS_SESSION`; the owner writes through the Symphony's store calls.
    fn actor(&self) -> Result<String, Stop> {
        let actor = self
            .parsed
            .get("actor")
            .filter(|a| !a.is_empty())
            .or_else(|| self.env.var("TASKS_SESSION"))
            .ok_or(Stop::NoSession)?;
        super::check_actor(actor)?;
        Ok(actor.to_string())
    }

    pub(super) fn say(&mut self, line: &str) {
        let _ = writeln!(self.out, "{line}");
        let _ = self.out.flush();
    }

    fn arg(&self, dest: &str) -> String {
        self.parsed.or(dest, "").to_string()
    }

    /// `git ARGS` in the working directory: its output, or "" if it fails.
    fn git(&self, args: &[&str]) -> String {
        sys::output(sys::git().args(args).current_dir(&self.env.cwd))
            .ok()
            .filter(|done| done.status.success())
            .map(|done| String::from_utf8_lossy(&done.stdout).trim().to_string())
            .unwrap_or_default()
    }
}

/// A session as `register` and `attach` make it, where the command runs; `pid` is its monitor's.
fn new_session(c: &Context, pid: Option<i64>) -> NewSession {
    NewSession {
        kind: c.parsed.or("kind", "worker").into(),
        parent: c.parsed.get("parent").map(str::to_string),
        worktree: c.git(&["rev-parse", "--show-toplevel"]),
        branch: c.git(&["rev-parse", "--abbrev-ref", "HEAD"]),
        model: c.arg("model"),
        pid,
        host: if pid.is_some() {
            sys::hostname()
        } else {
            String::new()
        },
        affinity: c.arg("affinity").split(',').map(str::to_string).collect(),
    }
}

/// `attach`: register (or take back a handle, `--resume`) with this process as the session's monitor,
/// say so, and watch until the session is refused a heartbeat.
fn attach(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let p = c.parsed;
    let pull_every = match c.env.var("TASKS_PULL_SECONDS") {
        None => PULL_SECONDS,
        Some(text) => text.parse::<f64>().map_err(|_| {
            Stop::Board(format!(
                "TASKS_PULL_SECONDS={text} is not a number of seconds"
            ))
        })?,
    };
    let me = i64::from(std::process::id());
    let session = match p.get("resume").filter(|h| !h.is_empty()) {
        Some(handle) => store.reattach(handle, Some(me))?,
        None => store.register(&new_session(c, Some(me)))?,
    };
    let (handle, kind) = (&session.row.handle, &session.row.kind);
    c.say(&format!(
        "ATTACHED {handle} {kind} — run commands with --as {handle}"
    ));
    let env = c.env;
    let probes = probes::machine(env);
    let root = probes::repo_root(&env.cwd);
    let landed = |t: &Task| {
        root.as_ref()
            .is_some_and(|root| probes::landed(root, &t.slug))
    };
    let tips = probes::tips(root.as_deref());
    let settings = Watch {
        interval: p.float("interval", 3.0),
        beat: p.float("beat", 20.0),
        polls: p.get("polls").map(|_| p.int("polls", 0)),
        probes: Some(&probes),
        landed: Some(&landed),
        tips: Some(&tips),
        pull_every: Some(pull_every),
    };
    let (out, err, input, clock) = (&mut *c.out, &mut *c.err, &mut *c.input, c.clock);
    let mut emit = |line: &str| {
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    };
    let mut wait = |seconds: f64| clock.wait(seconds, input, err);
    watch(store, handle, &settings, &mut emit, &mut wait)?;
    Ok(())
}

/// Run the command `c.parsed` names on `store`.
pub fn run(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let p = c.parsed;
    match p.command.as_str() {
        "callback" => callback(store, c)?,
        "register" => {
            // Checked before the session exists, so a refused address registers nothing.
            let web = p
                .get("session_url")
                .map(crate::session_url::checked)
                .transpose()
                .map_err(Stop::Refused)?
                .flatten();
            let session = store.register(&new_session(c, None))?;
            if let Some(url) = web {
                store.set_session_url(&session.row.handle, &url)?;
            }
            c.say(&session.row.handle);
        }
        "attach" => attach(store, c)?,
        "agents" => agents(store, c)?,
        "list" => list::run(store, c)?,
        "asks" => asks::run(store, c)?,
        "search" => super::search::run(store, c)?,
        "stats" => stats::run(store, c)?,
        "next" => {
            if let Some(t) = store.next_for(&c.actor()?)? {
                // Past `[limits] machine_agents` the task is still named: its asker runs already.
                let hc = crate::launch::hc::Binary::of(c.env);
                if let Some(warning) = crate::launch::running::warning_next(store, c.env, &hc) {
                    let _ = writeln!(c.err, "⚠ {warning}");
                }
                c.say(&format!("{}  p{}  {}", t.slug, t.priority, t.title));
            } else {
                c.say(
                    "nothing to take: no concept, design or ready task is free of an executor, of \
                     another's reservation and of an open dependency (`just tasks list` shows the \
                     board)",
                );
            }
        }
        "limits" => limits::run(store, c)?,
        "settings" => settings::run(store, c)?,
        "workflow" => workflow::run(store, c)?,
        "stamps" => stamps::stamps(store, c)?,
        "runs" => runs::runs(store, c)?,
        "needs" => stamps::needs(store, c)?,
        "gate" => gate::gate(store, c)?,
        "create" => create(store, c)?,
        "priority" => {
            // This command's input errors use exit 2, including a task the board does not hold;
            // who acts is no input of its, refused as say, depends or reserve refuse it, exit 1.
            let actor = c.actor()?;
            let t = store
                .set_priority(&c.arg("slug"), p.int("priority", 2), &actor)
                .map_err(|error| {
                    if store.refuses_actor(&error, &actor) {
                        Stop::from(error)
                    } else {
                        Stop::Board(error.to_string())
                    }
                })?;
            c.say(&format!("{} is p{}", t.slug, t.priority));
        }
        "parent" => {
            if p.flag("none") == p.get("parent").is_some() {
                return Err(Stop::Refused("name the parent, or pass --none".into()));
            }
            let t = store.set_parent(&c.arg("slug"), p.get("parent"), &c.actor()?)?;
            c.say(&format!(
                "{} is under {}",
                t.slug,
                t.parent.as_deref().unwrap_or("nothing")
            ));
        }
        "depends" => {
            let (mut add, mut remove) = (p.all("add").to_vec(), p.all("remove").to_vec());
            for [old, new] in p.all("replace").as_chunks::<2>().0 {
                remove.push(old.clone());
                add.push(new.clone());
            }
            let t = store.change_depends(&c.arg("slug"), &add, &remove, &c.actor()?)?;
            let on = if t.depends.is_empty() {
                "nothing".to_string()
            } else {
                t.depends.join(", ")
            };
            c.say(&format!("{} depends on {on} and is {}", t.slug, t.status));
        }
        "done" => {
            let (slug, note) = (c.arg("slug"), c.arg("note"));
            let t = match p.get("covered_by") {
                Some(other) => {
                    crate::covering::close(store, Some(c.env), &slug, other, &c.actor()?, &note)?
                }
                None => store.done(&slug, &c.actor()?, &note)?,
            };
            c.say(&format!("{slug} is {}", t.status));
            finish::after_finish(store, c, &t);
        }
        "claim" => claim(store, c)?,
        "reserve" => {
            let t = store.reserve(&c.arg("slug"), &c.actor()?, RESERVE_SECONDS)?;
            c.say(&format!("reserved {}", t.slug));
        }
        "release" => {
            let slug = c.arg("slug");
            let t = store.release(&slug, &c.actor()?, p.get("role"))?;
            c.say(&format!("released {slug} ({})", t.status));
        }
        "role" => {
            let (slug, role) = (c.arg("slug"), c.arg("role"));
            store.take_role_beside(&slug, &c.actor()?, &role, p.get("beside"))?;
            c.say(&format!("{role} on {slug}"));
        }
        "handover" => {
            let (slug, to) = (c.arg("slug"), c.arg("to"));
            store.hand_over(&slug, &c.actor()?, &to)?;
            c.say(&format!("handed {slug} to {to} (orchestrator)"));
        }
        "move" => {
            let (slug, status) = (c.arg("slug"), c.arg("status"));
            if status == "review" {
                crate::land::reopenable(store, Some(c.env), &slug)?;
            }
            let board = store.board_command();
            let on = blocking(&board, &slug, &status, p.get("on"), p.get("why"))?;
            let t = store.move_on(&slug, &status, &c.actor()?, p.get("reason"), on)?;
            c.say(&format!("{slug} is {}", t.status));
            finish::after_move(store, c, &t);
        }
        "verdict" => {
            let slug = c.arg("slug");
            let t = store.verdict(&slug, &c.actor()?, &c.arg("verdict"), &c.arg("text"))?;
            // Done is not landed: an approve says what lands it, a subtask's the task it is under.
            c.say(&match (t.status.as_str(), t.parent.as_deref()) {
                ("done", None) => crate::land::approved_task(c.env, &t),
                ("done", Some(parent)) => format!(
                    "{slug} is approved: done on the board; its work lands when the task it is \
                     under, {parent}, does"
                ),
                (status, _) => format!("{slug} is {status}"),
            });
            finish::after_finish(store, c, &t);
            finish::poke(c, &slug);
        }
        "say" => {
            let to = p.get("to").map(recipient);
            let text = said(c)?;
            let m = store.say(&c.arg("slug"), &c.actor()?, &text, to.as_deref())?;
            c.say(&format!("sent #{} on {}", m.id, m.slug));
        }
        "launch" => panes::launch(store, c)?,
        "staff" => panes::staff(store, c)?,
        "restore" => panes::restore(store, c)?,
        "cleanup" => panes::cleanup(store, c)?,
        "clean" => panes::clean(store, c)?,
        "close" => panes::close(store, c)?,
        "reap" => {
            panes::reap(store, c)?;
            if !p.flag("dry_run") {
                trigger::trigger(c.env, &c.env.cwd, "a reap", None);
            }
        }
        "finish" => panes::finish(store, c)?,
        "land" => land(store, c)?,
        "regenerate" => regenerate(store, c)?,
        "show" => show(store, c)?,
        "who" => who(store, c)?,
        "inbox" => inbox(store, c)?,
        "pull" => {
            let me = c.actor()?;
            for line in super::pull::heard_now(store, c.env, &me)?.lines() {
                c.say(&line);
            }
            // What the owner said that nobody received, while nobody holds the chat seat; it
            // stays until an orchestrator's inbox or the seat's holder takes it.
            for (_, text) in store.unheard_owner_events(&me)? {
                c.say(&text);
            }
        }
        "wait" => wait::run(store, c)?,
        "leave" => {
            let me = c.actor()?;
            store.leave(&me)?;
            c.say("left");
            trigger::trigger(c.env, &c.env.cwd, &format!("{me} left"), None);
        }
        "export" => export(store, c)?,
        // The parser knows no other command (`beat` and `api` never come here).
        other => return Err(Stop::Refused(format!("no command {other}"))),
    }
    Ok(())
}

/// `callback`: the session's native address registered, taken off (`--off`) or left as it is,
/// and its web address kept; then who hears the owner's chat.
fn callback(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let p = c.parsed;
    let actor = c.actor()?;
    // Checked first and kept last: an address it refuses, or a registration the board
    // refuses, changes nothing, the session's address included.
    let web = p
        .get("session_url")
        .map(crate::session_url::checked)
        .transpose()
        .map_err(Stop::Refused)?;
    let keep_web = |c: &mut Context<'_>| -> Result<(), Stop> {
        if let Some(url) = &web {
            let kept = store.set_session_url(&actor, url.as_deref().unwrap_or(""))?;
            c.say(&format!(
                "SESSION URL {actor} {}",
                kept.as_deref().unwrap_or("none")
            ));
        }
        Ok(())
    };
    let address = if p.flag("off") {
        None
    } else {
        p.get("address")
            .map(str::to_string)
            .or_else(|| {
                c.env
                    .var("CLAUDE_CODE_MESSAGING_SOCKET")
                    .map(|path| format!("uds:{path}"))
            })
            .or_else(|| {
                c.env
                    .var("CODEX_THREAD_ID")
                    .map(|id| format!("{}{id}", crate::launch::CODEX))
            })
    };
    if !p.flag("off") && address.is_none() {
        if web.is_some() {
            return keep_web(c);
        }
        return Err(Stop::Refused("no native callback detected; pass an address, use inbox for an in-process subagent, or --off to unregister".into()));
    }
    // Through `just tasks`, which an agent runs unasked, a Codex thread is the caller's own:
    // the board writes it into the handle's launch record for a restore to resume.
    if let Some(address) = address.as_deref() {
        crate::launch::own_thread(c.env, address).map_err(Stop::Refused)?;
    }
    store.callback_in(
        &actor,
        address.as_deref(),
        p.flag("chat"),
        p.flag("if_free"),
        c.env.var("HC_TERMINAL_ID"),
    )?;
    keep_web(c)?;
    c.say(&format!(
        "CALLBACK {actor} {}",
        address.as_deref().unwrap_or("none — inbox fallback")
    ));
    c.say(&chat_line(store.chat_seat()?));
    Ok(())
}

/// `agents`: the board's sessions with what the machine says of each, as a table or `--json`.
fn agents(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let p = c.parsed;
    let probes = probes::machine(c.env);
    let view = describe(store, &probes, store.now(), p.flag("all"))?;
    if p.flag("json") {
        c.say(&serde_json::to_string_pretty(&view.json()).expect("JSON values serialize"));
        return Ok(());
    }
    let lines = table(&view);
    if lines.is_empty() {
        c.say(if p.flag("all") {
            "no agents"
        } else {
            "no agents (--all adds the sessions that left)"
        });
    }
    for line in lines {
        c.say(&line);
    }
    let down: Vec<&str> = [("hc", view.herdr), ("slots", view.slots)]
        .iter()
        .filter(|(_, asked)| !asked)
        .map(|(name, _)| *name)
        .collect();
    if !down.is_empty() {
        c.say(&format!(
            "(probe unavailable: {}; states come from the board alone)",
            down.join(", ")
        ));
    }
    Ok(())
}

/// `claim SLUG`: the executor's role taken, through its workflow's edge where a custom workflow
/// holds the task.
fn claim(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let (slug, actor) = (c.arg("slug"), c.actor()?);
    // A task a custom workflow holds is claimed through its one edge that gives the
    // executor's role, at the revision read now (docs/workflow/WORKFLOW.md, "Operations").
    if crate::store::workflow::position_held(&store.get(&slug)?.fields) {
        let principal = crate::workflow::agent_session(&actor);
        let host = crate::workflow::Host::local();
        let role = symphony_api::workflow::Role::Executor;
        let taken = store.claim_by_workflow(&slug, &principal, &host, role)?;
        let at = &taken.next.snapshot.at;
        c.say(&format!(
            "claimed {slug} ({}): it is at {}/{}, workflow revision {}; its edges: `just \
             tasks workflow actions {slug}`",
            taken.transition, at.phase, at.state, taken.next.snapshot.task_revision
        ));
        return Ok(());
    }
    let t = store.claim(&slug, &actor)?;
    // Only a top-level task's slug is the branch that lands: a slice works on its parent's
    // branch, and an instruction subtask's slug is never one.
    let lands = crate::on_review::delivery_task(t.parent.as_deref(), t.instruction);
    c.say(&if lands {
        format!(
            "claimed {0}: its work goes on the branch {0}, which the land takes",
            t.slug
        )
    } else {
        format!("claimed {}", t.slug)
    });
    Ok(())
}

/// `land SLUG`, `land SLUG --dispatch` and `land --watch`: the default landing, its last line said.
fn land(store: &Store, c: &mut Context) -> Result<(), Stop> {
    use crate::land::dispatch;
    let p = c.parsed;
    let slug = c.arg("slug");
    crate::land::admit(c.env).map_err(Stop::Refused)?;
    let line = if p.flag("watch") {
        if !slug.is_empty() || p.flag("dispatch") {
            return Err(Stop::Refused(
                "tasks land --watch takes no slug and no --dispatch: it reads every task \
                 armed at the host"
                    .into(),
            ));
        }
        let watching = dispatch::Watching {
            transport: None,
            clock: &dispatch::seconds,
            rest: &dispatch::rest,
            rounds: None,
        };
        dispatch::watch(store, c.env, &mut *c.out, &watching).map_err(Stop::Refused)?
    } else if slug.is_empty() {
        return Err(Stop::Refused(
            "tasks land takes the slug of the task it lands, or --watch".into(),
        ));
    } else {
        let actor = c.actor()?;
        let landed = if p.flag("dispatch") {
            crate::land::dispatch(store, c.env, &slug, &actor, &mut *c.out)
        } else {
            crate::land::land(store, c.env, &slug, &actor, c.program, &mut *c.out)
        };
        landed.map_err(|stop| match stop {
            crate::land::Stop::Refused(why) => Stop::Refused(why),
            crate::land::Stop::Board(error) => Stop::from(error),
        })?
    };
    c.say(&line);
    Ok(())
}

/// `regenerate`: the regenerator run, or with `--status` or `--wait` where it stands, refused
/// while what it made is not current.
fn regenerate(store: &Store, c: &mut Context) -> Result<(), Stop> {
    use crate::land::regenerate;
    let p = c.parsed;
    if p.flag("status") || p.flag("wait") {
        let (env, wait) = (c.env, p.flag("wait"));
        let mut said = false;
        let standing = regenerate::settled(env, regenerate::WAITING, &mut |standing| {
            if wait && !said {
                c.say(&format!("waiting: {}", standing.line));
                said = true;
            }
            wait
        })
        .map_err(Stop::Refused)?;
        if !standing.current {
            return Err(Stop::Refused(standing.line));
        }
        c.say(&standing.line);
    } else {
        regenerate::admit(c.env).map_err(Stop::Refused)?;
        let line = regenerate::run(store, c.env, &mut *c.out).map_err(Stop::Refused)?;
        c.say(&line);
    }
    Ok(())
}

/// `inbox`: the actor's unread messages and queued callbacks said and acknowledged.
fn inbox(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let me = c.actor()?;
    let messages = store.inbox(&me)?;
    for m in &messages {
        c.say(&event(m));
    }
    if messages.is_empty() {
        store.touch(&me)?;
    } else {
        let ids: Vec<i64> = messages.iter().map(|m| m.id).collect();
        store.ack(&me, &ids)?;
    }
    let callbacks = store.callback_inbox(&me)?;
    for (key, text) in &callbacks {
        c.say(&super::attach::PaneWatch::delivery_text(key, text));
    }
    store.ack_callbacks(
        &me,
        &callbacks
            .into_iter()
            .map(|(key, _)| key)
            .collect::<Vec<_>>(),
    )?;
    trigger::hourly(c.env, &c.env.cwd);
    Ok(())
}

/// `export`: the board's snapshot written as Markdown to `--out`, else [`EXPORT_PATH`].
fn export(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let p = c.parsed;
    let named = p.or("out", EXPORT_PATH).to_string();
    // Through `just tasks`, which an agent runs unasked, the snapshot stays in the project.
    let marked = c.env.var(crate::confine::MARKER).is_some();
    let path =
        crate::confine::checked(marked, &c.env.cwd, Path::new(&named)).map_err(Stop::Refused)?;
    // ...and replaces only an earlier snapshot, never a file of the project's own.
    if marked && path.symlink_metadata().is_ok() && !snapshot(&path) {
        return Err(Stop::Refused(format!(
            "{named} holds something other than an earlier snapshot: the board recipe, \
             which an agent runs unasked, replaces only a file whose first line is \
             `{SNAPSHOT_TITLE}`, and the binary run by itself anything"
        )));
    }
    let text = export_markdown(&store.tasks(None)?, store.now());
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&path, text));
    written.map_err(|e| Stop::Refused(format!("cannot write {named}: {e}")))?;
    c.say(&format!("wrote {named}"));
    Ok(())
}

/// Whether the file at `path` is a snapshot `tasks export` wrote: its first line is
/// [`SNAPSHOT_TITLE`]. Only that much of it is read.
fn snapshot(path: &Path) -> bool {
    let mut head = Vec::new();
    let read = std::fs::File::open(path).and_then(|file| {
        file.take(SNAPSHOT_TITLE.len() as u64 + 1)
            .read_to_end(&mut head)
    });
    read.is_ok()
        && head
            .strip_prefix(SNAPSHOT_TITLE.as_bytes())
            .is_some_and(|rest| matches!(rest, [] | [b'\n'] | [b'\r']))
}

/// What `callback` says of the owner's chat: who hears it and, when that session holds no seat,
/// that it hears it as a fallback, why, and what takes the seat.
fn chat_line(seat: Option<(String, crate::ChatSeat)>) -> String {
    use crate::ChatSeat::{Held, NextInLine, Orchestrator, Waiting};
    let Some((handle, why)) = seat else {
        return "CHAT nobody registered".into();
    };
    let fallback = match why {
        Held => return format!("CHAT {handle}"),
        NextInLine => "next in line by --chat --if-free",
        Orchestrator => "an orchestrator with a callback",
        Waiting => "an orchestrator reading by tasks wait",
    };
    format!("CHAT {handle} (fallback: {fallback}; --chat takes the seat)")
}

/// Whom `say --to` addresses: `owner` and a role after `@` in any case, as the board spells them;
/// a session's handle as it is written.
fn recipient(to: &str) -> String {
    if to.eq_ignore_ascii_case("owner") {
        return "owner".into();
    }
    let role = to.strip_prefix('@').and_then(|role| {
        crate::ROLES
            .iter()
            .find(|known| known.eq_ignore_ascii_case(role))
    });
    match role {
        Some(role) => format!("@{role}"),
        None => to.into(),
    }
}

/// The line `say` posts: its `text` argument, or what `--file` names (stdin for `-`), so a long
/// post is written as a file and never quoted for a shell; the store trims its ends as it trims any
/// line (`Store::say`). One of the two, never both and never neither.
fn said(c: &mut Context) -> Result<String, Stop> {
    let p = c.parsed;
    match (p.get("file"), p.get("text")) {
        (Some(_), Some(_)) => Err(Stop::Refused(
            "say takes its line as TEXT or from --file, never both".into(),
        )),
        (None, None) => Err(Stop::Refused(
            "say needs its line: TEXT, or --file FILE (- for stdin)".into(),
        )),
        (None, Some(text)) => Ok(text.to_string()),
        (Some(file), None) => file_or_stdin(c, file, "line", "--file"),
    }
}

/// What `file` holds, by its path from the command's folder, or stdin for `-`: the text an option
/// `flag` names, `what` in its refusals (`no body file X: --body-file names it …`).
fn file_or_stdin(c: &mut Context, file: &str, what: &str, flag: &str) -> Result<String, Stop> {
    if file == "-" {
        let mut text = String::new();
        c.input
            .read_to_string(&mut text)
            .map_err(|e| Stop::Refused(format!("cannot read the {what} from stdin: {e}")))?;
        return Ok(text);
    }
    std::fs::read_to_string(c.env.cwd.join(file)).map_err(|e| {
        Stop::Refused(match e.kind() {
            std::io::ErrorKind::NotFound => format!(
                "no {what} file {file}: {flag} names it by its path from here, or - for stdin"
            ),
            _ => format!("cannot read the {what} file {file}: {e}"),
        })
    })
}

/// `create SLUG TITLE`: a new task from its arguments and its body (the `body` argument, a file, or
/// stdin for `-`), refused when the brief names the worktree of a dependency that has not landed
/// ([`imported_worktree`]).
fn create(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let p = c.parsed;
    // The board's own rule takes one plain word too; a task filed here names its area.
    let slug = c.arg("slug");
    if !slug.contains('/') {
        return Err(Stop::Refused(format!(
            "slug {} names no area: a task's slug is area/short-name, such as tools/{slug}",
            crate::quote::repr(&slug)
        )));
    }
    let body = match p.get("body_file") {
        Some(file) => file_or_stdin(c, file, "body", "--body-file")?,
        None => c.arg("body"),
    };
    // Asked once, and only for a dependency not done; outside a repository git lists none.
    let mut listed: Option<Vec<Listed>> = None;
    for dep in p.all("depends") {
        if store.get(dep)?.status == "done" {
            continue;
        }
        let listed =
            listed.get_or_insert_with(|| sweep::git::worktrees(&c.env.cwd).unwrap_or_default());
        if let Some(named) = imported_worktree(&body, dep, listed) {
            return Err(Stop::Refused(format!(
                "the brief names {named}, the worktree of {dep}, which has not landed; code and data build only on main \
                 (RULES.md workflow/shared-data-lands-first). Wait for that task to land, or ask on its thread"
            )));
        }
    }
    // `--no-role` files it for another orchestrator, named with `--for`, and no reservation.
    let no_role = p.flag("no_role");
    let for_orchestrator = match (no_role, p.get("assignee")) {
        (true, Some(to)) => Some(live_orchestrator(store, to)?),
        (true, None) => {
            return Err(Stop::Refused(
                "--no-role files a task for another orchestrator: name a live one with --for \
                 HANDLE (every open task has an orchestrator)"
                    .into(),
            ));
        }
        (false, _) => None,
    };
    let new = NewTask {
        slug,
        title: c.arg("title"),
        by: c.actor()?,
        body,
        priority: p.int("priority", 2),
        status: p.or("stage", "ready").into(),
        parent: p.get("parent").map(str::to_string),
        links: p.all("link").to_vec(),
        depends: p.all("depends").to_vec(),
        assignee: p.get("assignee").filter(|_| !no_role).map(str::to_string),
        instruction: p.flag("instruction"),
        // Reds are marked by what files them (`tasks api` `create`); work under one is red work.
        red: None,
        // No task is filed as a fast fix any more: what it needs to land is its assessor's.
    };
    let t = match p.get("arm") {
        Some(arm) => store.create_in_arm(&new, arm)?,
        None => store.create(&new)?,
    };
    // The filer orchestrates it when it is an orchestrator under the limit on its open tasks, else
    // the board gives it. One line says who, and why the filer or the orchestrator `--for` names was
    // passed over, or that it waits for one under its limit that stays, where any orchestrator is
    // on the board.
    let role = match store.give_filed_saying(&t.slug, &new.by, for_orchestrator.as_deref()) {
        Ok(filed) => {
            let passed = filed
                .passed
                .map(|why| format!("; {why}"))
                .unwrap_or_default();
            match filed.orchestrator {
                Some(to) => format!("{passed}; {to} orchestrates it"),
                None if !any_orchestrator(store)? => passed,
                None => {
                    format!("{passed}; it waits for an orchestrator under its limit that stays")
                }
            }
        }
        Err(error) => format!(
            "; no orchestrator was given it yet ({error}): the board gives it at its next read"
        ),
    };
    c.say(&format!("created {} ({}){role}", t.slug, t.status));
    Ok(())
}

/// Whether a session of kind orchestrator that has not left is on the board. With none, as on a new
/// project's board, a task waiting for one is not said to wait: nothing there would give it one.
fn any_orchestrator(store: &Store) -> Result<bool, Stop> {
    let sessions = store.sessions()?;
    Ok((sessions.iter()).any(|s| s.row.kind == "orchestrator" && s.health != crate::Health::Left))
}

/// `handle`, when it names a live orchestrator's session: what `--no-role --for` files a task for.
fn live_orchestrator(store: &Store, handle: &str) -> Result<String, Stop> {
    let named = |why: String| {
        Stop::Refused(format!(
            "--no-role --for names the orchestrator that takes the task, and {why}"
        ))
    };
    match store.session(handle) {
        Ok(s) if s.row.kind != "orchestrator" => Err(named(format!(
            "{handle} is a session of kind {}",
            s.row.kind
        ))),
        Ok(s) if s.health != crate::Health::Live => {
            Err(named(format!("{handle} is {}", s.health.as_str())))
        }
        Ok(_) => Ok(handle.to_string()),
        Err(Error::Refused(_)) => Err(named(format!("no session is {handle}"))),
        Err(error) => Err(error.into()),
    }
}

/// Where `body` names a worktree of dependency `dep` as a source of data, which only builds on main
/// once `dep` lands. A worktree among `listed` on `dep`'s branch (its slug, or its slug with the last
/// `/` spelled `-`, as the sweep ties them), whatever its folder is called (`agent-*`, `codex-*`), is
/// named by its last two path components, so an absolute, `~/` or `../` spelling counts; its full path
/// is returned. Else `.claude/worktrees/` and the slug with dashes, the folder a launch would make.
fn imported_worktree(body: &str, dep: &str, listed: &[Listed]) -> Option<String> {
    let branches: Vec<String> = [Some(dep.to_string()), sweep::spelled(dep)]
        .into_iter()
        .flatten()
        .map(|b| format!("refs/heads/{b}"))
        .collect();
    let found = listed
        .iter()
        .filter(|w| w.branch.as_ref().is_some_and(|b| branches.contains(b)))
        .find(|w| tail(&w.path).is_some_and(|tail| names_folder(body, &tail)));
    if let Some(worktree) = found {
        return Some(worktree.path.display().to_string());
    }
    let needle = format!(".claude/worktrees/{}", dep.replace('/', "-"));
    names_folder(body, &needle).then_some(needle)
}

/// `path`'s folder and the folder it is in (`worktrees/agent-a1b2`); none at the root.
fn tail(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let above = path.parent()?.file_name()?.to_str()?;
    Some(format!("{above}/{name}"))
}

/// Whether `body` names the folder `needle`: not followed by more of a name, nor by `/artifacts/`
/// (evidence, not data).
fn names_folder(body: &str, needle: &str) -> bool {
    body.match_indices(needle).any(|(at, _)| {
        let rest = &body[at + needle.len()..];
        let more = rest
            .chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '-');
        !more && !rest.starts_with("/artifacts/")
    })
}

/// What a move of `slug` to `status` waits on, as `--on` and `--why` name it: a block names a
/// thing, so one without `--on` is refused, saying what to pass (`board`, the command that reaches
/// this board); a move anywhere else waits on nothing they name.
fn blocking<'a>(
    board: &str,
    slug: &str,
    status: &str,
    on: Option<&'a str>,
    why: Option<&'a str>,
) -> Result<Option<crate::On<'a>>, Stop> {
    match (status, on) {
        ("blocked", Some(thing)) => Ok(Some(crate::On { thing, why })),
        ("blocked", None) => Err(Stop::Refused(format!(
            "a block names what it waits on: {board} move {slug} blocked --on THING [--why \
             TEXT], THING one of {}; nothing was moved",
            crate::waits_on::THINGS
        ))),
        (_, None) if why.is_none() => Ok(None),
        _ => Err(Stop::Refused(format!(
            "--on and --why say what a block waits on: {slug} moves to {status}; nothing was \
             moved"
        ))),
    }
}

/// What `task` waits on ([`crate::waits_on::line`]), read over the tasks it needs, so `show` reads
/// no whole board: `children`, the tasks right under it that `show` lists (the board's listings
/// read every depth), and in one read its dependencies and a task its block names; its landing's
/// record taken at the tip it judged, as the listings take it. None for a finished task.
fn waits_on(
    store: &Store,
    task: &Task,
    children: &[Task],
) -> Result<Option<crate::waits_on::Line>, Stop> {
    use crate::waits_on::{Blocked, Landing, Look, Thing, line};
    if crate::health::FINISHED.contains(&task.status.as_str()) {
        return Ok(None);
    }
    let mut near = task.depends.clone();
    if let Some(Thing::Task(target)) = Blocked::of(task).as_ref().and_then(Blocked::thing) {
        near.push(target.to_string());
    }
    let mut tasks = vec![task.clone()];
    tasks.extend(children.iter().cloned());
    tasks.extend(store.tasks_by_slug(&near)?);
    let look = Look::new(&tasks, &[]);
    Ok(line(task, &look, Landing::of(task, None).as_ref()))
}

fn show(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let slug = c.arg("slug");
    let task = store.get(&slug)?;
    let thread = store.thread(&slug)?;
    let children = store.children(&slug)?;
    let lines: Vec<stages::Line> = thread.iter().map(stages::Line::from).collect();
    let facts = stages::Facts {
        status: &task.status,
        created: task.created,
        updated: task.updated,
    };
    let stages = stages::stages(facts, &lines, store.now());
    // The task's tokens, its subtasks' with them, and their estimated cost.
    let slugs: Vec<&str> = std::iter::once(slug.as_str())
        .chain(children.iter().map(|child| child.slug.as_str()))
        .collect();
    let spent = store.spent_on(&slugs)?;
    let prices = crate::usage::Prices::embedded();
    let tally = crate::usage::Tally::at(&spent, prices);
    if c.parsed.flag("json") {
        let json = json!({
            "task": shape::task(&task),
            "thread": shape::messages(&thread),
            "children": shape::tasks(&children),
            "stages": shape::stages(&stages),
            "tokens": tally.json(&spent, prices),
        });
        c.say(&serde_json::to_string_pretty(&json).expect("JSON values serialize"));
        return Ok(());
    }
    c.say(&format!(
        "{} — {}  [{}, p{}]",
        task.slug, task.title, task.status, task.priority
    ));
    for r in &task.roles {
        c.say(&format!(
            "  {} {} ({})",
            r.role,
            r.handle,
            r.health.as_str()
        ));
    }
    if let Some(line) = waits_on(store, &task, &children)? {
        let since = store.now() - stages.current_since;
        c.say(&format!("  {}", crate::waits_on::said(&line, since)));
    }
    let held = (task.roles.iter()).any(|r| r.role == "orchestrator" && r.health.holds());
    let waiting = match crate::orchestration::waiting(&task.fields) {
        Some(at) if !held && any_orchestrator(store)? => Some(at),
        _ => None,
    };
    if let Some(at) = waiting {
        let minutes = ((store.now() - at) / 60.0).max(0.0).floor();
        let limit = store.limits().orchestrator_tasks;
        c.say(&format!(
            "  waits for an orchestrator under its limit that stays, for {minutes} min: the first \
             live past its first live line that waits, watches or holds a task, and orchestrates \
             fewer than {limit} open tasks ([limits] orchestrator_tasks), is given it, and any \
             such orchestrator takes it with: {} role {} orchestrator",
            store.board_command(),
            task.slug
        ));
    }
    let parent: Vec<String> = task.parent.iter().cloned().collect();
    for (label, items) in [
        ("links", &task.links),
        ("depends on", &task.depends),
        ("parent", &parent),
    ] {
        if !items.is_empty() {
            c.say(&format!("  {label}: {}", items.join(", ")));
        }
    }
    if let Some(arm) = task.arm() {
        c.say(&format!("  arm: {arm}"));
    }
    c.say(&format!("  {}", stages::summary_line(&stages)));
    for line in tally.lines() {
        c.say(&format!("  {line}"));
    }
    if !children.is_empty() {
        c.say("  subtasks:");
        let width = children
            .iter()
            .map(|child| child.slug.chars().count())
            .max()
            .unwrap_or(0);
        for child in &children {
            let held = roles_text(child);
            let held = if held.is_empty() { "-".into() } else { held };
            c.say(&format!(
                "    {:<width$} {:<11} {held}",
                child.slug, child.status
            ));
        }
    }
    let launched = store.task_sessions(&slug)?;
    if !launched.is_empty() {
        c.say("  Sessions:");
        for s in &launched {
            c.say(&format!(
                "    {:<18} {:<9} {}  {}  {}  {}  {}",
                s.handle,
                s.role,
                s.claude_session,
                s.worktree,
                if s.branch.is_empty() {
                    "(detached)"
                } else {
                    &s.branch
                },
                if s.pane.is_empty() { "-" } else { &s.pane },
                if s.closed.is_none() { "open" } else { "closed" }
            ));
        }
    }
    if task.instruction {
        c.say(&format!(
            "  an instruction subtask: close it with `just tasks done {} \"what came of it\"`",
            task.slug
        ));
    }
    if !task.body.trim().is_empty() {
        c.say("");
        c.say(task.body.trim_end());
    }
    c.say("");
    for m in &thread {
        c.say(&message_line(m));
    }
    Ok(())
}

fn who(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let all = c.parsed.flag("all");
    let sessions: Vec<Session> = store
        .sessions()?
        .into_iter()
        .filter(|s| all || s.health.as_str() != "left")
        .collect();
    if c.parsed.flag("json") {
        let json = json!({
            "sessions": Value::Array(sessions.iter().map(shape::session).collect()),
        });
        c.say(&serde_json::to_string_pretty(&json).expect("JSON values serialize"));
        return Ok(());
    }
    let mut held: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for t in store.tasks(None)? {
        for r in &t.roles {
            held.entry(r.handle.clone())
                .or_default()
                .push(format!("{}({})", t.slug, r.role));
        }
    }
    if sessions.is_empty() {
        c.say(if all {
            "no sessions"
        } else {
            "no sessions (--all adds the ones that left)"
        });
        return Ok(());
    }
    let names: Vec<&str> = sessions.iter().map(|s| s.row.handle.as_str()).collect();
    /// `s`'s row, its handle indented under its parent's, then its children's rows.
    fn rows(
        s: &Session,
        depth: usize,
        sessions: &[Session],
        held: &BTreeMap<String, Vec<String>>,
        out: &mut Vec<Vec<String>>,
    ) {
        let r = &s.row;
        out.push(vec![
            format!("{}{}", "  ".repeat(depth), r.handle),
            r.kind.clone(),
            s.health.as_str().to_string(),
            if r.branch.is_empty() { "-" } else { &r.branch }.to_string(),
            held.get(&r.handle).map(|h| h.join(" ")).unwrap_or_default(),
        ]);
        for child in sessions {
            if child.row.parent.as_deref() == Some(r.handle.as_str()) {
                rows(child, depth + 1, sessions, held, out);
            }
        }
    }
    let mut table = vec![
        ["HANDLE", "KIND", "HEALTH", "BRANCH", "ROLES"]
            .iter()
            .map(|h| h.to_string())
            .collect(),
    ];
    for s in &sessions {
        if !s.row.parent.as_deref().is_some_and(|p| names.contains(&p)) {
            rows(s, 0, &sessions, &held, &mut table);
        }
    }
    for line in columns(&table) {
        c.say(&line);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{chat_line, imported_worktree, recipient};
    use crate::ChatSeat;
    use crate::sweep::git::Listed;
    use std::path::PathBuf;

    #[test]
    fn the_chat_line_says_when_its_recipient_hears_chat_only_as_a_fallback() {
        let seat = |why| Some(("amber-heron".to_string(), why));
        assert_eq!(chat_line(seat(ChatSeat::Held)), "CHAT amber-heron");
        for (why, fallback) in [
            (ChatSeat::NextInLine, "next in line by --chat --if-free"),
            (ChatSeat::Orchestrator, "an orchestrator with a callback"),
            (ChatSeat::Waiting, "an orchestrator reading by tasks wait"),
        ] {
            assert_eq!(
                chat_line(seat(why)),
                format!("CHAT amber-heron (fallback: {fallback}; --chat takes the seat)")
            );
        }
        assert_eq!(chat_line(None), "CHAT nobody registered");
    }

    #[test]
    fn the_owner_and_a_role_are_addressed_in_any_case_and_a_handle_as_written() {
        for (to, addressed) in [
            ("owner", "owner"),
            ("Owner", "owner"),
            ("OWNER", "owner"),
            ("@orchestrator", "@orchestrator"),
            ("@Orchestrator", "@orchestrator"),
            ("@REVIEWER", "@reviewer"),
            // What the board refuses stays as it was written, so its refusal names it.
            ("@nobody", "@nobody"),
            ("Quiet-Otter", "Quiet-Otter"),
            ("orchestrator", "orchestrator"),
        ] {
            assert_eq!(recipient(to), addressed, "{to}");
        }
    }

    /// A worktree at `path` on branch `name`, detached when `name` is empty.
    fn at(path: &str, name: &str) -> Listed {
        Listed {
            path: PathBuf::from(path),
            branch: (!name.is_empty()).then(|| format!("refs/heads/{name}")),
            ..Listed::default()
        }
    }

    #[test]
    fn a_slug_named_folder_counts_only_as_a_whole_name() {
        for body in [
            "Read .claude/worktrees/content-lineup/content/x.json",
            "Read .claude/worktrees/content-lineup.",
            "Read .claude/worktrees/content-lineup now",
            "Read .claude/worktrees/content-lineup",
        ] {
            assert_eq!(
                imported_worktree(body, "content/lineup", &[]).as_deref(),
                Some(".claude/worktrees/content-lineup"),
                "{body}"
            );
        }
        // x-y never names x-yz, nor x-yz x-y, whichever of the two is the dependency.
        for (body, dep) in [
            (
                "Read .claude/worktrees/content-lineup-stretch/x.json",
                "content/lineup",
            ),
            (
                "Read .claude/worktrees/content-lineup_2/x.json",
                "content/lineup",
            ),
            (
                "Read .claude/worktrees/content-lineup2/x.json",
                "content/lineup",
            ),
            (
                "Read .claude/worktrees/content-lineup/x.json",
                "content/lineup-stretch",
            ),
        ] {
            assert_eq!(imported_worktree(body, dep, &[]), None, "{body} for {dep}");
        }
    }

    #[test]
    fn a_dependency_in_an_agent_folder_is_found_by_its_branch() {
        let listed = [
            at("/r", "main"),
            at("/r/.claude/worktrees/agent-a1b2", "content/lineup"),
        ];
        let body = "Import .claude/worktrees/agent-a1b2/content/x.json.";
        assert_eq!(
            imported_worktree(body, "content/lineup", &listed).as_deref(),
            Some("/r/.claude/worktrees/agent-a1b2")
        );
        for (body, dep) in [
            // Its boundary holds as a slug's does.
            (
                "Import .claude/worktrees/agent-a1b2c/content/x.json",
                "content/lineup",
            ),
            (
                "See .claude/worktrees/agent-a1b2/artifacts/lamp.png",
                "content/lineup",
            ),
            // Another dependency's branch is not on it.
            (body, "content/other"),
        ] {
            assert_eq!(
                imported_worktree(body, dep, &listed),
                None,
                "{body} for {dep}"
            );
        }
    }

    #[test]
    fn a_dependency_in_a_codex_folder_is_found_by_its_branch_spelled_either_way() {
        let listed = [
            at("/r/.claude/worktrees/codex-lineup-pilot", "content/lineup"),
            at("/u/.codex/worktrees/1767/fixtures", "content-stretch"),
        ];
        for (body, dep, named) in [
            (
                "Import /r/.claude/worktrees/codex-lineup-pilot/content/x.json",
                "content/lineup",
                "/r/.claude/worktrees/codex-lineup-pilot",
            ),
            (
                "Import ~/.codex/worktrees/1767/fixtures/content/x.json",
                "content/stretch",
                "/u/.codex/worktrees/1767/fixtures",
            ),
        ] {
            assert_eq!(
                imported_worktree(body, dep, &listed).as_deref(),
                Some(named),
                "{body}"
            );
        }
    }

    #[test]
    fn a_brief_naming_no_worktree_names_none() {
        let listed = [
            at("/r", "main"),
            at("/r/.claude/worktrees/agent-a1b2", "content/lineup"),
        ];
        for body in [
            "",
            "Build the lineup from content/x.json on main.",
            "Read /r/content/x.json and the lineup's thread.",
        ] {
            assert_eq!(
                imported_worktree(body, "content/lineup", &listed),
                None,
                "{body}"
            );
        }
    }
}
