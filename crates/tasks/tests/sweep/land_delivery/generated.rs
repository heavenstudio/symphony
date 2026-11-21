//! What the regenerator makes where the project delivers by pull request: a task of its own, never
//! a commit on the default branch; one generation for one input key; the land that delivers it as
//! `generated` or refuses it stale; and what a run cut short leaves the next one.

use super::*;
use std::time::Duration;
use symphony_board::land::RECIPE;
use symphony_board::land::regenerate::{self, PENDING, Pending, WITHDRAWN};

/// How many times the bundle was made.
fn made(r: &Repo) -> usize {
    fs::read_to_string(r.dir.join("made"))
        .unwrap_or_default()
        .lines()
        .count()
}

/// The project file of a project that delivers by pull request and makes `gen/out.txt` of
/// `src/*.txt`, counting each time it does; with `key`, by the key those sources hash to.
fn manifest(r: &Repo, key: bool) -> String {
    let keys = if key {
        "keys = { \"the bundle\" = [\"sh\", \"-c\", \"cat src/*.txt | cksum\"] }\n"
    } else {
        ""
    };
    format!(
        "{DELIVERS}generators = [{{ name = \"the bundle\", outputs = \"gen/\", command = [\"sh\", \
         \"-c\", \"echo made >> '{}'; mkdir -p gen && cat src/*.txt > gen/out.txt\"] }}]\n{keys}",
        r.dir.join("made").display()
    )
}

/// A repository delivering by pull request whose bundle is behind its one source, and its host.
fn generating(key: bool) -> (Repo, PathBuf) {
    let r = Repo::new();
    write(&r.root, "src/a.txt", "a\n");
    git(&r.root, &["add", "src"]);
    project_file(&r, &manifest(&r, key));
    let bare = remote(&r);
    (r, bare)
}

/// One run of the regenerator, `host` answering what it asks: what it said, and how it ended.
fn round(r: &Repo, host: Option<&Host>) -> (String, Result<String, String>) {
    let mut out = Vec::new();
    let host = host.map(|host| host as &dyn Transport);
    let result = regenerate::run_by(&r.board.store, &r.env(&r.root), &mut out, host);
    (String::from_utf8(out).unwrap(), result)
}

fn pending(r: &Repo) -> Option<Pending> {
    Pending::read(&lands(r))
}

/// A commit on main here, pushed to the host: `file` as `text`.
fn on_main(r: &Repo, file: &str, text: &str) -> String {
    write(&r.root, file, text);
    git(&r.root, &["add", "-A"]);
    git(&r.root, &["commit", "-qm", &format!("{file} on main")]);
    git(&r.root, &["push", "-q", "origin", "main"]);
    head(&r.root, "main")
}

/// What a round made, assessed at its head as needing no stamp, by a session that filed and
/// orchestrates nothing: the record, and who lands it.
fn assessed(r: &Repo) -> (Pending, String) {
    round(r, None).1.unwrap();
    let pending = pending(r).unwrap();
    let assessor = r.board.session("reviewer");
    assess(r, &pending.slug, &assessor, &[], &pending.head);
    (pending, r.board.session("orchestrator"))
}

fn status(r: &Repo, slug: &str) -> String {
    r.board.store.get(slug).unwrap().status
}

#[test]
fn a_round_files_what_it_made_as_a_task_and_commits_nothing_to_the_default_branch() {
    let (r, bare) = generating(true);
    let tip = head(&r.root, "main");
    let (said, result) = round(&r, None);
    let made_as = pending(&r).unwrap();
    assert_eq!(
        made_as.slug,
        format!("generated/outputs-at-{}-{}", &tip[..12], &made_as.head[..7])
    );
    assert_eq!(made_as.tip, tip);
    assert_eq!(
        result.unwrap(),
        format!(
            "Regenerate the bundle for main at {}: made as {}, awaiting delivery by pull request \
             as {slug} (`just land {slug}`, once its stamps stand); nothing committed to main",
            &tip[..7],
            &made_as.head[..7],
            slug = made_as.slug
        ),
        "{said}"
    );
    // Nothing moved, here or there, and nothing was pushed.
    assert_eq!(head(&r.root, "main"), tip);
    assert_eq!(head(&bare, "main"), tip);
    assert_eq!(
        git(&bare, &["for-each-ref", "--format=%(refname)"]),
        "refs/heads/main"
    );
    assert_eq!(git(&r.root, &["status", "--porcelain"]), "");
    // Its branch is the one commit made, on the tip, of the outputs alone.
    let branch = format!("refs/heads/{}", made_as.slug);
    assert_eq!(head(&r.root, &branch), made_as.head);
    assert_eq!(head(&r.root, &format!("{}^", made_as.head)), tip);
    assert_eq!(
        git(&r.root, &["diff", "--name-only", &tip, &made_as.head]),
        "gen/out.txt"
    );
    // A task in review, which nothing assessed and no stamp was waived for.
    let task = r.board.store.get(&made_as.slug).unwrap();
    assert_eq!(task.status, "review");
    assert_eq!(
        task.title,
        format!("Regenerate the bundle for main at {}", &tip[..7])
    );
    let host = Host::new(&r, &bare);
    let orch = r.board.session("orchestrator");
    let (_, refused) = landed(&r, &made_as.slug, &orch, &host);
    let refused = refused.unwrap_err();
    assert!(
        refused.contains("no assessment names what") || refused.contains("approve"),
        "{refused}"
    );
    assert!(!host.verbs().contains(&"merge".to_string()));
    assert_eq!(head(&bare, "main"), tip);
    // Where it stands: not current, and not being made.
    let standing = regenerate::status(&r.env(&r.root)).unwrap();
    assert!(!standing.current && !standing.making, "{}", standing.line);
    assert!(
        standing.line.contains(&format!(
            "awaiting delivery by pull request as {}",
            made_as.slug
        )),
        "{}",
        standing.line
    );
}

#[test]
fn one_input_key_is_generated_once_however_often_it_is_asked_and_main_moves() {
    let (r, _bare) = generating(true);
    round(&r, None).1.unwrap();
    let first = pending(&r).unwrap();
    assert_eq!(made(&r), 1);
    // Asked again on the same tip: the tip it finished with.
    let (_, again) = round(&r, None);
    assert!(again.unwrap().contains("awaiting delivery"), "said again");
    assert_eq!(made(&r), 1);
    // Main moves with no source of the bundle's: the same key, nothing made, the same task.
    let moved = on_main(&r, "sim/other.gd", "extends Node\n");
    let (said, result) = round(&r, None);
    assert_eq!(
        result.unwrap(),
        format!(
            "the bundle: made of main at {}, awaiting delivery by pull request as {slug} (`just \
             land {slug}`, once its stamps stand): nothing is made again of the same inputs",
            &first.tip[..7],
            slug = first.slug
        ),
        "{said}"
    );
    assert_eq!(made(&r), 1);
    assert_eq!(pending(&r).unwrap(), first);
    assert_eq!(head(&r.root, "main"), moved);
    assert_eq!(r.board.store.tasks(None).unwrap().len(), 1, "one task");
}

#[test]
fn the_land_delivers_it_as_generated_and_the_next_round_finds_the_default_branch_current() {
    let (r, bare) = generating(true);
    let (made_as, orch) = assessed(&r);
    // The host's default branch moved meanwhile, by no source of the bundle's: its key is read
    // again there, and what was made is still what the generators make of it.
    let moved = on_main(&r, "sim/other.gd", "extends Node\n");
    let host = Host::new(&r, &bare);
    let (said, result) = landed(&r, &made_as.slug, &orch, &host);
    let line = result.unwrap_or_else(|why| panic!("{why}\n{said}"));
    let squash = head(&bare, "main");
    assert!(
        line.starts_with(&format!(
            "landed {} on main at {}",
            made_as.slug,
            &squash[..12]
        )),
        "{line}"
    );
    assert_eq!(
        host.verbs(),
        ["publish", "arm", "merge", "result", "landed"]
    );
    assert_eq!(host.request("merge")["method"], "squash");
    let receipt = Receipt::read(&lands(&r), &made_as.slug).unwrap();
    assert_eq!(receipt.operation, "generated");
    assert_eq!(
        (receipt.head, receipt.parent),
        (made_as.head.clone(), moved)
    );
    // The checkout follows the host, which holds the bundle and what main gained.
    assert_eq!(head(&r.root, "main"), squash);
    assert_eq!(git(&r.root, &["show", "main:gen/out.txt"]), "a");
    assert_eq!(git(&r.root, &["show", "main:sim/other.gd"]), "extends Node");
    assert_eq!(status(&r, &made_as.slug), "done");
    // The round a land starts: nothing to make, nothing pending, no second task.
    let (said, result) = round(&r, None);
    assert_eq!(
        result.unwrap(),
        format!("the bundle: current on main at {}", &squash[..7]),
        "{said}"
    );
    assert!(
        said.contains(&format!("{} was delivered as", made_as.slug)),
        "{said}"
    );
    assert_eq!(made(&r), 1);
    assert!(pending(&r).is_none());
    assert!(regenerate::status(&r.env(&r.root)).unwrap().current);
    assert_eq!(r.board.store.tasks(None).unwrap().len(), 1);
}

#[test]
fn a_stale_one_is_refused_by_the_land_before_anything_is_published() {
    for key in [true, false] {
        let (r, bare) = generating(key);
        let (made_as, orch) = assessed(&r);
        // A source of the bundle's landed since: what was made is not what main makes now.
        let moved = on_main(&r, "src/b.txt", "b\n");
        let host = Host::new(&r, &bare);
        let (said, result) = landed(&r, &made_as.slug, &orch, &host);
        let refused = result.unwrap_err();
        assert!(
            refused.starts_with(&format!(
                "{} is stale: it was made of main at {}, origin/main is at {} and ",
                made_as.slug,
                &made_as.tip[..7],
                &moved[..7]
            )) && refused.ends_with("; nothing published"),
            "{refused}\n{said}"
        );
        assert!(host.verbs().is_empty(), "{:?}", host.verbs());
        assert_eq!(head(&bare, "main"), moved);
        assert!(Receipt::read(&lands(&r), &made_as.slug).is_none());
        assert_ne!(status(&r, &made_as.slug), "done");
    }
}

#[test]
fn inputs_that_changed_cancel_its_task_tell_its_pull_request_and_make_the_newest_tip() {
    let (r, bare) = generating(true);
    let (first, orch) = assessed(&r);
    // Bound to a pull request whose merge the host refused.
    let host = Host::new(&r, &bare).merging(Merges::Refuses("checks"));
    landed(&r, &first.slug, &orch, &host).1.unwrap_err();
    assert!(
        Journal::read(&lands(&r), &first.slug)
            .unwrap()
            .binding
            .is_some()
    );
    let asked = host.verbs().len();
    let moved = on_main(&r, "src/b.txt", "b\n");
    let (said, result) = round(&r, Some(&host));
    let second = pending(&r).unwrap();
    assert!(result.unwrap().contains(&second.slug), "{said}");
    assert_ne!(second.slug, first.slug);
    assert_eq!((second.tip.as_str(), made(&r)), (moved.as_str(), 2));
    assert_eq!(
        git(&r.root, &["show", &format!("{}:gen/out.txt", second.head)]),
        "a\nb"
    );
    // The first is cancelled, saying why, and its pull request's checks no longer pass.
    assert_eq!(status(&r, &first.slug), "cancelled");
    assert!(
        (r.board.said(&first.slug).iter())
            .any(|line| line.contains("what it delivers is stale: main is at")),
        "{:?}",
        r.board.said(&first.slug)
    );
    assert_eq!(host.verbs()[asked..], ["result", "publish"]);
    assert_eq!(host.request("publish")["snapshot"]["eligible"], false);
    assert_eq!(status(&r, &second.slug), "review");
}

#[test]
fn a_generator_with_no_key_is_made_again_once_any_other_path_changed() {
    let (r, _bare) = generating(false);
    round(&r, None).1.unwrap();
    let first = pending(&r).unwrap();
    on_main(&r, "sim/other.gd", "extends Node\n");
    round(&r, None).1.unwrap();
    assert_eq!(made(&r), 2);
    assert_ne!(pending(&r).unwrap().slug, first.slug);
    assert_eq!(status(&r, &first.slug), "cancelled");
}

#[test]
fn a_merge_asked_for_is_never_superseded_and_a_delivery_not_followed_here_is_waited_for() {
    let (r, bare) = generating(true);
    let (made_as, orch) = assessed(&r);
    // The merge's answer is lost and so is the reading after it: asked for, its result unknown.
    let host = Host::new(&r, &bare).losing(&["merge", "result"]);
    landed(&r, &made_as.slug, &orch, &host).1.unwrap_err();
    assert_eq!(
        Journal::read(&lands(&r), &made_as.slug).unwrap().stage,
        Stage::MergeRequested
    );
    // Main moves here with a new source: the round supersedes nothing the host may hold.
    write(&r.root, "src/b.txt", "b\n");
    git(&r.root, &["add", "-A"]);
    git(&r.root, &["commit", "-qm", "a source, here alone"]);
    let (said, result) = round(&r, None);
    assert!(result.unwrap().contains("awaiting delivery"), "{said}");
    assert_eq!(
        (made(&r), status(&r, &made_as.slug).as_str()),
        (1, "review")
    );
    // The land run again proves the commit; the checkout, which diverged, cannot follow it yet.
    let refused = landed(&r, &made_as.slug, &orch, &host).1.unwrap_err();
    assert!(refused.contains("reconciliation pending"), "{refused}");
    let (_, result) = round(&r, None);
    let line = result.unwrap();
    assert!(
        line.contains("which main here does not hold yet") && line.contains(&made_as.slug),
        "{line}"
    );
    assert_eq!(made(&r), 1);
    assert_eq!(pending(&r).unwrap(), made_as);
}

#[test]
fn a_head_that_is_not_what_the_regenerator_made_is_refused_by_name() {
    let (r, bare) = generating(true);
    let (made_as, orch) = assessed(&r);
    let branch = format!("refs/heads/{}", made_as.slug);
    let host = Host::new(&r, &bare);
    let refused = |r: &Repo| {
        let assessor = r.board.session("reviewer");
        let tip = head(&r.root, &branch);
        assess(r, &made_as.slug, &assessor, &[], &tip);
        landed(r, &made_as.slug, &orch, &host).1.unwrap_err()
    };
    let keep = |pending: &Pending| pending.write(&lands(&r)).unwrap();
    // A commit of somebody's on its branch: not the commit made.
    let tree = head(&r.root, &format!("{}^{{tree}}", made_as.head));
    let more = git(
        &r.root,
        &["commit-tree", &tree, "-p", &made_as.head, "-m", "more"],
    );
    git(&r.root, &["update-ref", &branch, &more]);
    let line = refused(&r);
    assert!(
        line.ends_with(&format!(
            "'s head {} is not the commit the regenerator made ({})",
            &more[..7],
            &made_as.head[..7]
        )),
        "{line}"
    );
    // The record made to name it: two commits past the tip, not one on it.
    keep(&Pending {
        head: more.clone(),
        ..made_as.clone()
    });
    let line = refused(&r);
    assert!(
        line.ends_with(&format!(
            "is not one commit on {}, which origin/main holds",
            &made_as.tip[..7]
        )),
        "{line}"
    );
    // One commit on the tip that changes a source beside the outputs.
    let wt = r.add("stray", None, &made_as.tip);
    write(&wt, "gen/out.txt", "a\n");
    write(&wt, "src/a.txt", "changed\n");
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-qm", "outputs, and a source"]);
    let stray = head(&wt, "HEAD");
    git(&r.root, &["update-ref", &branch, &stray]);
    keep(&Pending {
        head: stray,
        ..made_as.clone()
    });
    let line = refused(&r);
    assert!(
        line.ends_with("changes src/a.txt, outside the generators' outputs"),
        "{line}"
    );
    // A generator the project no longer has made it.
    git(&r.root, &["update-ref", &branch, &made_as.head]);
    let mut renamed = made_as.clone();
    renamed.made[0].generator = "another".into();
    keep(&renamed);
    let line = refused(&r);
    assert!(
        line.contains("the project's generators are not the ones that made it"),
        "{line}"
    );
    assert!(host.verbs().is_empty(), "{:?}", host.verbs());
}

#[test]
fn a_task_no_round_made_is_never_delivered_as_generated() {
    let (r, bare) = generating(true);
    round(&r, None).1.unwrap();
    // An ordinary task that writes a generator's outputs is refused as it was.
    let (people, _, tip) = in_review(&r, SLUG, "gen/out.txt");
    assess(&r, SLUG, &people.assessor, &[], &tip);
    let host = Host::new(&r, &bare);
    let refused = landed(&r, SLUG, &people.orch, &host).1.unwrap_err();
    assert!(
        refused.contains("changes 1 generated path (gen/out.txt)"),
        "{refused}"
    );
}

/// A round stopped right after it kept its record: a branch named `generated` is in the way of
/// every branch under it, so the ref cannot be made. The record, and the repository.
fn cut_after_the_record() -> (Repo, PathBuf, Pending) {
    let (r, bare) = generating(true);
    let tip = head(&r.root, "main");
    git(&r.root, &["update-ref", "refs/heads/generated", &tip]);
    let stopped = round(&r, None).1.unwrap_err();
    assert!(stopped.contains("update-ref"), "{stopped}");
    git(&r.root, &["update-ref", "-d", "refs/heads/generated"]);
    let kept = pending(&r).unwrap();
    assert_eq!(made(&r), 1);
    assert!(r.board.store.get(&kept.slug).is_err(), "no task yet");
    (r, bare, kept)
}

/// The next run after a cut: it files what the first made, once, and makes nothing.
fn assert_filed_once(r: &Repo, kept: &Pending) {
    let (said, result) = round(r, None);
    assert!(result.unwrap().contains("awaiting delivery"), "{said}");
    assert_eq!(made(r), 1, "{said}");
    assert_eq!(pending(r).as_ref(), Some(kept));
    assert_eq!(
        head(&r.root, &format!("refs/heads/{}", kept.slug)),
        kept.head
    );
    assert_eq!(status(r, &kept.slug), "review");
    assert_eq!(r.board.store.tasks(None).unwrap().len(), 1);
    // And once more changes nothing.
    round(r, None).1.unwrap();
    assert_eq!((made(r), r.board.store.tasks(None).unwrap().len()), (1, 1));
}

#[test]
fn a_round_cut_after_its_record_files_what_it_made_and_makes_nothing_again() {
    let (r, _bare, kept) = cut_after_the_record();
    assert!(!regenerate::status(&r.env(&r.root)).unwrap().current);
    assert_filed_once(&r, &kept);
}

#[test]
fn a_round_cut_after_its_branch_or_its_task_goes_on_from_there() {
    use symphony_board::NewTask;
    // The branch made, no task.
    let (r, _bare, kept) = cut_after_the_record();
    let branch = format!("refs/heads/{}", kept.slug);
    git(&r.root, &["update-ref", &branch, &kept.head]);
    assert_filed_once(&r, &kept);
    // The task filed, not claimed.
    let (r, _bare, kept) = cut_after_the_record();
    let store = &r.board.store;
    let filer = r.board.session("filer");
    store
        .create(&NewTask::new(&kept.slug, "Regenerate", &filer))
        .unwrap();
    git(
        &r.root,
        &[
            "update-ref",
            &format!("refs/heads/{}", kept.slug),
            &kept.head,
        ],
    );
    assert_filed_once(&r, &kept);
    // Claimed, not yet in review, the session that claimed it gone.
    let (r, _bare, kept) = cut_after_the_record();
    let store = &r.board.store;
    let filer = r.board.session("filer");
    store
        .create(&NewTask::new(&kept.slug, "Regenerate", &filer))
        .unwrap();
    store.claim(&kept.slug, &filer).unwrap();
    git(
        &r.root,
        &[
            "update-ref",
            &format!("refs/heads/{}", kept.slug),
            &kept.head,
        ],
    );
    assert_filed_once(&r, &kept);
}

#[test]
fn the_host_merging_onto_a_base_that_gained_an_input_is_never_taken_as_current() {
    let (r, bare) = generating(true);
    let (first, orch) = assessed(&r);
    // The last reading here finds the inputs it was made of; a source lands on the host between
    // that and the host's own merge, which no check here pins.
    let host = Host::new(&r, &bare).before("merge", || {
        on_main(&r, "src/b.txt", "b\n");
    });
    landed(&r, &first.slug, &orch, &host).1.unwrap();
    let squash = head(&bare, "main");
    let receipt = Receipt::read(&lands(&r), &first.slug).unwrap();
    assert_ne!(receipt.parent, first.tip);
    assert_eq!(receipt.operation, "generated");
    // What main holds is the bundle of one source beside two: delivered, and stale.
    assert_eq!(git(&r.root, &["show", "main:gen/out.txt"]), "a");
    let standing = regenerate::status(&r.env(&r.root)).unwrap();
    assert!(!standing.current, "{}", standing.line);
    // The round the land starts says so and makes the newest tip, as a task of its own.
    let (said, result) = round(&r, None);
    assert!(
        said.contains(&format!(
            "{} was delivered as {}, onto {} where ",
            first.slug,
            &squash[..7],
            &receipt.parent[..7]
        )) && said
            .contains("what main holds of them is stale, and is made again of the newest tip"),
        "{said}"
    );
    let second = pending(&r).unwrap();
    assert!(result.unwrap().contains(&second.slug));
    assert_eq!((second.tip.as_str(), made(&r)), (squash.as_str(), 2));
    assert_eq!(
        git(&r.root, &["show", &format!("{}:gen/out.txt", second.head)]),
        "a\nb"
    );
    assert!(!regenerate::status(&r.env(&r.root)).unwrap().current);
}

#[test]
fn a_pull_request_that_could_not_be_told_is_told_by_a_later_round() {
    let (r, bare) = generating(true);
    let (first, orch) = assessed(&r);
    let host = Host::new(&r, &bare).merging(Merges::Refuses("checks"));
    landed(&r, &first.slug, &orch, &host).1.unwrap_err();
    on_main(&r, "src/b.txt", "b\n");
    // The host gives no answer while the round cancels the task: it is cancelled all the same,
    // and nothing says its pull request is blocked.
    let deaf = Host::new(&r, &bare).losing(&["result"]);
    let (said, result) = round(&r, Some(&deaf));
    result.unwrap();
    assert_eq!(status(&r, &first.slug), "cancelled");
    assert!(
        said.contains("was not told its evidence no longer stands")
            && said.contains("its checks may still pass; the next round tells it"),
        "{said}"
    );
    let named = fs::read_to_string(lands(&r).join(WITHDRAWN)).unwrap();
    assert_eq!(named, first.slug);
    // A later round, of the same tip or another, tells it, once.
    let asked = host.verbs().len();
    round(&r, Some(&host)).1.unwrap();
    assert_eq!(host.verbs()[asked..], ["result", "publish"]);
    assert_eq!(host.request("publish")["snapshot"]["eligible"], false);
    assert_eq!(host.request("publish")["slug"], first.slug.as_str());
    assert!(!lands(&r).join(WITHDRAWN).exists());
    assert_eq!(made(&r), 2);
}

#[test]
fn a_key_seen_before_is_made_again_once_the_outputs_are_another_keys() {
    let (r, bare) = generating(true);
    let deliver = |r: &Repo| {
        let (made_as, orch) = assessed(r);
        let host = Host::new(r, &bare);
        landed(r, &made_as.slug, &orch, &host).1.unwrap();
        round(r, None).1.unwrap();
        assert!(pending(r).is_none());
    };
    deliver(&r);
    on_main(&r, "src/b.txt", "b\n");
    deliver(&r);
    assert_eq!(made(&r), 2);
    // The first key again, over the second's outputs: made, and delivered, a third time.
    git(&r.root, &["rm", "-q", "src/b.txt"]);
    git(&r.root, &["commit", "-qm", "back to one source"]);
    git(&r.root, &["push", "-q", "origin", "main"]);
    deliver(&r);
    assert_eq!(made(&r), 3);
    assert_eq!(git(&r.root, &["show", "main:gen/out.txt"]), "a");
    assert!(regenerate::status(&r.env(&r.root)).unwrap().current);
}

#[test]
fn a_land_of_a_delivered_task_run_again_starts_the_regenerator_it_never_started() {
    let (r, bare) = generating(true);
    let (people, wt, _) = in_review(&r, SLUG, "sim/jump.gd");
    write(&wt, "src/b.txt", "b\n");
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-qm", "a source of the bundle's"]);
    let tip = head(&wt, "HEAD");
    assess(&r, SLUG, &people.assessor, &[], &tip);
    let host = Host::new(&r, &bare);
    // Delivered and closed, its journal gone, by a land that started no regenerator.
    landed(&r, SLUG, &people.orch, &host).1.unwrap();
    assert!(!Journal::path(&lands(&r), SLUG).exists());
    let squash = head(&bare, "main");
    let vars = [
        (RECIPE, "1"),
        ("TASKS_REGENERATE", "on"),
        ("TASKS_BIN", env!("CARGO_BIN_EXE_tasks")),
    ];
    let again = Host::new(&r, &bare);
    let (said, result) = landed_with(&r, SLUG, &people.orch, &again, &vars);
    result.unwrap_err();
    assert!(again.verbs().is_empty(), "nothing asked of the host");
    assert!(
        said.contains(&format!(
            "land {SLUG}: delivered already; the regenerator makes the bundle in the background"
        )),
        "{said}"
    );
    started_and_filed(&r);
    assert_eq!(
        (pending(&r).unwrap().tip.as_str(), made(&r)),
        (squash.as_str(), 1)
    );
    assert_eq!(landings(&r, SLUG).len(), 1);
}

/// Wait for the regenerator a land started detached to file what it made.
fn started_and_filed(r: &Repo) {
    let log = lands(r).join("regenerate.log");
    let waited = test_time::until_true(
        "the regenerator the land started files what it made",
        test_time::backstop(Duration::from_secs(60)),
        || {
            (fs::read_to_string(&log).unwrap_or_default().lines())
                .any(|line| line.contains("nothing committed to main"))
        },
    );
    waited.unwrap_or_else(|stall| {
        panic!("{stall}: {}", fs::read_to_string(&log).unwrap_or_default())
    });
}

#[test]
fn a_task_somebody_cancelled_is_not_filed_again_while_its_inputs_stay() {
    let (r, _bare) = generating(true);
    round(&r, None).1.unwrap();
    let made_as = pending(&r).unwrap();
    let orch = r.board.session("orchestrator");
    let store = &r.board.store;
    let _ = store.take_role(&made_as.slug, &orch, "orchestrator");
    store.move_to(&made_as.slug, "cancelled", &orch).unwrap();
    on_main(&r, "sim/other.gd", "extends Node\n");
    let line = round(&r, None).1.unwrap();
    assert!(
        line.contains(
            "was cancelled: nothing is made again until what the generators are made of changes"
        ),
        "{line}"
    );
    assert_eq!((made(&r), store.tasks(None).unwrap().len()), (1, 1));
    // Its inputs change: the newest tip is made, as a task of its own.
    on_main(&r, "src/b.txt", "b\n");
    round(&r, None).1.unwrap();
    assert_eq!((made(&r), store.tasks(None).unwrap().len()), (2, 2));
}

/// A land of a task changing a source of the bundle's, delivered, its journal put back where a
/// run killed right after it wrote `Dispatching` left it: what follows a land never started.
#[test]
fn a_land_cut_before_it_started_the_regenerator_loses_no_output_and_makes_none_twice() {
    let (r, bare) = generating(true);
    let (people, wt, _) = in_review(&r, SLUG, "sim/jump.gd");
    write(&wt, "src/b.txt", "b\n");
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-qm", "a source of the bundle's"]);
    let tip = head(&wt, "HEAD");
    assess(&r, SLUG, &people.assessor, &[], &tip);
    let host = Host::new(&r, &bare);
    // This board starts no regenerator unless told to: the land's own start is the one cut.
    landed(&r, SLUG, &people.orch, &host).1.unwrap();
    let squash = head(&bare, "main");
    assert!(pending(&r).is_none() && made(&r) == 0);
    let receipt = Receipt::read(&lands(&r), SLUG).unwrap();
    Journal {
        slug: SLUG.into(),
        operation: "task".into(),
        stage: Stage::Dispatching,
        head: receipt.head,
        base_observed: String::new(),
        revision: String::new(),
        title: String::new(),
        body: String::new(),
        by: people.orch.clone(),
        closes: vec!["review".into()],
        stood: String::new(),
        binding: Some(symphony_board::land::delivery::Binding {
            repository: 7,
            name: "owner/name".into(),
            pull: 3,
            url: String::new(),
        }),
        squash: Some(squash.clone()),
        held: None,
        import: None,
        green: None,
        armed: false,
    }
    .write(&lands(&r))
    .unwrap();
    // The land run again starts nothing of the land's twice, and the regenerator, which is safe to.
    let vars = [
        (RECIPE, "1"),
        ("TASKS_REGENERATE", "on"),
        ("TASKS_BIN", env!("CARGO_BIN_EXE_tasks")),
    ];
    let (said, result) = landed_with(&r, SLUG, &people.orch, &host, &vars);
    result.unwrap();
    assert!(said.contains("is not started twice"), "{said}");
    assert!(
        said.contains(&format!(
            "land {SLUG}: the regenerator makes the bundle in the background"
        )),
        "{said}"
    );
    assert_eq!(landings(&r, SLUG).len(), 1);
    started_and_filed(&r);
    let made_as = pending(&r).unwrap();
    assert_eq!((made_as.tip.as_str(), made(&r)), (squash.as_str(), 1));
    assert_eq!(
        git(&r.root, &["show", &format!("{}:gen/out.txt", made_as.head)]),
        "a\nb"
    );
    // Any later start, a land's or this one again: the same record, nothing made, one task.
    let (_, again) = round(&r, None);
    assert!(again.unwrap().contains("awaiting delivery"));
    assert_eq!(made(&r), 1);
    assert_eq!(pending(&r).unwrap(), made_as);
}

#[test]
fn a_later_round_alone_makes_what_a_land_that_started_none_left_behind() {
    let (r, bare) = generating(true);
    let (first, orch) = assessed(&r);
    let host = Host::new(&r, &bare);
    // Delivered by a land that started no regenerator, then a source lands the same way.
    landed(&r, &first.slug, &orch, &host).1.unwrap();
    let moved = {
        git(&r.root, &["pull", "-q", "--ff-only", "origin", "main"]);
        on_main(&r, "src/b.txt", "b\n")
    };
    // No list of builds is kept: the default branch's ref and the keys passed on are all it reads.
    let (said, result) = round(&r, None);
    let second = pending(&r).unwrap();
    assert!(result.unwrap().contains(&second.slug), "{said}");
    assert_eq!((second.tip.as_str(), made(&r)), (moved.as_str(), 2));
    assert_ne!(second.slug, first.slug);
}

#[test]
fn with_no_delivery_key_a_round_commits_and_keeps_no_pending_record() {
    let r = Repo::new();
    write(&r.root, "src/a.txt", "a\n");
    git(&r.root, &["add", "src"]);
    let text = manifest(&r, true).replace("delivery = [\"host\", \"deliver\"]\n", "");
    project_file(&r, &text);
    let before = head(&r.root, "main");
    let (said, result) = round(&r, None);
    let line = result.unwrap();
    assert!(
        line.starts_with(&format!(
            "Regenerate the bundle for main at {}: landed on main at ",
            &before[..7]
        )),
        "{line}\n{said}"
    );
    assert_eq!(head(&r.root, "main^"), before);
    assert!(pending(&r).is_none());
    assert!(
        r.board.store.tasks(None).unwrap().is_empty(),
        "no task filed"
    );
}

#[test]
fn an_input_landing_between_the_publication_and_the_request_asks_for_no_merge() {
    let (r, bare) = generating(true);
    let (made_as, orch) = assessed(&r);
    // Fresh when it is first judged; a source lands while it is published.
    let host = Host::new(&r, &bare).before("publish", || {
        on_main(&r, "src/b.txt", "b\n");
    });
    let refused = landed(&r, &made_as.slug, &orch, &host).1.unwrap_err();
    assert!(
        refused.contains("is stale: it was made of main at"),
        "{refused}"
    );
    assert_eq!(host.verbs(), ["publish"], "no merge asked");
    assert!(Receipt::read(&lands(&r), &made_as.slug).is_none());
}

#[test]
fn the_land_waits_for_a_regenerator_that_runs_when_it_must_read_a_key() {
    let (r, bare) = generating(true);
    let (made_as, orch) = assessed(&r);
    on_main(&r, "sim/other.gd", "extends Node\n");
    let lock = fs::File::open(lands(&r).join("regenerate.lock")).unwrap();
    lock.try_lock().unwrap();
    let host = Host::new(&r, &bare);
    let refused = landed(&r, &made_as.slug, &orch, &host).1.unwrap_err();
    assert!(
        refused.starts_with("the regenerator runs, and judges what")
            && refused.contains("nothing published"),
        "{refused}"
    );
    assert!(host.verbs().is_empty());
    drop(lock);
    landed(&r, &made_as.slug, &orch, &host).1.unwrap();
}

#[test]
fn a_record_whose_branch_moved_or_whose_generators_changed_is_not_gone_on_from() {
    let (r, _bare) = generating(true);
    round(&r, None).1.unwrap();
    let made_as = pending(&r).unwrap();
    // Its branch moved by somebody: the round stops and says so, making nothing.
    let branch = format!("refs/heads/{}", made_as.slug);
    git(&r.root, &["update-ref", &branch, &made_as.tip]);
    let stopped = round(&r, None).1.unwrap_err();
    assert!(
        stopped.ends_with(&format!(
            "the branch {} is at {}, not the commit the regenerator made ({}): move it away, and \
             the next round files it",
            made_as.slug,
            &made_as.tip[..7],
            &made_as.head[..7]
        )),
        "{stopped}"
    );
    assert_eq!(made(&r), 1);
    git(&r.root, &["update-ref", &branch, &made_as.head]);
    // A generator the project no longer has made it: cancelled, and the tip made by today's.
    let mut renamed = made_as.clone();
    renamed.made[0].generator = "another".into();
    renamed.write(&lands(&r)).unwrap();
    let (said, result) = round(&r, None);
    // The same tip, another commit made of it: a name of its own, the cancelled task's left.
    let again = pending(&r).unwrap();
    assert!(result.unwrap().contains(&again.slug), "{said}");
    assert_ne!(again.slug, made_as.slug);
    assert!(
        again
            .slug
            .starts_with(&format!("generated/outputs-at-{}-", &made_as.tip[..12]))
    );
    assert_eq!(
        (made(&r), status(&r, &made_as.slug).as_str()),
        (2, "cancelled")
    );
}

/// A task bound to a pull request whose merge the host refused, then cancelled as stale by a
/// round the host gave no answer to: the repository, the host's bare, and the task.
fn withdrawn_untold() -> (Repo, PathBuf, Pending) {
    let (r, bare) = generating(true);
    let (first, orch) = assessed(&r);
    {
        let host = Host::new(&r, &bare).merging(Merges::Refuses("checks"));
        landed(&r, &first.slug, &orch, &host).1.unwrap_err();
        on_main(&r, "src/b.txt", "b\n");
        let deaf = Host::new(&r, &bare).losing(&["result"]);
        round(&r, Some(&deaf)).1.unwrap();
    }
    assert!(lands(&r).join(WITHDRAWN).exists());
    (r, bare, first)
}

#[test]
fn a_cancelled_tasks_pull_request_is_forgotten_only_once_it_is_told_or_no_longer_open() {
    // Closed on the host: nothing is left to tell.
    let (r, bare, _) = withdrawn_untold();
    let host = Host::new(&r, &bare);
    host.pull.borrow_mut().2 = true;
    round(&r, Some(&host)).1.unwrap();
    assert_eq!(host.verbs(), ["result"]);
    assert!(!lands(&r).join(WITHDRAWN).exists());
    // Merged all the same, by a person: nothing is left to tell, and nothing is published to it.
    let (r, bare, first) = withdrawn_untold();
    let host = Host::new(&r, &bare);
    *host.pull.borrow_mut() = (Some(first.head.clone()), Some(first.head.clone()), false);
    round(&r, Some(&host)).1.unwrap();
    assert_eq!(host.verbs(), ["result"]);
    assert!(!lands(&r).join(WITHDRAWN).exists());
    assert_eq!(status(&r, &first.slug), "cancelled");
    // The host takes no publication, or answers what is no state: still to tell, and said.
    for answer in [
        (
            "publish",
            json!({"state": "unavailable", "why": "no token"}),
        ),
        ("result", json!({"state": "unavailable", "why": "no token"})),
    ] {
        let (r, bare, first) = withdrawn_untold();
        let host = Host::new(&r, &bare).saying(answer.0, answer.1);
        let (said, result) = round(&r, Some(&host));
        result.unwrap();
        assert!(
            said.contains(&format!(
                "the pull request of {}, which was cancelled, was not told",
                first.slug
            )),
            "{said}"
        );
        assert!(lands(&r).join(WITHDRAWN).exists());
        assert!(!host.verbs().contains(&"merge".to_string()));
    }
}

#[test]
fn a_project_file_the_remote_default_does_not_commit_refuses_a_generated_one_too() {
    let (r, bare) = generating(true);
    let (made_as, orch) = assessed(&r);
    // Committed here and pushed nowhere: the generators and keys read here are not the remote's.
    project_file(&r, &format!("{}push = false\n", manifest(&r, true)));
    let host = Host::new(&r, &bare);
    let refused = landed(&r, &made_as.slug, &orch, &host).1.unwrap_err();
    assert!(
        refused.starts_with("symphony.toml here is not the one origin/main commits at"),
        "{refused}"
    );
    assert!(host.verbs().is_empty(), "nothing published");
    assert!(Journal::read(&lands(&r), &made_as.slug).is_none());
}

#[test]
fn a_record_gone_between_the_publication_and_the_request_asks_for_no_merge() {
    let (r, bare) = generating(true);
    let (made_as, orch) = assessed(&r);
    // What a round that found it stale does to the record, while it is published.
    let host = Host::new(&r, &bare).before("publish", || {
        fs::remove_file(lands(&r).join(PENDING)).unwrap();
    });
    let refused = landed(&r, &made_as.slug, &orch, &host).1.unwrap_err();
    assert!(
        refused.contains(&format!(
            "{} is no longer what the regenerator delivers: no merge asked",
            made_as.slug
        )),
        "{refused}"
    );
    assert_eq!(host.verbs(), ["publish"], "no merge asked");
    assert!(Receipt::read(&lands(&r), &made_as.slug).is_none());
}

#[test]
fn a_stale_record_the_board_never_took_goes_with_its_branch() {
    // Its branch made, no task, and an input landed since.
    let (r, _bare, kept) = cut_after_the_record();
    let branch = format!("refs/heads/{}", kept.slug);
    git(&r.root, &["update-ref", &branch, &kept.head]);
    on_main(&r, "src/b.txt", "b\n");
    let (said, result) = round(&r, None);
    let again = pending(&r).unwrap();
    assert!(result.unwrap().contains(&again.slug), "{said}");
    assert_ne!(again.slug, kept.slug);
    assert_eq!(git(&r.root, &["for-each-ref", &branch]), "", "{said}");
    assert!(
        r.board.store.get(&kept.slug).is_err(),
        "nothing filed for it"
    );
    // A branch somebody moved is theirs: it stays.
    let (r, _bare, kept) = cut_after_the_record();
    let branch = format!("refs/heads/{}", kept.slug);
    git(&r.root, &["update-ref", &branch, &kept.tip]);
    on_main(&r, "src/b.txt", "b\n");
    round(&r, None).1.unwrap();
    assert_eq!(head(&r.root, &branch), kept.tip);
}

#[test]
fn a_withdrawal_named_before_its_task_was_cancelled_says_why_it_is_not_told_yet() {
    let (r, bare) = generating(true);
    let (first, orch) = assessed(&r);
    let host = Host::new(&r, &bare).merging(Merges::Refuses("checks"));
    landed(&r, &first.slug, &orch, &host).1.unwrap_err();
    // A round cut after it named the task, before it cancelled it; an input landed since.
    fs::write(lands(&r).join(WITHDRAWN), &first.slug).unwrap();
    on_main(&r, "src/b.txt", "b\n");
    let host = Host::new(&r, &bare);
    let (said, result) = round(&r, Some(&host));
    result.unwrap();
    assert!(
        said.contains(&format!(
            "was not told its evidence no longer stands ({} is review and its evidence stands at \
             {}): its checks may still pass",
            first.slug,
            &first.head[..7]
        )),
        "{said}"
    );
    // The round cancels it, then tells it.
    assert_eq!(status(&r, &first.slug), "cancelled");
    assert_eq!(host.verbs(), ["result", "result", "publish"]);
    assert_eq!(host.request("publish")["snapshot"]["eligible"], false);
    assert!(!lands(&r).join(WITHDRAWN).exists());
}

#[test]
fn a_check_that_passed_on_a_tip_is_not_run_again_while_its_generated_task_waits() {
    let (r, _bare) = generating(true);
    let checks = r.dir.join("checks");
    let check = format!(
        "[verdicts]\ncheck = [\"sh\", \"-c\", \"echo checked >> '{}'\"]\n\n",
        checks.display()
    );
    project_file(&r, &format!("{check}{}", manifest(&r, true)));
    let ran = || {
        fs::read_to_string(&checks)
            .unwrap_or_default()
            .lines()
            .count()
    };
    let tip = head(&r.root, "main");
    round(&r, None).1.unwrap();
    assert_eq!(ran(), 1);
    // Started again while its task waits: the same tip, the pass kept.
    let (said, result) = round(&r, None);
    assert!(result.unwrap().contains("awaiting delivery"), "{said}");
    assert_eq!(ran(), 1, "{said}");
    assert!(
        said.contains(&format!("on main at {} before: not run again", &tip[..7])),
        "{said}"
    );
    // A tip that changed what is made is made and checked.
    on_main(&r, "src/b.txt", "b\n");
    round(&r, None).1.unwrap();
    assert_eq!(ran(), 2);
}
