//! `tasks api`: the store's methods, called as JSON. One call `{"method": NAME, "args": {ARG: VALUE}}`,
//! or a list of them, as JSON on stdin; each answer is `{"ok": RESULT}`, `{"refused": WHY}` (the
//! store's `TaskError`) or `{"error": WHY}` (a call it cannot read, or a board it cannot reach), as
//! pretty-printed JSON.
//!
//! `tasks api --lines` serves a call per line of stdin until it ends, each answer on a line of its
//! own as soon as it is made, over one connection: how the client keeps one process for a board.
//!
//! It is a contract, versioned `MAJOR.MINOR`: `{"method": "version"}` answers
//! `{"api": {"major": M, "minor": N}, "schema": S}`, S the board schema this build writes, without
//! reaching the board. A program that must know what a build serves (a project's launcher choosing
//! a build, a release checking one) asks it, never this file. Within a major, a method, an argument
//! or a field of an answer is added, never removed, renamed or changed in meaning, and an argument
//! added has a default, its absence read as before; each addition raises the minor. A new major is
//! a deliberate act, a task of its own. `tests/cli/api/contract.rs` holds the calls a project's own
//! programs make: each request, and the fields of its answer the caller reads, with their JSON
//! types.

use super::shape;
use crate::launch::Bounds;
use crate::{Beat, Error, NewAsk, NewLaunch, NewSession, NewTask, Store};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};
use std::io::{BufRead, Read, Write};

mod assessment;
#[path = "api/assessment/context.rs"]
mod context;
#[cfg_attr(
    not(test),
    expect(unused_imports, reason = "preserve connect-only compatibility entries")
)]
pub use context::{run, serve};

/// The api's major: version 2 refuses the retired `create.fast_fix` argument.
pub const MAJOR: i64 = 2;

/// The api's minor within [`MAJOR`]: 1 added a task's `id`, `bound`, `edit_bound` and
/// `integration_state`; 2 added `evidence`; 3 added `import` to its `assessment`, where the
/// assessment names one; 4 added `workflow_read`, `workflow_actions` and `workflow_apply`; 5 added
/// `attempts`; 6 added `workflow_apply`'s `attestations` and `workflow_observe`.
pub const MINOR: i64 = 6;

/// Why a call has no result.
enum Fail {
    Refused(String),
    Error(String),
}

impl From<Error> for Fail {
    fn from(error: Error) -> Fail {
        match error {
            Error::Refused(why) => Fail::Refused(why),
            Error::Db(_) => Fail::Error(error.to_string()),
        }
    }
}

/// The board, reached the first time a call needs it: a call it cannot read is answered without it.
struct Lazy<F: FnOnce() -> Result<Store, String>> {
    connect: Option<F>,
    context: Option<super::Env>,
    store: Option<Result<Store, String>>,
}

impl<F: FnOnce() -> Result<Store, String>> Lazy<F> {
    fn get(&mut self) -> Result<&Store, Fail> {
        if self.store.is_none() {
            let connect = self.connect.take().expect("connect runs once");
            self.store = Some(connect());
        }
        match self.store.as_ref().expect("connected above") {
            Ok(store) => Ok(store),
            Err(why) => Err(Fail::Error(why.clone())),
        }
    }
}

/// A call's arguments by name, each taken once; any left over is refused by name.
struct Args {
    method: String,
    given: Map<String, Value>,
}

impl Args {
    fn take<T: DeserializeOwned>(&mut self, name: &str) -> Result<Option<T>, Fail> {
        match self.given.remove(name) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => serde_json::from_value(value)
                .map(Some)
                .map_err(|e| Fail::Error(format!("{}: argument {name}: {e}", self.method))),
        }
    }

    fn need<T: DeserializeOwned>(&mut self, name: &str) -> Result<T, Fail> {
        self.take(name)?.ok_or_else(|| {
            Fail::Error(format!(
                "{}: no argument {name}, which it needs",
                self.method
            ))
        })
    }

    fn actor(&mut self, name: &str) -> Result<String, Fail> {
        let actor = self.need::<String>(name)?;
        super::check_actor(&actor)?;
        Ok(actor)
    }

    fn or<T: DeserializeOwned>(&mut self, name: &str, default: T) -> Result<T, Fail> {
        Ok(self.take(name)?.unwrap_or(default))
    }

    /// No argument is left that the method does not take.
    fn end(&self) -> Result<(), Fail> {
        match self.given.keys().next() {
            Some(name) => Err(Fail::Error(format!(
                "{}: no argument named {name}",
                self.method
            ))),
            None => Ok(()),
        }
    }

    /// The rest of the arguments, as the store's own struct of them.
    fn rest<T: DeserializeOwned>(&mut self) -> Result<T, Fail> {
        let rest = std::mem::take(&mut self.given);
        serde_json::from_value(Value::Object(rest))
            .map_err(|e| Fail::Error(format!("{}: {e}", self.method)))
    }
}

pub(super) fn run_in<F: FnOnce() -> Result<Store, String>>(
    input: &mut dyn Read,
    out: &mut dyn Write,
    connect: F,
    context: Option<&super::Env>,
) -> i32 {
    let mut board = Lazy {
        connect: Some(connect),
        context: context.cloned(),
        store: None,
    };
    let mut text = String::new();
    let calls = input
        .read_to_string(&mut text)
        .map_err(|e| format!("cannot read the call: {e}"))
        .and_then(|_| {
            serde_json::from_str::<Value>(&text).map_err(|e| format!("the call is not JSON: {e}"))
        });
    let (answer, code) = match calls {
        Ok(Value::Array(calls)) => (
            Value::Array(calls.into_iter().map(|c| answer(c, &mut board)).collect()),
            0,
        ),
        Ok(call) => (answer(call, &mut board), 0),
        Err(why) => (
            json!({
                "error": why,
            }),
            2,
        ),
    };
    let _ = writeln!(
        out,
        "{}",
        serde_json::to_string_pretty(&answer).expect("JSON values serialize")
    );
    code
}

pub(super) fn serve_in<F: FnOnce() -> Result<Store, String>>(
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    connect: F,
    context: Option<&super::Env>,
) -> i32 {
    let mut board = Lazy {
        connect: Some(connect),
        context: context.cloned(),
        store: None,
    };
    for line in input.lines() {
        let answer = match line {
            Err(e) => json!({
                "error": format!("cannot read the call: {e}"),
            }),
            Ok(line) if line.trim().is_empty() => continue,
            Ok(line) => match serde_json::from_str::<Value>(&line) {
                Ok(Value::Array(calls)) => {
                    Value::Array(calls.into_iter().map(|c| answer(c, &mut board)).collect())
                }
                Ok(call) => answer(call, &mut board),
                Err(e) => json!({
                    "error": format!("the call is not JSON: {e}"),
                }),
            },
        };
        if writeln!(
            out,
            "{}",
            serde_json::to_string(&answer).expect("JSON values serialize")
        )
        .and_then(|()| out.flush())
        .is_err()
        {
            break; // nobody reads the answers any more
        }
    }
    0
}

fn answer<F: FnOnce() -> Result<Store, String>>(call: Value, board: &mut Lazy<F>) -> Value {
    match one(call, board) {
        Ok(result) => json!({
            "ok": result,
        }),
        Err(Fail::Refused(why)) => json!({
            "refused": why,
        }),
        Err(Fail::Error(why)) => json!({
            "error": why,
        }),
    }
}

fn one<F: FnOnce() -> Result<Store, String>>(
    call: Value,
    board: &mut Lazy<F>,
) -> Result<Value, Fail> {
    let Value::Object(mut call) = call else {
        return Err(Fail::Error(format!("a call is an object, not {call}")));
    };
    let method = match call.remove("method") {
        Some(Value::String(method)) => method,
        _ => return Err(Fail::Error("a call names its method".into())),
    };
    let given = match call.remove("args") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(args)) => args,
        Some(other) => {
            return Err(Fail::Error(format!(
                "{method}: args is {other}, not an object"
            )));
        }
    };
    if let Some(name) = call.keys().next() {
        return Err(Fail::Error(format!("a call has no field named {name}")));
    }
    let mut a = Args { method, given };
    let result = dispatch(&mut a, board)?;
    Ok(result)
}

/// The id of the project `context`'s working directory is in, as the machine's registry names it:
/// the scope of its board's attempts ([`crate::execution::board_scope`]).
fn project_of(context: Option<&super::Env>) -> Result<String, Fail> {
    let context =
        context.ok_or_else(|| Fail::Refused("attempts are read in a project's checkout".into()))?;
    let machine = crate::project::machine_folder(
        |name| context.var(name).map(std::ffi::OsString::from),
        cfg!(target_os = "macos"),
    )
    .ok();
    crate::project::find(&context.cwd, machine.as_deref())
        .map(|project| project.id)
        .map_err(Fail::Refused)
}

/// Task `slug`'s attempts, oldest first, read as the board's own service: what a project's program
/// reads through `tasks api`, its controls left to the host that holds the provider.
fn attempts(store: &Store, project: &str, slug: &str) -> Result<symphony_api::TaskAttempts, Fail> {
    use crate::execution::{Act, Grant, Reader, Refusal, board_scope};
    let scope = board_scope(project, store);
    let grant = Grant::from_host(
        crate::workflow::board_service("tasks-api"),
        scope.clone(),
        &[Act::Read],
        "tasks-api/1",
    )
    .on_tasks(&[slug]);
    let read = Reader::new(store, scope).history(&grant, slug);
    let attempts = read.map_err(|refusal| match refusal {
        Refusal::Board(error) => Fail::from(error),
        other => Fail::Refused(other.to_string()),
    })?;
    Ok(symphony_api::TaskAttempts {
        attempts: attempts
            .iter()
            .map(crate::execution::Attempt::wire)
            .collect(),
    })
}

fn none(done: Result<(), Error>) -> Result<Value, Fail> {
    done.map(|()| Value::Null).map_err(Fail::from)
}

/// What a block waits on, as `move` and `hold` take it (`on`, `why`): none when neither is given,
/// as before minor 15, which records nothing; a `why` alone is refused, naming nothing to give
/// it to. The store checks the thing and the words ([`crate::Store::move_on`]).
fn blocked_on<'a>(
    on: Option<&'a str>,
    why: Option<&'a str>,
) -> Result<Option<crate::On<'a>>, Fail> {
    match (on, why) {
        (Some(thing), why) => Ok(Some(crate::On { thing, why })),
        (None, None) => Ok(None),
        (None, Some(_)) => Err(Fail::Refused(format!(
            "a block's why goes with what it waits on (`on`, one of {})",
            crate::waits_on::THINGS
        ))),
    }
}

/// Each method: its arguments read and checked before the board is reached.
fn dispatch<F: FnOnce() -> Result<Store, String>>(
    a: &mut Args,
    board: &mut Lazy<F>,
) -> Result<Value, Fail> {
    let method = a.method.clone();
    macro_rules! args {
        ($($name:ident: $how:ident $(($default:expr))?),* $(,)?) => {
            $(let $name = args!(@ $name $how $($default)?);)*
            a.end()?;
        };
        (@ $name:ident need) => { a.need(stringify!($name))? };
        (@ $name:ident actor) => { a.actor(stringify!($name))? };
        (@ $name:ident take) => { a.take(stringify!($name))? };
        (@ $name:ident or $default:expr) => { a.or(stringify!($name), $default)? };
    }
    Ok(match method.as_str() {
        // This build's, so the board is not reached.
        "version" => {
            args!();
            json!({
                "api": {
                    "major": MAJOR,
                    "minor": MINOR,
                },
                "schema": crate::schema::VERSION,
            })
        }
        "assessment_state" | "record_waits" => return assessment::call(a, board),
        // Sessions
        "register" => {
            let new: NewSession = a.rest()?;
            shape::session(&board.get()?.register(&new)?)
        }
        "touch" => {
            args!(handle: actor);
            let handle: String = handle;
            return none(board.get()?.touch(&handle));
        }
        "bind_agent" => {
            args!(agent: need, handle: actor, session: or(false));
            let (agent, handle): (String, String) = (agent, handle);
            // Through `just tasks`, which an agent runs unasked: no binding, whose id staffing may
            // record as the handle's conversation ([`crate::launch::may_bind`]).
            if let Some(env) = &board.context {
                crate::launch::may_bind(env).map_err(Fail::Refused)?;
            }
            json!(board.get()?.bind_agent(&agent, &handle, session)?)
        }
        "agent_beat" => {
            let agent: String = a.need("agent")?;
            let beat: Beat = a.rest()?;
            json!(board.get()?.agent_beat(&agent, &beat)?.as_deref())
        }
        "reattach" => {
            args!(handle: actor, pid: take);
            let handle: String = handle;
            shape::session(&board.get()?.reattach(&handle, pid)?)
        }
        "revive" | "leave" => {
            args!(handle: actor);
            let (handle, store): (String, _) = (handle, board.get()?);
            return none(if method == "revive" {
                store.revive(&handle)
            } else {
                store.leave(&handle)
            });
        }
        "session" => {
            args!(handle: need);
            let handle: String = handle;
            shape::session(&board.get()?.session(&handle)?)
        }
        "sessions" => {
            args!();
            Value::Array(
                board
                    .get()?
                    .sessions()?
                    .iter()
                    .map(shape::session)
                    .collect(),
            )
        }
        "handle_for_agent" => {
            args!(agent: need);
            let agent: String = agent;
            json!(board.get()?.handle_for_agent(&agent)?.as_deref())
        }
        "bound_agents" => {
            args!();
            let bound = board.get()?.bound_agents()?;
            Value::Object(
                bound
                    .into_iter()
                    .map(|(h, agent)| (h, json!(agent)))
                    .collect(),
            )
        }
        // The agent sessions behind a task
        "record_launch" => {
            let new: NewLaunch = a.rest()?;
            super::check_actor(&new.handle)?;
            // Through `just tasks`, which an agent runs unasked: no record a restore would start
            // outside the project in, nor one naming a conversation the board did not record.
            if let Some(bounds) = board.context.as_ref().and_then(Bounds::unasked) {
                bounds.record(&new).map_err(Fail::Refused)?;
            }
            shape::launch(&board.get()?.record_launch(&new)?)
        }
        "task_sessions" => {
            args!(slug: need);
            let slug: String = slug;
            shape::launches(&board.get()?.task_sessions(&slug)?)
        }
        "sessions_of" => {
            args!(handle: need);
            let handle: String = handle;
            shape::launches(&board.get()?.sessions_of(&handle)?)
        }
        "all_task_sessions" => {
            args!();
            shape::launches(&board.get()?.all_task_sessions()?)
        }
        "open_task_sessions" => {
            args!();
            shape::launches(&board.get()?.open_task_sessions()?)
        }
        "close_task_session" => {
            args!(slug: need, handle: actor, why: need, restore: need);
            let (slug, handle, why, restore): (String, String, String, String) =
                (slug, handle, why, restore);
            return none(
                board
                    .get()?
                    .close_task_session(&slug, &handle, &why, &restore),
            );
        }
        "reopen_task_session" => {
            args!(slug: need, handle: actor, pane: need);
            let (slug, handle, pane): (String, String, String) = (slug, handle, pane);
            return none(board.get()?.reopen_task_session(&slug, &handle, &pane));
        }
        // Tasks
        "create" => {
            let new: NewTask = a.rest()?;
            super::check_actor(&new.by)?;
            let store = board.get()?;
            let made = store.create(&new)?;
            // Every open task has an orchestrator: filed, it stays filed when no give is written,
            // and the board's next read of the whole board gives it.
            match store.give_filed(&made.slug, &new.by, None) {
                Ok(_) => shape::task(&store.get(&made.slug)?),
                Err(error) => {
                    eprintln!("{}: no orchestrator was given it yet: {error}", made.slug);
                    shape::task(&made)
                }
            }
        }
        "amend" => {
            args!(slug: need, title: need, links: or(Vec::<String>::new()), by: actor);
            let (slug, title, by): (String, String, String) = (slug, title, by);
            let links: Vec<String> = links;
            shape::task(&board.get()?.amend(&slug, &title, &links, &by)?)
        }
        // What a program outside the board keeps of a task, and writes it again by.
        "bound" => {
            args!(slug: need);
            let slug: String = slug;
            json!(board.get()?.bound(&slug)?)
        }
        "edit_bound" => {
            let slug: String = a.need("slug")?;
            let bound: crate::recovery::Bound = a.need("bound")?;
            let by = a.actor("by")?;
            let edit: crate::recovery::Edit = a.rest()?;
            json!(board.get()?.edit_bound(&slug, &bound, &edit, &by)?)
        }
        // This project's, so the board is not reached.
        "integration_state" => {
            args!(name: need);
            let name: String = name;
            let Some(env) = &board.context else {
                return Err(Fail::Error(format!(
                    "{method}: this entry was given no folder to find the project from"
                )));
            };
            let place = crate::project::locate(&env.cwd).map_err(Fail::Error)?;
            let path = crate::folders::integration(&place.common, &name).map_err(Fail::Refused)?;
            json!({"path": path})
        }
        // Read-only: the task's evidence at its branch's head, as the land reads it.
        "evidence" => {
            args!(slug: need, head: take);
            let (slug, head): (String, Option<String>) = (slug, head);
            let context = board.context.clone().ok_or_else(|| {
                Fail::Error("evidence: it is read from a checkout of the task's repository".into())
            })?;
            let snapshot =
                crate::land::evidence::of(board.get()?, &context, &slug, head.as_deref())
                    .map_err(Fail::Refused)?;
            json!(&snapshot)
        }
        "mark_red" => {
            args!(slug: need, red: need, by: actor);
            let (slug, red, by): (String, String, String) = (slug, red, by);
            shape::task(&board.get()?.mark_red(&slug, &red, &by)?)
        }
        "unmark_red" => {
            args!(slug: need, by: actor);
            let (slug, by): (String, String) = (slug, by);
            shape::task(&board.get()?.unmark_red(&slug, &by)?)
        }
        "ancestors" => {
            args!(slug: need);
            let slug: String = slug;
            json!(&board.get()?.ancestors(&slug)?)
        }
        "children" => {
            args!(slug: need);
            let slug: String = slug;
            shape::tasks(&board.get()?.children(&slug)?)
        }
        "descendants" => {
            args!(slug: need);
            let slug: String = slug;
            shape::tasks(&board.get()?.descendants(&slug)?)
        }
        "set_parent" => {
            args!(slug: need, parent: take, by: actor);
            let (slug, parent, by): (String, Option<String>, String) = (slug, parent, by);
            shape::task(&board.get()?.set_parent(&slug, parent.as_deref(), &by)?)
        }
        "get" => {
            args!(slug: need);
            let slug: String = slug;
            shape::task(&board.get()?.get(&slug)?)
        }
        "tasks" => {
            args!(status: take);
            let status: Option<String> = status;
            shape::tasks(&board.get()?.tasks(status.as_deref())?)
        }
        "tasks_for_branch" => {
            args!(branch: need);
            let branch: String = branch;
            shape::tasks(&board.get()?.tasks_for_branch(&branch)?)
        }
        "linked_to" => {
            args!(doc: need);
            let doc: String = doc;
            shape::tasks(&board.get()?.linked_to(&doc)?)
        }
        "claimable" => {
            args!(handle: need);
            let handle: String = handle;
            json!(&board.get()?.claimable(&handle)?)
        }
        "claim" => {
            args!(slug: need, handle: actor);
            let (slug, handle): (String, String) = (slug, handle);
            shape::task(&board.get()?.claim(&slug, &handle)?)
        }
        "reserve" => {
            args!(slug: need, handle: actor, ttl: or(crate::health::RESERVE_SECONDS));
            let (slug, handle): (String, String) = (slug, handle);
            shape::task(&board.get()?.reserve(&slug, &handle, ttl)?)
        }
        "take_role" => {
            args!(slug: need, handle: actor, role: need);
            let (slug, handle, role): (String, String, String) = (slug, handle, role);
            shape::task(&board.get()?.take_role(&slug, &handle, &role)?)
        }
        "release" => {
            args!(slug: need, handle: actor, role: take);
            let (slug, handle, role): (String, String, Option<String>) = (slug, handle, role);
            shape::task(&board.get()?.release(&slug, &handle, role.as_deref())?)
        }
        // A custom workflow (docs/WORKFLOW.md): an agent session's reads and operations, the
        // principal the session the call names, as the local host binds it.
        "workflow_read" => {
            args!();
            json!(board.get()?.workflow_read()?)
        }
        "workflow_actions" => {
            args!(slug: need, handle: actor);
            let (slug, handle): (String, String) = (slug, handle);
            let principal = crate::workflow::agent_session(&handle);
            json!(board.get()?.workflow_actions(
                &slug,
                &principal,
                &crate::workflow::Host::local()
            )?)
        }
        // A guarded edge's `attestations`, each its project authority's signed word, none by
        // default; `workflow_observe` hands the board that authority's signed observation.
        "workflow_apply" => {
            args!(handle: actor, request: need, attestations: take);
            let (handle, request): (String, symphony_api::workflow::OperationRequest) =
                (handle, request);
            let attestations: Option<Vec<symphony_api::workflow::Signed>> = attestations;
            let principal = crate::workflow::agent_session(&handle);
            let store = board.get()?;
            json!(store.apply_attested(
                &principal,
                &crate::workflow::Host::local(),
                &request,
                attestations.as_deref().unwrap_or_default()
            )?)
        }
        "workflow_observe" => {
            args!(signed: need);
            let signed: symphony_api::workflow::Signed = signed;
            board.get()?.observe_authority(&signed)?.answer()
        }
        // A task's execution attempts (docs/EXECUTION_ATTEMPTS.md), read as the board's own
        // service under its scope: the project the call runs in, and the board's database.
        "attempts" => {
            args!(slug: need);
            let slug: String = slug;
            let project = project_of(board.context.as_ref())?;
            let store = board.get()?;
            store.get(&slug)?;
            json!(attempts(store, &project, &slug)?)
        }
        "move" => {
            args!(slug: need, status: need, handle: actor, from: take, reason: take, on: take, why: take);
            let (slug, status, handle): (String, String, String) = (slug, status, handle);
            let from: Option<Vec<String>> = from;
            let (reason, on, why): (Option<String>, Option<String>, Option<String>) =
                (reason, on, why);
            let context = board.context.clone();
            let store = board.get()?;
            if status == "review" {
                crate::land::reopenable(store, context.as_ref(), &slug)?;
            }
            let on = blocked_on(on.as_deref(), why.as_deref())?;
            shape::task(&match from {
                Some(_) if reason.is_some() => {
                    return Err(Fail::Refused(
                        "a move from named statuses takes no reason".into(),
                    ));
                }
                Some(_) if on.is_some() => {
                    return Err(Fail::Refused(
                        "a move from named statuses names no block's thing".into(),
                    ));
                }
                Some(from) => {
                    let from: Vec<&str> = from.iter().map(String::as_str).collect();
                    store.move_from(&slug, &from, &status, &handle)?
                }
                None => store.move_on(&slug, &status, &handle, reason.as_deref(), on)?,
            })
        }
        "hold" => {
            args!(slug: need, handle: actor, on: take, why: take);
            let (slug, handle, on, why): (String, String, Option<String>, Option<String>) =
                (slug, handle, on, why);
            let on = blocked_on(on.as_deref(), why.as_deref())?;
            shape::task(&board.get()?.hold_blocked_on(&slug, &handle, on)?)
        }
        "verdict" => {
            args!(slug: need, handle: actor, verdict: need, text: need);
            let (slug, handle, verdict, text): (String, String, String, String) =
                (slug, handle, verdict, text);
            shape::task(&board.get()?.verdict(&slug, &handle, &verdict, &text)?)
        }
        "done" => {
            args!(slug: need, handle: actor, note: need, covered_by: take);
            let (slug, handle, note, covered_by): (String, String, String, Option<String>) =
                (slug, handle, note, covered_by);
            let context = board.context.clone();
            let store = board.get()?;
            shape::task(&match covered_by {
                Some(other) => {
                    crate::covering::close(store, context.as_ref(), &slug, &other, &handle, &note)?
                }
                None => store.done(&slug, &handle, &note)?,
            })
        }
        "landed" => {
            let closes = ["review", "blocked"].map(String::from).to_vec();
            args!(slug: need, handle: actor, text: need, requested: or(false), from: or(closes));
            let (slug, handle, text, from): (String, String, String, Vec<String>) =
                (slug, handle, text, from);
            let from: Vec<&str> = from.iter().map(String::as_str).collect();
            shape::landed(
                &board
                    .get()?
                    .landed(&slug, &handle, &text, requested, &from)?,
            )
        }
        "ever_executed" => {
            args!(slug: need);
            let slug: String = slug;
            let executed: Vec<String> = board.get()?.ever_executed(&slug)?.into_iter().collect();
            json!(&executed)
        }
        "next_for" => {
            args!(handle: need);
            let handle: String = handle;
            board
                .get()?
                .next_for(&handle)?
                .as_ref()
                .map_or(Value::Null, shape::task)
        }
        // Messages
        "say" => {
            args!(slug: need, sender: actor, text: need, to: take, key: take, adopt: or(false));
            let (slug, sender, text): (String, String, String) = (slug, sender, text);
            let (to, key): (Option<String>, Option<String>) = (to, key);
            let adopt: bool = adopt;
            let board = board.get()?;
            let said = match key.as_deref() {
                Some(key) if adopt => {
                    board.adopt_or_say(&slug, &sender, &text, to.as_deref(), key)?
                }
                Some(key) => board.say_keyed(&slug, &sender, &text, to.as_deref(), key)?,
                None => board.say(&slug, &sender, &text, to.as_deref())?,
            };
            shape::message(&said)
        }
        "notify" => {
            args!(slug: need, actor: actor, handle: need, text: need);
            let (slug, actor, handle, text): (String, String, String, String) =
                (slug, actor, handle, text);
            shape::message(&board.get()?.notify(&slug, &actor, &handle, &text)?)
        }
        "last_messages" => {
            args!();
            let last = board.get()?.last_messages()?;
            Value::Object(
                last.into_iter()
                    .map(|(h, m)| (h, shape::message(&m)))
                    .collect(),
            )
        }
        "landings_posted" => {
            args!();
            let posted: Vec<String> = board.get()?.landings_posted()?.into_iter().collect();
            json!(&posted)
        }
        "thread" => {
            args!(slug: need);
            let slug: String = slug;
            shape::messages(&board.get()?.thread(&slug)?)
        }
        "threads" => {
            args!(slugs: need);
            let slugs: Vec<String> = slugs;
            let threads = board.get()?.threads(&slugs)?;
            Value::Array(
                slugs
                    .iter()
                    .map(|slug| shape::messages(&threads[slug]))
                    .collect(),
            )
        }
        "in_status_since" => {
            args!(slugs: need);
            let slugs: Vec<String> = slugs;
            let since = board.get()?.in_status_since(&slugs)?;
            shape::keyed(since.into_iter().map(|(slug, since)| (slug, json!(since))))
        }
        "snapshot" => {
            args!();
            let snap = board.get()?.board()?;
            json!({
                "tasks": shape::tasks(&snap.tasks),
                "sessions": Value::Array(
                    snap.sessions.iter().map(shape::session).collect(),
                ),
                "latest_message": snap.latest_message,
            })
        }
        "inbox" => {
            args!(handle: need);
            let handle: String = handle;
            shape::messages(&board.get()?.inbox(&handle)?)
        }
        "ack" => {
            args!(handle: actor, ids: need);
            let (handle, ids): (String, Vec<i64>) = (handle, ids);
            return none(board.get()?.ack(&handle, &ids));
        }
        "to_owner" => {
            args!(limit: or(40usize));
            shape::messages(&board.get()?.to_owner(limit)?)
        }
        // Asks: what the owner waits on an agent for
        "open_ask" => {
            let new: NewAsk = a.rest()?;
            let store = board.get()?;
            shape::ask(&store.open_ask(&new)?, store.now())
        }
        "ask" => {
            args!(id: need);
            let store = board.get()?;
            shape::ask(&store.ask(id)?, store.now())
        }
        "asks" => {
            args!(since: take);
            let store = board.get()?;
            let now = store.now();
            Value::Array(
                store
                    .asks(since)?
                    .iter()
                    .map(|ask| shape::ask(ask, now))
                    .collect(),
            )
        }
        "hold_ask" => {
            args!(id: need, holder: take, why: need);
            let (holder, why): (Option<String>, String) = (holder, why);
            let store = board.get()?;
            shape::ask(&store.hold_ask(id, holder.as_deref(), &why)?, store.now())
        }
        "answer_ask" => {
            args!(id: need, by: take, answer: need);
            let (by, answer): (Option<String>, String) = (by, answer);
            if let Some(by) = &by {
                super::check_actor(by)?;
            }
            let store = board.get()?;
            shape::ask(&store.answer_ask(id, by.as_deref(), &answer)?, store.now())
        }
        "answer_about" => {
            args!(
                kind: need, path: take, card: take, block: take, tree: take, task: take,
                thread: take, by: take, answer: need,
            );
            let about = NewAsk {
                kind,
                path,
                card,
                block,
                tree,
                task,
                thread,
                ..NewAsk::default()
            };
            let (by, answer): (Option<String>, String) = (by, answer);
            if let Some(by) = &by {
                super::check_actor(by)?;
            }
            let store = board.get()?;
            let now = store.now();
            store
                .answer_about(&about, by.as_deref(), &answer)?
                .map_or(Value::Null, |ask| shape::ask(&ask, now))
        }
        "close_ask" => {
            args!(id: need, by: actor, why: need);
            let (by, why): (String, String) = (by, why);
            let store = board.get()?;
            shape::ask(&store.close_ask(id, &by, &why)?, store.now())
        }
        _ => return Err(Fail::Error(format!("the board has no method {method}"))),
    })
}
