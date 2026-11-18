//! The durable engine identity of a board home, beside its server record: `backend.json`,
//! `{"format":1,"engine":"postgresql"}` or `surrealdb`. Absence preserves a SurrealDB board;
//! malformed metadata, orphan PostgreSQL data, or data of the wrong engine refuse rather than
//! falling back. Initialization is explicit, atomic under the starters' lock, and scratch-only.

use super::{Doing, Home, Postgres, Running, ServerError, io, postgres, scratch};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::Path;

const FILE: &str = "backend.json";

/// The engine whose data a board home owns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    SurrealDb,
    PostgreSql,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Identity {
    format: u32,
    #[serde(deserialize_with = "engine_name")]
    engine: Engine,
}

/// The identity file's engine is a string, rather than Serde's other enum representations.
fn engine_name<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Engine, D::Error> {
    struct Name;
    impl<'de> serde::de::Visitor<'de> for Name {
        type Value = Engine;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a backend engine name")
        }

        fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Engine, E> {
            match value {
                "surrealdb" => Ok(Engine::SurrealDb),
                "postgresql" => Ok(Engine::PostgreSql),
                _ => Err(E::unknown_variant(value, &["surrealdb", "postgresql"])),
            }
        }
    }
    deserializer.deserialize_str(Name)
}

fn present(path: &Path) -> Result<bool, ServerError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(why) => Err(io("could not inspect board data", path)(why)),
    }
}

impl Home {
    fn identity(&self) -> Result<Option<Engine>, ServerError> {
        let path = self.dir.join(FILE);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(why) if why.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(why) => return Err(io("could not read backend identity", &path)(why)),
        };
        if !text.trim_start().starts_with('{') {
            return Err(ServerError::Io(format!(
                "{} is not a backend identity object",
                path.display()
            )));
        }
        let identity: Identity = serde_json::from_str(&text).map_err(|why| {
            ServerError::Io(format!(
                "{} is not a known backend identity: {why}",
                path.display()
            ))
        })?;
        if identity.format != 1 {
            return Err(ServerError::Io(format!(
                "{} has unknown backend identity format {}",
                path.display(),
                identity.format
            )));
        }
        Ok(Some(identity.engine))
    }

    fn data_matches(&self, engine: Engine) -> Result<(), ServerError> {
        let wrong = self.dir.join(match engine {
            Engine::SurrealDb => postgres::DATA,
            Engine::PostgreSql => super::DATA,
        });
        if present(&wrong)? {
            return Err(ServerError::Io(format!(
                "backend identity {engine:?} conflicts with data at {}: nothing was started",
                wrong.display()
            )));
        }
        if engine == Engine::PostgreSql {
            let pg = self.dir.join(postgres::DATA);
            if present(&pg)? && !pg.join("PG_VERSION").is_file() {
                return Err(ServerError::Io(format!(
                    "{} is not a complete PostgreSQL data folder",
                    pg.display()
                )));
            }
        }
        Ok(())
    }

    /// The engine this board owns. No identity means SurrealDB, with no PostgreSQL data present.
    pub fn backend(&self) -> Result<Engine, ServerError> {
        let engine = self.identity()?.unwrap_or(Engine::SurrealDb);
        self.data_matches(engine)?;
        Ok(engine)
    }

    /// Initialize a scratch board's engine explicitly. Existing identities never change; an
    /// engine's existing data may be adopted only when no data of the other engine is present.
    /// Sync a temporary file, rename it whole, then sync the folder before returning success.
    pub fn initialize_backend(&self, engine: Engine) -> Result<(), ServerError> {
        if !scratch(&self.dir) {
            return Err(ServerError::Io("backend initialization is allowed only on a scratch board: live PostgreSQL activation is not enabled".into()));
        }
        let _locked = self.lock(Doing::Starting)?;
        if let Some(held) = self.identity()? {
            if held != engine {
                return Err(ServerError::Io(
                    "an existing board backend identity cannot be changed".into(),
                ));
            }
            return self.data_matches(engine);
        }
        self.data_matches(engine)?;
        let path = self.dir.join(FILE);
        let temporary = path.with_extension(format!("json.{}", std::process::id()));
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary)
            .map_err(io("could not write backend identity", &temporary))?;
        let text =
            serde_json::to_vec(&Identity { format: 1, engine }).expect("an identity serializes");
        file.write_all(&text)
            .and_then(|()| file.sync_all())
            .map_err(io("could not sync backend identity", &temporary))?;
        fs::rename(&temporary, &path).map_err(io("could not publish backend identity", &path))?;
        File::open(&self.dir)
            .and_then(|folder| folder.sync_all())
            .map_err(io("could not sync board home", &self.dir))
    }

    /// PostgreSQL programs for this board's project, with the project file's install guidance.
    pub fn project_postgresql(&self, root: &Path) -> Result<Postgres, ServerError> {
        let programs = postgres::discover_programs(root).map_err(ServerError::Io)?;
        Ok(self.postgresql(&programs))
    }

    /// The selected board server, for commands and the machine server's board reader alike.
    /// `start` false never starts one; PostgreSQL selection remains scratch-only.
    pub fn board_server(&self, root: &Path, start: bool) -> Result<Running, ServerError> {
        match self.backend()? {
            Engine::SurrealDb if start => self.ensure(),
            Engine::SurrealDb => self.recorded().ok_or_else(|| {
                ServerError::NotUp(format!("no server of the board in {}", self.dir.display()))
            }),
            Engine::PostgreSql => {
                let pg = self.project_postgresql(root)?;
                let serving = if start {
                    pg.ensure()?
                } else {
                    pg.current()?.ok_or_else(|| {
                        ServerError::NotUp(format!(
                            "no PostgreSQL server of the board in {}",
                            self.dir.display()
                        ))
                    })?
                };
                Ok(Running {
                    addr: format!("postgresql:{}", serving.socket.display()),
                    pid: serving.pid,
                    version: serving.version,
                })
            }
        }
    }

    pub(super) fn require_surreal(&self) -> Result<(), ServerError> {
        if self.backend()? == Engine::SurrealDb {
            return Ok(());
        }
        Err(ServerError::Io(
            "this board owns PostgreSQL data: use its PostgreSQL lifecycle operation".into(),
        ))
    }

    pub(super) fn allow_postgresql(&self) -> Result<(), ServerError> {
        if self.identity()? == Some(Engine::SurrealDb) {
            return Err(ServerError::Io(
                "this board owns SurrealDB data: PostgreSQL was not started".into(),
            ));
        }
        self.data_matches(Engine::PostgreSql)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Board {
        home: Home,
    }
    impl Board {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("backend-{}-{name}", std::process::id()));
            fs::create_dir_all(&dir).unwrap();
            Self {
                home: Home::new(&dir, Path::new("/no/surreal")),
            }
        }
    }
    impl Drop for Board {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(self.home.dir());
        }
    }

    #[test]
    fn absence_preserves_surreal_and_an_identity_is_explicit_atomic_and_immutable() {
        let board = Board::new("initialize");
        assert_eq!(board.home.backend().unwrap(), Engine::SurrealDb);
        board.home.initialize_backend(Engine::PostgreSql).unwrap();
        assert_eq!(board.home.backend().unwrap(), Engine::PostgreSql);
        board.home.initialize_backend(Engine::PostgreSql).unwrap();
        assert!(board.home.initialize_backend(Engine::SurrealDb).is_err());
        let text = fs::read_to_string(board.home.dir.join(FILE)).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&text).unwrap(),
            serde_json::json!({"format": 1, "engine": "postgresql"})
        );
        assert!(board.home.ensure().is_err());
        assert!(board.home.stop().is_err());
        assert!(board.home.hold("worker", "wrong backend").is_err());
        assert!(board.home.reclaim().is_err());
        assert!(board.home.snapshot(1.0).is_err());
    }

    #[test]
    fn unknown_metadata_and_wrong_engine_data_never_fall_back() {
        let board = Board::new("refuse");
        for text in [
            "broken",
            r#"{"format":2,"engine":"postgresql"}"#,
            r#"{"format":1,"engine":"other"}"#,
            r#"{"format":1,"engine":"postgresql","extra":1}"#,
            r#"{"format":1,"engine":"postgresql","engine":"surrealdb"}"#,
            r#"{"format":1,"format":1,"engine":"postgresql"}"#,
            r#"{"format":1,"engine":1}"#,
            r#"{"format":1,"engine":{"postgresql":null}}"#,
            r#"[1,"postgresql"]"#,
            r#"{"format":1}"#,
            r#"{"engine":"postgresql"}"#,
        ] {
            fs::write(board.home.dir.join(FILE), text).unwrap();
            assert!(board.home.backend().is_err(), "{text}");
            assert!(board.home.board_server(board.home.dir(), false).is_err());
        }
        fs::remove_file(board.home.dir.join(FILE)).unwrap();
        let pg = board.home.dir.join(postgres::DATA);
        fs::create_dir(&pg).unwrap();
        assert!(
            board.home.backend().is_err(),
            "orphan PostgreSQL data is never SurrealDB"
        );
        assert!(
            board.home.initialize_backend(Engine::PostgreSql).is_err(),
            "no partial cluster"
        );
        fs::write(pg.join("PG_VERSION"), "18\n").unwrap();
        board.home.initialize_backend(Engine::PostgreSql).unwrap();
        fs::create_dir(board.home.dir.join(super::super::DATA)).unwrap();
        assert!(board.home.backend().is_err(), "both data folders conflict");
    }

    #[test]
    fn engine_names_accept_json_escapes_and_either_field_order() {
        let board = Board::new("escaped");
        for (text, engine) in [
            (
                r#"{"engine":"postgre\u0073ql","format":1}"#,
                Engine::PostgreSql,
            ),
            (
                r#"{"format":1,"engine":"surreal\u0064b"}"#,
                Engine::SurrealDb,
            ),
        ] {
            fs::write(board.home.dir.join(FILE), text).unwrap();
            assert_eq!(board.home.backend().unwrap(), engine);
        }
    }

    #[test]
    fn metadata_errors_are_not_read_as_absent_data() {
        let board = Board::new("metadata-error");
        let file = board.home.dir.join("not-a-folder");
        fs::write(&file, "kept").unwrap();
        let home = Home::new(&file, Path::new("/no/surreal"));
        for engine in [Engine::SurrealDb, Engine::PostgreSql] {
            assert!(home.data_matches(engine).is_err());
        }
        assert_eq!(fs::read_to_string(file).unwrap(), "kept");
    }

    #[test]
    fn dangling_or_cyclic_postgresql_data_is_not_an_empty_home() {
        let board = Board::new("data-links");
        board.home.initialize_backend(Engine::PostgreSql).unwrap();
        let pg = board.home.dir.join(postgres::DATA);
        for target in ["missing", postgres::DATA] {
            std::os::unix::fs::symlink(target, &pg).unwrap();
            assert!(board.home.backend().is_err());
            assert!(board.home.allow_postgresql().is_err());
            fs::remove_file(&pg).unwrap();
        }
    }

    #[test]
    fn no_live_identity_is_initialized_even_by_a_supervised_home() {
        let home =
            Home::new(Path::new("/not-a-scratch-board"), Path::new("/no/surreal")).supervised(true);
        assert!(
            home.initialize_backend(Engine::PostgreSql)
                .unwrap_err()
                .to_string()
                .contains("scratch")
        );
    }

    #[test]
    fn absent_identity_keeps_legacy_data_and_refuses_adopting_it_as_postgresql() {
        let board = Board::new("legacy");
        fs::create_dir(board.home.dir.join(super::super::DATA)).unwrap();
        assert_eq!(board.home.backend().unwrap(), Engine::SurrealDb);
        assert!(board.home.initialize_backend(Engine::PostgreSql).is_err());
        assert!(!board.home.dir.join(FILE).exists());
        board.home.initialize_backend(Engine::SurrealDb).unwrap();
        assert!(board.home.allow_postgresql().is_err());
    }

    #[test]
    fn concurrent_initializers_publish_one_whole_identity_under_the_starters_lock() {
        for (name, engines, expected) in [
            ("same", [Engine::PostgreSql, Engine::PostgreSql], 2),
            ("different", [Engine::PostgreSql, Engine::SurrealDb], 1),
        ] {
            let board = Board::new(name);
            let start = std::sync::Barrier::new(2);
            let succeeded = std::thread::scope(|scope| {
                let threads: Vec<_> = engines
                    .into_iter()
                    .map(|engine| {
                        let (home, start) = (&board.home, &start);
                        scope.spawn(move || {
                            start.wait();
                            home.initialize_backend(engine).is_ok()
                        })
                    })
                    .collect();
                threads
                    .into_iter()
                    .map(|thread| thread.join().unwrap())
                    .filter(|succeeded| *succeeded)
                    .count()
            });
            assert_eq!(succeeded, expected);
            assert!(board.home.backend().is_ok());
            assert_eq!(
                fs::read_dir(board.home.dir())
                    .unwrap()
                    .filter_map(Result::ok)
                    .filter(|entry| entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with("backend.json."))
                    .count(),
                0
            );
        }
    }
}
