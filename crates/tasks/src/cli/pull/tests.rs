//! What a PULL says once its recipient was told one before ([`Digest::changes_since`]).

use super::*;

fn task(slug: &str) -> Task {
    Task {
        branch: None,
        id: 0,
        slug: slug.into(),
        title: format!("Task {slug}"),
        body: String::new(),
        status: "ready".into(),
        priority: 2,
        parent: None,
        created_by: String::new(),
        created: 0.0,
        updated: 0.0,
        links: Vec::new(),
        depends: Vec::new(),
        roles: Vec::<crate::Role>::new(),
        instruction: false,
        fields: Default::default(),
    }
}

/// A scoped digest for `keen` listing `lists` (a category and the slugs or handles in it).
fn digest(executing: usize, lists: &[(&'static str, &[&str])]) -> Digest {
    let mut tasks: BTreeMap<&'static str, Vec<Task>> = BTreeMap::new();
    let mut idle = Vec::new();
    for (name, named) in lists {
        if *name == "idle" {
            idle.extend(named.iter().map(|h| h.to_string()));
        } else {
            tasks.insert(name, named.iter().map(|slug| task(slug)).collect());
        }
    }
    Digest {
        now: 0.0,
        executing,
        running: Vec::new(),
        tasks,
        idle,
        notes: BTreeMap::new(),
        liveness_unknown: Vec::new(),
        unanswered: Vec::new(),
        unanswered_scanned: true,
        cap: None,
        slots: None,
        volume: BTreeMap::from([("keen".to_string(), 9)]),
        scoped: true,
    }
}

fn backlog(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("t/{i:02}")).collect()
}

/// What the recipient was told once it acknowledged the PULL that said `said`.
fn told(said: &Said) -> BTreeSet<(String, String)> {
    said.entries.clone()
}

#[test]
fn a_backlog_that_gains_one_row_says_that_row_and_its_count_not_the_six_it_said_before() {
    let before = backlog(33);
    let before: Vec<&str> = before.iter().map(String::as_str).collect();
    let said = digest(2, &[("ready_unstaffed", &before)]).said();
    let mut after = before.clone();
    after.push("t/new");
    let now = digest(2, &[("ready_unstaffed", &after)]);
    assert_eq!(
        now.changes_since(&said, &told(&said)),
        [
            "PULL ready_unstaffed=34 (was 33)",
            "  ready_unstaffed p2 t/new — Task t/new",
        ]
    );
    // The whole digest, on demand, still lists the first six and counts the rest.
    let whole = now.lines();
    assert_eq!(whole.len(), 1 + SHOWN + 1, "{whole:?}");
    assert_eq!(whole.last().unwrap(), "  ready_unstaffed … and 28 more");
}

#[test]
fn rows_resolved_and_counts_moved_are_said_once_beside_what_was_added() {
    let said = digest(
        1,
        &[
            ("ready_unstaffed", &["t/a", "t/b"]),
            ("in_progress_orphaned", &["t/o"]),
            ("idle", &["calm-vole"]),
        ],
    )
    .said();
    let now = digest(
        2,
        &[
            ("ready_unstaffed", &["t/b", "t/c"]),
            ("idle", &["calm-vole"]),
        ],
    );
    assert_eq!(
        now.changes_since(&said, &told(&said)),
        [
            "PULL executing=2 (was 1) in_progress_orphaned=0 (was 1)",
            "  ready_unstaffed p2 t/c — Task t/c",
            "  resolved ready_unstaffed t/a",
            "  resolved in_progress_orphaned t/o",
        ]
    );
}

#[test]
fn an_idle_session_still_idle_is_not_said_again_and_one_that_ends_is_resolved() {
    let said = digest(0, &[("idle", &["calm-vole", "slate-owl"])]).said();
    let still = digest(0, &[("idle", &["calm-vole", "slate-owl"])]);
    assert_eq!(
        still.changes_since(&said, &told(&said)),
        ["PULL counts unchanged"]
    );
    let one_left = digest(0, &[("idle", &["slate-owl", "amber-heron"])]);
    assert_eq!(
        one_left.changes_since(&said, &told(&said)),
        [
            "PULL counts unchanged",
            "  idle amber-heron",
            "  resolved idle calm-vole"
        ]
    );
}

#[test]
fn many_rows_added_at_once_list_the_first_six_and_count_the_rest() {
    let said = digest(0, &[]).said();
    let added = backlog(9);
    let added: Vec<&str> = added.iter().map(String::as_str).collect();
    let now = digest(0, &[("review_unreviewed", &added)]);
    let lines = now.changes_since(&said, &told(&said));
    assert_eq!(lines[0], "PULL review_unreviewed=9 (was 0)");
    assert_eq!(lines.len(), 1 + SHOWN + 1, "{lines:?}");
    assert_eq!(lines.last().unwrap(), "  review_unreviewed … and 3 more");
}

#[test]
fn a_row_told_by_an_earlier_pull_is_not_added_though_the_last_one_did_not_name_it() {
    // Told t/a before, and the last PULL said only t/b: t/a is not new.
    let said = digest(0, &[("ready_unstaffed", &["t/b"])]).said();
    let mut told = told(&said);
    told.insert(("ready_unstaffed".into(), "t/a".into()));
    let now = digest(0, &[("ready_unstaffed", &["t/a", "t/b"])]);
    assert_eq!(
        now.changes_since(&said, &told),
        ["PULL ready_unstaffed=2 (was 1)"]
    );
}

#[test]
fn a_row_that_left_and_came_back_since_the_last_pull_is_said_again_not_resolved() {
    // The last PULL said t/a; it left the digest (forgotten from what was told) and is back.
    let said = digest(0, &[("ready_unstaffed", &["t/a", "t/b"])]).said();
    let told = BTreeSet::from([("ready_unstaffed".to_string(), "t/b".to_string())]);
    let now = digest(0, &[("ready_unstaffed", &["t/a", "t/b"])]);
    assert_eq!(
        now.changes_since(&said, &told),
        [
            "PULL counts unchanged",
            "  ready_unstaffed p2 t/a — Task t/a"
        ]
    );
}

#[test]
fn the_low_volume_condition_is_said_when_it_starts_and_resolved_when_it_ends() {
    let mut quiet = digest(0, &[]);
    quiet.volume.insert("keen".into(), 1);
    let busy = digest(0, &[]);
    let said = busy.said();
    assert_eq!(
        quiet.changes_since(&said, &told(&said)),
        [
            "PULL counts unchanged",
            "  low_volume keen open=1 threshold=4 cap=none — pick disjoint ready work or resolve owned blockers using actual resources and ownership",
        ]
    );
    let said = quiet.said();
    assert_eq!(
        busy.changes_since(&said, &told(&said)),
        ["PULL counts unchanged", "  resolved low_volume keen"]
    );
}

#[test]
fn liveness_and_slots_are_said_when_they_change() {
    let mut before = digest(0, &[]);
    before.liveness_unknown = vec!["amber-heron".into()];
    (before.cap, before.slots) = (Some(4), Some(2));
    let said = before.said();
    assert_eq!(said.counts["slots"], "2/4");
    assert_eq!(
        said.counts["red_unstaffed"], "0",
        "counts hidden at zero are kept"
    );
    let mut now = digest(0, &[]);
    (now.cap, now.slots) = (Some(4), Some(3));
    assert_eq!(
        now.changes_since(&said, &told(&said)),
        ["PULL slots=3/4 (was 2/4)", "  liveness unknown: resolved"]
    );
    now.liveness_unknown = vec!["slate-owl".into()];
    (now.cap, now.slots) = (None, None);
    assert_eq!(
        now.changes_since(&said, &told(&said)),
        [
            "PULL slots=none (was 2/4)",
            "  liveness unknown: native presence sweep overdue or failed; work remains held: slate-owl",
        ]
    );
}

#[test]
fn a_look_whose_unanswered_scan_failed_says_nothing_of_those_posts_and_the_next_one_does() {
    // The last PULL counted one unanswered post, t/a's post #3.
    let mut said = digest(0, &[]).said();
    said.counts.insert("unanswered".into(), "1".into());
    let post = ("unanswered".to_string(), "t/a#3".to_string());
    said.entries.insert(post.clone());
    // A look whose scan failed knows of none: it neither counts nor resolves them.
    let mut skipped = digest(0, &[("ready_unstaffed", &["t/b"])]);
    skipped.unanswered_scanned = false;
    assert_eq!(
        skipped.changes_since(&said, &told(&said)),
        [
            "PULL ready_unstaffed=1 (was 0)",
            "  ready_unstaffed p2 t/b — Task t/b"
        ]
    );
    // Once told it, its recipient knows the post as before, and their count as unknown.
    let after = skipped.said_after(Some(&said));
    assert_eq!(after.counts["unanswered"], UNKNOWN);
    assert!(after.entries.contains(&post), "{after:?}");
    assert_eq!(after.counts["ready_unstaffed"], "1");
    // Another failed look says nothing more of them.
    let again = skipped.said_after(Some(&after));
    assert_eq!(
        skipped.changes_since(&after, &told(&after)),
        ["PULL counts unchanged"]
    );
    assert_eq!(again, after);
    // The next look whose scan answers says their count, and resolves the post now answered.
    let answered = digest(0, &[("ready_unstaffed", &["t/b"])]);
    assert_eq!(
        answered.changes_since(&after, &told(&after)),
        [
            "PULL unanswered=0 (was unknown)",
            "  resolved unanswered t/a#3"
        ]
    );
    // With no PULL before, a failed look's count is unknown and it names no post.
    let first = skipped.said_after(None);
    assert_eq!(first.counts["unanswered"], UNKNOWN);
    assert!(!first.entries.iter().any(|(name, _)| name == "unanswered"));
    // A look whose scan answered leaves what it says.
    assert_eq!(answered.said_after(Some(&said)), answered.said());
}

#[test]
fn what_a_pull_said_survives_the_record_it_is_kept_in() {
    let mut said = digest(1, &[("landed_open", &["t/x"]), ("idle", &["calm-vole"])]);
    said.liveness_unknown = vec!["amber-heron".into()];
    let said = said.said();
    let kept: Said = serde_json::from_value(serde_json::to_value(&said).unwrap()).unwrap();
    assert_eq!(kept, said);
    assert!(said.entries.contains(&("landed_open".into(), "t/x".into())));
    assert!(said.entries.contains(&("idle".into(), "calm-vole".into())));
}
