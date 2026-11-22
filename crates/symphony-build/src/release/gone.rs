//! A red check whose build lost files while it ran, said as what it is (docs/workflow/RELEASES.md): an
//! environment failure, not failed tests. A target that never ran, or never compiled, because a
//! file its own build had made was gone ([`super::rerun`] reads which from the log) failed for its
//! build folder: what removed the file ran while the target the log names ran ([`Went`]), that
//! target or something it started, or something outside the run; in a run of the tests' lanes side
//! by side, while those lanes ran, any of them, never one target the log's order names. What the
//! build folder holds when the red is filed, each entry with when it was made, names who wrote
//! there after the build.

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use super::record::local;
use super::rerun::{Log, Went};

/// How many of the targets and of the files a red task names at most.
const LISTED: usize = 3;
/// How many entries of the build folder it names at most.
const HELD_LISTED: usize = 8;

/// What a red task says of the files `log` says its targets found gone; none when none did.
pub(crate) fn said(log: &Log) -> Option<String> {
    let failed: Vec<&str> = (log.targets.iter())
        .filter(|failed| !failed.gone.is_empty())
        .map(|failed| failed.target.as_str())
        .collect();
    if failed.is_empty() {
        return None;
    }
    let mut files: Vec<&str> = Vec::new();
    for file in log.targets.iter().flat_map(|failed| &failed.gone) {
        if !files.contains(&file.as_str()) {
            files.push(file);
        }
    }
    let target = "that target or something it started";
    let (when, who) = match &log.went {
        Some(Went::While(header, selected)) => {
            let selected = (selected.as_ref())
                .map(|args| format!(" (`{args}`)"))
                .unwrap_or_default();
            let when = format!(
                "while `{header}`{selected} ran, the last target cargo started before the first \
                 that found one gone"
            );
            (when, target)
        }
        Some(Went::Lanes(lanes, at_once)) => {
            let names: Vec<&str> = lanes.iter().map(String::as_str).collect();
            let at_once = if *at_once >= lanes.len() {
                "all at once".to_string()
            } else {
                format!("at most {at_once} at once")
            };
            let when = format!(
                "while the tests' {count} lanes ran side by side, {at_once} ({names})",
                count = lanes.len(),
                names = listed(&names, names.len()),
            );
            let who = "a test of any of those lanes or something it started (their outputs follow \
                       one another in the log, but they ran at the same time, so it names no one \
                       target)";
            (when, who)
        }
        Some(Went::Building) | None => ("while it built, before any target ran".into(), target),
    };
    let folder = (files.iter())
        .find_map(|file| folder_of(Path::new(file)))
        .map(|(folder, profile)| format!(" {}", holds(&folder, &profile)))
        .unwrap_or_default();
    Some(format!(
        "An environment failure, not a failed test: {count} of its failed targets found files its \
         own build had made gone, and never ran or never compiled for that ({targets}): {files}. \
         They went {when}: what removed them ran then, {who}, or something outside the \
         run.{folder}",
        count = failed.len(),
        targets = listed(&failed, LISTED),
        files = listed(&files, LISTED),
    ))
}

/// The first `most` of `items`, each in backquotes, and how many more there are.
fn listed(items: &[&str], most: usize) -> String {
    let mut said: Vec<String> = (items.iter().take(most))
        .map(|item| format!("`{item}`"))
        .collect();
    if items.len() > most {
        said.push(format!("{} more", items.len() - most));
    }
    match said.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => said.concat(),
    }
}

/// The build folder a file of a build is in, and the profile's folder in it that holds the file:
/// `F/PROFILE/deps/FILE` or `F/PROFILE/build/UNIT/out/…`. None for a path of no other shape.
fn folder_of(file: &Path) -> Option<(PathBuf, PathBuf)> {
    let parts: Vec<Component<'_>> = file.components().collect();
    (2..parts.len()).rev().find_map(|at| {
        let name = parts[at].as_os_str();
        let deps = name == "deps" && at + 1 < parts.len();
        let out = name == "build"
            && parts
                .get(at + 2)
                .is_some_and(|part| part.as_os_str() == "out");
        if !(deps || out) {
            return None;
        }
        let profile: PathBuf = parts[..at].iter().collect();
        Some((profile.parent()?.to_path_buf(), profile))
    })
}

/// What the build folder `folder` holds now, each entry with when it was made, and whether the
/// profile's folder `profile` is still in it.
fn holds(folder: &Path, profile: &Path) -> String {
    let Ok(entries) = fs::read_dir(folder) else {
        return format!("Its build folder, {}, is gone.", folder.display());
    };
    let mut held: Vec<(String, Option<f64>)> = entries
        .filter_map(Result::ok)
        .map(|entry| {
            let meta = entry.metadata().ok();
            let mut name = entry.file_name().to_string_lossy().into_owned();
            if meta.as_ref().is_some_and(fs::Metadata::is_dir) {
                name.push('/');
            }
            let made = (meta.as_ref())
                .and_then(|meta| meta.created().or_else(|_| meta.modified()).ok())
                .and_then(|when| when.duration_since(UNIX_EPOCH).ok())
                .map(|since| since.as_secs_f64());
            (name, made)
        })
        .collect();
    held.sort_by(|(one, _), (other, _)| one.cmp(other));
    let profile_name = profile.file_name().map_or_else(
        || profile.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let profile_there = if profile.is_dir() {
        format!("its `{profile_name}/` among them")
    } else {
        format!("and no `{profile_name}/`")
    };
    if held.is_empty() {
        return format!(
            "Its build folder, {}, holds nothing now, {profile_there}.",
            folder.display()
        );
    }
    let mut said: Vec<String> = (held.iter().take(HELD_LISTED))
        .map(|(name, made)| match made {
            Some(made) => format!("`{name}` (made {})", local(*made)),
            None => format!("`{name}`"),
        })
        .collect();
    if held.len() > HELD_LISTED {
        said.push(format!("{} more", held.len() - HELD_LISTED));
    }
    format!(
        "Its build folder, {}, holds now: {}, {profile_there}.",
        folder.display(),
        said.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::release::rerun::Failed;
    use crate::test_tree::Tree;

    fn failed(target: &str, tests: &[&str], gone: &[&str]) -> Failed {
        Failed {
            target: target.into(),
            tests: tests.iter().map(|test| (*test).to_string()).collect(),
            gone: gone.iter().map(|file| (*file).to_string()).collect(),
        }
    }

    #[test]
    fn a_build_file_is_placed_in_its_build_folder_and_profile() {
        for (file, folder, profile) in [
            (
                "/c/.target/debug/deps/a-0123",
                "/c/.target",
                "/c/.target/debug",
            ),
            (
                "/c/.git/t/release/deps/liba-1.rlib",
                "/c/.git/t",
                "/c/.git/t/release",
            ),
            (
                "/c/.target/debug/build/index-c3/out/index.rs",
                "/c/.target",
                "/c/.target/debug",
            ),
            (
                "/c/t/aarch64-apple-darwin/debug/deps/a-1",
                "/c/t/aarch64-apple-darwin",
                "/c/t/aarch64-apple-darwin/debug",
            ),
        ] {
            assert_eq!(
                folder_of(Path::new(file)),
                Some((PathBuf::from(folder), PathBuf::from(profile))),
                "{file}"
            );
        }
        for file in [
            "/c/src/index.rs",
            "/c/.target/debug/deps",
            "/c/.target/debug/build/index-c3/index.rs",
            "deps/a",
            "",
        ] {
            assert_eq!(folder_of(Path::new(file)), None, "{file}");
        }
    }

    #[test]
    fn a_list_names_its_first_items_and_counts_the_rest() {
        assert_eq!(listed(&[], 3), "");
        assert_eq!(listed(&["a"], 3), "`a`");
        assert_eq!(listed(&["a", "b"], 3), "`a` and `b`");
        assert_eq!(listed(&["a", "b", "c"], 3), "`a`, `b` and `c`");
        assert_eq!(
            listed(&["a", "b", "c", "d", "e"], 3),
            "`a`, `b`, `c` and 2 more"
        );
    }

    #[test]
    fn a_log_whose_targets_failed_by_themselves_says_nothing_of_the_environment() {
        let log = Log {
            targets: vec![
                failed("-p a --lib", &["t"], &[]),
                failed("-p a --test b", &[], &[]),
            ],
            ..Log::default()
        };
        assert_eq!(said(&log), None);
        assert_eq!(said(&Log::default()), None);
    }

    #[test]
    fn files_gone_from_the_build_are_an_environment_failure_naming_when_and_what_is_there_now() {
        let tree = Tree::new();
        // What the folder holds once something emptied it and wrote its own marks.
        let target = tree.path("copy/.target");
        tree.write("copy/.target/tree", "the tree that emptied it");
        tree.write("copy/.target/bin/tool", "");
        let file = |rest: &str| format!("{}/{rest}", target.display());
        let (unexecuted, rlib, out) = (
            file("debug/deps/b-0123"),
            file("debug/deps/libc-4567.rlib"),
            file("debug/build/index-89/out/index.rs"),
        );
        let log = Log {
            targets: vec![
                failed("-p a --test golden", &["shared_app"], &[]),
                failed("-p a --test b", &[], &[&unexecuted]),
                failed("-p a --test c", &[], &[]),
                failed("-p a --doc", &[], &[&rlib, &out]),
                failed("-p d --doc", &[], &[&rlib]),
                failed("-p e --doc", &[], &[&out]),
            ],
            went: Some(Went::While(
                "Running tests/a.rs (.target/debug/deps/a-ffff)".into(),
                Some("-p a --test a".into()),
            )),
            ..Log::default()
        };
        let said = said(&log).unwrap();
        let made = |name: &str| {
            let meta = fs::metadata(target.join(name)).unwrap();
            let when = meta.created().or_else(|_| meta.modified()).unwrap();
            local(when.duration_since(UNIX_EPOCH).unwrap().as_secs_f64())
        };
        assert_eq!(
            said,
            format!(
                "An environment failure, not a failed test: 4 of its failed targets found files \
                 its own build had made gone, and never ran or never compiled for that (`-p a \
                 --test b`, `-p a --doc`, `-p d --doc` and 1 more): `{unexecuted}`, `{rlib}` and \
                 `{out}`. They went while `Running tests/a.rs (.target/debug/deps/a-ffff)` (`-p a \
                 --test a`) ran, the last target cargo started before the first that found one \
                 gone: what removed them ran then, that target or something it started, or \
                 something outside the run. Its build folder, {}, holds now: `bin/` (made {}), \
                 `tree` (made {}), and no `debug/`.",
                target.display(),
                made("bin"),
                made("tree"),
            )
        );

        // Gone before any target ran, with no package to name, its folder kept: said so.
        tree.write("copy/.target/debug/deps/kept", "");
        let building = Log {
            went: Some(Went::Building),
            targets: vec![failed("-p a --doc", &[], &[&rlib])],
            ..Log::default()
        };
        let said = super::said(&building).unwrap();
        assert!(
            said.contains(
                "(`-p a --doc`): `{rlib}`. They went while it built, before any target ran:"
                    .replace("{rlib}", &rlib)
                    .as_str()
            ),
            "{said}"
        );
        assert!(said.ends_with(", its `debug/` among them."), "{said}");
        let unnamed = Log {
            went: Some(Went::While("Doc-tests a".into(), None)),
            ..building
        };
        assert!(
            super::said(&unnamed)
                .unwrap()
                .contains("They went while `Doc-tests a` ran, the last target"),
            "no arguments are guessed for a target whose package the log does not say"
        );
    }

    #[test]
    fn files_gone_while_lanes_ran_side_by_side_are_said_of_those_lanes_and_of_no_one_target() {
        let tree = Tree::new();
        let target = tree.path("t");
        let file = format!("{}/debug/deps/b-0123", target.display());
        let lanes: Vec<String> = ["perf", "sweep", "rest", "doc"].map(String::from).into();
        let log = |at_once: usize| Log {
            targets: vec![
                failed("-p a --test b", &[], &[&file]),
                failed("-p a --doc", &[], &[&file]),
            ],
            went: Some(Went::Lanes(lanes.clone(), at_once)),
            ..Log::default()
        };
        assert_eq!(
            said(&log(8)).unwrap(),
            format!(
                "An environment failure, not a failed test: 2 of its failed targets found files \
                 its own build had made gone, and never ran or never compiled for that (`-p a \
                 --test b` and `-p a --doc`): `{file}`. They went while the tests' 4 lanes ran \
                 side by side, all at once (`perf`, `sweep`, `rest` and `doc`): what removed them \
                 ran then, a test of any of those lanes or something it started (their outputs \
                 follow one another in the log, but they ran at the same time, so it names no one \
                 target), or something outside the run. Its build folder, {}, is gone.",
                target.display()
            )
        );
        for (at_once, said) in [
            (4, "4 lanes ran side by side, all at once (`perf`, "),
            (2, "4 lanes ran side by side, at most 2 at once (`perf`, "),
        ] {
            let told = super::said(&log(at_once)).unwrap();
            assert!(told.contains(said), "{at_once}: {told}");
        }
        // Every lane is named, however many.
        let many: Vec<String> = (1..=9).map(|n| format!("l{n}")).collect();
        let crowded = Log {
            went: Some(Went::Lanes(many, 8)),
            ..log(8)
        };
        let told = said(&crowded).unwrap();
        assert!(
            told.contains(
                "9 lanes ran side by side, at most 8 at once (`l1`, `l2`, `l3`, `l4`, \
                 `l5`, `l6`, `l7`, `l8` and `l9`)"
            ),
            "{told}"
        );
    }

    #[test]
    fn a_build_folder_gone_or_empty_or_crowded_is_said_as_it_is() {
        let tree = Tree::new();
        let target = tree.path("t");
        let profile = target.join("debug");
        assert_eq!(
            holds(&target, &profile),
            format!("Its build folder, {}, is gone.", target.display())
        );
        fs::create_dir_all(&target).unwrap();
        assert_eq!(
            holds(&target, &profile),
            format!(
                "Its build folder, {}, holds nothing now, and no `debug/`.",
                target.display()
            )
        );
        for n in 0..HELD_LISTED + 2 {
            tree.write(&format!("t/f{n:02}"), "");
        }
        let said = holds(&target, &profile);
        assert!(said.contains("`f07` (made "), "{said}");
        assert!(!said.contains("`f08`"), "{said}");
        assert!(said.ends_with(", 2 more, and no `debug/`."), "{said}");
    }
}
