//! A land dispatched at review (`tasks land SLUG --dispatch`, docs/TASKS.md "A land delivered by
//! a pull request"): the head published with its evidence whatever it says and the host's own
//! merge of it armed, the host merging once `symphony/eligible` is green, and the watcher
//! (`tasks land --watch`) closing the task once it has, or taking what holds the merge as a
//! failure. The host is the bare repository of [`Host`], which merges an armed pull request by
//! itself at the next read once its published evidence stands, as GitHub does seconds after.

use super::*;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;
use symphony_board::land::dispatch::{self, CLOSES, FAILED, Watching};
use symphony_board::land::{dispatch_by, file_stem};

/// The dispatch of `slug` as `actor` with `host` answering: what it printed, and how it ended.
fn dispatched(
    r: &Repo,
    slug: &str,
    actor: &str,
    host: &dyn Transport,
) -> (String, Result<String, String>) {
    dispatched_with(r, slug, actor, host, &[])
}

/// [`dispatched`] with `vars` in its environment.
fn dispatched_with(
    r: &Repo,
    slug: &str,
    actor: &str,
    host: &dyn Transport,
    vars: &[(&str, &str)],
) -> (String, Result<String, String>) {
    let mut out = Vec::new();
    let env = landing(r, vars);
    let result = dispatch_by(&r.board.store, &env, slug, actor, &mut out, host);
    (String::from_utf8(out).unwrap(), result.map_err(refusal))
}

/// `rounds` rounds at most of the watcher at `clock`, `host` answering and `vars` in its
/// environment: what it printed, the line it ended with, and how often it rested.
fn watched(
    r: &Repo,
    host: &dyn Transport,
    clock: i64,
    rounds: u32,
    vars: &[(&str, &str)],
) -> (String, String, u32) {
    let rested = std::cell::Cell::new(0);
    let rest = |wait: Duration| {
        assert_eq!(wait, Duration::from_secs(30), "[land] watch_seconds");
        rested.set(rested.get() + 1);
    };
    let watching = Watching {
        transport: Some(host),
        clock: &|| clock,
        rest: &rest,
        rounds: Some(rounds),
    };
    let mut out = Vec::new();
    let line = dispatch::watch(&r.board.store, &landing(r, vars), &mut out, &watching).unwrap();
    (String::from_utf8(out).unwrap(), line, rested.get())
}

/// `slug` in review, writing `file`, assessed at its head as needing `tests` and `review` and its
/// tests green there: everything but its approve.
fn pending(r: &Repo, slug: &str, file: &str) -> (People, PathBuf, String) {
    let (people, wt, tip) = in_review(r, slug, file);
    assess(r, slug, &people.assessor, &["tests", "review"], &tip);
    ran(r, slug, &tip, "cargo test -p sim", 0);
    (people, wt, tip)
}

fn journal(r: &Repo, slug: &str) -> Journal {
    Journal::read(&lands(r), slug).unwrap()
}

fn status(r: &Repo, slug: &str) -> String {
    r.board.store.get(slug).unwrap().status
}

/// The last post on `slug`'s thread: its text, and to whom.
fn last_post(r: &Repo, slug: &str) -> (String, Option<String>) {
    let post = r.board.store.thread(slug).unwrap().pop().unwrap();
    (post.text, post.recipient)
}

/// Two hosts, each answering for its own task, as one GitHub answers for two pull requests.
struct Two<'a, 'r>(&'a Host<'r>, &'a Host<'r>);

impl Transport for Two<'_, '_> {
    fn ask(&self, verb: &str, request: &Value) -> Result<Value, String> {
        if request["slug"] == SLUG {
            self.0.ask(verb, request)
        } else {
            self.1.ask(verb, request)
        }
    }
}

/// `slug`, approved and dispatched with a host whose merge waits on more than the evidence: armed,
/// its evidence standing. The host and when its evidence first stood.
fn armed_green<'r>(r: &'r Repo, bare: &Path, people: &People) -> (Host<'r>, i64) {
    let host = Host::new(r, bare).arming(Arms::Always);
    let (_, result) = dispatched(r, SLUG, &people.orch, &host);
    let line = result.unwrap();
    assert!(
        line.ends_with("once symphony/eligible is green, as it is now"),
        "{line}"
    );
    assert!(host.is_armed());
    let green = journal(r, SLUG).green.unwrap();
    (host, green)
}

// -- at review -------------------------------------------------------------------------------------

#[test]
fn a_move_to_review_publishes_its_checks_pending_and_arms_the_hosts_own_merge() {
    let (r, bare) = delivering();
    let before = head(&bare, "main");
    let (people, _, tip) = pending(&r, SLUG, "sim/jump.gd");
    let host = Host::new(&r, &bare);
    let (_, result) = dispatched(&r, SLUG, &people.exe, &host);
    let line = result.unwrap();
    let dispatched_line = format!(
        "land {SLUG}: dispatched {} as pull request #3 of owner/name \
         (https://example.test/owner/name/pull/3), which merges it itself once symphony/eligible \
         is green: its evidence says so once it stands",
        &tip[..7]
    );
    assert_eq!(line, dispatched_line);
    assert_eq!(host.verbs(), ["publish", "arm"]);
    // Published whatever it says: no approve yet, so eligible is pending and says why.
    let published = host.request("publish");
    assert_eq!(published["head"], tip.as_str());
    assert_eq!(published["snapshot"]["eligible"], false);
    let blocking = published["snapshot"]["blocking"].as_array().unwrap();
    assert!(!blocking.is_empty());
    // Armed for the squash a land asks, at that head.
    let armed = host.request("arm");
    assert_eq!(armed["head"], tip.as_str());
    assert_eq!(armed["method"], "squash");
    assert_eq!(armed["title"], format!("{SLUG}: A task"));
    assert_eq!(
        (armed["repository"].as_i64(), armed["pull"].as_i64()),
        (Some(7), Some(3))
    );
    assert!(host.is_armed());
    let kept = journal(&r, SLUG);
    assert_eq!((kept.stage, kept.green), (Stage::Armed, None));
    assert_eq!(kept.closes, CLOSES.map(str::to_string));
    assert_eq!(status(&r, SLUG), "review", "a dispatch moves nothing");
    assert_nothing_landed(&r, &bare, &before);
    assert_eq!(last_post(&r, SLUG).0, dispatched_line);
    // Dispatched again as it is: the pull request read, nothing published or armed, said once.
    let (_, result) = dispatched(&r, SLUG, &people.exe, &host);
    assert_eq!(
        result.unwrap(),
        format!(
            "{SLUG}: armed at {} by pull request #3, its evidence as published",
            &tip[..7]
        )
    );
    assert_eq!(host.verbs()[2..], ["result"]);
    let said = (r.board.said(SLUG).into_iter())
        .filter(|line| line.contains(": dispatched "))
        .count();
    assert_eq!(said, 1);
}

#[test]
fn a_stamp_red_at_the_head_is_published_as_red_and_one_not_given_yet_as_missing_alone() {
    let (r, bare) = delivering();
    let (people, _, tip) = in_review(&r, SLUG, "sim/jump.gd");
    assess(&r, SLUG, &people.assessor, &["tests", "review"], &tip);
    ran(&r, SLUG, &tip, "cargo test -p sim", 1);
    let host = Host::new(&r, &bare);
    dispatched(&r, SLUG, &people.exe, &host).1.unwrap();
    let stamps = host.request("publish")["snapshot"]["stamps"].clone();
    let stamp = |name: &str| {
        (stamps.as_array().unwrap().iter())
            .find(|stamp| stamp["name"] == name)
            .unwrap()
            .clone()
    };
    assert_eq!(
        (
            stamp("tests")["standing"].clone(),
            stamp("tests")["red"].clone()
        ),
        (json!("missing"), json!(true))
    );
    // The approve not given yet is missing, not red: said by no `red` at all.
    assert_eq!(stamp("review")["standing"], "missing");
    assert!(stamp("review").get("red").is_none(), "{}", stamp("review"));
    // Green at the head again: red no more.
    ran(&r, SLUG, &tip, "cargo test -p sim", 0);
    dispatched(&r, SLUG, &people.exe, &host).1.unwrap();
    let stamps = host.request("publish")["snapshot"]["stamps"].clone();
    let tests = (stamps.as_array().unwrap().iter())
        .find(|stamp| stamp["name"] == "tests")
        .unwrap();
    assert_eq!(tests["standing"], "met");
    assert!(tests.get("red").is_none(), "{tests}");
}

#[test]
fn an_approve_turns_eligible_green_the_host_merges_and_the_watcher_closes_it_once() {
    let (r, bare) = delivering();
    project_file(
        &r,
        &format!(
            "{DELIVERS}\n[board]\non_land = [\"sh\", \"-c\", \"echo $1 $2 >> '{}'\", \"hook\"]\n",
            r.dir.join("landings").display()
        ),
    );
    git(&r.root, &["push", "-q", "origin", "main"]);
    let base = head(&bare, "main");
    let (people, _, tip) = pending(&r, SLUG, "sim/jump.gd");
    let host = Host::new(&r, &bare);
    dispatched(&r, SLUG, &people.exe, &host).1.unwrap();
    // Nothing merges while eligible is pending, and the watcher asks the host nothing of it.
    let vars = [("TASKS_ON_LAND", "on"), ("TASKS_ON_LAND_SAYS", "60")];
    let (_, line, rested) = watched(&r, &host, dispatch::seconds(), 1, &vars);
    assert_eq!(line, "1 armed at the host after 1 round(s)");
    assert_eq!(rested, 1);
    assert_eq!(host.verbs(), ["publish", "arm"]);
    assert_eq!(head(&bare, "main"), base);
    // The approve, and the write's dispatch: eligible green at the head, the merge left armed.
    approve(&r, SLUG, &format!("{tip}: reads well"));
    assert_eq!(status(&r, SLUG), "done");
    let (_, result) = dispatched(&r, SLUG, &people.orch, &host);
    assert!(result.unwrap().ends_with(", as it is now"));
    assert_eq!(host.verbs()[2..], ["result", "publish", "arm"]);
    assert_eq!(host.request("publish")["snapshot"]["eligible"], true);
    assert!(journal(&r, SLUG).green.is_some());
    assert_eq!(
        head(&bare, "main"),
        base,
        "merged by the host, not by the dispatch"
    );
    // The host merges on green; the watcher's next round finds it and closes the task.
    let (said, line, _) = watched(&r, &host, dispatch::seconds(), 3, &vars);
    assert_eq!(line, "nothing is armed at the host: the watcher ends");
    let squash = head(&bare, "main");
    assert_ne!(squash, base);
    assert!(
        said.contains(&format!("landed {SLUG} on main at {}", &squash[..12])),
        "{said}"
    );
    assert_eq!(head(&r.root, "main"), squash, "the checkout follows");
    assert_eq!(landings(&r, SLUG).len(), 1);
    assert_eq!(status(&r, SLUG), "done");
    assert_eq!(Receipt::read(&lands(&r), SLUG).unwrap().head, tip);
    assert!(!Journal::path(&lands(&r), SLUG).exists());
    assert_eq!(host.verbs()[5..], ["result", "result", "landed"]);
    let counted = fs::read_to_string(r.dir.join("landings")).unwrap_or_default();
    assert_eq!(
        counted,
        format!("{SLUG} {squash}\n"),
        "on_land started once"
    );
}

#[test]
fn a_task_that_needs_no_reviewer_is_merged_from_review_and_closed_from_there() {
    let (r, bare) = delivering();
    let (people, _, tip) = in_review(&r, SLUG, "sim/jump.gd");
    assess(&r, SLUG, &people.assessor, &["tests"], &tip);
    ran(&r, SLUG, &tip, "cargo test -p sim", 0);
    let host = Host::new(&r, &bare).arming(Arms::Always);
    dispatched(&r, SLUG, &people.exe, &host).1.unwrap();
    assert_eq!(status(&r, SLUG), "review");
    let (said, ..) = watched(&r, &host, dispatch::seconds(), 2, &[]);
    assert!(said.contains(&format!("landed {SLUG} on main")), "{said}");
    assert_eq!(status(&r, SLUG), "done");
    assert_eq!(landings(&r, SLUG).len(), 1);
}

#[test]
fn the_host_merging_at_once_is_asked_to_merge_by_the_dispatch_itself() {
    // A repository whose rules wait on nothing but the evidence, which stands: GitHub arms no
    // auto-merge of a pull request it would merge now, and the dispatch asks the merge.
    let (r, bare) = delivering();
    let (people, _, tip) = ready(&r, SLUG);
    let host = Host::new(&r, &bare);
    let (_, result) = dispatched(&r, SLUG, &people.orch, &host);
    let line = result.unwrap();
    assert!(
        line.starts_with(&format!("landed {SLUG} on main at ")),
        "{line}"
    );
    assert_eq!(
        host.verbs(),
        ["publish", "arm", "merge", "result", "landed"]
    );
    assert_eq!(host.request("merge")["head"], tip.as_str());
    assert_eq!(landings(&r, SLUG).len(), 1);
}

// -- a new head ------------------------------------------------------------------------------------

#[test]
fn a_changes_verdict_turns_the_merge_off_and_a_new_head_is_pushed_pending_at_it() {
    let (r, bare) = delivering();
    let (people, wt, tip) = pending(&r, SLUG, "sim/jump.gd");
    let host = Host::new(&r, &bare);
    dispatched(&r, SLUG, &people.exe, &host).1.unwrap();
    // Changes: the task back in progress, its checks said again and its merge off.
    let reviewer = r.board.session("reviewer");
    let store = &r.board.store;
    store.take_role(SLUG, &reviewer, "reviewer").unwrap();
    store
        .verdict(SLUG, &reviewer, "changes", "the jump is short")
        .unwrap();
    assert_eq!(status(&r, SLUG), "in_progress");
    let (_, result) = dispatched(&r, SLUG, &people.orch, &host);
    let line = result.unwrap();
    assert_eq!(
        line,
        format!(
            "land {SLUG}: its auto-merge is off while {SLUG} is in_progress, its checks as its \
             evidence says at {}; it is armed again once it moves to review",
            &tip[..7]
        )
    );
    // Off before anything is published: nothing the host reads after can merge it.
    assert_eq!(host.verbs()[2..], ["result", "disarm", "publish"]);
    assert!(!host.is_armed());
    let kept = journal(&r, SLUG);
    assert_eq!((kept.stage, kept.armed), (Stage::Eligible, false));
    // Dispatched again while in progress: published, nothing armed.
    let (_, result) = dispatched(&r, SLUG, &people.orch, &host);
    assert!(
        result
            .unwrap()
            .ends_with("it is armed once it moves to review")
    );
    assert!(!host.is_armed());
    // A new commit and the move back to review: the new head pushed and armed, pending at it.
    write(&wt, "sim/jump.gd", "extends Node # higher\n");
    git(&wt, &["commit", "-qam", "a higher jump"]);
    let new = head(&wt, "HEAD");
    store.move_to(SLUG, "review", &people.exe).unwrap();
    let (_, result) = dispatched(&r, SLUG, &people.exe, &host);
    assert!(
        result
            .unwrap()
            .contains(&format!("dispatched {} as pull request #3", &new[..7]))
    );
    let published = host.request("publish");
    assert_eq!(published["head"], new.as_str());
    assert_eq!(
        head(&bare, &format!("refs/heads/{SLUG}")),
        new,
        "never forced"
    );
    assert_eq!(published["snapshot"]["eligible"], false);
    let blocking = published["snapshot"]["blocking"].to_string();
    assert!(
        blocking.contains(&format!("not the head {}", &new[..7])),
        "{blocking}"
    );
    assert!(host.is_armed());
    assert_eq!(journal(&r, SLUG).head, new);
}

#[test]
fn a_head_or_evidence_changed_with_no_dispatch_is_published_again_by_the_watcher() {
    // The owner's approve through the Symphony, or a run recorded at the head: no command of the
    // board's dispatches for it, and the watcher's next round finds it.
    let (r, bare) = delivering();
    let (people, _, tip) = pending(&r, SLUG, "sim/jump.gd");
    let host = Host::new(&r, &bare).arming(Arms::Always);
    dispatched(&r, SLUG, &people.exe, &host).1.unwrap();
    approve(&r, SLUG, &format!("{tip}: reads well"));
    let (said, ..) = watched(&r, &host, dispatch::seconds(), 1, &[]);
    assert!(
        said.contains(&format!(
            "land {SLUG}: its evidence changed since it was published: publishing it again"
        )),
        "{said}"
    );
    assert_eq!(host.verbs()[2..], ["publish", "arm"]);
    assert_eq!(host.request("publish")["snapshot"]["eligible"], true);
    // Armed through it: the thread heard its dispatch once, at the first arming.
    let dispatches = (r.board.said(SLUG).iter())
        .filter(|line| line.contains(&format!("land {SLUG}: dispatched ")))
        .count();
    assert_eq!(dispatches, 1);
    // Its next round finds it merged.
    let (said, line, _) = watched(&r, &host, dispatch::seconds(), 2, &[]);
    assert!(said.contains(&format!("landed {SLUG} on main")), "{said}");
    assert_eq!(line, "nothing is armed at the host: the watcher ends");
}

/// The lines that said `slug` went back to review as its approve does not name its head, each
/// with whom it was said to.
fn sent_back(r: &Repo, slug: &str) -> Vec<(String, Option<String>)> {
    (r.board.store.thread(slug).unwrap().into_iter())
        .filter(|post| post.text.contains("which its approve by"))
        .map(|post| (post.text, post.recipient))
        .collect()
}

/// A commit on `wt`'s branch after its approve, as a merge of main that settles a conflict is:
/// the new head.
fn moved_on(wt: &Path) -> String {
    write(wt, "sim/jump.gd", "extends Node # with main\n");
    git(wt, &["commit", "-qam", "main merged into it"]);
    head(wt, "HEAD")
}

#[test]
fn a_head_moved_under_an_approve_is_assessed_and_approved_again_with_no_hand_move() {
    let (r, bare) = delivering();
    let (people, wt, tip) = pending(&r, SLUG, "sim/jump.gd");
    let host = Host::new(&r, &bare).arming(Arms::Always);
    dispatched(&r, SLUG, &people.exe, &host).1.unwrap();
    approve(&r, SLUG, &format!("{tip}: reads well"));
    assert_eq!(status(&r, SLUG), "done");
    // The head moves under the approve; the next write's dispatch sends it back to review.
    let new = moved_on(&wt);
    dispatched(&r, SLUG, &people.orch, &host).1.unwrap();
    assert_eq!(status(&r, SLUG), "review");
    let task = r.board.store.get(SLUG).unwrap();
    assert!(symphony_board::orchestration::unlanded(&task).is_none());
    let reviewer = task.holders("reviewer")[0].to_string();
    assert_eq!(
        sent_back(&r, SLUG),
        [(
            format!(
                "land {SLUG}: {SLUG}'s head is {new7}, which its approve by {reviewer} does not \
                 name (it names {}); {SLUG} is back in review. Its assessor reads {new7} and \
                 its reviewer approves it naming it; its pull request shows {new7} with \
                 `symphony/eligible` pending until then",
                &tip[..7],
                new7 = &new[..7]
            ),
            Some("@orchestrator".to_string())
        )]
    );
    // Meanwhile the pull request is at the new head, pending there, its merge armed across it.
    let published = host.request("publish");
    assert_eq!(published["head"], new.as_str());
    assert_eq!(published["snapshot"]["status"], "review");
    assert_eq!(published["snapshot"]["eligible"], false);
    assert!(host.is_armed());
    // The approve of the new head before its assessment, which is written all the same, the
    // task left done.
    ran(&r, SLUG, &new, "cargo test -p sim", 0);
    // A later round is the same reviewer's, resumed.
    (r.board.store)
        .verdict(
            SLUG,
            &reviewer,
            "approve",
            &format!("{new}: the merge reads well"),
        )
        .unwrap();
    assess(&r, SLUG, &people.assessor, &["tests", "review"], &new);
    assert_eq!(status(&r, SLUG), "done");
    let (_, result) = dispatched(&r, SLUG, &people.orch, &host);
    assert!(result.unwrap().ends_with(", as it is now"));
    assert_eq!(host.request("publish")["snapshot"]["eligible"], true);
    assert_eq!(status(&r, SLUG), "done", "an approve naming the head stays");
    assert_eq!(sent_back(&r, SLUG).len(), 1);
    // Merged and closed: landed, it takes no assessment.
    let (said, ..) = watched(&r, &host, dispatch::seconds(), 2, &[]);
    assert!(said.contains(&format!("landed {SLUG} on main")), "{said}");
    assert_eq!(Receipt::read(&lands(&r), SLUG).unwrap().head, new);
    let again = Needs {
        stamps: vec!["tests".into()],
        tip: Some(new.clone()),
        by: people.assessor.clone(),
        why: "after its land".into(),
        import: None,
    };
    let refused = (r.board.store)
        .set_needs(SLUG, &Catalogue::symphony(), &again)
        .unwrap_err();
    assert_eq!(
        refused.to_string(),
        format!("{SLUG} is done: only an open task, or one approved and not landed, is assessed")
    );
}

#[test]
fn a_head_moved_under_an_approve_with_no_write_is_sent_back_by_the_watcher_once() {
    let (r, bare) = delivering();
    let (people, wt, _) = ready(&r, SLUG);
    // Armed, its evidence standing, and a check of another's the host still waits on.
    let host = Host::new(&r, &bare).arming(Arms::Always);
    host.waits();
    dispatched(&r, SLUG, &people.orch, &host).1.unwrap();
    assert_eq!(status(&r, SLUG), "done");
    let new = moved_on(&wt);
    let (said, ..) = watched(&r, &host, dispatch::seconds(), 2, &[]);
    assert!(
        said.contains(&format!(
            "land {SLUG}: its head changed since it was published: publishing it again"
        )),
        "{said}"
    );
    assert_eq!(status(&r, SLUG), "review");
    assert_eq!(host.request("publish")["head"], new.as_str());
    assert_eq!(host.request("publish")["snapshot"]["eligible"], false);
    assert!(host.is_armed());
    // Said once, however many rounds read it in review.
    let sent = sent_back(&r, SLUG);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].1.as_deref(), Some("@orchestrator"));
    assert!(landings(&r, SLUG).is_empty());
}

/// The board's own line of `slug`'s move back to review from done, by `by`.
fn reopened_by(slug: &str, by: &str) -> String {
    format!("{slug} is back in review from done, by {by}: ")
}

/// Whether a post on `slug`'s thread names the hand move back to review.
fn hand_move(r: &Repo, slug: &str) -> bool {
    let hand = format!("`just tasks move {slug} review` does");
    (r.board.store.thread(slug).unwrap().iter()).any(|post| post.text.contains(&hand))
}

#[test]
fn an_assessors_dispatch_sends_a_head_moved_under_an_approve_back_as_its_orchestrator() {
    // The dispatch an assessment write starts runs as the assessor, who holds no role on the task
    // that may send it back to review: the move is its orchestrator's, with no hand move asked.
    let (r, bare) = delivering();
    let (people, wt, tip) = pending(&r, SLUG, "sim/jump.gd");
    let host = Host::new(&r, &bare).arming(Arms::Always);
    dispatched(&r, SLUG, &people.exe, &host).1.unwrap();
    approve(&r, SLUG, &format!("{tip}: reads well"));
    assert_eq!(status(&r, SLUG), "done");
    let task = r.board.store.get(SLUG).unwrap();
    assert!(task.roles.iter().all(|role| role.handle != people.assessor));
    let reviewer = task.holders("reviewer")[0].to_string();
    let new = moved_on(&wt);
    assess(&r, SLUG, &people.assessor, &["tests", "review"], &new);
    let (_, result) = dispatched(&r, SLUG, &people.assessor, &host);
    result.unwrap();
    assert_eq!(status(&r, SLUG), "review");
    let thread = r.board.store.thread(SLUG).unwrap();
    let by = |handle: &str| {
        (thread.iter())
            .filter(|post| post.text.starts_with(&reopened_by(SLUG, handle)))
            .count()
    };
    // The board's line to its executor and its reviewer names the orchestrator as the mover.
    assert_eq!((by(&people.orch), by(&people.assessor)), (2, 0));
    assert_eq!(
        sent_back(&r, SLUG),
        [(
            format!(
                "land {SLUG}: {SLUG}'s head is {new7}, which its approve by {reviewer} does not \
                 name (it names {}); {SLUG} is back in review. Its assessor reads {new7} and \
                 its reviewer approves it naming it; its pull request shows {new7} with \
                 `symphony/eligible` pending until then",
                &tip[..7],
                new7 = &new[..7]
            ),
            Some("@orchestrator".to_string())
        )]
    );
    assert!(!hand_move(&r, SLUG));
    // Published at the new head, pending there, its merge armed across it.
    let published = host.request("publish");
    assert_eq!(published["head"], new.as_str());
    assert_eq!(published["snapshot"]["status"], "review");
    assert_eq!(published["snapshot"]["eligible"], false);
    assert!(host.is_armed());
}

#[test]
fn a_head_moved_under_an_approve_with_no_orchestrator_goes_back_only_by_who_may_move_it() {
    // No orchestrator to move as: the dispatch moves as who started it, and an assessor's is
    // refused naming the hand move, as before; its executor's moves it.
    let (r, bare) = delivering();
    let (people, wt, tip) = pending(&r, SLUG, "sim/jump.gd");
    let host = Host::new(&r, &bare).arming(Arms::Always);
    dispatched(&r, SLUG, &people.exe, &host).1.unwrap();
    approve(&r, SLUG, &format!("{tip}: reads well"));
    // Its orchestrator left the board, and no other is there to be given it.
    let store = &r.board.store;
    store.leave(&people.orch).unwrap();
    let task = store.get(SLUG).unwrap();
    assert!(task.holders("orchestrator").is_empty());
    let reviewer = task.holders("reviewer")[0].to_string();
    let new = moved_on(&wt);
    assess(&r, SLUG, &people.assessor, &["tests", "review"], &new);
    let (_, result) = dispatched(&r, SLUG, &people.assessor, &host);
    let moved = format!(
        "{SLUG}'s head is {}, which its approve by {reviewer} does not name (it names {})",
        &new[..7],
        &tip[..7]
    );
    let line = format!(
        "{moved}, and it does not go back to review ({} is not {SLUG}'s executor, reviewer or \
         orchestrator, which alone send it back to review)",
        people.assessor
    );
    assert_eq!(result.unwrap_err(), line);
    assert_eq!(status(&r, SLUG), "done");
    assert_eq!(
        last_post(&r, SLUG),
        (
            format!("land refused: {line}. `just tasks move {SLUG} review` does"),
            Some("@orchestrator".to_string())
        )
    );
    assert!(
        sent_back(&r, SLUG)
            .iter()
            .all(|(text, _)| !text.contains("is back in review"))
    );
    // Its executor's dispatch, a session that may move it, sends it back.
    dispatched(&r, SLUG, &people.exe, &host).1.unwrap();
    assert_eq!(status(&r, SLUG), "review");
    let reopened = (store.thread(SLUG).unwrap().into_iter())
        .filter(|post| post.text.starts_with(&reopened_by(SLUG, &people.exe)))
        .count();
    assert_eq!(reopened, 2);
}

#[test]
fn an_approve_that_names_no_commit_goes_back_to_review_for_one_naming_the_head() {
    // A delivery takes an approve naming the head alone: one that names none never lets the
    // host merge, so the task waits in review, where a reviewer can name it.
    let (r, bare) = delivering();
    let (people, _, tip) = pending(&r, SLUG, "sim/jump.gd");
    let host = Host::new(&r, &bare).arming(Arms::Always);
    approve(&r, SLUG, "reads well");
    dispatched(&r, SLUG, &people.orch, &host).1.unwrap();
    assert_eq!(status(&r, SLUG), "review");
    let sent = sent_back(&r, SLUG);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(
        sent[0].0.contains(&format!(
            "{SLUG}'s head is {}, which its approve by ",
            &tip[..7]
        )) && sent[0].0.contains("does not name (it names no commit)"),
        "{}",
        sent[0].0
    );
    assert_eq!(host.request("publish")["snapshot"]["eligible"], false);
}

// -- a merge that fails ----------------------------------------------------------------------------

#[test]
fn a_red_check_of_anothers_or_a_conflict_sends_it_back_to_work_its_merge_off_first() {
    for case in ["red", "conflict"] {
        let (r, bare) = delivering();
        let before = head(&bare, "main");
        let (people, _, tip) = ready(&r, SLUG);
        let (host, green) = armed_green(&r, &bare, &people);
        match case {
            "red" => host.red_check("ci/build"),
            _ => host.conflicts(),
        }
        let (said, ..) = watched(&r, &host, green, 1, &[]);
        let why = match case {
            "red" => format!(
                "a check that is not the Symphony's is red on its head {}, which owner/name \
                 merges past none of: ci/build (failure)",
                &tip[..7]
            ),
            _ => "owner/name reports pull request #3 in conflict with main".to_string(),
        };
        let line = format!("land {SLUG}: not merged: {why}; nothing merged, its auto-merge off");
        assert!(said.contains(&line), "{case}: {said}");
        assert_eq!(
            host.verbs()[2..],
            ["result", "disarm"],
            "{case}: disarmed first"
        );
        assert!(!host.is_armed(), "{case}");
        assert_eq!(
            status(&r, SLUG),
            "in_progress",
            "{case}: done, then review, then work"
        );
        let (text, to) = last_post(&r, SLUG);
        assert!(text.starts_with(&line), "{case}: {text}");
        assert!(
            text.contains("is back in progress. Its executor merges main"),
            "{text}"
        );
        assert_eq!(to.as_deref(), Some("@orchestrator"));
        let kept = journal(&r, SLUG);
        assert_eq!((kept.stage, kept.green), (Stage::Eligible, None), "{case}");
        assert_nothing_landed(&r, &bare, &before);
        // Nothing is armed: the watcher ends.
        let (_, line, _) = watched(&r, &host, green, 1, &[]);
        assert_eq!(
            line, "nothing is armed at the host: the watcher ends",
            "{case}"
        );
    }
}

#[test]
fn a_pull_request_closed_by_someone_sends_the_task_back_to_work_and_is_bound_no_more() {
    let (r, bare) = delivering();
    let (people, ..) = ready(&r, SLUG);
    let (host, green) = armed_green(&r, &bare, &people);
    host.close_by_hand();
    let (said, ..) = watched(&r, &host, green, 1, &[]);
    assert!(
        said.contains(&format!(
            "land {SLUG}: not merged: pull request #3 was closed without a merge"
        )),
        "{said}"
    );
    assert!(
        !host.verbs().contains(&"disarm".to_string()),
        "nothing to turn off"
    );
    assert_eq!(status(&r, SLUG), "in_progress");
    assert!(
        !Journal::path(&lands(&r), SLUG).exists(),
        "the next dispatch opens another"
    );
}

#[test]
fn the_bound_passed_leaves_it_where_it_is_its_merge_off_and_a_land_arms_it_again() {
    let (r, bare) = delivering();
    let (people, ..) = ready(&r, SLUG);
    let (host, green) = armed_green(&r, &bare, &people);
    host.waits();
    // At the bound, it waits.
    let (said, ..) = watched(&r, &host, green + 30 * 60, 1, &[]);
    assert!(
        said.contains("its evidence standing for 30 min: waiting for owner/name to merge it"),
        "{said}"
    );
    assert!(host.is_armed());
    // Past it: disarmed, said, the task left done.
    let (said, ..) = watched(&r, &host, green + 30 * 60 + 1, 1, &[]);
    assert!(
        said.contains(
            "owner/name neither merged pull request #3 nor reported it red within 30 min of its \
             evidence standing (`[land] merge_within_minutes`)"
        ),
        "{said}"
    );
    assert!(!host.is_armed());
    assert_eq!(status(&r, SLUG), "done");
    let (text, to) = last_post(&r, SLUG);
    assert!(
        text.ends_with(&format!(
            "The work is not at fault: {SLUG} stays done, and `just land {SLUG}` arms it again"
        )),
        "{text}"
    );
    assert_eq!(to.as_deref(), Some("@orchestrator"));
    assert_eq!(journal(&r, SLUG).stage, Stage::Eligible);
    // `just land` arms it again, its evidence standing.
    let (_, result) = landed(&r, SLUG, &people.orch, &host);
    let line = result.unwrap();
    assert!(
        line.ends_with("once symphony/eligible is green, as it is now"),
        "{line}"
    );
    assert!(host.is_armed());
    assert_eq!(journal(&r, SLUG).stage, Stage::Armed);
}

#[test]
fn a_bound_of_its_own_settings_counts_from_when_its_evidence_stood() {
    let (r, bare) = delivering();
    project_file(
        &r,
        &format!("{DELIVERS}merge_within_minutes = 5\nwatch_seconds = 30\n"),
    );
    git(&r.root, &["push", "-q", "origin", "main"]);
    let (people, ..) = ready(&r, SLUG);
    let (host, green) = armed_green(&r, &bare, &people);
    host.waits();
    let (said, ..) = watched(&r, &host, green + 5 * 60 + 1, 1, &[]);
    assert!(
        said.contains("within 5 min of its evidence standing"),
        "{said}"
    );
}

#[test]
fn a_task_that_cannot_go_back_has_a_task_filed_for_its_failed_merge_once() {
    // A document save stays pending and shown: it does not go back to work.
    let slug = "docs/owner-save-17";
    let (r, bare) = delivering();
    let (people, _, tip) = in_review(&r, slug, "sim/jump.gd");
    assess(&r, slug, &people.assessor, &["tests"], &tip);
    ran(&r, slug, &tip, "cargo test -p sim", 0);
    let host = Host::new(&r, &bare).arming(Arms::Always);
    dispatched(&r, slug, &people.exe, &host).1.unwrap();
    host.red_check("ci/build");
    let (said, ..) = watched(&r, &host, dispatch::seconds(), 1, &[]);
    let filed = format!("{FAILED}docs-owner-save-17");
    assert!(said.contains(&format!("filed {filed}")), "{said}");
    assert_eq!(status(&r, slug), "review", "it stays");
    let task = r.board.store.get(&filed).unwrap();
    assert_eq!(
        task.title,
        format!("The merge of {slug} by pull request failed")
    );
    assert!(task.body.contains("ci/build (failure)"), "{}", task.body);
    assert!(
        task.body
            .contains("is a document save, which stays pending and shown")
    );
    assert_eq!(task.priority, 1);
    let (text, to) = last_post(&r, slug);
    assert!(text.contains(&format!("filed {filed}")), "{text}");
    assert_eq!(to.as_deref(), Some("@orchestrator"));
    // Armed again and failing again: a line on the task filed, no second task.
    dispatched(&r, slug, &people.exe, &host).1.unwrap();
    let (said, ..) = watched(&r, &host, dispatch::seconds(), 1, &[]);
    assert!(said.contains(&format!("a line added to {filed}")), "{said}");
    let thread = r.board.said(&filed);
    assert!(
        thread.iter().any(|line| line.contains("not merged")),
        "{thread:?}"
    );
}

// -- what the watcher reads ------------------------------------------------------------------------

#[test]
fn a_merge_no_watcher_saw_is_settled_by_a_land_or_a_dispatch_run_again() {
    for by in ["land", "dispatch"] {
        let (r, bare) = delivering();
        let (people, _, tip) = ready(&r, SLUG);
        let (host, _) = armed_green(&r, &bare, &people);
        // The host merged while no watcher ran (a reboot).
        host.merge_on_green();
        let squash = head(&bare, "main");
        let (_, result) = match by {
            "land" => landed(&r, SLUG, &people.orch, &host),
            _ => dispatched(&r, SLUG, &people.orch, &host),
        };
        let line = result.unwrap();
        assert!(
            line.starts_with(&format!("landed {SLUG} on main at {}", &squash[..12])),
            "{by}"
        );
        assert_eq!(head(&r.root, "main"), squash, "{by}");
        assert_eq!(landings(&r, SLUG).len(), 1, "{by}");
        assert_eq!(Receipt::read(&lands(&r), SLUG).unwrap().head, tip, "{by}");
        assert_eq!(
            host.verbs()
                .iter()
                .filter(|verb| *verb == "publish")
                .count(),
            1,
            "{by}"
        );
    }
}

#[test]
fn a_merge_at_a_head_the_branch_moved_past_is_held_for_a_person() {
    let (r, bare) = delivering();
    let (people, wt, _) = ready(&r, SLUG);
    let (host, green) = armed_green(&r, &bare, &people);
    host.merge_on_green();
    write(&wt, "sim/jump.gd", "extends Node # later\n");
    git(&wt, &["commit", "-qam", "later"]);
    let (said, ..) = watched(&r, &host, green, 1, &[]);
    assert!(
        said.contains("the delivery of sim/jump is held for reconciliation"),
        "{said}"
    );
    assert!(said.contains("'s branch is at"), "{said}");
    assert!(landings(&r, SLUG).is_empty());
}

/// The line a watcher ends with once nothing is left for it.
const ENDS: &str = "nothing is armed at the host: the watcher ends";

#[test]
fn a_land_cut_short_after_the_merge_is_finished_by_the_watchers_next_round() {
    for stage in [
        Stage::MergeRequested,
        Stage::RemoteMerged,
        Stage::Reconciled,
        Stage::Closed,
        Stage::Dispatching,
    ] {
        let (r, bare) = delivering();
        project_file(
            &r,
            &format!(
                "{DELIVERS}\n[board]\non_land = [\"sh\", \"-c\", \"echo $1 $2 >> '{}'\", \
                 \"hook\"]\n",
                r.dir.join("landings").display()
            ),
        );
        git(&r.root, &["push", "-q", "origin", "main"]);
        let (people, _, tip) = ready(&r, SLUG);
        // The journal as it stood when the host was told it landed, closed and nothing started
        // yet, kept aside: what a run cut short there leaves.
        let (kept, aside) = (Journal::path(&lands(&r), SLUG), r.dir.join("cut.delivery"));
        let host = Host::new(&r, &bare)
            .arming(Arms::Always)
            .before("landed", move || {
                fs::copy(&kept, &aside).unwrap();
            });
        dispatched(&r, SLUG, &people.orch, &host).1.unwrap();
        host.merge_on_green();
        let squash = head(&bare, "main");
        let (_, line, _) = watched(&r, &host, dispatch::seconds(), 3, &[]);
        assert_eq!(line, ENDS, "{stage:?}");
        assert_eq!(landings(&r, SLUG).len(), 1, "{stage:?}");
        let text = fs::read_to_string(r.dir.join("cut.delivery")).unwrap();
        let mut cut: Journal = serde_json::from_str(&text).unwrap();
        assert_eq!(cut.stage, Stage::Closed);
        cut.stage = stage;
        cut.write(&lands(&r)).unwrap();
        assert_eq!(dispatch::armed(&lands(&r)).len(), 1, "{stage:?}");
        // The watcher stays for it, finishes it in one round and ends.
        let asked = host.verbs().len();
        let vars = [("TASKS_ON_LAND", "on"), ("TASKS_ON_LAND_SAYS", "60")];
        let (said, line, rested) = watched(&r, &host, dispatch::seconds(), 3, &vars);
        assert_eq!(line, ENDS, "{stage:?}: {said}");
        assert_eq!(rested, 1, "{stage:?}");
        assert!(
            said.contains(&format!("landed {SLUG} on main at {}", &squash[..12])),
            "{stage:?}: {said}"
        );
        assert_eq!(landings(&r, SLUG).len(), 1, "{stage:?}: closed once");
        assert_eq!(status(&r, SLUG), "done", "{stage:?}");
        assert_eq!(head(&r.root, "main"), squash, "{stage:?}");
        assert_eq!(Receipt::read(&lands(&r), SLUG).unwrap().head, tip);
        assert!(!Journal::path(&lands(&r), SLUG).exists(), "{stage:?}");
        assert!(dispatch::armed(&lands(&r)).is_empty(), "{stage:?}");
        let verbs = host.verbs()[asked..].to_vec();
        if stage == Stage::MergeRequested {
            assert_eq!(verbs, ["result", "landed"], "the pull request read first");
        } else {
            assert_eq!(verbs, ["landed"], "{stage:?}");
        }
        // What follows a land is started once: by this round, but for a run that named it started.
        let counted = fs::read_to_string(r.dir.join("landings")).unwrap_or_default();
        if stage == Stage::Dispatching {
            assert_eq!(counted, "", "started by the run before, not twice");
            assert!(said.contains("is not started twice"), "{said}");
        } else {
            assert_eq!(counted, format!("{SLUG} {squash}\n"), "{stage:?}");
        }
    }
}

#[test]
fn a_merge_whose_land_stopped_before_its_close_is_closed_by_the_watcher_and_a_held_one_is_not() {
    for held in [false, true] {
        let (r, bare) = delivering();
        let (people, _, tip) = ready(&r, SLUG);
        let (host, green) = armed_green(&r, &bare, &people);
        host.merge_on_green();
        let squash = head(&bare, "main");
        // A run that found the merge and stopped there (the board lost): nothing proved or closed.
        let mut cut = journal(&r, SLUG);
        cut.stage = Stage::MergeRequested;
        cut.held = held.then(|| "a person reads it".to_string());
        cut.write(&lands(&r)).unwrap();
        let asked = host.verbs().len();
        let (said, line, rested) = watched(&r, &host, green, 3, &[]);
        assert_eq!(line, ENDS, "{held}: {said}");
        if held {
            assert_eq!(
                (said.as_str(), rested),
                ("", 0),
                "no round reads a held one"
            );
            assert_eq!(host.verbs().len(), asked);
            assert!(landings(&r, SLUG).is_empty());
            assert_eq!(journal(&r, SLUG), cut, "held as it was");
            assert_ne!(head(&r.root, "main"), squash);
            continue;
        }
        assert_eq!(rested, 1);
        assert!(
            said.contains("a delivery is under way (merge requested at"),
            "{said}"
        );
        assert_eq!(landings(&r, SLUG).len(), 1);
        assert_eq!(head(&r.root, "main"), squash, "the checkout follows");
        assert_eq!(Receipt::read(&lands(&r), SLUG).unwrap().head, tip);
        assert!(!Journal::path(&lands(&r), SLUG).exists());
        assert_eq!(host.verbs()[asked..], ["result", "landed"]);
    }
}

#[test]
fn a_merge_asked_for_that_the_host_never_made_goes_back_to_the_armed_and_is_no_stop() {
    let (r, bare) = delivering();
    let (people, _, _) = ready(&r, SLUG);
    let (host, green) = armed_green(&r, &bare, &people);
    let base = head(&bare, "main");
    // A run that asked for the merge and stopped; the host holds the pull request open, a check
    // of another's its rules require still pending.
    host.waits();
    let mut cut = journal(&r, SLUG);
    cut.stage = Stage::MergeRequested;
    cut.write(&lands(&r)).unwrap();
    let (said, line, _) = watched(&r, &host, green, 1, &[]);
    assert_eq!(line, "1 armed at the host after 1 round(s)", "{said}");
    assert!(
        said.contains(&format!("of {SLUG} is still open: nothing merged")),
        "{said}"
    );
    assert_eq!(posts(&r, SLUG, "is still open: nothing merged"), 1);
    let kept = journal(&r, SLUG);
    assert_eq!((kept.stage, kept.armed), (Stage::Eligible, true));
    assert!(
        !(lands(&r).join(format!("{}.stopped", file_stem(SLUG)))).exists(),
        "not a land a round could not finish"
    );
    assert_eq!(head(&bare, "main"), base);
    assert!(landings(&r, SLUG).is_empty());
}

#[test]
fn a_round_that_cannot_finish_a_land_tries_again_and_says_so_once_past_the_bound() {
    let (r, bare) = delivering();
    let (people, _, _) = ready(&r, SLUG);
    let (host, green) = armed_green(&r, &bare, &people);
    host.merge_on_green();
    let squash = head(&bare, "main");
    // The main checkout is not clean: the merge is proved and nothing here follows it.
    let project = fs::read_to_string(r.root.join("symphony.toml")).unwrap();
    write(&r.root, "symphony.toml", &format!("{project}# an edit\n"));
    let (said, line, rested) = watched(&r, &host, green, 2, &[]);
    assert_eq!(
        line,
        "0 armed at the host and 1 merged with its land not ended after 2 round(s)"
    );
    assert_eq!(rested, 2);
    assert_eq!(journal(&r, SLUG).stage, Stage::RemoteMerged);
    assert!(
        said.contains(&format!(
            "{SLUG}: its land did not end ({SLUG} landed remotely as {}",
            &squash[..7]
        )) && said.contains("): the next round tries again"),
        "{said}"
    );
    assert_eq!(posts(&r, SLUG, "reconciliation pending"), 1, "said once");
    assert_eq!(posts(&r, SLUG, "has not ended"), 0, "within its bound");
    // Both rounds stopped at the same; the watcher's log says it once.
    assert_eq!(said.matches("its land did not end").count(), 1, "{said}");
    assert!(landings(&r, SLUG).is_empty());
    let stopped = lands(&r).join(format!("{}.stopped", file_stem(SLUG)));
    assert_eq!(fs::read_to_string(&stopped).unwrap(), green.to_string());
    // Past `[land] merge_within_minutes` of the first round that stopped: told once, however many
    // rounds stop after.
    let late = green + 31 * 60;
    for _ in 0..2 {
        let (_, line, _) = watched(&r, &host, late, 2, &[]);
        assert!(
            line.starts_with("0 armed at the host and 1 merged"),
            "{line}"
        );
    }
    assert_eq!(posts(&r, SLUG, "has not ended"), 1);
    let (text, to) = last_post(&r, SLUG);
    assert!(
        text.starts_with(&format!(
            "land {SLUG}: the land of {} has not ended (merged remotely at",
            &journal(&r, SLUG).head[..7]
        )),
        "{text}"
    );
    assert!(
        text.contains("within 30 min of the watcher's first round that could not finish it")
            && text.contains("reconciliation pending")
            && text.ends_with(&format!("or run `just land {SLUG}`, which goes on from it")),
        "{text}"
    );
    assert_eq!(to.as_deref(), Some("@orchestrator"));
    assert!(landings(&r, SLUG).is_empty());
    // Put right, the next round finishes it and the watcher ends.
    git(&r.root, &["checkout", "--", "symphony.toml"]);
    let (said, line, _) = watched(&r, &host, late, 3, &[]);
    assert_eq!(line, ENDS, "{said}");
    assert_eq!(landings(&r, SLUG).len(), 1);
    assert_eq!(head(&r.root, "main"), squash);
    assert!(!stopped.exists() && !Journal::path(&lands(&r), SLUG).exists());
}

#[test]
fn two_tasks_armed_are_watched_by_one_watcher_until_neither_is() {
    const OTHER: &str = "sim/run";
    let (r, bare) = delivering();
    let (people, _, tip) = pending(&r, SLUG, "sim/jump.gd");
    let (others, _, other_tip) = pending(&r, OTHER, "sim/run.gd");
    let (one, two) = (
        Host::new(&r, &bare).arming(Arms::Always),
        Host::new(&r, &bare).arming(Arms::Always),
    );
    let host = Two(&one, &two);
    dispatched(&r, SLUG, &people.exe, &host).1.unwrap();
    dispatched(&r, OTHER, &others.exe, &host).1.unwrap();
    let (_, line, _) = watched(&r, &host, dispatch::seconds(), 1, &[]);
    assert_eq!(line, "2 armed at the host after 1 round(s)");
    approve(&r, SLUG, &format!("{tip}: reads well"));
    approve(&r, OTHER, &format!("{other_tip}: reads well"));
    // A round publishes both again, the next closes both, and the third ends it.
    let (said, line, rested) = watched(&r, &host, dispatch::seconds(), 5, &[]);
    assert_eq!(line, "nothing is armed at the host: the watcher ends");
    assert_eq!(rested, 2);
    assert!(said.contains(&format!("landed {SLUG} on main")), "{said}");
    assert!(said.contains(&format!("landed {OTHER} on main")), "{said}");
    assert_eq!(
        (status(&r, SLUG), status(&r, OTHER)),
        ("done".into(), "done".into())
    );
    assert_eq!(head(&r.root, "main"), head(&bare, "main"));
    let squashes = git(&r.root, &["rev-list", "--count", "main~2..main"]);
    assert_eq!(squashes, "2", "one squash each");
}

#[test]
fn a_second_watcher_ends_at_once_while_one_runs() {
    let (r, bare) = delivering();
    let (people, ..) = pending(&r, SLUG, "sim/jump.gd");
    let host = Host::new(&r, &bare);
    dispatched(&r, SLUG, &people.exe, &host).1.unwrap();
    let path = lands(&r).join(dispatch::WATCH_LOCK);
    fs::write(&path, "the watcher (pid 1)").unwrap();
    let held = fs::OpenOptions::new().read(true).open(&path).unwrap();
    held.lock().unwrap();
    let (_, line, rested) = watched(&r, &host, dispatch::seconds(), 1, &[]);
    assert_eq!(line, "the watcher (pid 1) runs already: nothing started");
    assert_eq!(rested, 0);
    held.unlock().unwrap();
}

// -- cancelled -------------------------------------------------------------------------------------

#[test]
fn a_cancelled_tasks_pull_request_is_closed_unmerged_its_branch_left() {
    for by in ["watcher", "dispatch"] {
        let (r, bare) = delivering();
        let before = head(&bare, "main");
        let (people, _, tip) = pending(&r, SLUG, "sim/jump.gd");
        let host = Host::new(&r, &bare);
        dispatched(&r, SLUG, &people.exe, &host).1.unwrap();
        r.board
            .store
            .move_to(SLUG, "cancelled", &people.orch)
            .unwrap();
        let said = match by {
            "watcher" => watched(&r, &host, dispatch::seconds(), 1, &[]).0,
            _ => dispatched(&r, SLUG, &people.orch, &host).1.unwrap(),
        };
        let line = format!(
            "land {SLUG}: pull request #3 of owner/name closed without a merge, as {SLUG} was \
             cancelled; its branch is left"
        );
        assert!(said.contains(&line), "{by}: {said}");
        assert_eq!(
            host.verbs().last().map(String::as_str),
            Some("close"),
            "{by}"
        );
        assert!(host.pull.borrow().2 && !host.is_armed(), "{by}");
        assert_eq!(head(&bare, &format!("refs/heads/{SLUG}")), tip, "{by}");
        assert!(!Journal::path(&lands(&r), SLUG).exists(), "{by}");
        assert_nothing_landed(&r, &bare, &before);
    }
}

// -- what the host does not do ---------------------------------------------------------------------

/// A host on which a red check is run again green and the pull request merged by itself just
/// before the disarm a failure or a move asks reaches it.
struct MergesAtDisarm<'a, 'r>(&'a Host<'r>);

impl Transport for MergesAtDisarm<'_, '_> {
    fn ask(&self, verb: &str, request: &Value) -> Result<Value, String> {
        if verb == "disarm" {
            self.0.foreign.borrow_mut().clear();
            *self.0.waiting.borrow_mut() = false;
            self.0.merge_on_green();
        }
        self.0.ask(verb, request)
    }
}

#[test]
fn a_result_the_host_does_not_give_is_read_again_at_the_next_round_nothing_changed() {
    for case in ["lost", "unavailable"] {
        let (r, bare) = delivering();
        let (people, ..) = ready(&r, SLUG);
        let (host, green) = armed_green(&r, &bare, &people);
        host.waits();
        // Red, were it read.
        host.red_check("ci/build");
        let why = match case {
            "lost" => {
                host.lost.borrow_mut().push("result");
                "result: no answer"
            }
            _ => {
                let away = json!({"state": "unavailable", "why": "the host is away"});
                host.says.borrow_mut().push(("result", away));
                "the host is away"
            }
        };
        let (said, ..) = watched(&r, &host, green, 1, &[]);
        assert!(
            said.contains(&format!(
                "{SLUG}: pull request #3 was not read ({why}): the next round reads it"
            )),
            "{case}: {said}"
        );
        assert_eq!(host.verbs()[2..], ["result"], "{case}: nothing disarmed");
        assert!(host.is_armed(), "{case}");
        assert_eq!(status(&r, SLUG), "done", "{case}");
        let kept = journal(&r, SLUG);
        assert_eq!(
            (kept.stage, kept.green),
            (Stage::Armed, Some(green)),
            "{case}"
        );
    }
}

#[test]
fn a_disarm_the_host_does_not_answer_leaves_it_armed_and_the_next_round_tries_again() {
    let (r, bare) = delivering();
    let before = head(&bare, "main");
    let (people, ..) = ready(&r, SLUG);
    let (host, green) = armed_green(&r, &bare, &people);
    host.waits();
    host.red_check("ci/build");
    host.lost.borrow_mut().push("disarm");
    let (said, ..) = watched(&r, &host, green, 1, &[]);
    assert!(
        said.contains(&format!(
            "{SLUG}: pull request #3 was not disarmed (disarm: no answer): the next round tries \
             again"
        )),
        "{said}"
    );
    assert_eq!(
        status(&r, SLUG),
        "done",
        "nothing moved before the merge is off"
    );
    let kept = journal(&r, SLUG);
    assert_eq!((kept.stage, kept.green), (Stage::Armed, Some(green)));
    // The next round turns it off, and then the task goes back.
    let (said, ..) = watched(&r, &host, green, 1, &[]);
    assert!(
        said.contains(&format!("land {SLUG}: not merged: a check")),
        "{said}"
    );
    assert_eq!(host.verbs()[2..], ["result", "disarm", "result", "disarm"]);
    assert_eq!(status(&r, SLUG), "in_progress");
    assert_nothing_landed(&r, &bare, &before);
}

#[test]
fn an_armed_task_moved_back_by_a_write_no_dispatch_saw_is_disarmed_by_the_watcher() {
    let (r, bare) = delivering();
    let (people, _, tip) = pending(&r, SLUG, "sim/jump.gd");
    let host = Host::new(&r, &bare);
    dispatched(&r, SLUG, &people.exe, &host).1.unwrap();
    assert!(host.is_armed());
    // Moved by the board itself, which starts no dispatch; the host's first answer is lost.
    (r.board.store)
        .move_to(SLUG, "in_progress", &people.exe)
        .unwrap();
    host.lost.borrow_mut().push("disarm");
    let (said, ..) = watched(&r, &host, dispatch::seconds(), 1, &[]);
    assert!(
        said.contains("was not disarmed (disarm: no answer): the next round tries again"),
        "{said}"
    );
    assert_eq!(journal(&r, SLUG).stage, Stage::Armed, "still watched");
    let (said, ..) = watched(&r, &host, dispatch::seconds(), 1, &[]);
    let line = format!(
        "land {SLUG}: its auto-merge is off while {SLUG} is in_progress, its checks as its \
         evidence says at {}; it is armed again once it moves to review",
        &tip[..7]
    );
    assert!(said.contains(&line), "{said}");
    assert_eq!(
        host.verbs()[2..],
        ["disarm", "disarm"],
        "its evidence not standing, the host is not read"
    );
    assert!(!host.is_armed());
    let kept = journal(&r, SLUG);
    assert_eq!((kept.stage, kept.green), (Stage::Eligible, None));
    assert_eq!(last_post(&r, SLUG), (line, None));
    let (_, line, _) = watched(&r, &host, dispatch::seconds(), 1, &[]);
    assert_eq!(line, "nothing is armed at the host: the watcher ends");
}

#[test]
fn a_merge_the_host_made_as_its_disarm_was_asked_is_settled_as_its_land() {
    for case in ["red", "moved back"] {
        let (r, bare) = delivering();
        let (people, _, tip) = ready(&r, SLUG);
        let (host, green) = armed_green(&r, &bare, &people);
        host.waits();
        match case {
            "red" => host.red_check("ci/build"),
            _ => {
                let store = &r.board.store;
                store.move_to(SLUG, "review", &people.orch).unwrap();
                store.move_to(SLUG, "in_progress", &people.orch).unwrap();
            }
        }
        let (said, ..) = watched(&r, &MergesAtDisarm(&host), green, 1, &[]);
        let squash = head(&bare, "main");
        assert!(
            said.contains(&format!("landed {SLUG} on main at {}", &squash[..12])),
            "{case}: {said}"
        );
        assert!(!said.contains("not merged"), "{case}: {said}");
        assert_eq!(head(&r.root, "main"), squash, "{case}");
        assert_eq!(landings(&r, SLUG).len(), 1, "{case}");
        assert_eq!(Receipt::read(&lands(&r), SLUG).unwrap().head, tip, "{case}");
        assert_eq!(
            status(&r, SLUG),
            "done",
            "{case}: closed from where it stood"
        );
    }
}

#[test]
fn a_close_the_host_does_not_make_is_asked_again_while_armed_and_said_where_nothing_reads_it() {
    // Armed: the watcher's next round asks again.
    let (r, bare) = delivering();
    let (people, ..) = pending(&r, SLUG, "sim/jump.gd");
    let host = Host::new(&r, &bare);
    dispatched(&r, SLUG, &people.exe, &host).1.unwrap();
    host.lost.borrow_mut().push("close");
    (r.board.store)
        .move_to(SLUG, "cancelled", &people.orch)
        .unwrap();
    let (said, ..) = watched(&r, &host, dispatch::seconds(), 1, &[]);
    assert!(
        said.contains(&format!(
            "{SLUG} is cancelled and pull request #3 was not closed (close: no answer): the next \
             round tries again"
        )),
        "{said}"
    );
    assert!(Journal::path(&lands(&r), SLUG).exists());
    let (said, ..) = watched(&r, &host, dispatch::seconds(), 1, &[]);
    assert!(said.contains("closed without a merge, as"), "{said}");
    assert!(!Journal::path(&lands(&r), SLUG).exists());

    // Not armed: no round reads it, so the orchestrator is told how to close it, the host's
    // refusal said as it gave it.
    let (r, bare) = delivering();
    let (people, ..) = pending(&r, SLUG, "sim/jump.gd");
    let host = Host::new(&r, &bare).saying("close", json!({"state": "refused"}));
    dispatched(&r, SLUG, &people.exe, &host).1.unwrap();
    let store = &r.board.store;
    store.move_to(SLUG, "in_progress", &people.exe).unwrap();
    dispatched(&r, SLUG, &people.exe, &host).1.unwrap();
    assert_eq!(journal(&r, SLUG).stage, Stage::Eligible);
    store.move_to(SLUG, "cancelled", &people.orch).unwrap();
    let line = dispatched(&r, SLUG, &people.orch, &host).1.unwrap();
    let said = format!(
        "land {SLUG}: {SLUG} is cancelled and pull request #3 was not closed (it gave no reason); \
         it is not armed, so nothing merges it, and `just land {SLUG} --dispatch` closes it"
    );
    assert_eq!(line, said);
    assert_eq!(last_post(&r, SLUG), (said, Some("@orchestrator".into())));

    // Merged by hand meanwhile: held for a person, as nothing closes a cancelled task.
    let (r, bare) = delivering();
    let (people, ..) = pending(&r, SLUG, "sim/jump.gd");
    let host = Host::new(&r, &bare).arming(Arms::Always);
    dispatched(&r, SLUG, &people.exe, &host).1.unwrap();
    *host.eligible.borrow_mut() = true;
    host.merge_on_green();
    (r.board.store)
        .move_to(SLUG, "cancelled", &people.orch)
        .unwrap();
    let (said, ..) = watched(&r, &host, dispatch::seconds(), 1, &[]);
    assert!(
        said.contains(&format!(
            "pull request #3 was merged while {SLUG} was cancelled"
        )),
        "{said}"
    );
    assert!(
        said.contains(&format!(
            "the delivery of {SLUG} is held for reconciliation"
        )),
        "{said}"
    );
    assert!(landings(&r, SLUG).is_empty());
}

#[test]
fn a_merge_armed_before_stays_watched_through_a_publication_that_stops_and_is_closed_once() {
    for case in ["arm lost", "publish unavailable", "watcher, arm lost"] {
        let (r, bare) = delivering();
        let base = head(&bare, "main");
        let (people, _, tip) = pending(&r, SLUG, "sim/jump.gd");
        let host = Host::new(&r, &bare);
        dispatched(&r, SLUG, &people.exe, &host).1.unwrap();
        assert!(host.is_armed(), "{case}");
        // The approve, and its publication stopped before the host answered its arming again:
        // eligible green at the head all the same, and the host's merge armed still.
        approve(&r, SLUG, &format!("{tip}: reads well"));
        match case {
            "publish unavailable" => {
                let away =
                    json!({"state": "unavailable", "why": "its checks did not read back yet"});
                host.says.borrow_mut().push(("publish", away));
            }
            _ => host.lost.borrow_mut().push("arm"),
        }
        let line = match case {
            "publish unavailable" => format!("{SLUG} was not published (unavailable)"),
            _ => format!(
                "pull request #3 of {SLUG} was not armed again at {} (arm: no answer): its merge \
                 armed before stands, which the watcher reads every round and arms again",
                &tip[..7]
            ),
        };
        if case.starts_with("watcher") {
            // The watcher's own round publishes the approve no dispatch was started for.
            let (said, ..) = watched(&r, &host, dispatch::seconds(), 1, &[]);
            assert!(said.contains(&line), "{case}: {said}");
        } else {
            let stopped = dispatched(&r, SLUG, &people.orch, &host).1.unwrap_err();
            assert!(stopped.starts_with(&line), "{case}: {stopped}");
            // Said on the thread, as nobody reads a dispatch's log.
            let (text, to) = last_post(&r, SLUG);
            assert!(
                text.starts_with(&format!("land {SLUG}: its dispatch stopped: {line}")),
                "{case}: {text}"
            );
            assert_eq!(to.as_deref(), Some("@orchestrator"), "{case}");
        }
        host.says.borrow_mut().clear();
        assert_eq!(head(&bare, "main"), base, "{case}: nothing merged yet");
        assert_eq!(host.request("publish")["snapshot"]["eligible"], true);
        let kept = journal(&r, SLUG);
        assert!(kept.armed && kept.watched(), "{case}: {:?}", kept.stage);
        assert_ne!(kept.stage, Stage::Armed, "{case}");
        // The host merges on green; the watcher's next round reads it and closes the task once.
        let (said, line, _) = watched(&r, &host, dispatch::seconds(), 3, &[]);
        assert_eq!(
            line, "nothing is armed at the host: the watcher ends",
            "{case}"
        );
        let squash = head(&bare, "main");
        assert_ne!(squash, base, "{case}");
        assert!(
            said.contains(&format!("landed {SLUG} on main at {}", &squash[..12])),
            "{case}: {said}"
        );
        assert_eq!(head(&r.root, "main"), squash, "{case}");
        assert_eq!(landings(&r, SLUG).len(), 1, "{case}");
        assert_eq!(status(&r, SLUG), "done", "{case}");
        assert_eq!(Receipt::read(&lands(&r), SLUG).unwrap().head, tip, "{case}");
    }
}

#[test]
fn a_first_dispatch_that_stops_before_it_binds_is_said_and_made_by_the_next_dispatch() {
    let (r, bare) = delivering();
    let (people, _, tip) = pending(&r, SLUG, "sim/jump.gd");
    let host = Host::new(&r, &bare).losing(&["publish"]);
    let stopped = dispatched(&r, SLUG, &people.exe, &host).1.unwrap_err();
    assert!(stopped.contains("publish: no answer"), "{stopped}");
    let kept = journal(&r, SLUG);
    assert_eq!(
        (kept.stage, kept.binding.is_none(), kept.armed),
        (Stage::Prepared, true, false)
    );
    assert!(
        !kept.watched(),
        "nothing the host may merge: nothing was armed"
    );
    assert_eq!(
        last_post(&r, SLUG),
        (
            format!(
                "land {SLUG}: its dispatch stopped: {stopped}; `just land {SLUG} --dispatch` \
                 dispatches it again, as a later write to it does"
            ),
            Some("@orchestrator".into())
        )
    );
    // The next dispatch, a later write's: bound, published and armed.
    let line = dispatched(&r, SLUG, &people.exe, &host).1.unwrap();
    assert!(
        line.contains(&format!("dispatched {} as pull request #3", &tip[..7])),
        "{line}"
    );
    assert!(host.is_armed());
    let kept = journal(&r, SLUG);
    assert_eq!((kept.stage, kept.armed), (Stage::Armed, true));
}

// -- a dispatch after the merge ----------------------------------------------------------------------

#[test]
fn a_dispatch_after_the_land_of_its_merge_says_it_landed_and_stops_nothing() {
    let (r, bare) = delivering();
    let (people, wt, tip) = ready(&r, SLUG);
    let (host, _) = armed_green(&r, &bare, &people);
    // The host merges on green, and the watcher's round closes the task and drops its journal.
    let (said, ..) = watched(&r, &host, dispatch::seconds(), 2, &[]);
    let squash = head(&bare, "main");
    assert!(
        said.contains(&format!("landed {SLUG} on main at {}", &squash[..12])),
        "{said}"
    );
    assert!(!Journal::path(&lands(&r), SLUG).exists());
    let (asked, thread) = (host.verbs().len(), r.board.said(SLUG).len());
    // A write's dispatch that waited for that land (the approve's, the assessor's): the host is
    // asked nothing, and the thread is told of no stop and no command to run.
    let (_, result) = dispatched(&r, SLUG, &people.orch, &host);
    assert_eq!(
        result.unwrap(),
        format!(
            "{SLUG} landed already as {} from {} by pull request #3 of owner/name: nothing is \
             dispatched",
            &squash[..7],
            &tip[..7]
        )
    );
    assert_eq!(host.verbs().len(), asked);
    assert_eq!(r.board.said(SLUG).len(), thread);
    assert_eq!(posts(&r, SLUG, "its dispatch stopped"), 0);
    assert_eq!(posts(&r, SLUG, "--dispatch"), 0);
    assert!(
        !Journal::path(&lands(&r), SLUG).exists(),
        "none left behind"
    );
    assert_eq!(landings(&r, SLUG).len(), 1);
    // Its branch at another head than the one merged is the refusal it was: a slug delivered once.
    write(&wt, "sim/jump.gd", "extends Node2D\n");
    git(&wt, &["commit", "-qam", "more, after it landed"]);
    let stopped = dispatched(&r, SLUG, &people.orch, &host).1.unwrap_err();
    assert!(
        stopped.contains("its branch is now at") && stopped.contains("a slug is delivered once"),
        "{stopped}"
    );
    assert_eq!(host.verbs().len(), asked);
    assert_eq!(posts(&r, SLUG, &stopped), 1, "said on the thread");
}

/// A host whose own merge, armed and held by a check of another's, comes between a dispatch's
/// read of the pull request and its publication, as GitHub merges seconds after the last check.
struct MergesBeforePublish<'a, 'r>(&'a Host<'r>);

impl Transport for MergesBeforePublish<'_, '_> {
    fn ask(&self, verb: &str, request: &Value) -> Result<Value, String> {
        if verb == "publish" {
            *self.0.waiting.borrow_mut() = false;
            self.0.merge_on_green();
        }
        self.0.ask(verb, request)
    }
}

#[test]
fn a_publish_refused_for_a_merge_made_meanwhile_stops_nothing_at_its_head_alone() {
    for case in ["its head", "another head"] {
        let (r, bare) = delivering();
        let (people, wt, tip) = ready(&r, SLUG);
        let (host, _) = armed_green(&r, &bare, &people);
        host.waits();
        // What has the next dispatch publish again: its evidence changed, or its head.
        if case == "its head" {
            ran(&r, SLUG, &tip, "cargo test -p sim --lib", 0);
        } else {
            write(&wt, "sim/jump.gd", "extends Node2D\n");
            git(&wt, &["commit", "-qam", "more, while it was armed"]);
        }
        let asked = host.verbs().len();
        let racing = MergesBeforePublish(&host);
        let stopped = dispatched(&r, SLUG, &people.orch, &racing).1.unwrap_err();
        // Read open, refused at the publication, then read again: merged.
        assert_eq!(
            host.verbs()[asked..],
            ["result", "publish", "result"],
            "{case}"
        );
        let squash = head(&bare, "main");
        let refused = format!(
            "{SLUG} was not published (refused): pull request #3 is merged or closed: nothing \
             more is published; nothing merged"
        );
        if case == "another head" {
            // Merged at the head armed, not the one this dispatch was asked to publish.
            assert_eq!(stopped, refused, "{case}");
            assert_eq!(posts(&r, SLUG, "its dispatch stopped"), 1, "{case}");
            let (text, to) = last_post(&r, SLUG);
            assert_eq!(
                text,
                format!(
                    "land {SLUG}: its dispatch stopped: {refused}; `just land {SLUG} --dispatch` \
                     dispatches it again, as a later write to it does"
                )
            );
            assert_eq!(to.as_deref(), Some("@orchestrator"));
            assert_eq!(landings(&r, SLUG).len(), 0, "{case}");
            continue;
        }
        assert_eq!(
            stopped,
            format!(
                "{SLUG}: pull request #3 of owner/name merged {} while it was dispatched: nothing \
                 more is published to it, and the watcher's next round closes the task",
                &tip[..7]
            )
        );
        assert_eq!(posts(&r, SLUG, "its dispatch stopped"), 0);
        assert_eq!(posts(&r, SLUG, "--dispatch"), 0);
        assert_eq!(landings(&r, SLUG).len(), 0, "the watcher's to close");
        let kept = journal(&r, SLUG);
        assert!(kept.watched(), "{:?}", kept.stage);
        // The watcher's next round reads the merge and closes the task, once.
        let (said, line, _) = watched(&r, &host, dispatch::seconds(), 2, &[]);
        assert_eq!(line, "nothing is armed at the host: the watcher ends");
        assert!(
            said.contains(&format!("landed {SLUG} on main at {}", &squash[..12])),
            "{said}"
        );
        assert_eq!(landings(&r, SLUG).len(), 1);
        assert_eq!(status(&r, SLUG), "done");
        assert_eq!(Receipt::read(&lands(&r), SLUG).unwrap().head, tip);
    }
}

/// The posts on `slug`'s thread that hold `text`.
fn posts(r: &Repo, slug: &str, text: &str) -> usize {
    (r.board.said(slug).iter())
        .filter(|line| line.contains(text))
        .count()
}

#[test]
fn a_refusal_a_watcher_round_stops_at_is_said_once_at_a_head_however_many_rounds_read_it() {
    // A new head refused at the head: one that changes a generator's outputs, as an executor
    // committing the built client during review does.
    let (r, bare) = delivering();
    project_file(
        &r,
        &format!(
            "{DELIVERS}generators = [{{ name = \"the client\", outputs = \"dist/\", command = \
             [\"true\"] }}]\n"
        ),
    );
    git(&r.root, &["push", "-q", "origin", "main"]);
    let (people, wt, _) = pending(&r, SLUG, "sim/jump.gd");
    let host = Host::new(&r, &bare);
    dispatched(&r, SLUG, &people.exe, &host).1.unwrap();
    assert!(host.is_armed());
    write(&wt, "dist/app.js", "built\n");
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-qm", "its own build"]);
    let refused = "changes 1 generated path (dist/app.js)";
    let (said, line, rested) = watched(&r, &host, dispatch::seconds(), 3, &[]);
    assert_eq!(line, "1 armed at the host after 3 round(s)", "{said}");
    assert_eq!(rested, 3);
    assert_eq!(
        said.matches(refused).count(),
        1,
        "each round reads it, and the log says it once: {said}"
    );
    assert_eq!(posts(&r, SLUG, refused), 1, "said once on the thread");
    // Kept at its head where a round reads it, a land delivered by a pull request.
    let kept = fs::read_to_string(lands(&r).join(format!("{}.said", file_stem(SLUG)))).unwrap();
    assert!(kept.starts_with(&head(&wt, "HEAD")), "{kept}");
    assert!(kept.contains(refused), "{kept}");
    assert_eq!(
        host.verbs()[..2],
        ["publish", "arm"],
        "nothing published since"
    );
    // Another head refused: said again, once.
    write(&wt, "dist/app.js", "built again\n");
    git(&wt, &["commit", "-qam", "its own build again"]);
    watched(&r, &host, dispatch::seconds(), 2, &[]);
    assert_eq!(posts(&r, SLUG, refused), 2, "once a head");

    // An arming refused after its publication, the merge armed before: the task stays watched
    // and is published again each round, and its refusal is said once.
    let (r, bare) = delivering();
    let (people, _, tip) = pending(&r, SLUG, "sim/jump.gd");
    let mut host = Host::new(&r, &bare);
    dispatched(&r, SLUG, &people.exe, &host).1.unwrap();
    host.arms = Arms::Refuses("policy");
    host.waits();
    approve(&r, SLUG, &format!("{tip}: reads well"));
    let refused = "refused to arm pull request #3 of";
    let (said, line, _) = watched(&r, &host, dispatch::seconds(), 3, &[]);
    assert_eq!(line, "1 armed at the host after 3 round(s)", "{said}");
    assert_eq!(
        said.matches(refused).count(),
        2,
        "its evidence changed, then its publication did not end, said once each: {said}"
    );
    assert_eq!(posts(&r, SLUG, refused), 1, "said once on the thread");
    let arms = host.verbs().iter().filter(|verb| *verb == "arm").count();
    assert_eq!(arms, 4, "the dispatch's, then one a round");
    assert!(journal(&r, SLUG).watched());
}

// -- what a dispatch leaves alone ------------------------------------------------------------------

#[test]
fn a_task_never_in_review_or_a_project_delivering_nothing_is_not_dispatched() {
    let (r, bare) = delivering();
    let wt = r.add("sim-jump", Some(SLUG), "main");
    write(&wt, "sim/jump.gd", "extends Node\n");
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-qm", "work"]);
    r.task(SLUG);
    let exe = r.board.session("worker");
    r.board.store.claim(SLUG, &exe).unwrap();
    let host = Host::new(&r, &bare);
    let (_, result) = dispatched(&r, SLUG, &exe, &host);
    assert_eq!(
        result.unwrap(),
        format!("{SLUG} is in_progress: it is dispatched once it moves to review")
    );
    assert!(host.verbs().is_empty());
    // A project with no `[land] delivery`.
    let r = Repo::new();
    project_file(&r, "[land]\ncheck = false\n");
    let bare = remote(&r);
    let (people, ..) = pending(&r, SLUG, "sim/jump.gd");
    let host = Host::new(&r, &bare);
    let (_, result) = dispatched(&r, SLUG, &people.exe, &host);
    assert_eq!(
        result.unwrap(),
        format!("{SLUG}: [land] delivery is not set, so nothing is dispatched")
    );
    assert!(host.verbs().is_empty());
}

#[test]
fn a_refusal_at_the_head_goes_to_the_thread_and_nothing_is_pushed() {
    // The project file here is not the one the remote default branch commits.
    let (r, bare) = delivering();
    let (people, ..) = pending(&r, SLUG, "sim/jump.gd");
    project_file(&r, &format!("{DELIVERS}# changed here\n"));
    let host = Host::new(&r, &bare);
    let (_, result) = dispatched(&r, SLUG, &people.exe, &host);
    let line = result.unwrap_err();
    assert!(line.contains("symphony.toml"), "{line}");
    assert!(host.verbs().is_empty(), "nothing pushed");
    assert!(git(&bare, &["branch", "--list", SLUG]).is_empty());
}

#[test]
fn an_arming_the_host_refuses_names_who_acts_and_arms_nothing() {
    for (kind, to) in [("conflict", "@executor"), ("permission", "@orchestrator")] {
        let (r, bare) = delivering();
        let (people, _, tip) = pending(&r, SLUG, "sim/jump.gd");
        let host = Host::new(&r, &bare).arming(Arms::Refuses(kind));
        let (_, result) = dispatched(&r, SLUG, &people.exe, &host);
        let line = result.unwrap_err();
        assert!(
            line.starts_with(&format!(
                "owner/name refused to arm pull request #3 of {SLUG} at {}: no: {kind}",
                &tip[..7]
            )),
            "{kind}: {line}"
        );
        assert!(!host.is_armed());
        assert_eq!(last_post(&r, SLUG).1.as_deref(), Some(to), "{kind}");
        assert_eq!(
            journal(&r, SLUG).stage,
            Stage::Eligible,
            "{kind}: published, not armed"
        );
    }
}

// -- what starts it --------------------------------------------------------------------------------

/// A stand-in `tasks` in `r`'s folder that records each start, its arguments and the land's
/// mark, one line a start: where the lines go.
fn stand_in(r: &Repo) -> (PathBuf, PathBuf) {
    let record = r.dir.join("started");
    let program = r.dir.join("tasks-stand-in");
    fs::write(
        &program,
        format!(
            "#!/bin/sh\nprintf '%s|%s|%s\\n' \"$*\" \"${{SYMPHONY_LAND-unset}}\" \
             \"${{TASKS_SESSION-unset}}\" >> '{}'\n",
            record.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    (program, record)
}

/// The lines `record` holds once it holds `count`.
fn started(record: &Path, count: usize) -> Vec<String> {
    test_time::until(
        format!("{count} start(s) recorded in {}", record.display()),
        test_time::backstop(Duration::from_secs(60)),
        || {
            let text = fs::read_to_string(record).unwrap_or_default();
            let lines: Vec<String> = text.lines().map(str::to_string).collect();
            (lines.len() >= count && text.ends_with('\n')).then_some(lines)
        },
    )
    .unwrap()
}

/// `tasks ARGS` as `handle` from the checkout, `vars` set: its exit, what it printed, what it
/// said on stderr.
fn tasks(r: &Repo, handle: &str, args: &[&str], vars: &[(&str, &str)]) -> (i32, String, String) {
    let mut env = landing(r, vars);
    env.vars.insert("TASKS_SESSION".into(), handle.into());
    env.vars.insert("TASKS_SWEEP".into(), "off".into());
    let args: Vec<String> = args.iter().map(|a| (*a).to_string()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = symphony_board::cli::main(&args, &env, &mut "".as_bytes(), &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn a_move_to_review_and_a_later_verdict_start_its_dispatch_detached_with_the_lands_mark() {
    let (r, bare) = delivering();
    let (program, record) = stand_in(&r);
    let bin = program.to_string_lossy().into_owned();
    let on = [("TASKS_DISPATCH", "on"), ("TASKS_BIN", bin.as_str())];
    let wt = r.add("sim-jump", Some(SLUG), "main");
    write(&wt, "sim/jump.gd", "extends Node\n");
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-qm", "work"]);
    r.task(SLUG);
    let (orch, exe) = (r.board.session("orchestrator"), r.board.session("worker"));
    let store = &r.board.store;
    store.take_role(SLUG, &orch, "orchestrator").unwrap();
    store.claim(SLUG, &exe).unwrap();
    let (code, out, err) = tasks(&r, &exe, &["move", SLUG, "review"], &on);
    assert_eq!(code, 0, "{out}{err}");
    let said = (out.lines())
        .find(|line| line.starts_with("→ its dispatch to the host started detached"))
        .unwrap_or_else(|| panic!("{out}"));
    assert!(
        said.ends_with(&format!("/symphony/tasks/land/{})", dispatch::DISPATCH_LOG)),
        "{said}"
    );
    // Started as a land is, by its mark, as the mover and with no session of its own.
    assert_eq!(
        started(&record, 1),
        [format!("land {SLUG} --dispatch --as {exe}|dispatch|unset")]
    );
    // A write after it was published dispatches it again; one before, or with the board told not
    // to, starts nothing.
    let host = Host::new(&r, &bare);
    let reviewer = r.board.session("reviewer");
    store.take_role(SLUG, &reviewer, "reviewer").unwrap();
    let off = [("TASKS_DISPATCH", "off"), ("TASKS_BIN", bin.as_str())];
    dispatched(&r, SLUG, &exe, &host).1.unwrap();
    let (code, out, _) = tasks(&r, &reviewer, &["verdict", SLUG, "changes", "short"], &off);
    assert_eq!(code, 0);
    assert!(!out.contains("dispatch"), "{out}");
    store.move_to(SLUG, "review", &exe).unwrap();
    let (code, out, err) = tasks(&r, &reviewer, &["verdict", SLUG, "approve", "fine"], &on);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains("→ its dispatch to the host started detached"),
        "{out}"
    );
    assert_eq!(
        started(&record, 2)[1],
        format!("land {SLUG} --dispatch --as {reviewer}|dispatch|unset")
    );
}

#[test]
fn a_move_to_review_marks_it_so_a_write_dispatches_it_before_any_pull_request_binds() {
    let (r, bare) = delivering();
    let (program, record) = stand_in(&r);
    let bin = program.to_string_lossy().into_owned();
    let on = [("TASKS_DISPATCH", "on"), ("TASKS_BIN", bin.as_str())];
    let wt = r.add("sim-jump", Some(SLUG), "main");
    write(&wt, "sim/jump.gd", "extends Node\n");
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-qm", "work"]);
    r.task(SLUG);
    let (orch, exe) = (r.board.session("orchestrator"), r.board.session("worker"));
    let store = &r.board.store;
    store.take_role(SLUG, &orch, "orchestrator").unwrap();
    store.claim(SLUG, &exe).unwrap();
    let mark = lands(&r).join(format!("{}.dispatching", file_stem(SLUG)));
    assert!(!mark.exists());
    let (code, out, err) = tasks(&r, &exe, &["move", SLUG, "review"], &on);
    assert_eq!(code, 0, "{out}{err}");
    started(&record, 1);
    assert_eq!(
        fs::read_to_string(&mark).unwrap(),
        exe,
        "it names who moved it"
    );
    // Its dispatch bound nothing (the stand-in dispatches nothing, as one that stopped): a later
    // write, the approve, dispatches it again by its mark.
    let reviewer = r.board.session("reviewer");
    store.take_role(SLUG, &reviewer, "reviewer").unwrap();
    let (code, out, err) = tasks(&r, &reviewer, &["verdict", SLUG, "approve", "fine"], &on);
    assert_eq!(code, 0, "{out}{err}");
    assert!(
        out.contains("→ its dispatch to the host started detached"),
        "{out}"
    );
    assert_eq!(
        started(&record, 2)[1],
        format!("land {SLUG} --dispatch --as {reviewer}|dispatch|unset")
    );
    // Bound by a dispatch, the mark goes: from then on its binding starts it.
    let env = landing(&r, &on);
    let host = Host::new(&r, &bare);
    dispatched(&r, SLUG, &exe, &host).1.unwrap();
    assert!(!mark.exists());
    assert!(dispatch::poke(&env, &r.root, SLUG, &exe).is_some());
    started(&record, 3);
    // A task never moved to review is marked by nothing and bound to nothing: none started.
    r.task("sim/walk");
    assert_eq!(dispatch::poke(&env, &r.root, "sim/walk", &exe), None);
    // Nor one out of review before it bound one: its dispatch takes its mark away.
    fs::write(
        lands(&r).join(format!("{}.dispatching", file_stem("sim/walk"))),
        &exe,
    )
    .unwrap();
    let said = dispatched(&r, "sim/walk", &exe, &host).1.unwrap();
    assert!(
        said.ends_with("it is dispatched once it moves to review"),
        "{said}"
    );
    assert_eq!(dispatch::poke(&env, &r.root, "sim/walk", &exe), None);
    assert_eq!(fs::read_to_string(&record).unwrap().lines().count(), 3);
}

#[test]
fn an_arming_starts_the_watcher_where_none_runs_and_none_while_one_does() {
    let (r, bare) = delivering();
    let (program, record) = stand_in(&r);
    let bin = program.to_string_lossy().into_owned();
    let on = [("TASKS_DISPATCH", "on"), ("TASKS_BIN", bin.as_str())];
    let (people, ..) = pending(&r, SLUG, "sim/jump.gd");
    let host = Host::new(&r, &bare);
    let (said, result) = dispatched_with(&r, SLUG, &people.exe, &host, &on);
    result.unwrap();
    assert!(
        said.contains(&format!(
            "land {SLUG}: the watcher reads what the host made of it every round (its lines go to"
        )),
        "{said}"
    );
    assert_eq!(started(&record, 1), ["land --watch|dispatch|unset"]);
    // One holding the lock: none started.
    let path = lands(&r).join(dispatch::WATCH_LOCK);
    let held = (fs::OpenOptions::new().read(true).write(true).create(true))
        .truncate(false)
        .open(&path)
        .unwrap();
    held.lock().unwrap();
    let (said, result) = dispatched_with(&r, SLUG, &people.exe, &host, &on);
    result.unwrap();
    assert!(!said.contains("the watcher"), "{said}");
    held.unlock().unwrap();
    assert_eq!(fs::read_to_string(&record).unwrap().lines().count(), 1);
}

#[test]
fn a_round_that_comes_to_the_same_writes_nothing_to_the_watchers_log() {
    // A task waiting for its approve: read every round, said once, however long it waits.
    let (r, bare) = delivering();
    let (people, _, tip) = pending(&r, SLUG, "sim/jump.gd");
    let host = Host::new(&r, &bare);
    dispatched(&r, SLUG, &people.exe, &host).1.unwrap();
    let waiting = format!(
        "{SLUG}: armed at {}, its evidence not standing yet\n",
        &tip[..7]
    );
    let (said, line, rested) = watched(&r, &host, dispatch::seconds(), 40, &[]);
    assert_eq!(line, "1 armed at the host after 40 round(s)");
    assert_eq!(rested, 40);
    assert_eq!(said, waiting, "one line, not one a round");
    // Another watcher knows nothing of what the one before said: once again.
    let (said, ..) = watched(&r, &host, dispatch::seconds(), 2, &[]);
    assert_eq!(said, waiting);

    // What a round comes to changed (the minutes its evidence stood): said again, once each.
    let (r, bare) = delivering();
    let (people, ..) = ready(&r, SLUG);
    let (host, green) = armed_green(&r, &bare, &people);
    host.waits();
    let round = std::cell::Cell::new(0);
    let clock = || {
        round.set(round.get() + 1);
        green + (round.get() - 1) * 30
    };
    let watching = Watching {
        transport: Some(&host),
        clock: &clock,
        rest: &|_| {},
        rounds: Some(4),
    };
    let mut out = Vec::new();
    dispatch::watch(&r.board.store, &landing(&r, &[]), &mut out, &watching).unwrap();
    let said = String::from_utf8(out).unwrap();
    let stood: Vec<&str> = said.lines().collect();
    assert_eq!(stood.len(), 2, "four rounds, two states: {said}");
    assert!(
        stood[0].contains("its evidence standing for 0 min"),
        "{said}"
    );
    assert!(
        stood[1].contains("its evidence standing for 1 min"),
        "{said}"
    );
}

#[test]
fn the_land_command_takes_a_slug_or_watch_alone() {
    let (r, _) = delivering();
    let land = [("SYMPHONY_LAND", "1")];
    let (code, _, err) = tasks(&r, "amber", &["land", "--watch", SLUG], &land);
    assert_eq!(code, 1);
    assert!(
        err.contains("tasks land --watch takes no slug and no --dispatch"),
        "{err}"
    );
    let (code, _, err) = tasks(&r, "amber", &["land"], &land);
    assert_eq!(code, 1);
    assert!(
        err.contains("tasks land takes the slug of the task it lands, or --watch"),
        "{err}"
    );
    // Nothing armed: the watcher ends at once.
    let (code, out, err) = tasks(&r, "amber", &["land", "--watch"], &land);
    assert_eq!(
        (code, out.as_str()),
        (0, "nothing is armed at the host: the watcher ends\n"),
        "{err}"
    );
}

/// `slug`, filed here where it is not yet, claimed and in review with no branch, then
/// approved by a fresh reviewer through the command line: what the approve printed.
fn approved_line(r: &Repo, slug: &str) -> String {
    let (exe, reviewer) = (r.board.session("worker"), r.board.session("reviewer"));
    let store = &r.board.store;
    if store.get(slug).is_err() {
        r.task(slug);
        store.claim(slug, &exe).unwrap();
        store.move_to(slug, "review", &exe).unwrap();
    }
    store.take_role(slug, &reviewer, "reviewer").unwrap();
    let off = [("TASKS_DISPATCH", "off")];
    let (code, out, err) = tasks(r, &reviewer, &["verdict", slug, "approve", "fine"], &off);
    assert_eq!(code, 0, "{out}{err}");
    out
}

#[test]
fn an_approve_says_the_host_merges_it_naming_its_pull_request_once_bound() {
    let (r, bare) = delivering();
    // Bound by its dispatch at review: the pull request its journal names.
    let (people, ..) = pending(&r, SLUG, "sim/jump.gd");
    let host = Host::new(&r, &bare).arming(Arms::Always);
    dispatched(&r, SLUG, &people.orch, &host).1.unwrap();
    assert_eq!(
        approved_line(&r, SLUG),
        format!(
            "{SLUG} is approved: done on the board, not on main until the host merges it; the \
             host merges pull request #3 of owner/name (https://example.test/owner/name/pull/3) \
             once its checks pass, and the board closes the task; `just land {SLUG} --as HANDLE` \
             only settles it by hand\n"
        )
    );
    // Not bound yet (its dispatch has not published it): no pull request to name.
    assert_eq!(
        approved_line(&r, "sim/walk"),
        "sim/walk is approved: done on the board, not on main until the host merges it; the \
         host merges its pull request, bound by its dispatch, once its checks pass, and the \
         board closes the task; `just land sim/walk --as HANDLE` only settles it by hand\n"
    );
    // A journal that does not read as its own binds nothing.
    fs::write(Journal::path(&lands(&r), "sim/run"), "{").unwrap();
    assert!(approved_line(&r, "sim/run").contains("its pull request, bound by its dispatch,"));
    // A project whose `land` is a recipe of its own keeps its line: `just land` runs that recipe
    // in the Symphony's place.
    write(&r.root, "justfile", "land *ARGS:\n    @echo its own\n");
    assert_eq!(
        approved_line(&r, "sim/fall"),
        format!(
            "sim/fall is approved: done on the board, not on main until it lands; the `land` \
             recipe of the project's own lands it ({}): `just land sim/fall`\n",
            r.root.join("justfile").display()
        )
    );
}
