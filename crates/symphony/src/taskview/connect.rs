//! How the Symphony reaches a project's board to read it, and how a failed read is told.
//!
//! Every reach goes through [`open`], which holds a folder this process serves ([`serve`]) to the
//! repository found from it when it began to serve it: a reach made after a caller's check that the
//! folder still holds its project, once its `.git` went, is refused as gone, and never walks up to
//! the board of the checkout that holds the folder, whatever the caller. Its project's journal is
//! held the same way ([`served_journal`]).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use symphony_board::lock::Recovered as _;

/// How long the server waits on each board request: its board tick shares the data worker's thread
/// with every document refresh, and a page gives up on an answer after 15 s (the client's
/// `request.ts`). A read with no whole answer in that time goes once more
/// ([`symphony_board::rpc::Kind`]), and so waits at most twice this.
const BOARD_WAIT: std::time::Duration = std::time::Duration::from_secs(3);

/// A page read's board failure: a board that took the request and answered too late is slow (504),
/// any other failure 503, said after `unreachable` (the words a route puts before it).
pub fn read_failure(error: &str, unreachable: &str) -> (u16, String) {
    if symphony_board::rpc::slow(error) {
        (504, format!("the task board is slow: {error}"))
    } else {
        (503, format!("{unreachable}{error}"))
    }
}

/// A folder this process serves, as it was found when it began to serve it ([`serve`]).
struct Served {
    /// The repository (its common dir) the board's lookup from the folder found, none for a folder
    /// no repository held.
    repository: Option<PathBuf>,
    /// The journal of its project's chat and asks ([`crate::asks::default_path`]).
    journal: PathBuf,
}

/// The folders this process serves, by their path.
static SERVED: Mutex<BTreeMap<PathBuf, Served>> = Mutex::new(BTreeMap::new());

/// Begin to serve the folder `root`: from here on, for the life of this process, every reach of its
/// board from that path, spelled as here ([`connect`], [`connect_in`], [`connect_running`]), the
/// server's own and any route's, is held to the repository
/// the board's lookup from it finds now, on the files alone
/// ([`symphony_board::project::locate_quick`]), which is returned. Once the lookup finds another
/// (a folder left without its `.git` leads it up to the checkout that holds the folder) or that one
/// is a repository no more, a reach is refused as a folder that is gone, saying
/// [`symphony_board::project::GONE`], and starts or reads no board
/// ([`symphony_board::cli::held_board`]). A folder no repository holds now is held to nothing, as a
/// folder this process does not serve: its reach is refused only once it is gone. Its project's
/// journal is the one found from it now, too ([`crate::asks::default_path`]), never one found from
/// the folder again, which without its `.git` is the journal of the checkout that holds it. Served
/// again, the folder is held to what the lookups find then.
pub fn serve(root: &Path) -> Option<PathBuf> {
    let repository = symphony_board::project::locate_quick(root).map(|place| place.common);
    let journal = crate::asks::found_path(root);
    SERVED.lock().recover().insert(
        root.to_path_buf(),
        Served {
            repository: repository.clone(),
            journal,
        },
    );
    repository
}

/// The repository a reach from `root` is held to ([`serve`]): none for a folder this process does
/// not serve, or that no repository held when it began to.
fn held(root: &Path) -> Option<PathBuf> {
    SERVED
        .lock()
        .recover()
        .get(root)
        .and_then(|served| served.repository.clone())
}

/// The journal of the project whose folder `root` this process serves, as it was found when it
/// began to serve it ([`serve`]); none for a folder it does not serve.
pub(crate) fn served_journal(root: &Path) -> Option<PathBuf> {
    SERVED
        .lock()
        .recover()
        .get(root)
        .map(|served| served.journal.clone())
}

/// The board of the project at `root`, as this process's environment reaches it.
pub fn connect(root: &Path) -> Result<symphony_board::Store, String> {
    connect_in(root, crate::children::board_env())
}

/// [`connect`] to a board whose server is running, never starting one: for a read that can go
/// without the board, as the branch scan's [`crate::branches::branch_cards`] does.
pub fn connect_running(root: &Path) -> Result<symphony_board::Store, String> {
    open(
        root,
        crate::children::board_env(),
        symphony_board::cli::held_running_board,
    )
}

/// The board of the project at `root` as `env` reaches it, opened to read by the schema rule: a
/// board one version ahead is read, and written only while the record its migration wrote lets
/// this build write it ([`symphony_board::schema::COMPATIBLE`]), its guard reading that record in
/// each write; otherwise a write on it is refused naming the project and `just symphony`. A board
/// behind this build, or not made yet, is refused before it is read, naming `just tasks`, and
/// never moved forward or made. So a branch's build, its command line or a preview server, cannot
/// move the live board past the build its board command runs.
/// A folder this process serves is reached held to its repository ([`serve`]).
pub fn connect_in(
    root: &Path,
    env: symphony_board::cli::Env,
) -> Result<symphony_board::Store, String> {
    open(root, env, symphony_board::cli::held_board)
}

/// How a reach finds the board of the project whose folder its `Env` runs in, held to the
/// repository it is handed ([`symphony_board::cli::held_board`],
/// [`symphony_board::cli::held_running_board`]).
type Reach =
    fn(&symphony_board::cli::Env, Option<&Path>) -> Result<symphony_board::cli::Reached, String>;

/// The board `reach` finds for the project at `root` under `env`, held to the repository `root` is
/// served from ([`serve`]), opened to read as [`connect_in`] says.
fn open(
    root: &Path,
    mut env: symphony_board::cli::Env,
    reach: Reach,
) -> Result<symphony_board::Store, String> {
    env.cwd = root.to_path_buf();
    // A read-only web request is never a board session or a launcher.
    env.vars.remove("TASKS_SESSION");
    let reached = reach(&env, held(root).as_deref())?;
    let client = match reached.database() {
        symphony_board::Database::SurrealDb(client) => client.waiting(BOARD_WAIT).into(),
        symphony_board::Database::PostgreSql(client) => client.waiting(BOARD_WAIT).into(),
    };
    let how = symphony_board::Opening::read()
        .of(reached.project.as_deref())
        .live(reached.live);
    let store = match env.vars.get("TASKS_NOW") {
        Some(text) => {
            let now = text
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite())
                .ok_or_else(|| format!("TASKS_NOW={text} is not a time in seconds"))?;
            symphony_board::Store::reading_on(
                client,
                Box::new(move || now),
                Box::new(symphony_board::sys::pid_alive),
                &symphony_board::sys::hostname(),
                &how,
            )
        }
        None => symphony_board::Store::reading_on(
            client,
            Box::new(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs_f64()
            }),
            Box::new(symphony_board::sys::pid_alive),
            &symphony_board::sys::hostname(),
            &how,
        ),
    }
    .map_err(|e| e.to_string())?;
    // The cap the command line holds starts to, which the agents view and the PULL digest show.
    Ok(symphony_board::cli::capped(store, &env))
}

#[cfg(test)]
#[path = "connect_tests.rs"]
mod tests;
