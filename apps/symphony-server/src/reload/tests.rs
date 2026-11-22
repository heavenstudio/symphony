use super::{Retry, RunningImage, Setup, source_event, stamp_install, watch_with};
use crate::backstop::{self, Backstop};
use crate::front::{Shared, told};
use crate::testing::handing::{Handing, event, renamed, tell_at, watching};
use crate::testing::{Tree, git, joined, repository};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[test]
fn a_source_edit_whose_event_is_lost_still_starts_a_build_within_a_period() {
    let mut tree = Tree::new();
    tree.root = tree.root.canonicalize().unwrap();
    let root = repository(&tree);
    tree.write(
        "main/symphony/apps/symphony-server/Cargo.toml",
        "[package]\nname = \"symphony-server\"\n",
    );
    tree.write(
        "main/symphony/apps/symphony-server/src/main.rs",
        "fn main() {}\n",
    );
    std::fs::create_dir_all(root.join("tools")).unwrap();
    tree.write("main/apps/symphony-host/Cargo.toml", "[package]\nname = 'symphony-host'\n[dependencies]\nsymphony-server = { path = '../../symphony/apps/symphony-server' }\n");
    tree.write(
        "main/apps/symphony-host/src/bin/symphony-server.rs",
        "fn main() {}\n",
    );
    // The owned build command records its selected package and fails immediately. It is the
    // machine's copy of the fixture script, already run, so the bound on each build covers no
    // first exec of a new file.
    let build = root.join("tools/rs-bin.sh");
    crate::fixture_script::write(
        &build,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" > \"${0%/tools/rs-bin.sh}/build-args\"\nexit 1\n",
        false,
    );
    crate::testing::assert_primed(&build);
    // The project names that command as its Cargo: the server rebuilds through it.
    tree.write(
        "main/symphony.toml",
        "[project]\ncargo = [\"tools/rs-bin.sh\"]\n",
    );
    git(&root, &["add", "."]);
    git(&root, &["commit", "-qm", "Reload sources"]);
    // The installed client is current, so only the server build runs.
    let home = tree.path("home");
    tree.write("home/client/public/index.html", "client");
    // The installed bundle's hash: each file's name and contents, each after its length.
    let mut distribution = Sha256::new();
    let (name, contents) = ("index.html", "client");
    distribution.update(name.len().to_le_bytes());
    distribution.update(name.as_bytes());
    distribution.update(contents.len().to_le_bytes());
    distribution.update(contents.as_bytes());
    let distribution: String = distribution
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    tree.write(
        "home/client-installed.json",
        &serde_json::json!({
            "source": symphony::reload::client_fingerprint(&root).unwrap(),
            "distribution": distribution,
        })
        .to_string(),
    );
    let shared = Shared::new(
        i64::from(std::process::id()),
        1,
        &root,
        Some(&home),
        crate::testing::open_gate(),
    );
    let hub = Arc::new(crate::live::Hub::new(&root, &home.join("client/public")));
    assert!(shared.api.set(hub.clone()).is_ok());
    let serve = crate::cli::Serve {
        home: Some(home.clone()),
        root: root.clone(),
        port: 1,
        hosts: Vec::new(),
        install: None,
    };
    let setup = Setup {
        listeners: Vec::new(),
        lease: Some(std::fs::File::create(home.join("server.lock")).unwrap()),
        control: None,
        fingerprint: Some(symphony::reload::fingerprint(&root).unwrap()),
        running: None,
    };
    let period = Duration::from_millis(500);
    // A watcher that never delivers: past the startup read, only the period sees an edit.
    let worker = watch_with::<notify::NullWatcher>(
        serve,
        setup,
        shared.clone(),
        Vec::new(),
        Backstop::new(period),
        Retry::new(),
    )
    .unwrap();
    // The first edit may land before or after the startup read; the second lands after the
    // first attempt has ended, so the worker is waiting again.
    for edit in ["first", "second"] {
        hub.server_notice.send_replace(String::new());
        tree.write(
            "main/symphony/apps/symphony-server/src/main.rs",
            &format!("fn main() {{ /* {edit} edit */ }}\n"),
        );
        let failed = test_time::until(
            format!("a build for the {edit} edit"),
            test_time::backstop(period + Duration::from_secs(30)),
            || Some(hub.server_notice.borrow().clone()).filter(|text| !text.is_empty()),
        )
        .unwrap();
        assert!(
                failed.starts_with(
                    "Symphony server update failed; the previous server is still serving. symphony-server build exit status: 1"
                ),
                "{failed}"
            );
        assert!(
            std::fs::read_to_string(root.join("build-args"))
                .unwrap()
                .contains("--path apps/symphony-host")
        );
    }
    shared.stop();
    joined(worker, "the stop to end the reload loop", stall());
}

#[test]
fn client_fingerprint_and_events_cover_assets_config_and_transitive_crates() {
    let tree = Tree::new();
    let root = repository(&tree);
    tree.write(
        "main/symphony/apps/symphony-server/Cargo.toml",
        "[package]\nname = \"symphony-server\"\n[dependencies]\nwire = { path = \"../../../crates/wire\" }\n",
    );
    tree.write(
        "main/symphony/apps/symphony-server/src/main.rs",
        "fn main() {}\n",
    );
    tree.write("main/symphony/apps/symphony-web/package.json", "{}\n");
    tree.write(
        "main/crates/wire/Cargo.toml",
        "[package]\nname = \"wire\"\n[build-dependencies]\nleaf = { path = \"../leaf\" }\n",
    );
    tree.write("main/crates/wire/src/lib.rs", "// wire\n");
    tree.write(
        "main/crates/leaf/Cargo.toml",
        "[package]\nname = \"leaf\"\n",
    );
    tree.write("main/crates/leaf/src/lib.rs", "// leaf\n");
    tree.write("main/apps/symphony-host/Cargo.toml", "[package]\nname = 'symphony-host'\n[dependencies]\nsymphony-server = { path = '../../symphony/apps/symphony-server' }\n");
    tree.write(
        "main/apps/symphony-host/src/bin/symphony-server.rs",
        "fn main() {}\n",
    );
    let directories = symphony::reload::source_directories(&root).unwrap();
    assert!(directories.contains(&root.join("crates/leaf")));
    let mut before = symphony::reload::client_fingerprint(&root).unwrap();
    let server_before = symphony::reload::fingerprint(&root).unwrap();
    tree.write(
        "main/apps/symphony-host/src/bin/symphony-server.rs",
        "fn main() { /* host */ }\n",
    );
    assert_ne!(server_before, symphony::reload::fingerprint(&root).unwrap());
    assert_eq!(before, symphony::reload::client_fingerprint(&root).unwrap());
    assert!(source_event(
        &root,
        &directories,
        &root.join("apps/symphony-host/src/bin/symphony-server.rs")
    ));
    // Each build input, and whether the client's build reads it: the web client's package but
    // what its installs and builds write there, the shell's notice styles, the launchers and the
    // workspace's settings. The server's crates are its own build's alone.
    for (file, client) in [
        ("symphony/apps/symphony-web/src/kinds/board.tsx", true),
        ("symphony/apps/symphony-web/src/styles/frame.css", true),
        ("symphony/apps/symphony-web/shell/shell.ts", true),
        ("symphony/apps/symphony-web/package.json", true),
        ("symphony/apps/symphony-web/package-lock.json", true),
        ("symphony/apps/symphony-web/vite.config.ts", true),
        ("symphony/apps/symphony-web/.node-version", true),
        ("symphony/apps/symphony-web/build.sh", true),
        ("symphony/apps/symphony-web/package.sh", true),
        ("symphony/apps/symphony-web/npm.sh", true),
        ("symphony/apps/symphony-server/src/notices.css", true),
        ("symphony/tools/node.sh", true),
        ("crates/wire/src/lib.rs", false),
        ("crates/leaf/src/lib.rs", false),
        ("crates/leaf/build.rs", false),
        ("Cargo.toml", true),
        ("Cargo.lock", true),
        ("symphony/Cargo.toml", true),
        ("symphony/Cargo.lock", true),
        ("symphony/.cargo/config.toml", true),
        ("symphony/justfile", true),
        ("symphony/tools/rs-bin.sh", true),
        ("symphony/tools/check_lock.sh", true),
        ("rust-toolchain.toml", true),
        (".cargo/config.toml", true),
        ("justfile", true),
        ("tools/rs-bin.sh", true),
    ] {
        let all_before = symphony::reload::fingerprint(&root).unwrap();
        tree.write(&format!("main/{file}"), "changed build input\n");
        let after = symphony::reload::client_fingerprint(&root).unwrap();
        assert_eq!(
            before != after,
            client,
            "client identity and {file}: the client's build reads it, or does not"
        );
        assert_ne!(
            all_before,
            symphony::reload::fingerprint(&root).unwrap(),
            "reload identity must include {file}"
        );
        assert!(
            source_event(&root, &directories, &root.join(file)),
            "watch must wake for {file}"
        );
        before = after;
    }
    // What an install, a build or a coverage run writes in the client's package, its guides and
    // its browser tests' package are no input of its build, and wake no reload.
    for file in [
        "symphony/apps/symphony-web/node_modules/vite/package.json",
        "symphony/apps/symphony-web/.install/lock",
        "symphony/apps/symphony-web/dist/assets/board.js",
        "symphony/apps/symphony-web/coverage/index.html",
        "symphony/apps/symphony-web/AGENTS.md",
        "symphony/apps/symphony-web/src/styles/README.md",
        "symphony/apps/symphony-web-tests/tests/browser/main.rs",
    ] {
        tree.write(&format!("main/{file}"), "not a client input\n");
        assert_eq!(
            before,
            symphony::reload::client_fingerprint(&root).unwrap(),
            "{file} must not rebuild the client"
        );
        assert!(
            !source_event(&root, &directories, &root.join(file)),
            "watch must not wake for {file}"
        );
    }
    for file in [
        "symphony/apps/symphony-server/src/main.rs",
        "docs/TOPIC.md",
        "crates/unrelated/src/lib.rs",
    ] {
        tree.write(&format!("main/{file}"), "not a client input\n");
        assert_eq!(
            before,
            symphony::reload::client_fingerprint(&root).unwrap(),
            "{file} must not rebuild the client"
        );
    }
    std::fs::remove_file(root.join("symphony/apps/symphony-web/src/styles/frame.css")).unwrap();
    assert_ne!(
        before,
        symphony::reload::client_fingerprint(&root).unwrap(),
        "deleted assets must invalidate the client too"
    );
}

#[test]
fn idle_source_watch_retries_when_a_missing_referenced_crate_is_created() {
    let tree = Tree::new();
    let root = repository(&tree);
    tree.write("main/Cargo.toml", "[workspace]\nmembers = [\"symphony/apps/symphony-server\", \"crates/present\", \"crates/late\"]\n");
    tree.write("main/symphony/apps/symphony-server/Cargo.toml", "[package]\nname = \"symphony-server\"\n[dependencies]\npresent = { path = \"../../../crates/present\" }\nlate = { path = \"../../../crates/late\" }\n");
    tree.write(
        "main/symphony/apps/symphony-server/src/main.rs",
        "fn main() {}\n",
    );
    tree.write(
        "main/crates/present/Cargo.toml",
        "[package]\nname = \"present\"\n",
    );
    tree.write("main/crates/present/src/lib.rs", "pub fn present() {}\n");
    let directories = symphony::reload::source_directories(&root).unwrap();
    assert_eq!(
        directories.len(),
        2,
        "this is a partial graph, not the empty-graph fallback"
    );
    assert!(!directories.contains(&root.join("crates/late")));
    // The failed build's attempted fingerprint excludes the still-missing dependency.
    let attempted = symphony::reload::fingerprint(&root).unwrap();

    tree.write(
        "main/crates/unrelated/Cargo.toml",
        "[package]\nname = \"unrelated\"\n",
    );
    assert_eq!(
        symphony::reload::fingerprint(&root).unwrap(),
        attempted,
        "rediscovery must not rebuild for an unrelated manifest"
    );
    assert!(!source_event(
        &root,
        &directories,
        &root.join("crates/unrelated/src/lib.rs")
    ));

    tree.write("main/crates/late/src/lib.rs", "pub fn late() {}\n");
    let manifest = tree
        .write(
            "main/crates/late/Cargo.toml",
            "[package]\nname = \"late\"\n",
        )
        .canonicalize()
        .unwrap();
    assert!(
        source_event(&root, &directories, &manifest),
        "creation of the missing manifest must wake the reload worker using the old graph"
    );
    let retry = symphony::reload::fingerprint(&root).unwrap();
    assert_ne!(
        retry, attempted,
        "the worker must retry the repaired source set"
    );
    let recovered = symphony::reload::source_directories(&root).unwrap();
    assert!(recovered.contains(&root.join("crates/late")));
    assert!(!recovered.contains(&root.join("crates/unrelated")));
    assert!(source_event(
        &root,
        &recovered,
        &root.join("crates/late/src/lib.rs")
    ));
}

#[test]
fn idle_source_watch_follows_build_inputs_and_recovers_an_incomplete_graph() {
    let root = Path::new("/repo");
    let directories = BTreeSet::from([
        root.join("symphony/apps/symphony-server"),
        root.join("symphony/crates/tasks"),
        root.join("symphony/crates/symphony-api"),
    ]);
    for (path, expected) in [
        ("Cargo.toml", true),
        ("Cargo.lock", true),
        (".cargo/config.toml", true),
        ("symphony/crates/tasks/Cargo.toml", true),
        ("symphony/crates/tasks/build.rs", true),
        ("symphony/crates/tasks/src/new/module.rs", true),
        ("symphony/apps/symphony-server/src/api.rs", true),
        ("symphony/apps/symphony-web/src/kinds/board.tsx", true),
        ("symphony/apps/symphony-web/src/styles/frame.css", true),
        ("symphony/apps/symphony-web/shell/shell.ts", true),
        ("symphony/apps/symphony-web/package-lock.json", true),
        ("symphony/apps/symphony-web/vite.config.ts", true),
        ("symphony/apps/symphony-web/build.sh", true),
        ("symphony/apps/symphony-web/package.sh", true),
        (
            "symphony/apps/symphony-web/node_modules/vite/index.js",
            false,
        ),
        ("symphony/apps/symphony-web/.install/lock", false),
        ("symphony/apps/symphony-web/AGENTS.md", false),
        ("symphony/crates/symphony-api/src/lib.rs", true),
        ("tools/rs-bin.sh", true),
        ("justfile", true),
        ("symphony/crates/tasks/tests/store.rs", false),
        ("docs/README.md", false),
        (".target/debug/symphony-server", false),
    ] {
        assert_eq!(
            source_event(root, &directories, &root.join(path)),
            expected,
            "{path}"
        );
    }
    assert!(source_event(
        root,
        &BTreeSet::new(),
        &root.join("crates/new/src/lib.rs")
    ));
    assert!(!source_event(
        root,
        &BTreeSet::new(),
        &root.join(symphony::project::Project::of(root).artifact("log.txt"))
    ));
}

#[test]
fn a_spawn_racing_a_handover_never_masks_the_replacements_death() {
    use super::{Answer, Control, answer};
    use std::io::{ErrorKind, Read};
    use std::os::fd::AsRawFd;
    use std::sync::atomic::{AtomicBool, Ordering};
    use symphony_board::sys::{self, Child, Command, Stdio};

    /// Children killed and reaped however the test ends.
    struct Children(Vec<Child>);
    impl Drop for Children {
        fn drop(&mut self) {
            for child in &mut self.0 {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    let spawned = AtomicBool::new(false);
    std::thread::scope(|scope| {
        // Children outliving every handover, spawned beside its pipes' making through the
        // helpers, as a build or a board command is spawned beside a reload. A std spawn that
        // skips the lock could keep a real pipe's first ends for its life. Each lives until the
        // test kills it.
        let other = scope.spawn(|| {
            let mut children = Children(Vec::new());
            for _ in 0..32 {
                let child = Command::new("sleep")
                    .arg("86400")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap();
                children.0.push(child);
            }
            spawned.store(true, Ordering::Relaxed);
            children
        });
        // Handovers made back to back, so the racers' spawns fall while pairs are being made.
        // Each generation's ends go at once, as a replacement or a parent that dies does. A
        // racer never keeps a copy past its exec, so an end not yet answered is kept and asked
        // again once every racer has started: waiting for it here would leave the spawns that
        // start meanwhile unraced.
        let mut handovers = 0;
        let mut held = Vec::new();
        while !spawned.load(Ordering::Relaxed) {
            let ((ready, commit), ends) = Control::pair().unwrap();
            let Control {
                ready: replacement_ready,
                commit: replacement_commit,
            } = ends;
            drop((replacement_ready, commit));
            for end in [ready, replacement_commit] {
                sys::set_nonblocking(&end, true).unwrap();
                match (&end).read(&mut [0]) {
                    Ok(0) => {}
                    Err(e)
                        if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) =>
                    {
                        held.push(end);
                    }
                    other => panic!("handover {handovers} answered {other:?}"),
                }
            }
            handovers += 1;
        }
        // An end a racer kept would have held its answer until that racer ended.
        let mut children = other.join().unwrap();
        for end in &held {
            assert_eq!(answer(end, || false).unwrap(), Answer::Ended);
        }
        for child in &mut children.0 {
            assert!(child.try_wait().unwrap().is_none(), "a racer ended first");
        }
        assert!(handovers > 0, "no handover was made beside the spawns");
    });

    // A replacement started as replace starts one, its ends kept across exec, that ends before
    // it is ready: its death is the end of its parent's ready pipe.
    let ((ready, _commit), ends) = Control::pair().unwrap();
    let fds = [ends.ready.as_raw_fd(), ends.commit.as_raw_fd()];
    let mut command = sys::Command::new("true");
    for fd in fds {
        command.pass_fd(fd);
    }
    let mut replacement = command.stdin(sys::Stdio::Null).spawn().unwrap();
    drop(ends);
    assert!(replacement.wait().unwrap().success());
    assert_eq!(answer(&ready, || false).unwrap(), Answer::Ended);
}

/// A replacement that failed its hand-off is killed, said first with its pid and why, and reaped;
/// one that ended already is sent nothing and nothing is said.
#[test]
fn a_failed_replacement_is_killed_and_said_unless_it_ended_already() {
    use std::os::unix::process::ExitStatusExt;
    use symphony_board::sys::{Command, Stdio};
    // It lives until killed, or until its input closes: a wait on one never killed ends at once,
    // as `wait` closes that input first.
    let mut running = Command::new("/bin/cat")
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = running.id();
    let why = "replacement did not become ready: it ended";
    let ((), told) = crate::front::told::during(|| super::end_replacement(&mut running, why));
    assert_eq!(
        told,
        [format!(
            "· Symphony reload: SIGKILL to the replacement server, pid {pid}: {why}"
        )]
    );
    assert_eq!(running.wait().unwrap().signal(), Some(libc::SIGKILL));
    let mut ended = Command::new("true").spawn().unwrap();
    test_time::until_true(
        "true to exit",
        test_time::backstop(Duration::from_secs(30)),
        || crate::sys::exited(&ended),
    )
    .unwrap();
    let ((), told) = crate::front::told::during(|| super::end_replacement(&mut ended, why));
    assert!(told.is_empty(), "{told:?}");
    assert!(ended.wait().unwrap().success());
}

#[test]
fn a_replacement_is_ready_once_committed_or_once_its_parent_has_gone() {
    use super::{Answer, Control, answer};
    use std::io::Write;
    use std::os::unix::net::UnixStream;

    let replacement = |control: Control| {
        std::thread::spawn(move || {
            Setup {
                listeners: Vec::new(),
                lease: None,
                control: Some(control),
                fingerprint: None,
                running: None,
            }
            .ready()
        })
    };
    let ((ready, mut commit), ends) = Control::pair().unwrap();
    let ready_thread = replacement(ends);
    assert_eq!(answer(&ready, || false).unwrap(), Answer::Byte(b'R'));
    commit.write_all(b"C").unwrap();
    assert_eq!(ready_thread.join().unwrap(), Ok(()));

    // A parent gone after the replacement said it was ready leaves it serving.
    let ((ready, commit), ends) = Control::pair().unwrap();
    let ready_thread = replacement(ends);
    assert_eq!(answer(&ready, || false).unwrap(), Answer::Byte(b'R'));
    drop(commit);
    assert_eq!(ready_thread.join().unwrap(), Ok(()));

    // Anything but the commit is refused.
    let ((ready, mut commit), ends) = Control::pair().unwrap();
    let ready_thread = replacement(ends);
    assert_eq!(answer(&ready, || false).unwrap(), Answer::Byte(b'R'));
    commit.write_all(b"X").unwrap();
    assert!(ready_thread.join().unwrap().is_err());

    // An older generation's one socket pair carries both.
    let (mut parent, end) = UnixStream::pair().unwrap();
    let ready_thread = replacement(Control::socket(end.into()).unwrap());
    let mut said = [0];
    std::io::Read::read_exact(&mut parent, &mut said).unwrap();
    assert_eq!(said, *b"R");
    parent.write_all(b"C").unwrap();
    assert_eq!(ready_thread.join().unwrap(), Ok(()));

    // Asked to stop first, the parent's wait says so.
    let ((ready, _commit), _ends) = Control::pair().unwrap();
    assert_eq!(answer(&ready, || true).unwrap(), Answer::Stopped);
}

#[test]
fn a_transfer_names_the_handover_pipes_or_an_older_generations_socket() {
    use super::Transfer;
    let ends = |fields: &str| {
        serde_json::from_str::<Transfer>(&format!(
            r#"{{"parent":1,"listeners":[3],"lease":4,{fields}"fingerprint":"f"}}"#
        ))
        .unwrap()
        .ends()
    };
    assert_eq!(ends(r#""ready":5,"commit":6,"#), Ok(vec![5, 6]));
    // A generation from before the pipes reloads into this one with its one socket.
    assert_eq!(ends(r#""control":5,"#), Ok(vec![5]));
    for fields in ["", r#""ready":5,"#, r#""ready":5,"commit":6,"control":7,"#] {
        assert!(ends(fields).is_err(), "{fields}");
    }
    // What this generation sends names no socket: an older replacement refuses it whole.
    let sent = serde_json::to_value(Transfer {
        parent: 1,
        listeners: vec![3],
        lease: 4,
        ready: Some(5),
        commit: Some(6),
        control: None,
        fingerprint: "f".into(),
    })
    .unwrap();
    assert!(sent.get("control").is_none(), "{sent}");
}

#[test]
fn a_handover_is_taken_by_its_one_name_and_only_from_this_process_parent() {
    use super::inherited;
    use std::collections::BTreeMap;
    use std::env::VarError;
    let from = |parent: i64| {
        format!(
            r#"{{"parent":{parent},"listeners":[3],"lease":4,"ready":5,"commit":6,"fingerprint":"f{parent}"}}"#
        )
    };
    let taken = |vars: &[(&str, String)], parent: i64| {
        let vars: BTreeMap<String, String> = vars
            .iter()
            .map(|(name, value)| ((*name).to_string(), value.clone()))
            .collect();
        inherited(
            |name| vars.get(name).cloned().ok_or(VarError::NotPresent),
            parent,
        )
        .map(|transfer| transfer.map(|transfer| transfer.fingerprint))
    };
    let name = "SYMPHONY_HANDOVER";
    assert_eq!(taken(&[(name, from(7))], 7), Ok(Some("f7".into())));
    // Not from this process's parent: a descendant, which takes nothing.
    assert_eq!(taken(&[(name, from(5))], 7), Ok(None));
    assert_eq!(taken(&[], 7), Ok(None));
    assert!(taken(&[(name, "not a transfer".into())], 7).is_err());
    // No other name hands one over.
    assert_eq!(taken(&[("HANDOVER", from(7))], 7), Ok(None));
}

#[test]
fn standalone_source_watch_includes_client_assets_and_its_own_build_scripts() {
    let root = Path::new("/standalone");
    let directories = BTreeSet::from([root.join("apps/symphony-server")]);
    for (path, expected) in [
        ("apps/symphony-web/src/styles/frame.css", true),
        ("apps/symphony-web/node_modules/vite/index.js", false),
        ("apps/symphony-server/client/public/index.html", true),
        ("apps/symphony-server/client/public/assets/current.js", true),
        ("tools/rs-bin.sh", true),
        ("tools/cargo.sh", true),
        ("tools/test-rs.sh", true),
        ("tools/check_lock.sh", true),
        ("build/symphony-ui/public/index.html", false),
        (".target/debug/symphony-server", false),
        ("docs/README.md", false),
    ] {
        assert_eq!(
            source_event(root, &directories, &root.join(path)),
            expected,
            "{path}"
        );
    }
}

/// How often the reload workers of the cases below read the sources with no event.
const PERIOD: Duration = Duration::from_millis(500);

/// A wait that only a stall reaches, for a build and its outcome.
fn stall() -> Duration {
    test_time::backstop(PERIOD + Duration::from_secs(30))
}

/// A main checkout whose project builds its server with a fixture script (its Cargo,
/// `tools/rs-bin.sh`, run from the root with the install's arguments), and the state of a server
/// serving it from the install folder `home`. Its installed image is the plain file
/// `home/bin/symphony-server`, which this test process stands in for as the image it runs.
struct Reloading {
    tree: Tree,
    root: PathBuf,
    home: PathBuf,
    shared: Arc<Shared>,
    hub: Arc<crate::live::Hub>,
}

impl Reloading {
    /// `build(folder, home)` is the build's body: `folder` holds the case's records and stand-ins,
    /// `home` is the install folder.
    fn new(build: impl FnOnce(&Path, &Path) -> String) -> Self {
        let mut tree = Tree::new();
        tree.root = tree.root.canonicalize().unwrap();
        let root = repository(&tree);
        tree.write(
            "main/symphony/apps/symphony-server/Cargo.toml",
            "[package]\nname = \"symphony-server\"\n",
        );
        tree.write(
            "main/symphony/apps/symphony-server/src/main.rs",
            "fn main() {}\n",
        );
        tree.write("main/apps/symphony-host/Cargo.toml", "[package]\nname = 'symphony-host'\n[dependencies]\nsymphony-server = { path = '../../symphony/apps/symphony-server' }\n");
        tree.write(
            "main/apps/symphony-host/src/bin/symphony-server.rs",
            "fn main() {}\n",
        );
        tree.write(
            "main/symphony.toml",
            "[project]\ncargo = [\"tools/rs-bin.sh\"]\n",
        );
        let (folder, home) = (tree.path("case"), tree.path("home"));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::create_dir_all(root.join("tools")).unwrap();
        let script = root.join("tools/rs-bin.sh");
        let body = format!("#!/bin/sh\n{}", build(&folder, &home));
        crate::fixture_script::write(&script, &body, false);
        crate::testing::assert_primed(&script);
        git(&root, &["add", "."]);
        git(&root, &["commit", "-qm", "Reload sources"]);
        tree.write("home/bin/symphony-server", RUNNING);
        let shared = Shared::new(
            i64::from(std::process::id()),
            1,
            &root,
            Some(&home),
            crate::testing::open_gate(),
        );
        let hub = Arc::new(crate::live::Hub::new(&root, &home.join("client/public")));
        assert!(shared.api.set(hub.clone()).is_ok());
        Self {
            tree,
            root,
            home,
            shared,
            hub,
        }
    }

    /// The case's records and stand-ins.
    fn folder(&self) -> PathBuf {
        self.tree.path("case")
    }

    fn image(&self) -> PathBuf {
        self.home.join("bin/symphony-server")
    }

    /// The setup of a start from the installed image, the image it runs: unstamped, as an install
    /// by a recipe leaves it, or `stamped` as built from the sources as they are.
    fn setup(&self, stamped: bool) -> Setup {
        let fingerprint = stamped.then(|| {
            let fingerprint = symphony::reload::fingerprint(&self.root).unwrap();
            stamp_install(&self.home, &fingerprint).unwrap();
            fingerprint
        });
        Setup {
            listeners: Vec::new(),
            lease: Some(std::fs::File::create(self.home.join("server.lock")).unwrap()),
            control: None,
            fingerprint,
            running: RunningImage::at(&self.image()),
        }
    }

    /// The reload worker of a server set up by `setup`, watching by `W`.
    fn watch<W: notify::Watcher + Send + 'static>(
        &self,
        setup: Setup,
    ) -> std::thread::JoinHandle<()> {
        self.retrying::<W>(setup, Retry::new())
    }

    /// [`Self::watch`], trying a failed version again as `retry` says.
    fn retrying<W: notify::Watcher + Send + 'static>(
        &self,
        setup: Setup,
        retry: Retry,
    ) -> std::thread::JoinHandle<()> {
        self.waiting_on::<W>(setup, Backstop::new(PERIOD), retry)
    }

    /// [`Self::retrying`], the worker waiting on `backstop`.
    fn waiting_on<W: notify::Watcher + Send + 'static>(
        &self,
        setup: Setup,
        backstop: Backstop,
        retry: Retry,
    ) -> std::thread::JoinHandle<()> {
        watch_with::<W>(
            self.serve(),
            setup,
            self.shared.clone(),
            Vec::new(),
            backstop,
            retry,
        )
        .unwrap()
    }

    /// The follow of `current` that a release's server runs, on a thread of its own, watching by `W`
    /// and waiting on `backstop`: this server as the release `release-1`, its install folder as its
    /// machine folder.
    fn following<W: notify::Watcher + 'static>(
        &self,
        backstop: Backstop,
        retry: Retry,
    ) -> std::thread::JoinHandle<()> {
        let (serve, shared) = (self.serve(), self.shared.clone());
        let follow = super::Follow {
            machine: self.home.clone(),
            release: "release-1".into(),
            sockets: Vec::new(),
            lease: std::fs::File::open(&self.root).unwrap(),
        };
        std::thread::spawn(move || {
            super::follow::<W>(&serve, &follow, &shared, backstop, retry);
        })
    }

    /// How the case's server was started: from the install folder, serving the checkout.
    fn serve(&self) -> crate::cli::Serve {
        crate::cli::Serve {
            home: Some(self.home.clone()),
            root: self.root.clone(),
            port: 1,
            hosts: Vec::new(),
            install: None,
        }
    }

    /// Each build's arguments, in the order they ran.
    fn builds(&self) -> Vec<String> {
        std::fs::read_to_string(self.folder().join("builds"))
            .map(|text| text.lines().map(String::from).collect())
            .unwrap_or_default()
    }

    /// The install's stamp, once it is written.
    fn stamp(&self) -> Option<serde_json::Value> {
        serde_json::from_slice(&std::fs::read(self.home.join("installed.json")).ok()?).ok()
    }

    /// The server's notice: what it says of a failed update.
    fn notice(&self) -> String {
        self.hub.server_notice.borrow().clone()
    }

    /// The server's notice once it says other than `before`.
    fn notice_after(&self, before: &str) -> String {
        test_time::until("the notice to change", stall(), || {
            Some(self.notice()).filter(|notice| notice != before)
        })
        .unwrap()
    }

    /// The checkout registered in the server's machine folder, so a starter's request to hand over
    /// to a build of it is taken.
    fn register(&self) {
        symphony::machine::Registry {
            projects: vec![symphony::machine::Entry {
                id: "project".into(),
                name: "project".into(),
                root: self.root.clone(),
                manifest: None,
                plugins: Vec::new(),
                pin: 1,
                schema: 1,
            }],
        }
        .write(&self.home)
        .unwrap();
    }

    /// A starter's request to hand over to the case's replacement that ends before it is ready
    /// ([`ends`]), and the flag that tells the reload worker of it, which wakes it.
    fn ask_hand_over(&self) {
        crate::host::Handover {
            exe: self.folder().join("replacement"),
            root: self.root.clone(),
            install: self.home.clone(),
        }
        .ask(&self.home)
        .unwrap();
        // As the starter's signal does: the request wakes the waiting loop.
        self.shared.ask_handover();
    }

    /// The worker stopped and joined, within a stall's bound.
    fn stop(&self, worker: std::thread::JoinHandle<()>) {
        self.shared.stop();
        joined(worker, "the stop to end the worker", stall());
    }
}

/// The bytes of the image the case's server runs.
const RUNNING: &str = "the image this generation runs";

fn sha256(text: &str) -> String {
    Sha256::digest(text)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A build's body that records its arguments and installs the image again as `cargo install
/// --force` installs a build that changed nothing: the same bytes in a new file, of the same size
/// and modification time.
fn reinstalls(folder: &Path, home: &Path) -> String {
    format!(
        "printf '%s\\n' \"$*\" >> '{builds}'\ncp -p '{image}' '{next}' && mv '{next}' '{image}'\n",
        builds = folder.join("builds").display(),
        image = home.join("bin/symphony-server").display(),
        next = home.join("bin/next").display(),
    )
}

/// An unstamped start, as a recipe's own `cargo install` leaves one, whose reload build installs
/// the same bytes again: the install is stamped as that build's, an earlier failed update's notice
/// goes, and this generation keeps serving, handing over to nothing.
#[test]
fn a_build_that_leaves_the_running_image_installed_stamps_it_and_hands_over_to_nothing() {
    let server = Reloading::new(reinstalls);
    let started = std::fs::metadata(server.image()).unwrap();
    let stale =
        "Symphony server update failed; the previous server is still serving. a build before";
    server.hub.tell_update(stale.into());
    let worker = server.watch::<notify::NullWatcher>(server.setup(false));
    let notice = test_time::until("the build's outcome on the notice", stall(), || {
        Some(server.notice()).filter(|notice| notice != stale)
    })
    .unwrap();
    // A replacement started from it would have failed: the image is no program.
    assert_eq!(notice, "", "the build was kept and the old failure cleared");
    let installed = std::fs::metadata(server.image()).unwrap();
    assert_ne!(
        installed.ino(),
        started.ino(),
        "the same bytes as a new file"
    );
    let stamp = server.stamp().expect("the install is stamped");
    assert_eq!(
        stamp["source"],
        symphony::reload::fingerprint(&server.root).unwrap()
    );
    assert_eq!(stamp["binary"], sha256(RUNNING));
    // The file the build installed, so the next start takes the stamp's hash unread.
    assert_eq!(stamp["image"]["inode"], installed.ino());
    assert!(!server.shared.stopping(), "still serving");
    assert!(!server.shared.replaced.load(Ordering::SeqCst));
    let builds = server.builds();
    assert_eq!(builds.len(), 1, "{builds:?}");
    assert!(
        builds[0].ends_with(&format!("--root {}", server.home.display())),
        "{builds:?}"
    );
    server.stop(worker);
}

/// A build that installs other bytes hands over to them as before: a replacement started from the
/// install says it is ready on the hand-off's pipe and is committed, the install is stamped as the
/// build's, and this generation drains.
#[test]
fn a_build_that_installs_another_image_hands_over_to_it() {
    let server = Reloading::new(|folder, home| {
        // The other image: a replacement that says it is ready and records the commit it reads,
        // with its arguments.
        let replacement = folder.join("replacement");
        crate::fixture_script::write(
            &replacement,
            &format!(
                r#"#!/bin/sh
ready=$(printf '%s' "$SYMPHONY_HANDOVER" | sed 's/.*"ready":\([0-9]*\).*/\1/')
commit=$(printf '%s' "$SYMPHONY_HANDOVER" | sed 's/.*"commit":\([0-9]*\).*/\1/')
printf R >&"$ready"
said=$(dd bs=1 count=1 2>/dev/null <&"$commit")
printf '%s %s\n' "$said" "$*" > '{next}' && mv '{next}' '{replaced}'
"#,
                next = folder.join("replaced.next").display(),
                replaced = folder.join("replaced").display(),
            ),
            false,
        );
        crate::testing::assert_primed(&replacement);
        format!(
            "printf '%s\\n' \"$*\" >> '{builds}'\nln -s '{replacement}' '{next}' && mv '{next}' '{image}'\n",
            builds = folder.join("builds").display(),
            replacement = replacement.display(),
            next = home.join("bin/next").display(),
            image = home.join("bin/symphony-server").display(),
        )
    });
    let worker = server.watch::<notify::NullWatcher>(server.setup(false));
    let replaced = server.folder().join("replaced");
    test_time::until("the hand-off to the other image", stall(), || {
        let notice = server.notice();
        if !notice.is_empty() {
            return Some(Err(notice));
        }
        (server.shared.stopping() && replaced.is_file()).then_some(Ok(()))
    })
    .unwrap()
    .unwrap();
    joined(worker, "the hand-off to end the worker", stall());
    assert!(server.shared.replaced.load(Ordering::SeqCst));
    assert_eq!(
        std::fs::read_to_string(&replaced).unwrap(),
        format!(
            "C serve --home {home} --root {root} --port 1 --install {home}\n",
            home = server.home.display(),
            root = server.root.display()
        )
    );
    let stamp = server.stamp().expect("the install is stamped");
    assert_eq!(
        stamp["source"],
        symphony::reload::fingerprint(&server.root).unwrap()
    );
    assert_ne!(stamp["binary"], sha256(RUNNING));
    assert_eq!(server.builds().len(), 1);
}

/// A build's body that records its arguments and installs another image: a stand-in replacement
/// that records its start and ends before it says it is ready, so every hand-over to it fails.
fn installs_a_replacement_that_ends(folder: &Path, home: &Path) -> String {
    in_turn(folder, &[ends(folder, home)])
}

/// A build's body that records its arguments, then runs the step of its turn among `steps`: the
/// first build the first, and every build from the last step's turn on the last.
fn in_turn(folder: &Path, steps: &[String]) -> String {
    let builds = folder.join("builds");
    let mut body = format!(
        "printf '%s\\n' \"$*\" >> '{builds}'\ncase $(($(wc -l < '{builds}'))) in\n",
        builds = builds.display()
    );
    for (turn, step) in steps.iter().enumerate() {
        let turn = if turn + 1 == steps.len() {
            "*".to_string()
        } else {
            (turn + 1).to_string()
        };
        body.push_str(&format!("{turn}) {step} ;;\n"));
    }
    body + "esac\n"
}

/// A build's step that installs a stand-in replacement, `folder/replacement`, that records its
/// start and ends before it says it is ready, so every hand-over to it fails.
fn ends(folder: &Path, home: &Path) -> String {
    let replacement = folder.join("replacement");
    crate::fixture_script::write(
        &replacement,
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{started}'\n",
            started = folder.join("started").display()
        ),
        false,
    );
    crate::testing::assert_primed(&replacement);
    format!(
        "ln -s '{replacement}' '{next}' && mv '{next}' '{image}'",
        replacement = replacement.display(),
        next = home.join("bin/next").display(),
        image = home.join("bin/symphony-server").display(),
    )
}

/// A build's step that installs the running image's bytes again, from a copy of them in `folder`,
/// as a new file: a build that is kept, whatever image the builds before it installed.
fn installs_running(folder: &Path, home: &Path) -> String {
    let running = folder.join("running");
    std::fs::write(&running, RUNNING).unwrap();
    format!(
        "cp '{running}' '{next}' && mv '{next}' '{image}'",
        running = running.display(),
        next = home.join("bin/next").display(),
        image = home.join("bin/symphony-server").display(),
    )
}

/// A build's step that leaves the checkout on another branch, where its sources are not read: the
/// build is dropped.
const LEAVES_MAIN: &str = "git symbolic-ref HEAD refs/heads/aside";

/// The reload's clock in a test's hands: it stands where the test sets it, starts at a whole
/// minute of wall time, so a notice names the minute each wait ends, and counts its reads.
struct Clock {
    start: (Instant, SystemTime),
    now: Arc<Mutex<(Instant, SystemTime)>>,
    reads: Arc<AtomicUsize>,
}

impl Clock {
    fn new() -> Self {
        let start = (
            Instant::now(),
            UNIX_EPOCH + Duration::from_secs(1_800_000_000),
        );
        Self {
            start,
            now: Arc::new(Mutex::new(start)),
            reads: Arc::default(),
        }
    }

    /// A retry whose waits run by this clock.
    fn retry(&self) -> Retry {
        let (now, reads) = (self.now.clone(), self.reads.clone());
        Retry::timed_by(move || {
            reads.fetch_add(1, Ordering::SeqCst);
            *now.lock().unwrap()
        })
    }

    /// The clock set to `by` past its start.
    fn set(&self, by: Duration) {
        *self.now.lock().unwrap() = (self.start.0 + by, self.start.1 + by);
    }

    /// The clock [set](Self::set) to `by` past its start, and two of the reload's reads of it
    /// after that: the check that read it first has ended, whatever it did. The reload reads it
    /// at each check of a version whose failure waits, so not once a try ends in none.
    fn at(&self, by: Duration) {
        self.set(by);
        let read = self.reads.load(Ordering::SeqCst);
        test_time::until_true("two checks at the time set", stall(), || {
            self.reads.load(Ordering::SeqCst) >= read + 2
        })
        .unwrap();
    }

    /// The minute `by` past the clock's start, as a notice names it.
    fn minute(&self, by: Duration) -> String {
        let seconds = (self.start.1 + by)
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        symphony::clock::stamp_at(libc::time_t::try_from(seconds).unwrap())
    }
}

const MINUTE: Duration = Duration::from_secs(60);

/// What the notice says of every failed update before its reason.
const FAILED: &str = "Symphony server update failed; the previous server is still serving.";

/// How many replacements the case's builds installed have started.
fn started(server: &Reloading) -> usize {
    std::fs::read_to_string(server.folder().join("started"))
        .map(|text| text.lines().count())
        .unwrap_or_default()
}

/// A version whose hand-over failed is built and handed over again once the wait after its failure
/// is over on the reload's clock, a test's here, and not before; its second failure waits longer.
/// The notice says each time when it is tried again. A source change is built at once, whatever
/// that wait.
#[test]
fn a_version_whose_hand_over_failed_is_tried_again_after_a_wait_that_grows() {
    let server = Reloading::new(installs_a_replacement_that_ends);
    let clock = Clock::new();
    // The `tries`th build and hand-over have failed, and the notice says the next is due `again`
    // past the start.
    let failed = |tries: usize, again: Duration| {
        let said = format!(
            "{FAILED} replacement did not become ready: it ended. It tries this version again at \
             about {}.",
            clock.minute(again)
        );
        test_time::until(format!("failure {tries} on the notice"), stall(), || {
            (server.notice() == said).then(|| (server.builds().len(), started(&server)))
        })
        .unwrap()
    };
    let worker = server.retrying::<notify::NullWatcher>(server.setup(false), clock.retry());
    // The start's first read builds the sources, an unstamped install's, and hands over: that fails.
    assert_eq!(failed(1, 5 * MINUTE), (1, 1));
    clock.at(5 * MINUTE - Duration::from_secs(1));
    assert_eq!(
        (server.builds().len(), started(&server)),
        (1, 1),
        "not before 5 min"
    );
    clock.at(5 * MINUTE);
    assert_eq!(failed(2, 15 * MINUTE), (2, 2), "tried again at 5 min");
    // Nine minutes after the second failure, where a wait as long as the first's would be over.
    clock.at(14 * MINUTE);
    assert_eq!(
        (server.builds().len(), started(&server)),
        (2, 2),
        "10 min this time"
    );
    let before = symphony::reload::fingerprint(&server.root).unwrap();
    server.tree.write(
        "main/symphony/apps/symphony-server/src/main.rs",
        "fn main() { /* changed */ }\n",
    );
    assert_ne!(symphony::reload::fingerprint(&server.root).unwrap(), before);
    // The clock stands where the failed version's wait is not over, and the new version's first
    // failure waits the first wait again.
    assert_eq!(failed(3, 19 * MINUTE), (3, 3), "a source change at once");
    server.stop(worker);
    let builds = server.builds();
    assert!(builds.iter().all(|build| build == &builds[0]), "{builds:?}");
}

/// A build that fails leaves the notice its reason and when that version is built again, the
/// reason ending where the next sentence starts: a build that said nothing leaves no separator
/// there, and a diagnostic that ends a sentence no second full stop.
#[test]
fn a_failed_builds_notice_ends_its_reason_before_saying_when_it_is_tried_again() {
    // The first build says nothing; each later one says a diagnostic that ends a sentence.
    let server = Reloading::new(|folder, _| {
        format!(
            "printf '%s\\n' \"$*\" >> '{builds}'\nif [ -e '{said}' ]; then echo 'error: no such \
             crate.' >&2; else : > '{said}'; fi\nexit 1\n",
            builds = folder.join("builds").display(),
            said = folder.join("said").display(),
        )
    });
    let clock = Clock::new();
    let worker = server.retrying::<notify::NullWatcher>(server.setup(false), clock.retry());
    let first = server.notice_after("");
    assert_eq!(
        first,
        format!(
            "{FAILED} symphony-server build exit status: 1. It tries this version again at about \
             {}.",
            clock.minute(5 * MINUTE)
        )
    );
    clock.at(5 * MINUTE);
    assert_eq!(
        server.notice_after(&first),
        format!(
            "{FAILED} symphony-server build exit status: 1: error: no such crate. It tries this \
             version again at about {}.",
            clock.minute(15 * MINUTE)
        )
    );
    assert_eq!(server.builds().len(), 2);
    assert!(!server.shared.stopping(), "still serving");
    server.stop(worker);
}

/// A build whose sources cannot be read once it has ended is dropped, as one of sources that
/// changed under it is; when they read as that version again, it is built again once the wait
/// after a failure is over on the reload's clock, and not before.
#[test]
fn a_build_dropped_for_sources_unread_after_it_is_tried_again_after_the_wait() {
    // The first build leaves the checkout on another branch, where its sources are not read; every
    // build installs the running image's bytes again, so one that is not dropped is kept.
    let server = Reloading::new(|folder, home| {
        format!(
            "{}[ -e '{aside}' ] || {{ git symbolic-ref HEAD refs/heads/aside && : > '{aside}'; }}\n",
            reinstalls(folder, home),
            aside = folder.join("aside").display(),
        )
    });
    let version = symphony::reload::fingerprint(&server.root).unwrap();
    // A watched folder of no crate: the worker lets it go at the first check after it went.
    let marker = server.root.join("crates");
    std::fs::create_dir(&marker).unwrap();
    let clock = Clock::new();
    let worker = server.retrying::<Seeing>(server.setup(false), clock.retry());
    test_time::until_true("the first build, off main", stall(), || {
        server.folder().join("aside").is_file()
    })
    .unwrap();
    std::fs::remove_dir(&marker).unwrap();
    test_time::until_true("a check after the first build", stall(), || {
        seen(false, &marker)
    })
    .unwrap();
    assert_eq!(server.stamp(), None, "the build's result was dropped");
    assert!(symphony::reload::fingerprint(&server.root).is_err());
    git(&server.root, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    assert_eq!(
        symphony::reload::fingerprint(&server.root).unwrap(),
        version
    );
    clock.at(5 * MINUTE - Duration::from_secs(1));
    assert_eq!(server.builds().len(), 1, "not before 5 min");
    // Its try is kept, a failure no more, so the clock is read no further.
    clock.set(5 * MINUTE);
    let stamp = test_time::until("the version's second build, kept", stall(), || {
        server.stamp()
    })
    .unwrap();
    assert_eq!(stamp["source"], version);
    assert_eq!(server.builds().len(), 2);
    assert_eq!(server.notice(), "", "a dropped build is no failed update");
    server.stop(worker);
}

/// A hand-over a starter asked for that fails says why on the notice, and names no retry: the
/// starter asks again. One that fails while a retry of the sources' own failed update waits keeps
/// that retry's time on the notice, and the retry still comes when it said.
#[test]
fn a_starters_failed_hand_over_keeps_a_waiting_retrys_time_on_the_notice() {
    let server = Reloading::new(installs_a_replacement_that_ends);
    server.register();
    let clock = Clock::new();
    // A stamped start builds nothing, so no retry waits when the starter first asks.
    let worker = server.retrying::<notify::NullWatcher>(server.setup(true), clock.retry());
    server.ask_hand_over();
    let asked = format!("{FAILED} replacement did not become ready: it ended");
    assert_eq!(server.notice_after(""), asked);
    assert_eq!((server.builds().len(), started(&server)), (0, 1));

    server.tree.write(
        "main/symphony/apps/symphony-server/src/main.rs",
        "fn main() { /* changed */ }\n",
    );
    let failed = format!(
        "{asked}. It tries this version again at about {}.",
        clock.minute(5 * MINUTE)
    );
    assert_eq!(server.notice_after(&asked), failed);
    server.ask_hand_over();
    let kept = format!(
        "{asked}. The version whose update failed before is tried again at about {}.",
        clock.minute(5 * MINUTE)
    );
    assert_eq!(server.notice_after(&failed), kept);
    assert_eq!((server.builds().len(), started(&server)), (1, 3));
    // The retry comes at the time both notices named.
    clock.at(5 * MINUTE);
    assert_eq!(
        server.notice_after(&kept),
        format!(
            "{asked}. It tries this version again at about {}.",
            clock.minute(15 * MINUTE)
        )
    );
    assert_eq!((server.builds().len(), started(&server)), (2, 4));
    server.stop(worker);
}

/// The loop's look for a hand-off replaces nothing and says no failed update where none was asked
/// for, where the server has no machine folder to take a request from, where none was left there,
/// and where the one left is refused; the flag is taken each time it was set, so the next look
/// does not take the same request again.
#[test]
fn a_hand_off_look_with_no_request_to_follow_replaces_nothing() {
    use super::{HandOff, handed_over};
    let server = Reloading::new(installs_a_replacement_that_ends);
    let lease = std::fs::File::open(&server.root).unwrap();
    let look = |home: Option<&Path>| {
        let serve = crate::cli::Serve {
            home: home.map(Path::to_path_buf),
            root: server.root.clone(),
            port: 1,
            hosts: Vec::new(),
            install: None,
        };
        let came = handed_over(&serve, &server.shared, &[], &lease, &Retry::new());
        assert!(
            !server.shared.handover.load(Ordering::SeqCst),
            "the flag is taken"
        );
        came
    };
    assert_eq!(look(Some(&server.home)), HandOff::None, "none asked for");
    server.shared.ask_handover();
    assert_eq!(look(None), HandOff::None, "no machine folder");
    server.shared.ask_handover();
    assert_eq!(look(Some(&server.home)), HandOff::None, "no request left");
    // A request for a checkout the registry does not hold: refused, said in the log alone.
    server.ask_hand_over();
    assert_eq!(look(Some(&server.home)), HandOff::None, "a refused request");
    assert!(
        !server.home.join(crate::host::HANDOVER).exists(),
        "a refused request is taken"
    );
    assert_eq!(server.notice(), "", "no update failed");
    assert_eq!(started(&server), 0, "no replacement started");
}

/// The reload loop on a period no test reaches ([`backstop::NO_PERIOD`]), from a stamped start,
/// which builds nothing: once it waits, past its first read of the sources, the server's stop wakes
/// it and it ends.
#[test]
fn the_servers_stop_wakes_the_waiting_reload_loop_and_ends_it() {
    let server = Reloading::new(reinstalls);
    let (backstop, waits) = backstop::watched();
    let worker =
        server.waiting_on::<notify::NullWatcher>(server.setup(true), backstop, Retry::new());
    waits.begun(1);
    server.stop(worker);
    assert_eq!(server.builds(), Vec::<String>::new());
    assert_eq!(server.notice(), "");
}

/// The reload loop on a period no test reaches, from a stamped start: a starter's hand-off asked
/// while it waits wakes it, and it takes the request then, starting the replacement it names, whose
/// failure the notice says. It waits again after that, until its stop.
#[test]
fn a_hand_off_asked_while_the_reload_loop_waits_wakes_it_to_take_the_request() {
    let server = Reloading::new(installs_a_replacement_that_ends);
    server.register();
    let (backstop, waits) = backstop::watched();
    let worker =
        server.waiting_on::<notify::NullWatcher>(server.setup(true), backstop, Retry::new());
    waits.begun(1);
    server.ask_hand_over();
    assert_eq!(
        server.notice_after(""),
        format!("{FAILED} replacement did not become ready: it ended")
    );
    assert_eq!((server.builds().len(), started(&server)), (0, 1));
    waits.begun(2);
    server.stop(worker);
}

/// A release's server's follow of `current`, on a period no test reaches, the link naming no
/// release: a starter's hand-off asked while it waits wakes it, and it takes the request and
/// refuses it, starting nothing. It waits again after that, and its stop wakes it and ends it.
#[test]
fn a_release_servers_follow_takes_a_hand_off_and_its_stop_at_their_wakes() {
    let server = Reloading::new(installs_a_replacement_that_ends);
    server.register();
    let (backstop, waits) = backstop::watched();
    let follow = server.following::<notify::NullWatcher>(backstop, Retry::new());
    waits.begun(1);
    server.ask_hand_over();
    let request = server.home.join(crate::host::HANDOVER);
    test_time::until_true("the follow to take the hand-off request", stall(), || {
        !request.exists()
    })
    .unwrap();
    assert!(
        !server.shared.handover.load(Ordering::SeqCst),
        "the flag is taken"
    );
    waits.begun(2);
    assert_eq!(started(&server), 0, "no start replaces a release's server");
    assert_eq!(server.notice(), "", "no update failed");
    server.stop(follow);
}

/// A release's server's follow of `current`, on a period no test reaches, hears the link move from
/// its own watcher of the machine folder, made before its first read and kept while it follows,
/// and reads it then: a release not installed whole leaves it serving, said on the notice with when
/// it is tried again. The watcher goes with the follow at its stop. The test tells the watcher's
/// events itself: under load a real backend can lose one outright, which the follow's period is
/// for (`backstop.rs`), and on no period a lost event would leave the follow waiting for good.
#[test]
fn a_release_servers_follow_reads_current_when_its_watcher_hears_the_link_move() {
    use notify::EventKind::{Create, Modify};
    use notify::event::{CreateKind, ModifyKind, RenameMode};
    use symphony::machine::release;
    let server = Reloading::new(installs_a_replacement_that_ends);
    let clock = Clock::new();
    let (backstop, waits) = backstop::watched();
    let follow = server.following::<Handing<false>>(backstop, clock.retry());
    waits.begun(1);
    // Past its first read, which found no link, it waits with the machine folder watched.
    assert_eq!(
        watching(&server.home),
        Some(notify::RecursiveMode::NonRecursive)
    );
    // The move `release::point` makes, to a release it would refuse, and what a backend says of
    // it: the new link made beside `current`, then renamed over it.
    point(&server.home, "release-2");
    let (made, current) = (
        server.home.join(".current.test").display().to_string(),
        server.home.join(release::CURRENT).display().to_string(),
    );
    let over = Modify(ModifyKind::Name(RenameMode::Both));
    tell_at(&server.home, event(Create(CreateKind::Any), [&made]));
    tell_at(&server.home, event(over, [&made, &current]));
    assert_eq!(
        server.notice_after(""),
        format!(
            "{FAILED} The release release-2 did not take over: {} holds no manifest. It is tried \
             again at about {}.",
            release::folder(&server.home, "release-2").display(),
            clock.minute(5 * MINUTE)
        )
    );
    assert_eq!(started(&server), 0);
    assert_eq!(
        watching(&server.home),
        Some(notify::RecursiveMode::NonRecursive),
        "still watched after the hand-off it failed"
    );
    server.stop(follow);
    assert_eq!(
        watching(&server.home),
        None,
        "the watcher goes with the follow"
    );
}

/// A build kept as the running image after another version's update failed leaves no retry of
/// that version waiting, as the sources read the kept one: a starter's hand-over that fails after
/// it names none.
#[test]
fn a_kept_build_leaves_no_retry_for_a_failed_hand_over_to_name() {
    let server = Reloading::new(|folder, home| {
        in_turn(
            folder,
            &[ends(folder, home), installs_running(folder, home)],
        )
    });
    server.register();
    let clock = Clock::new();
    let worker = server.retrying::<notify::NullWatcher>(server.setup(false), clock.retry());
    let asked = format!("{FAILED} replacement did not become ready: it ended");
    let failed = format!(
        "{asked}. It tries this version again at about {}.",
        clock.minute(5 * MINUTE)
    );
    assert_eq!(server.notice_after(""), failed);
    server.tree.write(
        "main/symphony/apps/symphony-server/src/main.rs",
        "fn main() { /* changed */ }\n",
    );
    let changed = symphony::reload::fingerprint(&server.root).unwrap();
    assert_eq!(
        server.notice_after(&failed),
        "",
        "the changed sources' build, kept"
    );
    assert_eq!(
        server.stamp().expect("a kept build's stamp")["source"],
        changed
    );
    server.ask_hand_over();
    assert_eq!(server.notice_after(""), asked, "no retry named");
    assert_eq!((server.builds().len(), started(&server)), (2, 2));
    server.stop(worker);
}

/// A retry whose own build is dropped, for sources that cannot be read once it has ended, says so
/// on the notice that named its time, with the time it is tried again: the notice keeps no time
/// already past. That time is the one to come: once the sources read as that version again, it is
/// built then and not before. Once a build is kept the notice says no failure, and a build dropped
/// after it is said in the log alone.
#[test]
fn a_retrys_dropped_build_names_on_the_notice_when_it_is_tried_again() {
    let server = Reloading::new(|folder, home| {
        in_turn(
            folder,
            &[
                ends(folder, home),
                LEAVES_MAIN.into(),
                installs_running(folder, home),
                LEAVES_MAIN.into(),
            ],
        )
    });
    let version = symphony::reload::fingerprint(&server.root).unwrap();
    // A watched folder of no crate: the worker lets it go at the first check after it went.
    let marker = server.root.join("crates");
    std::fs::create_dir(&marker).unwrap();
    let clock = Clock::new();
    let worker = server.retrying::<Seeing>(server.setup(false), clock.retry());
    let failed = format!(
        "{FAILED} replacement did not become ready: it ended. It tries this version again at \
         about {}.",
        clock.minute(5 * MINUTE)
    );
    assert_eq!(server.notice_after(""), failed);
    // The retry, whose build leaves the checkout off main: the read after it fails, and the clock
    // is read again for that failure's wait.
    clock.at(5 * MINUTE);
    std::fs::remove_dir(&marker).unwrap();
    test_time::until_true("a check after the retry's build", stall(), || {
        seen(false, &marker)
    })
    .unwrap();
    assert_eq!(server.builds().len(), 2);
    let dropped = format!(
        "{FAILED} The sources changed or could not be read during the build; its result is \
         dropped, and that version is tried again at about {} if they read as it again.",
        clock.minute(15 * MINUTE)
    );
    assert_eq!(server.notice(), dropped);

    git(&server.root, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    assert_eq!(
        symphony::reload::fingerprint(&server.root).unwrap(),
        version
    );
    clock.at(15 * MINUTE - Duration::from_secs(1));
    assert_eq!(server.builds().len(), 2, "not before the time named");
    clock.set(15 * MINUTE);
    assert_eq!(server.notice_after(&dropped), "", "the retry then, kept");
    assert_eq!(
        server.stamp().expect("a kept build's stamp")["source"],
        version
    );
    assert_eq!((server.builds().len(), started(&server)), (3, 1));

    // A source change, whose build leaves the checkout off main again. With nothing waiting since
    // the kept build, the clock is read for that drop's wait first: then the sources are put back.
    let read = clock.reads.load(Ordering::SeqCst);
    server.tree.write(
        "main/symphony/apps/symphony-server/src/main.rs",
        "fn main() { /* changed */ }\n",
    );
    test_time::until_true("the changed sources' build, dropped", stall(), || {
        clock.reads.load(Ordering::SeqCst) > read
    })
    .unwrap();
    git(&server.root, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    clock.at(15 * MINUTE);
    assert_eq!(server.builds().len(), 4);
    assert_eq!(server.notice(), "", "a drop with no failure on the notice");
    server.stop(worker);
}

/// A watcher that delivers nothing and keeps each folder it is asked to watch (`true`) or no
/// longer to watch (`false`). The reload worker lets go of a folder that went only at a period's
/// check, which comes after its first read of the sources and any build that read started: a test
/// sees that read done by the folder it removed being let go.
struct Seeing;

static SEEN: Mutex<Vec<(bool, PathBuf)>> = Mutex::new(Vec::new());

fn seen(watched: bool, path: &Path) -> bool {
    SEEN.lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .any(|seen| *seen == (watched, path.to_path_buf()))
}

fn see(watched: bool, path: &Path) -> notify::Result<()> {
    SEEN.lock()
        .unwrap_or_else(|e| e.into_inner())
        .push((watched, path.into()));
    Ok(())
}

impl notify::Watcher for Seeing {
    fn new<F: notify::EventHandler>(_: F, _: notify::Config) -> notify::Result<Self> {
        Ok(Self)
    }

    fn watch(&mut self, path: &Path, _: notify::RecursiveMode) -> notify::Result<()> {
        see(true, path)
    }

    fn unwatch(&mut self, path: &Path) -> notify::Result<()> {
        see(false, path)
    }

    fn kind() -> notify::WatcherKind {
        notify::WatcherKind::NullWatcher
    }
}

/// A start from a stamped install builds nothing until the sources change: past its first read
/// and a period's check, no build has run. Once a source changes its one build runs, and as it
/// leaves the image installed it is kept, stamped as the changed sources'.
#[test]
fn a_stamped_start_builds_nothing_until_the_sources_change() {
    let server = Reloading::new(reinstalls);
    // A watched folder of no crate, so neither it nor its going changes the sources' fingerprint.
    let marker = server.root.join("crates");
    std::fs::create_dir(&marker).unwrap();
    let worker = server.watch::<Seeing>(server.setup(true));
    // Watched in the start's first look at the sources, before its first read: once it is,
    // removing it is seen only at a later check.
    test_time::until_true("the start's first look at the sources", stall(), || {
        seen(true, &marker)
    })
    .unwrap();
    std::fs::remove_dir(&marker).unwrap();
    test_time::until_true("a period's check after the first read", stall(), || {
        seen(false, &marker)
    })
    .unwrap();
    assert_eq!(server.builds(), Vec::<String>::new());

    server.tree.write(
        "main/symphony/apps/symphony-server/src/main.rs",
        "fn main() { /* changed */ }\n",
    );
    let changed = symphony::reload::fingerprint(&server.root).unwrap();
    let stamp = test_time::until("the changed sources' build", stall(), || {
        let notice = server.notice();
        if !notice.is_empty() {
            return Some(Err(notice));
        }
        server
            .stamp()
            .filter(|stamp| stamp["source"] == changed)
            .map(Ok)
    })
    .unwrap()
    .unwrap();
    assert_eq!(stamp["binary"], sha256(RUNNING));
    assert_eq!(server.builds().len(), 1, "{:?}", server.builds());
    assert!(!server.shared.replaced.load(Ordering::SeqCst));
    server.stop(worker);
}

/// A kept build says so in one line; a build that installed other bytes is not kept and says
/// nothing here, and neither is a build of a generation that runs no installed image.
#[test]
fn a_kept_build_says_so_in_one_line() {
    let server = Reloading::new(|_, _| String::new());
    let mut running = RunningImage::at(&server.image()).unwrap();
    running.hash(&server.home);
    let kept = || super::kept(Some(&running), &server.home, "source", &server.shared);
    let (was, told) = told::during(kept);
    assert!(was);
    assert_eq!(
        told,
        ["· Symphony reload: the build left the running image as it was; stamped"]
    );
    assert_eq!(server.stamp().unwrap()["source"], "source");
    std::fs::write(server.image(), "another image").unwrap();
    let (was, told) = told::during(kept);
    assert!(!was);
    assert!(told.is_empty(), "{told:?}");
    let (was, told) = told::during(|| super::kept(None, &server.home, "source", &server.shared));
    assert!(!was);
    assert!(told.is_empty(), "{told:?}");
}

/// The release a reload hands over to is its own folder's server, and only one installed whole:
/// a folder without its manifest, or whose server does not run, is said why.
#[test]
fn a_reload_hands_over_only_to_a_release_installed_whole_by_its_own_folder() {
    use symphony::machine::release;
    let tree = Tree::new();
    let machine = tree.path("machine");
    let why = super::to_release(&machine, "release-1").unwrap_err();
    assert_eq!(
        why,
        format!(
            "{} holds no manifest",
            release::folder(&machine, "release-1").display()
        )
    );
    // Never run here: plain files of their own, which the test may change.
    let build = tree.path("build");
    for binary in release::BINARIES {
        let path = tree.write(&format!("build/{binary}"), "#!/bin/sh\n");
        std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
    }
    tree.write("client/index.html", "<p>");
    let version = release::Version { major: 0, minor: 3 };
    release::install(
        &machine,
        &release::Built {
            release: "release-1",
            commit: "abc",
            serves: release::Serves {
                schema: symphony_board::SCHEMA,
                additions: symphony_board::schema::ADDITIONS,
                protocol: version,
                tasks_api: version,
                pin: symphony::machine::PIN,
            },
            binaries: &build,
            client: &tree.path("client"),
        },
    )
    .unwrap();
    let server = release::folder(&machine, "release-1").join("bin/symphony-server");
    assert_eq!(super::to_release(&machine, "release-1"), Ok(server.clone()));
    std::fs::set_permissions(&server, std::os::unix::fs::PermissionsExt::from_mode(0o644)).unwrap();
    assert_eq!(
        super::to_release(&machine, "release-1"),
        Err(format!("{} does not run", server.display()))
    );
}

#[test]
fn current_moves_at_a_change_naming_its_link_a_rescan_or_the_watchers_error() {
    use notify::EventKind::{Access, Create, Modify, Remove};
    use notify::event::{AccessKind, CreateKind, ModifyKind, RemoveKind, RenameMode};
    let over = Modify(ModifyKind::Name(RenameMode::Both));
    // `point`'s new link renamed over the old, as each backend says it.
    assert!(super::moves_current(&event(
        over,
        ["/m/.current.7", "/m/current"]
    )));
    assert!(super::moves_current(&event(
        Create(CreateKind::Any),
        ["/m/current"]
    )));
    assert!(super::moves_current(&event(
        Remove(RemoveKind::Any),
        ["/m/current"]
    )));
    // The new link made beside it, the registry written, a release installed, or the link read.
    assert!(!super::moves_current(&event(
        Create(CreateKind::Any),
        ["/m/.current.7"]
    )));
    assert!(!super::moves_current(&event(over, ["/m/projects.json"])));
    assert!(!super::moves_current(&event(
        Create(CreateKind::Folder),
        ["/m/releases/current-2"]
    )));
    assert!(!super::moves_current(&event(
        Access(AccessKind::Any),
        ["/m/current"]
    )));
    // What a lost or failing stream says: read it again.
    assert!(super::moves_current(&Ok(notify::Event::new(Modify(
        ModifyKind::Any
    ))
    .set_flag(notify::event::Flag::Rescan))));
    assert!(super::moves_current(&Err(notify::Error::generic(
        "stream ended"
    ))));
}

#[test]
fn a_release_server_hears_current_move_from_a_watch_of_the_machine_folder_alone() {
    // A folder no other test watches (`testing::handing`).
    let machine = Path::new("/current-watch");
    let wakes = crate::backstop::Wakes::new();
    let watcher = super::watch_current::<Handing<false>>(machine, wakes.waker())
        .expect("the machine folder watched");
    assert_eq!(
        watcher.watched,
        vec![(machine.to_path_buf(), notify::RecursiveMode::NonRecursive)]
    );
    watcher.tell(event(
        notify::EventKind::Create(notify::event::CreateKind::Any),
        [machine.join("registry.json")],
    ));
    assert!(!wakes.wait(Some(Duration::ZERO)), "another entry's event");
    // A move's burst is one wake: the loop reads once it is quiet.
    watcher.tell(renamed(machine.join("current")));
    watcher.tell(renamed(machine.join("current")));
    assert!(
        wakes.wait(Some(Duration::ZERO)),
        "the move wakes the follow"
    );
    assert!(!wakes.wait(Some(Duration::ZERO)), "its burst is one wake");
}

#[test]
fn a_release_server_that_cannot_watch_its_machine_folder_reads_current_once_a_period() {
    let wakes = crate::backstop::Wakes::new();
    let watcher = super::watch_current::<Handing<true>>(Path::new("/m"), wakes.waker());
    assert!(watcher.is_none());
    // The handler went with the watcher: nothing wakes the follow but its stop, a hand-off and
    // the backstop's period.
    assert!(
        !wakes.wait(Some(Duration::ZERO)),
        "a watcher that cannot watch wakes nothing"
    );
}

/// A watcher whose backend panics the first [`DEATHS`] times one is made, before the follow that
/// makes it has waited for anything, as a bug at a follow's start would, and watches nothing after.
struct Dying;

/// How many times a [`Dying`] watcher panics.
const DEATHS: usize = 3;

/// How many [`Dying`] watchers were made: one test makes them.
static MADE: AtomicUsize = AtomicUsize::new(0);

impl notify::Watcher for Dying {
    fn new<F: notify::EventHandler>(_: F, _: notify::Config) -> notify::Result<Self> {
        assert!(
            MADE.fetch_add(1, Ordering::SeqCst) >= DEATHS,
            "the watcher's backend is gone"
        );
        Ok(Self)
    }

    fn watch(&mut self, _: &Path, _: notify::RecursiveMode) -> notify::Result<()> {
        Ok(())
    }

    fn unwatch(&mut self, _: &Path) -> notify::Result<()> {
        Ok(())
    }

    fn kind() -> notify::WatcherKind {
        notify::WatcherKind::NullWatcher
    }
}

/// `machine`'s `current` named at the release `name`, as `release::point` moves it.
fn point(machine: &Path, name: &str) {
    use symphony::machine::release;
    let link = machine.join(".current.test");
    std::os::unix::fs::symlink(Path::new(release::RELEASES).join(name), &link).unwrap();
    std::fs::rename(&link, machine.join(release::CURRENT)).unwrap();
}

/// A release's server whose follow of `current` dies, as one that panics does, says so on its
/// notice, and a follow is started in its place once the supervisor has waited for a wake: one
/// that dies again at its start, before any wait of its own, is started once a wake and no more,
/// never in a spin. The follow that lives reads `current` at once, tries the release it names and
/// says why that one did not take over, and the link named back at this release clears the notice.
#[test]
fn a_release_servers_follow_that_dies_is_said_on_the_notice_and_started_again_once_a_wait() {
    use symphony::machine::release;
    let server = Reloading::new(installs_a_replacement_that_ends);
    let clock = Clock::new();
    let (backstop, waits) = backstop::watched();
    point(&server.home, "release-2");
    let follow = server.following::<Dying>(backstop, clock.retry());
    let died = format!(
        "Symphony server reload stopped following {}: it panicked: the watcher's backend is gone. \
         It is started again at its next check; this server is still the release release-1.",
        server.home.join(release::CURRENT).display()
    );
    assert_eq!(server.notice_after(""), died);
    // Each death is followed by one wait, on a period no test reaches: only a wake starts the next.
    for death in 1..=DEATHS {
        waits.begun(death);
        assert_eq!(MADE.load(Ordering::SeqCst), death, "one start a wait");
        assert_eq!(server.notice(), died);
        server.shared.ask_handover();
    }
    // The follow that lives reads `current` before any wait of its own.
    let refused = server.notice_after(&died);
    assert_eq!(
        refused,
        format!(
            "{FAILED} The release release-2 did not take over: {} holds no manifest. It is tried \
             again at about {}.",
            release::folder(&server.home, "release-2").display(),
            clock.minute(5 * MINUTE)
        )
    );
    assert_eq!(MADE.load(Ordering::SeqCst), DEATHS + 1);
    waits.begun(DEATHS + 1);
    point(&server.home, "release-1");
    server.shared.ask_handover();
    assert_eq!(
        server.notice_after(&refused),
        "",
        "it follows the link again"
    );
    assert!(!server.shared.stopping(), "still serving");
    server.stop(follow);
}

/// What a follow says in the log: one line for each reason it hands over to nothing, when that
/// reason changes and not at each check, and one for each hand-off it begins, with why the
/// release did not take over. A server that is not following says so where an agent reads it.
#[test]
fn a_release_servers_follow_says_each_attempt_and_each_reason_it_makes_none_once() {
    use symphony::machine::release;
    let server = Reloading::new(installs_a_replacement_that_ends);
    let clock = Clock::new();
    let (backstop, waits) = backstop::watched();
    let (serve, shared) = (server.serve(), server.shared.clone());
    let following = super::Follow {
        machine: server.home.clone(),
        release: "release-1".into(),
        sockets: Vec::new(),
        lease: std::fs::File::open(&server.root).unwrap(),
    };
    let link = server.home.join(release::CURRENT);
    // The moves of `current`, each once the follow waits after the check before it, then its stop.
    let mover = std::thread::spawn({
        let (home, shared) = (server.home.clone(), server.shared.clone());
        move || {
            waits.begun(1);
            // A wake with nothing moved: the same reason, not said again.
            shared.ask_handover();
            waits.begun(2);
            for (wait, name) in [(3, "release-1"), (4, "release-2"), (5, "release-1")] {
                point(&home, name);
                shared.ask_handover();
                waits.begun(wait);
            }
            shared.stop();
        }
    });
    let retry = clock.retry();
    let ((), said) = told::during(|| {
        super::follow::<notify::NullWatcher>(&serve, &following, &shared, backstop, retry);
    });
    joined(mover, "the moves of current to end", stall());
    let named = |name: &str| {
        format!(
            "· Symphony reload: {} names {name}, the release this server is: it follows the link",
            link.display()
        )
    };
    assert_eq!(
        said,
        [
            format!(
                "· Symphony reload: {} names no release, so the release release-1 hands over to \
                 none",
                link.display()
            ),
            named("release-1"),
            "· Symphony reload: current names release-2; handing over to its server".to_string(),
            format!(
                "· Symphony reload: {FAILED} The release release-2 did not take over: {} holds no \
                 manifest. It is tried again at about {}.",
                release::folder(&server.home, "release-2").display(),
                clock.minute(5 * MINUTE)
            ),
            named("release-1"),
        ]
    );
}

#[test]
fn a_panic_is_said_by_its_text_when_it_has_one() {
    let text = |panic: Box<dyn std::any::Any + Send>| super::panicked(panic.as_ref()).to_string();
    assert_eq!(text(Box::new("a literal")), "a literal");
    assert_eq!(
        text(Box::new(String::from("a formatted one"))),
        "a formatted one"
    );
    assert_eq!(text(Box::new(7)), "with no message");
}
