use super::*;
use crate::test_tree::Tree;
use std::os::unix::fs::symlink;
use std::time::Instant;

const NOW: f64 = 1_800_000_000.0;
const DAY: f64 = 86_400.0;

fn git_ok(dir: &Path, args: &[&str]) -> String {
    git(dir, args).unwrap_or_else(|e| panic!("{e}"))
}

fn soon() -> Bound<'static> {
    Bound::until(Instant::now() + std::time::Duration::from_secs(120))
}

/// A repository at `main` in `tree` with one commit: its main checkout, common dir and commit.
fn repo(tree: &Tree) -> (PathBuf, PathBuf, String) {
    let main = tree.path("main");
    fs::create_dir_all(&main).unwrap();
    let main = main.canonicalize().unwrap();
    git_ok(&main, &["init", "-q", "-b", "main"]);
    fs::write(main.join("a.txt"), "one\n").unwrap();
    git_ok(&main, &["add", "a.txt"]);
    git_ok(
        &main,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "commit",
            "-q",
            "-m",
            "one",
        ],
    );
    let head = git_ok(&main, &["rev-parse", "HEAD"]);
    let common = main.join(".git");
    (main, common, head)
}

#[test]
fn a_path_is_folded_as_written() {
    assert_eq!(normal(Path::new("/a/b/../c/./d")), PathBuf::from("/a/c/d"));
    assert_eq!(
        normal(Path::new("/w/symphony/../symphony-wt/nightly")),
        PathBuf::from("/w/symphony-wt/nightly")
    );
}

#[test]
fn a_real_path_resolves_the_links_of_its_existing_part() {
    let tree = Tree::new();
    let root = tree.root.canonicalize().unwrap();
    fs::create_dir_all(root.join("target/inside")).unwrap();
    symlink(root.join("target"), root.join("link")).unwrap();
    assert_eq!(
        real(&root.join("link/inside/not/yet")),
        root.join("target/inside/not/yet")
    );
    assert_eq!(real(&root.join("link/../target")), root.join("target"));
    symlink(root.join("nowhere"), root.join("dangling")).unwrap();
    assert_eq!(
        real(&root.join("dangling/x")),
        root.join("dangling/x"),
        "a link to nothing stays as written"
    );
}

#[test]
fn the_worktree_listing_reads_paths_and_lock_reasons() {
    let tree = Tree::new();
    let root = tree.root.canonicalize().unwrap();
    let listing = format!(
        "worktree {r}/main\0HEAD abc\0branch refs/heads/main\0\0\
         worktree {r}/wt/nightly\0HEAD def\0detached\0locked {LOCK_REASON}\0\0\
         worktree {r}/wt/agent\0HEAD 123\0branch refs/heads/x\0locked\0prunable gone\0\0",
        r = root.display()
    );
    assert_eq!(
        listed(&listing),
        [
            Listed {
                path: root.join("main"),
                locked: None
            },
            Listed {
                path: root.join("wt/nightly"),
                locked: Some(LOCK_REASON.into())
            },
            Listed {
                path: root.join("wt/agent"),
                locked: Some(String::new())
            },
        ]
    );
    assert!(listed("").is_empty());
}

#[test]
fn a_folder_crossing_a_checkout_is_named() {
    let others = [PathBuf::from("/w/main"), PathBuf::from("/w/wt/agent")];
    assert_eq!(crossed(Path::new("/w/wt/nightly"), &others), None);
    assert_eq!(
        crossed(Path::new("/w/wt/agent"), &others).unwrap(),
        "/w/wt/agent is the worktree /w/wt/agent"
    );
    assert_eq!(
        crossed(Path::new("/w/main/inside"), &others).unwrap(),
        "/w/main/inside is inside the checkout /w/main"
    );
    assert_eq!(
        crossed(Path::new("/w/wt"), &others).unwrap(),
        "/w/wt holds the checkout /w/wt/agent"
    );
    assert_eq!(crossed(Path::new("/w/main-wt/nightly"), &others), None);
}

#[test]
fn the_nightly_makes_its_checkout_marks_it_and_takes_it_back_later() {
    let tree = Tree::new();
    let (main, common, head) = repo(&tree);
    let asked = main.join("../wt/nightly");
    let mut first = place(&main, &main, &common, &asked).unwrap();
    assert_eq!(first.checkout, main.parent().unwrap().join("wt/nightly"));
    assert_eq!(
        first.copies,
        main.parent().unwrap().join("wt/nightly-conciliate")
    );
    assert!(first.admin.is_none(), "not made yet");
    let noted = first.checkout(&main, &common, &head, soon(), NOW).unwrap();
    assert!(noted.is_empty(), "{noted:?}");
    let admin = first.admin.clone().unwrap();
    assert_eq!(
        fs::read_to_string(admin.join(MARK)).unwrap(),
        format!("{}\n", first.checkout.display())
    );
    assert_eq!(
        fs::read_to_string(admin.join(BUILT)).unwrap(),
        format!("{NOW}\n")
    );
    assert_eq!(
        fs::read_to_string(first.checkout.join("a.txt")).unwrap(),
        "one\n"
    );
    let listed = git_ok(&main, &["worktree", "list", "--porcelain"]);
    assert!(
        listed.contains(&format!(
            "worktree {}\nHEAD {head}\ndetached\nlocked {LOCK_REASON}",
            first.checkout.display()
        )),
        "{listed}"
    );

    // A later run takes it back, and resets it.
    fs::write(first.checkout.join("a.txt"), "changed\n").unwrap();
    fs::write(first.checkout.join("stray.txt"), "left\n").unwrap();
    fs::create_dir_all(first.checkout.join(".target")).unwrap();
    fs::write(first.checkout.join(".target/kept"), "build\n").unwrap();
    let mut again = place(&main, &main, &common, &asked).unwrap();
    assert_eq!(again.admin.as_ref(), Some(&admin));
    again
        .checkout(&main, &common, &head, soon(), NOW + DAY)
        .unwrap();
    assert_eq!(
        fs::read_to_string(again.checkout.join("a.txt")).unwrap(),
        "one\n"
    );
    assert!(!again.checkout.join("stray.txt").exists());
    assert!(again.checkout.join(".target/kept").exists());

    // Its folder gone, its registration is still the nightly's: made again in its place.
    fs::remove_dir_all(&again.checkout).unwrap();
    let mut made = place(&main, &main, &common, &asked).unwrap();
    assert!(made.admin.is_some());
    made.checkout(&main, &common, &head, soon(), NOW + DAY)
        .unwrap();
    assert_eq!(
        fs::read_to_string(made.checkout.join("a.txt")).unwrap(),
        "one\n"
    );
}

#[test]
fn a_build_folder_a_week_old_is_cleared_before_the_next_build() {
    let tree = Tree::new();
    let target = tree.path(".target");
    let stamp = tree.path("built");
    fs::create_dir_all(target.join("debug")).unwrap();
    assert_eq!(
        prune_build(&target, &stamp, NOW),
        None,
        "first seen: stamped"
    );
    assert_eq!(fs::read_to_string(&stamp).unwrap(), format!("{NOW}\n"));
    assert_eq!(prune_build(&target, &stamp, NOW + 6.9 * DAY), None);
    assert!(target.join("debug").exists());
    assert_eq!(
        prune_build(&target, &stamp, NOW + 7.0 * DAY),
        Some(format!(
            "cleared {}, built on for 7 days or more",
            target.display()
        ))
    );
    assert!(!target.exists());
    assert_eq!(
        fs::read_to_string(&stamp).unwrap(),
        format!("{}\n", NOW + 7.0 * DAY)
    );
    assert_eq!(
        prune_build(&target, &stamp, NOW + 15.0 * DAY),
        None,
        "nothing to clear"
    );

    // A build folder that is a link goes as a link: what it points at stays.
    let elsewhere = tree.path("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    fs::write(elsewhere.join("kept"), "kept").unwrap();
    symlink(&elsewhere, &target).unwrap();
    assert!(prune_build(&target, &stamp, NOW + 30.0 * DAY).is_some());
    assert!(fs::symlink_metadata(&target).is_err());
    assert!(elsewhere.join("kept").exists());
}

#[test]
fn a_git_lock_no_process_holds_is_removed_and_one_held_stops_the_run() {
    let tree = Tree::new();
    let admin = tree.path("admin");
    fs::create_dir_all(&admin).unwrap();
    assert_eq!(stale_locks(&admin), Ok(Vec::new()));
    let lock = tree.write("admin/index.lock", "");
    assert_eq!(
        stale_locks(&admin),
        Ok(vec![format!(
            "removed {}, which an interrupted git left and no process holds",
            lock.display()
        )])
    );
    assert!(!lock.exists());
    let lock = tree.write("admin/HEAD.lock", "");
    let mut holder = crate::children::command("sleep")
        .arg("30")
        .stdin(fs::File::open(&lock).unwrap())
        .spawn()
        .unwrap();
    let held = stale_locks(&admin).unwrap_err();
    holder.kill().unwrap();
    holder.wait().unwrap();
    assert_eq!(
        held,
        format!(
            "{} is held by process {}: a git runs in the nightly's checkout",
            lock.display(),
            holder.id()
        )
    );
    assert!(lock.exists(), "a held lock stays");
}

#[test]
fn nothing_but_the_nightlys_own_checkout_is_taken_and_what_is_refused_stays() {
    let tree = Tree::new();
    let (main, common, head) = repo(&tree);
    let wt = main.parent().unwrap().join("wt");
    // An agent's worktree with work of its own.
    let agent = wt.join("agent");
    git_ok(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "agent",
            &agent.to_string_lossy(),
        ],
    );
    fs::write(agent.join("precious.txt"), "uncommitted\n").unwrap();
    let refuse = |asked: &Path| place(&main, &main, &common, asked).unwrap_err();

    // A link in the checkout's place, to that worktree.
    symlink(&agent, wt.join("nightly")).unwrap();
    let said = refuse(&wt.join("nightly"));
    assert!(said.contains("is a link"), "{said}");
    fs::remove_file(wt.join("nightly")).unwrap();

    // The worktree named outright, a folder holding it, one inside the main checkout.
    assert!(refuse(&agent).contains("(a worktree the nightly did not lock)"));
    assert!(refuse(&wt).contains("holds the checkout"));
    assert!(refuse(&main.join("inside")).contains("is inside the checkout"));
    // Or through a link above it.
    symlink(&wt, main.parent().unwrap().join("wt-link")).unwrap();
    assert!(
        refuse(&main.parent().unwrap().join("wt-link/agent"))
            .contains("(a worktree the nightly did not lock)")
    );
    // The copies' folder taken by another worktree.
    git_ok(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            &wt.join("taken-conciliate").to_string_lossy(),
        ],
    );
    assert!(refuse(&wt.join("taken")).contains("taken-conciliate is the worktree"));

    // A link in the copies' place.
    let other = wt.join("other");
    symlink(&agent, wt.join("other-conciliate")).unwrap();
    assert!(refuse(&other).contains("other-conciliate is a link"));

    // A folder that is no worktree, and a worktree locked by someone else, or by the
    // nightly's reason but never marked.
    fs::create_dir_all(wt.join("plain")).unwrap();
    assert!(refuse(&wt.join("plain")).contains("(not a worktree of this repository)"));
    let locked = wt.join("locked");
    git_ok(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            "--lock",
            &locked.to_string_lossy(),
        ],
    );
    assert!(refuse(&locked).contains("(a worktree the nightly did not lock)"));
    let unmarked = wt.join("unmarked");
    git_ok(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            "--lock",
            "--reason",
            LOCK_REASON,
            &unmarked.to_string_lossy(),
        ],
    );
    assert!(refuse(&unmarked).contains("(a worktree without the nightly's marker)"));
    // The nightly's own, whose .git was pointed elsewhere.
    let nightly = wt.join("nightly");
    let mut own = place(&main, &main, &common, &nightly).unwrap();
    own.checkout(&main, &common, &head, soon(), NOW).unwrap();
    let pointed = fs::read_to_string(nightly.join(".git")).unwrap();
    fs::write(
        nightly.join(".git"),
        format!("gitdir: {}\n", agent.join(".git").display()),
    )
    .unwrap();
    assert!(refuse(&nightly).contains("(its .git is not the worktree's)"));
    fs::write(nightly.join(".git"), pointed).unwrap();
    assert!(place(&main, &main, &common, &nightly).is_ok());

    // Nothing refused was touched.
    assert_eq!(
        fs::read_to_string(agent.join("precious.txt")).unwrap(),
        "uncommitted\n"
    );
    assert_eq!(
        git_ok(&agent, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "agent"
    );
    assert_eq!(
        git_ok(&agent, &["status", "--porcelain"]),
        "?? precious.txt"
    );
}
