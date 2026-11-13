//! What the project's generators make is the regenerator's, of main's own sources: a land keeps
//! main's side of every path inside their outputs that a branch changed, in a conflict or not, so
//! no branch's own build of them lands. The tip it lands stays the one approved. A conflict
//! anywhere else, a path merely named like the outputs included, refuses. What origin gained there
//! is main's own, kept but where it conflicts.

use super::*;

/// The project's generators and its counting check: `the stamp`, which writes nothing, then `the
/// bundle`, which `outputs` name, writing `gen/out.txt` from `src/`. On main with `src/a.txt` and
/// its bundle.
fn bundled(r: &Repo, outputs: &str) {
    let check = format!("echo ran >> '{}'", r.dir.join("runs").display());
    let more = format!(
        "\n[land]\ngenerators = [\n  {{ name = \"the stamp\", outputs = \"stamp/\", command = \
         [\"true\"] }},\n  {{ name = \"the bundle\", outputs = \"{outputs}\", command = [\"sh\", \
         \"-c\", \"mkdir -p gen && cat src/*.txt > gen/out.txt\"], when_changed = true }},\n]\n"
    );
    checked_with(r, &check, &more);
    write(&r.root, "src/a.txt", "a\n");
    write(&r.root, "gen/out.txt", "a\n");
    git(&r.root, &["add", "-A"]);
    git(&r.root, &["commit", "-qm", "the sources and their bundle"]);
}

/// A commit in `dir` writing each of `files` as its text.
fn committed(dir: &Path, files: &[(&str, &str)], message: &str) {
    for (file, text) in files {
        write(dir, file, text);
    }
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", message]);
}

#[test]
fn a_branch_with_its_own_stale_bundle_lands_with_mains_on_its_approved_tip() {
    let r = Repo::new();
    bundled(&r, "gen/");
    let bare = remote(&r);
    let wt = branch(&r, SLUG, "src/b.txt", "b\n");
    committed(&wt, &[("gen/out.txt", "a\nb\n")], "its bundle");
    let tip = head(&wt, "HEAD");
    let people = approved(&r, SLUG, &format!("{tip}: good"));
    // Main moved on, its sources and its bundle with them: the two bundles conflict.
    committed(
        &r.root,
        &[("src/c.txt", "c\n"), ("gen/out.txt", "a\nc\n")],
        "c on main",
    );
    let base = head(&r.root, "main");
    let (said, result) = landed(&r, SLUG, &people.orch);
    let line = result.unwrap();
    let merge = head(&r.root, "main");
    assert_eq!(
        line,
        format!(
            "landed {SLUG} on main at {}, its generated paths (gen/out.txt) taken from main, for \
             the regenerator to make; pushed to origin",
            &merge[..12]
        )
    );
    // One merge, of the tip as approved onto main, holding main's bundle and both sources.
    assert_eq!(head(&r.root, "main^1"), base);
    assert_eq!(head(&r.root, "main^2"), tip);
    assert_eq!(git(&r.root, &["show", "main:gen/out.txt"]), "a\nc");
    assert_eq!(git(&r.root, &["show", "main:src/b.txt"]), "b");
    assert_eq!(head(&bare, "main"), merge, "what was checked is pushed");
    assert_eq!(runs(&r).len(), 1);
    assert!(
        said.contains(&format!(
            "land {SLUG}: it conflicts in its generated paths (gen/out.txt): taking main's side \
             there, for the regenerator to make\n"
        )),
        "{said}"
    );
    // The thread said what the merge took, before main moved.
    assert!(
        r.board.said(SLUG).contains(&format!(
            "land {SLUG}: merged {tip} as {merge}, its generated paths (gen/out.txt) taken from \
             main, for the regenerator to make, checked at {merge}; moving main there"
        )),
        "{:?}",
        r.board.said(SLUG)
    );
    assert_eq!(last(&r, SLUG), line);
    assert_eq!(git(&r.root, &["status", "--porcelain"]), "");
}

#[test]
fn a_branchs_own_bundle_never_lands_even_with_no_conflict() {
    let r = Repo::new();
    bundled(&r, "gen/");
    // It rebuilt the bundle, and added a file main has none of.
    let wt = branch(&r, SLUG, "src/b.txt", "b\n");
    committed(
        &wt,
        &[("gen/out.txt", "a\nb\n"), ("gen/extra.txt", "x\n")],
        "its bundle",
    );
    let tip = head(&wt, "HEAD");
    let people = approved(&r, SLUG, "good");
    let base = head(&r.root, "main");
    let (said, result) = landed(&r, SLUG, &people.orch);
    let merge = head(&r.root, "main");
    assert_eq!(
        result.unwrap(),
        format!(
            "landed {SLUG} on main at {}, its generated paths (gen/extra.txt, gen/out.txt) taken \
             from main, for the regenerator to make; not pushed: no origin remote",
            &merge[..12]
        )
    );
    assert_eq!(head(&r.root, "main^1"), base);
    assert_eq!(head(&r.root, "main^2"), tip);
    assert_eq!(git(&r.root, &["show", "main:gen/out.txt"]), "a");
    assert!(!r.root.join("gen/extra.txt").exists(), "main had none");
    assert_eq!(
        git(
            &r.root,
            &["ls-tree", "-r", "--name-only", "main", "--", "gen"]
        ),
        "gen/out.txt"
    );
    assert_eq!(git(&r.root, &["show", "main:src/b.txt"]), "b");
    assert!(
        said.contains(&format!(
            "land {SLUG}: it changed its generated paths (gen/extra.txt, gen/out.txt): taking \
             main's side there, for the regenerator to make\n"
        )),
        "{said}"
    );
}

#[test]
fn many_generated_paths_are_named_by_their_generators_outputs() {
    let r = Repo::new();
    bundled(&r, "gen/");
    let wt = branch(&r, SLUG, "src/b.txt", "b\n");
    committed(
        &wt,
        &[
            ("gen/1.txt", "1\n"),
            ("gen/2.txt", "2\n"),
            ("gen/3.txt", "3\n"),
            ("gen/4.txt", "4\n"),
        ],
        "its bundle",
    );
    let people = approved(&r, SLUG, "good");
    let line = landed(&r, SLUG, &people.orch).1.unwrap();
    assert!(
        line.contains(", its generated paths (gen/) taken from main, for the regenerator to make;"),
        "{line}"
    );
    assert_eq!(
        git(
            &r.root,
            &["ls-tree", "-r", "--name-only", "main", "--", "gen"]
        ),
        "gen/out.txt"
    );
}

#[test]
fn a_branch_conflicting_in_its_bundle_and_in_a_source_refuses_to_its_executor() {
    let r = Repo::new();
    bundled(&r, "gen/");
    let wt = branch(&r, SLUG, "src/a.txt", "a1\n");
    committed(&wt, &[("gen/out.txt", "a1\n")], "its bundle");
    let people = approved(&r, SLUG, "good");
    committed(
        &r.root,
        &[("src/a.txt", "a2\n"), ("gen/out.txt", "a2\n")],
        "a2 on main",
    );
    let before = head(&r.root, "main");
    let (_, result) = landed(&r, SLUG, &people.orch);
    let line = format!(
        "{SLUG} conflicts with main at {}: gen/out.txt, src/a.txt",
        &before[..7]
    );
    assert_refused(&r, &result, &line, &before);
    assert!(runs(&r).is_empty(), "nothing checked");
    assert!(
        last(&r, SLUG).starts_with(&format!("land refused: {line}. Merge main into {SLUG}")),
        "{}",
        last(&r, SLUG)
    );
}

#[test]
fn a_path_only_named_like_the_generators_outputs_is_no_generated_path() {
    let r = Repo::new();
    // `gen`, with no slash, holds `gen/out.txt`, never `gen-old/out.txt`.
    bundled(&r, "gen");
    committed(&r.root, &[("gen-old/out.txt", "old\n")], "an old bundle");
    let wt = branch(&r, SLUG, "gen-old/out.txt", "old, the branch's\n");
    let people = approved(&r, SLUG, "good");
    committed(&r.root, &[("gen-old/out.txt", "old, main's\n")], "main's");
    let before = head(&r.root, "main");
    let (_, result) = landed(&r, SLUG, &people.orch);
    assert_refused(
        &r,
        &result,
        &format!(
            "{SLUG} conflicts with main at {}: gen-old/out.txt",
            &before[..7]
        ),
        &before,
    );
    assert_eq!(
        fs::read_to_string(wt.join("gen-old/out.txt")).unwrap(),
        "old, the branch's\n"
    );
}

#[test]
fn what_origin_gained_in_generated_paths_is_kept_but_where_it_conflicts() {
    let r = Repo::new();
    bundled(&r, "gen/");
    let bare = remote(&r);
    branch(&r, SLUG, "src/b.txt", "b\n");
    let people = approved(&r, SLUG, "good");
    // A regenerator elsewhere pushed a bundle of its own, and a file of it main has none of.
    pushed_elsewhere(&r, &bare, "gen/other.txt", "elsewhere\n");
    let line = landed(&r, SLUG, &people.orch).1.unwrap();
    assert!(line.ends_with("; pushed to origin"), "{line}");
    assert!(!line.contains("generated paths"), "{line}");
    assert_eq!(
        git(&r.root, &["show", "main:gen/other.txt"]),
        "elsewhere",
        "origin's own generated file is main's"
    );
    assert_eq!(head(&bare, "main"), head(&r.root, "main"));
}

#[test]
fn what_origin_gained_conflicting_in_generated_paths_takes_mains_side_there() {
    let r = Repo::new();
    bundled(&r, "gen/");
    let bare = remote(&r);
    branch(&r, SLUG, "src/b.txt", "b\n");
    let people = approved(&r, SLUG, "good");
    pushed_elsewhere(&r, &bare, "gen/out.txt", "elsewhere\n");
    committed(&r.root, &[("gen/out.txt", "a, main's\n")], "main's bundle");
    let (said, result) = landed(&r, SLUG, &people.orch);
    let line = result.unwrap();
    assert!(line.ends_with("; pushed to origin"), "{line}");
    assert_eq!(git(&r.root, &["show", "main:gen/out.txt"]), "a, main's");
    assert_eq!(
        git(&r.root, &["log", "-1", "--format=%s", "main"]),
        "Merge remote-tracking branch 'origin/main'"
    );
    assert!(
        said.contains(&format!(
            "land {SLUG}: what origin/main gained conflicts in generated paths (gen/out.txt): \
             taking main's side there, for the regenerator to make\n"
        )),
        "{said}"
    );
    assert_eq!(head(&bare, "main"), head(&r.root, "main"));
}
