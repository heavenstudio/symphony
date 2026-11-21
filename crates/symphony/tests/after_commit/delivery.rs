//! An owner's save in a project that delivers by pull request (`[land] delivery`): kept and
//! answered as pending, shown on its page, committed on a branch of its own and filed as the
//! owner's task, never committed to the default branch here, and dropped only once the remote
//! default branch and the one here hold it (`symphony::commit::pending`).
use super::*;
use symphony::commit::pending::{self, State};

const APPROVE: &str =
    r#"{"path":"docs/a.md","slug":"one","state":"approved","note":"yes","tap":"t"}"#;
const COMMENT: &str =
    r#"{"path":"docs/a.md","after_line":1,"kind":"comment","title":"T","body":"words"}"#;

/// How a note opens over one save the page draws.
const SHOWS_ONE: &str =
    "> **Pending delivery.** This page shows an owner's save that `main` does not hold yet";

fn payload(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

/// The fixture's repository delivering by pull request, with a remote that holds its `main`.
fn delivering(f: &Fixture) -> PathBuf {
    let root = f.repo();
    write(
        &root.join("symphony.toml"),
        "[land]\ndelivery = [\"symphony-github\", \"deliver\"]\n",
    );
    git(&root, &["add", "symphony.toml"]);
    git(&root, &["commit", "-qm", "deliver by pull request"]);
    let origin = f.home.join("origin.git");
    git(
        &f.home,
        &["init", "-q", "--bare", "-b", "main", "origin.git"],
    );
    git(
        &root,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    git(&root, &["push", "-q", "origin", "main"]);
    git(&root, &["fetch", "-q", "origin"]);
    root
}

/// The journal's entries, by their file names.
fn journal(root: &Path) -> Vec<String> {
    let folder = root.join(".git/symphony/server/verdict-publications");
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .map(|read| {
            read.map(|entry| entry.unwrap().file_name().into_string().unwrap())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// The land's receipt of a delivery of `slug`, kept in the lands' folder as the land keeps it.
fn receipt(root: &Path, slug: &str, head: &str, parent: &str, squash: &str) {
    use symphony_board::land::delivery::Receipt;
    let receipt = Receipt {
        slug: slug.into(),
        task: 1,
        operation: "task".into(),
        repository: 7,
        name: "host/project".into(),
        pull: 3,
        head: head.into(),
        base_observed: parent.into(),
        parent: parent.into(),
        squash: squash.into(),
        tree: git(root, &["rev-parse", &format!("{squash}^{{tree}}")]),
        revision: "r".into(),
        by: "lander".into(),
        import: None,
    };
    let path = Receipt::path(&root.join(".git/symphony/tasks/land"), slug);
    write(&path, &serde_json::to_string(&receipt).unwrap());
}

/// What a delivered land of `branch` leaves: the host's one commit on the remote's `main`, its
/// tree the merge of that tip and the branch, and the land's receipt of it under `slug`.
fn landed(root: &Path, slug: &str, branch: &str) -> String {
    let parent = git(root, &["rev-parse", "origin/main"]);
    let tree = git(root, &["merge-tree", "--write-tree", "origin/main", branch]);
    let squash = git(root, &["commit-tree", &tree, "-p", &parent, "-m", "squash"]);
    git(
        root,
        &["push", "-q", "origin", &format!("{squash}:refs/heads/main")],
    );
    git(root, &["fetch", "-q", "origin"]);
    let head = git(root, &["rev-parse", branch]);
    receipt(root, slug, &head, &parent, &squash);
    squash
}

/// One pass of a server's committer over the journal: a start, then its close.
fn pass(root: &Path) {
    let saves = ClosingSaves::start(root);
    saves.0.settle();
}

fn branches(root: &Path) -> Vec<String> {
    git(
        root,
        &[
            "branch",
            "--list",
            "--format=%(refname:short)",
            "docs/owner-save-*",
        ],
    )
    .lines()
    .map(str::to_owned)
    .collect()
}

pub(super) fn kept_pending_on_a_branch_and_filed_as_the_owners_task(f: &Fixture) {
    let root = delivering(f);
    let tip = git(&root, &["rev-parse", "main"]);
    // A commit on the default branch is refused whoever asks for it, and main does not move.
    let verdict = DOC.replace("[ ]", "[x]");
    let refused = commit_verdicts(
        &root,
        &[f.edit("docs/a.md", DOC, &verdict)],
        "owner verdict",
    );
    assert!(!refused.committed);
    assert_eq!(
        refused.error,
        "not committed: this project delivers by pull request ([land] delivery), so nothing is \
         committed to main here: a save waits for the pull request that delivers it"
    );
    assert_eq!(git(&root, &["rev-parse", "main"]), tip);
    // The fixture's edit wrote the file, as an edit handed to that entry is: put back.
    git(&root, &["checkout", "-q", "--", "docs/a.md"]);
    let saves = ClosingSaves::start(&root);
    // Answered as pending, never as committed, and nothing of the checkout written.
    let answer = saves.post("", "/api/decision", payload(APPROVE));
    assert_eq!(answer, json!({"ok":true,"delivery":"pending"}));
    assert_eq!(git(&root, &["status", "--porcelain"]), "");
    assert_eq!(
        std::fs::read_to_string(root.join("docs/a.md")).unwrap(),
        DOC
    );
    // The edit is on the disk before the answer: the journal holds the owner's words.
    let kept = journal(&root);
    assert_eq!(kept.len(), 1, "{kept:?}");
    // The same tap again, and the same answer after a reload, append nothing.
    let again = saves.post("", "/api/decision", payload(APPROVE));
    assert_eq!(again["repeat"], true, "{again}");
    drop(saves);
    let again = ClosingSaves::start(&root).post(
        "",
        "/api/decision",
        json!({"path":"docs/a.md","slug":"one","state":"approved","note":"yes","tap":"u"}),
    );
    assert_eq!(again["repeat"], true, "{again}");
    assert_eq!(journal(&root), kept);
    // The refusals a save always had still stand, and keep nothing.
    let refused = ClosingSaves::start(&root)
        .0
        .post(
            "",
            "/api/decision",
            &json!({"path":"docs/a.md","slug":"none","state":"approved","note":"","tap":"v"}),
        )
        .unwrap_err();
    assert!(refused.status >= 400, "{}", refused.error);
    assert_eq!(journal(&root), kept);

    // Main never moved; the branch holds the one edit on main's tip.
    assert_eq!(git(&root, &["rev-parse", "main"]), tip);
    let pending = pending::saves(&root);
    assert_eq!(pending.len(), 1, "{pending:?}");
    let branch = pending[0].branch.clone();
    assert_eq!(
        branch,
        format!("{}{}", pending::BRANCHES, pending[0].sequence)
    );
    assert_eq!(pending[0].state, State::Waiting);
    assert_eq!(pending[0].file, "docs/a.md");
    assert_eq!(git(&root, &["rev-parse", &format!("{branch}^")]), tip);
    assert_eq!(
        git(&root, &["diff", "--name-only", "main", &branch]),
        "docs/a.md"
    );
    let delivered = git(&root, &["show", &format!("{branch}:docs/a.md")]);
    assert!(delivered.contains("- [x] **one**"), "{delivered}");
    assert!(delivered.contains("owner · approved — yes"), "{delivered}");

    // The page shows it, with the note that says it waits; the file does not hold it.
    let shown = pending::shown(&root, "docs/a.md", DOC.into());
    assert_eq!(shown.trim_end(), delivered.trim_end());
    assert_eq!(pending::shown(&root, "docs/b.md", TWO.into()), TWO);
    let note = pending::note(&root, "docs/a.md").unwrap();
    assert!(
        note.contains(&format!("`{branch}` (waiting for its pull request)")),
        "{note}"
    );
    assert!(note.starts_with(SHOWS_ONE), "{note}");
    assert_eq!(pending::note(&root, "docs/b.md"), None);

    // The page as served: the note first, then the document as the save leaves it; the board's
    // count of what waits on the owner reads the same text.
    let render = symphony::render::Render::new(&root);
    let page = symphony::docview::document(&render, "docs/a.md", "", &[]).unwrap();
    assert_eq!(page.sections[0].key, pending::NOTE);
    assert!(
        page.sections[0].html.contains("Pending delivery."),
        "{}",
        page.sections[0].html
    );
    let html: String = (page.sections.iter().map(|s| s.html.as_str())).collect();
    assert!(html.contains("approved"), "{html}");
    let other = symphony::docview::document(&render, "docs/b.md", "", &[]).unwrap();
    assert!(other.sections.iter().all(|s| s.key != pending::NOTE));
    let docs = symphony::board::documents_shown(&root).unwrap();
    let one = docs.iter().find(|doc| doc.path == "docs/a.md").unwrap();
    assert_eq!(one.decisions[0].state, symphony::register::State::Approved);
    let plain = symphony::board::documents(&root).unwrap();
    let one = plain.iter().find(|doc| doc.path == "docs/a.md").unwrap();
    assert_eq!(one.decisions[0].state, symphony::register::State::Pending);

    // Filed once as the owner's task of that slug, in review, under its receipt.
    let store = f.store();
    let task = store.get(&branch).unwrap();
    assert_eq!(task.status, "review");
    assert_eq!(task.title, "Deliver the owner's save of docs/a.md");
    let branch_tip = git(&root, &["rev-parse", &branch]);
    let receipt = format!("document-save:{branch}:{branch_tip}");
    assert_eq!(
        store.owner_saved(&receipt).unwrap(),
        std::slice::from_ref(&branch)
    );
    let thread = store.thread(&branch).unwrap();
    assert!(thread[0].text.contains("owner"), "{}", thread[0].text);
    // A restart takes the entry up again and files, commits and moves nothing twice.
    pass(&root);
    assert_eq!(journal(&root), kept);
    assert_eq!(git(&root, &["rev-parse", &branch]), branch_tip);
    assert_eq!(branches(&root), std::slice::from_ref(&branch));
    assert_eq!(store.thread(&branch).unwrap().len(), thread.len());
    // Its text on the remote with no receipt of a delivery proves nothing: held, never dropped.
    git(
        &root,
        &["push", "-q", "origin", &format!("{branch}:refs/heads/main")],
    );
    git(&root, &["fetch", "-q", "origin"]);
    git(&root, &["merge", "-q", "--ff-only", "origin/main"]);
    pass(&root);
    let pending = pending::saves(&root);
    assert_eq!(journal(&root), kept);
    let State::Held(why) = &pending[0].state else {
        panic!("{pending:?}");
    };
    assert!(why.contains("no delivery receipt"), "{why}");
    assert_eq!(store.get(&branch).unwrap().status, "review");
    f.clean_workers();
}

pub(super) fn a_then_b_through_delivery_cancellation_and_restart(f: &Fixture) {
    let root = delivering(f);
    let saves = ClosingSaves::start(&root);
    saves.post("", "/api/decision", payload(APPROVE));
    // B, of the same document, is written against A and shown with it, and waits for A.
    assert_eq!(
        saves.post("", "/api/comment", payload(COMMENT)),
        json!({"ok":true,"delivery":"pending"})
    );
    // C, of another document, waits for neither.
    saves.post(
        "",
        "/api/decision",
        json!({"path":"docs/b.md","slug":"two","state":"rejected","note":"no","tap":"b"}),
    );
    drop(saves);
    let tip = git(&root, &["rev-parse", "main"]);
    let pending = pending::saves(&root);
    assert_eq!(pending.len(), 3, "{pending:?}");
    let [a, b, c] = [0, 1, 2].map(|n| pending[n].branch.clone());
    assert_eq!(
        pending.iter().map(|save| &save.state).collect::<Vec<_>>(),
        [&State::Waiting, &State::Queued, &State::Waiting]
    );
    assert_eq!(branches(&root), [a.clone(), c.clone()]);
    assert_eq!(git(&root, &["rev-parse", &format!("{c}^")]), tip);
    let both = pending::shown(&root, "docs/a.md", DOC.into());
    assert!(
        both.contains("approved — yes") && both.contains("words"),
        "{both}"
    );
    let store = f.store();
    assert!(store.get(&b).is_err(), "B has no task before its branch");

    // The host squashes A and the land keeps its receipt: proved, and still shown and kept
    // while the checkout has not followed. B still waits.
    let squash = landed(&root, &a, &a);
    pass(&root);
    let pending = pending::saves(&root);
    assert_eq!(
        pending.iter().map(|save| &save.state).collect::<Vec<_>>(),
        [&State::Following, &State::Queued, &State::Waiting]
    );
    assert_eq!(pending::shown(&root, "docs/a.md", DOC.into()), both);
    let note = pending::note(&root, "docs/a.md").unwrap();
    assert!(note.contains("2 owner's saves"), "{note}");
    assert!(note.contains("has not followed yet"), "{note}");
    // The checkout was never written, so it follows the remote by a fast-forward; main moves
    // on past it, too.
    assert_eq!(git(&root, &["status", "--porcelain"]), "");
    git(&root, &["merge", "-q", "--ff-only", "origin/main"]);
    write(&root.join("other"), "main moved on\n");
    git(&root, &["commit", "-qam", "another landing"]);
    git(&root, &["push", "-q", "origin", "main"]);
    git(&root, &["fetch", "-q", "origin"]);
    let moved = git(&root, &["rev-parse", "main"]);
    // A restart: A is dropped, and B is committed on the main that holds A, a head of its own.
    pass(&root);
    let pending = pending::saves(&root);
    assert_eq!(
        pending
            .iter()
            .map(|save| (&save.branch, &save.state))
            .collect::<Vec<_>>(),
        [(&b, &State::Waiting), (&c, &State::Waiting)]
    );
    assert_eq!(git(&root, &["rev-parse", &format!("{b}^")]), moved);
    assert!(git(&root, &["merge-base", "--is-ancestor", &squash, &b]).is_empty());
    assert_eq!(
        git(&root, &["diff", "--name-only", "main", &b]),
        "docs/a.md"
    );
    assert_eq!(
        git(&root, &["show", &format!("{b}:docs/a.md")]).trim_end(),
        both.trim_end()
    );
    assert_eq!(store.get(&b).unwrap().status, "review");

    // B's task is cancelled: its words are kept and shown, said as not delivered, and nothing
    // of it reaches main.
    store.move_to(&b, "cancelled", "owner").unwrap();
    pass(&root);
    let pending = pending::saves(&root);
    let State::Held(why) = &pending[0].state else {
        panic!("{pending:?}");
    };
    assert!(why.contains("was cancelled"), "{why}");
    let on_main = std::fs::read_to_string(root.join("docs/a.md")).unwrap();
    assert!(!on_main.contains("words"), "{on_main}");
    assert_eq!(pending::shown(&root, "docs/a.md", on_main.clone()), both);
    assert_eq!(git(&root, &["rev-parse", "main"]), moved);
    // A save written beside the cancelled words conflicts with main without them: refused as
    // every such save is, kept with why, and never committed anywhere.
    let saves = ClosingSaves::start(&root);
    let beside =
        json!({"path":"docs/a.md","after_line":3,"kind":"comment","title":"U","body":"beside"});
    assert_eq!(
        saves.post("", "/api/comment", beside),
        json!({"ok":true,"delivery":"pending"})
    );
    // One written elsewhere on the page waits for neither, and carries its own words alone.
    let end = pending::shown(&root, "docs/a.md", on_main.clone())
        .lines()
        .count();
    saves.post(
        "",
        "/api/comment",
        json!({"path":"docs/a.md","after_line":end,"kind":"comment","title":"V","body":"later"}),
    );
    drop(saves);
    let pending = pending::saves(&root);
    assert_eq!(pending.len(), 4, "{pending:?}");
    let State::Held(why) = &pending[2].state else {
        panic!("{pending:?}");
    };
    assert!(why.contains("conflicts with main"), "{why}");
    let d = pending[3].branch.clone();
    assert_eq!(pending[3].state, State::Waiting, "{pending:?}");
    // A branch of each save but the refused one; a delivered one's is the sweep's to remove.
    let mut made = [a.clone(), b.clone(), c.clone(), d.clone()];
    made.sort();
    assert_eq!(branches(&root), made);
    let later = git(&root, &["show", &format!("{d}:docs/a.md")]);
    assert!(later.contains("later"), "{later}");
    assert!(
        !later.contains("words") && !later.contains("beside"),
        "{later}"
    );
    let all = pending::shown(&root, "docs/a.md", on_main.clone());
    for kept in ["words", "beside", "later"] {
        assert!(all.contains(kept), "{all}");
    }
    assert_eq!(git(&root, &["rev-parse", "main"]), moved);

    // C is brought by another task's delivery, which carried its commit: its receipt is named
    // on C's own task, which stays open for its orchestrator, and C's entry goes.
    landed(&root, "tools/another", &c);
    git(&root, &["merge", "-q", "--ff-only", "origin/main"]);
    pass(&root);
    let left: Vec<String> = (pending::saves(&root).into_iter().map(|save| save.branch)).collect();
    assert_eq!(left, [b.clone(), pending[2].branch.clone(), d.clone()]);
    let thread = store.thread(&c).unwrap();
    let told: Vec<_> = (thread.iter())
        .filter(|post| post.text.contains("close it as covered by that landing"))
        .collect();
    assert_eq!(told.len(), 1, "{thread:?}");
    assert!(told[0].text.contains("tools/another"), "{}", told[0].text);
    assert_eq!(store.get(&c).unwrap().status, "review");
    // D is delivered by its own receipt, and a restart after it leaves the cancelled B alone.
    landed(&root, &d, &d);
    git(&root, &["merge", "-q", "--ff-only", "origin/main"]);
    pass(&root);
    pass(&root);
    let left: Vec<String> = (pending::saves(&root).into_iter().map(|save| save.branch)).collect();
    assert_eq!(left, [b, pending[2].branch.clone()]);
    assert_eq!(journal(&root).len(), 2);
    assert_eq!(git(&root, &["status", "--porcelain"]), "");
    f.clean_workers();
}

pub(super) fn journaled_before_the_key_and_no_longer_applying(f: &Fixture) {
    let root = delivering(f);
    let tip = git(&root, &["rev-parse", "main"]);
    let folder = root.join(".git/symphony/server/verdict-publications");
    std::fs::create_dir_all(&folder).unwrap();
    // As a build without the key journaled it: an edit and no word of delivery.
    let verdict = DOC.replace("[ ]", "[x]");
    write(
        &folder.join(format!("{:020}.json", 7)),
        &json!({"edit":{"rel":"docs/a.md","base":DOC,"new":verdict},"publication":null})
            .to_string(),
    );
    // And one written against a text main no longer has.
    let stale = TWO.replace("second proposal", "an older proposal");
    write(
        &folder.join(format!("{:020}.json", 8)),
        &json!({"edit":{"rel":"docs/b.md","base":stale,
            "new":stale.replace("[ ]", "[x]")},"publication":null,"delivery":{}})
        .to_string(),
    );
    pass(&root);
    assert_eq!(git(&root, &["rev-parse", "main"]), tip);
    assert_eq!(git(&root, &["status", "--porcelain"]), "");
    let pending = pending::saves(&root);
    assert_eq!(pending.len(), 2, "{pending:?}");
    assert_eq!(pending[0].branch, "docs/owner-save-7");
    assert_eq!(pending[0].state, State::Waiting);
    assert_eq!(
        git(&root, &["show", "docs/owner-save-7:docs/a.md"]),
        verdict.trim_end()
    );
    // Kept with why, its words still in the journal, and no branch or task made of it.
    let State::Held(why) = &pending[1].state else {
        panic!("{pending:?}");
    };
    assert!(why.contains("conflicts with main"), "{why}");
    assert_eq!(pending::shown(&root, "docs/b.md", TWO.into()), TWO);
    let note = pending::note(&root, "docs/b.md").unwrap();
    assert!(note.contains("kept, not delivered"), "{note}");
    assert!(!note.contains('\n'), "{note}");
    assert_eq!(journal(&root).len(), 2);
    assert_eq!(branches(&root), ["docs/owner-save-7"]);
    assert!(f.store().get("docs/owner-save-8").is_err());
    // A branch made and never journaled (the pass cut short between the two) is taken up, and
    // its task filed once.
    let made = git(&root, &["rev-parse", "docs/owner-save-7"]);
    let entry = folder.join(format!("{:020}.json", 7));
    write(
        &entry,
        &json!({"edit":{"rel":"docs/a.md","base":DOC,"new":verdict},"publication":null,
            "delivery":{}})
        .to_string(),
    );
    pass(&root);
    assert_eq!(git(&root, &["rev-parse", "docs/owner-save-7"]), made);
    assert_eq!(pending::saves(&root)[0].state, State::Waiting);
    let journaled: Value = serde_json::from_slice(&std::fs::read(&entry).unwrap()).unwrap();
    assert_eq!(journaled["delivery"]["tip"], made);
    assert_eq!(journaled["delivery"]["filed"], true);

    // Equal text an earlier delivery brought is some other owner action's. A save kept on a
    // main that held it already is covered by no such receipt, and is held for a person; only
    // a delivery newer than the save's base covers it.
    let third = "# C\n\n<!-- register:start -->\n- [ ] **three** — third proposal.\n<!-- register:end -->\n";
    write(&root.join("docs/c.md"), third);
    git(&root, &["add", "docs/c.md"]);
    git(&root, &["commit", "-qm", "a third page"]);
    let before = git(&root, &["rev-parse", "main"]);
    let answered = third.replace("[ ]", "[x]");
    write(&root.join("docs/c.md"), &answered);
    git(
        &root,
        &["commit", "-qam", "an earlier delivery of the same text"],
    );
    git(&root, &["push", "-q", "origin", "main"]);
    git(&root, &["fetch", "-q", "origin"]);
    let earlier = git(&root, &["rev-parse", "main"]);
    receipt(&root, "tools/earlier", &earlier, &before, &earlier);
    let entry = folder.join(format!("{:020}.json", 9));
    let kept = |base: &str| {
        json!({"edit":{"rel":"docs/c.md","base":third,"new":answered},"publication":null,
            "delivery":{"base":base}})
        .to_string()
    };
    for base in [earlier.as_str(), ""] {
        write(&entry, &kept(base));
        pass(&root);
        let pending = pending::saves(&root);
        let State::Held(why) = &pending[2].state else {
            panic!("{pending:?}");
        };
        assert!(why.contains("no delivery receipt"), "{base:?}: {why}");
    }
    write(&entry, &kept(&before));
    pass(&root);
    assert_eq!(pending::saves(&root).len(), 2);
    assert!(!entry.exists());
    f.clean_workers();
}

pub(super) fn a_settled_conflict_is_proved_by_its_receipt_alone(f: &Fixture) {
    let root = delivering(f);
    let saves = ClosingSaves::start(&root);
    saves.post("", "/api/comment", payload(COMMENT));
    let end = pending::shown(&root, "docs/a.md", DOC.into())
        .lines()
        .count();
    saves.post(
        "",
        "/api/comment",
        json!({"path":"docs/a.md","after_line":end,"kind":"comment","title":"V","body":"later"}),
    );
    drop(saves);
    let pending = pending::saves(&root);
    let [a, b] = [0, 1].map(|n| pending[n].branch.clone());
    assert_eq!(pending[0].state, State::Waiting);
    assert_eq!(pending[1].state, State::Queued);
    assert!(pending.iter().all(|save| save.shown), "{pending:?}");
    let tip = git(&root, &["rev-parse", &a]);

    // Main changes the line A's words were written beside: they no longer apply to the page
    // here, and the note says so by name where the page cannot draw them.
    let revised = DOC.replace("first paragraph", "first paragraph, revised");
    write(&root.join("docs/a.md"), &revised);
    git(&root, &["commit", "-qam", "another landing"]);
    git(&root, &["push", "-q", "origin", "main"]);
    git(&root, &["fetch", "-q", "origin"]);
    let moved = git(&root, &["rev-parse", "main"]);
    pass(&root);
    let pending = pending::saves(&root);
    assert_eq!(pending[0].state, State::Conflicted);
    assert!(!pending[0].shown && pending[1].shown, "{pending:?}");
    let page = pending::shown(&root, "docs/a.md", revised.clone());
    assert!(!page.contains("words") && page.contains("later"), "{page}");
    let note = pending::note(&root, "docs/a.md").unwrap();
    assert!(
        note.contains(&format!(
            "`{a}` (in conflict: its words no longer apply to `main` as it is here, so this \
             page cannot show them; its branch holds them, and needs the conflict settled \
             before its pull request delivers it)"
        )),
        "{note}"
    );
    // A page that cannot draw one of its saves keeps them rather than shows them.
    assert!(
        note.starts_with(
            "> **Pending delivery.** This page keeps 2 owner's saves that `main` does not hold yet"
        ),
        "{note}"
    );

    // A receipt proves nothing unless it is this save's delivery on the fetched remote.
    // One whose commit the remote does not hold:
    let tree = git(&root, &["rev-parse", &format!("{a}^{{tree}}")]);
    let unpushed = git(
        &root,
        &["commit-tree", &tree, "-p", &moved, "-m", "never pushed"],
    );
    receipt(&root, &a, &tip, &moved, &unpushed);
    pass(&root);
    assert_eq!(pending::saves(&root)[0].state, State::Conflicted);
    // One of a head that does not hold the save's commit, its squash on the remote:
    receipt(&root, &a, &moved, &moved, &moved);
    pass(&root);
    let pending = pending::saves(&root);
    assert_eq!(pending.len(), 2, "{pending:?}");
    assert_eq!(pending[0].state, State::Conflicted);
    assert_eq!(pending[1].state, State::Queued);
    assert_eq!(journal(&root).len(), 2);

    // The conflict is settled on the branch, as any task's is: the owner's words beside the
    // revised line, which the edit was never written against.
    git(&root, &["checkout", "-q", &a]);
    let conflict = Command::new("git")
        .args(["merge", "-q", "main"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(!conflict.status.success(), "the merge must conflict");
    let settled = revised.replace(
        "first paragraph",
        "> [!comment] T\n> words\n\nfirst paragraph",
    );
    write(&root.join("docs/a.md"), &settled);
    git(&root, &["commit", "-qam", "settle the conflict with main"]);
    git(&root, &["checkout", "-q", "main"]);
    // Its delivery, by its receipt: proved though the edit no longer merges onto the squash.
    let squash = landed(&root, &a, &a);
    pass(&root);
    let pending = pending::saves(&root);
    assert_eq!(pending[0].state, State::Following);
    assert_eq!(pending[1].state, State::Queued);
    git(&root, &["merge", "-q", "--ff-only", "origin/main"]);
    // The checkout follows: A is dropped and B, held back until now, gets its branch on it.
    pass(&root);
    let pending = pending::saves(&root);
    assert_eq!(pending.len(), 1, "{pending:?}");
    assert_eq!(
        (&pending[0].branch, &pending[0].state),
        (&b, &State::Waiting)
    );
    assert_eq!(git(&root, &["rev-parse", &format!("{b}^")]), squash);
    let delivered = git(&root, &["show", &format!("{b}:docs/a.md")]);
    for kept in ["words", "later", "first paragraph, revised"] {
        assert!(delivered.contains(kept), "{delivered}");
    }

    // B's reviewer approves it: done on the board, and not landed until its land, through the
    // whole wait for the host. It is still on its way, and a later save still waits for it.
    let store = f.store();
    let reviewer = store
        .register(&symphony_board::NewSession::new("worker"))
        .unwrap()
        .row
        .handle;
    store.take_role(&b, &reviewer, "reviewer").unwrap();
    let head = git(&root, &["rev-parse", &b]);
    let approved = store.verdict(&b, &reviewer, "approve", &head).unwrap();
    assert_eq!(approved.status, "done");
    let saves = ClosingSaves::start(&root);
    let top = json!({"path":"docs/a.md","after_line":1,"kind":"comment","title":"W","body":"top"});
    saves.post("", "/api/comment", top);
    drop(saves);
    pass(&root);
    let pending = pending::saves(&root);
    assert_eq!(pending.len(), 2, "{pending:?}");
    assert_eq!(pending[0].state, State::Waiting, "{pending:?}");
    assert_eq!(pending[1].state, State::Queued, "{pending:?}");
    // A landing closes it with no receipt that proves the save: held with why, never dropped,
    // and the later save waits for it no longer.
    let app = symphony::verdicts::app_handle(&store).unwrap();
    store
        .landed(&b, &app, "landed by hand", true, &["done"])
        .unwrap();
    pass(&root);
    let pending = pending::saves(&root);
    assert_eq!(pending.len(), 2, "{pending:?}");
    let State::Held(why) = &pending[0].state else {
        panic!("{pending:?}");
    };
    assert!(
        why.contains("was closed and no delivery receipt proves"),
        "{why}"
    );
    assert_eq!(pending[1].state, State::Waiting, "{pending:?}");
    assert_eq!(git(&root, &["rev-parse", "main"]), squash);
    assert_eq!(git(&root, &["status", "--porcelain"]), "");
    f.clean_workers();
}

/// The land's journal of a delivery of `slug` at `stage`, held with `held` when it names why,
/// kept in the lands' folder as the land keeps it.
fn journaled(root: &Path, slug: &str, stage: symphony_board::land::delivery::Stage, held: &str) {
    use symphony_board::land::delivery::Journal;
    let journal = Journal {
        slug: slug.into(),
        operation: "task".into(),
        stage,
        head: git(root, &["rev-parse", slug]),
        base_observed: git(root, &["rev-parse", "origin/main"]),
        revision: "r".into(),
        title: "docs: owner verdicts".into(),
        body: String::new(),
        by: "lander".into(),
        closes: vec!["done".into()],
        stood: String::new(),
        binding: None,
        squash: None,
        held: (!held.is_empty()).then(|| held.into()),
        import: None,
        green: None,
        armed: false,
    };
    journal
        .write(&root.join(".git/symphony/tasks/land"))
        .unwrap();
}

pub(super) fn the_land_of_a_pending_saves_task_is_said_from_its_journal(f: &Fixture) {
    use symphony_board::land::delivery::Stage;
    let root = delivering(f);
    let saves = ClosingSaves::start(&root);
    saves.post("", "/api/comment", payload(COMMENT));
    drop(saves);
    let pending = pending::saves(&root);
    let a = pending[0].branch.clone();
    assert_eq!(pending[0].state, State::Waiting, "{pending:?}");
    let state = |root: &Path| pending::saves(root)[0].state.clone();
    // A land run before it asks for the merge says nothing more: still waiting.
    journaled(&root, &a, Stage::Eligible, "");
    assert_eq!(state(&root), State::Waiting);
    // Once it asks the host for the merge, the note says so, and the page still draws the save.
    journaled(&root, &a, Stage::MergeRequested, "");
    assert_eq!(state(&root), State::Merging);
    let note = pending::note(&root, "docs/a.md").unwrap();
    assert!(note.starts_with(SHOWS_ONE), "{note}");
    assert!(
        note.contains(&format!(
            "`{a}` (its land has asked for the merge of its pull request)"
        )),
        "{note}"
    );
    // A delivery the land holds for a person is said with its why, at whatever stage.
    let closed = "pull request #3 was closed without a merge";
    for stage in [Stage::Eligible, Stage::MergeRequested] {
        journaled(&root, &a, stage, closed);
        assert_eq!(
            state(&root),
            State::Held(format!("its delivery is held for reconciliation: {closed}"))
        );
    }
    let note = pending::note(&root, "docs/a.md").unwrap();
    assert!(
        note.contains(&format!(
            "`{a}` (kept, not delivered: its delivery is held for reconciliation: {closed})"
        )),
        "{note}"
    );
    // Main changes the line its words were written beside: the page keeps them and cannot draw
    // them, which the note says beside the merge asked for.
    let revised = DOC.replace("first paragraph", "first paragraph, revised");
    write(&root.join("docs/a.md"), &revised);
    git(&root, &["commit", "-qam", "another landing"]);
    journaled(&root, &a, Stage::MergeRequested, "");
    let pending = pending::saves(&root);
    assert_eq!(pending[0].state, State::Merging);
    assert!(!pending[0].shown);
    let note = pending::note(&root, "docs/a.md").unwrap();
    assert!(
        note.starts_with(
            "> **Pending delivery.** This page keeps an owner's save that `main` does not hold yet"
        ),
        "{note}"
    );
    assert!(
        note.contains(
            "(its land has asked for the merge of its pull request; its words no longer apply \
             to this page as it is here, so they are not shown: they are kept)"
        ),
        "{note}"
    );
    // With no journal, it reads as the conflict it is.
    symphony_board::land::delivery::Journal::drop_in(&root.join(".git/symphony/tasks/land"), &a);
    assert_eq!(state(&root), State::Conflicted);
    f.clean_workers();
}
