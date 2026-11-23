//! The board's recovery generation, and the writes bound to it.
//!
//! A task's number and version name it only along one history of the board: a copy of the whole
//! board from an older dump keeps the numbers the dump held, gives the ones made since to other
//! tasks and starts every version again. So what a program outside the board keeps of a task (a
//! mapping, the base of an edit) is a [`Bound`]: the task's number and version with the board's
//! recovery generation, a text the database makes and replaces in the transaction of every copy
//! ([`Store::copy`]) and of every rebind ([`Store::rebind`]). A bound write ([`Store::amend_bound`],
//! [`Store::edit_bound`]) checks all three inside its transaction, before anything it writes, and a mismatch is a
//! refusal, never read again and tried on what the board holds now.
//!
//! The generation is a row of the database, so putting the database's files back puts it back
//! too. What tells that apart is an [`Authority`] outside the database's data: it names the
//! generation the board is bound to, or none. A store is handed one ([`Store::recovering`]) and
//! finds none by itself; with none, or one that names no generation or another than the board's,
//! every bound call is refused. The supported order of a managed physical restore is: revoke the
//! binding, durably, before the restored data is served; then rebind, which replaces the
//! generation in one transaction and only then publishes it. A rebind never adopts a generation
//! it found, so one interrupted anywhere leaves the board refusing, and is run again.
//!
//! What this does not tell apart, and no check here claims to: a database put back without the
//! revoke, where no copy came between; and a database and its authority both put back to an
//! older pair that agree.

use super::postgres::recovery as pg;
use super::statement::Statement;
use super::work::{OWNER, no_task};
use super::{Error, REFUSED, Store, refused};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

/// The version of [`Bound`]'s shape and of the check a bound write makes of it: a token of
/// another is refused, never read as this one.
pub const COMPAT: u32 = 1;

/// What a program keeps of a task to write it again later: its number and version, and the
/// recovery generation of the board they were read on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bound {
    pub compat: u32,
    pub generation: String,
    /// The task's number (`Task::id`), which names it only on the board of `generation`.
    pub id: i64,
    pub version: i64,
}

/// A task's content with the token it was read under, from one statement.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundTask {
    pub bound: Bound,
    pub title: String,
    pub body: String,
}

/// What a bound edit ([`Store::edit_bound`]) changes of a task: what is left out stays as it is.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    /// The title it takes, when one is given.
    #[serde(default)]
    pub title: Option<String>,
    /// The brief it takes, whole, when one is given.
    #[serde(default)]
    pub body: Option<String>,
    /// The documents it is linked to as well; one it already has stays one link.
    #[serde(default)]
    pub links: Vec<String>,
}

/// What names, outside the database's own data, the recovery generation a board is bound to.
///
/// One for a board on this machine is a file in its home ([`crate::server::Home::recovery`]). A
/// board several machines write needs one they all read, which its deployer supplies: a file on
/// one of them is not it.
pub trait Authority: Send + Sync {
    /// The generation the board is bound to: none when it was revoked or never bound. An
    /// authority that cannot be read is an error, and refuses as none does.
    fn bound(&self) -> Result<Option<String>, String>;
    /// Bind the board to no generation, durably before this returns.
    fn revoke(&self) -> Result<(), String>;
    /// Bind the board to `generation`, durably before this returns.
    fn publish(&self, generation: &str) -> Result<(), String>;
}

/// What a bound call is refused with on a store handed no authority.
const NO_AUTHORITY: &str = "this board was opened with no recovery authority, so nothing here can \
    tell a restored board from the one a token was read on: a write bound to a recovery \
    generation is refused in this environment, and nothing was written";

/// What a bound call is refused with while the authority names no generation.
const UNBOUND: &str = "this board is bound to no recovery generation (never bound, or revoked for \
    a restore): nothing was written; `just tasks recovery rebind` binds it once the board is the \
    one to keep";

/// What a bound call is refused with when the board's generation is not the token's.
const RESTORED: &str = "the board was copied or restored since this was read, and the task's \
    number and version may name another now: nothing was written; read the task again";

/// What a bound write is refused with when the task moved on since its token was read.
const STALE: &str = "the task changed since this was read: nothing was written; read the task \
    again";

#[derive(Deserialize)]
struct Read {
    generation: Option<String>,
    id: i64,
    version: i64,
    title: String,
    body: String,
}

impl Store {
    /// This store, its bound calls answering to `authority` from now on.
    pub fn recovering(mut self, authority: Arc<dyn Authority>) -> Store {
        self.authority = Some(authority);
        self
    }

    /// The board's own recovery generation, whatever an authority names: what `tasks recovery
    /// status` says. None on a board that holds none.
    pub fn recovery_generation(&self) -> Result<Option<String>, Error> {
        self.retrying(|| self.read(Statement::from(pg::generation()), json!({})))
    }

    /// The generation this store's authority binds the board to, or the refusal of a store with
    /// no authority or an authority that names none.
    fn authorized(&self) -> Result<String, Error> {
        let authority = self
            .authority
            .as_ref()
            .ok_or_else(|| refused(NO_AUTHORITY.into()))?;
        match authority.bound() {
            Ok(Some(generation)) => Ok(generation),
            Ok(None) => Err(refused(UNBOUND.into())),
            Err(why) => Err(refused(format!(
                "the board's recovery authority could not be read ({why}): nothing was written"
            ))),
        }
    }

    /// The variables a bound write's fence reads (`postgres::recovery::FENCED`), for the task
    /// `slug` at `expected`: refused before anything is sent when the token is of another shape,
    /// the store has no authority, or the authority binds the board to none or to another
    /// generation than the token's. The fence then holds the board to that generation in the
    /// write's transaction.
    pub(crate) fn bound_vars(&self, slug: &str, expected: &Bound) -> Result<Value, Error> {
        if expected.compat != COMPAT {
            return Err(refused(format!(
                "this token is of compatibility version {}, and this build checks version \
                 {COMPAT}: nothing was written; read the task again with this build",
                expected.compat
            )));
        }
        if self.authorized()? != expected.generation {
            return Err(refused(RESTORED.into()));
        }
        let Error::Refused(missing) = no_task(slug) else {
            unreachable!("no task is a refusal");
        };
        Ok(json!({
            "slug": slug,
            "bound": expected,
            "missing": format!("{REFUSED}{missing}"),
            "bound_restored": format!("{REFUSED}{RESTORED}"),
            "bound_stale": format!("{REFUSED}{STALE}"),
        }))
    }

    /// `row`, a task read with the board's generation, as its content and token: refused unless
    /// the board's generation is `authorized`, the one its authority binds it to.
    fn bound_task(slug: &str, row: Option<Read>, authorized: &str) -> Result<BoundTask, Error> {
        let row = row.ok_or_else(|| no_task(slug))?;
        match row.generation {
            Some(generation) if generation == authorized => Ok(BoundTask {
                bound: Bound {
                    compat: COMPAT,
                    generation,
                    id: row.id,
                    version: row.version,
                },
                title: row.title,
                body: row.body,
            }),
            _ => Err(refused(RESTORED.into())),
        }
    }

    /// The task `slug` and the token to write it again by, read in one statement. Refused on a
    /// store with no authority, and while the authority binds the board to no generation or to
    /// another than the board's own.
    pub fn bound(&self, slug: &str) -> Result<BoundTask, Error> {
        let authorized = self.authorized()?;
        let row: Option<Read> =
            self.retrying(|| self.read(Statement::from(pg::read()), json!({"slug": slug})))?;
        Store::bound_task(slug, row, &authorized)
    }

    /// [`Store::amend`], only on the task `expected` was read as: refused, with nothing written
    /// (no title, link, line on the thread or act of `handle`'s), unless the board is at the
    /// token's generation and `slug` is the task of its number at its version. The check and the
    /// write are one transaction; one that met another's write is checked again against the same
    /// token, never a newer one. The task as the write left it, with the token to write it next,
    /// is that transaction's own answer.
    pub fn amend_bound(
        &self,
        slug: &str,
        expected: &Bound,
        title: &str,
        links: &[String],
        handle: &str,
    ) -> Result<BoundTask, Error> {
        let title = title.trim();
        if title.is_empty() {
            return Err(refused("a task needs a title".into()));
        }
        let fence = self.bound_vars(slug, expected)?;
        let acts = handle != OWNER;
        let body = || Statement::from(pg::amend(acts)).posting();
        self.bound_write(
            slug,
            expected,
            handle,
            &fence,
            json!({"title": title, "links": links}),
            body,
        )
    }

    /// A task's title, its brief or both replaced and its links added to, only on the task
    /// `expected` was read as: [`Store::amend_bound`]'s check, refusal and answer, for an edit
    /// that may rewrite the brief, which no other write does. A title or a brief that changed is
    /// a line on the thread naming `handle`; the brief's own words are not kept there. Refused
    /// before anything is sent when the edit changes nothing or gives an empty title.
    pub fn edit_bound(
        &self,
        slug: &str,
        expected: &Bound,
        edit: &Edit,
        handle: &str,
    ) -> Result<BoundTask, Error> {
        let title = edit.title.as_deref().map(str::trim);
        if title == Some("") {
            return Err(refused("a task needs a title".into()));
        }
        if title.is_none() && edit.body.is_none() && edit.links.is_empty() {
            return Err(refused(
                "an edit gives a title, a brief or links: nothing was written".into(),
            ));
        }
        let fence = self.bound_vars(slug, expected)?;
        let acts = handle != OWNER;
        let more = json!({
            "retitle": title.is_some(), "title": title.unwrap_or_default(),
            "rewrite": edit.body.is_some(), "body": edit.body.as_deref().unwrap_or_default(),
            "links": edit.links,
        });
        let body = || Statement::from(pg::edit(acts)).posting();
        self.bound_write(slug, expected, handle, &fence, more, body)
    }

    /// `body`, a write that starts with the fence, sent with `fence`'s variables and `more`'s as
    /// `handle`'s act, and its answer read as the task under `expected`'s generation.
    fn bound_write<'a>(
        &self,
        slug: &str,
        expected: &Bound,
        handle: &str,
        fence: &Value,
        more: Value,
        body: impl Fn() -> super::statement::Statement<'a>,
    ) -> Result<BoundTask, Error> {
        let row: Option<Read> = self.retrying(|| {
            let mut vars = self.acting(handle, (self.clock)(), more.clone());
            if let (Some(vars), Value::Object(fence)) = (vars.as_object_mut(), fence.clone()) {
                vars.extend(fence);
            }
            self.transact(body(), vars)
        })?;
        Store::bound_task(slug, row, &expected.generation)
    }

    /// Bind the board anew: the authority revoked, the board's generation replaced in one
    /// transaction, and only then the new one published. What a managed physical restore ends
    /// with, and what puts right a board whose authority and generation disagree. Every token
    /// read before it is refused from then on. The generation found is never adopted: a rebind
    /// interrupted before it published leaves the board bound to none, and is run again.
    pub fn rebind(&self) -> Result<String, Error> {
        let authority = self
            .authority
            .as_ref()
            .ok_or_else(|| refused(NO_AUTHORITY.into()))?;
        authority.revoke().map_err(|why| {
            refused(format!(
                "the board's recovery authority could not be revoked ({why}): nothing was changed"
            ))
        })?;
        // Held to this store's own schema, as a copy is: a rebind is never a reader's ordinary
        // write on a board one schema ahead ([`crate::schema::COMPATIBLE`]).
        let generation: String = self.retrying(|| {
            self.run_transaction(
                Statement::from(pg::rebind()),
                json!({}),
                super::transaction::Run::Own,
            )
        })?;
        publish(authority.as_ref(), &generation)?;
        Ok(generation)
    }

    /// What a copy of the whole board ends with: `generation`, which its swap made, published to
    /// this store's authority when it has one.
    pub(super) fn copied(&self, generation: &str) -> Result<(), Error> {
        match &self.authority {
            Some(authority) => publish(authority.as_ref(), generation),
            None => Ok(()),
        }
    }
}

/// `generation`, which the board now holds, published to `authority`.
fn publish(authority: &dyn Authority, generation: &str) -> Result<(), Error> {
    authority.publish(generation).map_err(|why| {
        Error::Db(format!(
            "the board's recovery generation was replaced, and its authority did not take the \
             new one ({why}): bound writes are refused until `just tasks recovery rebind` binds it"
        ))
    })
}
