//! A land delivered by pull request of a head that imports the project's upstream
//! (`[land.upstream]`, docs/workflow/TASKS.md "Imports"): merged by a merge commit only where the project
//! file, the head's assessment at that head and the history fetched from the upstream agree, one
//! import after another; a task beside it squashed as ever, and refused with an import while the
//! upstream does not answer; and what refuses before anything is asked, what is held once merged,
//! and what a lost answer or a refused merge leaves.

use super::*;
use std::net::{TcpListener, TcpStream};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use symphony_board::land::{NETWORK, land_by_with_clock};

/// What the land's line says after the reason an upstream was not fetched: that every land reads
/// it, and what the lander does.
const UNFETCHED: &str = "every land reads it while symphony.toml on main names it \
                         ([land.upstream]), a task's as well as an import's, so nothing landed. \
                         Land again once it answers; an address that no longer answers is \
                         corrected on main by the project's owner, as no land delivers it";

/// The project file of a project that delivers by pull request and imports `upstream`'s main.
fn manifest(upstream: &Path) -> String {
    format!(
        "{DELIVERS}\n[land.upstream]\nurl = \"{}\"\nbranch = \"main\"\n",
        upstream.display()
    )
}

/// A checkout of its own at `at`, cloned from `from`, that commits as `who`.
fn cloned(r: &Repo, from: &Path, at: &Path, who: &str) {
    let (from, at) = (from.to_string_lossy(), at.to_string_lossy());
    git(&r.dir, &["clone", "-q", "-b", "main", &from, &at]);
    let email = format!("{who}@example.com");
    for (key, value) in [
        ("user.email", email.as_str()),
        ("user.name", who),
        ("commit.gpgsign", "false"),
    ] {
        git(Path::new(at.as_ref()), &["config", key, value]);
    }
}

/// A repository delivering by pull request whose default branch was seeded from an upstream and
/// imports from it: the repository, its host, the upstream's checkout and the seed, the commit
/// both histories share.
fn importing() -> (Repo, PathBuf, PathBuf, String) {
    let r = Repo::new();
    let upstream = r.dir.join("upstream");
    cloned(&r, &r.root, &upstream, "u");
    let seed = head(&upstream, "HEAD");
    project_file(&r, &manifest(&upstream));
    let bare = remote(&r);
    (r, bare, upstream, seed)
}

/// A commit on `dir`'s branch writing `path`: its whole name.
fn commit(dir: &Path, path: &str) -> String {
    write(dir, path, &format!("{path}\n"));
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", &format!("writes {path}")]);
    head(dir, "HEAD")
}

/// `wt`'s branch merging `commit` from `upstream`'s `branch`, as an executor imports it: the head.
fn merges(wt: &Path, upstream: &Path, branch: &str, commit: &str) -> String {
    git(wt, &["fetch", "-q", &upstream.to_string_lossy(), branch]);
    let message = format!("import {}", &commit[..7]);
    git(wt, &["merge", "-q", "--no-ff", "-m", &message, commit]);
    head(wt, "HEAD")
}

/// `slug`'s needs written at `tip` as `tests` and `review`, naming the import of `import`.
fn assess_import(r: &Repo, slug: &str, by: &str, tip: &str, import: Option<&str>) {
    let needs = Needs {
        stamps: vec!["tests".into(), "review".into()],
        tip: Some(tip.into()),
        by: by.into(),
        why: "an import of the upstream".into(),
        import: import.map(str::to_string),
    };
    (r.board.store)
        .set_needs(slug, &Catalogue::symphony(), &needs)
        .unwrap();
}

/// `slug` ready to deliver, its branch a commit of its own then a merge of the upstream's
/// `commit`, assessed at that head naming `named` the commit it imports, its tests green and a
/// fresh reviewer's approve naming it: the people, the worktree and the head.
fn import_ready(
    r: &Repo,
    slug: &str,
    upstream: &Path,
    commit: &str,
    named: Option<&str>,
) -> (People, PathBuf, String) {
    let (people, wt, _) = in_review(r, slug, &format!("sim/{}.gd", slug.replace('/', "-")));
    let tip = merges(&wt, upstream, "main", commit);
    stand(r, slug, &people, &tip, named);
    (people, wt, tip)
}

/// `slug`'s evidence standing at `tip`: assessed there naming `named`, green and approved.
fn stand(r: &Repo, slug: &str, people: &People, tip: &str, named: Option<&str>) {
    assess_import(r, slug, &people.assessor, tip, named);
    ran(r, slug, tip, "cargo test -p sim", 0);
    approve(r, slug, &format!("{tip}: reads well"));
}

/// The land of [`SLUG`] refused before its host was asked anything, nothing landed: its line, and
/// the last post on its thread with whom it names.
fn refused_unasked(
    r: &Repo,
    bare: &Path,
    orch: &str,
    before: &str,
) -> (String, String, Option<String>) {
    let host = Host::new(r, bare);
    let (_, result) = landed(r, SLUG, orch, &host);
    let line = result.unwrap_err();
    assert!(host.verbs().is_empty(), "{line}: {:?}", host.verbs());
    assert_nothing_landed(r, bare, before);
    let post = r.board.store.thread(SLUG).unwrap().pop().unwrap();
    (line, post.text, post.recipient)
}

#[test]
fn an_import_is_merged_by_a_merge_commit_and_the_next_one_follows_from_it() {
    let (r, bare, upstream, seed) = importing();
    let first = commit(&upstream, "up/a.txt");
    let (people, _, tip) = import_ready(&r, SLUG, &upstream, &first, Some(&first));
    let base = head(&bare, "main");
    let host = Host::new(&r, &bare);
    let line = landed(&r, SLUG, &people.orch, &host).1.unwrap();
    assert_eq!(
        host.verbs(),
        ["publish", "arm", "merge", "result", "landed"]
    );
    // Each publication and merge says what it imports, from where.
    let url = upstream.to_string_lossy().into_owned();
    let told = json!({"url": url, "branch": "main", "commit": first});
    for verb in ["publish", "arm", "merge"] {
        let request = host.request(verb);
        assert_eq!(request["operation"], "import", "{verb}");
        assert_eq!(request["upstream"], told, "{verb}");
    }
    let merge = host.request("merge");
    assert_eq!(merge["method"], "merge");
    // Titled as a land's own merge, which the release cadence counts.
    assert_eq!(merge["title"], format!("Merge branch '{SLUG}'"));
    let asking = format!("land {SLUG}: asking owner/name to merge {tip} by pull request #3");
    let said = r.board.said(SLUG);
    assert!(
        said.iter().any(|line| line.starts_with(&asking)),
        "{said:?}"
    );
    // One merge commit, the base then the head, the upstream's commit in its history.
    let merged = head(&bare, "main");
    assert_eq!(head(&r.root, "main"), merged, "the checkout follows");
    assert_eq!(
        git(&r.root, &["rev-list", "--parents", "-n1", "main"]),
        format!("{merged} {base} {tip}")
    );
    assert_eq!(git(&r.root, &["merge-base", &first, "main"]), first);
    assert_eq!(
        line,
        format!(
            "landed {SLUG} on main at {}; merged from {tip}, an import of {first}, by pull \
             request #3 of owner/name",
            &merged[..12]
        )
    );
    assert_eq!(r.board.store.get(SLUG).unwrap().status, "done");
    let receipt = Receipt::read(&lands(&r), SLUG).unwrap();
    assert_eq!(receipt.operation, "import");
    assert_eq!(
        (receipt.parent.as_str(), receipt.squash.as_str()),
        (base.as_str(), merged.as_str())
    );
    assert_eq!(receipt.tree, head(&r.root, "main^{tree}"));
    let import = receipt.import.unwrap();
    assert_eq!(import.parents, [base, tip.clone()]);
    assert_eq!(
        (import.url.as_str(), import.branch.as_str()),
        (url.as_str(), "main")
    );
    assert_eq!(import.repository, 9, "the upstream the program pinned");
    assert_eq!(
        (import.before.as_str(), import.commit.as_str()),
        (seed.as_str(), first.as_str())
    );
    assert_eq!(
        (import.tip.as_str(), import.after.as_str()),
        (first.as_str(), first.as_str())
    );

    // The next import follows from this one: the upstream's newer commit, imported on a default
    // branch that holds the first.
    let second = commit(&upstream, "up/b.txt");
    let (people, _, tip) = import_ready(&r, "sim/again", &upstream, &second, Some(&second));
    let host = Host::new(&r, &bare);
    let (_, result) = landed(&r, "sim/again", &people.orch, &host);
    result.unwrap();
    assert_eq!(head(&r.root, "main^2"), tip);
    let import = Receipt::read(&lands(&r), "sim/again")
        .unwrap()
        .import
        .unwrap();
    assert_eq!(
        (import.before.as_str(), import.after.as_str()),
        (first.as_str(), second.as_str())
    );
    assert_eq!(git(&r.root, &["merge-base", &second, "main"]), second);
}

#[test]
fn a_task_in_a_project_that_imports_is_squashed_as_ever() {
    let (r, bare, _, _) = importing();
    let (people, _, tip) = ready(&r, SLUG);
    let base = head(&bare, "main");
    let host = Host::new(&r, &bare);
    landed(&r, SLUG, &people.orch, &host).1.unwrap();
    for verb in ["publish", "arm", "merge"] {
        let request = host.request(verb);
        assert_eq!(request["operation"], "task", "{verb}");
        assert!(request.get("upstream").is_none(), "{verb}: {request}");
    }
    assert_eq!(host.request("merge")["method"], "squash");
    let squash = head(&bare, "main");
    assert_eq!(
        git(&r.root, &["rev-list", "--parents", "-n1", "main"]),
        format!("{squash} {base}")
    );
    let receipt = Receipt::read(&lands(&r), SLUG).unwrap();
    assert_eq!(
        (receipt.operation.as_str(), receipt.head.as_str()),
        ("task", tip.as_str())
    );
    assert!(receipt.import.is_none());
}

#[test]
fn an_upstream_that_does_not_answer_refuses_a_tasks_land_as_well_and_publishes_nothing() {
    // An address with no repository there: git says so at once.
    let r = Repo::new();
    let gone = r.dir.join("gone");
    project_file(&r, &manifest(&gone));
    let bare = remote(&r);
    let before = head(&bare, "main");
    let (people, ..) = ready(&r, SLUG);
    let (line, ..) = refused_unasked(&r, &bare, &people.orch, &before);
    let named = format!("the upstream {} was not fetched: ", gone.display());
    assert!(line.starts_with(&named), "{line}");
    assert!(line.ends_with(&format!(": {UNFETCHED}")), "{line}");

    // An address that takes the fetch and never answers: refused once the land's network timeout
    // passes, the line naming the upstream and not origin.
    let r = Repo::new();
    let silent = TcpListener::bind("127.0.0.1:0").unwrap();
    silent.set_nonblocking(true).unwrap();
    let url = format!("git://{}/upstream", silent.local_addr().unwrap());
    let upstream = format!("[land.upstream]\nurl = \"{url}\"\nbranch = \"main\"\n");
    project_file(&r, &format!("{DELIVERS}{upstream}"));
    let bare = remote(&r);
    let before = head(&bare, "main");
    let (people, ..) = ready(&r, SLUG);
    // Time stands still until the address holds the fetch; from then on each look at the clock is
    // past the network timeout of any deadline read before it.
    let (began, held, ticks) = (Instant::now(), Mutex::new(None), AtomicU64::new(0));
    let clock = || {
        let mut held = held.lock().unwrap();
        if held.is_none() {
            *held = silent.accept().ok().map(|(fetch, _): (TcpStream, _)| fetch);
        }
        let looks = match *held {
            Some(_) => ticks.fetch_add(1, Ordering::SeqCst) + 1,
            None => 0,
        };
        began + (NETWORK + Duration::from_secs(1)) * u32::try_from(looks).unwrap()
    };
    let host = Host::new(&r, &bare);
    let env = landing(&r, &[]);
    let result = land_by_with_clock(
        &r.board.store,
        &env,
        SLUG,
        &people.orch,
        &mut Vec::new(),
        &host,
        &clock,
    );
    let line = result.map_err(refusal).unwrap_err();
    assert!(
        held.lock().unwrap().is_some(),
        "the fetch never reached {url}"
    );
    assert_eq!(
        line,
        format!(
            "the upstream {url} was not fetched: no answer within {} s: {UNFETCHED}",
            NETWORK.as_secs()
        )
    );
    assert!(host.verbs().is_empty(), "{:?}", host.verbs());
    assert_nothing_landed(&r, &bare, &before);
}

#[test]
fn a_head_holding_upstream_commits_and_assessed_as_no_import_at_it_is_refused_never_squashed() {
    // Assessed at the head, naming no import.
    let (r, bare, upstream, _) = importing();
    let before = head(&bare, "main");
    let first = commit(&upstream, "up/a.txt");
    let (people, _, tip) = import_ready(&r, SLUG, &upstream, &first, None);
    let (line, told, to) = refused_unasked(&r, &bare, &people.orch, &before);
    let url = upstream.display();
    assert!(
        line.starts_with(&format!(
            "{SLUG}'s head {} holds {}, a commit of the upstream {url} that origin/main does not \
             hold: a task is squashed",
            &tip[..7],
            &first[..7]
        )),
        "{line}"
    );
    assert_eq!(to.as_deref(), Some("@orchestrator"));
    assert!(
        told.contains(&format!(
            "`just tasks needs {SLUG} STAMP… --tip {tip} --import {first}`"
        )),
        "{told}"
    );

    // Assessed as an import at an earlier head: it binds nothing at the head approved.
    let (r, bare, upstream, _) = importing();
    let before = head(&bare, "main");
    let first = commit(&upstream, "up/a.txt");
    let (people, wt, _) = in_review(&r, SLUG, "sim/jump.gd");
    let earlier = merges(&wt, &upstream, "main", &first);
    assess_import(&r, SLUG, &people.assessor, &earlier, Some(&first));
    let tip = commit(&wt, "sim/fix.gd");
    ran(&r, SLUG, &tip, "cargo test -p sim", 0);
    approve(&r, SLUG, &format!("{tip}: reads well"));
    let (line, _, _) = refused_unasked(&r, &bare, &people.orch, &before);
    assert!(
        line.starts_with(&format!("{SLUG}'s head {} holds", &tip[..7])),
        "{line}"
    );
    assert_ne!(earlier, tip);
}

#[test]
fn an_import_named_at_an_earlier_head_binds_nothing_at_one_that_no_longer_imports() {
    let (r, bare, upstream, _) = importing();
    let before = head(&bare, "main");
    let first = commit(&upstream, "up/a.txt");
    let (people, wt, own) = in_review(&r, SLUG, "sim/jump.gd");
    let earlier = merges(&wt, &upstream, "main", &first);
    assess_import(&r, SLUG, &people.assessor, &earlier, Some(&first));
    // The branch drops the import: its head holds no upstream commit, and nobody assessed it.
    git(&wt, &["reset", "-q", "--hard", &own]);
    let tip = commit(&wt, "sim/fix.gd");
    ran(&r, SLUG, &tip, "cargo test -p sim", 0);
    approve(&r, SLUG, &format!("{tip}: reads well"));
    let (line, ..) = refused_unasked(&r, &bare, &people.orch, &before);
    assert_eq!(
        line,
        format!(
            "{SLUG} is not eligible for delivery by pull request at {}: its assessment by {} read \
             {}, not the head {}: an assessor reads the head",
            &tip[..7],
            people.assessor,
            &earlier[..7],
            &tip[..7]
        )
    );
}

#[test]
fn an_import_named_where_no_upstream_is_or_where_its_history_says_otherwise_is_refused() {
    // The project file names no upstream: no import is delivered, whatever the assessment says.
    let (r, bare) = delivering();
    let before = head(&bare, "main");
    let elsewhere = r.dir.join("elsewhere");
    cloned(&r, &r.root, &elsewhere, "e");
    let theirs = commit(&elsewhere, "up/a.txt");
    let (people, ..) = import_ready(&r, SLUG, &elsewhere, &theirs, Some(&theirs));
    let (line, _, to) = refused_unasked(&r, &bare, &people.orch, &before);
    assert_eq!(
        line,
        format!(
            "{SLUG}'s assessment names an import of {}, and symphony.toml on main names no \
             upstream ([land.upstream]): no import is delivered here",
            &theirs[..7]
        )
    );
    assert_eq!(to.as_deref(), Some("@orchestrator"));

    // A commit off the upstream's branch.
    let (r, bare, upstream, _) = importing();
    let before = head(&bare, "main");
    git(&upstream, &["checkout", "-q", "-b", "side"]);
    let side = commit(&upstream, "up/side.txt");
    git(&upstream, &["checkout", "-q", "main"]);
    let (people, wt, _) = in_review(&r, SLUG, "sim/jump.gd");
    let tip = merges(&wt, &upstream, "side", &side);
    stand(&r, SLUG, &people, &tip, Some(&side));
    let (line, ..) = refused_unasked(&r, &bare, &people.orch, &before);
    assert!(
        line.starts_with(&format!(
            "{} is not on {}'s main as fetched now",
            &side[..7],
            upstream.display()
        )),
        "{line}"
    );

    // A commit the default branch holds already.
    let (r, bare, _, seed) = importing();
    let before = head(&bare, "main");
    let (people, _, tip) = in_review(&r, SLUG, "sim/jump.gd");
    stand(&r, SLUG, &people, &tip, Some(&seed));
    let (line, ..) = refused_unasked(&r, &bare, &people.orch, &before);
    assert_eq!(
        line,
        format!(
            "origin/main holds {} already: nothing to import",
            &seed[..7]
        )
    );

    // An upstream with no history in common.
    let r = Repo::new();
    let unrelated = r.dir.join("unrelated");
    let at = unrelated.to_string_lossy().into_owned();
    git(&r.dir, &["init", "-q", "-b", "main", &at]);
    for (key, value) in [("user.email", "n@example.com"), ("user.name", "n")] {
        git(&unrelated, &["config", key, value]);
    }
    let theirs = commit(&unrelated, "up/a.txt");
    project_file(&r, &manifest(&unrelated));
    let bare = remote(&r);
    let before = head(&bare, "main");
    let (people, wt, _) = in_review(&r, SLUG, "sim/jump.gd");
    git(&wt, &["fetch", "-q", &at, "main"]);
    let message = "import unrelated";
    git(
        &wt,
        &[
            "merge",
            "-q",
            "--no-ff",
            "--allow-unrelated-histories",
            "-m",
            message,
            &theirs,
        ],
    );
    let tip = head(&wt, "HEAD");
    stand(&r, SLUG, &people, &tip, Some(&theirs));
    let (line, ..) = refused_unasked(&r, &bare, &people.orch, &before);
    assert!(
        line.starts_with(&format!("main and {at}'s main share no commit")),
        "{line}"
    );

    // An upstream that cannot be fetched.
    let r = Repo::new();
    let gone = r.dir.join("gone");
    let elsewhere = r.dir.join("elsewhere");
    cloned(&r, &r.root, &elsewhere, "e");
    project_file(&r, &manifest(&gone));
    let bare = remote(&r);
    let before = head(&bare, "main");
    let theirs = commit(&elsewhere, "up/a.txt");
    let (people, ..) = import_ready(&r, SLUG, &elsewhere, &theirs, Some(&theirs));
    let (line, ..) = refused_unasked(&r, &bare, &people.orch, &before);
    assert!(
        line.starts_with(&format!(
            "the upstream {} was not fetched: ",
            gone.display()
        )),
        "{line}"
    );
}

#[test]
fn an_import_whose_head_is_not_its_branchs_own_merge_of_the_upstream_is_refused() {
    // The upstream's commit merged on a side line, not the branch's own.
    let (r, bare, upstream, _) = importing();
    let before = head(&bare, "main");
    let first = commit(&upstream, "up/a.txt");
    let (people, wt, own) = in_review(&r, SLUG, "sim/jump.gd");
    let side = merges(&wt, &upstream, "main", &first);
    git(&wt, &["reset", "-q", "--hard", &own]);
    git(&wt, &["merge", "-q", "--no-ff", "-m", "the side", &side]);
    let tip = head(&wt, "HEAD");
    stand(&r, SLUG, &people, &tip, Some(&first));
    let (line, ..) = refused_unasked(&r, &bare, &people.orch, &before);
    assert!(
        line.starts_with(&format!(
            "{SLUG}'s head {} merges {} on no commit of its own first-parent line",
            &tip[..7],
            &first[..7]
        )),
        "{line}"
    );

    // Beside the import, a commit neither the branch's own nor the upstream's.
    let (r, bare, upstream, _) = importing();
    let before = head(&bare, "main");
    git(&upstream, &["checkout", "-q", "-b", "side"]);
    let foreign = commit(&upstream, "up/side.txt");
    git(&upstream, &["checkout", "-q", "main"]);
    let first = commit(&upstream, "up/a.txt");
    let (people, wt, _) = in_review(&r, SLUG, "sim/jump.gd");
    merges(&wt, &upstream, "main", &first);
    let tip = merges(&wt, &upstream, "side", &foreign);
    stand(&r, SLUG, &people, &tip, Some(&first));
    let (line, ..) = refused_unasked(&r, &bare, &people.orch, &before);
    assert!(
        line.starts_with(&format!(
            "{SLUG}'s head {} holds {}, a commit neither of its own first-parent line nor of \
             {}'s history",
            &tip[..7],
            &foreign[..7],
            &first[..7]
        )),
        "{line}"
    );

    // An upstream commit older than what the default branch holds of the upstream: one that
    // forked before the last import and was merged into the upstream after it.
    let (r, bare, upstream, seed) = importing();
    let earlier = commit(&upstream, "up/a.txt");
    git(
        &r.root,
        &["fetch", "-q", &upstream.to_string_lossy(), "main"],
    );
    git(
        &r.root,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "an earlier import",
            &earlier,
        ],
    );
    git(&r.root, &["push", "-q", "origin", "main"]);
    let before = head(&bare, "main");
    git(&upstream, &["checkout", "-q", "-b", "late", &seed]);
    let late = commit(&upstream, "up/late.txt");
    git(&upstream, &["checkout", "-q", "main"]);
    git(&upstream, &["merge", "-q", "--no-ff", "-m", "late", &late]);
    let (people, wt, _) = in_review(&r, SLUG, "sim/jump.gd");
    let tip = merges(&wt, &upstream, "main", &late);
    stand(&r, SLUG, &people, &tip, Some(&late));
    let (line, ..) = refused_unasked(&r, &bare, &people.orch, &before);
    assert!(
        line.starts_with(&format!(
            "{}, where main and {}'s main last met, is not an ancestor of {}",
            &earlier[..7],
            upstream.display(),
            &late[..7]
        )),
        "{line}"
    );
}

#[test]
fn an_import_the_host_squashed_is_held_and_closes_nothing() {
    let (r, bare, upstream, _) = importing();
    let first = commit(&upstream, "up/a.txt");
    let (people, ..) = import_ready(&r, SLUG, &upstream, &first, Some(&first));
    let local = head(&r.root, "main");
    let host = Host::new(&r, &bare).merging(Merges::Squash);
    let line = landed(&r, SLUG, &people.orch, &host).1.unwrap_err();
    assert!(
        line.starts_with(&format!(
            "the delivery of {SLUG} is held for reconciliation: "
        )),
        "{line}"
    );
    assert!(
        line.contains(&format!(
            "has one parent: the import of {first} was squashed, its files on main without the \
             upstream's commit in its history. A new import task of the same commit"
        )),
        "{line}"
    );
    assert_eq!(
        head(&r.root, "main"),
        local,
        "the checkout follows nothing unproved"
    );
    assert!(landings(&r, SLUG).is_empty());
    assert!(Receipt::read(&lands(&r), SLUG).is_none());
    assert_eq!(r.board.store.get(SLUG).unwrap().status, "done");
}

#[test]
fn an_import_merged_with_its_parents_in_another_order_is_held() {
    let (r, bare, upstream, _) = importing();
    let first = commit(&upstream, "up/a.txt");
    let (people, _, tip) = import_ready(&r, SLUG, &upstream, &first, Some(&first));
    let base = head(&bare, "main");
    let host = Host::new(&r, &bare).merging(Merges::Reversed);
    let line = landed(&r, SLUG, &people.orch, &host).1.unwrap_err();
    let made = head(&bare, "main");
    assert!(
        line.contains(&format!(
            "{} has the parents {}, {}, not the base then the head {}: what landed is not the \
             import that was read",
            &made[..7],
            &tip[..7],
            &base[..7],
            &tip[..7]
        )),
        "{line}"
    );
    assert!(landings(&r, SLUG).is_empty());
    assert!(Receipt::read(&lands(&r), SLUG).is_none());
}

#[test]
fn a_delivery_command_that_pins_no_upstream_merges_no_import() {
    let (r, bare, upstream, _) = importing();
    let before = head(&bare, "main");
    let first = commit(&upstream, "up/a.txt");
    let (people, ..) = import_ready(&r, SLUG, &upstream, &first, Some(&first));
    // As a program that does not know imports answers: published, and no upstream pinned.
    let host = Host::new(&r, &bare).saying("publish", json!({"upstream": null}));
    let line = landed(&r, SLUG, &people.orch, &host).1.unwrap_err();
    assert!(
        line.contains(&format!(
            "the publication of the import of {} pins no upstream: the delivery command does not \
             deliver imports; nothing merged",
            &first[..7]
        )),
        "{line}"
    );
    assert!(
        !host.verbs().iter().any(|verb| verb == "merge"),
        "{:?}",
        host.verbs()
    );
    assert_nothing_landed(&r, &bare, &before);
}

#[test]
fn an_assessment_that_changes_its_import_while_it_is_published_merges_nothing() {
    let (r, bare, upstream, _) = importing();
    let before = head(&bare, "main");
    let first = commit(&upstream, "up/a.txt");
    // Delivered from review, needing no stamp, so its assessor may write again meanwhile.
    let (people, wt, _) = in_review(&r, SLUG, "sim/jump.gd");
    let tip = merges(&wt, &upstream, "main", &first);
    let assessed = |import: Option<&str>| Needs {
        stamps: vec!["none".into()],
        tip: Some(tip.clone()),
        by: people.assessor.clone(),
        why: "nothing to stamp".into(),
        import: import.map(str::to_string),
    };
    let store = &r.board.store;
    let catalogue = Catalogue::symphony();
    store
        .set_needs(SLUG, &catalogue, &assessed(Some(&first)))
        .unwrap();
    let host = Host::new(&r, &bare).before("publish", || {
        store.set_needs(SLUG, &catalogue, &assessed(None)).unwrap();
    });
    let line = landed(&r, SLUG, &people.orch, &host).1.unwrap_err();
    assert_eq!(
        line,
        format!(
            "the assessment of {SLUG} changed what it says {} imports while it was delivered: \
             nothing merged; run the land again",
            &tip[..7]
        )
    );
    assert!(
        !host.verbs().iter().any(|verb| verb == "merge"),
        "{:?}",
        host.verbs()
    );
    assert_nothing_landed(&r, &bare, &before);
}

#[test]
fn an_import_whose_merge_answer_was_lost_is_adopted_and_one_refused_names_who_acts() {
    let (r, bare, upstream, _) = importing();
    let first = commit(&upstream, "up/a.txt");
    let (people, _, tip) = import_ready(&r, SLUG, &upstream, &first, Some(&first));
    let host = Host::new(&r, &bare).losing(&["merge", "result"]);
    let line = landed(&r, SLUG, &people.orch, &host).1.unwrap_err();
    assert!(
        line.contains("was asked for and its result is not known"),
        "{line}"
    );
    assert!(landings(&r, SLUG).is_empty());
    // The next run reads the pull request, asks for no second merge, and proves the import.
    let line = landed(&r, SLUG, &people.orch, &host).1.unwrap();
    assert_eq!(
        host.verbs(),
        ["publish", "arm", "merge", "result", "result", "landed"]
    );
    assert!(
        line.contains(&format!(
            "; merged from {tip}, an import of {first}, by pull request #3"
        )),
        "{line}"
    );
    assert_eq!(head(&r.root, "main^2"), tip);
    let receipt = Receipt::read(&lands(&r), SLUG).unwrap();
    assert_eq!(receipt.import.unwrap().commit, first);

    // A merge the host refuses for a conflict goes back to the executor, by the method asked.
    let (r, bare, upstream, _) = importing();
    let before = head(&bare, "main");
    let first = commit(&upstream, "up/a.txt");
    let (people, _, tip) = import_ready(&r, SLUG, &upstream, &first, Some(&first));
    let host = Host::new(&r, &bare).merging(Merges::Refuses("conflict"));
    let line = landed(&r, SLUG, &people.orch, &host).1.unwrap_err();
    assert_eq!(
        line,
        format!(
            "owner/name refused to merge {SLUG} at {} by pull request #3: no: conflict",
            &tip[..7]
        )
    );
    assert_nothing_landed(&r, &bare, &before);
    let post = r.board.store.thread(SLUG).unwrap().pop().unwrap();
    assert_eq!(post.recipient.as_deref(), Some("@executor"));
}

#[test]
fn an_import_merged_on_a_base_that_moved_is_proved_on_the_base_it_was_merged_on() {
    let (r, bare, upstream, _) = importing();
    let first = commit(&upstream, "up/a.txt");
    let (people, _, tip) = import_ready(&r, SLUG, &upstream, &first, Some(&first));
    // The host's main gains a commit between the publication and the merge.
    let other = r.dir.join("other");
    cloned(&r, &bare, &other, "o");
    let moved = commit(&other, "sim/other.gd");
    let host = Host::new(&r, &bare).before("merge", || {
        git(&other, &["push", "-q", "origin", "main"]);
    });
    landed(&r, SLUG, &people.orch, &host).1.unwrap();
    let receipt = Receipt::read(&lands(&r), SLUG).unwrap();
    assert_eq!(
        receipt.parent, moved,
        "merged on the base as it was when merged"
    );
    assert_eq!(
        receipt.import.unwrap().parents,
        [moved.clone(), tip.clone()]
    );
    assert_eq!(git(&r.root, &["merge-base", &moved, "main"]), moved);
}
