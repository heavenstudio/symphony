//! [`Home::reclaim`]: the board's store rebuilt from an export of itself, for a store that grew
//! before every server kept its values in its sstables ([`super::Storage`]).

use super::snapshot::Before;
use super::storage::store;
use super::{DATA, Doing, Home, Kept, OLD, RECLAIM, Running, ServerError, Starters, io};
use crate::live::seen;
use crate::rpc;
use crate::sys::{command, output};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::net::SocketAddr;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// What [`Home::reclaim`] did.
#[derive(Debug, Clone, PartialEq)]
pub struct Reclaimed {
    /// The bytes the board's store took on disk before, and takes now.
    pub before: u64,
    pub after: u64,
    /// Every database carried, as `namespace/database`.
    pub databases: Vec<String>,
    /// Where the exports stay, one SurrealQL file a database, named by its place among them and its
    /// names (`0002.ns.tasks.surql` for database `tasks` of namespace `ns`).
    pub exports: PathBuf,
    /// The old store, kept as a snapshot before any server of the reclaim started on it
    /// ([`Home::keep`]): kept as any snapshot is, until the owner removes it while its log holds
    /// writes, else among the newest [`super::snapshot::KEEP`] whose log is empty.
    pub kept: Kept,
    /// The board's server, on the new store.
    pub running: Running,
}

/// A store's databases, and the users and accesses of its root and of each namespace, which a
/// database's export does not hold.
#[derive(Debug, PartialEq)]
pub(super) struct Layout {
    pub(super) databases: Vec<(String, String)>,
    definitions: BTreeMap<String, Value>,
}

impl Home {
    /// Rebuild the board's store from an export of itself, which holds only what is live. Under the
    /// starters' lock: end the board's server, recorded or not; keep the store as a snapshot
    /// ([`Home::keep`], a clone, or a plain copy where the volume cannot clone), refused when it can
    /// be neither; start a server on the store that is recorded nowhere, and export every database
    /// it holds with SurrealDB's own `export`; import them into a new store, started with this
    /// board's [`super::Storage`]; export that one again and compare each export byte for byte, and
    /// the root's and every namespace's users and accesses; then swap the stores, start the board's
    /// server on the new one, and remove the old, which the snapshot keeps. Until the swap the old
    /// store is untouched, and a failure starts the board's server on it again; a failure after it
    /// ends every server on the new store, then moves the old one back and does the same. The
    /// exports stay in `reclaim/exports/`, until the next reclaim. Refused while the board is held
    /// ([`Home::hold`]). A board below the record of the schema it was seen at beside its store
    /// ([`seen::FILE`]) is refused in the rollback's words: before its running server is ended,
    /// which is left serving, and with none running, on the reclaim's own server on the store,
    /// once the store is kept.
    pub fn reclaim(&self) -> Result<Reclaimed, ServerError> {
        self.require_surreal()?;
        if !self.surreal.is_file() {
            return Err(self.no_surreal());
        }
        let held = self.lock(Doing::Reclaiming)?;
        self.require_surreal()?;
        self.unheld()?;
        let work = self.dir.join(RECLAIM);
        let data = self.dir.join(DATA);
        self.finish_swap()?;
        // A board that went back is refused while its server still runs, before the halt's clean
        // stop could remove a log segment its store never applied.
        let serving = self.serving();
        if let Some(running) = &serving {
            self.refuse_went_back(running)?;
        }
        let halted = self.halt()?;
        // The store is kept before any server of the reclaim starts on it: the old store it removes
        // once the new one is in place, and with none running, one that may have ended uncleanly or
        // gone back, whose log the reclaim's own server's clean stop would remove.
        let kept = match self.keep(crate::store::now(), Before::Reclaim) {
            Ok(kept) => kept,
            Err(problem) => return Err(self.serve_again(&held, problem)),
        };
        let before = bytes(&data);
        if work.exists() {
            fs::remove_dir_all(&work).map_err(io("could not remove", &work))?;
        }
        let (exports, check, fresh) = (work.join("exports"), work.join("check"), work.join(DATA));
        for folder in [&exports, &check] {
            fs::create_dir_all(folder).map_err(io("could not make", folder))?;
        }
        let databases = match self.rebuild(&data, &fresh, &exports, &check, &halted) {
            Ok(databases) => databases,
            Err(problem) => {
                let _ = fs::remove_dir_all(&fresh);
                return Err(self.serve_again(&held, problem));
            }
        };
        let old = work.join(OLD);
        if let Err(problem) = fs::rename(&data, &old) {
            return Err(self.serve_again(
                &held,
                ServerError::Reclaim(format!("could not move {}: {problem}", data.display())),
            ));
        }
        let running = match fs::rename(&fresh, &data)
            .map_err(io("could not move", &fresh))
            .and_then(|()| self.start(&held))
        {
            Ok(running) => running,
            Err(problem) => {
                // No server on the new store outlives its move aside: one there would be adopted
                // later in place of the old store's, and what was written meanwhile lost with it.
                if let Err(running_on) = self.halt() {
                    return Err(ServerError::NotUp(format!(
                        "the reclaim's new store is the board's at {}, its old store is at {}, and a \
                         server on the new one did not end: {running_on}. The board's server did not \
                         start on the new store: {problem}",
                        data.display(),
                        old.display()
                    )));
                }
                if data.exists() {
                    fs::rename(&data, &fresh).map_err(io("could not move", &data))?;
                }
                fs::rename(&old, &data).map_err(io("could not move", &old))?;
                return Err(self.serve_again(&held, ServerError::Reclaim(problem.to_string())));
            }
        };
        fs::remove_dir_all(&old).map_err(io("could not remove", &old))?;
        fs::remove_dir_all(&check).map_err(io("could not remove", &check))?;
        Ok(Reclaimed {
            before,
            after: bytes(&data),
            databases,
            exports,
            kept,
            running,
        })
    }

    /// Why a reclaim stopped, the old store in place, once the board's server is started on that
    /// store again; with the start's own failure beside it, if it fails too.
    fn serve_again(&self, starters: &Starters, problem: ServerError) -> ServerError {
        let Err(again) = self.start(starters) else {
            return problem;
        };
        let why = match problem {
            ServerError::Reclaim(why) => why,
            other => other.to_string(),
        };
        ServerError::NotUp(format!(
            "the reclaim stopped with the board's old store in place, untouched, and the board's \
             server did not start on it again: {again}. The reclaim stopped because: {why}"
        ))
    }

    /// Refused, in the rollback's words, when the board `running` serves reads below the record of
    /// the schema it was seen at beside its store ([`seen::FILE`] in this folder: the live board's,
    /// or a copy's beside the copied store); nothing is read with no record.
    pub(super) fn refuse_went_back(&self, running: &Running) -> Result<(), ServerError> {
        let record = self.dir.join(seen::FILE);
        if seen::read_at(&record).is_none() {
            return Ok(());
        }
        let client = rpc::Client::new(&running.addr, crate::NAMESPACE, crate::DATABASE);
        let database = crate::Database::named(&client).map_err(ServerError::Reclaim)?;
        let found = crate::Store::unchecked_on(
            database,
            Box::new(crate::store::now),
            Box::new(|_| true),
            "",
            &crate::Opening::write(),
        )
        .board_version()
        .map_err(|e| {
            ServerError::Reclaim(format!(
                "the schema of the board at {} could not be read: {e}",
                running.addr
            ))
        })?;
        let common = self.dir.parent().unwrap_or(&self.dir);
        match seen::went_back_in(&record, found, || crate::live::project_of(common)) {
            Some(back) => Err(ServerError::Reclaim(back.refusal())),
            None => Ok(()),
        }
    }

    /// What a reclaim cut short between its two moves left: the old store put back where the new
    /// one never arrived. One cut later leaves the old store beside the new, in the work folder the
    /// reclaim removes next. Under the lock.
    fn finish_swap(&self) -> Result<(), ServerError> {
        let (old, data) = (self.dir.join(RECLAIM).join(OLD), self.dir.join(DATA));
        if old.exists() && !data.exists() {
            fs::rename(&old, &data).map_err(io("could not move", &old))?;
        }
        Ok(())
    }

    /// Stop the board's server cleanly, recorded or found on its data, and forget its address
    /// ([`Home::stop_serving`]): the ports the record named and each stopped server had, which a
    /// client may still hold as the board's. Under the lock.
    pub(super) fn halt(&self) -> Result<Vec<u16>, ServerError> {
        let recorded = self.recorded();
        let stopped = self.stop_serving()?;
        Ok(recorded
            .iter()
            .chain(&stopped)
            .filter_map(|running| running.addr.parse::<SocketAddr>().ok())
            .map(|addr| addr.port())
            .collect())
    }

    /// Export every database of the store at `data` into `exports`, import each into a new store at
    /// `fresh`, and export that one into `check`: the databases, once every pair of exports and the
    /// two stores' layouts are the same. A store without the board's own database is refused. Each
    /// server is started on none of the `halted` ports, so a client still holding the board's old
    /// address writes into neither store, and is ended before this returns.
    fn rebuild(
        &self,
        data: &Path,
        fresh: &Path,
        exports: &Path,
        check: &Path,
        halted: &[u16],
    ) -> Result<Vec<String>, ServerError> {
        let old = self.spawn(&store(data), halted)?;
        let board = (crate::NAMESPACE.to_string(), crate::DATABASE.to_string());
        let read = self
            .refuse_went_back(&old)
            .and_then(|()| layout(&old))
            .and_then(|held| {
                if !held.databases.contains(&board) {
                    let named: Vec<String> = held
                        .databases
                        .iter()
                        .map(|(ns, db)| format!("{ns}/{db}"))
                        .collect();
                    return Err(ServerError::Reclaim(format!(
                        "the store at {} holds no {}/{}, the board's own database; it holds [{}]",
                        data.display(),
                        board.0,
                        board.1,
                        named.join(", ")
                    )));
                }
                for (at, (ns, db)) in held.databases.iter().enumerate() {
                    self.transfer("export", &old, ns, db, &exports.join(file(at, ns, db)))?;
                }
                Ok(held)
            });
        self.end(old.pid)?;
        let held = read?;
        let new = self.spawn(&store(fresh), halted)?;
        let built = (|| {
            for (at, (ns, db)) in held.databases.iter().enumerate() {
                self.transfer("import", &new, ns, db, &exports.join(file(at, ns, db)))?;
            }
            for (at, (ns, db)) in held.databases.iter().enumerate() {
                let name = file(at, ns, db);
                let (first, again) = (exports.join(&name), check.join(&name));
                self.transfer("export", &new, ns, db, &again)?;
                let read = |path: &Path| fs::read(path).map_err(io("could not read", path));
                if !same_export(&read(&first)?, &read(&again)?) {
                    return Err(ServerError::Reclaim(format!(
                        "{ns}/{db} exported from the new store is not its export from the old: {} against {}",
                        again.display(),
                        first.display()
                    )));
                }
            }
            let rebuilt = layout(&new)?;
            let missing: Vec<String> = held
                .databases
                .iter()
                .filter(|pair| !rebuilt.databases.contains(pair))
                .map(|(ns, db)| format!("{ns}/{db}"))
                .collect();
            if !missing.is_empty() {
                return Err(ServerError::Reclaim(format!(
                    "the new store lacks {}",
                    missing.join(", ")
                )));
            }
            for (scope, defined) in &held.definitions {
                if rebuilt.definitions.get(scope) != Some(defined) {
                    return Err(ServerError::Reclaim(format!(
                        "the users and accesses of {} differ in the new store: {defined} against {}",
                        if scope.is_empty() { "the root" } else { scope },
                        rebuilt.definitions.get(scope).unwrap_or(&Value::Null)
                    )));
                }
            }
            Ok(())
        })();
        self.end(new.pid)?;
        built.map(|()| {
            held.databases
                .iter()
                .map(|(ns, db)| format!("{ns}/{db}"))
                .collect()
        })
    }

    /// SurrealDB's own `export` or `import` (`verb`) of database `ns`/`db` on `running`, to or from
    /// `file`.
    fn transfer(
        &self,
        verb: &str,
        running: &Running,
        ns: &str,
        db: &str,
        file: &Path,
    ) -> Result<(), ServerError> {
        let endpoint = format!("http://{}", running.addr);
        let out = output(
            command(&self.surreal)
                .args([
                    verb,
                    "--endpoint",
                    &endpoint,
                    "--namespace",
                    ns,
                    "--database",
                    db,
                ])
                .arg(file),
        )
        .map_err(io(&format!("could not run {verb} with"), &self.surreal))?;
        if out.status.success() {
            return Ok(());
        }
        Err(ServerError::Reclaim(format!(
            "{verb} of {ns}/{db} at {} ({}) failed: {}",
            running.addr,
            file.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

/// Whether `again`, a database's export from the reclaim's new store, says what `first`, its export
/// from the old one, says: byte for byte, but for a line either repeats that defines an array
/// field's elements ([`element_definition`]), which is read once.
///
/// That repeat is SurrealDB's (3.3.0), made by its import. For a field of `TYPE array<T>` SurrealDB
/// defines the elements, `NAME.*`, itself. When NAME is a SurrealQL word, which it prints escaped
/// (`` `before` ``), it keeps that definition apart from where a `DEFINE FIELD` of `` `before`.* ``
/// keeps one: an export prints it at the head of the table's fields, not after the array field's
/// line as it prints `embedding.*`. An import runs both lines, the element line and the array
/// field's, so the new store holds both definitions and its export prints `` `before`.* `` twice,
/// the second time after the array field's line. Schema 17's `before ON ask` (`TYPE array<object>`)
/// is the board's first array field named by a word. The two lines are one text, one type for the
/// same elements, so the store with both reads and writes every row as the store with one does
/// (`INFO FOR TABLE` names one), and its own export, imported again, keeps both and prints them as
/// it did.
///
/// Nothing else is read past: a row, a repeated row, any other definition, repeated or not, and an
/// element definition whose text differs from the one before it, are each compared byte for byte, so
/// a store that lost a row or a definition is still refused.
fn same_export(first: &[u8], again: &[u8]) -> bool {
    said_once(first).eq(said_once(again))
}

/// The lines of `export` (split at each `\n`, so the sequence is the bytes), each repeat of an
/// [`element_definition`] already said left out.
fn said_once(export: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut defined = std::collections::HashSet::new();
    export
        .split(|byte| *byte == b'\n')
        .filter(move |line| !element_definition(line) || defined.insert(*line))
}

/// Whether `line` of an export is SurrealDB's definition of an array field's elements:
/// `DEFINE FIELD OVERWRITE NAME.* ON TABLE …`, the field's name ending in `.*`.
fn element_definition(line: &[u8]) -> bool {
    let Some(rest) = line.strip_prefix(b"DEFINE FIELD OVERWRITE ") else {
        return false;
    };
    rest.windows(4)
        .position(|word| word == b" ON ")
        .is_some_and(|end| rest[..end].ends_with(b".*"))
}

/// The file of database `ns`/`db`, the `at`-th of a store's sorted databases, in a reclaim's
/// `exports/` and `check/`: `0002.ns.tasks.surql`. The place keeps every database's file its own on
/// a filesystem that folds case, as macOS's does: by their names alone `sf/CaseProbe` and
/// `sf/caseprobe` would share one file, the second export overwriting the first, and both imports
/// and both checks would read the same database.
fn file(at: usize, ns: &str, db: &str) -> String {
    format!("{at:04}.{ns}.{db}.surql")
}

/// The bytes `path` takes on disk, every file in it counted by the blocks it holds, as `du` counts
/// them: what a reclaim frees, measured. A clone's shared blocks count in each copy.
pub fn bytes(path: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return 0;
    };
    let own = meta.blocks() * 512;
    if !meta.is_dir() {
        return own;
    }
    fs::read_dir(path).map_or(own, |entries| {
        own + entries
            .filter_map(Result::ok)
            .map(|entry| bytes(&entry.path()))
            .sum::<u64>()
    })
}

/// The databases the server `running` holds, sorted, with the users and accesses of its root (the
/// empty scope) and of each namespace, refused as [`names`] refuses an answer. Each statement runs
/// in no namespace or database: one about the server itself, or one that enters its own with `USE`.
pub(super) fn layout(running: &Running) -> Result<Layout, ServerError> {
    let client = rpc::Client::new(&running.addr, crate::NAMESPACE, crate::DATABASE);
    let scoped = false;
    let ask = |sql: &str| {
        client.call(sql, json!({}), scoped).map_err(|problem| {
            ServerError::Reclaim(format!(
                "could not read {sql:?} at {}: {problem}",
                running.addr
            ))
        })
    };
    let defined = |info: &Value| json!({"users": info["users"], "accesses": info["accesses"]});
    let root = ask("INFO FOR ROOT;")?.pop().unwrap_or(Value::Null);
    let mut layout = Layout {
        databases: Vec::new(),
        definitions: BTreeMap::from([(String::new(), defined(&root))]),
    };
    for ns in names(&root, "namespaces", "the root", &running.addr)? {
        let info = ask(&format!("USE NS {ns}; INFO FOR NS;"))?
            .pop()
            .unwrap_or(Value::Null);
        for db in names(&info, "databases", &ns, &running.addr)? {
            layout.databases.push((ns.clone(), db));
        }
        layout.definitions.insert(ns, defined(&info));
    }
    layout.databases.sort_unstable();
    Ok(layout)
}

/// The names under `field` of `info`, the `INFO` answer for `scope` at `addr`. An answer without
/// that object is refused, never read as none: a reclaim would carry nothing and remove the store.
/// So is a name the board's RPC could not say.
fn names(info: &Value, field: &str, scope: &str, addr: &str) -> Result<Vec<String>, ServerError> {
    let Some(named) = info.get(field).and_then(Value::as_object) else {
        return Err(ServerError::Reclaim(format!(
            "INFO for {scope} at {addr} answered no {field} object: {info}"
        )));
    };
    match named.keys().find(|name| !crate::db::plain(name)) {
        Some(name) => Err(ServerError::Reclaim(format!(
            "{name:?} at {addr} is not a plain name for a namespace or database"
        ))),
        None => Ok(named.keys().cloned().collect()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refusal(info: &Value, field: &str) -> String {
        names(info, field, "the root", "127.0.0.1:1")
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn an_info_answer_without_its_names_is_refused_never_read_as_none() {
        for info in [
            json!({"users": {}, "accesses": {}}),
            json!({"namespaces": null}),
            json!({"namespaces": ["sf"]}),
            Value::Null,
        ] {
            let said = refusal(&info, "namespaces");
            assert!(
                said.contains("INFO for the root at 127.0.0.1:1 answered no namespaces object"),
                "{said}"
            );
        }
    }

    #[test]
    fn databases_whose_names_differ_only_in_case_never_share_a_file_where_case_folds() {
        let held = [
            ("sf", "CaseProbe"),
            ("sf", "caseprobe"),
            ("SF", "tasks"),
            ("sf", "tasks"),
        ];
        let folded: std::collections::BTreeSet<String> = held
            .iter()
            .enumerate()
            .map(|(at, (ns, db))| file(at, ns, db).to_lowercase())
            .collect();
        assert_eq!(folded.len(), held.len(), "{folded:?}");
        assert_eq!(file(2, "ns", "tasks"), "0002.ns.tasks.surql");
    }

    /// An export of a table with an array field named by a word, `before`, as SurrealDB 3.3.0 prints
    /// it from a store whose schema defined it: its elements' line at the head of the fields.
    const ELEMENTS: &str = "DEFINE FIELD OVERWRITE `before`.* ON ask TYPE object PERMISSIONS FULL;";
    const OLD: &str = "OPTION IMPORT;\n\n\
        DEFINE TABLE ask TYPE NORMAL SCHEMAFULL PERMISSIONS NONE;\n\n\
        DEFINE FIELD OVERWRITE `before`.* ON ask TYPE object PERMISSIONS FULL;\n\
        DEFINE FIELD OVERWRITE about ON ask TYPE string PERMISSIONS FULL;\n\
        DEFINE FIELD OVERWRITE `before` ON ask TYPE array<object> PERMISSIONS FULL;\n\
        DEFINE FIELD OVERWRITE `before`.*.handle ON ask TYPE string PERMISSIONS FULL;\n\
        DEFINE FIELD OVERWRITE embedding ON ask TYPE array<float, 384> PERMISSIONS FULL;\n\
        DEFINE FIELD OVERWRITE embedding.* ON ask TYPE float PERMISSIONS FULL;\n\n\
        INSERT [ { about: 'x', before: [{ handle: 'quiet-plover' }], id: ask:1 } ];\n\
        INSERT [ { about: 'y', before: [], id: ask:2 } ];\n";

    /// `export` with `line` put after its line `after`, which it holds once.
    fn with_after(export: &str, after: &str, line: &str) -> String {
        assert_eq!(export.matches(after).count(), 1, "{after}");
        export.replacen(after, &format!("{after}\n{line}"), 1)
    }

    #[test]
    fn an_export_of_the_new_store_repeating_an_array_field_s_elements_says_what_the_old_one_said() {
        let array = "DEFINE FIELD OVERWRITE `before` ON ask TYPE array<object> PERMISSIONS FULL;";
        let again = with_after(OLD, array, ELEMENTS);
        assert_eq!(again.matches(ELEMENTS).count(), 2);
        assert!(same_export(OLD.as_bytes(), again.as_bytes()));
        // A store reclaimed once holds both, and so does its next export: the same either way.
        assert!(same_export(again.as_bytes(), again.as_bytes()));
        assert!(same_export(again.as_bytes(), OLD.as_bytes()));
        assert!(same_export(OLD.as_bytes(), OLD.as_bytes()));
        let elements = "DEFINE FIELD OVERWRITE embedding.* ON ask TYPE float PERMISSIONS FULL;";
        assert!(same_export(
            OLD.as_bytes(),
            with_after(OLD, elements, elements).as_bytes()
        ));
    }

    #[test]
    fn an_export_that_lost_or_repeated_anything_else_or_changed_a_byte_is_not_the_same() {
        let old = OLD.as_bytes();
        let row = "INSERT [ { about: 'y', before: [], id: ask:2 } ];";
        let field = "DEFINE FIELD OVERWRITE about ON ask TYPE string PERMISSIONS FULL;";
        let nested =
            "DEFINE FIELD OVERWRITE `before`.*.handle ON ask TYPE string PERMISSIONS FULL;";
        let other_type = "DEFINE FIELD OVERWRITE `before`.* ON ask TYPE string PERMISSIONS FULL;";
        for (why, again) in [
            ("a row lost", OLD.replacen(&format!("{row}\n"), "", 1)),
            ("a row said twice", with_after(OLD, row, row)),
            ("a field said twice", with_after(OLD, field, field)),
            ("a nested field said twice", with_after(OLD, nested, nested)),
            (
                "the elements' only line lost",
                OLD.replacen(&format!("{ELEMENTS}\n"), "", 1),
            ),
            ("other elements", with_after(OLD, field, other_type)),
            (
                "a byte changed",
                OLD.replacen("quiet-plover", "quiet-plovers", 1),
            ),
            ("a line ending lost", OLD.trim_end().to_string()),
            (
                "lines joined",
                OLD.replacen(&format!("{field}\n"), field, 1),
            ),
        ] {
            assert!(!same_export(old, again.as_bytes()), "{why}");
            assert!(!same_export(again.as_bytes(), old), "{why}");
        }
    }

    #[test]
    fn only_a_field_named_with_a_trailing_star_defines_an_array_s_elements() {
        assert!(element_definition(ELEMENTS.as_bytes()));
        assert!(element_definition(
            b"DEFINE FIELD OVERWRITE embedding.* ON task_chunk TYPE float PERMISSIONS FULL;"
        ));
        for line in [
            "DEFINE FIELD OVERWRITE `before` ON ask TYPE array<object> PERMISSIONS FULL;",
            "DEFINE FIELD OVERWRITE `before`.*.handle ON ask TYPE string PERMISSIONS FULL;",
            "DEFINE FIELD OVERWRITE kind ON ask TYPE string ASSERT $value INSIDE ['a.* ON '] PERMISSIONS FULL;",
            "DEFINE INDEX `before`.* ON ask FIELDS about;",
            "INSERT [ { words: 'DEFINE FIELD OVERWRITE `before`.* ON ask' } ];",
            "-- DEFINE FIELD OVERWRITE `before`.* ON ask TYPE object PERMISSIONS FULL;",
            "DEFINE FIELD OVERWRITE `before`.*",
            "",
        ] {
            assert!(!element_definition(line.as_bytes()), "{line}");
        }
    }

    #[test]
    fn an_info_answer_names_what_it_holds_and_refuses_a_name_the_board_cannot_say() {
        let info = json!({"databases": {"tasks": "DEFINE …", "scratch_board": "DEFINE …"}});
        let mut held = names(&info, "databases", "sf", "127.0.0.1:1").unwrap();
        held.sort();
        assert_eq!(held, ["scratch_board", "tasks"]);
        let empty = names(&json!({"databases": {}}), "databases", "sf", "127.0.0.1:1");
        assert_eq!(empty.unwrap(), Vec::<String>::new());
        let said = refusal(
            &json!({"namespaces": {"sf": "", "not-plain": ""}}),
            "namespaces",
        );
        assert!(
            said.contains("\"not-plain\" at 127.0.0.1:1 is not a plain name"),
            "{said}"
        );
    }
}
