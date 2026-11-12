//! Which build moves a project's live board forward ([`symphony_board::live`]), held at every entry a
//! binary reaches the board by: a repository of its own under the temp folder, its main checkout's
//! board (the `tasks` database on the SurrealDB server its common dir records) made by a build one
//! schema version behind this one, and a linked worktree of it (`wt/`, branch `feature`). Each run is
//! this build's binary in a process of its own, as a session runs one: by its path from either
//! checkout, through the launcher, under Cargo, with `TASKS_SERVER`, or by the server's address.

mod cleared;
mod flags;

use crate::common::{NOW, surreal};
use crate::{Ran, git, repository, scratch};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use symphony_board::live::SOURCE;
use symphony_board::rpc::Client;
use symphony_board::server::Home;
use symphony_board::{NewTask, Opening, Schema, Store};

/// This build's schema source, which the binaries under test embed.
const OWN: &str = include_str!("../../src/schema.rs");

/// The binaries under test, this build's.
const TASKS: &str = env!("CARGO_BIN_EXE_tasks");
const COPY: &str = env!("CARGO_BIN_EXE_tasks-copy");
const SEND: &str = env!("CARGO_BIN_EXE_agent-send");

/// What the main checkout holds at [`SOURCE`].
#[derive(Clone, Copy)]
enum Held {
    /// This build's source, byte for byte: this build is main's own, wherever it was built.
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
}

impl Live {
    /// The project with `held` in its main checkout's working tree, its board's server running and
    /// its board not made yet.
    fn new(name: &str, held: Held) -> Live {
        let base = scratch(name);
        let main = repository(&base.join("main"));
        std::fs::write(main.join("symphony.toml"), "[project]\nid = \"live\"\n").unwrap();
        git(&main, &["add", "-A"]);
        git(&main, &["commit", "-qm", "project"]);
        git(&main, &["worktree", "add", "-q", "-b", "feature", "../wt"]);
        let live = Live {
            wt: base.join("wt"),
            outside: base.join("outside"),
            machine: base.join("machine"),
            main,
            base,
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
        live.home().ensure().unwrap();
        live
    }

    /// As [`Live::new`], its board made by a build one schema version behind this one (main's,
    /// while this build's schema has not landed), holding the task `live/one`.
    fn older(name: &str, held: Held) -> Live {
        let live = Live::new(name, held);
        live.seed("tasks");
        live
    }

    /// The database `db` on the board's server, made by the build one schema version behind.
    fn seed(&self, db: &str) {
        seed(self.client(db));
    }

    /// The main checkout's working tree holding `held` at [`SOURCE`].
    fn hold(&self, held: Held) {
        let source = self.main.join(SOURCE);
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        match held {
            Held::Own => std::fs::write(&source, OWN).unwrap(),
            Held::Other => std::fs::write(&source, format!("{OWN}// a branch's line\n")).unwrap(),
            Held::Nothing => {
                let _ = std::fs::remove_file(&source);
            }
        }
    }

    fn home(&self) -> Home {
        Home::in_common(&self.main.join(".git"), Some(&surreal()))
    }

    /// The address of the board's server.
    fn addr(&self) -> String {
        self.home().recorded().expect("the board's server").addr
    }

    fn client(&self, db: &str) -> Client {
        Client::new(&self.addr(), "sf", db)
    }

    /// The schema version the database `db` records, none before it is made: one read, which
    /// defines and writes nothing.
    fn version(&self, db: &str) -> Option<i64> {
        Store::unchecked(self.client(db), Box::new(|| NOW), Box::new(|_| true), "")
            .board_version()
            .unwrap()
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

    /// The refusal of a build whose schema source is not the one the main checkout holds.
    fn another_source(&self) -> String {
        format!(
            "the live board is at schema {}, behind this build's {}, and only a build of the schema \
             source its main checkout holds ({}) moves it forward: this build's is another, \
             wherever it was built, so nothing was written; `just tasks migrate` in {} moves it \
             with main's own build, and a newer schema is tried on a scratch board \
             (`--database NAME`)",
            before(),
            this(),
            self.main.join(SOURCE).display(),
            self.main.display()
        )
    }

    /// The refusal of any command but `tasks migrate` in the main checkout, where it holds no source.
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
        let _ = self.home().stop();
    }
}

/// The board `client` reaches, made by the build one schema version behind, holding the task
/// `live/one`.
fn seed(client: Client) {
    Store::opening(
        client,
        Box::new(|| NOW),
        Box::new(|_| false),
        "this-host",
        &Opening::write().at(Schema::this_build().before()),
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
    let mut command = Command::new(program);
    command
        .env_clear()
        .env("PATH", path)
        .env("HOME", base.join("home"))
        .env("SF_SURREAL", surreal())
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
    assert_eq!(
        live.client("tasks")
            .query(
                "RETURN (SELECT VALUE title FROM task);",
                serde_json::json!({})
            )
            .unwrap()
            .last()
            .cloned(),
        Some(serde_json::json!(["Made by main's build"]))
    );
}

#[test]
fn a_build_of_the_source_the_main_checkout_holds_moves_the_live_board_from_any_folder() {
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
fn where_the_main_checkout_holds_no_source_only_migrate_run_there_moves_the_live_board() {
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
    // The live board itself, not made yet, a branch's build does not make where the main checkout
    // holds the board's source: it would lock main's build out as moving it would.
    refused(
        &live.tasks(&live.wt, &["list"]),
        &format!(
            "the live board is not made yet, and only a build of the schema source its main \
             checkout holds ({}) makes it: this build's is another, wherever it was built, so \
             nothing was written; `just tasks migrate` in {} makes it with main's own build, and \
             a newer schema is tried on a scratch board (`--database NAME`)",
            live.main.join(SOURCE).display(),
            live.main.display()
        ),
    );
    assert_eq!(live.version("tasks"), None);
    // Where it holds none, the first writer makes it.
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
    live.home().stop().unwrap();
    let ran = live.run(Path::new(TASKS), &live.wt, &under, &["list"], "");
    refused(&ran, &live.under_cargo(&cargo));
    assert!(live.home().recorded().is_none());

    live.home().ensure().unwrap();
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
        help.out
            .starts_with("usage: just tasks migrate [-h] [--as ACTOR]\n\nMove the board forward"),
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
                    "usage: just tasks migrate [-h] [--as ACTOR]\njust tasks migrate: error: \
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

/// A board server in the common dir `0`, stopped when the case ends, however it ends.
struct Served(PathBuf);

impl Served {
    fn home(&self) -> Home {
        Home::in_common(&self.0, Some(&surreal()))
    }

    /// The schema version of its `tasks` database: one read, which defines and writes nothing.
    fn version(&self) -> Option<i64> {
        let addr = self.home().recorded().expect("the board's server").addr;
        Store::unchecked(
            Client::new(&addr, "sf", "tasks"),
            Box::new(|| NOW),
            Box::new(|_| true),
            "",
        )
        .board_version()
        .unwrap()
    }
}

impl Drop for Served {
    fn drop(&mut self) {
        let _ = self.home().stop();
    }
}

/// A repository at `path` holding the file of the project `loose`, committed on `main`.
fn loose_project(path: &Path) -> PathBuf {
    let seed = repository(path);
    std::fs::write(seed.join("symphony.toml"), "[project]\nid = \"loose\"\n").unwrap();
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
        let addr = served.home().ensure().unwrap().addr;
        seed(Client::new(&addr, "sf", "tasks"));
        let tasks =
            |args: &[&str]| run(&base, &machine, Path::new(TASKS), &checkout, &[], args, "");

        refused(
            &tasks(&["list"]),
            &format!(
                "the loose board is at schema {}, behind this build's {}, and its repository has \
                 no main checkout to say which schema is its own, so only `just tasks migrate` run \
                 in a checkout of it moves it forward: nothing was written; `just tasks migrate` in \
                 any checkout of the repository at {} moves it, and a newer schema is tried on a \
                 scratch board (`--database NAME`)",
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
