//! Which build moves a project's live board forward ([`symphony_board::live`]), held at every entry a
//! binary reaches the board by: a repository of its own under the temp folder, its main checkout's
//! board (the `tasks` database on the PostgreSQL server of its common dir) made by a build one
//! schema version behind this one, and a linked worktree of it (`wt/`, branch `feature`). Each run is
//! this build's binary in a process of its own, as a session runs one: by its path from either
//! checkout, through the launcher, under Cargo, with `TASKS_SERVER`, or by the server's address.

mod cleared;
mod default_branch;
mod first;
mod flags;
mod rolled_back;
mod served;
mod unfollowed;
mod writers;

use crate::children::Child as _;
use crate::common::{NOW, project_root};
use crate::{Ran, git, project_file, repository, scratch};
use std::cell::Cell;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use symphony_board::live::{POSTGRES_SOURCE, SOURCE};
use symphony_board::server::Home;
use symphony_board::{Database, NewTask, Opening, Program, Raw, Schema, Store};

/// This build's schema source, which the binaries under test embed.
const OWN: &str = include_str!("../../src/schema.rs");
/// This build's PostgreSQL definitions, which a build moving a live board must also match
/// ([`POSTGRES_SOURCE`]).
const OWN_POSTGRES: &str = include_str!("../../src/store/postgres/schema.rs");

/// The binaries under test, this build's.
const TASKS: &str = env!("CARGO_BIN_EXE_tasks");
const COPY: &str = env!("CARGO_BIN_EXE_tasks-copy");
const SEND: &str = env!("CARGO_BIN_EXE_agent-send");

/// What the main checkout holds at [`SOURCE`] and commits on the branch it has out.
#[derive(Clone, Copy)]
enum Held {
    /// This build's source, byte for byte: on `main`, this build is main's own, wherever it was
    /// built.
    Own,
    /// Another (this build's and one line): this build is a branch's.
    Other,
    /// None, as a consuming project's checkout, which holds a pin.
    Nothing,
}

/// The project `live`: its main checkout, a linked worktree, a folder outside any repository, and a
/// machine folder whose registry lists the project. Its board's server is stopped when the case
/// ends, however it ends.
struct Live {
    base: PathBuf,
    main: PathBuf,
    wt: PathBuf,
    outside: PathBuf,
    machine: PathBuf,
    /// What the branch the main checkout has out commits at [`SOURCE`].
    held: Cell<Held>,
}

impl Live {
    /// The project with `held` committed on its default branch, `main`, which its main checkout
    /// has out, its board's server running and its board not made yet.
    fn new(name: &str, held: Held) -> Live {
        let base = scratch(name);
        let main = repository(&base.join("main"));
        std::fs::write(main.join("symphony.toml"), project_file("live", "")).unwrap();
        git(&main, &["add", "-A"]);
        git(&main, &["commit", "-qm", "project"]);
        git(&main, &["worktree", "add", "-q", "-b", "feature", "../wt"]);
        let live = Live {
            wt: base.join("wt"),
            outside: base.join("outside"),
            machine: base.join("machine"),
            main,
            base,
            held: Cell::new(Held::Nothing),
        };
        live.hold(held);
        std::fs::create_dir_all(&live.outside).unwrap();
        std::fs::create_dir_all(live.base.join("home")).unwrap();
        std::fs::create_dir_all(&live.machine).unwrap();
        std::fs::write(
            live.machine.join("projects.json"),
            serde_json::json!({"projects": [{"id": "live", "root": live.main}]}).to_string(),
        )
        .unwrap();
        live.start();
        live
    }

    /// As [`Live::new`], its board made by a build one schema version behind this one (main's,
    /// while this build's schema has not landed), holding the task `live/one`.
    fn older(name: &str, held: Held) -> Live {
        let live = Live::new(name, held);
        live.seed("tasks");
        live
    }

    /// The database `db` on the board's server, made by the build one schema version behind: while
    /// the default branch commits no source, as a consuming project's pinned build makes its board,
    /// since the server names the board it serves and this test's build is not that version's.
    fn seed(&self, db: &str) {
        let held = self.held.get();
        self.hold(Held::Nothing);
        seed(self.database(db));
        self.hold(held);
    }

    /// `held` at [`SOURCE`] in the main checkout's working tree, committed on the branch it has out,
    /// with this build's PostgreSQL definitions at [`POSTGRES_SOURCE`] beside a source it holds.
    fn hold(&self, held: Held) {
        let source = self.main.join(SOURCE);
        let postgres = self.main.join(POSTGRES_SOURCE);
        std::fs::create_dir_all(postgres.parent().unwrap()).unwrap();
        match held {
            Held::Own => std::fs::write(&source, OWN).unwrap(),
            Held::Other => std::fs::write(&source, format!("{OWN}// a branch's line\n")).unwrap(),
            Held::Nothing => {
                let _ = std::fs::remove_file(&source);
                let _ = std::fs::remove_file(&postgres);
            }
        }
        if !matches!(held, Held::Nothing) {
            std::fs::write(&postgres, OWN_POSTGRES).unwrap();
        }
        git(&self.main, &["add", "-A"]);
        git(&self.main, &["commit", "-q", "--allow-empty", "-m", "held"]);
        self.held.set(held);
    }

    fn home(&self) -> Home {
        Home::in_common(&self.main.join(".git"))
    }

    /// The board's server started by this test's process, whose children's commands reach it:
    /// a scratch board's server ends with the process that started it.
    fn start(&self) {
        self.home().board_server(&self.main, true).unwrap();
    }

    /// Whether the board's server runs.
    fn serving(&self) -> bool {
        self.home().board_server(&self.main, false).is_ok()
    }

    /// The address of the board's server.
    fn addr(&self) -> String {
        self.home()
            .board_server(&self.main, false)
            .expect("the board's server")
            .addr
    }

    /// The database `db` on the board's server.
    fn database(&self, db: &str) -> Database {
        on_server(&self.addr(), db)
    }

    /// The schema version the database `db` records, none before it is made: one read, which
    /// defines and writes nothing.
    fn version(&self, db: &str) -> Option<i64> {
        version(self.database(db))
    }

    /// `program ARGS` in `cwd`, `input` on stdin, on an environment of only what a session's run
    /// carries and `vars`: no board named, the machine folder this project's.
    fn run(
        &self,
        program: &Path,
        cwd: &Path,
        vars: &[(&str, &str)],
        args: &[&str],
        input: &str,
    ) -> Ran {
        run(&self.base, &self.machine, program, cwd, vars, args, input)
    }

    /// `tasks ARGS` in `cwd`.
    fn tasks(&self, cwd: &Path, args: &[&str]) -> Ran {
        self.run(Path::new(TASKS), cwd, &[], args, "")
    }

    /// The refusal of a build whose schema source is not the one the default branch, `main`,
    /// commits.
    fn another_source(&self) -> String {
        format!(
            "the live board is at schema {}, behind this build's {}, and only a build of the schema \
             source its default branch `main` commits ({SOURCE}) moves it forward: this build's is \
             another, wherever it was built and whatever a checkout holds, so nothing was written; \
             `just tasks migrate` in {} moves it with that branch's own build, and a newer schema \
             is tried on a scratch board (`--database NAME`)",
            before(),
            this(),
            self.main.display()
        )
    }

    /// The refusal of any command but `tasks migrate` in the main checkout, where the default branch
    /// commits no source.
    fn only_migrate(&self) -> String {
        format!(
            "the live board is at schema {}, behind this build's {}, and only `just tasks migrate` \
             run in its main checkout moves it forward: nothing was written; `just tasks migrate` \
             in {} moves it, and a newer schema is tried on a scratch board (`--database NAME`)",
            before(),
            this(),
            self.main.display()
        )
    }

    /// The refusal of a process Cargo runs from `cargo`, a folder of this repository.
    fn under_cargo(&self, cargo: &Path) -> String {
        format!(
            "the live board is the live board of the repository Cargo runs this process from \
             (CARGO_MANIFEST_DIR={}: a test, an example or `cargo run`), and none of those ever \
             reaches it: nothing was read or written; give it a board of its own (`TASKS_SERVER` \
             and `TASKS_DATABASE`, or a fixture repository in a temporary folder)",
            cargo.display()
        )
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        let _ = self.home().stop_board();
    }
}

/// The database `db` on the PostgreSQL at `addr`, `postgresql:SOCKET`.
fn on_server(addr: &str, db: &str) -> Database {
    let socket = addr
        .strip_prefix("postgresql:")
        .expect("a PostgreSQL address");
    symphony_board::pg::Client::new(Path::new(socket), db).into()
}

/// The schema version the board in `db` records, none before it is made: one read, which defines
/// and writes nothing.
fn version(db: Database) -> Option<i64> {
    Store::unchecked_on(
        db,
        Box::new(|| NOW),
        Box::new(|_| true),
        "",
        &Opening::write(),
    )
    .board_version()
    .unwrap()
}

/// The board in `db`, made by the build one schema version behind, holding the task `live/one`:
/// main's own `tasks` on this machine before its rebuild at this build's schema, so the line it
/// writes (a build at schema 18 or later keeps one) is the migrate's own, which leaves it out.
fn seed(db: Database) {
    let tasks = Path::new(TASKS).canonicalize().unwrap();
    Store::opening_on(
        db,
        Box::new(|| NOW),
        Box::new(|_| false),
        &symphony_board::sys::hostname(),
        &Opening::write()
            .at(Schema::this_build().before())
            .by(Program::at(&tasks)),
    )
    .unwrap()
    .create(&NewTask::new("live/one", "Made by main's build", "owner"))
    .unwrap();
}

/// `program ARGS` in `cwd`, `input` on stdin, on an environment of only what a session's run carries
/// and `vars`: no board named, `HOME` in `base`, the machine folder `machine`.
fn run(
    base: &Path,
    machine: &Path,
    program: &Path,
    cwd: &Path,
    vars: &[(&str, &str)],
    args: &[&str],
    input: &str,
) -> Ran {
    let path = format!(
        "{}:/usr/sbin:/sbin",
        std::env::var("PATH").unwrap_or_default()
    );
    let mut command = Command::of(program);
    command
        .cleared()
        .env("PATH", path)
        .env("HOME", base.join("home"))
        .env("TASKS_NOW", NOW.to_string())
        .env("TASKS_SWEEP", "off")
        .env("TASKS_DISPATCH", "inline")
        .env("SYMPHONY_MACHINE_DIR", machine)
        .envs(vars.iter().copied())
        .args(args)
        .current_dir(cwd);
    // Its input a file, which ends where it ends whatever other tests' children hold open.
    let file = base.join("input");
    std::fs::write(&file, input).unwrap();
    command.stdin(std::fs::File::open(&file).unwrap());
    let done = command.output().unwrap();
    Ran {
        code: done.status.code().unwrap_or(-1),
        out: String::from_utf8(done.stdout).unwrap(),
        err: String::from_utf8(done.stderr).unwrap(),
    }
}

fn before() -> i64 {
    Schema::this_build().before().version()
}

fn this() -> i64 {
    Schema::this_build().version()
}

/// The refusal `why` of `tasks` as `tasks-copy` says it: its scratch board is named by what
/// `tasks-copy` takes, `TASKS_DATABASE` or its own second argument, never `--database`.
fn as_copy(why: &str) -> String {
    let flag = "(`--database NAME`)";
    assert!(why.ends_with(flag), "{why}");
    format!(
        "❌ tasks-copy: {}(`TASKS_DATABASE=NAME` with `--board`, or `tasks-copy ADDR NAME`)\n",
        why.strip_suffix(flag).unwrap()
    )
}

/// `ran` failed with exit 2, its standard error `said` on one line after `✖`.
fn refused(ran: &Ran, said: &str) {
    assert_eq!(
        (ran.code, ran.err.as_str()),
        (2, format!("✖ {said}\n").as_str()),
        "{}",
        ran.out
    );
}

#[test]
fn a_branch_build_moves_the_live_board_by_no_entry_and_each_refusal_names_what_does() {
    let live = Live::older("main-only-branch", Held::Other);
    let why = live.another_source();

    // By its path, from the worktree and from the main checkout: where it runs proves nothing.
    for cwd in [&live.wt, &live.main] {
        refused(&live.tasks(cwd, &["list"]), &why);
    }
    // The api the Symphony and the agents call, which answers each call.
    let call = r#"{"method": "get", "args": {"slug": "live/one"}}"#;
    let ran = live.run(Path::new(TASKS), &live.wt, &[], &["api"], call);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&ran.out).ok(),
        Some(serde_json::json!({"error": why})),
        "{}{}",
        ran.out,
        ran.err
    );
    // The worktree sweep.
    refused(&live.tasks(&live.wt, &["sweep"]), &why);
    // A message to an agent.
    let ran = live.run(
        Path::new(SEND),
        &live.wt,
        &[],
        &["someone", "hello", "--as", "me"],
        "",
    );
    refused(&ran, &why);
    // `TASKS_SERVER` on the board's port: from a folder of the repository, and from outside any,
    // where the machine's registry names the board's project.
    let addr = live.addr();
    for cwd in [&live.wt, &live.outside] {
        let ran = live.run(
            Path::new(TASKS),
            cwd,
            &[("TASKS_SERVER", &addr)],
            &["list"],
            "",
        );
        refused(&ran, &why);
    }
    // A copy into the board, by the board its folder reaches and by its server's address.
    let ran = live.run(Path::new(COPY), &live.wt, &[], &["--board"], "{}");
    assert_eq!((ran.code, ran.err), (2, as_copy(&why)));
    let ran = live.run(Path::new(COPY), &live.outside, &[], &[&addr], "{}");
    assert_eq!((ran.code, ran.err), (2, as_copy(&why)));
    // `tasks migrate`: only in the main checkout, and there only by main's own build.
    let main = live.main.display();
    refused(
        &live.tasks(&live.wt, &["migrate"]),
        &format!(
            "`tasks migrate` moves the live board only from its main checkout, {main}: nothing was \
             read or written; run it there (`cd {main} && just tasks migrate`)"
        ),
    );
    refused(&live.tasks(&live.main, &["migrate"]), &why);

    assert_eq!(live.version("tasks"), Some(before()));
    let titles = (Raw::of(&live.database("tasks")).table("task"))
        .fields(&["title"])
        .unwrap();
    let titles: Vec<serde_json::Value> = (titles.as_array().unwrap().iter())
        .map(|row| row["title"].clone())
        .collect();
    assert_eq!(
        Some(serde_json::Value::Array(titles)),
        Some(serde_json::json!(["Made by main's build"]))
    );
}

#[test]
fn a_build_of_the_source_the_default_branch_commits_moves_the_live_board_from_any_folder() {
    let live = Live::older("main-only-own", Held::Own);
    let ran = live.tasks(&live.wt, &["list"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert!(ran.out.contains("live/one"), "{}", ran.out);
    assert_eq!(live.version("tasks"), Some(this()));
    // At its own schema, the main checkout's `tasks migrate` has nothing to move.
    let ran = live.tasks(&live.main, &["migrate"]);
    assert_eq!(
        (ran.code, ran.out, ran.err),
        (
            0,
            format!(
                "✔ the live board is at schema {}, this build's: nothing to move\n",
                this()
            ),
            String::new()
        )
    );
}

#[test]
fn where_the_default_branch_commits_no_source_only_migrate_in_the_main_checkout_moves_the_board() {
    let live = Live::older("main-only-pinned", Held::Nothing);
    let why = live.only_migrate();
    for cwd in [&live.wt, &live.main] {
        refused(&live.tasks(cwd, &["list"]), &why);
    }
    let addr = live.addr();
    let ran = live.run(Path::new(COPY), &live.outside, &[], &[&addr], "{}");
    assert_eq!((ran.code, ran.err), (2, as_copy(&why)));
    assert!(
        live.tasks(&live.wt, &["migrate"])
            .err
            .contains("only from its main checkout")
    );
    assert_eq!(live.version("tasks"), Some(before()));

    // From any folder of the main checkout.
    let docs = live.main.join("docs");
    std::fs::create_dir_all(&docs).unwrap();
    let ran = live.tasks(&docs, &["migrate", "--as", "owner"]);
    assert_eq!(
        (ran.code, ran.out, ran.err),
        (
            0,
            format!(
                "✔ moved the live board from schema {} to {}\n",
                before(),
                this()
            ),
            String::new()
        )
    );
    assert_eq!(live.version("tasks"), Some(this()));
    let ran = live.tasks(&live.main, &["migrate"]);
    assert_eq!(
        ran.out,
        format!(
            "✔ the live board is at schema {}, this build's: nothing to move\n",
            this()
        )
    );
    let ran = live.tasks(&live.wt, &["list"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert!(ran.out.contains("live/one"), "{}", ran.out);
}

#[test]
fn a_scratch_board_is_moved_by_any_writer_and_a_live_one_not_made_yet_by_mains_build() {
    let live = Live::new("main-only-scratch", Held::Other);
    live.seed("scratch");
    let ran = live.tasks(&live.wt, &["--database", "scratch", "list"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(live.version("scratch"), Some(this()));
    let ran = live.run(
        Path::new(TASKS),
        &live.wt,
        &[("TASKS_DATABASE", "fresh")],
        &["migrate"],
        "",
    );
    assert_eq!(
        (ran.code, ran.out),
        (
            0,
            format!(
                "✔ made the board in database fresh at {} at schema {}\n",
                live.addr(),
                this()
            )
        )
    );
    assert_eq!(live.version("tasks"), None);
    // The live board itself, not made yet, a branch's build does not make where the default branch
    // commits the board's source: it would lock that branch's build out as moving it would.
    refused(
        &live.tasks(&live.wt, &["list"]),
        &format!(
            "the live board is not made yet, and only a build of the schema source its default \
             branch `main` commits ({SOURCE}) makes it: this build's is another, wherever it was \
             built and whatever a checkout holds, so nothing was written; `just tasks migrate` in \
             {} makes it with that branch's own build, and a newer schema is tried on a scratch \
             board (`--database NAME`)",
            live.main.display()
        ),
    );
    assert_eq!(live.version("tasks"), None);
    // Where it commits none, the first writer makes it.
    live.hold(Held::Nothing);
    let ran = live.tasks(&live.wt, &["list"]);
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(live.version("tasks"), Some(this()));
}

#[test]
fn a_process_cargo_runs_from_the_repository_never_reaches_its_live_board() {
    let live = Live::new("main-only-cargo", Held::Own);
    let cargo = live.wt.join("crates/tasks");
    std::fs::create_dir_all(&cargo).unwrap();
    let under = [("CARGO_MANIFEST_DIR", cargo.to_str().unwrap())];
    // Refused before its server is started.
    live.home().stop_board().unwrap();
    let ran = live.run(Path::new(TASKS), &live.wt, &under, &["list"], "");
    refused(&ran, &live.under_cargo(&cargo));
    assert!(!live.serving());

    live.start();
    live.seed("tasks");
    let addr = live.addr();
    let ran = live.run(Path::new(TASKS), &live.wt, &under, &["list"], "");
    refused(&ran, &live.under_cargo(&cargo));
    let named = [under[0], ("TASKS_SERVER", &addr)];
    let ran = live.run(Path::new(TASKS), &live.wt, &named, &["list"], "");
    refused(&ran, &live.under_cargo(&cargo));
    // By the server's address, from outside the repository: the folder Cargo runs from names it.
    let ran = live.run(Path::new(COPY), &live.outside, &under, &[&addr], "{}");
    assert_eq!(
        (ran.code, ran.err),
        (2, format!("❌ tasks-copy: {}\n", live.under_cargo(&cargo)))
    );
    assert_eq!(live.version("tasks"), Some(before()));
    // A scratch board of its own it reaches.
    let ran = live.run(
        Path::new(TASKS),
        &live.wt,
        &under,
        &["--database", "scratch", "list"],
        "",
    );
    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(live.version("scratch"), Some(this()));
    // The same build, not under Cargo, is main's own.
    assert_eq!(live.tasks(&live.wt, &["list"]).code, 0);
    assert_eq!(live.version("tasks"), Some(this()));
}

#[test]
fn migrate_takes_only_help_and_an_actor_and_says_what_it_did_to_a_scratch_board() {
    let env = crate::env();
    let help = crate::run(&env, &["migrate", "--help"], "");
    assert_eq!(help.code, 0);
    assert!(
        help.out.starts_with(
            "usage: just tasks migrate [-h] [--leave-behind] [--as ACTOR]\n\nMove the board forward"
        ),
        "{}",
        help.out
    );
    for args in [
        &["migrate", "--now"][..],
        &["migrate", "--as"],
        &["migrate", "--as", "me", "extra"],
    ] {
        let ran = crate::run(&env, args, "");
        assert_eq!(
            (ran.code, ran.out.as_str(), ran.err),
            (
                2,
                "",
                format!(
                    "usage: just tasks migrate [-h] [--leave-behind] [--as ACTOR]\njust tasks migrate: error: \
                     unrecognized arguments: {}\n",
                    args[1..].join(" ")
                )
            )
        );
    }
    let board = format!(
        "the board in database {} at {}",
        env.vars["TASKS_DATABASE"], env.vars["TASKS_SERVER"]
    );
    let made = crate::run(&env, &["migrate", "--as", "me"], "");
    assert_eq!(
        (made.code, made.out),
        (0, format!("✔ made {board} at schema {}\n", this()))
    );
    let again = crate::run(&env, &["migrate"], "");
    assert_eq!(
        again.out,
        format!(
            "✔ {board} is at schema {}, this build's: nothing to move\n",
            this()
        )
    );
    let mut nameless = env.clone();
    nameless
        .vars
        .insert("TASKS_DATABASE".into(), "not a name".into());
    let ran = crate::run(&nameless, &["migrate"], "");
    assert_eq!(
        (ran.code, ran.err),
        (
            2,
            "✖ TASKS_DATABASE=\"not a name\" is not a plain name for a database\n".to_string()
        )
    );
}

/// A board server in the common dir `0`, stopped when the case ends, however it ends: one of a
/// repository with no main checkout, so its programs are the machine's, which its project file
/// names too.
struct Served(PathBuf);

impl Served {
    fn pg(&self) -> symphony_board::server::Postgres {
        let programs =
            symphony_board::server::postgres::discover_programs(&project_root()).unwrap();
        Home::in_common(&self.0).postgresql(&programs)
    }

    /// The board's server started by this test's process, its address.
    fn start(&self) -> String {
        let serving = self.pg().ensure().unwrap();
        format!("postgresql:{}", serving.socket.display())
    }

    /// The schema version of its `tasks` database: one read, which defines and writes nothing.
    fn version(&self) -> Option<i64> {
        let serving = self.pg().current().unwrap().expect("the board's server");
        version(on_server(
            &format!("postgresql:{}", serving.socket.display()),
            "tasks",
        ))
    }
}

impl Drop for Served {
    fn drop(&mut self) {
        let _ = self.pg().stop();
    }
}

/// A repository at `path` holding the file of the project `loose`, committed on `main`.
fn loose_project(path: &Path) -> PathBuf {
    let seed = repository(path);
    std::fs::write(seed.join("symphony.toml"), project_file("loose", "")).unwrap();
    git(&seed, &["add", "-A"]);
    git(&seed, &["commit", "-qm", "project"]);
    seed
}

#[test]
fn a_repository_with_no_main_checkout_moves_its_live_board_by_migrate_in_any_checkout_of_it() {
    let base = scratch("main-only-loose");
    let seed_repository = loose_project(&base.join("seed"));
    // A bare repository and a worktree of it.
    git(&base, &["clone", "-q", "--bare", "seed", "bare.git"]);
    git(
        &base.join("bare.git"),
        &["worktree", "add", "-q", "../wt", "main"],
    );
    // A submodule, whose common dir is kept in its superproject's `modules/`.
    let sup = repository(&base.join("super"));
    git(
        &sup,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            seed_repository.to_str().unwrap(),
            "sub",
        ],
    );
    git(&sup, &["commit", "-qm", "submodule"]);
    let machine = base.join("machine");
    std::fs::create_dir_all(&machine).unwrap();
    for (common, checkout) in [
        (base.join("bare.git"), base.join("wt")),
        (sup.join(".git/modules/sub"), sup.join("sub")),
    ] {
        let served = Served(common.canonicalize().unwrap());
        let addr = served.start();
        seed(on_server(&addr, "tasks"));
        let tasks =
            |args: &[&str]| run(&base, &machine, Path::new(TASKS), &checkout, &[], args, "");

        refused(
            &tasks(&["list"]),
            &format!(
                "the loose board is at schema {}, behind this build's {}, and its repository has \
                 no main checkout and its default branch `main` commits no board source, so only \
                 `just tasks migrate` run in a checkout of it moves it forward: nothing was \
                 written; `just tasks migrate` in any checkout of the repository at {} moves it, \
                 and a newer schema is tried on a scratch board (`--database NAME`)",
                before(),
                this(),
                served.0.display()
            ),
        );
        assert_eq!(served.version(), Some(before()));

        let ran = tasks(&["migrate"]);
        assert_eq!(
            (ran.code, ran.out, ran.err),
            (
                0,
                format!(
                    "✔ moved the loose board from schema {} to {}\n",
                    before(),
                    this()
                ),
                String::new()
            ),
            "{}",
            checkout.display()
        );
        assert_eq!(served.version(), Some(this()));
        let ran = tasks(&["list"]);
        assert_eq!(ran.code, 0, "{}", ran.err);
        assert!(ran.out.contains("live/one"), "{}", ran.out);
    }
}

/// A stand-in for `cargo build … --bin NAME`: this build's `tasks`, linked where Cargo puts NAME, as
/// a target folder shared with a branch's build would hold it.
const CARGO: &str = r#"#!/bin/sh
name=""; while [ $# -gt 0 ]; do if [ "$1" = --bin ]; then name="$2"; fi; shift; done
[ -n "$name" ] || exit 0
mkdir -p "$CARGO_TARGET_DIR/debug"
ln -f "@TASKS@" "$CARGO_TARGET_DIR/debug/$name" 2>/dev/null || cp -p "@TASKS@" "$CARGO_TARGET_DIR/debug/$name"
"#;

#[test]
fn the_launcher_in_a_worktree_runs_the_main_checkouts_build_which_the_board_judges_by_its_source() {
    let live = Live::older("main-only-launcher", Held::Other);
    // The main checkout's launcher, and the sources it checks before it builds, in both checkouts.
    let tools = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools")
        .canonicalize()
        .unwrap();
    for checkout in [&live.main, &live.wt] {
        std::fs::create_dir_all(checkout.join("tools")).unwrap();
        for tool in ["rs-bin.sh", "cargo.sh"] {
            std::os::unix::fs::symlink(tools.join(tool), checkout.join("tools").join(tool))
                .unwrap();
        }
        std::fs::write(checkout.join("Cargo.toml"), "[workspace]\n").unwrap();
        let bin = checkout.join("crates/tasks/src/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(checkout.join("crates/tasks/Cargo.toml"), "[package]\n").unwrap();
        std::fs::write(bin.join("tasks.rs"), "fn main() {}\n").unwrap();
    }
    let cargo = live.base.join("cargo");
    std::fs::write(&cargo, CARGO.replace("@TASKS@", TASKS)).unwrap();
    std::fs::set_permissions(&cargo, std::fs::Permissions::from_mode(0o755)).unwrap();

    let vars = [("CARGO", cargo.to_str().unwrap())];
    let launcher = live.wt.join("tools/rs-bin.sh");
    let ran = live.run(&launcher, &live.wt, &vars, &["tasks", "list"], "");
    refused(&ran, &live.another_source());
    // It ran the main checkout's build, in the main checkout's target folder.
    assert!(live.main.join(".target/rs-bin/tasks").exists());
    assert!(!live.wt.join(".target").exists());
    assert_eq!(live.version("tasks"), Some(before()));
}
