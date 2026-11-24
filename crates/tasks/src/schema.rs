//! The board's contract: its version ([`VERSION`]), its additive steps on it ([`ADDED`]), and the
//! rule for writers of different contracts on one board ([`decide`]). Its tables, indexes and
//! triggers are `store/postgres/schema.rs`'s, one database a board, each row's id its key as
//! JSON and its fields one document.
//!
//! `meta`'s `schema` row names the board's contract, so opening a board already in this shape
//! writes nothing, and from schema 25 `writable_from`, the oldest schema whose Symphony may write
//! the board ([`Schema::writable_from`]), written in the migration's one transaction beside the
//! version. A board moved by a build before schema 25 has none, which lets no older Symphony write
//! it. Its `additions` row counts the additive steps on that contract the board has ([`ADDED`]),
//! in the `version` field `meta` already defines: no row is none.
//!
//! `recovery`'s `board` row names the board's recovery generation, from schema 22: a text the
//! database makes, replaced in the transaction of every copy of the whole board and by a rebind
//! (`store/recovery.rs`), never by an ordinary write and never part of a dump. A build of an older
//! schema copies a board without replacing it, which is why it came with a schema step: the
//! schema guard refuses that build's copy.
//!
//! `execution_attempt`, `execution_fact`, `execution_fence` and `execution_artifact` are execution
//! attempts, from schema 23 (`crate::execution`), read by no cache.
//!
//! `workflow` holds a custom delivery workflow's records, from schema 24 (`store/workflow.rs`,
//! docs/workflow/WORKFLOW.md): the active generation, an immutable definition by its digest, an
//! activation by its generation, a task's transition by its slug and workflow revision, a receipt
//! by its principal and key, and an external intent; and, from schema 26, for its guards
//! (`workflow/guard.rs`), the state the board accepted from each approval authority and each
//! guarded edge's admission. A task the workflow holds keeps its position in
//! `fields._board.workflow`. Two triggers are the fence every writer meets, whatever it is: a
//! task's position, or the status of a task that has one, changes only beside the transition
//! recorded for it, and from schema 27 a position goes only beside the transition made by an
//! activation recorded as a return to the board's writers; and a recorded definition, activation,
//! transition, receipt or admission is never changed, nor an authority's accepted generation
//! lowered within its epoch. A board with no custom workflow holds no such record and meets
//! neither.
//!
//! `setting` holds the stored preferences, from the first additive step on schema 28 ([`ADDED`],
//! `crate::settings`): the setting's key, the value it holds, its revision and every change of it,
//! read at each read of the settings. The second additive step on schema 28 is the index of
//! `session` by `handle` (`session_handle`), where a session is read by its handle rather than by
//! its id. The third marks a person's line ([`PERSON_LINES`]), the fourth keeps a board's people
//! ([`PEOPLE`]) and the fifth which group a thing belongs to ([`OWNERSHIP`]).
//!
//! `writer` holds a program that opens the board to write, the schema it was built at and when it
//! was last seen (`store/writers.rs`), from schema 18: builds of other schemas write it, and a line
//! written moves no marker.
//!
//! The board is a contract between the builds that read and write it (docs/workflow/TASKS.md, where it
//! lives). Its version, [`VERSION`], is raised by a breaking step alone; an additive step ([`ADDED`])
//! leaves it as it is, so every build of one contract reads and writes every board of it, whatever
//! additive steps either has. What an additive step may be is held by the store's catalogue test
//! (`tests/store/catalogue.rs`): tables of its own, definitions of the database's own, an index
//! that is not unique, and an optional field on a table every read and write of which keeps a field
//! it does not know ([`OPEN`]). Anything else is breaking.
//!
//! The rule for writers of different contracts on one board ([`decide`]): a build opens a board for
//! writing only at its own contract. A board at an older contract, or none, is migrated forward once
//! by the first newer build to open it for writing: the analyzer, then the steps and the records in
//! one transaction that first checks the version and the additive steps it read are still the
//! board's, before any other write. A project's live board is moved forward only by a build its main
//! checkout says is its own ([`crate::live`]). An older build refuses a newer board before any
//! mutation, naming the project, the program to rebuild and `just symphony`; a reader one version
//! behind reads it. A reader never moves a board forward or makes one: a board behind it, or none,
//! it refuses before it reads, naming `just tasks`. Every write transaction starts with the
//! schema guard (`store::postgres::GUARD`), so a store opened before a newer build migrated stops at
//! its next write.
//!
//! On a board of its own contract, a writer that has additive steps the board lacks adds them in
//! the same way, but only where it may move the board, and holds no writer: every build of the
//! contract keeps writing it. Where it may not (a project's live board it is not the build of), it
//! and every reader use the board as it is: the guard refuses, before anything is written, a write
//! that names a table of a step the board lacks, and the database refuses a table or a column it
//! does not define. A whole-board
//! copy, its staging and a recovery's rebind are held to the build's own additive steps exactly,
//! and so is the whole-board read a copy is made from ([`crate::Store::dump`]): a board with steps
//! the build lacks would lose what they hold, one lacking its steps could not hold its rows.
//!
//! One exception, for the Symphony alone ([`COMPATIBLE`]): a store opened to read, by the rule
//! ([`Access::Read`]), writes a board one schema ahead of its own whose `writable_from` names its
//! schema or an older one, which the guard reads in each write's own transaction. Every store
//! opened to write, a store taken unchecked and every whole-board copy keep the rule above.

use crate::store::Program;

/// The board's contract: raised by a breaking step alone, one whose definitions change or remove
/// what a build of the contract before reads or writes, or whatever else [`ADDED`] may not hold. A
/// board at an older version runs the definitions once, and their `IF NOT EXISTS` adds what is new
/// but leaves every definition already there as it was. Protocol changes can require a wider version step and
/// retirement of old read tokens: an already-open store does not repeat its opening check. A
/// version listed in [`COMPATIBLE`] says its step keeps a Symphony one schema behind writing.
///
/// 28 is the contract's first: builds before it read a board's rows as no field more than they
/// know and replace some rows whole, so no step of theirs could be declared additive. From it on,
/// every build reads and writes past what it does not know ([`OPEN`]).
pub const VERSION: i64 = 28;

/// An additive step on the contract [`VERSION`]: its definitions, run after the board's tables and
/// the steps before it, and the board's count of additive steps (`meta`'s `additions`) counts it
/// once a build that may move the board has run them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Addition {
    /// Its definitions, each `IF NOT EXISTS`.
    pub postgres: &'static str,
    /// The tables its step defines: a write naming one is refused on a board without the step.
    pub tables: &'static [&'static str],
    /// Why it is additive: what it adds, and how a build without it reads and writes around it,
    /// what no catalogue shows (docs/workflow/CHECKS.md, a schema step).
    pub why: &'static str,
}

/// This build's additive steps on [`VERSION`], in order: a breaking step empties it, its
/// definitions going into the board's tables, as what they added is then the contract's.
pub const ADDED: &[Addition] = &[
    Addition {
        postgres: "CREATE TABLE IF NOT EXISTS setting (id jsonb PRIMARY KEY, \
                   doc jsonb NOT NULL CHECK (jsonb_typeof(doc) = 'object'));",
        tables: &["setting"],
        why: "the table setting, the board's store of preferences (crate::settings): a build \
              without it never reads or writes the table and reads every setting from the files \
              alone, so a stored value holds only for builds that have the step, and no rule on a \
              table there before reads it",
    },
    Addition {
        postgres: "CREATE INDEX IF NOT EXISTS session_handle ON session (handle);",
        tables: &[],
        why: "an index of session by handle, where a write's actor, an inbox's reader and a \
              channel's session are read by handle, not by the row's id: a build without it reads \
              the same rows by scanning the table, and writes them as it did",
    },
    PERSON_LINES,
    PEOPLE,
    OWNERSHIP,
];

/// The step that marks a person's line ([`crate::rows::MessageRow::person`]): the nullable column
/// `person` on `message`, an open table, set on a line a person a board named wrote
/// ([`crate::Store::for_person`]) and on no other. It names no table of its own, so no write is
/// refused for naming one; a write for a person needs it ([`crate::Store::is_person`]).
pub const PERSON_LINES: Addition = Addition {
    postgres: "ALTER TABLE message ADD COLUMN IF NOT EXISTS person boolean;",
    tables: &[],
    why: "the nullable column person on message, true on a line a person a board named wrote: a \
          build without it writes a line with none, as every line before it was written, and \
          reads one with it past the column it does not know (OPEN), as the owner's word it does \
          not hear; a build with it sets the column only for a person, which needs the step, and \
          reads it through the row, so a board without it reads every line as no person's",
};

/// The step that keeps who is on a board ([`crate::people`]): the tables `people` (a row a person,
/// the groups they are in and the powers given them), `people_group` (a row a group, its powers)
/// and `people_log` (every change, numbered), each row whole in its `doc`, as `setting` keeps one.
pub const PEOPLE: Addition = Addition {
    postgres: "CREATE TABLE IF NOT EXISTS people (id jsonb PRIMARY KEY, \
               doc jsonb NOT NULL CHECK (jsonb_typeof(doc) = 'object'));\n\
               CREATE TABLE IF NOT EXISTS people_group (id jsonb PRIMARY KEY, \
               doc jsonb NOT NULL CHECK (jsonb_typeof(doc) = 'object'));\n\
               CREATE TABLE IF NOT EXISTS people_log (id jsonb PRIMARY KEY, \
               doc jsonb NOT NULL CHECK (jsonb_typeof(doc) = 'object'));",
    tables: &["people", "people_group", "people_log"],
    why: "the tables people, people_group and people_log, who is on a board with their powers \
          and the record of each change (crate::people): a build without them never reads or \
          writes them, so it admits nobody but the person at the machine and checks no power, as \
          every build before them did, and no rule on a table there before reads them",
};

/// The step that keeps which group a thing belongs to ([`crate::people::ownership`]): the table
/// `ownership`, the board's map of places to groups and every hand-over, a row a line of its log,
/// numbered, whole in its `doc`.
pub const OWNERSHIP: Addition = Addition {
    postgres: "CREATE TABLE IF NOT EXISTS ownership (id jsonb PRIMARY KEY, \
               doc jsonb NOT NULL CHECK (jsonb_typeof(doc) = 'object'));",
    tables: &["ownership"],
    why: "the table ownership, the board's map of places to groups and its hand-overs \
          (crate::people::ownership): a build without it never reads or writes the table, so it \
          gives no card or task a group, as every build before it, and no rule on a table there \
          before reads it",
};

/// The number of [`PEOPLE`] among [`ADDED`], counted from 1: 0 once a breaking step made its tables
/// the contract's own, which every board of it has.
pub(crate) fn people_step() -> i64 {
    step_of(PEOPLE)
}

/// The number of [`OWNERSHIP`] among [`ADDED`], counted from 1: 0 once a breaking step made its
/// table the contract's own.
pub(crate) fn ownership_step() -> i64 {
    step_of(OWNERSHIP)
}

/// The number of `step` among [`ADDED`], counted from 1: 0 where it is not there.
fn step_of(step: Addition) -> i64 {
    let at = ADDED.iter().position(|added| *added == step);
    at.map_or(0, |at| i64::try_from(at + 1).unwrap_or(i64::MAX))
}

/// The number of [`PERSON_LINES`] among [`ADDED`], counted from 1: 0 once a breaking step made the
/// field the contract's own, which every board of it has.
pub(crate) fn person_step() -> i64 {
    let at = ADDED.iter().position(|step| *step == PERSON_LINES);
    at.map_or(0, |at| i64::try_from(at + 1).unwrap_or(i64::MAX))
}

/// How many additive steps this build has on its contract: [`ADDED`]'s length.
pub const ADDITIONS: i64 = {
    assert!(ADDED.len() < 1 << 32);
    ADDED.len() as i64
};

/// The tables an additive step may give an optional field: every read of their rows ignores a
/// field it does not know, and every write of them keeps one (`tests/store/contract.rs`). The rest are closed: a field on one is a breaking step.
pub const OPEN: [&str; 11] = [
    "task",
    "link",
    "session",
    "role",
    "message",
    "delivery",
    "delivery_key",
    "barred_key",
    "task_session",
    "ask",
    "usage",
];

/// The schema versions whose step keeps a Symphony one schema behind writing the board, each with
/// one line saying why: what the step added, and why no rule in code that writes a table older than
/// it must now follow anything new. A version not listed does not, which is every step's default,
/// so a schema bump that writes nothing here keeps the older Symphony read-only on the board it
/// moved, until a release at its schema serves it.
///
/// A step is listed only when it changes no table that existed before it: no column, index,
/// trigger, function or check added to or changed on one. The store's catalogue test holds a
/// listed version to that on the database (`tests/store/catalogue.rs`, over the committed
/// definitions of each version); what it cannot see, a rule held in code that the older
/// Symphony's writes would skip, is its reviewer's to read against the line
/// ([CHECKS](../../../docs/workflow/CHECKS.md#a-schema-step-that-keeps-the-symphony-writing)). The
/// schema that brought this list and the record it writes ([`WRITABLE_FROM_SINCE`]) changed `meta`
/// and is not listed.
pub const COMPATIBLE: &[(i64, &str)] = &[];

/// The schema whose step brought `meta`'s `writable_from`: a Symphony below it never reads
/// the record, and refuses every write on a board ahead of it whatever the record says.
pub const WRITABLE_FROM_SINCE: i64 = 25;
const _: () = assert!(WRITABLE_FROM_SINCE <= VERSION);

/// Whether `version`'s step keeps a Symphony one schema behind writing ([`COMPATIBLE`]).
pub fn keeps_writes(version: i64) -> bool {
    COMPATIBLE.iter().any(|(listed, _)| *listed == version)
}

/// A board schema: its contract, its additive steps on it, and the steps that bring a board of any
/// older version to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Schema {
    version: i64,
    /// The tables each of its additive steps on `version` defines, in order ([`ADDED`]).
    added: Vec<Vec<String>>,
    /// What the steps add to this build's tables, which every migration makes first: nothing for
    /// this build's own schema.
    postgres: String,
    /// Whether its own step keeps a Symphony one schema behind writing ([`COMPATIBLE`]).
    keeps: bool,
}

impl Schema {
    /// This build's: [`VERSION`], by its own tables.
    pub fn this_build() -> Schema {
        Schema::of_this_contract(ADDITIONS)
    }

    /// This build's contract with its first `steps` additive steps ([`ADDED`]), all of them past
    /// their count: a build of the same contract before the rest.
    pub fn of_this_contract(steps: i64) -> Schema {
        let contract = Schema {
            version: VERSION,
            added: Vec::new(),
            postgres: String::new(),
            keeps: keeps_writes(VERSION),
        };
        let taken = usize::try_from(steps.max(0)).unwrap_or(usize::MAX);
        (ADDED.iter().take(taken)).fold(contract, |schema, step| {
            schema.then_additive(step.postgres, step.tables)
        })
    }

    /// Its contract.
    pub fn version(&self) -> i64 {
        self.version
    }

    /// How many additive steps it has on its contract.
    pub fn additions(&self) -> i64 {
        i64::try_from(self.added.len()).unwrap_or(i64::MAX)
    }

    /// Each table its additive steps define, with the number of the step that defines it.
    pub(crate) fn added_tables(&self) -> Vec<(String, i64)> {
        (1..)
            .zip(&self.added)
            .flat_map(|(step, tables)| tables.iter().map(move |table| (table.clone(), step)))
            .collect()
    }

    /// The oldest schema whose Symphony may write a board this schema's build moved: the version
    /// before when its step keeps that Symphony writing ([`COMPATIBLE`]), its own otherwise. A
    /// migration writes it beside the version (`meta`'s `writable_from`).
    pub fn writable_from(&self) -> i64 {
        self.version - i64::from(self.keeps)
    }

    /// The schema one version on, `postgres` run after this one's steps: a newer build's, whose
    /// step says nothing of the Symphony one behind, so keeps it read-only
    /// ([`Schema::keeping_writes`]).
    pub fn then_each(&self, postgres: &str) -> Schema {
        Schema {
            version: self.version + 1,
            added: Vec::new(),
            postgres: format!("{}\n{postgres}", self.postgres),
            keeps: false,
        }
    }

    /// This contract with one additive step more, `postgres` run after this one's steps, defining
    /// `tables`: a build of the same contract with a step this one lacks ([`ADDED`]).
    pub fn then_additive(&self, postgres: &str, tables: &[&str]) -> Schema {
        let mut added = self.added.clone();
        added.push(tables.iter().map(|table| (*table).into()).collect());
        Schema {
            added,
            postgres: format!("{}\n{postgres}", self.postgres),
            ..self.clone()
        }
    }

    /// This schema, its step declared to keep a Symphony one schema behind writing: a newer
    /// build whose version [`COMPATIBLE`] lists.
    pub fn keeping_writes(self) -> Schema {
        Schema {
            keeps: true,
            ..self
        }
    }

    /// The schema one version back, by the same steps: an older build's, as far as the rule goes,
    /// whose step declares nothing.
    pub fn before(&self) -> Schema {
        Schema {
            version: self.version - 1,
            added: Vec::new(),
            keeps: false,
            ..self.clone()
        }
    }

    /// What the steps add to this build's tables. Only a migration reads them.
    pub(crate) fn postgres_steps(&self) -> &str {
        &self.postgres
    }
}

/// What a store opens a board for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// Reads and writes, at the store's own version only: an older board, or one not made yet, is
    /// migrated forward to it.
    Write,
    /// Reads, of a board at the store's own version or one ahead, and never moves or makes one: a
    /// board behind it, or not made yet, is refused. Its writes still meet the schema guard, so one
    /// version behind it writes only a board whose `writable_from` names its version or an older
    /// one ([`COMPATIBLE`]); a whole-board copy it never writes there. The Symphony's opening.
    Read,
}

/// What a reader's refusal of a board not made yet says after the board's name
/// ([`crate::board_health::absent`] reads it).
pub(crate) const NOT_MADE: &str = " is not made yet, and a reader never makes a board";

/// What opening a board at `found` (none: no board yet) takes, for a build of version `own`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Opened {
    /// The board is at this build's version: nothing to write.
    Current,
    /// The board is one version ahead and the store only reads it.
    Ahead,
    /// The board is older, or none, and the store writes: this build migrates it forward.
    Migrate,
    /// Refused, in these words, before anything is read or written.
    Refused(String),
}

/// The schema rule for a build of version `own`, the program `program`, opening, for `access`, a
/// board at `found` of the project `project` (none: a board no project names, such as a scratch
/// one). Only a writer moves a board forward or makes one: a reader is the Symphony, whose build may
/// be a branch's, and it would move the live board past the build its project's board command runs,
/// which then refuses it.
pub(crate) fn decide(
    found: Option<i64>,
    own: i64,
    access: Access,
    project: Option<&str>,
    program: &Program,
) -> Opened {
    let named = Named::of(project);
    match (access, found) {
        (_, Some(found)) if found == own => Opened::Current,
        (Access::Write, None) => Opened::Migrate,
        (Access::Write, Some(found)) if found < own => Opened::Migrate,
        (Access::Write, Some(found)) => {
            let refusal = Refusal::of(project, own, program);
            Opened::Refused(format!("{}{found}{}", refusal.before, refusal.after))
        }
        (Access::Read, Some(found)) if found == own + 1 => Opened::Ahead,
        (Access::Read, Some(found)) if found > own => Opened::Refused(format!(
            "{} is at schema {found}, more than one ahead of this build's {own}: nothing was \
             read; `just symphony` in {} brings its Symphony to the board's schema",
            named.board, named.project
        )),
        (Access::Read, Some(found)) => Opened::Refused(format!(
            "{} is at schema {found}, behind this build's {own}, and a reader never moves a board \
             forward: nothing was read or written; `just tasks migrate` in the main checkout of {} \
             moves it once the project's board command is at schema {own}, and a newer schema is \
             tried on a scratch board (`TASKS_DATABASE=NAME`)",
            named.board, named.project
        )),
        (Access::Read, None) => Opened::Refused(format!(
            "{}{NOT_MADE}: nothing was read or written; `just tasks list` in {} makes it",
            named.board, named.project
        )),
    }
}

/// How a refusal names the board and its project.
pub(crate) struct Named {
    pub board: String,
    pub project: String,
}

impl Named {
    pub(crate) fn of(project: Option<&str>) -> Named {
        match project {
            Some(project) => Named {
                board: format!("the {project} board"),
                project: project.into(),
            },
            None => Named {
                board: "this board".into(),
                project: "its project".into(),
            },
        }
    }
}

/// A write's refusal around the board's version, which only the schema guard reads at the write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Refusal {
    /// The board, as a refusal names it.
    pub board: String,
    pub before: String,
    pub after: String,
}

impl Refusal {
    /// The refusal of `program`, a build of version `own`, writing a board of `project` at another.
    pub(crate) fn of(project: Option<&str>, own: i64, program: &Program) -> Refusal {
        let named = Named::of(project);
        Refusal {
            board: named.board.clone(),
            before: format!("{} is at schema ", named.board),
            after: format!(
                ", and this build writes only a board at its own schema, {own}: nothing was \
                 written; rebuild {} ({}) at the board's schema, and `just symphony` in {} brings \
                 its Symphony to it",
                program.name, program.path, named.project
            ),
        }
    }
}

/// What a write takes of the board's additive steps on its contract, which the schema guard
/// (`store::postgres::GUARD`) holds it to: no fewer than `least`, no more than `most`, refused in
/// the words `before`, the board's count, then `after`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Steps {
    pub least: i64,
    pub most: i64,
    pub before: String,
    pub after: String,
}

impl Steps {
    /// An ordinary write that names a table of no step: any count.
    pub(crate) fn any() -> Steps {
        Steps {
            least: 0,
            most: i64::MAX,
            before: String::new(),
            after: String::new(),
        }
    }

    /// An ordinary write that names `table`, of the step `step`, on `board` (as a refusal names
    /// it), by a build of contract `own`.
    pub(crate) fn naming(table: &str, step: i64, board: &str, own: i64) -> Steps {
        Steps {
            least: step,
            most: i64::MAX,
            before: format!("{board} has "),
            after: format!(
                " of schema {own}'s additive steps, and this write names `{table}`, which step \
                 {step} defines: nothing was written; the board takes the step when a build that \
                 may move it runs (`just tasks migrate` in its main checkout)"
            ),
        }
    }

    /// A whole-board copy, its staging or a recovery's rebind by a build of contract `own` with
    /// `steps` additive steps, on `board`: exactly its own.
    pub(crate) fn exactly(steps: i64, board: &str, own: i64) -> Steps {
        Steps {
            least: steps,
            most: steps,
            before: format!("{board} has "),
            after: format!(
                " of schema {own}'s additive steps and this build has {steps}: a whole-board copy \
                 or a recovery's rebind runs only on a board with the build's own, as it would \
                 lose what steps it lacks hold, or write rows a board without its own cannot \
                 hold; nothing was written; run it with a build at the board's steps"
            ),
        }
    }
}

/// The highest of the additive steps `added` (each table with the step that defines it) whose table
/// `sql` names, with that table: a whole word of it, however it is quoted (`table`, 'table',
/// "table") and wherever it is, in a statement nested in another, in a block or in a subquery.
/// Case is not looked at, so a word that only might be the table counts.
pub(crate) fn needs<'a>(sql: &str, added: &'a [(String, i64)]) -> Option<(&'a str, i64)> {
    if added.is_empty() {
        return None;
    }
    let mut needed: Option<(&str, i64)> = None;
    for word in sql.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
        if word.is_empty() {
            continue;
        }
        for (table, step) in added {
            if table.eq_ignore_ascii_case(word) && needed.is_none_or(|(_, most)| *step > most) {
                needed = Some((table, *step));
            }
        }
    }
    needed
}

/// The schema guard's variables (`store::postgres::GUARD`), for a store of version `own` refusing
/// in `refusal`'s words, which takes a board one ahead whose record lets it when `ahead`, and the
/// additive steps `steps` on its own: added to a write's own (`vars`, an object, or nothing).
pub(crate) fn guard_vars(
    vars: &mut serde_json::Value,
    own: i64,
    ahead: bool,
    refusal: &Refusal,
    steps: &Steps,
    refused: &str,
) {
    if !vars.is_object() {
        *vars = serde_json::json!({});
    }
    vars["board_schema"] = own.into();
    vars["board_schema_ahead"] = ahead.into();
    vars["board_schema_before"] = format!("{refused}{}", refusal.before).into();
    vars["board_schema_after"] = refusal.after.clone().into();
    vars["board_additions_least"] = steps.least.into();
    vars["board_additions_most"] = steps.most.into();
    vars["board_additions_before"] = format!("{refused}{}", steps.before).into();
    vars["board_additions_after"] = steps.after.clone().into();
}

/// Whether a write of a store of version `own` passes the schema guard on a board at `found` (none:
/// no record) whose `writable_from` is `from` as the database gave it, `ahead` saying whether the
/// write may take a board one ahead: the guard's rule, for a reader that asks before it writes
/// ([`crate::Store::board_standing`]). Anything but an integer in `from` lets nothing through.
pub fn writes(found: Option<i64>, from: &serde_json::Value, own: i64, ahead: bool) -> bool {
    found == Some(own)
        || (ahead
            && found.is_some_and(|found| Some(found) == own.checked_add(1))
            && from.as_i64().is_some_and(|from| from <= own))
}

/// The statements that change a database: a transaction holding one is a write.
const MUTATING: [&str; 5] = ["ALTER", "CREATE", "DELETE", "INSERT", "UPDATE"];

/// Whether `sql` can change the database: it names one of the statements that do, as a whole word in
/// any case. The board's statements are this crate's own text, every value a variable beside it.
/// The PostgreSQL clients count a request by it; the store says what its statements do, and a
/// debug build holds what it says to this (the store's `Statement::disagreement`).
pub(crate) fn mutates(sql: &str) -> bool {
    sql.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .any(|word| {
            MUTATING
                .iter()
                .any(|statement| statement.eq_ignore_ascii_case(word))
        })
}

#[cfg(test)]
#[path = "schema_tests.rs"]
mod tests;
