//! A run of `tools/rs-bin.sh` whose build is as Cargo left it runs that build without asking Cargo:
//! it reads the stamp the launcher wrote after the last build it asked for, and asks Cargo again
//! when anything Cargo reads changed since that build started. The fixture is the launcher's own (a
//! repository under the temp folder over the stand-in Cargo), its main checkout holding a dependency
//! crate the binaries read and built once, with Cargo and rustup homes of its own. A file changed is
//! one whose status changed in the stamp's second or after, which nothing but the clock sets back; so
//! a case that needs a stamp to vouch for the changes made before it waits for the clock's next
//! second before that build starts ([`settle`]). Its builds say what they were built from
//! (`STANDIN_UPLIFT=copy`), so a stale one would say so.

use super::{Fixture, Ran, git};
use std::fs::{self, File};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The main checkout of a fixture with a dependency crate, run with the stand-in counting its runs.
struct Tree {
    f: Fixture,
    calls: PathBuf,
    vars: Vec<(String, String)>,
}

impl Tree {
    /// A fixture whose main checkout has built once: that first build, into a target folder that
    /// did not exist, found it still missing when Cargo started (the seed's to make), and so left no
    /// stamp. It started in a later second than any file was made, so the next build's stamp
    /// vouches for them all.
    fn new(name: &str) -> Tree {
        let f = Fixture::new(name);
        for (file, text) in [
            ("crates/dep/Cargo.toml", "[package]\n"),
            ("crates/dep/src/lib.rs", "lib"),
            ("crates/dep/src/extra.rs", "extra"),
        ] {
            let path = f.main.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        git(&f.main, &["add", "-A"]);
        git(&f.main, &["commit", "-qm", "dep"]);
        let calls = f.base.join("calls");
        let homes = [("CARGO_HOME", "cargo-home"), ("RUSTUP_HOME", "rustup-home")];
        let mut vars = vec![
            ("STANDIN_UPLIFT".to_string(), "copy".to_string()),
            ("STANDIN_CALLS".to_string(), calls.display().to_string()),
        ];
        for (name, folder) in homes {
            let home = f.base.join(folder);
            fs::create_dir_all(&home).unwrap();
            vars.push((name.to_string(), home.display().to_string()));
        }
        let tree = Tree { f, calls, vars };
        settle();
        assert_eq!(
            tree.run(&[], &["tasks", "list"]).ok(),
            tree.says("main", " dep extra dep lib")
        );
        assert_eq!(
            fs::read_to_string(&tree.calls).unwrap(),
            "tasks into a new target\n"
        );
        assert!(!tree.stamp().exists(), "the first build left a stamp");
        tree
    }

    /// The main checkout's root.
    fn main(&self) -> &Path {
        &self.f.main
    }

    /// The fixture's variables.
    fn vars(&self) -> Vec<(&str, &str)> {
        self.vars
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect()
    }

    /// The launcher in the main checkout, run there with `extra` on the fixture's variables.
    fn run(&self, extra: &[(&str, &str)], args: &[&str]) -> Ran {
        let mut vars = self.vars();
        vars.extend_from_slice(extra);
        self.f.run(self.main(), self.main(), &vars, args)
    }

    /// How many times the stand-in Cargo has run.
    fn calls(&self) -> usize {
        fs::read_to_string(&self.calls)
            .map(|text| text.lines().count())
            .unwrap_or(0)
    }

    /// What `tasks list` printed, having asked Cargo exactly once.
    fn built(&self, extra: &[(&str, &str)], why: &str) -> String {
        let before = self.calls();
        let out = self.run(extra, &["tasks", "list"]).ok();
        assert_eq!(self.calls(), before + 1, "Cargo was not asked after {why}");
        out
    }

    /// What `tasks list` printed, having run without asking Cargo.
    fn skipped(&self, extra: &[(&str, &str)], why: &str) -> String {
        let before = self.calls();
        let out = self.run(extra, &["tasks", "list"]).ok();
        assert_eq!(self.calls(), before, "Cargo was asked {why}");
        out
    }

    /// What `tasks list` printed once `change` was made to a tree whose stamp vouched for it before:
    /// a build, which the next run takes without asking Cargo.
    fn changed(&self, why: &str, change: impl FnOnce()) -> String {
        self.skipped(&[], &format!("before {why}"));
        change();
        settle();
        let out = self.built(&[], why);
        assert_eq!(
            self.skipped(&[], &format!("after {why} was built")),
            out,
            "after {why}"
        );
        out
    }

    /// What a build of `tasks` from this checkout says when built from `source` and `deps`.
    fn says(&self, source: &str, deps: &str) -> String {
        format!(
            "tasks from {} source {source}{deps} exe {} cwd {} args list",
            self.main().display(),
            self.main().join(".target/rs-bin/tasks").display(),
            self.main().display()
        )
    }

    /// The stamp the launcher keeps for `tasks`.
    fn stamp(&self) -> PathBuf {
        self.main().join(".target/rs-bin/.tasks.fresh")
    }

    /// `text` written at `file` under the main checkout, its folder made.
    fn write(&self, file: &str, text: &str) {
        let path = self.main().join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    /// A file holding `text` outside the checkout, its times set an hour back, as a copy or an
    /// archive keeps a file written earlier.
    fn older(&self, text: &str) -> PathBuf {
        let folder = self.f.base.join("older");
        fs::create_dir_all(&folder).unwrap();
        let path = folder.join(text);
        fs::write(&path, text).unwrap();
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(3600))
            .unwrap();
        path
    }
}

/// The second the clock reads.
fn second() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// Waits for the clock's next second, so a build started after it vouches for every change made
/// before.
fn settle() {
    let now = second();
    test_time::until_true(
        "the clock's next second",
        test_time::backstop(Duration::from_secs(1)),
        || second() > now,
    )
    .unwrap();
}

/// The stamp's text with its second set to `at`.
fn at(text: &str, at: i64) -> String {
    let line = text.lines().nth(1).unwrap();
    assert!(line.starts_with("at "), "{text}");
    text.replacen(line, &format!("at {at}"), 1)
}

#[test]
fn a_tree_unchanged_since_its_last_build_runs_that_build_without_cargo() {
    let t = Tree::new("unchanged");
    let built = t.built(&[], "a first run that left no stamp");
    assert_eq!(built, t.says("main", " dep extra dep lib"));
    assert_eq!(t.skipped(&[], "of an unchanged tree"), built);
    assert_eq!(t.skipped(&[], "of an unchanged tree again"), built);
    // A variable nothing in the build reads changes nothing.
    assert_eq!(
        t.skipped(&[("TASKS_SESSION", "someone")], "for an unread variable"),
        built
    );
    // `--build` and `--link` always ask Cargo, and leave the stamp good.
    for mode in ["--build", "--link"] {
        let before = t.calls();
        t.run(&[], &[mode, "tasks"]).ok();
        assert_eq!(t.calls(), before + 1, "{mode} did not ask Cargo");
    }
    assert_eq!(t.skipped(&[], "after --build and --link"), built);
    // Each binary has a stamp of its own.
    let before = t.calls();
    t.run(&[], &["symphony", "x"]).ok();
    t.run(&[], &["symphony", "x"]).ok();
    assert_eq!(t.calls(), before + 1, "symphony's second run asked Cargo");
    // A worktree reaching the live board runs the main checkout's build: unchanged, without Cargo.
    let from_wt =
        t.f.run(&t.f.wt, &t.f.wt, &t.vars(), &["tasks", "list"])
            .ok();
    assert!(
        from_wt.starts_with(&format!("tasks from {} source main ", t.main().display())),
        "{from_wt}"
    );
    assert_eq!(t.calls(), before + 1, "the worktree's run asked Cargo");
}

#[test]
fn each_source_changed_brings_cargo_back_however_it_arrived() {
    let t = Tree::new("sources");
    t.built(&[], "a first run that left no stamp");
    let main = t.main();
    let source = main.join("crates/tasks/src/bin/tasks.rs");
    let cases: [(&str, &dyn Fn(), &str, &str); 6] = [
        (
            "an edited source",
            &|| t.write("crates/tasks/src/bin/tasks.rs", "edited"),
            "edited",
            " dep extra dep lib",
        ),
        (
            "a source written again as it was",
            &|| t.write("crates/tasks/src/bin/tasks.rs", "edited"),
            "edited",
            " dep extra dep lib",
        ),
        (
            "an edited source of a dependency crate",
            &|| t.write("crates/dep/src/lib.rs", "lib2"),
            "edited",
            " dep extra dep lib2",
        ),
        // Its time an hour back, before the build Cargo last compiled and before a `--build` that
        // asked Cargo for nothing new: what a move, `cp -p`, `rsync -a` or an archive leaves.
        (
            "a source written earlier moved into place after a --build",
            &|| {
                t.run(&[], &["--build", "tasks"]).ok();
                fs::rename(t.older("moved"), &source).unwrap();
            },
            "moved",
            " dep extra dep lib2",
        ),
        (
            "a source written earlier copied into place with its times",
            &|| {
                let older = t.older("copied");
                let done = Command::new("cp")
                    .arg("-p")
                    .arg(&older)
                    .arg(&source)
                    .status();
                assert!(done.unwrap().success());
                assert_eq!(
                    fs::metadata(&source).unwrap().modified().unwrap(),
                    fs::metadata(&older).unwrap().modified().unwrap()
                );
            },
            "copied",
            " dep extra dep lib2",
        ),
        (
            "a deleted source",
            &|| fs::remove_file(main.join("crates/dep/src/extra.rs")).unwrap(),
            "copied",
            " dep lib2",
        ),
    ];
    for (why, change, source, deps) in cases {
        assert_eq!(t.changed(why, change), t.says(source, deps), "after {why}");
    }
}

#[test]
fn everything_under_a_folder_a_build_script_watches_counts() {
    let t = Tree::new("watched");
    let says = |assets: &str| t.says("main", &format!(" dep extra dep lib{assets}"));
    t.write("crates/dep/assets/deep/a.txt", "a");
    settle();
    assert_eq!(t.built(&[], "a watched folder was made"), says(" asset a"));
    let deep = t.main().join("crates/dep/assets/deep");
    let cases: [(&str, &dyn Fn(), &str); 4] = [
        (
            "a file in a folder under it edited in place",
            &|| t.write("crates/dep/assets/deep/a.txt", "a2"),
            " asset a2",
        ),
        (
            "a file made in a folder under it",
            &|| t.write("crates/dep/assets/deep/b.txt", "b"),
            " asset a2 asset b",
        ),
        (
            "a file removed from a folder under it",
            &|| fs::remove_file(deep.join("a.txt")).unwrap(),
            " asset b",
        ),
        (
            "a file in a folder made under that one",
            &|| t.write("crates/dep/assets/deep/deeper/c.txt", "c"),
            " asset b asset c",
        ),
    ];
    for (why, change, assets) in cases {
        assert_eq!(t.changed(why, change), says(assets), "after {why}");
    }
    let edit = || t.write("crates/dep/assets/deep/deeper/c.txt", "c2");
    assert_eq!(
        t.changed("a file two folders down edited in place", edit),
        says(" asset b asset c2")
    );
}

#[test]
fn each_setting_in_the_checkout_changed_brings_cargo_back() {
    let t = Tree::new("settings");
    let says = t.built(&[], "a first run that left no stamp");
    for (why, file, text) in [
        ("a lock made", "Cargo.lock", "lock"),
        ("a lock changed", "Cargo.lock", "lock2"),
        (
            "the workspace's manifest",
            "Cargo.toml",
            "[workspace]\n# changed\n",
        ),
        (
            "the binary's package manifest",
            "crates/tasks/Cargo.toml",
            "[package]\n# changed\n",
        ),
        (
            "a dependency crate's manifest",
            "crates/dep/Cargo.toml",
            "[package]\n# changed\n",
        ),
        (
            "a toolchain pin made",
            "rust-toolchain.toml",
            "[toolchain]\nchannel = \"1\"\n",
        ),
        (
            "a toolchain pin changed",
            "rust-toolchain.toml",
            "[toolchain]\nchannel = \"2\"\n",
        ),
        (
            "a Cargo configuration made",
            ".cargo/config.toml",
            "[build]\n",
        ),
        (
            "a Cargo configuration changed",
            ".cargo/config.toml",
            "[build]\njobs = 1\n",
        ),
    ] {
        assert_eq!(t.changed(why, || t.write(file, text)), says);
    }
}

#[test]
fn each_setting_outside_the_checkout_changed_brings_cargo_back() {
    // Configuration, pins and settings outside the checkout: above it, and in Cargo's and rustup's
    // homes, made and then changed.
    let t = Tree::new("outside");
    t.built(&[], "a first run that left no stamp");
    let base = &t.f.base;
    for (why, path) in [
        ("a pin above the checkout", base.join("rust-toolchain")),
        (
            "a configuration above the checkout",
            base.join(".cargo/config"),
        ),
        (
            "Cargo's own configuration",
            base.join("cargo-home/config.toml"),
        ),
        ("rustup's settings", base.join("rustup-home/settings.toml")),
    ] {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        for (done, text) in [("made", "x"), ("changed", "y")] {
            t.changed(&format!("{why} was {done}"), || {
                fs::write(&path, text).unwrap()
            });
        }
    }
}

#[test]
fn a_switched_branch_is_built_and_switching_back_builds_again() {
    let t = Tree::new("branch");
    let main = t.main();
    let main_says = t.says("main", " dep extra dep lib");
    assert_eq!(t.built(&[], "a first run that left no stamp"), main_says);
    // A branch made and left: main's files are written again as they were.
    let made = || {
        git(main, &["checkout", "-q", "-b", "other"]);
        t.write("crates/tasks/src/bin/tasks.rs", "other");
        t.write("crates/dep/src/lib.rs", "otherlib");
        git(main, &["commit", "-qam", "other"]);
        git(main, &["checkout", "-q", "main"]);
    };
    assert_eq!(t.changed("a branch made and left", made), main_says);
    let other = || git(main, &["checkout", "-q", "other"]);
    assert_eq!(
        t.changed("switching to other", other),
        t.says("other", " dep extra dep otherlib")
    );
    let back = || git(main, &["checkout", "-q", "main"]);
    assert_eq!(t.changed("switching back", back), main_says);
}

#[test]
fn the_environment_cargo_reads_changed_brings_cargo_back() {
    let t = Tree::new("environment");
    t.built(&[], "a first run that left no stamp");
    t.skipped(&[], "of an unchanged tree");
    // A program named rustc earlier on PATH than the one PATH found before.
    let programs = t.f.base.join("programs");
    fs::create_dir_all(&programs).unwrap();
    let rustc = programs.join("rustc");
    fs::write(&rustc, "#!/bin/sh\n").unwrap();
    fs::set_permissions(&rustc, fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", programs.display(), std::env::var("PATH").unwrap());
    // Each set, then left out again: rustc's flags, a profile's setting, the rustup toolchain, a
    // compiler wrapper, a variable a build script declared, one the binary's sources read and one
    // the build script's own sources read (rustc's `# env-dep:`), and the rustc PATH finds.
    for (name, value) in [
        ("RUSTFLAGS", "-Cdebuginfo=0"),
        ("CARGO_PROFILE_DEV_OPT_LEVEL", "1"),
        ("RUSTUP_TOOLCHAIN", "stable"),
        ("RUSTC_WRAPPER", "/usr/bin/env"),
        ("STANDIN_CC", "cc"),
        ("STANDIN_PROBE", "x"),
        ("STANDIN_BUILD_PROBE", "x"),
        ("PATH", path.as_str()),
    ] {
        let set = [(name, value)];
        t.built(&set, &format!("{name} was set"));
        t.skipped(&set, &format!("with {name} set as before"));
        t.built(&[], &format!("{name} was left out"));
        t.skipped(&[], &format!("with {name} left out as before"));
    }
    // What a source reads is what its build says: the build each value made.
    let says = |probe: &str| t.says("main", &format!(" dep extra dep lib{probe}"));
    let probe = |value| [("STANDIN_PROBE", value)];
    assert_eq!(t.built(&probe("y"), "STANDIN_PROBE=y"), says(" probe y"));
    assert_eq!(
        t.skipped(&probe("y"), "with STANDIN_PROBE=y"),
        says(" probe y")
    );
    assert_eq!(t.built(&probe(""), "STANDIN_PROBE empty"), says(" probe "));
    assert_eq!(t.built(&[], "STANDIN_PROBE left out"), says(""));
    // An empty value is not the variable left out.
    t.built(&[("RUSTFLAGS", "")], "RUSTFLAGS was set empty");
    t.skipped(&[("RUSTFLAGS", "")], "with RUSTFLAGS empty as before");
    t.built(&[], "RUSTFLAGS was left out");
    // A declared name bash cannot read, as a target triple's hyphens make one, breaks no run.
    let out = t
        .run(&[("CC_standin-triple", "cc")], &["tasks", "list"])
        .ok();
    assert_eq!(out, t.says("main", " dep extra dep lib"));
}

#[test]
fn any_doubt_asks_cargo() {
    let t = Tree::new("doubt");
    let built = t.built(&[], "a first run that left no stamp");
    t.skipped(&[], "of an unchanged tree");
    // A build with no account of what it read, or one naming a path spelled with an escape, leaves
    // no stamp, so the next run asks Cargo too, and the one after a plain account no longer does.
    for depinfo in ["none", "escaped"] {
        let vars = [("STANDIN_DEPINFO", depinfo)];
        t.write("crates/tasks/src/bin/tasks.rs", "main");
        settle();
        t.built(
            &vars,
            &format!("a source changed, built with a {depinfo} account"),
        );
        t.built(&vars, &format!("a build with a {depinfo} account"));
        t.built(
            &[],
            &format!("a build with a {depinfo} account, by one with a plain one"),
        );
        t.skipped(&[], &format!("after a {depinfo} account was replaced"));
    }
    // A stamp gone, unreadable, of the format before or not the launcher's, or cut short.
    let stamp = t.stamp();
    fs::remove_file(&stamp).unwrap();
    t.built(&[], "the stamp was removed");
    fs::set_permissions(&stamp, fs::Permissions::from_mode(0o000)).unwrap();
    t.built(&[], "the stamp became unreadable");
    t.skipped(&[], "after an unreadable stamp was replaced");
    for (header, why) in [
        ("symphony rs-bin fresh 1", "of the format before"),
        ("something else", "not the launcher's"),
    ] {
        let text = fs::read_to_string(&stamp).unwrap();
        fs::write(&stamp, text.replacen("symphony rs-bin fresh 2", header, 1)).unwrap();
        t.built(&[], &format!("the stamp was {why}"));
    }
    let text = fs::read_to_string(&stamp).unwrap();
    fs::write(&stamp, text.trim_end_matches('\n')).unwrap();
    t.built(&[], "the stamp's last line had no newline");
    // A stamp whose second is after now: the clock was set back since. The same stamp written back
    // as it was is good.
    let text = fs::read_to_string(&stamp).unwrap();
    fs::write(&stamp, &text).unwrap();
    t.skipped(&[], "for a stamp written back as it was");
    let later = second() as i64 + 3600;
    fs::write(&stamp, at(&text, later)).unwrap();
    t.built(&[], "the stamp's second was after now");
    // A stamp written for another checkout's build in a target folder both share.
    let shared = t.main().join(".target");
    let vars = [("CARGO_TARGET_DIR", shared.to_str().unwrap())];
    let wt = &t.f.wt;
    let mut all = t.vars();
    all.extend_from_slice(&vars);
    let before = t.calls();
    let theirs =
        t.f.run(wt, wt, &all, &["tasks", "--database", "scratch", "list"]);
    assert!(
        theirs
            .ok()
            .starts_with(&format!("tasks from {} source branch ", wt.display())),
        "the worktree ran a build from another checkout"
    );
    assert_eq!(t.calls(), before + 1, "the worktree did not ask Cargo");
    assert_eq!(t.built(&[], "another checkout's stamp"), built);
    // The build it runs gone.
    fs::remove_file(t.main().join(".target/rs-bin/tasks")).unwrap();
    assert_eq!(t.built(&[], "the build it runs was removed"), built);
    t.skipped(&[], "after the build was linked again");
}

#[test]
fn a_stamp_touched_copied_or_restored_vouches_for_no_change_after_its_second() {
    let t = Tree::new("copied");
    t.built(&[], "a first run that left no stamp");
    t.skipped(&[], "of an unchanged tree");
    let stamp = t.stamp();
    let saved = t.f.base.join("saved");
    fs::copy(&stamp, &saved).unwrap();
    let ahead = |path: &Path| {
        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(SystemTime::now() + Duration::from_secs(3600))
            .unwrap();
    };
    // A source changed, then the stamp's times set an hour ahead.
    t.write("crates/tasks/src/bin/tasks.rs", "touched");
    ahead(&stamp);
    settle();
    let says = t.says("touched", " dep extra dep lib");
    assert_eq!(t.built(&[], "a change, the stamp touched after it"), says);
    t.skipped(&[], "after the touched stamp was replaced");
    // The stamp from before that change copied back with its times, then written back and set
    // ahead.
    let done = Command::new("cp")
        .arg("-p")
        .arg(&saved)
        .arg(&stamp)
        .status();
    assert!(done.unwrap().success());
    t.built(&[], "the stamp from before the change was copied back");
    t.skipped(&[], "after the copied stamp was replaced");
    fs::copy(&saved, &stamp).unwrap();
    ahead(&stamp);
    t.built(
        &[],
        "the stamp from before the change was restored and touched",
    );
}

#[test]
fn a_change_in_the_stamps_own_second_counts_and_one_in_the_second_before_does_not() {
    let t = Tree::new("second");
    t.built(&[], "a first run that left no stamp");
    t.skipped(&[], "of an unchanged tree");
    let stamp = t.stamp();
    let text = fs::read_to_string(&stamp).unwrap();
    // The second of the latest change to anything the stamp names, links followed.
    let newest = text
        .lines()
        .filter_map(|line| line.strip_prefix("+ "))
        .map(|path| fs::metadata(path).unwrap().ctime())
        .max()
        .unwrap();
    fs::write(&stamp, at(&text, newest + 1)).unwrap();
    t.skipped(&[], "for a stamp a second after the latest change");
    fs::write(&stamp, at(&text, newest)).unwrap();
    t.built(&[], "for a stamp in the second of the latest change");
}

#[test]
fn a_file_changed_while_cargo_builds_brings_cargo_back() {
    // The stand-in writes the binary's source once it has read the sources, and finishes in a later
    // second: the stamp's second, taken before Cargo started, is no later than the change.
    let t = Tree::new("during");
    t.built(&[], "a first run that left no stamp");
    let source = t.main().join("crates/tasks/src/bin/tasks.rs");
    let edit = [("STANDIN_EDIT", source.to_str().unwrap())];
    let main_says = t.says("main", " dep extra dep lib");
    t.write("crates/tasks/src/bin/tasks.rs", "main");
    assert_eq!(t.built(&edit, "a source written as it was"), main_says);
    let edited = t.says("edited while building", " dep extra dep lib");
    assert_eq!(t.built(&[], "a source changed while Cargo built"), edited);
    assert_eq!(t.skipped(&[], "after that change was built"), edited);
}

#[test]
fn runs_at_once_each_run_the_new_build_and_leave_a_good_stamp() {
    let t = Tree::new("together");
    t.built(&[], "a first run that left no stamp");
    t.write("crates/tasks/src/bin/tasks.rs", "together");
    settle();
    let runs: Vec<_> = (0..4)
        .map(|_| {
            let mut command = Command::new(t.main().join("tools/rs-bin.sh"));
            for name in [
                "TASKS_SERVER",
                "TASKS_DATABASE",
                "CARGO_TARGET_DIR",
                "GIT_DIR",
            ] {
                command.env_remove(name);
            }
            command
                .args(["tasks", "list"])
                .current_dir(t.main())
                .env("CARGO", super::standin_cargo())
                .envs(t.vars())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    let says = t.says("together", " dep extra dep lib");
    for run in runs {
        let done = run.wait_with_output().unwrap();
        let err = String::from_utf8_lossy(&done.stderr);
        assert!(done.status.success(), "{err}");
        assert_eq!(
            String::from_utf8(done.stdout).unwrap().trim(),
            says,
            "{err}"
        );
    }
    assert_eq!(t.skipped(&[], "after the runs at once"), says);
}
