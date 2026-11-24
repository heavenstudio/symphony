//! The land by the stamps a task needs (docs/workflow/analysis/needed-stamps.md, "The land's check"), in
//! scratch repositories on a board of their own: each row of the design's worked examples lands
//! once what it needs stands at its tip; a waiver on another tip is refused, the defaults back; a
//! missing stamp's refusal names it and who gives it; a stray red run is cleared by the same
//! command run again, green; a task waiting on the owner alone is held blocked until the owner
//! approves its tip; and a task no assessment read lands as before stamps.

use super::*;
use symphony_board::cli::machine_work::recorded::{self, FOLDER_VAR, Phase, Record, Step};
use symphony_board::stamps::{Catalogue, Needs};

/// A task's people, with the session that assesses it: none of the others, nor its filer.
pub(super) struct Staff {
    pub(super) orch: String,
    pub(super) exe: String,
    pub(super) assessor: String,
}

/// `slug` filed by the owner, orchestrated, claimed and moved to review by its executor, its
/// branch writing `files`: the people and the tip.
pub(super) fn in_review(r: &Repo, slug: &str, files: &[(&str, &str)]) -> (Staff, String) {
    let wt = r.add(&slug.replace('/', "-"), Some(slug), "main");
    for (file, text) in files {
        write(&wt, file, text);
    }
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-qm", &format!("work on {slug}")]);
    (staffed(r, slug), head(&wt, "HEAD"))
}

/// `slug` filed by the owner, orchestrated, claimed and moved to review by its executor: its
/// people.
fn staffed(r: &Repo, slug: &str) -> Staff {
    let store = &r.board.store;
    let staff = Staff {
        orch: r.board.session("orchestrator"),
        exe: r.board.session("worker"),
        assessor: r.board.session("reviewer"),
    };
    r.task(slug);
    store.take_role(slug, &staff.orch, "orchestrator").unwrap();
    store.claim(slug, &staff.exe).unwrap();
    store.move_to(slug, "review", &staff.exe).unwrap();
    staff
}

/// The assessment `by` writes of `slug`: `stamps` at `tip`.
pub(super) fn assess(r: &Repo, slug: &str, by: &str, stamps: &[&str], tip: &str) {
    let needs = Needs {
        stamps: if stamps.is_empty() {
            vec!["none".into()]
        } else {
            stamps.iter().map(|s| s.to_string()).collect()
        },
        tip: Some(tip.into()),
        by: by.into(),
        why: "as the guidelines say".into(),
        import: None,
    };
    (r.board.store)
        .set_needs(slug, &Catalogue::symphony(), &needs)
        .unwrap();
}

/// The work folder the land reads the recorder's runs from: the test's own.
fn work(r: &Repo) -> PathBuf {
    r.dir.join("work")
}

/// How many runs this repository's recorder kept: the next one's number.
static RUNS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

/// A run of `label` for `slug` at `commit`, kept in the work folder as the recorder keeps one that
/// finished with `exit`, after every run kept before it; `refused` is the line its log said its
/// runner refused it with, when it did.
fn kept(
    r: &Repo,
    slug: &str,
    commit: &str,
    (label, phase): (&str, Phase),
    exit: i32,
    refused: Option<&str>,
) {
    let n = RUNS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let started = 1000.0 + f64::from(n);
    let record = Record {
        version: 1,
        operation_id: format!("{n:032x}"),
        parent_id: None,
        phase,
        label: label.into(),
        state: Step::Finished,
        handle: Some("amber".into()),
        task: Some(slug.into()),
        checkout: r.root.to_string_lossy().into_owned(),
        branch: Some(slug.into()),
        commit: Some(commit.into()),
        machine_id: None,
        host: "host".into(),
        pid: 10,
        child_pid: Some(11),
        queued_at: started,
        started_at: Some(started),
        ended_at: Some(started + 0.5),
        beat_at: started,
        exit_code: Some(exit),
        signal: None,
        reason: String::new(),
        run: refused.map(|line| {
            let said = serde_json::json!({"pipeline": "test", "trigger": "amber", "refused": line});
            serde_json::from_value(said).unwrap()
        }),
        reading: None,
    };
    recorded::keep(&work(r), &record).unwrap();
}

/// A run of `label` for `slug` at `commit` that finished with `exit`, a test or a build.
fn ran(r: &Repo, slug: &str, commit: &str, label: &str, phase: Phase, exit: i32) {
    kept(r, slug, commit, (label, phase), exit, None);
}

/// The task's tests green at `tip`.
pub(super) fn green(r: &Repo, slug: &str, tip: &str) {
    ran(r, slug, tip, "cargo test -p sim", Phase::Test, 0);
}

/// The land of `slug` as `actor` from the main checkout, the recorder's runs read from the test's
/// work folder: what it printed, and the line it ended on or the refusal.
pub(super) fn landed_by(r: &Repo, slug: &str, actor: &str) -> (String, Result<String, String>) {
    let mut env = r.env(&r.root);
    env.vars
        .insert(FOLDER_VAR.into(), work(r).to_string_lossy().into_owned());
    let mut out = Vec::new();
    let result = land(&r.board.store, &env, slug, actor, None, &mut out);
    (String::from_utf8(out).unwrap(), result.map_err(refusal))
}

/// `slug` landed at `tip`: main's merge brought it, the task is done, its thread ends on the line.
fn assert_landed(r: &Repo, slug: &str, tip: &str, result: Result<String, String>) {
    let line = result.unwrap();
    assert_eq!(head(&r.root, "main^2"), tip);
    assert_eq!(r.board.store.get(slug).unwrap().status, "done");
    assert_eq!(last(r, slug), line);
}

/// The thread's last post: its text and to whom.
pub(super) fn told(r: &Repo, slug: &str) -> (String, Option<String>) {
    let post = r.board.store.thread(slug).unwrap().pop().unwrap();
    (post.text, post.recipient)
}

/// The rows of the design's worked examples that need no reviewer: documents only and a hotfix
/// need nothing, landed from review with no verdict; a release red whose fix leaves its test
/// alone needs its tests green at the tip.
#[test]
fn a_task_needing_no_review_lands_from_review_once_its_stamps_stand_at_its_tip() {
    let r = Repo::new();
    manifest(&r, "");
    for (slug, files, stamps) in [
        (
            "docs/design",
            vec![("docs/design.md", "# A design\n")],
            vec![],
        ),
        (
            "sim/hotfix",
            vec![("sim/world.gd", "extends RefCounted # fixed\n")],
            vec![],
        ),
        (
            "tests/release-red-lint",
            vec![("sim/lint.gd", "extends Node\n")],
            vec!["tests"],
        ),
    ] {
        let (staff, tip) = in_review(&r, slug, &files);
        assess(&r, slug, &staff.assessor, &stamps, &tip);
        if !stamps.is_empty() {
            green(&r, slug, &tip);
        }
        let (said, result) = landed_by(&r, slug, &staff.orch);
        assert!(
            said.starts_with(&format!(
                "land {slug}: it needs {} at {} (its assessment by {})",
                if stamps.is_empty() { "none" } else { "tests" },
                &tip[..7],
                staff.assessor
            )),
            "{said}"
        );
        assert!(!said.contains("approved by"), "{said}");
        assert_landed(&r, slug, &tip, result);
        // No review round: the landing closed it from review.
        let verdicts: Vec<String> = (r.board.store.thread(slug).unwrap().into_iter())
            .filter(|post| post.kind == "verdict")
            .map(|post| post.text)
            .collect();
        assert_eq!(verdicts.len(), 1, "{verdicts:?}");
        assert!(verdicts[0].starts_with(&format!("landed {slug} on main at ")));
        // The landing's line names the whole tip its stamps stood on, as an approve names one:
        // the commit a release candidate moves to.
        let stood = format!(
            "; its needs ({}) stood at {tip}",
            if stamps.is_empty() { "none" } else { "tests" }
        );
        assert!(verdicts[0].contains(&stood), "{}", verdicts[0]);
        assert_eq!(
            symphony_board::land::needs::stood_at(&verdicts[0]),
            Some(tip.as_str())
        );
    }
}

/// A land of a task that needs no review, stopped after it moved main, run again: it finishes by
/// its record, nothing merged again, closes the task from review with no verdict, and its line
/// names the tip its stamps stood on.
#[test]
fn an_interrupted_land_of_a_task_needing_no_review_finishes_naming_the_tip_its_stamps_stood_on() {
    let r = Repo::new();
    manifest(&r, "");
    let slug = "docs/finish";
    let (staff, tip) = in_review(&r, slug, &[("docs/finish.md", "# Finished\n")]);
    assess(&r, slug, &staff.assessor, &[], &tip);
    // A land stopped after it moved main: its merge there, and its record of it.
    let message = format!("Merge branch '{slug}'");
    git(&r.root, &["merge", "-q", "--no-ff", "-m", &message, slug]);
    let merge = head(&r.root, "main");
    let folder = r.root.join(".git/symphony/tasks/land");
    fs::create_dir_all(&folder).unwrap();
    symphony_board::land::Record {
        slug: slug.into(),
        tip: tip.clone(),
        merge: merge.clone(),
        checked: merge.clone(),
        regenerated: Vec::new(),
    }
    .write(&folder)
    .unwrap();
    assert_eq!(r.board.store.get(slug).unwrap().status, "review");
    let (said, result) = landed_by(&r, slug, &staff.orch);
    let line = result.unwrap();
    assert_eq!(
        line,
        format!(
            "landed {slug} on main at {}; its needs (none) stood at {tip}, finishing a land \
             interrupted after its merge; not pushed: no origin remote",
            &merge[..12]
        )
    );
    assert!(
        said.contains(&format!(
            "land {slug}: its tip {} is on main already, by this land's merge {}",
            &tip[..7],
            &merge[..7]
        )),
        "{said}"
    );
    assert_eq!(head(&r.root, "main"), merge, "nothing merged again");
    assert_eq!(r.board.store.get(slug).unwrap().status, "done");
    assert_eq!(last(&r, slug), line);
    let verdicts = (r.board.store.thread(slug).unwrap().into_iter())
        .filter(|post| post.kind == "verdict")
        .count();
    assert_eq!(verdicts, 1, "the landing's line alone: no review round");
    assert_eq!(
        symphony_board::land::needs::stood_at(&line),
        Some(tip.as_str())
    );
}

/// The rows that need a reviewer: a red's fix changing its test, a board command reworded, a row
/// added to a page. Each lands on a fresh reviewer's approve of its tip once its other stamps
/// stand there, and is refused, named stamp by stamp, while one does not.
#[test]
fn a_task_needing_review_lands_on_its_approve_once_its_other_stamps_stand_at_its_tip() {
    let r = Repo::new();
    manifest(&r, "");
    for (slug, file, stamps) in [
        (
            "tests/release-red-check",
            "tests/jump.gd",
            &["tests", "review"][..],
        ),
        ("tools/refusal", "sim/refusal.gd", &["tests", "review"]),
        (
            "ui/home-queue",
            "ui/queue.gd",
            &["tests", "review", "pictures"],
        ),
    ] {
        let (staff, tip) = in_review(&r, slug, &[(file, "extends Node\n")]);
        assess(&r, slug, &staff.assessor, stamps, &tip);
        let rev = r.board.session("reviewer");
        let store = &r.board.store;
        store.take_role(slug, &rev, "reviewer").unwrap();
        store
            .verdict(slug, &rev, "approve", &format!("{tip}: read it"))
            .unwrap();
        // Approved, but nothing else stands at the tip: refused to the executor, who gives them.
        let before = head(&r.root, "main");
        let (_, result) = landed_by(&r, slug, &staff.orch);
        let line = result.unwrap_err();
        assert!(
            line.starts_with(&format!(
                "{slug} lacks at its tip {} what it needs: tests (No build or test recorded for \
                 it at {}",
                &tip[..7],
                &tip[..7]
            )),
            "{line}"
        );
        assert!(
            line.ends_with("which its executor gives at that tip"),
            "{line}"
        );
        assert_eq!(
            line.contains("pictures (No picture posted at"),
            stamps.contains(&"pictures"),
            "{line}"
        );
        assert_eq!(head(&r.root, "main"), before, "main stays where it was");
        let (text, to) = told(&r, slug);
        assert!(text.starts_with(&format!("land refused: {line}. Give them at ")));
        assert_eq!(to.as_deref(), Some("@executor"));
        assert_eq!(
            store.get(slug).unwrap().status,
            "done",
            "its approve stands"
        );
        // Given at the tip, it lands on that approve.
        green(&r, slug, &tip);
        if stamps.contains(&"pictures") {
            let shot = format!("{tip}: before and after, artifacts/{slug}/wide.png");
            store.say(slug, &staff.exe, &shot, None).unwrap();
        }
        let (said, result) = landed_by(&r, slug, &staff.orch);
        assert!(
            said.starts_with(&format!("land {slug}: approved by {rev} at ")),
            "{said}"
        );
        assert!(
            said.contains(&format!(
                "land {slug}: it needs {} at {} (its assessment by {}); tests: Green at",
                stamps.join(", "),
                &tip[..7],
                staff.assessor
            )),
            "{said}"
        );
        assert_landed(&r, slug, &tip, result);
    }
}

/// The last row: a page the owner asked to see before it lands needs the owner's approve of its
/// tip. Approved by its reviewer it is done, which no move takes to blocked: the land refuses,
/// saying it waits on the owner's approve and where it is given, and lands once it is.
#[test]
fn a_task_needing_the_owner_waits_on_the_owners_approve_of_its_tip() {
    let r = Repo::new();
    manifest(&r, "");
    let slug = "ui/new-page";
    let (staff, tip) = in_review(&r, slug, &[("ui/page.gd", "extends Node\n")]);
    assess(
        &r,
        slug,
        &staff.assessor,
        &["tests", "review", "pictures", "owner"],
        &tip,
    );
    let store = &r.board.store;
    let rev = r.board.session("reviewer");
    store.take_role(slug, &rev, "reviewer").unwrap();
    store
        .verdict(slug, &rev, "approve", &format!("{tip}: read it"))
        .unwrap();
    green(&r, slug, &tip);
    let shot = format!("{tip}: artifacts/{slug}/phone.png");
    store.say(slug, &staff.exe, &shot, None).unwrap();
    let before = head(&r.root, "main");
    let (_, result) = landed_by(&r, slug, &staff.orch);
    let at = &tip[..7];
    let line = format!(
        "{slug} lacks at its tip {at} what it needs: owner (Waits on the owner's approve of {at}), \
         which the owner gives on the task's page"
    );
    assert_eq!(result, Err(line.clone()));
    assert_eq!(head(&r.root, "main"), before, "main stays where it was");
    assert_eq!(
        told(&r, slug),
        (
            format!(
                "land refused: {line}. {slug} stays done, its approve standing, waiting on the \
                 owner's approve of {at}, which the owner gives on the task's page (its Approve \
                 {at}); land it again once it is given"
            ),
            Some("@orchestrator".into())
        )
    );
    assert_eq!(store.get(slug).unwrap().status, "done");
    // The owner approves another commit: not the tip, still refused.
    let main = head(&r.root, "main");
    store
        .say(slug, "owner", &format!("approve {main}"), None)
        .unwrap();
    assert!(landed_by(&r, slug, &staff.orch).1.is_err());
    // The owner's approve of the tip, as the page posts it, lands it.
    store
        .say(slug, "owner", &format!("approve {tip}"), None)
        .unwrap();
    let (said, result) = landed_by(&r, slug, &staff.orch);
    assert!(said.contains("owner: Approved by the owner at "), "{said}");
    assert_landed(&r, slug, &tip, result);
}

/// A person the board gives decide approves for the owner: her approve of the tip stands for the
/// `owner` stamp once the board gives her decide, read at the land, and not before.
#[test]
fn a_task_needing_the_owner_lands_on_the_approve_of_one_the_board_gives_decide() {
    use symphony_board::people::{Holder, Power};
    let r = Repo::new();
    manifest(&r, "");
    let slug = "ui/her-page";
    let (staff, tip) = in_review(&r, slug, &[("ui/page.gd", "extends Node\n")]);
    assess(&r, slug, &staff.assessor, &["tests", "owner"], &tip);
    green(&r, slug, &tip);
    let store = &r.board.store;
    store
        .add_person("ana", &[], &[Power::Speak], &staff.orch)
        .unwrap();
    // Her approve as her page posts it: the server's store writes for her.
    (r.board.other().for_person("ana"))
        .say(slug, "ana", &format!("approve {tip}"), None)
        .unwrap();
    let before = head(&r.root, "main");
    let (_, result) = landed_by(&r, slug, &staff.orch);
    let refused = result.unwrap_err();
    assert!(
        refused.contains(&format!(
            "owner (Waits on the owner's approve of {})",
            &tip[..7]
        )),
        "her approve counts for nothing while she holds no decide: {refused}"
    );
    assert_eq!(head(&r.root, "main"), before);
    store
        .grant(Holder::Person("ana"), &[Power::Decide], &staff.orch)
        .unwrap();
    let (said, result) = landed_by(&r, slug, &staff.orch);
    assert!(said.contains("owner: Approved by ana at "), "{said}");
    assert_landed(&r, slug, &tip, result);
}

/// A task needing no review whose one missing stamp is the owner's is held blocked, keeping its
/// slot, and lands from blocked once the owner approves its tip.
#[test]
fn a_task_waiting_on_the_owner_alone_is_held_blocked_and_lands_from_there() {
    let r = Repo::new();
    manifest(&r, "");
    let slug = "docs/look";
    let (staff, tip) = in_review(&r, slug, &[("docs/look.md", "# A look\n")]);
    assess(&r, slug, &staff.assessor, &["owner"], &tip);
    let (_, result) = landed_by(&r, slug, &staff.orch);
    assert!(result.is_err());
    let at = &tip[..7];
    let (text, to) = told(&r, slug);
    assert!(
        text.ends_with(&format!(
            ". {slug} is held blocked, waiting on the owner's approve of {at}, which the owner \
             gives on the task's page (its Approve {at}); land it again once it is given"
        )),
        "{text}"
    );
    assert_eq!(to.as_deref(), Some("@orchestrator"));
    let store = &r.board.store;
    let held = store.get(slug).unwrap();
    assert_eq!(held.status, "blocked");
    // The hold names what it waits on: the owner's approve stamp of the tip, from review.
    let record = symphony_board::waits_on::Blocked::of(&held).expect("the hold names its thing");
    assert_eq!(
        (record.on.as_str(), record.why, record.from.as_deref()),
        (
            "check:owner",
            Some(format!("the owner's approve of {at}")),
            Some("review")
        )
    );
    // And it reads as the owner's decision, named by his approve of the tip, never by him.
    use symphony_board::waits_on::{DECISION, Landing, Look, line};
    let board = [held.clone()];
    let look = Look::new(&board, &[]);
    let waits = line(&held, &look, Landing::of(&held, None).as_ref()).unwrap();
    assert_eq!(
        (waits.kind, waits.who, waits.why.as_str()),
        (
            DECISION,
            symphony_board::waits_on::OWNER_APPROVE.to_string(),
            format!("the owner's approve of {at}").as_str()
        )
    );
    // Landed again before the owner gives it: still waiting, still blocked.
    assert!(landed_by(&r, slug, &staff.orch).1.is_err());
    let (text, _) = told(&r, slug);
    assert!(
        text.contains(&format!(". {slug} stays blocked, waiting on")),
        "{text}"
    );
    store
        .say(slug, "owner", &format!("approve {tip}"), None)
        .unwrap();
    let (_, result) = landed_by(&r, slug, &staff.orch);
    assert_landed(&r, slug, &tip, result);
    // Landed from blocked: the record of what it waited on goes with the block.
    let closed = store.get(slug).unwrap();
    assert!(
        (closed.fields.get(symphony_board::red::FIELDS))
            .is_none_or(|board| !board.contains_key(symphony_board::waits_on::BLOCKED)),
        "{:?}",
        closed.fields
    );
}

/// A register of the project's documents holding `cards`.
fn register(cards: &str) -> String {
    format!(
        "# Decisions\n\n<!-- register:start -->\n## Needs your decision\n\n### General\n\n{cards}\n\
         <!-- register:end -->\n"
    )
}

const LOOK: &str = "- [ ] **look** — Is the new page right?\n  \
                    [2026-11-20 04:00] agent — Posted for the owner's look.\n";

/// The card [`LOOK`] as the owner's approve on the branch's page saves it.
fn look_approved() -> String {
    LOOK.replace("[ ]", "[x]") + "  [2026-11-20 05:00] owner · approved — that is it\n"
}

/// The branch of `slug` posting the card [`LOOK`] beside a page, then the owner's approve of it
/// committed alone: its worktree.
fn card_approved(r: &Repo, slug: &str) -> PathBuf {
    let wt = r.add(&slug.replace('/', "-"), Some(slug), "main");
    write(&wt, "ui/page.gd", "extends Node\n");
    write(&wt, "docs/DECISIONS.md", &register(LOOK));
    git(&wt, &["add", "-A"]);
    git(
        &wt,
        &[
            "commit",
            "-qm",
            "the page, and its card for the owner's look",
        ],
    );
    write(&wt, "docs/DECISIONS.md", &register(&look_approved()));
    git(&wt, &["commit", "-qam", "the owner's approve of look"]);
    wt
}

/// The owner's approve of the card a task's branch posted stands for its `owner` stamp: it lands
/// with no approve of the tip, its line naming the card.
#[test]
fn a_task_lands_on_the_owners_approve_of_the_card_its_branch_posted() {
    let r = Repo::new();
    manifest(&r, "");
    let slug = "ui/look";
    let tip = head(&card_approved(&r, slug), "HEAD");
    let staff = staffed(&r, slug);
    assess(&r, slug, &staff.assessor, &["owner"], &tip);
    let (said, result) = landed_by(&r, slug, &staff.orch);
    let at = &tip[..7];
    assert!(
        said.contains(&format!(
            "owner: Approved by the owner on card look, at {at}"
        )),
        "{said}"
    );
    assert_landed(&r, slug, &tip, result);
}

/// A commit after the owner's approve of the card that is more than the owner's line keeps the
/// card's approve from standing, a fresh reviewer's approve of it notwithstanding: the land asks
/// for the owner's approve of the tip, saying which commit came after the card's.
#[test]
fn a_commit_after_the_cards_approve_asks_the_owners_approve_of_the_tip_though_reviewed() {
    let r = Repo::new();
    manifest(&r, "");
    let slug = "ui/look";
    let wt = card_approved(&r, slug);
    write(&wt, "ui/page.gd", "extends Node2D\n");
    git(&wt, &["commit", "-qam", "more of the page"]);
    let tip = head(&wt, "HEAD");
    let staff = staffed(&r, slug);
    assess(&r, slug, &staff.assessor, &["review", "owner"], &tip);
    let store = &r.board.store;
    let rev = r.board.session("reviewer");
    store.take_role(slug, &rev, "reviewer").unwrap();
    store
        .verdict(slug, &rev, "approve", &format!("{tip}: read it"))
        .unwrap();
    let before = head(&r.root, "main");
    let (_, result) = landed_by(&r, slug, &staff.orch);
    let at = &tip[..7];
    assert_eq!(
        result,
        Err(format!(
            "{slug} lacks at its tip {at} what it needs: owner (Waits on the owner's approve of \
             {at}; the owner's approve of card look came before {at}, which changes more than \
             the owner's lines), which the owner gives on the task's page"
        ))
    );
    assert_eq!(head(&r.root, "main"), before, "main stays where it was");
    // The owner's approve of the tip lands it, as before.
    store
        .say(slug, "owner", &format!("approve {tip}"), None)
        .unwrap();
    let (said, result) = landed_by(&r, slug, &staff.orch);
    assert!(said.contains("owner: Approved by the owner at "), "{said}");
    assert_landed(&r, slug, &tip, result);
}

/// A waiver stands only on the tip its assessor read: a commit after it brings the catalogue's
/// defaults back, and the land refuses to the orchestrator, naming the assessment and its tip.
#[test]
fn a_waiver_on_another_tip_is_refused_the_defaults_back() {
    let r = Repo::new();
    manifest(&r, "");
    let (staff, read) = in_review(&r, SLUG, &[("sim/jump.gd", "extends Node\n")]);
    assess(&r, SLUG, &staff.assessor, &["tests"], &read);
    let wt = r.dir.join(SLUG.replace('/', "-"));
    write(&wt, "sim/jump.gd", "extends Node # and more\n");
    git(&wt, &["commit", "-qam", "more after the assessment"]);
    let tip = head(&wt, "HEAD");
    green(&r, SLUG, &tip);
    let before = head(&r.root, "main");
    let (_, result) = landed_by(&r, SLUG, &staff.orch);
    let line = format!(
        "{}; its assessment by {} read {}, not the tip {}, so the catalogue's defaults stand \
         beside the stamps it names: an assessment lowers what a task needs only on the tip it \
         read",
        unapproved(SLUG, "review"),
        staff.assessor,
        &read[..7],
        &tip[..7]
    );
    assert_refused(&r, &result, &line, &before);
    assert_eq!(
        told(&r, SLUG),
        (
            format!(
                "land refused: {line}. Start an assessor for the tip, or staff a fresh reviewer, \
                 which approves naming it"
            ),
            Some("@orchestrator".into())
        )
    );
    assert_eq!(r.board.store.get(SLUG).unwrap().status, "review");
    // Assessed again at the new tip, it lands.
    let again = r.board.session("reviewer");
    assess(&r, SLUG, &again, &["tests"], &tip);
    let (_, result) = landed_by(&r, SLUG, &staff.orch);
    assert_landed(&r, SLUG, &tip, result);
}

/// A stray red run (a mistyped command, say) keeps the `tests` stamp from standing: the refusal,
/// said to the executor, names each red command and what clears it, and the same command run
/// again, green, at the tip does.
#[test]
fn a_stray_red_run_is_named_and_cleared_by_the_same_command_green_at_the_tip() {
    let r = Repo::new();
    manifest(&r, "");
    let (staff, tip) = in_review(&r, SLUG, &[("sim/jump.gd", "extends Node\n")]);
    assess(&r, SLUG, &staff.assessor, &["tests"], &tip);
    let stray = "cargo test -p sim --test jmup";
    ran(&r, SLUG, &tip, stray, Phase::Test, 101);
    green(&r, SLUG, &tip);
    let before = head(&r.root, "main");
    let (_, result) = landed_by(&r, SLUG, &staff.orch);
    let at = &tip[..7];
    let line = format!(
        "{SLUG} lacks at its tip {at} what it needs: tests (red at {at}: `{stray}` (exit 101); \
         the newest run of each command at the tip is what counts, so running that command again, \
         green, at {at} clears it; one that cannot pass (a command that names what is not \
         there) is cleared only by a new commit, where the stamps the task needs are given \
         again), which its executor gives at that tip"
    );
    assert_refused(&r, &result, &line, &before);
    let (text, to) = told(&r, SLUG);
    assert!(text.starts_with(&format!("land refused: {line}. Give them at {at}")));
    assert_eq!(to.as_deref(), Some("@executor"));
    assert_eq!(r.board.store.get(SLUG).unwrap().status, "review");
    // A run of it at another commit clears nothing; the same command green at the tip does.
    ran(&r, SLUG, &before, stray, Phase::Test, 0);
    assert!(landed_by(&r, SLUG, &staff.orch).1.is_err());
    ran(&r, SLUG, &tip, stray, Phase::Test, 0);
    let (_, result) = landed_by(&r, SLUG, &staff.orch);
    assert_landed(&r, SLUG, &tip, result);
}

/// A command its runner refused at the tip (Cargo's usage error for test names typed without
/// `--`, exit 1, as `tools/board-pg-lifecycle` met at 22fc65d3 on 2026-11-18) refuses no land
/// whose other runs there are green. A failing test, a compile error and a timeout beside it each
/// still do, and so does the same exit with no refusal read in its log.
#[test]
fn a_command_its_runner_refused_at_the_tip_does_not_refuse_the_land_and_a_real_red_still_does() {
    let typed = "just test-rs -p symphony-app --test hooks own_names owner_words named_recipes";
    let usage = "error: unexpected argument 'owner_words' found";
    let r = Repo::new();
    manifest(&r, "");
    let (staff, tip) = in_review(&r, SLUG, &[("sim/jump.gd", "extends Node\n")]);
    assess(&r, SLUG, &staff.assessor, &["tests"], &tip);
    green(&r, SLUG, &tip);
    kept(&r, SLUG, &tip, (typed, Phase::Test), 1, Some(usage));
    let (before, at) = (head(&r.root, "main"), &tip[..7]);
    for (label, exit, why) in [
        ("cargo test -p sim --test jump", 101, "a failing test"),
        ("cargo build -p sim", 101, "a compile error"),
        ("timeout 60 cargo test -p sim", 124, "a timeout"),
        (typed, 1, "the same exit, with no refusal read"),
    ] {
        ran(&r, SLUG, &tip, label, Phase::Test, exit);
        let line = landed_by(&r, SLUG, &staff.orch).1.expect_err(why);
        assert!(
            line.starts_with(&format!(
                "{SLUG} lacks at its tip {at} what it needs: tests (red at {at}: `{label}` (exit \
                 {exit}); "
            )),
            "{why}: {line}"
        );
        assert_eq!(head(&r.root, "main"), before, "{why}");
        // The same command green there clears it, and the next red stands alone.
        ran(&r, SLUG, &tip, label, Phase::Test, 0);
    }
    // Refused once more, after its green: still no run of the command, and the task lands.
    kept(&r, SLUG, &tip, (typed, Phase::Test), 1, Some(usage));
    let (_, result) = landed_by(&r, SLUG, &staff.orch);
    assert_landed(&r, SLUG, &tip, result);
}

/// A task needing no review is ready only once its executor moves it to review: in progress it is
/// refused to its executor, nothing merged.
#[test]
fn a_task_needing_no_review_is_refused_until_it_is_in_review() {
    let r = Repo::new();
    manifest(&r, "");
    let (staff, tip) = in_review(&r, SLUG, &[("docs/jump.md", "# Jump\n")]);
    assess(&r, SLUG, &staff.assessor, &[], &tip);
    let store = &r.board.store;
    store.move_to(SLUG, "in_progress", &staff.exe).unwrap();
    let before = head(&r.root, "main");
    let (_, result) = landed_by(&r, SLUG, &staff.orch);
    let line = format!(
        "{SLUG} is in_progress: it needs no review, so it lands once its executor says the work \
         is ready by moving it to review (`just tasks move {SLUG} review`)"
    );
    assert_refused(&r, &result, &line, &before);
    assert_eq!(
        told(&r, SLUG),
        (
            format!("land refused: {line}. Move it to review once the work is ready, then land it"),
            Some("@executor".into())
        )
    );
    assert!(runs(&r).is_empty(), "nothing is merged or checked");
    store.move_to(SLUG, "review", &staff.exe).unwrap();
    let (_, result) = landed_by(&r, SLUG, &staff.orch);
    assert_landed(&r, SLUG, &tip, result);
}

/// A task no assessment has read needs the catalogue's defaults, `review` alone: refused in review
/// as before stamps, with today's line to nobody, and landed on a fresh reviewer's approve, its
/// tests unread.
#[test]
fn a_task_no_assessment_read_lands_as_before_stamps() {
    let r = Repo::new();
    manifest(&r, "");
    let (staff, tip) = in_review(&r, SLUG, &[("sim/jump.gd", "extends Node\n")]);
    let before = head(&r.root, "main");
    let (_, result) = landed_by(&r, SLUG, &staff.orch);
    let line = unapproved(SLUG, "review");
    assert_refused(&r, &result, &line, &before);
    assert_eq!(told(&r, SLUG), (format!("land refused: {line}"), None));
    // A red run at the tip is no stamp it needs.
    ran(&r, SLUG, &tip, "cargo test -p sim", Phase::Test, 1);
    let rev = r.board.session("reviewer");
    let store = &r.board.store;
    store.take_role(SLUG, &rev, "reviewer").unwrap();
    store
        .verdict(SLUG, &rev, "approve", &format!("{tip}: good"))
        .unwrap();
    let (said, result) = landed_by(&r, SLUG, &staff.orch);
    assert!(
        said.starts_with(&format!("land {SLUG}: approved by {rev} at ")),
        "{said}"
    );
    assert!(!said.contains("it needs"), "nothing new is said: {said}");
    assert_landed(&r, SLUG, &tip, result);
}

/// A change of settings' values, as a role's model changed in the project file with the guide's
/// words that repeat it: none of it a path `[land] check_paths` names, so its assessment names
/// `tests` alone, and it lands on the settings check's recorded green run at its tip with no
/// reviewer's approve, refused until that run is there. A branch changing a command the land runs
/// is one the check paths name, which keeps `review`.
#[test]
fn a_change_of_settings_values_lands_on_the_settings_check_green_at_its_tip_with_no_reviewer() {
    use symphony_board::land::check_paths;
    let r = Repo::new();
    let land = "[land]\ncheck = false\n";
    let was = format!("[agents]\nlaunch = {{ default = \"claude\", model = \"opus\" }}\n\n{land}");
    project_file(&r, &was);
    write(&r.root, "AGENTS.md", "Executors run on Opus.\n");
    git(&r.root, &["add", "AGENTS.md"]);
    git(&r.root, &["commit", "-qm", "the guide"]);
    let slug = "tools/roles-run-on-fable";
    let is = format!(
        "[agents]\nlaunch = {{ default = \"claude\", model = \"fable\" }}\n\n\
         [agents.models]\nassessor = \"fable\"\n\n{land}"
    );
    let files = [
        ("symphony.toml", is.as_str()),
        ("AGENTS.md", "Executors run on Fable.\n"),
    ];
    let (staff, tip) = in_review(&r, slug, &files);
    let touched = |tip: &str| check_paths::touched(&r.root, "main", tip, &check_paths::defaults());
    assert_eq!(touched(&tip), Ok(Vec::new()), "no test, check or command");
    assess(&r, slug, &staff.assessor, &["tests"], &tip);
    let before = head(&r.root, "main");
    let (_, result) = landed_by(&r, slug, &staff.orch);
    let refused = result.unwrap_err();
    assert!(
        refused.contains("what it needs: tests (No build or test recorded for it at "),
        "{refused}"
    );
    assert_eq!(head(&r.root, "main"), before, "nothing merged");
    ran(&r, slug, &tip, "just settings-check", Phase::Test, 0);
    let (said, result) = landed_by(&r, slug, &staff.orch);
    assert!(said.contains("it needs tests at "), "{said}");
    assert!(!said.contains("approved by"), "{said}");
    assert_landed(&r, slug, &tip, result);
    let approves = (r.board.store.thread(slug).unwrap().iter())
        .filter(|post| post.kind == "verdict" && post.text.starts_with("approve"))
        .count();
    assert_eq!(approves, 0, "no reviewer read it");
    // A value that is a command the land runs is no plain setting: the check paths name it.
    let commands = is.replace(
        land,
        "[land]\ncheck = false\ngenerators = []\npush = false\n",
    );
    let (_, tip) = in_review(&r, "tools/land-command", &[("symphony.toml", &commands)]);
    assert_eq!(touched(&tip), Ok(vec!["symphony.toml [land]".to_string()]));
}
