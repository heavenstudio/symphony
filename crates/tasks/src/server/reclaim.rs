//! [`Home::reclaim`]: the board's store rebuilt from an export of itself, for a store that grew
//! before every server kept its values in its sstables ([`super::Storage`]).

use super::storage::store;
use super::{ADDRESS, DATA, Home, OLD, RECLAIM, Running, ServerError, io};
use crate::rpc;
use crate::sys::{Command, output};
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
    /// Where the exports stay, one SurrealQL file a database.
    pub exports: PathBuf,
    /// The board's server, on the new store.
    pub running: Running,
}

/// A store's databases, and the users and accesses of its root and of each namespace, which a
/// database's export does not hold.
#[derive(Debug, PartialEq)]
struct Layout {
    databases: Vec<(String, String)>,
    definitions: BTreeMap<String, Value>,
}

impl Home {
    /// Rebuild the board's store from an export of itself, which holds only what is live. Under the
    /// starters' lock: end the board's server, recorded or not; start one on the store that is
    /// recorded nowhere, and export every database it holds with SurrealDB's own `export`; import
    /// them into a new store, started with this board's [`super::Storage`]; export that one again
    /// and compare each export byte for byte, and the root's and every namespace's users and
    /// accesses; then swap the stores, start the board's server on the new one, and remove the old.
    /// Until the swap the old store is untouched, and a failure starts the board's server on it
    /// again. The exports stay in `reclaim/exports/`, until the next reclaim.
    pub fn reclaim(&self) -> Result<Reclaimed, ServerError> {
        if !self.surreal.is_file() {
            return Err(ServerError::NoSurreal(self.surreal.clone()));
        }
        let _held = self.lock()?;
        let work = self.dir.join(RECLAIM);
        let data = self.dir.join(DATA);
        self.finish_swap()?;
        let halted = self.halt()?;
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
                return Err(self.serve_again(problem));
            }
        };
        let old = work.join(OLD);
        if let Err(problem) = fs::rename(&data, &old) {
            return Err(self.serve_again(ServerError::Reclaim(format!(
                "could not move {}: {problem}",
                data.display()
            ))));
        }
        let running = match fs::rename(&fresh, &data)
            .map_err(io("could not move", &fresh))
            .and_then(|()| self.start())
        {
            Ok(running) => running,
            Err(problem) => {
                if data.exists() {
                    fs::rename(&data, &fresh).map_err(io("could not move", &data))?;
                }
                fs::rename(&old, &data).map_err(io("could not move", &old))?;
                return Err(self.serve_again(ServerError::Reclaim(problem.to_string())));
            }
        };
        fs::remove_dir_all(&old).map_err(io("could not remove", &old))?;
        fs::remove_dir_all(&check).map_err(io("could not remove", &check))?;
        Ok(Reclaimed {
            before,
            after: bytes(&data),
            databases,
            exports,
            running,
        })
    }

    /// Why a reclaim stopped, the old store in place, once the board's server is started on that
    /// store again; with the start's own failure beside it, if it fails too.
    fn serve_again(&self, problem: ServerError) -> ServerError {
        let Err(again) = self.start() else {
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

    /// End the board's server, recorded or found on its data, and forget its address: the ports
    /// they had, which a client may still hold as the board's. Under the lock.
    fn halt(&self) -> Result<Vec<u16>, ServerError> {
        let mut ports = Vec::new();
        if let Some(running) = self.recorded() {
            if self.serves(running.pid) {
                self.end(running.pid)?;
            }
            let address = self.dir.join(ADDRESS);
            fs::remove_file(&address).map_err(io("could not remove", &address))?;
            ports.push(running.addr);
        }
        if let Some(running) = self.unrecorded() {
            self.end(running.pid)?;
            ports.push(running.addr);
        }
        Ok(ports
            .iter()
            .filter_map(|addr| addr.parse::<SocketAddr>().ok())
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
        let file = |ns: &str, db: &str| format!("{ns}.{db}.surql");
        let old = self.spawn(&store(data), halted)?;
        let board = (crate::NAMESPACE.to_string(), crate::DATABASE.to_string());
        let read = layout(&old).and_then(|held| {
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
            for (ns, db) in &held.databases {
                self.transfer("export", &old, ns, db, &exports.join(file(ns, db)))?;
            }
            Ok(held)
        });
        self.end(old.pid)?;
        let held = read?;
        let new = self.spawn(&store(fresh), halted)?;
        let built = (|| {
            for (ns, db) in &held.databases {
                self.transfer("import", &new, ns, db, &exports.join(file(ns, db)))?;
            }
            for (ns, db) in &held.databases {
                let (first, again) = (exports.join(file(ns, db)), check.join(file(ns, db)));
                self.transfer("export", &new, ns, db, &again)?;
                let read = |path: &Path| fs::read(path).map_err(io("could not read", path));
                if read(&first)? != read(&again)? {
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
            Command::new(&self.surreal)
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
fn layout(running: &Running) -> Result<Layout, ServerError> {
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
    layout.databases.sort();
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
    match named.keys().find(|name| !rpc::plain(name)) {
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
