//! The board's tables in SurrealDB, one per SQLite table of `tools/tasks_store.py`, typed so a row
//! the rules would never write is refused rather than kept. A record's id is its SQLite primary key:
//! `task:⟨slug⟩`, `session:⟨handle⟩`, `role:[slug, handle, role]`, `link:[slug, kind, target]`,
//! `message:[slug, id]`, `delivery:[message, handle]`, `barred_key:⟨key⟩`, `task_session:[slug, handle]`,
//! `delivery_key:[slug, key]` (the message a keyed post made once on a thread, `Store::say_keyed`),
//! `ask:⟨sid⟩` (what the owner said that waits on an agent's answer, `Store::open_ask`, numbered by
//! `counter:ask`; never a SQLite table), `usage:[handle, slug, role, model, speed, round]` (the
//! tokens a session's beats added up, `crate::usage`, from schema 19; never a SQLite table either,
//! and with no event, as a beat moves the marker by its session's revision already).
//! A message is keyed by its task first so a thread is one range of keys: an index on `slug` would do
//! the same, but makes a transaction inserting many messages (the copy's) cost more than linearly
//! (15k messages: 22 CPU-s with it, 2 without, measured on 3.3.0).
//! SQLite's row ids are kept as `sid`; `counter:message` and `counter:task` hold the last id given out,
//! `counter:tree` counts the moves of a task under another, and `counter:['agent', key]` the binds of
//! one agent (`Store::bind_agent`): each a record the writes that must not pass each other write.
//! Mutation events advance `counter:board_structure` in the transaction that changes board inputs,
//! callback registration/observations or owner chat. Session mutations advance per-handle revisions
//! instead, so independent tool-call beats never write the structural counter. Readers compare the
//! combined BOARD_MARKER; guard-only writes on a revision row do not invalidate cached inputs.
//! An event UPDATEs the marker, or CREATEs it on a board's first write, and never UPSERTs it: an
//! UPSERT of a record that exists tries a create and rolls it back to a savepoint, and SurrealKV's
//! rollback walks every key the transaction has written, so the copy's one transaction, an event a
//! row, took time quadratic in its rows (the board of 2026-11-04: 17 min for that transaction; the
//! whole copy now takes 20 s).
//! Per-recipient `callback_lease` rows retain their revision and periodic PULL checkpoint across
//! callback retirement; an event key must never inherit an earlier holder's revision.
//!
//! What a write, each recipient of its dispatch and each tick read is found through an index, never a
//! scan of the table (the board crate's `perf` tests plan every one, and every query an event here
//! runs): messages by `sid` (a cursor's next), by `ts` (an inbox since its session connected), by
//! recipient then `sid` (the owner's newest), by sender, kind, `sid` and slug (a sender's newest
//! post, and newest verdict); deliveries by handle then message; roles by role; registrations by
//! address. `said:[sender]` names each sender's newest `msg` or verdict, the greatest `[sid, slug]`
//! (a copy takes one `sid` on two threads), and `DEFINE` fills it once when it holds nothing. Its
//! key is an array holding the sender, so every string a message's sender may be is one, the empty
//! string too: a record id that is the string itself refuses the empty one, and a copy of a board of
//! schema 12 holding a word with no sender failed on it. An
//! event keeps it as messages are made, deleted or edited: a message made, or edited into a
//! sender's word, is that sender's word if no greater one is; a word deleted, or edited to another
//! sender or out of the words, gives way to the sender's word before it, the greater of the last
//! key of the sender's posts and the last of its verdicts in `message_sender`: two keys read,
//! however many messages the board holds. They name the sender as a variable of its own,
//! `$sender`: the server plans `sender = $before.sender` as a scan of the whole table, with no
//! index (3.3.0 `EXPLAIN`). The board makes messages and edits none; its copy deletes
//! them all at once and empties `said` first, so no deletion looks back. What does look back: a
//! copy by a store of schema 12, which deletes the messages with `said` still full (each sender's
//! word deleted gives way to its next, until none is left: 19.5 s for a copy of the `perf` tests'
//! 50k messages, against 17.6 s with `said` emptied first), and a hand at the server deleting or
//! editing a word. Until `message_sender`, the look back walked `message_sid` from the newest
//! message down, and in an event SurrealKV walks a range backward over the rows it holds in memory
//! (every row written since it last wrote them to an sstable: up to 4 GiB, its default on the
//! machine measured, 256 MiB on a server `server::Storage` sets up) out of all proportion: over 150 s
//! for 50k messages, where the same walk took 4.6 s as a statement of its own and 1.8 s once a
//! restart had written the rows to disk, so a delete of a rare sender's newest word on a board just
//! copied did not answer in 600 s (3.3.0, 2026-11-07). These cost the copy's one transaction
//! less than the machine's load moves it: the live board's 49k messages and 36k deliveries swapped
//! in 123 s without the events and 97 s with them, one after the other at load 140; defining them
//! on that board took 15 s; and the `perf` tests' board of 50k messages copied in 41 and 44 s with
//! `message_sender`, 60 and 53 s without it, alternating on one server at load 65 to 85.
//!
//! `moved:sqlite` is the record the move from SQLite wrote, a one-time move older builds made and
//! this one no longer does: the board it moved, when, and a hash of every row the move left. The
//! table stays defined, so a board that holds the record keeps it and every board keeps its shape.
//!
//! `meta:schema` names the board's contract ([`VERSION`]), so opening a board already in this shape
//! writes nothing, and from schema 25 `writable_from`, the oldest schema whose Symphony may write
//! the board ([`Schema::writable_from`]), written in the migration's one transaction beside the
//! version. A board moved by a build before schema 25 has none, which lets no older Symphony write
//! it. `meta:additions` counts the additive steps on that contract the board has ([`ADDED`]), in
//! the `version` field `meta` already defines: no record is none.
//!
//! `recovery:board` names the board's recovery generation, from schema 22: a text the database
//! makes, replaced in the transaction of every copy of the whole board and by a rebind
//! (`store/recovery.rs`), never by an ordinary write and never part of a dump. A build of an
//! older schema copies a board without replacing it, which is why it came with a schema step:
//! the [`guard`] refuses that build's copy.
//!
//! `execution_attempt:⟨uuid⟩`, `execution_fact:[uuid, seq]`, `execution_fence:[kind, …]` and
//! `execution_artifact:[uuid, name]` are execution attempts, from schema 23 (`crate::execution`):
//! schemaless and with no event, as no reader caches them.
//!
//! `workflow:[KIND, …]` is a custom delivery workflow's record, from schema 24 (`store/workflow.rs`,
//! docs/workflow/WORKFLOW.md): the active generation (`['active']`), an immutable definition by its digest,
//! an activation by its generation, a task's transition by its slug and workflow revision, a
//! receipt by its principal and key, and an external intent; and, from schema 26, for its guards
//! (`workflow/guard.rs`), the state the board accepted from each approval authority
//! (`['authority', ADAPTER]`) and each guarded edge's admission (`['admission', SLUG, REVISION]`).
//! A task the workflow holds keeps its position in `fields._board.workflow`. Two events are the
//! fence every writer meets, whatever it is: a task's position, or the status of a task that has
//! one, changes only beside the transition recorded for it (`workflow_position`), and from schema
//! 27 a position goes only beside the transition made by an activation recorded as a return to the
//! board's writers; and a recorded definition, activation, transition, receipt or admission is
//! never changed, nor an authority's accepted generation lowered within its epoch
//! (`workflow_history`). No event reads a record's deletion: the whole-board copy replaces the
//! table. A board with no custom workflow holds no such record and meets neither.
//!
//! `setting:⟨key⟩` is a stored preference, from the first additive step on schema 28
//! ([`ADDED`], `crate::settings`): the setting's key, the value it holds, its revision and every
//! change of it. Schemaless and with no event: its readers read it at each read of the settings.
//! The second additive step on schema 28 is PostgreSQL's index of `session` by `handle`
//! (`session_handle`), where a session is read by its handle rather than by its id.
//!
//! `writer:[host, path]` is a program that opens the board to write, the schema it was built at and
//! when it was last seen (`store/writers.rs`), from schema 18: schemaless and with no event, as
//! builds of other schemas write it and a line written moves no marker.
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
//! [`GUARD`], so a store opened before a newer build migrated stops at its next write.
//!
//! On a board of its own contract, a writer that has additive steps the board lacks adds them in
//! the same way, but only where it may move the board, and holds no writer: every build of the
//! contract keeps writing it. Where it may not (a project's live board it is not the build of), it
//! and every reader use the board as it is: the guard refuses, before anything is written, a write
//! that names a table of a step the board lacks; the database refuses a field it does not define;
//! and a SurrealDB board is STRICT, so no write makes a table it does not define. A whole-board
//! copy, its staging and a recovery's rebind are held to the build's own additive steps exactly,
//! and so is the whole-board read a copy is made from ([`crate::Store::dump`]): a board with steps
//! the build lacks would lose what they hold, one lacking its steps could not hold its rows.
//!
//! One exception, for the Symphony alone ([`COMPATIBLE`]): a store opened to read, by the rule
//! ([`Access::Read`]), writes a board one schema ahead of its own whose `writable_from` names its
//! schema or an older one, which the guard reads in each write's own transaction. Every store
//! opened to write, a store taken unchecked and every whole-board copy keep the rule above.

use crate::store::Program;

/// The board's contract: raised by a breaking step alone, one whose `DEFINE` changes or removes
/// what a build of the contract before reads or writes, or whatever else [`ADDED`] may not hold. A
/// board at an older version runs `DEFINE` once, and its `IF NOT EXISTS` adds what is new but leaves
/// every definition already there as it was. Changing one (a field's type, an assertion, an index)
/// also needs `DEFINE … OVERWRITE` for it. Protocol changes can require a wider version step and
/// retirement of old read tokens: an already-open store does not repeat its opening check. A
/// version listed in [`COMPATIBLE`] says its step keeps a Symphony one schema behind writing.
///
/// 28 is the contract's first: builds before it read a board's rows as no field more than they
/// know and replace some rows whole, so no step of theirs could be declared additive. From it on,
/// every build reads and writes past what it does not know ([`OPEN`]).
pub const VERSION: i64 = 28;

/// An additive step on the contract [`VERSION`]: its definitions, run after [`DEFINE`] and the
/// steps before it, and the board's `meta:additions` counts it once a build that may move the board
/// has run them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Addition {
    /// Its definitions on SurrealDB, each `IF NOT EXISTS`.
    pub surreal: &'static str,
    /// Its definitions on PostgreSQL, each `IF NOT EXISTS`.
    pub postgres: &'static str,
    /// The tables its step defines: a write naming one is refused on a board without the step.
    pub tables: &'static [&'static str],
    /// Why it is additive: what it adds, and how a build without it reads and writes around it,
    /// what no catalogue shows (docs/workflow/CHECKS.md, a schema step).
    pub why: &'static str,
}

/// This build's additive steps on [`VERSION`], in order: a breaking step empties it, its
/// definitions going into [`DEFINE`] and the PostgreSQL tables, as what they added is then the
/// contract's.
pub const ADDED: &[Addition] = &[
    Addition {
        surreal: "DEFINE TABLE IF NOT EXISTS setting SCHEMALESS;",
        postgres: "CREATE TABLE IF NOT EXISTS setting (id jsonb PRIMARY KEY, \
                   doc jsonb NOT NULL CHECK (jsonb_typeof(doc) = 'object'));",
        tables: &["setting"],
        why: "the table setting, the board's store of preferences (crate::settings): a build \
              without it never reads or writes the table and reads every setting from the files \
              alone, so a stored value holds only for builds that have the step, and no rule on a \
              table there before reads it",
    },
    Addition {
        surreal: "",
        postgres: "CREATE INDEX IF NOT EXISTS session_handle ON session (handle);",
        tables: &[],
        why: "an index of session by handle on PostgreSQL, where a write's actor, an inbox's \
              reader and a channel's session are read by handle, not by the row's id: a build \
              without it reads the same rows by scanning the table, and writes them as it did; \
              SurrealDB reads a session by its record id and has nothing to add",
    },
    PERSON_LINES,
];

/// The step that marks a person's line ([`crate::rows::MessageRow::person`]): the optional field
/// `person` on `message`, an open table, written on a line a person a board named wrote
/// ([`crate::Store::for_person`]) and on no other. It names no table of its own, so no write is
/// refused for naming one; a write for a person needs it ([`crate::Store::is_person`]).
pub const PERSON_LINES: Addition = Addition {
    surreal: "DEFINE FIELD IF NOT EXISTS person ON message TYPE option<bool>;",
    postgres: "ALTER TABLE message ADD COLUMN IF NOT EXISTS person boolean;",
    tables: &[],
    why: "the optional field person on message, true on a line a person a board named wrote: a \
          build without it writes a line with none, as every line before it was written, and \
          reads one with it past the field it does not know (OPEN), as the owner's word it does \
          not hear; a build with it writes the field only for a person, which needs the step, and \
          reads it on PostgreSQL through the row, so a board without it reads no person",
};

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
/// field it does not know, and every write of them keeps one, on both engines
/// (`tests/store/contract.rs`). The rest are closed: a field on one is a breaking step.
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
/// A step is listed only when it changes no table that existed before it, on either engine: no
/// field, column, index, event, trigger, function or assertion added to or changed on one. The
/// store's catalogue test holds a listed version to that on the database (`tests/store/catalogue.rs`,
/// over the committed definitions of each version); what it cannot see, a rule held in code that
/// the older Symphony's writes would skip, is its reviewer's to read against the line
/// ([CHECKS](../../../docs/workflow/CHECKS.md#a-schema-step-that-keeps-the-symphony-writing)). The schema
/// that brought this list and the record it writes ([`WRITABLE_FROM_SINCE`]) changed `meta` and is
/// not listed.
pub const COMPATIBLE: &[(i64, &str)] = &[];

/// The schema whose step brought `meta:schema`'s `writable_from`: a Symphony below it never reads
/// the record, and refuses every write on a board ahead of it whatever the record says.
pub const WRITABLE_FROM_SINCE: i64 = 25;
const _: () = assert!(WRITABLE_FROM_SINCE <= VERSION);

/// Whether `version`'s step keeps a Symphony one schema behind writing ([`COMPATIBLE`]).
pub fn keeps_writes(version: i64) -> bool {
    COMPATIBLE.iter().any(|(listed, _)| *listed == version)
}

/// Structural mutations plus all committed session changes, including deletions and re-creations.
/// The separate structural record keeps the retired legacy scalar outside this identity.
pub const BOARD_MARKER: &str = "((SELECT VALUE last FROM ONLY counter:board_structure) ?? 0) + ((SELECT VALUE math::sum(last) FROM session_revision GROUP ALL)[0] ?? 0)";

/// A board schema: its contract, its additive steps on it, and the steps that bring a board of any
/// older version to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Schema {
    version: i64,
    /// The tables each of its additive steps on `version` defines, in order ([`ADDED`]).
    added: Vec<Vec<String>>,
    /// What its steps run after [`DEFINE`]: nothing for this build's own schema. Held apart from
    /// it, so a program that opens a board without ever migrating one (a reader) links and
    /// copies none of `DEFINE`'s text.
    after: String,
    /// On PostgreSQL, what the steps add to this build's tables, which every migration there
    /// makes first: nothing for this build's own schema.
    postgres: String,
    /// Whether its own step keeps a Symphony one schema behind writing ([`COMPATIBLE`]).
    keeps: bool,
}

impl Schema {
    /// This build's: [`VERSION`], by [`DEFINE`], and on PostgreSQL by its own tables.
    pub fn this_build() -> Schema {
        Schema::of_this_contract(ADDITIONS)
    }

    /// This build's contract with its first `steps` additive steps ([`ADDED`]), all of them past
    /// their count: a build of the same contract before the rest.
    pub fn of_this_contract(steps: i64) -> Schema {
        let contract = Schema {
            version: VERSION,
            added: Vec::new(),
            after: String::new(),
            postgres: String::new(),
            keeps: keeps_writes(VERSION),
        };
        let taken = usize::try_from(steps.max(0)).unwrap_or(usize::MAX);
        (ADDED.iter().take(taken)).fold(contract, |schema, step| {
            schema.then_additive(step.surreal, step.postgres, step.tables)
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
    /// migration writes it beside the version (`meta:schema`'s `writable_from`).
    pub fn writable_from(&self) -> i64 {
        self.version - i64::from(self.keeps)
    }

    /// The schema one version on, `surreal` run after this one's steps on SurrealDB and
    /// `postgres` after them on PostgreSQL: a newer build's, whose step says nothing of the
    /// Symphony one behind, so keeps it read-only ([`Schema::keeping_writes`]).
    pub fn then_each(&self, surreal: &str, postgres: &str) -> Schema {
        Schema {
            version: self.version + 1,
            added: Vec::new(),
            after: format!("{}\n{surreal}", self.after),
            postgres: format!("{}\n{postgres}", self.postgres),
            keeps: false,
        }
    }

    /// The schema one version on, `step` run after this one's on SurrealDB, and nothing more on
    /// PostgreSQL: a newer build's.
    pub fn then(&self, step: &str) -> Schema {
        self.then_each(step, "")
    }

    /// This contract with one additive step more, `surreal` run after this one's steps on SurrealDB
    /// and `postgres` on PostgreSQL, defining `tables`: a build of the same contract with a step
    /// this one lacks ([`ADDED`]).
    pub fn then_additive(&self, surreal: &str, postgres: &str, tables: &[&str]) -> Schema {
        let mut added = self.added.clone();
        added.push(tables.iter().map(|table| (*table).into()).collect());
        Schema {
            added,
            after: format!("{}\n{surreal}", self.after),
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

    /// The steps: [`DEFINE`], then what this schema runs after it. Only a migration reads them.
    pub(crate) fn steps(&self) -> String {
        format!("{DEFINE}{}", self.after)
    }

    /// What the steps add on PostgreSQL to this build's tables.
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
    /// board behind it, or not made yet, is refused. Its writes still meet the [`guard`], so one
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

/// A write's refusal around the board's version, which only the [`guard`] reads at the write.
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

/// The statements every write transaction starts with: it throws the refusal (`$board_schema_*`,
/// [`guard_vars`]) unless the board is at this store's version, or, for a write that may
/// (`$board_schema_ahead`), one version ahead with a `writable_from` that is a number no greater
/// than this store's version ([`writes`]), before the write's own statements. `NONE` sorts below
/// every number, so the record is held to be an integer before it is compared. On a board of its
/// own contract it then throws (`$board_additions_*`) unless the board's additive steps are
/// within what the write takes ([`Steps`]): no fewer than the step of a table it names, and for a
/// whole-board copy or a rebind, exactly the store's own. A write for a person (`$by_person`,
/// [`crate::Store::for_person`]) throws `$by_person_session` when a session on the board, live or
/// left, holds that name as its handle: the person never writes as that session, nor marks its
/// lines theirs.
pub(crate) const GUARD: &str = "LET $board_schema_meta = (SELECT version, writable_from FROM ONLY meta:schema);
LET $board_schema_found = $board_schema_meta.version;
LET $board_schema_from = $board_schema_meta.writable_from;
IF $board_schema_found != $board_schema AND !($board_schema_ahead = true AND $board_schema_found = $board_schema + 1 AND type::is_int($board_schema_from) AND $board_schema_from <= $board_schema) { THROW string::concat($board_schema_before, <string> $board_schema_found, $board_schema_after) };
LET $board_additions_found = (SELECT VALUE version FROM ONLY meta:additions) ?? 0;
IF $board_schema_found = $board_schema AND ($board_additions_found < $board_additions_least OR $board_additions_found > $board_additions_most) { THROW string::concat($board_additions_before, <string> $board_additions_found, $board_additions_after) };
IF $by_person != NONE AND record::exists(type::record('session', $by_person)) { THROW $by_person_session };";

/// What a write takes of the board's additive steps on its contract, which the [`GUARD`] holds it
/// to: no fewer than `least`, no more than `most`, refused in the words `before`, the board's
/// count, then `after`.
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
/// `` `table` ``, ⟨table⟩) and wherever it is, in a statement nested in another or in a
/// subquery. Case is not looked at, so a word that only might be the table counts.
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

/// [`GUARD`]'s variables, for a store of version `own` refusing in `refusal`'s words, which takes a
/// board one ahead whose record lets it when `ahead`, and the additive steps `steps` on its own:
/// added to a write's own (`vars`, an object, or nothing).
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

/// Whether a write of a store of version `own` passes the [`GUARD`] on a board at `found` (none:
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
const MUTATING: [&str; 10] = [
    "ALTER", "CREATE", "DEFINE", "DELETE", "INSERT", "REBUILD", "RELATE", "REMOVE", "UPDATE",
    "UPSERT",
];

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

// Full-text index construction needs a committed analyzer, installed before the schema transaction.
pub const ANALYZERS: &str = "DEFINE ANALYZER IF NOT EXISTS task_words TOKENIZERS blank, class, punct FILTERS lowercase, ascii;";

/// The board's tables, indexes and events, run in one transaction by a store opening a board at
/// another [`VERSION`]; public for the board crate's `perf` tests, which plan what its events run.
pub const DEFINE: &str = "
DEFINE TABLE IF NOT EXISTS task SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS sid ON task TYPE int;
DEFINE FIELD IF NOT EXISTS slug ON task TYPE string;
DEFINE FIELD IF NOT EXISTS title ON task TYPE string;
DEFINE FIELD IF NOT EXISTS body ON task TYPE string;
DEFINE FIELD IF NOT EXISTS status ON task TYPE string
    ASSERT $value IN ['backlog', 'concept', 'design', 'ready', 'in_progress', 'review', 'done', 'blocked', 'cancelled'];
DEFINE FIELD IF NOT EXISTS priority ON task TYPE int;
DEFINE FIELD IF NOT EXISTS parent ON task TYPE option<string>;
DEFINE FIELD IF NOT EXISTS created_by ON task TYPE string;
DEFINE FIELD IF NOT EXISTS created ON task TYPE float;
DEFINE FIELD IF NOT EXISTS updated ON task TYPE float;
DEFINE FIELD IF NOT EXISTS instruction ON task TYPE bool DEFAULT false;
DEFINE FIELD IF NOT EXISTS version ON task TYPE int DEFAULT 0;
DEFINE FIELD IF NOT EXISTS fields ON task TYPE option<object> FLEXIBLE;
DEFINE INDEX IF NOT EXISTS task_sid ON task FIELDS sid UNIQUE;
DEFINE INDEX IF NOT EXISTS task_status ON task FIELDS status;
DEFINE INDEX IF NOT EXISTS task_parent ON task FIELDS parent;
DEFINE INDEX IF NOT EXISTS task_slug ON task FIELDS slug UNIQUE;
DEFINE INDEX IF NOT EXISTS task_title_search ON task FIELDS title FULLTEXT ANALYZER task_words BM25;
DEFINE INDEX IF NOT EXISTS task_body_search ON task FIELDS body FULLTEXT ANALYZER task_words BM25;
DEFINE INDEX IF NOT EXISTS task_slug_search ON task FIELDS slug FULLTEXT ANALYZER task_words BM25;

DEFINE TABLE IF NOT EXISTS task_embedding_source SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS slug ON task_embedding_source TYPE string;
DEFINE FIELD IF NOT EXISTS source ON task_embedding_source TYPE string;
DEFINE FIELD IF NOT EXISTS source_hash ON task_embedding_source TYPE string;
DEFINE FIELD IF NOT EXISTS model ON task_embedding_source TYPE string;
DEFINE FIELD IF NOT EXISTS parts ON task_embedding_source TYPE int;
DEFINE TABLE IF NOT EXISTS task_embedding_checkpoint SCHEMALESS;
DEFINE TABLE IF NOT EXISTS task_chunk SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS slug ON task_chunk TYPE string;
DEFINE FIELD IF NOT EXISTS source ON task_chunk TYPE string;
DEFINE FIELD IF NOT EXISTS source_hash ON task_chunk TYPE string;
DEFINE FIELD IF NOT EXISTS text_hash ON task_chunk TYPE string;
DEFINE FIELD IF NOT EXISTS model ON task_chunk TYPE string;
DEFINE FIELD IF NOT EXISTS text ON task_chunk TYPE string;
DEFINE FIELD IF NOT EXISTS embedding ON task_chunk TYPE array<float, 384>;
DEFINE INDEX IF NOT EXISTS task_chunk_source ON task_chunk FIELDS slug, source;
DEFINE INDEX IF NOT EXISTS task_embedding ON task_chunk FIELDS embedding HNSW DIMENSION 384 DIST COSINE TYPE F32;

DEFINE TABLE IF NOT EXISTS link SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS slug ON link TYPE string;
DEFINE FIELD IF NOT EXISTS kind ON link TYPE string ASSERT $value IN ['doc', 'dep'];
DEFINE FIELD IF NOT EXISTS target ON link TYPE string;
DEFINE INDEX IF NOT EXISTS link_slug ON link FIELDS slug;

DEFINE TABLE IF NOT EXISTS session SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS handle ON session TYPE string;
DEFINE FIELD IF NOT EXISTS kind ON session TYPE string;
DEFINE FIELD IF NOT EXISTS parent ON session TYPE option<string>;
DEFINE FIELD IF NOT EXISTS worktree ON session TYPE string;
DEFINE FIELD IF NOT EXISTS branch ON session TYPE string;
DEFINE FIELD IF NOT EXISTS model ON session TYPE string;
DEFINE FIELD IF NOT EXISTS pid ON session TYPE option<int>;
DEFINE FIELD IF NOT EXISTS host ON session TYPE string;
DEFINE FIELD IF NOT EXISTS affinity ON session TYPE string;
DEFINE FIELD IF NOT EXISTS connected ON session TYPE float;
DEFINE FIELD IF NOT EXISTS last_seen ON session TYPE float;
DEFINE FIELD IF NOT EXISTS left_at ON session TYPE option<float>;
DEFINE FIELD IF NOT EXISTS agent ON session TYPE option<string>;
DEFINE FIELD IF NOT EXISTS busy_until ON session TYPE option<float>;
DEFINE FIELD IF NOT EXISTS busy_call ON session TYPE option<string>;
DEFINE FIELD IF NOT EXISTS last_tool ON session TYPE option<float>;
DEFINE INDEX IF NOT EXISTS session_agent ON session FIELDS agent;

DEFINE TABLE IF NOT EXISTS session_revision SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS last ON session_revision TYPE int;
DEFINE FIELD IF NOT EXISTS writes ON session_revision TYPE option<int>;

DEFINE TABLE IF NOT EXISTS role SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS slug ON role TYPE string;
DEFINE FIELD IF NOT EXISTS handle ON role TYPE string;
DEFINE FIELD IF NOT EXISTS role ON role TYPE string ASSERT $value IN ['orchestrator', 'executor', 'reviewer', 'reserved'];
DEFINE FIELD IF NOT EXISTS since ON role TYPE float;
DEFINE FIELD IF NOT EXISTS expires ON role TYPE option<float>;
DEFINE INDEX IF NOT EXISTS role_slug ON role FIELDS slug;
DEFINE INDEX IF NOT EXISTS role_handle ON role FIELDS handle;
DEFINE INDEX IF NOT EXISTS role_role ON role FIELDS role;

DEFINE TABLE IF NOT EXISTS message SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS sid ON message TYPE int;
DEFINE FIELD IF NOT EXISTS slug ON message TYPE string;
DEFINE FIELD IF NOT EXISTS sender ON message TYPE string;
DEFINE FIELD IF NOT EXISTS recipient ON message TYPE option<string>;
DEFINE FIELD IF NOT EXISTS kind ON message TYPE string ASSERT $value IN ['system', 'msg', 'verdict'];
DEFINE FIELD IF NOT EXISTS text ON message TYPE string;
DEFINE FIELD IF NOT EXISTS ts ON message TYPE float;
DEFINE INDEX IF NOT EXISTS message_text_search ON message FIELDS text FULLTEXT ANALYZER task_words BM25;
DEFINE INDEX IF NOT EXISTS message_sid ON message FIELDS sid;
DEFINE INDEX IF NOT EXISTS message_ts ON message FIELDS ts;
DEFINE INDEX IF NOT EXISTS message_recipient ON message FIELDS recipient, sid;
DEFINE INDEX IF NOT EXISTS message_sender ON message FIELDS sender, kind, sid, slug;

DEFINE TABLE IF NOT EXISTS said SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS sid ON said TYPE int;
DEFINE FIELD IF NOT EXISTS slug ON said TYPE string;

DEFINE TABLE IF NOT EXISTS delivery SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS message ON delivery TYPE int;
DEFINE FIELD IF NOT EXISTS handle ON delivery TYPE string;
DEFINE FIELD IF NOT EXISTS acked ON delivery TYPE float;
DEFINE INDEX IF NOT EXISTS delivery_handle ON delivery FIELDS handle, message;

DEFINE TABLE IF NOT EXISTS delivery_key SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS slug ON delivery_key TYPE string;
DEFINE FIELD IF NOT EXISTS key ON delivery_key TYPE string;
DEFINE FIELD IF NOT EXISTS message ON delivery_key TYPE int;
DEFINE FIELD IF NOT EXISTS ts ON delivery_key TYPE float;

DEFINE TABLE IF NOT EXISTS barred_key SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS key ON barred_key TYPE string;

DEFINE TABLE IF NOT EXISTS task_session SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS slug ON task_session TYPE string;
DEFINE FIELD IF NOT EXISTS handle ON task_session TYPE string;
DEFINE FIELD IF NOT EXISTS role ON task_session TYPE string;
DEFINE FIELD IF NOT EXISTS claude_session ON task_session TYPE string;
DEFINE FIELD IF NOT EXISTS worktree ON task_session TYPE string;
DEFINE FIELD IF NOT EXISTS branch ON task_session TYPE string;
DEFINE FIELD IF NOT EXISTS pane ON task_session TYPE string;
DEFINE FIELD IF NOT EXISTS model ON task_session TYPE string;
DEFINE FIELD IF NOT EXISTS started ON task_session TYPE float;
DEFINE FIELD IF NOT EXISTS closed ON task_session TYPE option<float>;
DEFINE FIELD IF NOT EXISTS closed_why ON task_session TYPE string;
DEFINE FIELD IF NOT EXISTS restore ON task_session TYPE string;
DEFINE INDEX IF NOT EXISTS task_session_slug ON task_session FIELDS slug;
DEFINE INDEX IF NOT EXISTS task_session_branch ON task_session FIELDS branch;

DEFINE TABLE IF NOT EXISTS counter SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS last ON counter TYPE int;
DEFINE FIELD IF NOT EXISTS ts ON counter TYPE option<float>;

DEFINE TABLE IF NOT EXISTS moved SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS board ON moved TYPE string;
DEFINE FIELD IF NOT EXISTS at ON moved TYPE float;
DEFINE FIELD IF NOT EXISTS rows ON moved TYPE string;

DEFINE TABLE IF NOT EXISTS meta SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS version ON meta TYPE int;
-- Schema 25: the oldest schema whose Symphony may write the board, which a migration writes.
DEFINE FIELD IF NOT EXISTS writable_from ON meta TYPE option<int>;

DEFINE TABLE IF NOT EXISTS writer SCHEMALESS;

DEFINE TABLE IF NOT EXISTS artifact_ledger SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS revision ON artifact_ledger TYPE int;
DEFINE FIELD IF NOT EXISTS body ON artifact_ledger TYPE object FLEXIBLE;

-- Schema 22: the board's recovery generation, which a copy replaces and a guarded write reads.
DEFINE TABLE IF NOT EXISTS recovery SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS generation ON recovery TYPE string;
IF !record::exists(recovery:board) { CREATE recovery:board SET generation = <string> rand::uuid::v4() RETURN NONE; };

-- Schema 24: custom delivery workflows' records, and the fences on them (`store/workflow.rs`).
DEFINE TABLE IF NOT EXISTS workflow SCHEMAFULL;
DEFINE FIELD OVERWRITE kind ON workflow TYPE string
    ASSERT $value IN ['active', 'definition', 'activation', 'event', 'receipt', 'intent', 'authority', 'admission'];
DEFINE FIELD IF NOT EXISTS body ON workflow TYPE object FLEXIBLE;
DEFINE FIELD IF NOT EXISTS at ON workflow TYPE float;
-- Schema 27: a position goes beside a return's transition (`Store::return_to_default`).
DEFINE EVENT OVERWRITE workflow_position ON task
    WHEN $event != 'DELETE' AND (($before.fields._board.workflow ?? NONE) != ($after.fields._board.workflow ?? NONE)
        OR (($before.fields._board.workflow ?? NONE) != NONE AND $before.status != $after.status)
        OR (($after.fields._board.workflow ?? NONE) = NONE AND $before.status IN ['done', 'cancelled']
            AND $after.status NOT IN ['done', 'cancelled']))
    THEN {
    LET $w = $after.fields._board.workflow ?? NONE;
    LET $was = $before.fields._board.workflow ?? NONE;
    IF $was = NONE AND $w = NONE {
        IF record::exists(workflow:['active']) {
            THROW string::concat('refused: explicit_action_required: ', $after.slug, ' has no workflow position, and a custom workflow is active: it leaves ', $before.status, ' only by a write that places it at its mapped pair');
        };
    } ELSE {
        IF $was != NONE AND $w = $was {
            THROW string::concat('refused: explicit_action_required: the status of ', $after.slug, ' follows its workflow position, which only its workflow edges move (just tasks workflow actions ', $after.slug, ')');
        };
        IF $w = NONE {
            LET $r = (SELECT VALUE body FROM ONLY type::record('workflow', ['event', $after.slug, $was.revision + 1]));
            IF $r = NONE OR $r.status != $after.status OR ($r.activation ?? NONE) = NONE
                OR (SELECT VALUE body.returned FROM ONLY type::record('workflow', ['activation', $r.activation])) != true {
                THROW string::concat('refused: workflow: ', $after.slug, ' keeps its workflow position: only an activation that returns the board to its own writers retires it, beside the transition recorded for it');
            };
        } ELSE {
            IF $was != NONE AND $w.revision != $was.revision + 1 {
                THROW string::concat('refused: workflow: ', $after.slug, ' moves from workflow revision ', <string>$was.revision, ' only to the next');
            };
            IF $was = NONE AND (SELECT VALUE kind FROM ONLY type::record('workflow', ['event', $after.slug, $w.revision + 1])) != NONE {
                THROW string::concat('refused: workflow: ', $after.slug, ' moved on from workflow revision ', <string>$w.revision, '; it never returns to a position it left');
            };
            LET $e =(SELECT VALUE body FROM ONLY type::record('workflow', ['event', $after.slug, $w.revision]));
            IF $e = NONE OR $e.slug != $after.slug OR $e.revision != $w.revision
                OR $e.to != { phase: $w.phase, state: $w.state } OR ($e.resume ?? NONE) != ($w.resume ?? NONE)
                OR $e.generation != $w.generation OR $e.digest != $w.digest OR $e.status != $after.status {
                THROW string::concat('refused: workflow: ', $after.slug, ' is at a workflow position only beside the transition recorded for it');
            };
        };
    };
};
DEFINE EVENT OVERWRITE workflow_history ON workflow
    WHEN $event = 'UPDATE' AND ($before.kind IN ['definition', 'activation', 'event', 'receipt', 'admission'] OR $before.kind != $after.kind
        OR ($before.kind = 'authority' AND $before.body.epoch = $after.body.epoch
            AND $after.body.generation < $before.body.generation))
    THEN {
    IF $before.kind = 'authority' AND $after.kind = 'authority' {
        THROW 'refused: workflow: the generation the board accepted from an approval authority is never lowered within its epoch';
    };
    THROW string::concat('refused: workflow: a recorded ', $before.kind, ' is never changed');
};
DEFINE EVENT OVERWRITE workflow_active ON workflow
    WHEN $event = 'UPDATE' AND $after.kind = 'active'
    THEN {
    LET $activation = (SELECT VALUE body FROM ONLY type::record('workflow', ['activation', $after.body.generation]));
    IF $activation = NONE OR $activation.digest != $after.body.digest
        OR $before.body.generation >= $after.body.generation {
        THROW 'refused: workflow: the active generation moves forward only to an activation recorded for it';
    };
};

DEFINE TABLE IF NOT EXISTS callback_state SCHEMALESS;
DEFINE INDEX IF NOT EXISTS callback_state_address ON callback_state FIELDS address;
DEFINE TABLE IF NOT EXISTS callback_lease SCHEMALESS;
DEFINE TABLE IF NOT EXISTS callback_event SCHEMALESS;
DEFINE INDEX IF NOT EXISTS callback_event_handle ON callback_event FIELDS handle, delivered;
DEFINE TABLE IF NOT EXISTS chat_event SCHEMALESS;

-- Schema 20 names the recovery reservations older SurrealDB builds made implicitly.
DEFINE TABLE IF NOT EXISTS unanswered_restore SCHEMALESS;

DEFINE TABLE IF NOT EXISTS ask SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS sid ON ask TYPE int;
DEFINE FIELD IF NOT EXISTS kind ON ask TYPE string ASSERT $value IN ['verdict', 'message', 'comment', 'chat', 'task'];
DEFINE FIELD IF NOT EXISTS about ON ask TYPE string;
DEFINE FIELD IF NOT EXISTS path ON ask TYPE option<string>;
DEFINE FIELD IF NOT EXISTS card ON ask TYPE option<string>;
DEFINE FIELD IF NOT EXISTS block ON ask TYPE option<string>;
DEFINE FIELD IF NOT EXISTS tree ON ask TYPE option<string>;
DEFINE FIELD IF NOT EXISTS task ON ask TYPE option<string>;
DEFINE FIELD IF NOT EXISTS thread ON ask TYPE option<string>;
DEFINE FIELD IF NOT EXISTS words ON ask TYPE string;
DEFINE FIELD IF NOT EXISTS asked ON ask TYPE float;
DEFINE FIELD IF NOT EXISTS lines ON ask TYPE int;
DEFINE FIELD IF NOT EXISTS last ON ask TYPE float;
DEFINE FIELD IF NOT EXISTS holder ON ask TYPE option<string>;
DEFINE FIELD IF NOT EXISTS held ON ask TYPE option<float>;
DEFINE FIELD IF NOT EXISTS before ON ask TYPE array<object>;
DEFINE FIELD IF NOT EXISTS before.*.handle ON ask TYPE string;
DEFINE FIELD IF NOT EXISTS before.*.since ON ask TYPE float;
DEFINE FIELD IF NOT EXISTS before.*.until ON ask TYPE float;
DEFINE FIELD IF NOT EXISTS before.*.why ON ask TYPE string;
DEFINE FIELD IF NOT EXISTS open ON ask TYPE bool;
DEFINE FIELD IF NOT EXISTS answered ON ask TYPE option<float>;
DEFINE FIELD IF NOT EXISTS answered_by ON ask TYPE option<string>;
DEFINE FIELD IF NOT EXISTS answer ON ask TYPE option<string>;
DEFINE FIELD IF NOT EXISTS closed ON ask TYPE bool;
DEFINE FIELD IF NOT EXISTS gone ON ask TYPE option<string>;
DEFINE FIELD IF NOT EXISTS gone_since ON ask TYPE option<float>;
DEFINE INDEX IF NOT EXISTS ask_sid ON ask FIELDS sid UNIQUE;
DEFINE INDEX IF NOT EXISTS ask_open ON ask FIELDS open, asked, sid;
DEFINE INDEX IF NOT EXISTS ask_about ON ask FIELDS about, open;
DEFINE INDEX IF NOT EXISTS ask_answered ON ask FIELDS answered;

DEFINE TABLE IF NOT EXISTS usage SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS handle ON usage TYPE string;
DEFINE FIELD IF NOT EXISTS slug ON usage TYPE string;
DEFINE FIELD IF NOT EXISTS role ON usage TYPE string;
DEFINE FIELD IF NOT EXISTS model ON usage TYPE string;
DEFINE FIELD IF NOT EXISTS speed ON usage TYPE string;
DEFINE FIELD IF NOT EXISTS round ON usage TYPE int;
DEFINE FIELD IF NOT EXISTS input ON usage TYPE int;
DEFINE FIELD IF NOT EXISTS cache_write_5m ON usage TYPE int;
DEFINE FIELD IF NOT EXISTS cache_write_1h ON usage TYPE int;
DEFINE FIELD IF NOT EXISTS cache_read ON usage TYPE int;
DEFINE FIELD IF NOT EXISTS output ON usage TYPE int;
DEFINE FIELD IF NOT EXISTS calls ON usage TYPE int;
DEFINE FIELD IF NOT EXISTS first ON usage TYPE float;
DEFINE FIELD IF NOT EXISTS last ON usage TYPE float;
DEFINE INDEX IF NOT EXISTS usage_slug ON usage FIELDS slug;
DEFINE INDEX IF NOT EXISTS usage_handle ON usage FIELDS handle;

-- Schema 23: execution attempts (`crate::execution`): an attempt's immutable identity beside its
-- revisioned state, its append-only facts, the fences its admission writes and its collected
-- artifacts. Schemaless and with no event: no reader caches them, and a write moves no marker.
DEFINE TABLE IF NOT EXISTS execution_attempt SCHEMALESS;
DEFINE INDEX IF NOT EXISTS execution_attempt_task ON execution_attempt FIELDS task, created;
DEFINE INDEX IF NOT EXISTS execution_attempt_due ON execution_attempt FIELDS open, due;
DEFINE TABLE IF NOT EXISTS execution_fact SCHEMALESS;
DEFINE INDEX IF NOT EXISTS execution_fact_attempt ON execution_fact FIELDS attempt, seq;
DEFINE TABLE IF NOT EXISTS execution_fence SCHEMALESS;
DEFINE TABLE IF NOT EXISTS execution_artifact SCHEMALESS;
DEFINE INDEX IF NOT EXISTS execution_artifact_attempt ON execution_artifact FIELDS attempt, name;

DEFINE EVENT OVERWRITE board_changed ON task WHEN $before != $after THEN { IF record::exists(counter:board_structure) { UPDATE counter:board_structure SET last = (last ?? 0) + 1 RETURN NONE; } ELSE { CREATE counter:board_structure SET last = 1 RETURN NONE; }; };
DEFINE EVENT OVERWRITE board_changed ON session WHEN $before != $after THEN {
    LET $revision = type::record('session_revision', record::id($after.id ?? $before.id));
    IF record::exists($revision) { UPDATE $revision SET last += 1 RETURN NONE; }
    ELSE { CREATE $revision SET last = 1 RETURN NONE; };
};
DEFINE EVENT OVERWRITE board_changed ON role WHEN $before != $after THEN { IF record::exists(counter:board_structure) { UPDATE counter:board_structure SET last = (last ?? 0) + 1 RETURN NONE; } ELSE { CREATE counter:board_structure SET last = 1 RETURN NONE; }; };
DEFINE EVENT OVERWRITE board_changed ON link WHEN $before != $after THEN { IF record::exists(counter:board_structure) { UPDATE counter:board_structure SET last = (last ?? 0) + 1 RETURN NONE; } ELSE { CREATE counter:board_structure SET last = 1 RETURN NONE; }; };
DEFINE EVENT OVERWRITE board_changed ON message WHEN $before != $after THEN { IF record::exists(counter:board_structure) { UPDATE counter:board_structure SET last = (last ?? 0) + 1 RETURN NONE; } ELSE { CREATE counter:board_structure SET last = 1 RETURN NONE; }; };
DEFINE EVENT OVERWRITE board_changed ON delivery WHEN $before != $after THEN { IF record::exists(counter:board_structure) { UPDATE counter:board_structure SET last = (last ?? 0) + 1 RETURN NONE; } ELSE { CREATE counter:board_structure SET last = 1 RETURN NONE; }; };
DEFINE EVENT OVERWRITE board_changed ON task_session WHEN $before != $after THEN { IF record::exists(counter:board_structure) { UPDATE counter:board_structure SET last = (last ?? 0) + 1 RETURN NONE; } ELSE { CREATE counter:board_structure SET last = 1 RETURN NONE; }; };
DEFINE EVENT OVERWRITE board_changed ON callback_state
    WHEN [$before.address, $before.owner, $before.host, $before.chat, $before.chat_fallback, $before.chat_resume, $before.confirmed_until, $before.in_process, $before.seen, $before.pull, $before.registration_revision]
      != [$after.address, $after.owner, $after.host, $after.chat, $after.chat_fallback, $after.chat_resume, $after.confirmed_until, $after.in_process, $after.seen, $after.pull, $after.registration_revision]
    THEN { IF record::exists(counter:board_structure) { UPDATE counter:board_structure SET last = (last ?? 0) + 1 RETURN NONE; } ELSE { CREATE counter:board_structure SET last = 1 RETURN NONE; }; };
DEFINE EVENT OVERWRITE board_changed ON chat_event WHEN $before != $after THEN { IF record::exists(counter:board_structure) { UPDATE counter:board_structure SET last = (last ?? 0) + 1 RETURN NONE; } ELSE { CREATE counter:board_structure SET last = 1 RETURN NONE; }; };
DEFINE EVENT OVERWRITE board_changed ON ask WHEN $before != $after THEN { IF record::exists(counter:board_structure) { UPDATE counter:board_structure SET last = (last ?? 0) + 1 RETURN NONE; } ELSE { CREATE counter:board_structure SET last = 1 RETURN NONE; }; };

DEFINE EVENT OVERWRITE said ON message
    WHEN $event != 'UPDATE' OR [$before.sender, $before.sid, $before.slug, $before.kind IN ['msg', 'verdict']]
        != [$after.sender, $after.sid, $after.slug, $after.kind IN ['msg', 'verdict']]
    THEN {
    IF $event != 'CREATE' {
        LET $was = (SELECT sid, slug FROM ONLY type::record('said', [$before.sender]));
        IF $was.sid = $before.sid AND $was.slug = $before.slug {
            LET $sender = $before.sender;
            LET $newest = array::max(array::concat(
                (SELECT VALUE [sid, slug] FROM message WHERE sender = $sender AND kind = 'msg' ORDER BY sid DESC, slug DESC LIMIT 1),
                (SELECT VALUE [sid, slug] FROM message WHERE sender = $sender AND kind = 'verdict' ORDER BY sid DESC, slug DESC LIMIT 1)
            ));
            IF $newest = NONE {
                DELETE type::record('said', [$before.sender]) RETURN NONE;
            } ELSE {
                UPDATE type::record('said', [$before.sender]) SET sid = $newest[0], slug = $newest[1] RETURN NONE;
            };
        };
    };
    IF $event != 'DELETE' AND $after.kind IN ['msg', 'verdict'] {
        IF record::exists(type::record('said', [$after.sender])) {
            UPDATE type::record('said', [$after.sender]) SET sid = $after.sid, slug = $after.slug
                WHERE sid < $after.sid OR (sid = $after.sid AND slug < $after.slug) RETURN NONE;
        } ELSE {
            CREATE type::record('said', [$after.sender]) SET sid = $after.sid, slug = $after.slug RETURN NONE;
        };
    };
};
IF (SELECT VALUE id FROM said LIMIT 1) = [] {
    FOR $spoke IN (SELECT sender, array::max(array::group([[sid, slug]])) AS last FROM message WHERE kind IN ['msg', 'verdict'] GROUP BY sender) {
        LET $newest = $spoke.last[0];
        CREATE type::record('said', [$spoke.sender]) SET sid = $newest[0], slug = $newest[1] RETURN NONE;
    };
};

// Transfer the structural count once. Session revisions remain independent and are not reset.
// A retired legacy token without its successor is corruption, never an empty board.
IF !record::exists(counter:board_structure) {
    LET $legacy_marker = (SELECT VALUE last FROM ONLY counter:board) ?? 0;
    IF $legacy_marker < 0 { THROW 'refused: structural marker missing after legacy marker retirement'; };
    CREATE counter:board_structure SET last = $legacy_marker RETURN NONE;
};
// Old cached readers decode this scalar as u64 before consulting their cache. A finite signed
// retirement makes them fail closed; current events never write this record again.
IF record::exists(counter:board) {
    UPDATE counter:board SET last = -9223372036854775807 - 1 RETURN NONE;
} ELSE {
    CREATE counter:board SET last = -9223372036854775807 - 1 RETURN NONE;
};
";

#[cfg(test)]
#[path = "schema_tests.rs"]
mod tests;
