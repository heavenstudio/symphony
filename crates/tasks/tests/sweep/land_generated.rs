//! A branch that committed what the project's generators make conflicts with main there whenever
//! main regenerated it too: the land takes main's side of those paths alone, the generators make
//! them again from the merge, and the check judges the result. The tip it lands stays the one
//! approved. A conflict anywhere else, a path merely named like the outputs included, refuses.

use super::*;

/// The project's generators and its check: `the stamp`, which writes nothing, then `the bundle`,
/// which `outputs` name and which runs only after another changed something, writing
/// `gen/out.txt` from `src/`; the check passes only on a bundle that is what a fresh generation
/// makes of the sources, as `just client-check` compares the client's distribution. On main with
/// `src/a.txt` and its bundle.
fn bundled(r: &Repo, outputs: &str) {
    let runs = r.dir.join("runs");
    let check = format!(
        "echo ran >> '{}'; cat src/*.txt | cmp -s - gen/out.txt",
        runs.display()
    );
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
fn a_branch_with_its_own_stale_bundle_lands_regenerated_from_the_merge_on_its_approved_tip() {
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
    let (merge, top) = (head(&r.root, "main~1"), head(&r.root, "main"));
    assert_eq!(
        line,
        format!(
            "landed {SLUG} on main at {}, its conflicts in generated paths (gen/out.txt) taken \
             from main and regenerated, the bundle regenerated in {}; pushed to origin",
            &merge[..12],
            &top[..7]
        )
    );
    // The merge is of the tip as approved, onto main; the bundle after it is a fresh one.
    assert_eq!(head(&r.root, "main~1^1"), base);
    assert_eq!(head(&r.root, "main~1^2"), tip);
    assert_eq!(
        git(&r.root, &["show", "main~1:gen/out.txt"]),
        "a\nc",
        "the merge took main's bundle"
    );
    assert_eq!(git(&r.root, &["show", "main:gen/out.txt"]), "a\nb\nc");
    assert_eq!(
        git(&r.root, &["log", "-1", "--format=%s", "main"]),
        format!("Regenerate the bundle after {SLUG}")
    );
    assert_eq!(head(&bare, "main"), top, "what was checked is pushed");
    assert_eq!(runs(&r).len(), 1);
    assert!(
        said.contains(&format!(
            "land {SLUG}: gen/out.txt conflicted, generated paths alone: taking main's side \
             there, to regenerate\n"
        )),
        "{said}"
    );
    // The thread said what the merge took, before main moved.
    assert!(
        r.board.said(SLUG).contains(&format!(
            "land {SLUG}: merged {tip} as {merge}, its conflicts in generated paths (gen/out.txt) \
             taken from main and regenerated, checked at {top}; moving main there"
        )),
        "{:?}",
        r.board.said(SLUG)
    );
    assert_eq!(last(&r, SLUG), line);
    assert_eq!(git(&r.root, &["status", "--porcelain"]), "");
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
