use super::{RunningImage, Setup, source_event, stamp_install, watch_with};
use crate::front::{Shared, told};
use crate::gate::Gate;
use crate::testing::{Tree, git, repository};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
        Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now)),
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
    let worker =
        watch_with::<notify::NullWatcher>(serve, setup, shared.clone(), Vec::new(), period)
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
    worker.join().unwrap();
}

#[test]
fn client_fingerprint_and_events_cover_assets_config_and_transitive_crates() {
    let tree = Tree::new();
    let root = repository(&tree);
    tree.write(
        "main/symphony/apps/symphony-server/Cargo.toml",
        "[package]\nname = \"symphony-server\"\n",
    );
    tree.write(
        "main/symphony/apps/symphony-server/src/main.rs",
        "fn main() {}\n",
    );
    tree.write("main/symphony/apps/symphony-ui/Cargo.toml", "[package]\nname = \"symphony-ui\"\n[dependencies]\nwire = { path = \"../../../crates/wire\" }\n");
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
    for file in [
        "symphony/apps/symphony-ui/src/main.rs",
        "symphony/apps/symphony-ui/assets/docs.css",
        "symphony/apps/symphony-ui/Dioxus.toml",
        "symphony/apps/symphony-ui/index.html",
        "symphony/apps/symphony-ui/bin/cargo-symphony-ui",
        "symphony/apps/symphony-ui/bundle.sh",
        "symphony/apps/symphony-ui/dx-path.sh",
        "symphony/apps/symphony-ui/package.sh",
        "symphony/apps/symphony-ui/tools.sh",
        "symphony/apps/symphony-ui/wasm-fresh.sh",
        "crates/wire/src/lib.rs",
        "crates/leaf/src/lib.rs",
        "crates/leaf/build.rs",
        "Cargo.toml",
        "Cargo.lock",
        "symphony/Cargo.toml",
        "symphony/Cargo.lock",
        "symphony/.cargo/config.toml",
        "symphony/justfile",
        "symphony/tools/rs-bin.sh",
        "symphony/tools/check_lock.sh",
        "rust-toolchain.toml",
        ".cargo/config.toml",
        "justfile",
        "tools/rs-bin.sh",
    ] {
        let all_before = symphony::reload::fingerprint(&root).unwrap();
        tree.write(&format!("main/{file}"), "changed build input\n");
        let after = symphony::reload::client_fingerprint(&root).unwrap();
        assert_ne!(before, after, "client identity must include {file}");
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
    for file in [
        "symphony/apps/symphony-server/src/main.rs",
        "symphony/apps/symphony-ui/tests/view.rs",
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
    std::fs::remove_file(root.join("symphony/apps/symphony-ui/assets/docs.css")).unwrap();
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
        ("symphony/apps/symphony-ui/src/main.rs", true),
        ("symphony/apps/symphony-ui/assets/docs.css", true),
        ("symphony/apps/symphony-ui/Dioxus.toml", true),
        ("symphony/apps/symphony-ui/index.html", true),
        ("symphony/apps/symphony-ui/bin/cargo-symphony-ui", true),
        ("symphony/apps/symphony-ui/bundle.sh", true),
        ("symphony/apps/symphony-ui/package.sh", true),
        ("symphony/apps/symphony-ui/tools.sh", true),
        ("symphony/apps/symphony-ui/wasm-fresh.sh", true),
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
        ("apps/symphony-ui/assets/frame.css", true),
        ("apps/symphony-server/client/public/index.html", true),
        (
            "apps/symphony-server/client/public/assets/current.wasm",
            true,
        ),
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
            Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now)),
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
        let serve = crate::cli::Serve {
            home: Some(self.home.clone()),
            root: self.root.clone(),
            port: 1,
            hosts: Vec::new(),
            install: None,
        };
        watch_with::<W>(serve, setup, self.shared.clone(), Vec::new(), PERIOD).unwrap()
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

    /// The worker stopped and joined.
    fn stop(&self, worker: std::thread::JoinHandle<()>) {
        self.shared.stop();
        worker.join().unwrap();
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
    worker.join().unwrap();
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
