//! A project's live board, and which builds may move it forward.
//!
//! A project's live board is the `tasks` database on its repository's board server, the one on the
//! data in its common dir (`symphony/tasks/postgresql`), however a run reaches it: through its
//! project, or by an address, a `TASKS_SERVER` or a client made for it, which is known by the
//! server listening there and the data it serves, wherever the run is ([`served`], [`find`]). Only
//! where this machine cannot say who listens there is it known by the record of the run's
//! repository, or of a registered project, that names its port ([`recorded`]). Any other board
//! is a scratch one, which every writer moves forward as the schema rule says
//! ([`crate::schema::decide`]).
//!
//! A writer moves a live board forward only when the repository's default branch says this build's
//! schema is its own ([`may_move`]), by what it commits ([`committed`]), never what a checkout holds
//! or which branch the main checkout has out: where it commits the board's source ([`SOURCE`],
//! this repository), only a build whose source is that file, byte for byte, wherever it was built
//! and however it runs, which alone makes the board too; where it commits none (a consuming
//! project, whose checkout holds a pin), only `tasks migrate` run in the main checkout itself, or in
//! any checkout of a repository with none (a bare repository's worktrees, a submodule's working
//! tree), while any writer makes a board not made yet. The default branch is `[project]
//! default_branch` in the main checkout's `symphony.toml`, else `main` ([`default_branch`]). Who
//! asks is never judged by the path of the build: a build of the same schema text is, to the board,
//! the default branch's own, and a path proves nothing.
//!
//! A process Cargo runs (a test, an example, `cargo run`: `CARGO_MANIFEST_DIR` is set) is refused the
//! live board of the repository it runs from, before its server is started or anything is read
//! ([`cargo_refusal`]): a test's board is a scratch one, or a fixture repository's own. So is every
//! process a test starts, with whatever environment, under `cargo test` or `cargo nextest run`: a
//! process that runs under one Cargo or nextest started is refused by that one's folder
//! ([`cargo_here`]), so a child a test starts with a cleared environment is refused as the test is.
//! The ancestry reaches no further: Cargo execs `cargo run` and an example in its own place, so a
//! child of theirs with a cleared environment is not refused, nor one of a test binary run by its
//! path, nor a process that outlived its parent (reparented to launchd or init).
//!
//! A live board that reads below the highest schema it was seen at went back ([`seen`]): every
//! reader and writer refuses it, `tasks migrate` included, until the owner accepts the older board.

use crate::{DATABASE, git, project, server};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub mod seen;

/// Where a repository that commits the board's own source keeps its schema: this repository's.
pub const SOURCE: &str = "crates/tasks/src/schema.rs";

/// The PostgreSQL definitions a build moving a live board must also match ([`may_move`]).
pub const POSTGRES_SOURCE: &str = "crates/tasks/src/store/postgres/schema.rs";
const OWN_POSTGRES_DIGEST: &str = env!("BOARD_POSTGRES_SCHEMA_SHA256");

/// The SHA-256 of this build's schema source, byte for byte, as `build.rs` read it: a committed
/// source is this build's when its digest is this one, and no build carries the source itself.
const OWN_DIGEST: &str = env!("BOARD_SCHEMA_SHA256");

/// This build's schema source, for the tests to commit as a branch's.
#[cfg(test)]
const OWN_SOURCE: &str = include_str!("schema.rs");

/// Whether `source` is this build's schema source, byte for byte ([`OWN_DIGEST`]).
fn is_own(source: &str) -> bool {
    has_digest(source, OWN_DIGEST)
}

fn has_digest(source: &str, expected: &str) -> bool {
    use sha2::{Digest as _, Sha256};
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(source.as_bytes());
    expected.len() == digest.len() * 2
        && digest
            .iter()
            .zip(expected.as_bytes().as_chunks::<2>().0)
            .all(|(&byte, digits)| {
                digits[0] == HEX[usize::from(byte >> 4)] && digits[1] == HEX[usize::from(byte & 15)]
            })
}

/// What Cargo sets for every process it runs, tests and examples included: the package's folder.
pub const CARGO: &str = "CARGO_MANIFEST_DIR";

/// How a refusal names a scratch board to a program that reads the board's database from the
/// environment alone: every program that reaches a board, the Symphony and `tasks-copy` among them.
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
    /// apart from, so it counts as any.
    Unreadable,
}

/// The port `common`'s board server record names ([`Recorded`]).
pub fn recorded(common: &Path) -> Recorded {
    let record = common
        .join(crate::server::FOLDER)
        .join(crate::server::ADDRESS);
    let Ok(text) = std::fs::read_to_string(record) else {
        return Recorded::None;
    };
    serde_json::from_slice::<serde_json::Value>(text.as_bytes())
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

/// The project named for the repository in `common`, as [`named`] names it by this process's machine
/// folder: what a refusal of a board reached by its folder alone (`tasks-reclaim`'s) names.
pub fn project_of(common: &Path) -> String {
    let machine =
        project::machine_folder(|name| std::env::var_os(name), cfg!(target_os = "macos")).ok();
    named(common, machine.as_deref())
}

/// Who this machine says serves an address ([`served`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Served {
    /// The board server of the repository whose common dir this is ([`server::board_served`]).
    Board(PathBuf),
    /// A process that is no board's server: a test's server in memory, one on other data.
    Other,
    /// No process this machine can name: nothing listens there (a server that is down, another
    /// host's), or `lsof` or `ps` cannot say.
    Unknown,
}

/// Who serves `addr` (`HOST:PORT` or `postgresql:SOCKET_FOLDER`) on this machine: the process
/// `lsof` finds there, by the command line `ps` prints for it ([`server::board_served`]).
pub fn served(addr: &str) -> Served {
    let mut command = crate::sys::command("lsof");
    command.args(["-nP", "-w", "-Fp"]);
    if let Some(socket) = addr.strip_prefix("postgresql:") {
        command
            .arg("--")
            .arg(Path::new(socket).join(server::postgres::SOCKET_FILE));
    } else {
        let (Some((host, _)), Some(port)) = (addr.rsplit_once(':'), port_of(addr)) else {
            return Served::Unknown;
        };
        command.args([&format!("-iTCP@{host}:{port}"), "-sTCP:LISTEN"]);
    }
    let listening = crate::sys::output(&mut command);
    let Ok(listening) = listening else {
        return Served::Unknown;
    };
    let mut read = false;
    for pid in String::from_utf8_lossy(&listening.stdout)
        .lines()
        .filter_map(|line| line.strip_prefix('p'))
    {
        let Ok(said) = crate::sys::output(
            crate::sys::command("ps").args(["-ww", "-o", "command=", "-p", pid]),
        ) else {
            continue;
        };
        let command = String::from_utf8_lossy(&said.stdout);
        if command.trim().is_empty() {
            continue;
        }
        read = true;
        if let Some(common) = server::board_served(command.trim()) {
            return Served::Board(common);
        }
    }
    if read { Served::Other } else { Served::Unknown }
}

/// The live board `served` says an address reaches, named as the project `machine`'s registry or
/// its main checkout names it: `Some(None)` for a server of no board's, and none when this machine
/// cannot say.
fn by_server(served: Served, machine: Option<&Path>) -> Option<Option<Live>> {
    match served {
        Served::Board(common) => Some(Some(live(&common, named(&common, machine)))),
        Served::Other => Some(None),
        Served::Unknown => None,
    }
}

/// The live board a run in `cwd` reaches with `TASKS_SERVER` at `addr` and database `database`: the
/// board the server listening there serves ([`served`]), wherever the run is; where this machine
/// cannot say who listens there, the board of `cwd`'s repository when its record names that port
/// ([`named_by_record`]). None for a scratch board.
pub fn named_server(
    addr: &str,
    database: &str,
    cwd: &Path,
    machine: Option<&Path>,
) -> Option<Live> {
    if database != DATABASE {
        return None;
    }
    by_server(served(addr), machine).unwrap_or_else(|| named_by_record(addr, cwd, machine))
}

/// The board of `cwd`'s repository when `addr` names it ([`names_address`]: its PostgreSQL's socket,
/// or a port its record names or none it can be told apart from), else none.
fn named_by_record(addr: &str, cwd: &Path, machine: Option<&Path>) -> Option<Live> {
    let common = git::read(cwd)?;
    names_address(&common, addr, false).then(|| live(&common, named(&common, machine)))
}

/// Whether `addr` names `common`'s board: a PostgreSQL socket by the canonical path of the board's
/// data, never by a TCP port or the server record, following links so `/private/tmp` on macOS is
/// recognized too; any other address by the port the record names ([`recorded`]).
fn names_address(common: &Path, addr: &str, strict: bool) -> bool {
    if let Some(socket) = addr.strip_prefix("postgresql:") {
        let expected = server::postgres::socket_folder(
            &common.join(server::FOLDER).join(server::postgres::DATA),
        );
        let resolved = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        return resolved(Path::new(socket)) == resolved(&expected);
    }
    recorded(common).names(port_of(addr), strict)
}

/// The live board a client of `addr` and `database` reaches, whoever made the client and wherever:
/// the board the server listening there serves ([`served`]); where this machine cannot say who
/// listens there, the board whose record names that port, of the repository this process runs in
/// (`cwd`) or of a project the machine's registry lists ([`found_by_record`]). None for a scratch
/// board.
pub fn find(
    addr: &str,
    database: &str,
    cwd: Option<&Path>,
    machine: Option<&Path>,
) -> Option<Live> {
    if database != DATABASE {
        return None;
    }
    by_server(served(addr), machine).unwrap_or_else(|| found_by_record(addr, cwd, machine))
}

/// The board of `cwd`'s repository, or of a project the machine's registry lists, whose record
/// names the port of `addr`.
fn found_by_record(addr: &str, cwd: Option<&Path>, machine: Option<&Path>) -> Option<Live> {
    if let Some(common) = cwd.and_then(git::read)
        && names_address(&common, addr, false)
    {
        return Some(live(&common, named(&common, machine)));
    }
    machine
        .map(project::registered_roots)
        .unwrap_or_default()
        .into_iter()
        .find_map(|(id, root)| {
            let common = git::read(&root)?;
            names_address(&common, addr, true).then(|| live(&common, id))
        })
}

/// [`find`] for this process: its working directory and its machine folder.
pub fn find_here(addr: &str, database: &str) -> Option<Live> {
    let cwd = std::env::current_dir().ok();
    let machine =
        project::machine_folder(|name| std::env::var_os(name), cfg!(target_os = "macos")).ok();
    find(addr, database, cwd.as_deref(), machine.as_deref())
}

/// The programs that start a test as a child of their own, by the name the kernel gives each, and
/// as a refusal names it: Cargo (`cargo test`, and a build script under `cargo build`) and nextest
/// (`cargo nextest run`, which Cargo execs, and which starts each test). Cargo execs `cargo run`,
/// `--example` and its subcommands in its own place, so their parent is whatever ran Cargo, never
/// Cargo: they are known by their own [`CARGO`] alone.
const STARTERS: [(&str, &str); 2] = [("cargo", "Cargo"), ("cargo-nextest", "nextest")];

/// A folder Cargo runs this process from: its own [`CARGO`], or the folder of a process it runs
/// under that Cargo or nextest started, which a test's child holds no [`CARGO`] of when the test
/// started it with a cleared environment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cargo {
    /// The package's folder, or the one the process Cargo or nextest started works in (a test's:
    /// its package's, where both run every test).
    pub dir: PathBuf,
    /// The process Cargo or nextest started, and which of them did, as a refusal names it
    /// ([`STARTERS`]), when it is not this process.
    pub caller: Option<(u32, &'static str)>,
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

/// The folder of each of `callers` (this process's, nearest first) that a process named `cargo` or
/// `cargo-nextest` started ([`STARTERS`]), as `folder` reads a process's: which this process runs
/// under, whatever environment it was handed. This process itself is not one of them: one Cargo
/// starts holds [`CARGO`]. Nor is anything whose chain to such a process breaks: a test binary run
/// by its path, a child of `cargo run` or an example, a process that outlived its parent
/// (reparented to launchd or init).
pub fn started_by_cargo(
    callers: &[crate::sys::Caller],
    folder: impl Fn(u32) -> Option<PathBuf>,
) -> Vec<Cargo> {
    callers
        .windows(2)
        .filter(|pair| pair[0].parent == pair[1].pid)
        .filter_map(|pair| {
            let (_, by) = STARTERS.iter().find(|(name, _)| pair[1].name == *name)?;
            Some(Cargo {
                dir: folder(pair[0].pid)?,
                caller: Some((pair[0].pid, by)),
            })
        })
        .collect()
}

/// Every [`Cargo`] this process runs under: its own [`CARGO`], then the folder of each process it
/// runs under that Cargo or nextest started, nearest first ([`started_by_cargo`]). Read once, at
/// the first ask.
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
        Some((pid, by)) => format!(
            "process {pid}, which {by} started in {} and this process runs under",
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
/// that board's repository. The board an address reaches is the one its server serves
/// ([`served`]), else, where this machine cannot say, the board of a `cargo` folder's repository
/// whose record names its port.
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
    cargo_refusal_by(served(addr), addr, cargo)
}

/// [`cargo_refusal_of`] of a live board's address, which `served` says who serves.
fn cargo_refusal_by(served: Served, addr: &str, cargo: &[Cargo]) -> Option<String> {
    if let Some(board) = by_server(served, None) {
        return cargo_refusal(&board?, cargo);
    }
    cargo.iter().find_map(|one| {
        let common = git::read(&one.dir)?;
        if !names_address(&common, addr, false) {
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
/// ([`crate::Schema::this_build`]): the refusal, in its words, when it may not. The build that may
/// is the one whose source is what the repository's default branch commits, never what a checkout
/// holds: where it commits the board's source, only a build of that very text, its common version
/// source ([`SOURCE`]) and its PostgreSQL definitions ([`POSTGRES_SOURCE`]) both read from the one
/// commit, under any command, and making the board is judged as moving it is; where it commits
/// none, only `tasks migrate`, run in the main checkout, or in any checkout of a repository with
/// none, while any writer makes a board not made yet. A refusal names a scratch board as `scratch`
/// says, in the words the program opening it takes ([`SCRATCH_FLAG`], [`SCRATCH_VARIABLE`]).
pub fn may_move(
    live: &Live,
    this_build: bool,
    migrating: bool,
    found: Option<i64>,
    own: i64,
    scratch: &str,
) -> Result<(), String> {
    let named = match named_branch(live) {
        Ok(named) => named,
        Err(refused) => return judged(live, &refused, this_build, migrating, found, own, scratch),
    };
    let branch = named.as_deref().unwrap_or(MAIN);
    // Resolve the branch once: both schema blobs must come from the same commit even if a land
    // moves the branch while this command reads them.
    let (source, postgres) = match source_on_files(
        &live.common,
        branch,
        named.is_some(),
        &[SOURCE, POSTGRES_SOURCE],
    ) {
        Ok(mut sources) => (sources.remove(0), sources.remove(0)),
        Err(why) => (Source::Unreadable(why.clone()), Source::Unreadable(why)),
    };
    let common = Committed {
        branch: branch.into(),
        source,
    };
    judged(live, &common, this_build, migrating, found, own, scratch)?;
    if matches!(common.source, Source::Nothing) {
        return Ok(());
    }
    match postgres {
        Source::Text(source) if has_digest(&source, OWN_POSTGRES_DIGEST) => Ok(()),
        _ => Err(format!(
            "the {} board's PostgreSQL schema source its default branch `{}` commits \
             ({POSTGRES_SOURCE}) is not this build's: nothing was written; a newer schema is \
             tried on a scratch board ({scratch})",
            live.project, common.branch
        )),
    }
}

/// [`may_move`]'s verdict on `live`, whose default branch commits `committed`.
fn judged(
    live: &Live,
    committed: &Committed,
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
    let branch = &committed.branch;
    // Where `tasks migrate` runs: the main checkout, else any checkout of the repository.
    let there = live.main().map_or_else(
        || {
            format!(
                "any checkout of the repository at {}",
                live.common.display()
            )
        },
        |main| main.display().to_string(),
    );
    match &committed.source {
        Source::Text(held) if this_build && is_own(held) => Ok(()),
        Source::Text(_) => Err(format!(
            "{board}, and only a build of the schema source its default branch `{branch}` commits \
             ({SOURCE}) {forward}: this build's is another, wherever it was built and whatever a \
             checkout holds, so nothing was written; `just tasks migrate` in {there} {does} with \
             that branch's own build, and {scratch}"
        )),
        Source::Nothing if migrating || found.is_none() => Ok(()),
        Source::Nothing => Err(match live.main() {
            Some(_) => format!(
                "{board}, and only `just tasks migrate` run in its main checkout moves it forward: \
                 nothing was written; `just tasks migrate` in {there} moves it, and {scratch}"
            ),
            None => format!(
                "{board}, and its repository has no main checkout and its default branch \
                 `{branch}` commits no board source, so only `just tasks migrate` run in a checkout \
                 of it moves it forward: nothing was written; `just tasks migrate` in {there} moves \
                 it, and {scratch}"
            ),
        }),
        Source::Unreadable(why) => Err(format!(
            "{board}, and the schema source its default branch `{branch}` commits ({SOURCE}) cannot \
             be read ({why}) to say whether this build's schema is its own: nothing was written"
        )),
    }
}

/// The board's source as a repository's default branch commits it ([`committed`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Committed {
    /// The branch it is read on ([`default_branch`]).
    pub branch: String,
    /// What that branch commits at [`SOURCE`].
    pub source: Source,
}

/// What a branch commits at [`SOURCE`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// The file's text.
    Text(String),
    /// No such file on a branch that exists, or no [`MAIN`] where the project names no default
    /// branch (a repository with no commit on it yet): a consuming project's, which pins a build.
    Nothing,
    /// `git` could not say, and why: among them, a branch the project names that does not exist,
    /// and one that exists but names no commit (a broken ref).
    Unreadable(String),
}

/// The default branch of a project that names none.
pub const MAIN: &str = "main";

/// The branch `[project] default_branch` names in a main checkout's `symphony.toml`, the key the
/// Symphony and the land read, by their one rule ([`project::default_branch`]): as written, never
/// trimmed. None where it names none, the file does not read, it is no text, or there is no main
/// checkout, when the default branch is [`MAIN`].
///
/// # Errors
///
/// For a branch that rule refuses (a blank one), which the Symphony and the land refuse too, as it
/// says it.
pub fn default_branch(main: Option<&Path>) -> Result<Option<String>, String> {
    let written = written_branch(main);
    project::default_branch(written.as_deref()).map(|named| named.map(str::to_string))
}

/// The text `[project] default_branch` is in a main checkout's `symphony.toml`, as written: none
/// where [`default_branch`] reads none.
fn written_branch(main: Option<&Path>) -> Option<String> {
    let text = std::fs::read_to_string(main?.join(project::FILE)).ok()?;
    let file = text.parse::<toml::Table>().ok()?;
    Some(
        file.get("project")?
            .get("default_branch")?
            .as_str()?
            .to_string(),
    )
}

/// The default branch [`default_branch`] reads in `live`'s main checkout, else what was refused:
/// the branch as written, and the source it leaves unreadable ([`Source::Unreadable`]), so the
/// board is not moved by a build the project's default branch never named.
fn named_branch(live: &Live) -> Result<Option<String>, Committed> {
    default_branch(live.main()).map_err(|why| Committed {
        branch: written_branch(live.main()).unwrap_or_default(),
        source: Source::Unreadable(why),
    })
}

/// The board's source ([`SOURCE`]) as the default branch commits it in the repository whose common
/// dir is `common`, read by `git` from that common dir: never what a working copy holds, nor the
/// branch a checkout has out. The default branch is `named`, the one the project names
/// ([`default_branch`]), else [`MAIN`]. Only a branch that exists without the file, and an unnamed
/// [`MAIN`] that does not exist, commit nothing: a named branch that does not exist, and one whose
/// ref names no commit, cannot be read.
pub fn committed(common: &Path, named: Option<&str>) -> Committed {
    let branch = named.unwrap_or(MAIN);
    Committed {
        branch: branch.into(),
        source: source_on(common, branch, named.is_some()).unwrap_or_else(Source::Unreadable),
    }
}

/// [`committed`]'s reading of `branch`, which the project names when `named`: why it cannot be
/// read, when it cannot.
fn source_on(common: &Path, branch: &str, named: bool) -> Result<Source, String> {
    source_on_files(common, branch, named, &[SOURCE]).map(|mut sources| sources.remove(0))
}

fn source_on_files(
    common: &Path,
    branch: &str,
    named: bool,
    sources: &[&str],
) -> Result<Vec<Source>, String> {
    let git = |args: &[&str]| {
        crate::sys::output(crate::sys::git().arg("--git-dir").arg(common).args(args))
            .map_err(|error| format!("git could not run: {error}"))
    };
    // What git said on standard error, on one line.
    let said = |out: &std::process::Output| {
        let said = String::from_utf8_lossy(&out.stderr);
        let said: Vec<&str> = said
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();
        if said.is_empty() {
            format!("git exited with {}", out.status)
        } else {
            said.join("; ")
        }
    };
    let reference = format!("refs/heads/{branch}");
    let tip = git(&[
        "rev-parse",
        "--verify",
        "--quiet",
        &format!("{reference}^{{commit}}"),
    ])?;
    if tip.status.code() == Some(1) {
        // No commit there: no such branch, or a ref that names none.
        let exists = git(&["show-ref", "--exists", &reference])?;
        return match exists.status.code() {
            Some(2) if !named => Ok(vec![Source::Nothing; sources.len()]),
            Some(2) => Err(format!(
                "there is no branch `{branch}`, which `[project] default_branch` names"
            )),
            Some(0) => Err(format!("{reference} names no commit")),
            _ => Err(said(&exists)),
        };
    }
    if !tip.status.success() {
        return Err(said(&tip));
    }
    let tip = String::from_utf8_lossy(&tip.stdout).trim().to_string();
    sources
        .iter()
        .map(|source| {
            let found = git(&[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{tip}:{source}"),
            ])?;
            if found.status.code() == Some(1) {
                return Ok(Source::Nothing);
            }
            if !found.status.success() {
                return Err(said(&found));
            }
            let blob = String::from_utf8_lossy(&found.stdout).trim().to_string();
            let read = git(&["cat-file", "blob", &blob])?;
            if !read.status.success() {
                return Err(said(&read));
            }
            String::from_utf8(read.stdout)
                .map(Source::Text)
                .map_err(|_| format!("{source} on `{branch}` is not UTF-8 text"))
        })
        .collect()
}

#[cfg(test)]
#[path = "live_tests.rs"]
mod tests;
