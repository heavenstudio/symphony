//! `tasks api`: the store's methods, called as JSON. One call `{"method": NAME, "args": {ARG: VALUE}}`,
//! or a list of them, as JSON on stdin; each answer is `{"ok": RESULT}`, `{"refused": WHY}` (the
//! store's `TaskError`) or `{"error": WHY}` (a call it cannot read, or a board it cannot reach), as
//! `json.dumps(indent=1)` writes it.
//!
//! `tasks api --lines` serves a call per line of stdin until it ends, each answer on a line of its
//! own as soon as it is made, over one connection: how the client keeps one process for a board.

use super::shape;
use crate::pyjson::{self, Py};
use crate::{Beat, Error, NewLaunch, NewSession, NewTask, Store};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use std::io::{BufRead, Read, Write};

/// What a caller of these methods may rely on beyond the schema and the server's RPC: one more each
/// time a method takes or does something an earlier build refused or did otherwise (1: `landed`'s
/// `from`, before which a build declares none). The live launcher (`tools/tasks.sh`) serves an
/// earlier main build only while this matches main's, so no caller reaches a build without what it
/// relies on.
#[expect(
    dead_code,
    reason = "tools/tasks.sh reads it from the committed source; no Rust code compares builds"
)]
pub const VERSION: i64 = 1;

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

/// Read the calls from `input`, answer each on `out`; exit 2 when the input is not a call at all.
pub fn run<F: FnOnce() -> Result<Store, String>>(
    input: &mut dyn Read,
    out: &mut dyn Write,
    connect: F,
) -> i32 {
    let mut board = Lazy {
        connect: Some(connect),
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
            Py::List(calls.into_iter().map(|c| answer(c, &mut board)).collect()),
            0,
        ),
        Ok(call) => (answer(call, &mut board), 0),
        Err(why) => (Py::obj(vec![("error", Py::Str(why))]), 2),
    };
    let _ = writeln!(out, "{}", pyjson::dumps(&answer));
    code
}

/// `tasks api --lines`: each line of `input` that is not blank is a call (or a list of them), answered
/// on a line of its own, until `input` ends.
pub fn serve<F: FnOnce() -> Result<Store, String>>(
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    connect: F,
) -> i32 {
    let mut board = Lazy {
        connect: Some(connect),
        store: None,
    };
    for line in input.lines() {
        let answer = match line {
            Err(e) => Py::obj(vec![(
                "error",
                Py::Str(format!("cannot read the call: {e}")),
            )]),
            Ok(line) if line.trim().is_empty() => continue,
            Ok(line) => match serde_json::from_str::<Value>(&line) {
                Ok(Value::Array(calls)) => {
                    Py::List(calls.into_iter().map(|c| answer(c, &mut board)).collect())
                }
                Ok(call) => answer(call, &mut board),
                Err(e) => Py::obj(vec![(
                    "error",
                    Py::Str(format!("the call is not JSON: {e}")),
                )]),
            },
        };
        if writeln!(out, "{}", pyjson::dumps_line(&answer))
            .and_then(|()| out.flush())
            .is_err()
        {
            break; // nobody reads the answers any more
        }
    }
    0
}

fn answer<F: FnOnce() -> Result<Store, String>>(call: Value, board: &mut Lazy<F>) -> Py {
    match one(call, board) {
        Ok(result) => Py::obj(vec![("ok", result)]),
        Err(Fail::Refused(why)) => Py::obj(vec![("refused", Py::Str(why))]),
        Err(Fail::Error(why)) => Py::obj(vec![("error", Py::Str(why))]),
    }
}

fn one<F: FnOnce() -> Result<Store, String>>(call: Value, board: &mut Lazy<F>) -> Result<Py, Fail> {
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

fn none(done: Result<(), Error>) -> Result<Py, Fail> {
    done.map(|()| Py::Null).map_err(Fail::from)
}

/// Each method: its arguments read and checked before the board is reached.
fn dispatch<F: FnOnce() -> Result<Store, String>>(
    a: &mut Args,
    board: &mut Lazy<F>,
) -> Result<Py, Fail> {
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
            Py::Bool(board.get()?.bind_agent(&agent, &handle, session)?)
        }
        "agent_beat" => {
            let agent: String = a.need("agent")?;
            let beat: Beat = a.rest()?;
            Py::opt_str(board.get()?.agent_beat(&agent, &beat)?.as_deref())
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
            Py::List(
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
            Py::opt_str(board.get()?.handle_for_agent(&agent)?.as_deref())
        }
        "bound_agents" => {
            args!();
            let bound = board.get()?.bound_agents()?;
            Py::Obj(
                bound
                    .into_iter()
                    .map(|(h, agent)| (h, Py::Str(agent)))
                    .collect(),
            )
        }
        // The agent sessions behind a task
        "record_launch" => {
            let new: NewLaunch = a.rest()?;
            super::check_actor(&new.handle)?;
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
            shape::task(&board.get()?.create(&new)?)
        }
        "ancestors" => {
            args!(slug: need);
            let slug: String = slug;
            Py::strs(&board.get()?.ancestors(&slug)?)
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
        "linked_to" => {
            args!(doc: need);
            let doc: String = doc;
            shape::tasks(&board.get()?.linked_to(&doc)?)
        }
        "claimable" => {
            args!(handle: need);
            let handle: String = handle;
            Py::strs(&board.get()?.claimable(&handle)?)
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
        "move" => {
            args!(slug: need, status: need, handle: actor);
            let (slug, status, handle): (String, String, String) = (slug, status, handle);
            shape::task(&board.get()?.move_to(&slug, &status, &handle)?)
        }
        "verdict" => {
            args!(slug: need, handle: actor, verdict: need, text: need);
            let (slug, handle, verdict, text): (String, String, String, String) =
                (slug, handle, verdict, text);
            shape::task(&board.get()?.verdict(&slug, &handle, &verdict, &text)?)
        }
        "done" => {
            args!(slug: need, handle: actor, note: need);
            let (slug, handle, note): (String, String, String) = (slug, handle, note);
            shape::task(&board.get()?.done(&slug, &handle, &note)?)
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
            Py::strs(&executed)
        }
        "next_for" => {
            args!(handle: need);
            let handle: String = handle;
            board
                .get()?
                .next_for(&handle)?
                .as_ref()
                .map_or(Py::Null, shape::task)
        }
        // Messages
        "say" => {
            args!(slug: need, sender: actor, text: need, to: take);
            let (slug, sender, text, to): (String, String, String, Option<String>) =
                (slug, sender, text, to);
            shape::message(&board.get()?.say(&slug, &sender, &text, to.as_deref())?)
        }
        "last_messages" => {
            args!();
            let last = board.get()?.last_messages()?;
            Py::Obj(
                last.into_iter()
                    .map(|(h, m)| (h, shape::message(&m)))
                    .collect(),
            )
        }
        "landings_posted" => {
            args!();
            let posted: Vec<String> = board.get()?.landings_posted()?.into_iter().collect();
            Py::strs(&posted)
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
            Py::List(
                slugs
                    .iter()
                    .map(|slug| shape::messages(&threads[slug]))
                    .collect(),
            )
        }
        "snapshot" => {
            args!();
            let snap = board.get()?.board()?;
            Py::obj(vec![
                ("tasks", shape::tasks(&snap.tasks)),
                (
                    "sessions",
                    Py::List(snap.sessions.iter().map(shape::session).collect()),
                ),
                ("latest_message", Py::Int(snap.latest_message)),
            ])
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
        _ => return Err(Fail::Error(format!("the board has no method {method}"))),
    })
}
