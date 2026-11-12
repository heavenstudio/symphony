//! A project's live board, and which builds may move it forward.
//!
//! A project's live board is the `tasks` database on the server its repository's common dir records
//! (`sf-tasks/server.json`), however a run reaches it: through its project, a `TASKS_SERVER` on that
//! port, or a client made for that address ([`find`]). Any other board is a scratch one, which every
//! writer moves forward as the schema rule says ([`crate::schema::decide`]).
//!
//! A writer moves a live board forward only when the project's main checkout says this build's
//! schema is its own ([`may_move`]): where that checkout holds the board's source ([`SOURCE`], this
//! repository), only a build whose source is that file, byte for byte, wherever it was built and
//! however it runs, which alone makes the board too; where it holds none (a consuming project, whose
//! checkout holds a pin), only `tasks migrate` run in the main checkout itself, while any writer
//! makes a board not made yet; and in a repository with no main checkout (a bare repository's
//! worktrees, a submodule's working tree), only `tasks migrate` run in any checkout of it. Who asks
//! is never judged by the path of the build: a build of the same schema text is, to the board,
//! main's own, and a path proves nothing.
//!
//! A process Cargo runs (a test, an example, `cargo run`: `CARGO_MANIFEST_DIR` is set) is refused the
//! live board of the repository it runs from, before its server is started or anything is read
//! ([`cargo_refusal`]): a test's board is a scratch one, or a fixture repository's own. So is every
//! process it starts, with whatever environment: a process that runs under one Cargo started is
//! refused by that one's folder ([`cargo_here`]), so a child a test starts with a cleared environment
//! is refused as the test is.

use crate::{DATABASE, git, project, server};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Where a main checkout that holds the board's own source keeps its schema: this repository's.
pub const SOURCE: &str = "crates/tasks/src/schema.rs";

/// This build's schema source, byte for byte.
const OWN_SOURCE: &str = include_str!("schema.rs");

/// What Cargo sets for every process it runs, tests and examples included: the package's folder.
pub const CARGO: &str = "CARGO_MANIFEST_DIR";

/// How a refusal names a scratch board to a program that reads the board's database from the
/// environment alone: every program that reaches a board, the Symphony and `tasks-cutover` among
/// them.
pub const SCRATCH_VARIABLE: &str = "`TASKS_DATABASE=NAME`";

/// How a refusal names a scratch board to `tasks` and `agent-send`, which take it as a flag.
pub const SCRATCH_FLAG: &str = "`--database NAME`";

/// A project's live board.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Live {
    /// The project, as refusals name it.
    pub project: String,
    /// Its repository's common dir, which records the board's server.
    pub common: PathBuf,
}

impl Live {
    /// The project's main checkout: its common dir less `/.git`; none for a bare repository.
    pub fn main(&self) -> Option<&Path> {
        crate::sweep::trigger::main_checkout(&self.common)
    }
}

/// The port of `addr` (`HOST:PORT`) as the board's client reads it: Rust's `u16`, an optional `+`
/// and digits, leading zeros or not. None for any other spelling.
pub fn port_of(addr: &str) -> Option<u16> {
    addr.rsplit_once(':')?.1.parse().ok()
}

/// What `common`'s board server record says of its port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recorded {
    /// No record: no live server to name.
    None,
    /// The port it records.
    Port(u16),
    /// A record naming no address, or a port spelled as no number: it names no port it can be told
    /// apart from, so it counts as any (`tools/rs-bin.sh` reads it the same way).
    Unreadable,
}

/// The port `common`'s board server record names ([`Recorded`]).
pub fn recorded(common: &Path) -> Recorded {
    let record = common.join(server::FOLDER).join("server.json");
    let Ok(text) = std::fs::read_to_string(record) else {
        return Recorded::None;
    };
    serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|record| port_of(record.get("addr")?.as_str()?))
        .map_or(Recorded::Unreadable, Recorded::Port)
}

impl Recorded {
    /// Whether a server on `port` is the one recorded: an unreadable record counts as it, when
    /// `strict` is false.
    fn names(self, port: Option<u16>, strict: bool) -> bool {
        match self {
            Recorded::None => false,
            Recorded::Port(recorded) => port == Some(recorded),
            Recorded::Unreadable => !strict,
        }
    }
}

/// The live board of the repository whose common dir is `common`, named `project`.
fn live(common: &Path, project: String) -> Live {
    Live {
        project,
        common: common.to_path_buf(),
    }
}

/// The project named for the repository in `common`: its main checkout's file or folder name, as
/// [`project::find`] names it from there.
fn named(common: &Path, machine: Option<&Path>) -> String {
    let at = crate::sweep::trigger::main_checkout(common).unwrap_or(common);
    project::find(at, machine).map_or_else(
        |_| {
            at.file_name().map_or_else(
                || at.display().to_string(),
                |name| name.to_string_lossy().into(),
            )
        },
        |found| found.id,
    )
}

/// The live board a run in `cwd` reaches with `TASKS_SERVER` at `addr` and database `database`: the
/// board of `cwd`'s repository when its record names that port (or names none it can be told apart
/// from, as the launcher reads it), else none.
pub fn named_server(
    addr: &str,
    database: &str,
    cwd: &Path,
    machine: Option<&Path>,
) -> Option<Live> {
    if database != DATABASE {
        return None;
    }
    let common = git::read(cwd)?;
    recorded(&common)
        .names(port_of(addr), false)
        .then(|| live(&common, named(&common, machine)))
}

/// The live board a client of `addr` and `database` reaches, whoever made the client: the board of
/// the repository this process runs in (`cwd`), or of a project the machine's registry lists, whose
/// record names that port. None for a scratch board.
pub fn find(
    addr: &str,
    database: &str,
    cwd: Option<&Path>,
    machine: Option<&Path>,
) -> Option<Live> {
    if database != DATABASE {
        return None;
    }
    let port = port_of(addr);
    if let Some(common) = cwd.and_then(git::read)
        && recorded(&common).names(port, false)
    {
        return Some(live(&common, named(&common, machine)));
    }
    machine
        .map(project::registered_roots)
        .unwrap_or_default()
        .into_iter()
        .find_map(|(id, root)| {
            let common = git::read(&root)?;
            recorded(&common)
                .names(port, true)
                .then(|| live(&common, id))
        })
}

/// [`find`] for this process: its working directory and its machine folder.
pub fn find_here(addr: &str, database: &str) -> Option<Live> {
    let cwd = std::env::current_dir().ok();
    let machine =
        project::machine_folder(|name| std::env::var_os(name), cfg!(target_os = "macos")).ok();
    find(addr, database, cwd.as_deref(), machine.as_deref())
}

/// The program whose children are the processes Cargo runs: tests, examples, `cargo run`.
const CARGO_PROGRAM: &str = "cargo";

/// A folder Cargo runs this process from: its own [`CARGO`], or the folder of a process it runs
/// under that Cargo started, which a test's child holds no [`CARGO`] of when the test started it
/// with a cleared environment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cargo {
    /// The package's folder, or the one the process Cargo started works in (a test's: its
    /// package's, where Cargo runs every test).
    pub dir: PathBuf,
    /// The process Cargo started, when it is not this process.
    pub caller: Option<u32>,
}

impl Cargo {
    /// This process's own [`CARGO`], `dir`.
    pub fn own(dir: impl Into<PathBuf>) -> Cargo {
        Cargo {
            dir: dir.into(),
            caller: None,
        }
    }
}

/// The folder of each of `callers` (this process's, nearest first) that a process named `cargo`
/// started, as `folder` reads a process's: which this process runs under, whatever environment it
/// was handed. This process itself is not one of them: one Cargo starts holds [`CARGO`].
pub fn started_by_cargo(
    callers: &[crate::sys::Caller],
    folder: impl Fn(u32) -> Option<PathBuf>,
) -> Vec<Cargo> {
    callers
        .windows(2)
        .filter(|pair| pair[1].name == CARGO_PROGRAM && pair[0].parent == pair[1].pid)
        .filter_map(|pair| {
            Some(Cargo {
                dir: folder(pair[0].pid)?,
                caller: Some(pair[0].pid),
            })
        })
        .collect()
}

/// Every [`Cargo`] this process runs under: its own [`CARGO`], then the folder of each process it
/// runs under that Cargo started, nearest first ([`started_by_cargo`]). Read once, at the first ask.
pub fn cargo_here() -> &'static [Cargo] {
    static HERE: OnceLock<Vec<Cargo>> = OnceLock::new();
    HERE.get_or_init(|| {
        let own = std::env::var_os(CARGO)
            .filter(|dir| !dir.is_empty())
            .map(Cargo::own);
        let started = started_by_cargo(&crate::sys::callers(), crate::sys::folder_of);
        own.into_iter().chain(started).collect()
    })
}

/// The refusal of `live` to a process Cargo runs under `cargo` ([`cargo_here`]), when one of those
/// is a folder of `live`'s repository: a test, and every child it starts, whatever environment it
/// hands it, never reaches it.
pub fn cargo_refusal(live: &Live, cargo: &[Cargo]) -> Option<String> {
    let cargo = cargo
        .iter()
        .find(|cargo| git::read(&cargo.dir).as_ref() == Some(&live.common))?;
    let from = match cargo.caller {
        None => format!("{CARGO}={}", cargo.dir.display()),
        Some(pid) => format!(
            "process {pid}, which Cargo started in {} and this process runs under",
            cargo.dir.display()
        ),
    };
    Some(format!(
        "the {} board is the live board of the repository Cargo runs this process from ({from}: a \
         test, an example or `cargo run`), and none of those ever reaches it: nothing was read or \
         written; give it a board of its own (`TASKS_SERVER` and `TASKS_DATABASE`, or a fixture \
         repository in a temporary folder)",
        live.project
    ))
}

/// The refusal, to a process Cargo runs under `cargo`, of the board a client of `addr` and
/// `database` reaches, or `live` when the caller knows it: none unless one of `cargo` is a folder of
/// that board's repository.
pub fn cargo_refusal_of(
    live: Option<&Live>,
    addr: &str,
    database: &str,
    cargo: &[Cargo],
) -> Option<String> {
    if let Some(live) = live {
        return cargo_refusal(live, cargo);
    }
    if database != DATABASE {
        return None;
    }
    cargo.iter().find_map(|one| {
        let common = git::read(&one.dir)?;
        if !recorded(&common).names(port_of(addr), false) {
            return None;
        }
        let board = self::live(&common, named(&common, None));
        cargo_refusal(&board, std::slice::from_ref(one))
    })
}

/// [`cargo_refusal_of`] for this process ([`cargo_here`]).
pub fn cargo_refusal_here(live: Option<&Live>, addr: &str, database: &str) -> Option<String> {
    let cargo = cargo_here();
    if cargo.is_empty() {
        return None;
    }
    cargo_refusal_of(live, addr, database, cargo)
}

/// Whether a writer opening for `migrating` (`tasks migrate`) may move `live`, found at `found`
/// (none: not made yet), forward to `own`, its schema `this_build` when the build's own
/// ([`crate::Schema::this_build`]): the refusal, in its words, when it may not. Where the main
/// checkout holds the board's source, making the board is judged as moving it is, so no other
/// schema text makes it either; anywhere else any writer makes it. A repository with no main
/// checkout is moved by `tasks migrate` alone, which runs in any checkout of it. A refusal names a
/// scratch board as `scratch` says, in the words the program opening it takes ([`SCRATCH_FLAG`],
/// [`SCRATCH_VARIABLE`]).
pub fn may_move(
    live: &Live,
    this_build: bool,
    migrating: bool,
    found: Option<i64>,
    own: i64,
    scratch: &str,
) -> Result<(), String> {
    let (board, forward, does) = match found {
        Some(found) => (
            format!(
                "the {} board is at schema {found}, behind this build's {own}",
                live.project
            ),
            "moves it forward",
            "moves it",
        ),
        None => (
            format!("the {} board is not made yet", live.project),
            "makes it",
            "makes it",
        ),
    };
    let scratch = format!("a newer schema is tried on a scratch board ({scratch})");
    let Some(main) = live.main() else {
        // No folder holds the board's source to compare with (a bare repository's worktrees, a
        // submodule's working tree): the consuming shape, with `tasks migrate` run in any checkout.
        return match found {
            None => Ok(()),
            Some(_) if migrating => Ok(()),
            Some(_) => Err(format!(
                "{board}, and its repository has no main checkout to say which schema is its own, \
                 so only `just tasks migrate` run in a checkout of it moves it forward: nothing was \
                 written; `just tasks migrate` in any checkout of the repository at {} moves it, \
                 and {scratch}",
                live.common.display()
            )),
        };
    };
    let source = main.join(SOURCE);
    match std::fs::read_to_string(&source) {
        Ok(held) if this_build && held == OWN_SOURCE => Ok(()),
        Ok(_) => Err(format!(
            "{board}, and only a build of the schema source its main checkout holds ({}) {forward}: \
             this build's is another, wherever it was built, so nothing was written; `just tasks \
             migrate` in {} {does} with main's own build, and {scratch}",
            source.display(),
            main.display()
        )),
        Err(missing) if missing.kind() == std::io::ErrorKind::NotFound => {
            if migrating || found.is_none() {
                Ok(())
            } else {
                Err(format!(
                    "{board}, and only `just tasks migrate` run in its main checkout moves it \
                     forward: nothing was written; `just tasks migrate` in {} moves it, and \
                     {scratch}",
                    main.display()
                ))
            }
        }
        Err(other) => Err(format!(
            "{board}, and {} cannot be read ({other}) to say whether this build's schema is its \
             main checkout's: nothing was written",
            source.display()
        )),
    }
}

#[cfg(test)]
#[path = "live_tests.rs"]
mod tests;
