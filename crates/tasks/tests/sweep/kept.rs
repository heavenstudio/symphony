//! What the main checkout keeps of removed worktrees, and its cap (the owner, 2026-11-12: "images
//! should be kept, build should not … there should be a cap to the size of the folder, after which
//! it should ask/warn agents to manually check and remove stale images and artifacts not in use.
//! Maybe 5 GB limit?"): a worktree's artifacts are kept and its build output is not; the sweep
//! never removes anything from what it keeps; under the cap it says nothing of it (a dry run says
//! what it holds); past it, each sweep says so in one line, and `tasks status` warns of it once,
//! standing, naming the largest folders with their tasks and stages, until a sweep measures it under.
//! Each of those is in use while something still references it (the owner, the same day: "if
//! there's something that still actively references the artifact, then it should be kept until
//! space used becomes a problems requiring it to be remove"): an open task tied to it, a post on an
//! open task's thread or a document on main naming a path under it; only the rest are suggested.

use crate::common::{RawWrite as _, Seed};
use crate::{Repo, git, text, write};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use symphony_board::sweep::kept::{self, Record};
use symphony_board::sweep::{self, size};

/// The project file on main naming the cap, in gigabytes.
fn capped(r: &Repo, cap: &str) {
    write(
        &r.root,
        "symphony.toml",
        &format!("[board]\nswept_cap_gb = {cap}\n"),
    );
    git(&r.root, &["add", "symphony.toml"]);
    git(&r.root, &["commit", "-qm", "the cap"]);
}

/// The main checkout's folder of what the sweep set aside.
fn swept(r: &Repo) -> PathBuf {
    sweep::swept(&r.root, "artifacts")
}

/// The sweep's record of it.
fn record(r: &Repo) -> Record {
    Record::read(&kept::path(&r.root.join(".git")))
}

/// What `tasks status` warns of what is set aside, run from the main checkout.
fn warnings(r: &Repo) -> Vec<String> {
    let mut env = r.env(&r.root);
    let health = r.dir.join("health.json");
    env.vars
        .insert("TASKS_HEALTH".into(), health.to_string_lossy().into_owned());
    env.vars.insert("SF_LOW_DISK_GB".into(), "0".into());
    let args = ["status", "--json"].map(String::from);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = symphony_board::cli::main(&args, &env, &mut std::io::empty(), &mut out, &mut err);
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
    let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
    json["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|w| w.as_str())
        .filter(|w| w.starts_with("the artifacts set aside in "))
        .map(str::to_string)
        .collect()
}

/// The lines of `said` that say the folder set aside.
fn set_aside_lines(said: &str) -> Vec<&str> {
    said.lines()
        .filter(|l| l.starts_with("set aside in "))
        .collect()
}

#[test]
fn a_worktrees_artifacts_are_kept_and_its_build_output_is_not() {
    let r = Repo::with(Seed::default().task("sim/a", "done"));
    let wt = r.merged("a", "sim/a");
    write(&wt, "artifacts/sim-a/shot.png", "a picture\n");
    write(
        &wt,
        "artifacts/sim-a/target/CACHEDIR.TAG",
        "Signature: 8a477f597d28d172789f06886806bc55\n",
    );
    write(
        &wt,
        "artifacts/sim-a/target/debug/app",
        "\u{7f}ELF a program\n",
    );
    write(&wt, "artifacts/sim-a/proof/server", "\u{7f}ELF a server\n");
    write(&wt, "artifacts/sim-a/proof/receipt.json", "{}\n");
    let said = r.sweep();
    r.assert_removed(&wt, &said);
    let kept = swept(&r).join("a");
    assert_eq!(
        fs::read_to_string(kept.join("sim-a/shot.png")).unwrap(),
        "a picture\n"
    );
    assert!(kept.join("sim-a/proof/receipt.json").is_file());
    for build in ["sim-a/target", "sim-a/proof/server"] {
        assert!(!kept.join(build).exists(), "{build}: {said}");
    }
}

#[test]
fn under_its_cap_the_sweep_keeps_what_it_sets_aside_and_warns_of_nothing() {
    let r = Repo::with(Seed::default().task("sim/a", "done"));
    let wt = r.merged("a", "sim/a");
    write(&wt, "artifacts/sim-a/shot.png", "a picture\n");
    let said = r.sweep();
    r.assert_removed(&wt, &said);
    assert!(set_aside_lines(&said).is_empty(), "{said}");
    assert!(warnings(&r).is_empty());
    let record = record(&r);
    assert_eq!(
        record.from,
        BTreeMap::from([("a".to_string(), vec!["sim/a".to_string()])])
    );
    let measured = record.measured.unwrap();
    assert_eq!(
        (measured.cap_gb, measured.bytes, measured.over()),
        (kept::CAP_GB, size::bytes(&swept(&r)), false)
    );
}

#[test]
fn past_its_cap_each_sweep_says_so_status_warns_once_and_nothing_is_removed() {
    let r = Repo::with(
        Seed::default()
            .task("sim/a", "done")
            .task("sim/b", "done")
            .task("sim/c", "cancelled"),
    );
    // 50,000 bytes: the first worktree's picture stays under it, the second's goes past it.
    capped(&r, "0.00005");
    let a = r.merged("a", "sim/a");
    write(&a, "artifacts/sim-a/shot.png", &"a".repeat(2_000));
    let said = r.sweep();
    r.assert_removed(&a, &said);
    assert!(set_aside_lines(&said).is_empty(), "under its cap: {said}");
    assert!(warnings(&r).is_empty());

    let b = r.merged("b", "sim/b");
    write(&b, "artifacts/sim-b/shot.png", &"b".repeat(60_000));
    let done = r.sweep_with(|_| {});
    let said = text(&done);
    r.assert_removed(&b, &said);
    let held = size::bytes(&swept(&r));
    let line = set_aside_lines(&said);
    assert_eq!(
        line,
        [format!(
            "set aside in {}: {}, over its 0.00005 GB cap ([board] swept_cap_gb): the sweep never \
             removes any, so check by hand and remove the stale images and artifacts no longer in \
             use; the largest: b {} (task sim/b, done; no reference found), a {} (task sim/a, \
             done; no reference found); suggested, as nothing found references them: b, a",
            swept(&r).display(),
            size::shown(held),
            size::shown(size::bytes(&swept(&r).join("b"))),
            size::shown(size::bytes(&swept(&r).join("a"))),
        )
        .as_str()],
        "{said}"
    );
    let warned = warnings(&r);
    assert_eq!(warned.len(), 1, "{warned:?}");
    assert!(
        warned[0].starts_with(&format!(
            "the artifacts set aside in {} hold {}, over their 0.00005 GB cap ([board] \
             swept_cap_gb), as the sweep measured them ",
            swept(&r).display(),
            size::shown(held)
        )) && warned[0].ends_with(&format!(
            " ago, said until a sweep measures them again (`just tasks sweep`, or the one a task's \
             finish starts; a dry run records nothing){}",
            &line[0][line[0].find(": the sweep never").unwrap()..]
        )),
        "{warned:?}"
    );

    // A dry run says it too, and writes nothing.
    let before = record(&r);
    let dry = text(&r.sweep_with(|o| o.dry_run = true));
    assert_eq!(set_aside_lines(&dry).len(), 1, "{dry}");
    assert_eq!(record(&r), before);

    // The next pass, still past it, says it again in its one line; the warning stands, once.
    let c = r.merged("c", "sim/c");
    write(&c, "artifacts/sim-c/notes.txt", "notes\n");
    let said = r.sweep();
    r.assert_removed(&c, &said);
    assert_eq!(set_aside_lines(&said).len(), 1, "{said}");
    assert!(
        set_aside_lines(&said)[0].contains("(task sim/c, cancelled; no reference found)"),
        "{said}"
    );
    assert_eq!(warnings(&r).len(), 1);

    // Nothing the sweep set aside was ever removed.
    for kept in ["a/sim-a/shot.png", "b/sim-b/shot.png", "c/sim-c/notes.txt"] {
        assert!(swept(&r).join(kept).is_file(), "{kept}");
    }

    // Under its cap again (here raised; a check by hand removes instead), the warning goes.
    capped(&r, "5");
    let said = r.sweep();
    assert!(set_aside_lines(&said).is_empty(), "{said}");
    assert!(warnings(&r).is_empty());
    assert!(swept(&r).join("b/sim-b/shot.png").is_file());
}

#[test]
fn past_its_cap_what_still_references_a_folder_is_in_use_and_only_the_rest_is_suggested() {
    let r = Repo::with(
        Seed::default()
            .task("sim/a", "done")
            .task("sim/b", "done")
            .task("sim/c", "done")
            .task("sim/d", "done")
            .task("tools/open", "in_progress"),
    );
    capped(&r, "0.000001");
    let mut trees = Vec::new();
    for (name, bytes) in [("a", 40_000), ("b", 30_000), ("c", 20_000), ("d", 10_000)] {
        let wt = r.merged(name, &format!("sim/{name}"));
        write(
            &wt,
            &format!("artifacts/sim-{name}/shot.png"),
            &name.repeat(bytes),
        );
        trees.push(wt);
    }
    let store = &r.board.store;
    // An open task's thread names b's picture where its worktree held it.
    store
        .say(
            "tools/open",
            "owner",
            "the jump, before and after: artifacts/sim-b/shot.png.",
            None,
        )
        .unwrap();
    // A finished task's thread is not read, nor what the main checkout holds and main does not.
    store
        .say("sim/d", "owner", "see artifacts/sim-d/shot.png", None)
        .unwrap();
    write(
        &r.root,
        "docs/NOTES.md",
        "artifacts/swept/d/sim-d/shot.png\n",
    );
    // A card on main names c's, where the sweep keeps it.
    write(
        &r.root,
        "docs/DECISIONS.md",
        "![the jump](../artifacts/swept/c/sim-c/shot.png)\n",
    );
    git(&r.root, &["add", "docs/DECISIONS.md"]);
    git(&r.root, &["commit", "-qm", "a card"]);
    let said = r.sweep();
    for wt in &trees {
        r.assert_removed(wt, &said);
    }
    // a's task is open again.
    store
        .client()
        .write(
            "UPDATE type::record('task', $slug) SET status = 'in_progress'",
            serde_json::json!({"slug": "sim/a"}),
        )
        .unwrap();
    let said = r.sweep();
    let shown = |name: &str| size::shown(size::bytes(&swept(&r).join(name)));
    let ask = format!(
        ": the sweep never removes any, so check by hand and remove the stale images and artifacts \
         no longer in use; the largest: a {} (task sim/a, in_progress; in use: task sim/a is \
         open), b {} (task sim/b, done; in use: the thread of tools/open), c {} (task sim/c, done; \
         in use: docs/DECISIONS.md), d {} (task sim/d, done; no reference found); suggested, as \
         nothing found references them: d",
        shown("a"),
        shown("b"),
        shown("c"),
        shown("d"),
    );
    let line = set_aside_lines(&said);
    assert_eq!(line.len(), 1, "{said}");
    assert!(line[0].ends_with(&ask), "{}\n{ask}", line[0]);
    let warned = warnings(&r);
    assert_eq!(warned.len(), 1, "{warned:?}");
    assert!(warned[0].ends_with(&ask), "{warned:?}");
    let measured = record(&r).measured.unwrap();
    assert_eq!(
        measured
            .largest
            .iter()
            .map(|l| (l.name.as_str(), l.uses.len()))
            .collect::<Vec<_>>(),
        [("a", 1), ("b", 1), ("c", 1), ("d", 0)]
    );
    for name in ["a", "b", "c", "d"] {
        assert!(
            swept(&r)
                .join(format!("{name}/sim-{name}/shot.png"))
                .is_file(),
            "{name}: nothing is removed"
        );
    }
}

#[test]
fn a_folder_removed_by_hand_is_forgotten_and_a_cap_that_cannot_be_read_is_said() {
    let r = Repo::with(Seed::default().task("sim/a", "done"));
    write(
        &r.root,
        "symphony.toml",
        "[board]\nswept_cap_gb = \"lots\"\n",
    );
    git(&r.root, &["add", "symphony.toml"]);
    git(&r.root, &["commit", "-qm", "a cap that is no number"]);
    let wt = r.merged("a", "sim/a");
    write(&wt, "artifacts/sim-a/shot.png", "a picture\n");
    let said = r.sweep();
    r.assert_removed(&wt, &said);
    assert!(
        said.contains(&format!(
            "set aside: {}: [board] swept_cap_gb is \"lots\", not a number of gigabytes over 0; \
             its cap is 5 GB",
            r.root.join("symphony.toml").display()
        )),
        "{said}"
    );
    assert!(record(&r).from.contains_key("a"));
    // Removed by hand, as the check asks: the next sweep forgets where it came from.
    fs::remove_dir_all(swept(&r).join("a")).unwrap();
    r.sweep();
    assert!(record(&r).from.is_empty());
}
