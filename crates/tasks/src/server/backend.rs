//! The durable engine identity of a board home, beside its server record: `backend.json`,
//! `{"format":1,"engine":"postgresql"}` or `surrealdb`. A home with none is read by what it holds:
//! a SurrealDB store ([`Home::surreal_store`]) keeps it a SurrealDB board, as every board was
//! before the identity; a home that holds neither engine's data is a board not made yet, which
//! is made on PostgreSQL, its identity written by the start that makes it
//! ([`Home::identified`]). Malformed metadata, an incomplete PostgreSQL data folder, or data of
//! the wrong engine refuse rather than falling back. Naming a scratch home's engine before it is
//! made is explicit and atomic under the starters' lock ([`Home::initialize_backend`]); an
//! identity changes only by a move of the board's store ([`super::moving`]), and while that
//! move's journal stands the home names no engine at all.

use super::{Doing, Home, Postgres, Running, ServerError, io, postgres, scratch};
use serde::{Deserialize, Serialize};
use std::fmt;
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

impl Engine {
    /// The folder of its data in a board's home.
    pub(super) fn data(self) -> &'static str {
        match self {
            Engine::SurrealDb => super::DATA,
            Engine::PostgreSql => postgres::DATA,
        }
    }

    /// The name of the folder of its data in a board's home.
    pub fn data_folder(self) -> &'static str {
        self.data()
    }

    /// Its name as the identity file and a command spell it.
    pub fn word(self) -> &'static str {
        match self {
            Engine::SurrealDb => "surrealdb",
            Engine::PostgreSql => "postgresql",
        }
    }

    /// The engine a command names, as [`Engine::word`] spells it.
    pub fn named(word: &str) -> Option<Engine> {
        [Engine::SurrealDb, Engine::PostgreSql]
            .into_iter()
            .find(|engine| engine.word() == word)
    }
}

impl fmt::Display for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Engine::SurrealDb => "SurrealDB",
            Engine::PostgreSql => "PostgreSQL",
        })
    }
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
    /// Refused while a move's or a restore's journal stands ([`super::moving`]): between its
    /// steps the home holds an identity and data that are not one engine's, so nothing reads it
    /// as either.
    fn settled(&self) -> Result<(), ServerError> {
        let journal = self.dir.join(super::moving::JOURNAL);
        if present(&journal)? {
            return Err(ServerError::Io(super::moving::unsettled(&journal)));
        }
        Ok(())
    }

    /// The identity file's engine, with no look at the data or at a journal: the move's own read.
    pub(super) fn identity(&self) -> Result<Option<Engine>, ServerError> {
        let path = self.dir.join(FILE);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(why) => {
                if why.kind() == std::io::ErrorKind::NotFound && !present(&path)? {
                    return Ok(None);
                }
                return Err(io("could not read backend identity", &path)(why));
            }
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

    /// Whether the home holds a SurrealDB store: its `data/`, or the store a reclaim cut short
    /// between its moves left in `reclaim/old` ([`Home::reclaim`]).
    pub(super) fn surreal_store(&self) -> Result<bool, ServerError> {
        Ok(present(&self.dir.join(super::DATA))?
            || present(&self.dir.join(super::RECLAIM).join(super::OLD))?)
    }

    /// The engine this board owns: the one its identity names. With none, SurrealDB for a home
    /// that holds a SurrealDB store, the board of a build from before the identity; PostgreSQL for
    /// any other, a board not made yet ([`Home::unmade`]) included, which a start makes there.
    pub fn backend(&self) -> Result<Engine, ServerError> {
        self.settled()?;
        let engine = match self.identity()? {
            Some(engine) => engine,
            None if self.surreal_store()? => Engine::SurrealDb,
            None => Engine::PostgreSql,
        };
        self.data_matches(engine)?;
        Ok(engine)
    }

    /// Whether the board is not made yet: its home names no engine and holds neither engine's
    /// data. Its first start makes it on PostgreSQL ([`Home::board_server`]).
    pub fn unmade(&self) -> Result<bool, ServerError> {
        self.settled()?;
        Ok(self.identity()?.is_none()
            && !self.surreal_store()?
            && !present(&self.dir.join(postgres::DATA))?)
    }

    /// The home named PostgreSQL's when it names no engine yet: what a start that makes its
    /// cluster writes first, so a cluster is never there without the identity that selects it.
    /// The caller holds the starters' lock and has allowed PostgreSQL here
    /// ([`Home::allow_postgresql`]).
    pub(super) fn identified(&self) -> Result<(), ServerError> {
        if self.identity()?.is_none() {
            self.identify(Engine::PostgreSql)?;
        }
        Ok(())
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
        self.identify(engine)
    }

    /// The identity written whole, whatever it was: a scratch home's first, or a move's switch.
    /// The caller holds the starters' lock.
    pub(super) fn identify(&self, engine: Engine) -> Result<(), ServerError> {
        let text =
            serde_json::to_vec(&Identity { format: 1, engine }).expect("an identity serializes");
        self.publish(FILE, &text)
    }

    /// `text` as the file `name` of this home, whole and durable before this returns: written
    /// and synced beside itself, renamed into place, then the home synced.
    pub(super) fn publish(&self, name: &str, text: &[u8]) -> Result<(), ServerError> {
        let path = self.dir.join(name);
        let temporary = self.dir.join(format!("{name}.{}", std::process::id()));
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary)
            .map_err(io("could not write", &temporary))?;
        file.write_all(text)
            .and_then(|()| file.sync_all())
            .map_err(io("could not sync", &temporary))?;
        fs::rename(&temporary, &path).map_err(io("could not publish", &path))?;
        self.synced()
    }

    /// This home's folder synced, so a rename in it is durable.
    pub(super) fn synced(&self) -> Result<(), ServerError> {
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
    /// `start` false never starts one. PostgreSQL serves the home whose identity names it, and a
    /// board not made yet: a start makes it there, its identity first ([`Postgres::ensure`]), and
    /// is refused with the setup guidance when the machine has no PostgreSQL to make it on, with
    /// nothing made.
    pub fn board_server(&self, root: &Path, start: bool) -> Result<Running, ServerError> {
        let not_up =
            || ServerError::NotUp(format!("no server of the board in {}", self.dir.display()));
        match self.backend()? {
            Engine::SurrealDb if start => self.ensure(),
            Engine::SurrealDb => self.recorded().ok_or_else(not_up),
            Engine::PostgreSql => {
                let unmade = self.unmade()?;
                if unmade && !start {
                    return Err(not_up());
                }
                let pg = self.project_postgresql(root).map_err(|why| {
                    if !unmade {
                        return why;
                    }
                    ServerError::Io(format!(
                        "nothing was made for the board in {}, which is not made yet and is \
                         made on PostgreSQL: {why}",
                        self.dir.display()
                    ))
                })?;
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

    /// The main checkout of the repository whose git folder holds this home: where its project's
    /// PostgreSQL programs are read from (`[board] postgresql`) by a caller that has no project.
    pub(super) fn project_root(&self) -> Result<&Path, ServerError> {
        crate::sweep::trigger::main_checkout(&self.within).ok_or_else(|| {
            ServerError::Io(format!(
                "the board in {} is on PostgreSQL, and {} is no main checkout's git folder to \
                 read its project's PostgreSQL programs from",
                self.dir.display(),
                self.within.display()
            ))
        })
    }

    /// The board's server stopped, on the engine its home selects, a PostgreSQL board's programs
    /// its project's ([`Home::project_root`]): what a caller that started the board by a command,
    /// whichever engine that made it on, stops it by. A board not made yet has none to stop, and
    /// nothing is made.
    pub fn stop_board(&self) -> Result<(), ServerError> {
        if self.unmade()? {
            return Ok(());
        }
        match self.backend()? {
            Engine::SurrealDb => self.stop().map(|_| ()),
            Engine::PostgreSql => self
                .project_postgresql(self.project_root()?)?
                .stop()
                .map(|_| ()),
        }
    }

    /// SurrealDB's own lifecycle is allowed on a home whose identity names it, or that names none
    /// and holds no PostgreSQL data: a board of a build from before the identity, or one its caller
    /// makes on SurrealDB by asking for it (the move, a fixture). A board command never does: it
    /// reaches the server [`Home::backend`] selects, PostgreSQL for a board not made yet.
    pub(super) fn require_surreal(&self) -> Result<(), ServerError> {
        self.settled()?;
        if self.identity()? == Some(Engine::PostgreSql) {
            return Err(ServerError::Io(
                "this board owns PostgreSQL data: use its PostgreSQL lifecycle operation".into(),
            ));
        }
        self.data_matches(Engine::SurrealDb)
    }

    /// PostgreSQL's lifecycle is allowed on the home [`Home::backend`] reads as PostgreSQL's: one
    /// whose identity names it, or a board not made yet, wherever it is kept.
    pub(super) fn allow_postgresql(&self) -> Result<(), ServerError> {
        if self.backend()? == Engine::SurrealDb {
            return Err(ServerError::Io(
                "this board owns SurrealDB data: PostgreSQL was not started".into(),
            ));
        }
        Ok(())
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
    fn an_empty_home_is_postgresql_and_an_identity_is_explicit_atomic_and_immutable() {
        let board = Board::new("initialize");
        assert_eq!(board.home.backend().unwrap(), Engine::PostgreSql);
        assert!(board.home.unmade().unwrap());
        board.home.initialize_backend(Engine::PostgreSql).unwrap();
        assert!(!board.home.unmade().unwrap(), "an identity is a board's");
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
    fn a_home_with_no_identity_is_surrealdbs_while_it_holds_a_surrealdb_store() {
        let board = Board::new("by-store");
        let dir = board.home.dir().to_path_buf();
        let data = dir.join(super::super::DATA);
        fs::create_dir(&data).unwrap();
        assert_eq!(board.home.backend().unwrap(), Engine::SurrealDb);
        assert!(!board.home.unmade().unwrap());
        board.home.require_surreal().unwrap();
        assert!(board.home.allow_postgresql().is_err());
        // A store a reclaim moved aside and did not put back is the board's still, never room
        // for a new one.
        let old = dir.join(super::super::RECLAIM).join(super::super::OLD);
        fs::create_dir_all(old.parent().unwrap()).unwrap();
        fs::rename(&data, &old).unwrap();
        assert_eq!(board.home.backend().unwrap(), Engine::SurrealDb);
        assert!(!board.home.unmade().unwrap());
        assert!(board.home.allow_postgresql().is_err());
        // Gone, the home holds no board: a new one is PostgreSQL's.
        fs::remove_dir_all(dir.join(super::super::RECLAIM)).unwrap();
        assert_eq!(board.home.backend().unwrap(), Engine::PostgreSql);
        assert!(board.home.unmade().unwrap());
        // What else a home keeps (its sweep's, its health's, its keys) is no engine's data.
        for kept in ["health.json", "sweep.log", "lock"] {
            fs::write(dir.join(kept), "").unwrap();
        }
        fs::create_dir(dir.join("keys")).unwrap();
        assert!(board.home.unmade().unwrap());
        assert!(!dir.join(FILE).exists(), "reading names no engine");
    }

    #[test]
    fn a_board_not_made_yet_is_postgresqls_anywhere_and_its_start_names_it_so() {
        // Outside a scratch place too: a new live board is made on PostgreSQL.
        let live = Home::new(Path::new("/not-a-scratch-board"), Path::new("/no/surreal"));
        assert!(!scratch(live.dir()));
        assert_eq!(live.backend().unwrap(), Engine::PostgreSql);
        live.allow_postgresql().unwrap();
        let board = Board::new("identified");
        board.home.identified().unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(
                &fs::read_to_string(board.home.dir.join(FILE)).unwrap()
            )
            .unwrap(),
            serde_json::json!({"format": 1, "engine": "postgresql"})
        );
        assert!(!board.home.unmade().unwrap());
        // A home that names an engine keeps it.
        let named = Board::new("named");
        named.home.initialize_backend(Engine::SurrealDb).unwrap();
        named.home.identified().unwrap();
        assert_eq!(named.home.backend().unwrap(), Engine::SurrealDb);
        // SurrealDB's own start may make a store on an empty home, for a caller that asks for it
        // by name; never on one whose identity names PostgreSQL.
        let empty = Board::new("surreal-asked");
        empty.home.require_surreal().unwrap();
        empty.home.initialize_backend(Engine::PostgreSql).unwrap();
        assert!(empty.home.require_surreal().is_err());
    }

    #[test]
    fn a_board_not_made_yet_is_never_made_by_a_reach_that_starts_nothing_or_without_postgresql() {
        let board = Board::new("unmade-reach");
        // A board not made yet has no server to stop, and stopping it makes nothing.
        board.home.stop_board().unwrap();
        assert!(board.home.unmade().unwrap());
        let refused = board
            .home
            .board_server(board.home.dir(), false)
            .unwrap_err();
        assert!(
            matches!(&refused, ServerError::NotUp(why) if why.starts_with("no server of the board in")),
            "{refused:?}"
        );
        // No PostgreSQL where the project file names it: refused with the setup, nothing made.
        let root = board.home.dir().join("project");
        fs::create_dir(&root).unwrap();
        fs::write(
            root.join(crate::project::FILE),
            "[board]\npostgresql = \"/no/postgresql/bin\"\n",
        )
        .unwrap();
        let refused = board
            .home
            .board_server(&root, true)
            .unwrap_err()
            .to_string();
        assert!(
            refused.starts_with(&format!(
                "nothing was made for the board in {}, which is not made yet and is made on \
                 PostgreSQL: PostgreSQL is not at /no/postgresql/bin",
                board.home.dir().display()
            )),
            "{refused}"
        );
        assert!(refused.contains("brew install postgresql@18"), "{refused}");
        assert!(!refused.contains('\n'), "one line: {refused}");
        assert!(board.home.unmade().unwrap(), "nothing was made: {refused}");
        assert!(!board.home.dir().join(postgres::DATA).exists());
        assert!(!board.home.dir().join(FILE).exists());
    }

    /// A PostgreSQL board's programs are its project's, read from the main checkout whose `.git`
    /// holds its home: a home kept anywhere else is refused its stop, naming where it is, and
    /// nothing is started or signalled.
    #[test]
    fn a_postgresql_boards_stop_reads_its_programs_from_the_main_checkout_whose_git_holds_it() {
        let board = Board::new("project-root");
        let root = board.home.dir().join("lamp");
        let common = root.join(".git");
        let home = Home::in_common(&common, None);
        assert_eq!(home.project_root().unwrap(), root.as_path());
        // Not a main checkout's git folder: a bare repository, or a common dir kept elsewhere.
        board.home.initialize_backend(Engine::PostgreSql).unwrap();
        fs::create_dir_all(board.home.dir().join(postgres::DATA)).unwrap();
        fs::write(
            board.home.dir().join(postgres::DATA).join("PG_VERSION"),
            "18\n",
        )
        .unwrap();
        let refused = board.home.stop_board().unwrap_err().to_string();
        assert_eq!(
            refused,
            format!(
                "the board in {} is on PostgreSQL, and {} is no main checkout's git folder to \
                 read its project's PostgreSQL programs from",
                board.home.dir().display(),
                board.home.dir().parent().unwrap().display()
            )
        );
        assert!(!board.home.unmade().unwrap());
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
    fn unreadable_identity_paths_are_never_absent_identity() {
        let board = Board::new("identity-links");
        let identity = board.home.dir.join(FILE);
        for target in ["missing", FILE] {
            std::os::unix::fs::symlink(target, &identity).unwrap();
            assert!(board.home.backend().is_err());
            assert!(board.home.initialize_backend(Engine::SurrealDb).is_err());
            assert!(board.home.initialize_backend(Engine::PostgreSql).is_err());
            assert!(fs::symlink_metadata(&identity).unwrap().is_symlink());
            fs::remove_file(&identity).unwrap();
        }
        fs::create_dir(&identity).unwrap();
        assert!(board.home.backend().is_err());
        assert!(board.home.initialize_backend(Engine::SurrealDb).is_err());
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
