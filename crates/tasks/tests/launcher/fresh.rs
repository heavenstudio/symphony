//! A run of `tools/rs-bin.sh` whose build is as Cargo left it runs that build without asking Cargo:
//! it reads the stamp the launcher wrote after the last build it asked for, and asks Cargo again
//! when anything Cargo reads moved since. The fixture is the launcher's own (a repository under the
//! temp folder over the stand-in Cargo), its main checkout holding a dependency crate the binaries
//! read and built once, with Cargo and rustup homes of its own and every file set an hour back, so
//! that only what a case changes is newer than a stamp; its builds say what they were built from
//! (`STANDIN_UPLIFT=copy`), so a stale one would say so.

use super::{Fixture, Ran, git};
use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

/// The main checkout of a fixture with a dependency crate, run with the stand-in counting its runs.
struct Tree {
    f: Fixture,
    calls: PathBuf,
    vars: Vec<(String, String)>,
}

impl Tree {
    /// A fixture whose main checkout has built once: that first build, into a target folder that
    /// did not exist, found it still missing when Cargo started (the seed's to make), and so left no
    /// stamp.
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
        tree.age();
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

    /// Every file of the fixture but those of its git and build folders set an hour back, through no
    /// link (the launcher and Cargo's wrapper are the workspace's own).
    fn age(&self) {
        age(&self.f.base);
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
}

/// Each file under `dir`, but in `.git` and `.target` folders, set an hour back; links left alone.
fn age(dir: &Path) {
    let back = SystemTime::now() - Duration::from_secs(3600);
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let kind = entry.file_type().unwrap();
        let name = entry.file_name();
        if kind.is_dir() && name != ".git" && name != ".target" {
            age(&entry.path());
        } else if kind.is_file() {
            File::open(entry.path())
                .unwrap()
                .set_modified(back)
                .unwrap();
        }
    }
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
fn each_file_cargo_reads_changed_brings_cargo_back() {
    let t = Tree::new("files");
    t.built(&[], "a first run that left no stamp");
    let main = t.main();
    // Each case changes one thing and says what the build then says; the tree is set back after
    // it, so the next starts from a good stamp.
    let cases: [(&str, &dyn Fn(), &str, &str); 13] = [
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
        (
            "a deleted source",
            &|| fs::remove_file(main.join("crates/dep/src/extra.rs")).unwrap(),
            "edited",
            " dep lib2",
        ),
        (
            "a lock made",
            &|| t.write("Cargo.lock", "lock"),
            "edited",
            " dep lib2",
        ),
        (
            "a lock changed",
            &|| t.write("Cargo.lock", "lock2"),
            "edited",
            " dep lib2",
        ),
        (
            "the workspace's manifest",
            &|| t.write("Cargo.toml", "[workspace]\n# changed\n"),
            "edited",
            " dep lib2",
        ),
        (
            "the binary's package manifest",
            &|| t.write("crates/tasks/Cargo.toml", "[package]\n# changed\n"),
            "edited",
            " dep lib2",
        ),
        (
            "a dependency crate's manifest",
            &|| t.write("crates/dep/Cargo.toml", "[package]\n# changed\n"),
            "edited",
            " dep lib2",
        ),
        (
            "a toolchain pin made",
            &|| t.write("rust-toolchain.toml", "[toolchain]\nchannel = \"1\"\n"),
            "edited",
            " dep lib2",
        ),
        (
            "a toolchain pin changed",
            &|| t.write("rust-toolchain.toml", "[toolchain]\nchannel = \"2\"\n"),
            "edited",
            " dep lib2",
        ),
        (
            "a Cargo configuration made",
            &|| t.write(".cargo/config.toml", "[build]\n"),
            "edited",
            " dep lib2",
        ),
        (
            "a Cargo configuration changed",
            &|| t.write(".cargo/config.toml", "[build]\njobs = 1\n"),
            "edited",
            " dep lib2",
        ),
    ];
    for (why, change, source, deps) in cases {
        t.age();
        t.skipped(&[], &format!("before {why}"));
        change();
        assert_eq!(t.built(&[], why), t.says(source, deps), "after {why}");
        t.age();
        assert_eq!(
            t.skipped(&[], &format!("after {why} was built")),
            t.says(source, deps)
        );
    }
    // Configuration, pins and settings outside the checkout: above it, and in Cargo's and rustup's
    // homes, made and then changed.
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
            t.age();
            t.skipped(&[], &format!("before {why} was {done}"));
            fs::write(&path, text).unwrap();
            t.built(&[], &format!("{why} was {done}"));
        }
    }
}

#[test]
fn a_switched_branch_is_built_and_switching_back_builds_again() {
    let t = Tree::new("branch");
    let main = t.main();
    let main_says = t.says("main", " dep extra dep lib");
    assert_eq!(t.built(&[], "a first run that left no stamp"), main_says);
    t.age();
    assert_eq!(t.skipped(&[], "on main"), main_says);
    // A branch made and left: main's files are written again as they were.
    git(main, &["checkout", "-q", "-b", "other"]);
    t.write("crates/tasks/src/bin/tasks.rs", "other");
    t.write("crates/dep/src/lib.rs", "otherlib");
    git(main, &["commit", "-qam", "other"]);
    git(main, &["checkout", "-q", "main"]);
    assert_eq!(t.built(&[], "the checkout of main"), main_says);
    t.age();
    assert_eq!(t.skipped(&[], "on main again"), main_says);
    git(main, &["checkout", "-q", "other"]);
    let other_says = t.says("other", " dep extra dep otherlib");
    assert_eq!(t.built(&[], "switching to other"), other_says);
    t.age();
    assert_eq!(t.skipped(&[], "on other"), other_says);
    git(main, &["checkout", "-q", "main"]);
    assert_eq!(t.built(&[], "switching back"), main_says);
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
    // compiler wrapper, a variable a build script declared, and the rustc PATH finds.
    for (name, value) in [
        ("RUSTFLAGS", "-Cdebuginfo=0"),
        ("CARGO_PROFILE_DEV_OPT_LEVEL", "1"),
        ("RUSTUP_TOOLCHAIN", "stable"),
        ("RUSTC_WRAPPER", "/usr/bin/env"),
        ("STANDIN_CC", "cc"),
        ("PATH", path.as_str()),
    ] {
        let set = [(name, value)];
        t.built(&set, &format!("{name} was set"));
        t.skipped(&set, &format!("with {name} set as before"));
        t.built(&[], &format!("{name} was left out"));
        t.skipped(&[], &format!("with {name} left out as before"));
    }
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
        t.built(
            &vars,
            &format!("a source changed, built with a {depinfo} account"),
        );
        t.age();
        t.built(&vars, &format!("a build with a {depinfo} account"));
        t.built(
            &[],
            &format!("a build with a {depinfo} account, by one with a plain one"),
        );
        t.skipped(&[], &format!("after a {depinfo} account was replaced"));
    }
    // A stamp gone, unreadable, not the launcher's, or cut short.
    let stamp = t.stamp();
    fs::remove_file(&stamp).unwrap();
    t.built(&[], "the stamp was removed");
    fs::set_permissions(&stamp, fs::Permissions::from_mode(0o000)).unwrap();
    t.built(&[], "the stamp became unreadable");
    t.skipped(&[], "after an unreadable stamp was replaced");
    let text = fs::read_to_string(&stamp).unwrap();
    fs::write(
        &stamp,
        text.replacen("symphony rs-bin fresh 1", "something else", 1),
    )
    .unwrap();
    t.built(&[], "the stamp was not the launcher's");
    let text = fs::read_to_string(&stamp).unwrap();
    fs::write(&stamp, text.trim_end_matches('\n')).unwrap();
    t.built(&[], "the stamp's last line had no newline");
    // A stamp written after now: the clock was set back since. The same stamp written back is good.
    let later = SystemTime::now() + Duration::from_secs(3600);
    let text = fs::read_to_string(&stamp).unwrap();
    fs::write(&stamp, &text).unwrap();
    File::open(&stamp).unwrap().set_modified(later).unwrap();
    t.skipped(&[], "for a stamp written back as it was");
    let at = later
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let line = text.lines().nth(1).unwrap();
    assert!(line.starts_with("at "), "{text}");
    fs::write(&stamp, text.replacen(line, &format!("at {at}"), 1)).unwrap();
    File::open(&stamp).unwrap().set_modified(later).unwrap();
    t.built(&[], "the stamp was written in the future");
    // The build it runs gone.
    fs::remove_file(t.main().join(".target/rs-bin/tasks")).unwrap();
    assert_eq!(t.built(&[], "the build it runs was removed"), built);
    t.skipped(&[], "after the build was linked again");
}

#[test]
fn the_stamps_time_is_taken_before_cargo_starts() {
    // A file changed while Cargo builds is newer than the stamp, so the next run asks Cargo: the
    // stamp's time is no later than the stand-in's first act, appending to its count.
    let t = Tree::new("time");
    t.built(&[], "a first run that left no stamp");
    t.write("crates/tasks/src/bin/tasks.rs", "later");
    t.built(&[], "the edit");
    let mtime = |path: &Path| fs::metadata(path).unwrap().modified().unwrap();
    assert!(
        mtime(&t.stamp()) <= mtime(&t.calls),
        "the stamp's time {:?} is after Cargo started, {:?}",
        mtime(&t.stamp()),
        mtime(&t.calls)
    );
}

#[test]
fn runs_at_once_each_run_the_new_build_and_leave_a_good_stamp() {
    let t = Tree::new("together");
    t.built(&[], "a first run that left no stamp");
    t.write("crates/tasks/src/bin/tasks.rs", "together");
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
    t.age();
    assert_eq!(t.skipped(&[], "after the runs at once"), says);
}
