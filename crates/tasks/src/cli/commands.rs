//! Each command of `just tasks`, as `tools/tasks.py` ran it: what it asks of the board and the lines it
//! prints.

use super::agents::{describe, table};
use super::args::Parsed;
use super::attach::{Watch, watch};
use super::text::{event, export_markdown, message_line, roles_text};
use super::{Clock, Env, probes, shape};
use crate::health::RESERVE_SECONDS;
use crate::pyjson::{self, Py};
use crate::stages;
use crate::sweep::{self, git::Listed, trigger};
use crate::sys;
use crate::{Error, NewSession, NewTask, Session, Store, Task};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;

/// Where `export` writes when no `--out` is given.
const EXPORT_PATH: &str = "docs/history/tasks.md";
/// How often an orchestrator's monitor prints the PULL digest, unless `TASKS_PULL_SECONDS` says.
const PULL_SECONDS: f64 = 600.0;

mod finish;
mod list;
mod panes;
mod stats;

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
        sys::output(sys::command("git").args(args).current_dir(&self.env.cwd))
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
        "ATTACHED {handle} {kind} — run commands with TASKS_SESSION={handle} (or --as {handle})"
    ));
    let env = c.env;
    let probes = probes::machine(env);
    let root = probes::repo_root(&env.cwd);
    let landed = |t: &Task| {
        root.as_ref()
            .is_some_and(|root| probes::landed(root, &t.slug))
    };
    let settings = Watch {
        interval: p.float("interval", 3.0),
        beat: p.float("beat", 20.0),
        polls: p.get("polls").map(|_| p.int("polls", 0)),
        probes: Some(&probes),
        landed: Some(&landed),
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
        "callback" => {
            let actor = c.actor()?;
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
                    .or_else(|| c.env.var("CODEX_THREAD_ID").map(|id| format!("codex:{id}")))
            };
            if !p.flag("off") && address.is_none() {
                return Err(Stop::Refused("no native callback detected; pass an address, use inbox for an in-process subagent, or --off to unregister".into()));
            }
            store.callback_in(
                &actor,
                address.as_deref(),
                p.flag("chat"),
                p.flag("if_free"),
                c.env.var("HC_TERMINAL_ID"),
            )?;
            c.say(&format!(
                "CALLBACK {actor} {}",
                address.as_deref().unwrap_or("none — inbox fallback")
            ));
            c.say(&format!(
                "CHAT {}",
                store
                    .chat_recipient()?
                    .as_deref()
                    .unwrap_or("nobody registered")
            ));
        }
        "register" => {
            let session = store.register(&new_session(c, None))?;
            c.say(&session.row.handle);
        }
        "attach" => attach(store, c)?,
        "agents" => {
            let probes = probes::machine(c.env);
            let view = describe(store, &probes, store.now(), p.flag("all"))?;
            if p.flag("json") {
                c.say(&pyjson::dumps(&view.json()));
                return Ok(());
            }
            for line in table(&view) {
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
        }
        "list" => list::run(store, c)?,
        "search" => super::search::run(store, c)?,
        "stats" => stats::run(store, c)?,
        "embed" => {
            // Only the cache-only maintenance mode may run without an agent. Explicit actors
            // still pass the normal validation, including the owner restriction.
            if !p.flag("maintenance")
                || p.get("actor").is_some()
                || c.env.var("TASKS_SESSION").is_some()
            {
                store.session(&c.actor()?)?;
            }
            super::embedding::run(store, c)?;
        }
        "next" => {
            if let Some(t) = store.next_for(&c.actor()?)? {
                c.say(&format!("{}  p{}  {}", t.slug, t.priority, t.title));
            }
        }
        "create" => create(store, c)?,
        "priority" => {
            // This command's input errors use exit 2, including a task the board does not hold.
            let t = store
                .set_priority(&c.arg("slug"), p.int("priority", 2), &c.actor()?)
                .map_err(|error| Stop::Board(error.to_string()))?;
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
            let slug = c.arg("slug");
            let t = store.done(&slug, &c.actor()?, &c.arg("note"))?;
            c.say(&format!("{slug} is {}", t.status));
            finish::after_finish(store, c, &t);
        }
        "claim" => {
            let t = store.claim(&c.arg("slug"), &c.actor()?)?;
            c.say(&format!("claimed {}", t.slug));
        }
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
            store.take_role(&slug, &c.actor()?, &role)?;
            c.say(&format!("{role} on {slug}"));
        }
        "handover" => {
            let (slug, to) = (c.arg("slug"), c.arg("to"));
            store.hand_over(&slug, &c.actor()?, &to)?;
            c.say(&format!("handed {slug} to {to} (orchestrator)"));
        }
        "move" => {
            let slug = c.arg("slug");
            let t = store.move_to(&slug, &c.arg("status"), &c.actor()?)?;
            c.say(&format!("{slug} is {}", t.status));
            finish::after_move(store, c, &t);
        }
        "verdict" => {
            let slug = c.arg("slug");
            let t = store.verdict(&slug, &c.actor()?, &c.arg("verdict"), &c.arg("text"))?;
            c.say(&format!("{slug} is {}", t.status));
            finish::after_finish(store, c, &t);
        }
        "say" => {
            let m = store.say(&c.arg("slug"), &c.actor()?, &c.arg("text"), p.get("to"))?;
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
        "show" => show(store, c)?,
        "who" => who(store, c)?,
        "inbox" => {
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
        }
        "leave" => {
            let me = c.actor()?;
            store.leave(&me)?;
            c.say("left");
            trigger::trigger(c.env, &c.env.cwd, &format!("{me} left"), None);
        }
        "export" => {
            let named = p.or("out", EXPORT_PATH).to_string();
            let path = c.env.cwd.join(&named);
            let text = export_markdown(&store.tasks(None)?, store.now());
            let written = path
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|()| std::fs::write(&path, text));
            written.map_err(|e| Stop::Refused(format!("cannot write {named}: {e}")))?;
            c.say(&format!("wrote {named}"));
        }
        // The parser knows no other command (`beat` and `api` never come here).
        other => return Err(Stop::Refused(format!("no command {other}"))),
    }
    Ok(())
}

/// `create SLUG TITLE`: a new task from its arguments and its body (the `body` argument, a file, or
/// stdin for `-`), refused when the brief names the worktree of a dependency that has not landed
/// ([`imported_worktree`]).
fn create(store: &Store, c: &mut Context) -> Result<(), Stop> {
    let p = c.parsed;
    let body = match p.get("body_file") {
        Some("-") => {
            let mut body = String::new();
            c.input
                .read_to_string(&mut body)
                .map_err(|e| Stop::Refused(format!("cannot read the body from stdin: {e}")))?;
            body
        }
        Some(file) => std::fs::read_to_string(c.env.cwd.join(file))
            .map_err(|e| Stop::Refused(format!("cannot read the body file {file}: {e}")))?,
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
    let new = NewTask {
        slug: c.arg("slug"),
        title: c.arg("title"),
        by: c.actor()?,
        body,
        priority: p.int("priority", 2),
        status: p.or("stage", "ready").into(),
        parent: p.get("parent").map(str::to_string),
        links: p.all("link").to_vec(),
        depends: p.all("depends").to_vec(),
        assignee: p.get("assignee").map(str::to_string),
        instruction: p.flag("instruction"),
        // Reds are marked by what files them (`tasks api` `create`); work under one is red work.
        red: None,
    };
    let t = store.create(&new)?;
    c.say(&format!("created {} ({})", t.slug, t.status));
    Ok(())
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
    if c.parsed.flag("json") {
        let json = Py::obj(vec![
            ("task", shape::task(&task)),
            ("thread", shape::messages(&thread)),
            ("children", shape::tasks(&children)),
            ("stages", shape::stages(&stages)),
        ]);
        c.say(&pyjson::dumps(&json));
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
    c.say(&format!("  {}", stages::summary_line(&stages)));
    if !children.is_empty() {
        c.say("  subtasks:");
        for child in &children {
            let held = roles_text(child);
            let held = if held.is_empty() { "-".into() } else { held };
            c.say(&format!(
                "    {:<28} {:<11} {held}",
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
        let json = Py::obj(vec![(
            "sessions",
            Py::List(sessions.iter().map(shape::session).collect()),
        )]);
        c.say(&pyjson::dumps(&json));
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
    let names: Vec<&str> = sessions.iter().map(|s| s.row.handle.as_str()).collect();
    fn line(
        s: &Session,
        depth: usize,
        sessions: &[Session],
        held: &BTreeMap<String, Vec<String>>,
        c: &mut Context,
    ) {
        let r = &s.row;
        let holds = held.get(&r.handle).map(|h| h.join(" ")).unwrap_or_default();
        c.say(&format!(
            "{}{:<20} {:<12} {:<6} {:<24} {holds}",
            "  ".repeat(depth),
            r.handle,
            r.kind,
            s.health.as_str(),
            if r.branch.is_empty() { "-" } else { &r.branch },
        ));
        for child in sessions {
            if child.row.parent.as_deref() == Some(r.handle.as_str()) {
                line(child, depth + 1, sessions, held, c);
            }
        }
    }
    for s in &sessions {
        if !s.row.parent.as_deref().is_some_and(|p| names.contains(&p)) {
            line(s, 0, &sessions, &held, c);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::imported_worktree;
    use crate::sweep::git::Listed;
    use std::path::PathBuf;

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
