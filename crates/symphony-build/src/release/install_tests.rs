//! A release's install in a temporary machine folder, its build a stand-in for cargo that writes
//! the four binaries, each answering what the install asks of it.

use super::*;
use crate::test_tree::Tree;
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant};

const TAG: &str = "release-2026-11-20";
const COMMIT: &str = "0123456789abcdef";

fn executable(tree: &Tree, file: &str, body: &str) -> PathBuf {
    let path = tree.write(file, body);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// A stand-in for cargo in `tree` that says its arguments and what it was told of the client in
/// `builds.log`, then writes into the build folder it is given each of `binaries`: a server built at
/// the board schema `server` and a board at `board`. It exits `status`.
fn builder(tree: &Tree, binaries: &[&str], (server, board): (i64, i64), status: i32) -> OsString {
    let log = tree.path("builds.log");
    let mut script = format!(
        "#!/bin/sh\necho \"$* beside=$SYMPHONY_CLIENT_BESIDE board=${{TASKS_SERVER-none}}\" >> '{}'\n\
         echo building\nout=\"$CARGO_TARGET_DIR/release\"\nmkdir -p \"$out\"\n",
        log.display()
    );
    for binary in binaries {
        let body = match *binary {
            "symphony-server" => format!(
                "[ \"$1\" = --serves ] || exit 9; echo '{{\"protocol\":{{\"major\":0,\"minor\":3}},\
                 \"schema\":{server},\"pin\":1}}'"
            ),
            "tasks" => format!(
                "[ \"$1\" = api ] || exit 9; grep -q '\"method\": \"version\"' || exit 8; echo \
                 '{{\"ok\": {{\"api\": {{\"major\": 1, \"minor\": 14}}, \"schema\": {board}}}}}'"
            ),
            _ => "exit 0".to_string(),
        };
        script.push_str(&format!(
            "cat > \"$out/{binary}\" <<'BINARY'\n#!/bin/sh\n{body}\nBINARY\nchmod +x \
             \"$out/{binary}\"\n"
        ));
    }
    script.push_str(&format!("exit {status}\n"));
    executable(tree, "cargo-stand-in", &script).into_os_string()
}

/// The release's checkout in `tree`, holding a client's distribution of two files.
fn checkout(tree: &Tree) -> PathBuf {
    tree.write(
        "checkout/apps/symphony-server/client/public/index.html",
        "<html>",
    );
    tree.write(
        "checkout/apps/symphony-server/client/public/assets/a.js",
        "a()",
    );
    tree.path("checkout")
}

/// The release `tag` installed from `tree`'s checkout into its `machine/` by `builder`, at
/// `commit`, its log in `logs`, `current` as it was.
fn installed(tree: &Tree, builder: &OsString, tag: &str, commit: &str, logs: &str) -> Installed {
    install(&Install {
        checkout: &tree.path("checkout"),
        machine: &tree.path("machine"),
        tag,
        commit,
        logs: &tree.path(logs),
        builder: Some(builder),
        until: Bound::until(Instant::now() + Duration::from_secs(600)),
    })
}

/// [`TAG`] installed at `commit`, then `current` pointed at it, as a refresh that nothing holds
/// does.
fn run(tree: &Tree, builder: &OsString, commit: &str) -> Installed {
    let mut made = installed(tree, builder, TAG, commit, "logs");
    point(&tree.path("machine"), &mut made);
    made
}

/// What `machine/releases` holds, by name.
fn releases(tree: &Tree) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(tree.path("machine/releases"))
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

fn builds(tree: &Tree) -> Vec<String> {
    fs::read_to_string(tree.path("builds.log"))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn a_green_build_is_installed_whole_with_its_manifest_and_current_pointed_at_it() {
    let tree = Tree::new();
    checkout(&tree);
    let builder = builder(&tree, &release::BINARIES, (18, 18), 0);
    let installed = run(&tree, &builder, COMMIT);
    assert!(installed.ran.green(), "{:?}", installed.ran);
    assert_eq!(installed.ran.name, "refresh-install");
    assert_eq!(
        installed.ran.command,
        "the install of release-2026-11-20 on the machine"
    );
    // The four binaries on the release profile, the server told its client is beside
    // it, and no board of the run's named to the build.
    assert_eq!(
        builds(&tree),
        [
            "build --release --locked -p symphony-server -p symphony-board -p symphony-app --bin \
             symphony-server --bin tasks --bin agent-send --bin symphony beside=1 board=none"
        ]
    );
    let machine = tree.path("machine");
    assert_eq!(releases(&tree), [TAG]);
    let manifest = release::installed(&machine, TAG).unwrap().unwrap();
    assert_eq!(manifest.release, TAG);
    assert_eq!(manifest.commit, COMMIT);
    assert_eq!(
        manifest.serves,
        Serves {
            schema: 18,
            additions: 0,
            protocol: Version { major: 0, minor: 3 },
            tasks_api: Version {
                major: 1,
                minor: 14
            },
            pin: 1,
        }
    );
    let paths: Vec<&str> = (manifest.artifacts.iter())
        .map(|artifact| artifact.path.as_str())
        .collect();
    assert_eq!(
        paths,
        [
            "bin/symphony-server",
            "bin/tasks",
            "bin/agent-send",
            "bin/symphony",
            manifest.client.as_str()
        ]
    );
    assert!(manifest.artifacts.iter().all(|artifact| artifact.size > 0));
    assert_eq!(manifest.artifacts[4].size, 6 + 3);
    assert_eq!(release::current(&machine).as_deref(), Some(TAG));
    assert!(symphony::builds::released(&machine, "tasks").is_some());
    let place = release::folder(&machine, TAG);
    assert_eq!(installed.said.len(), 3, "{:?}", installed.said);
    assert!(
        installed.said[0].starts_with(&format!(
            "installed {TAG} in {}, built in ",
            place.display()
        )) && installed.said[0].ends_with(&format!(" s: {}", manifest.sizes())),
        "{}",
        installed.said[0]
    );
    assert_eq!(
        installed.said[1],
        format!(
            "{TAG} serves board schema 18, plugin protocol 0.3 and tasks api 1.14, at Symphony \
             pin 1"
        )
    );
    assert_eq!(
        installed.said[2],
        format!(
            "{} names {TAG} (it named no release): a project with no build of its own runs it",
            machine.join("current").display()
        )
    );
    // The log holds the build's output and what was said after it.
    let log = fs::read_to_string(&installed.ran.log).unwrap();
    assert_eq!(installed.ran.log, tree.path("logs/refresh-install.log"));
    assert!(
        log.starts_with("$ the install of release-2026-11-20 on the machine\nbuilding\n"),
        "{log}"
    );
    assert!(log.ends_with(&format!("{}\n", installed.said[2])), "{log}");
    assert!(!tree.path("logs/refresh-install.request.json").exists());
}

#[test]
fn a_failed_build_leaves_no_folder_and_current_where_it_was() {
    let tree = Tree::new();
    checkout(&tree);
    let machine = tree.path("machine");
    // An earlier release, which current names.
    let before = builder(&tree, &release::BINARIES, (17, 17), 0);
    let mut earlier = installed(
        &tree,
        &before,
        "release-2026-11-19",
        "earlier",
        "logs-earlier",
    );
    assert!(point(&machine, &mut earlier) && earlier.ran.green());
    // The build fails, having written three binaries.
    let failing = builder(&tree, &release::BINARIES[..3], (18, 18), 101);
    let installed = run(&tree, &failing, COMMIT);
    assert!(!installed.ran.green());
    assert_eq!(installed.ran.exit_code, Some(101));
    assert_eq!(
        installed.said,
        [format!(
            "{TAG} is not installed: its build failed (exit 101), and nothing of it is in {}",
            machine.join("releases").display()
        )]
    );
    assert_eq!(releases(&tree), ["release-2026-11-19"]);
    assert!(installed.manifest.is_none());
    assert_eq!(
        release::current(&machine).as_deref(),
        Some("release-2026-11-19")
    );
    // A red install has nothing to point at: current stays, and nothing more is said.
    let mut red = installed;
    assert!(!point(&machine, &mut red));
    assert_eq!(red.said.len(), 1);
    assert_eq!(
        release::current(&machine).as_deref(),
        Some("release-2026-11-19")
    );
}

#[test]
fn an_install_points_nothing_and_a_point_that_fails_is_its_red() {
    let tree = Tree::new();
    checkout(&tree);
    let machine = tree.path("machine");
    let builder = builder(&tree, &release::BINARIES, (18, 18), 0);
    let mut made = installed(&tree, &builder, TAG, COMMIT, "logs");
    assert!(made.ran.green(), "{:?}", made.ran);
    assert_eq!(releases(&tree), [TAG]);
    assert_eq!(
        release::current(&machine),
        None,
        "the install points nothing"
    );
    assert_eq!(made.said.len(), 2, "{:?}", made.said);
    assert_eq!(
        made.manifest.as_ref().map(|m| m.release.as_str()),
        Some(TAG)
    );
    // A folder where the link goes: the point fails, current is not made, and the step is red.
    fs::create_dir_all(machine.join("current")).unwrap();
    assert!(!point(&machine, &mut made));
    assert!(!made.ran.green());
    let why = made.ran.error.clone().unwrap();
    assert_eq!(
        why,
        format!("{} is not a link", machine.join("current").display())
    );
    assert_eq!(
        made.said[2],
        format!("{TAG} is installed, and current is as it was: {why}")
    );
    assert!(machine.join("current").is_dir());
    let log = fs::read_to_string(&made.ran.log).unwrap();
    assert!(log.ends_with(&format!("{}\n", made.said[2])), "{log}");
}

#[test]
fn a_build_that_leaves_a_binary_out_or_no_client_is_red_and_installs_nothing() {
    let tree = Tree::new();
    checkout(&tree);
    let machine = tree.path("machine");
    // Green, and without `symphony`.
    let partial = builder(&tree, &release::BINARIES[..3], (18, 18), 0);
    let installed = run(&tree, &partial, COMMIT);
    assert!(!installed.ran.green());
    let why = installed.ran.error.clone().unwrap();
    assert!(
        why.starts_with("the build has no symphony that runs at "),
        "{why}"
    );
    assert_eq!(
        installed.said,
        [format!(
            "{TAG} is not installed: {why}; nothing of it is in {}",
            machine.join("releases").display()
        )]
    );
    assert_eq!(releases(&tree), [] as [&str; 0]);
    assert_eq!(release::current(&machine), None);
    assert!(
        fs::read_to_string(&installed.ran.log)
            .unwrap()
            .ends_with(&format!("{why}\n"))
    );
    // Every binary, and a commit that holds no client distribution.
    fs::remove_dir_all(tree.path("checkout/apps")).unwrap();
    let whole = builder(&tree, &release::BINARIES, (18, 18), 0);
    let installed = run(&tree, &whole, COMMIT);
    let why = installed.ran.error.unwrap();
    assert!(why.ends_with("has no index.html"), "{why}");
    assert_eq!(releases(&tree), [] as [&str; 0]);
}

#[test]
fn a_release_whose_server_and_board_differ_in_schema_is_never_installed() {
    let tree = Tree::new();
    checkout(&tree);
    let builder = builder(&tree, &release::BINARIES, (18, 19), 0);
    let installed = run(&tree, &builder, COMMIT);
    assert_eq!(
        installed.ran.error.as_deref(),
        Some(
            "the release's symphony-server is built at board schema 18 and its tasks at 19: one \
             release holds one schema"
        )
    );
    assert_eq!(releases(&tree), [] as [&str; 0]);
}

#[test]
fn a_release_installed_already_is_not_built_again_and_current_is_pointed() {
    let tree = Tree::new();
    checkout(&tree);
    let machine = tree.path("machine");
    let builder = builder(&tree, &release::BINARIES, (18, 18), 0);
    assert!(run(&tree, &builder, COMMIT).ran.green());
    let manifest = release::installed(&machine, TAG).unwrap().unwrap();
    // The link lost, as a refresh that died between the two would leave it.
    fs::remove_file(machine.join("current")).unwrap();
    let again = run(&tree, &builder, COMMIT);
    assert!(again.ran.green(), "{:?}", again.ran);
    assert_eq!(builds(&tree).len(), 1, "built once");
    assert_eq!(
        again.said,
        [
            format!(
                "{TAG} is installed already in {}: {}",
                release::folder(&machine, TAG).display(),
                manifest.sizes()
            ),
            format!(
                "{} names {TAG} (it named no release): a project with no build of its own runs \
                 it",
                machine.join("current").display()
            )
        ]
    );
    assert_eq!(release::current(&machine).as_deref(), Some(TAG));
    let log = fs::read_to_string(&again.ran.log).unwrap();
    assert!(log.contains("is installed already in"), "{log}");
    // Once more, with the link in place: it says so.
    let third = run(&tree, &builder, COMMIT);
    assert!(third.said[1].contains(&format!("names {TAG} (as it did)")));
    // The same name holding another commit is refused, and nothing is built or moved.
    let other = run(&tree, &builder, "another");
    assert!(!other.ran.green());
    let why = other.ran.error.unwrap();
    assert!(
        why.contains(&format!(
            "holds the commit {COMMIT}, not the release's another"
        )),
        "{why}"
    );
    assert_eq!(other.said, [format!("not installed: {why}")]);
    assert_eq!(builds(&tree).len(), 1);
    assert_eq!(
        release::installed(&machine, TAG).unwrap().unwrap(),
        manifest
    );
}

#[test]
fn a_folder_in_the_releases_place_that_is_no_whole_release_is_refused() {
    let tree = Tree::new();
    checkout(&tree);
    tree.write(&format!("machine/releases/{TAG}/manifest.json"), "{");
    let builder = builder(&tree, &release::BINARIES, (18, 18), 0);
    let installed = run(&tree, &builder, COMMIT);
    let why = installed.ran.error.unwrap();
    assert!(
        why.starts_with("the release folder is there and is not a whole release ("),
        "{why}"
    );
    assert_eq!(builds(&tree), [] as [&str; 0]);
}

#[test]
fn the_newer_release_takes_current_and_names_the_one_before() {
    let tree = Tree::new();
    checkout(&tree);
    let machine = tree.path("machine");
    let builder = builder(&tree, &release::BINARIES, (18, 18), 0);
    for tag in ["release-2026-11-19", TAG] {
        let mut made = installed(&tree, &builder, tag, tag, &format!("logs/{tag}"));
        assert!(point(&machine, &mut made) && made.ran.green());
        if tag == TAG {
            assert!(
                made.said[2].contains("(it named release-2026-11-19, which stays installed)"),
                "{:?}",
                made.said
            );
        }
    }
    assert_eq!(releases(&tree), ["release-2026-11-19", TAG]);
    assert_eq!(release::current(&machine).as_deref(), Some(TAG));
}

#[test]
fn what_the_binaries_say_is_read_whole_or_refused() {
    let server = r#"{"protocol":{"major":0,"minor":3},"schema":18,"pin":1}"#;
    let board = r#"{"ok": {"api": {"major": 1, "minor": 14}, "schema": 18}}"#;
    assert_eq!(read_serves(server, board).map(|serves| serves.pin), Ok(1));
    // A server built before it said its pin is read at pin 0, older than every project's.
    let unpinned = r#"{"protocol":{"major":0,"minor":3},"schema":18}"#;
    assert_eq!(read_serves(unpinned, board).map(|serves| serves.pin), Ok(0));
    // Its additive steps on the schema: none from a server built before it said them.
    assert_eq!(
        read_serves(server, board).map(|serves| serves.additions),
        Ok(0)
    );
    let stepped = r#"{"protocol":{"major":0,"minor":3},"schema":18,"additions":2,"pin":1}"#;
    assert_eq!(
        read_serves(stepped, board).map(|serves| serves.additions),
        Ok(2)
    );
    for (server, board, why) in [
        (
            "nonsense",
            board,
            "what symphony-server --serves said is not JSON: ",
        ),
        (server, "", "what tasks api answered is not JSON: "),
        (
            server,
            r#"{"error": "no board"}"#,
            r#"tasks api did not answer its version: {"error": "no board"}"#,
        ),
        (
            r#"{"protocol":{"major":0,"minor":3}}"#,
            board,
            "the release's symphony-server names no board schema",
        ),
        (
            server,
            r#"{"ok": {"api": {"major": 1, "minor": 14}}}"#,
            "the release's tasks names no board schema",
        ),
        (
            r#"{"protocol":{"major":0},"schema":18}"#,
            board,
            "the release's build names no plugin protocol minor",
        ),
        (
            r#"{"schema":18}"#,
            board,
            "the release's build names no plugin protocol major",
        ),
        (
            server,
            r#"{"ok": {"api": {"major": -1, "minor": 14}, "schema": 18}}"#,
            "the release's build names no tasks api major",
        ),
        (
            r#"{"protocol":{"major":0,"minor":3},"schema":18,"pin":4294967296}"#,
            board,
            "the release's symphony-server names the pin 4294967296",
        ),
        (
            r#"{"protocol":{"major":0,"minor":3},"schema":18,"additions":-1}"#,
            board,
            "the release's symphony-server names the additive steps -1",
        ),
        (
            r#"{"protocol":{"major":0,"minor":3},"schema":18,"additions":"2"}"#,
            board,
            "the release's symphony-server names the additive steps \"2\"",
        ),
    ] {
        let refused = read_serves(server, board).unwrap_err();
        assert!(refused.starts_with(why), "{refused}");
    }
}

#[test]
fn a_binary_that_cannot_say_what_it_serves_is_the_installs_red() {
    let tree = Tree::new();
    checkout(&tree);
    // A server that refuses the question.
    let script = "#!/bin/sh\nout=\"$CARGO_TARGET_DIR/release\"\nmkdir -p \"$out\"\nfor b in \
                  symphony-server tasks agent-send symphony; do printf '#!/bin/sh\\necho no >&2; \
                  exit 3\\n' > \"$out/$b\"; chmod +x \"$out/$b\"; done\n";
    let builder = executable(&tree, "refusing", script).into_os_string();
    let installed = run(&tree, &builder, COMMIT);
    let why = installed.ran.error.unwrap();
    assert!(
        why.starts_with("the release's symphony-server, asked what it serves, ended ")
            && why.ends_with(": no"),
        "{why}"
    );
    assert_eq!(releases(&tree), [] as [&str; 0]);
    // A builder that is not there never ran.
    let gone = tree.path("gone").into_os_string();
    let installed = run(&tree, &gone, COMMIT);
    assert!(!installed.ran.green());
    assert!(
        installed.said[0].starts_with(&format!("{TAG} is not installed: its build failed (")),
        "{:?}",
        installed.said
    );
}

#[test]
fn the_build_is_cargos_through_the_checkouts_launcher_unless_a_stand_in_is_named() {
    let named = step(TAG);
    let built = build(&named, Path::new("/c/.target"), None);
    assert_eq!(built.argv[..2], ["bash", "tools/cargo.sh"]);
    assert_eq!(built.argv[2..5], ["build", "--release", "--locked"]);
    assert_eq!(built.name, STEP);
    assert_eq!(
        built.set,
        [
            ("SYMPHONY_CLIENT_BESIDE".to_string(), "1".to_string()),
            ("CARGO_TARGET_DIR".to_string(), "/c/.target".to_string())
        ]
    );
    let stood_in = build(&named, Path::new("/c/.target"), Some("/x/cargo".as_ref()));
    assert_eq!(stood_in.argv[0], "/x/cargo");
    assert_eq!(stood_in.argv[1..], built.argv[2..]);
}

/// The install as a release makes it, with cargo itself: this checkout's four binaries built on
/// the release profile in its own build folder and installed in a machine folder under that build
/// folder (`.target/install-trial/machine`), which is left for a look and goes with the build
/// folder. It takes a release build's time, so it runs only when asked:
/// `tools/cargo.sh test -p symphony-build --lib the_real_build -- --ignored --nocapture`.
#[test]
#[ignore = "a release build of four binaries: minutes, and gigabytes of build folder"]
fn the_real_build_installs_this_checkouts_release() {
    let checkout = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let checkout = checkout.canonicalize().unwrap();
    let trial = checkout.join(".target/install-trial");
    let _ = fs::remove_dir_all(&trial);
    let machine = trial.join("machine");
    let mut installed = install(&Install {
        checkout: &checkout,
        machine: &machine,
        tag: "release-trial",
        commit: "the working tree",
        logs: &trial.join("logs"),
        builder: None,
        until: Bound::until(Instant::now() + Duration::from_secs(2 * 3600)),
    });
    point(&machine, &mut installed);
    for line in &installed.said {
        println!("{line}");
    }
    println!("the build took {:.0} s", installed.ran.seconds);
    assert!(installed.ran.green(), "{:?}", installed.ran);
    let manifest = release::installed(&machine, "release-trial")
        .unwrap()
        .unwrap();
    assert_eq!(manifest.serves.schema, symphony_board::SCHEMA);
    assert_eq!(manifest.serves.pin, symphony::machine::PIN);
    assert_eq!(
        manifest.serves.protocol.minor,
        symphony_plugin::protocol::PROTOCOL.minor
    );
    assert_eq!(release::current(&machine).as_deref(), Some("release-trial"));
}

/// `git ARGS` in `tree`'s checkout, as an author of the test's own.
fn checkout_git(tree: &Tree, args: &[&str]) {
    let named = ["-c", "user.name=t", "-c", "user.email=t@example.com"];
    super::super::git::git(&tree.path("checkout"), &[&named[..], args].concat()).unwrap();
}

#[test]
fn an_install_holds_the_notes_of_every_release_up_to_its_own_from_the_checkouts_history() {
    let tree = Tree::new();
    checkout(&tree);
    checkout_git(&tree, &["init", "-q", "-b", "main"]);
    tree.write(
        "checkout/crates/tasks/src/schema.rs",
        "pub const VERSION: i64 = 17;\n",
    );
    checkout_git(&tree, &["add", "-A"]);
    checkout_git(&tree, &["commit", "-q", "-m", "first"]);
    checkout_git(&tree, &["tag", "-a", "release-2026-11-19", "-m", "one"]);
    tree.write(
        "checkout/crates/tasks/src/schema.rs",
        "pub const VERSION: i64 = 18;\n",
    );
    checkout_git(&tree, &["add", "-A"]);
    let said = "Raise the schema\n\nConsumer-Note: once: run `just symphony-cli init --root .`";
    checkout_git(&tree, &["commit", "-q", "-m", said]);
    checkout_git(&tree, &["tag", "-a", TAG, "-m", "two"]);
    let builder = builder(&tree, &release::BINARIES, (18, 18), 0);
    let commit = super::super::git::git(&tree.path("checkout"), &["rev-parse", "HEAD"]).unwrap();
    let installed = run(&tree, &builder, &commit);
    assert!(installed.ran.green(), "{:?}", installed.ran);
    // Read as a project reads them: the newest noted release's, with no repository.
    let (newest, notes) = release::noted(&tree.path("machine")).unwrap();
    assert_eq!(newest, TAG);
    let taken = release::notes_between(&notes, Some("release-2026-11-19"), None).unwrap();
    assert!(
        taken.starts_with(&format!(
            "## release-2026-11-20\n\nCommit: {commit}\nFrom release-2026-11-19.\n\nCargo.toml and \
             Cargo.lock:\n- Name this release in each of the project's Symphony dependency lines, \
             `tag = \"release-2026-11-20\"`, or `rev = \"{commit}\"`"
        )) && taken.contains("\nBoard:\n- Board schema 17 to 18. ")
            && taken.ends_with("Run once:\n- run `just symphony-cli init --root .`\n"),
        "{taken}"
    );
    assert!(notes.contains("\nThe first release.\n"), "{notes}");
    // The same list by the commit the project is pinned at.
    let first = super::super::git::git(&tree.path("checkout"), &["rev-parse", "HEAD~1"]).unwrap();
    assert_eq!(
        release::notes_between(&notes, Some(&first[..12]), None).unwrap(),
        taken
    );
}

#[test]
fn a_checkout_git_cannot_read_is_installed_without_notes_and_its_log_says_so() {
    let tree = Tree::new();
    checkout(&tree);
    let builder = builder(&tree, &release::BINARIES, (18, 18), 0);
    let installed = run(&tree, &builder, COMMIT);
    assert!(installed.ran.green(), "{:?}", installed.ran);
    assert_eq!(release::noted(&tree.path("machine")), None);
    let log = tree.read("logs/refresh-install.log");
    assert!(log.contains("no notes.md: git "), "{log}");
}

#[test]
fn the_installs_line_names_the_additive_steps_of_its_schema_when_it_has_any() {
    let line = |additions| {
        served(&Manifest {
            release: TAG.into(),
            commit: COMMIT.into(),
            serves: Serves {
                schema: 28,
                additions,
                protocol: Version { major: 0, minor: 3 },
                tasks_api: Version {
                    major: 1,
                    minor: 14,
                },
                pin: 1,
            },
            client: "client/abc".into(),
            artifacts: Vec::new(),
        })
    };
    let rest = "plugin protocol 0.3 and tasks api 1.14, at Symphony pin 1";
    assert_eq!(line(0), format!("{TAG} serves board schema 28, {rest}"));
    assert_eq!(
        line(1),
        format!("{TAG} serves board schema 28 with its additive step, {rest}")
    );
    assert_eq!(
        line(3),
        format!("{TAG} serves board schema 28 with its 3 additive steps, {rest}")
    );
}
