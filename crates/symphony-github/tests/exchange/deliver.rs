//! `symphony-github deliver`, the verbs a land delivered by a pull request asks: what each
//! writes to GitHub and in what order, that a second run and a lost answer make nothing twice,
//! what is refused before anything is sent, and that the token is in nothing said or kept.
//! GitHub's auto-merge, `arm`, `disarm` and `close`, is [`crate::armed`]'s.

use crate::github::{Fake, Fault};
use crate::{Rig, TOKEN};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use symphony_github::credential::Secret;
use symphony_github::deliver::{self, Remote};
use symphony_github::reconcile::Context;
use symphony_github::state::{Mapping, Origin};
use symphony_github::transport::{Failure, Request, Response, Transport};

/// The number of the one GitHub App, whose token the settings' `token_file` holds: it pushes,
/// writes the checks and merges.
pub(crate) const APP: i64 = 42;
pub(crate) const SLUG: &str = "sim/jump";
pub(crate) const HEAD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// The fake repository, asked as the App of the token a request carries.
pub(crate) struct As(pub(crate) Arc<Fake>, pub(crate) i64);

impl Transport for As {
    fn send(&self, request: &Request) -> Result<Response, Failure> {
        let (response, lost) = (self.0).handle_as(
            self.1,
            request.method,
            &request.path,
            request.body.as_deref(),
        );
        if lost {
            Err(Failure::Unknown(
                "the connection ended with no answer".into(),
            ))
        } else {
            Ok(response)
        }
    }
}

/// The project's git: where `origin` leads, and what was pushed.
pub(crate) struct Pushes {
    pub(crate) url: String,
    github: Arc<Fake>,
    refuses: bool,
    pub(crate) pushed: RefCell<Vec<(String, String)>>,
}

impl Remote for Pushes {
    fn url(&self) -> Result<String, String> {
        Ok(self.url.clone())
    }

    fn push(&self, head: &str, branch: &str) -> Result<(), String> {
        if self.refuses {
            return Err(" ! [rejected] (non-fast-forward)".into());
        }
        self.pushed.borrow_mut().push((head.into(), branch.into()));
        self.github.push(branch, head);
        Ok(())
    }
}

/// What makes a request with a token.
pub(crate) type Connect = Box<dyn Fn(Secret) -> Result<Box<dyn Transport>, String>>;

/// A delivery's rig: the exchange's, its settings serving `publish`, and every line it said.
pub(crate) struct Delivery {
    pub(crate) rig: Rig,
    pub(crate) remote: Pushes,
    said: RefCell<String>,
    /// What its requests are made through: as the App whose token each carries, unless a test
    /// puts GitHub's Apps in front ([`crate::minting`]).
    pub(crate) connect: Connect,
    /// The program that signs a GitHub App's JWT.
    pub(crate) openssl: &'static str,
    /// Each rest its runs asked for between two reads, none of which waited.
    pub(crate) rested: RefCell<Vec<Duration>>,
}

/// The fake gh's script, written once for every test: it answers from the folder it is linked
/// from. macOS checks an executable newly written at its first run, which takes a good part of a
/// second; a link to one already run is not checked again.
fn gh_script() -> &'static Path {
    static SCRIPT: OnceLock<PathBuf> = OnceLock::new();
    SCRIPT.get_or_init(|| {
        let text = "#!/bin/sh\nf=\"$(dirname \"$0\")/$(printf %s \"$4\" | tr -c 'A-Za-z0-9' '_').json\"\n\
                    if [ \"$1 $2 $3\" != \"api -H Accept: application/vnd.github+json\" ]; then \
                    echo \"gh: not a read: $*\" >&2; exit 2; fi\n\
                    if [ -f \"$f\" ]; then cat \"$f\"; else echo 'gh: Not Found (HTTP 404)' >&2; \
                    exit 1; fi\n";
        let script = Path::new(env!("CARGO_TARGET_TMPDIR")).join("symphony-github-gh");
        if std::fs::read_to_string(&script).ok().as_deref() != Some(text) {
            // Written beside and moved in whole, as another test run may be running it.
            let written = script.with_extension(std::process::id().to_string());
            std::fs::write(&written, text).unwrap();
            std::fs::set_permissions(&written, std::fs::Permissions::from_mode(0o755)).unwrap();
            std::fs::rename(&written, &script).unwrap();
        }
        script
    })
}

fn settings(more: &str) -> String {
    format!("pull_requests = \"publish\"\napp_id = {APP}\n{more}")
}

/// What makes a request as the App whose token it carries.
fn connect(github: &Arc<Fake>) -> impl Fn(Secret) -> Result<Box<dyn Transport>, String> + use<> {
    let github = Arc::clone(github);
    move |token: Secret| -> Result<Box<dyn Transport>, String> {
        let app = if token.expose() == TOKEN { APP } else { 0 };
        Ok(Box::new(As(Arc::clone(&github), app)))
    }
}

impl Delivery {
    /// A delivery to a repository protected as one needs it, its rulesets verified.
    pub(crate) fn new(name: &str, more: &str) -> Delivery {
        let delivery = Delivery::unverified(name, more);
        delivery.verify().ok();
        delivery
    }

    /// The same, its rulesets never verified.
    pub(crate) fn unverified(name: &str, more: &str) -> Delivery {
        let rig = Rig::off(name);
        rig.github.protect(APP);
        rig.enable(&settings(more));
        let remote = Pushes {
            url: "git@github.com:o/n.git".into(),
            github: Arc::clone(&rig.github),
            refuses: false,
            pushed: RefCell::default(),
        };
        let connect: Connect = Box::new(connect(&rig.github));
        Delivery {
            rig,
            remote,
            said: RefCell::default(),
            connect,
            openssl: "openssl",
            rested: RefCell::default(),
        }
    }

    /// A gh CLI whose `api PATH` answers what GitHub answers a person now, who administers the
    /// repository: its answers written again at each call, from the fake as it is then, beside a
    /// link to the one script ([`gh_script`]).
    pub(crate) fn gh(&self) -> PathBuf {
        let dir = self.rig.dir.0.join("gh");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut paths = vec![
            "repos/o/n".to_string(),
            "repos/o/n/rules/branches/main?per_page=100&page=1".to_string(),
        ];
        paths.extend(
            (self.rig.github.ruleset_ids().iter()).map(|id| format!("repos/o/n/rulesets/{id}")),
        );
        let name = |path: &str| -> String {
            path.chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect()
        };
        for path in &paths {
            let read = self.rig.github.person_reads(&format!("/{path}"));
            if read.status == 200 {
                std::fs::write(dir.join(format!("{}.json", name(path))), &read.body).unwrap();
            }
        }
        let gh = dir.join("gh");
        std::os::unix::fs::symlink(gh_script(), &gh).unwrap();
        gh
    }

    /// `rulesets ARGS` run with [`Delivery::gh`].
    pub(crate) fn rulesets(&self, args: &[&str]) -> crate::Ran {
        let gh = self.gh();
        let mut all = vec!["rulesets"];
        all.extend_from_slice(args);
        crate::run_gh(
            &all,
            &self.rig.board,
            &self.rig.settings,
            &self.connect,
            self.rig.now.get(),
            vec![self.rig.dir.0.join("repository")],
            gh.to_str().unwrap(),
        )
    }

    /// `rulesets --verify`, as who administers the repository runs it at setup.
    pub(crate) fn verify(&self) -> crate::Ran {
        self.rulesets(&["--verify"])
    }

    /// `symphony-github ARGS`, each request made as the App whose token it carries.
    pub(crate) fn run(&self, args: &[&str]) -> crate::Ran {
        crate::run(
            args,
            &self.rig.board,
            &self.rig.settings,
            &self.connect,
            self.rig.now.get(),
            vec![self.rig.dir.0.join("repository")],
        )
    }

    /// The answer to `verb` of `request`.
    pub(crate) fn ask(&self, verb: &str, request: &Value) -> Value {
        let file = self.rig.dir.0.join("request.json");
        std::fs::write(&file, request.to_string()).unwrap();
        let ctx = Context {
            source: &self.rig.settings,
            board: &self.rig.board,
            connect: &self.connect,
            now: self.rig.now.get(),
            repository: vec![self.rig.dir.0.join("repository")],
            openssl: self.openssl,
            gh: "gh",
            rest: &|wait| self.rested.borrow_mut().push(wait),
        };
        let mut out = Vec::new();
        let code = deliver::run(&ctx, &self.remote, verb, &file, &mut out).unwrap();
        assert_eq!(code, 0);
        let out = String::from_utf8(out).unwrap();
        self.said.borrow_mut().push_str(&out);
        assert_eq!(out.lines().count(), 1, "one line of JSON: {out}");
        serde_json::from_str(&out).unwrap()
    }

    /// `publish` of `snapshot` as the board asks it while its journal binds no pull request:
    /// `pull` is `null`.
    pub(crate) fn publish(&self, snapshot: &Value) -> Value {
        self.publish_naming(snapshot, None)
    }

    /// [`Delivery::publish`] naming `pull`, the pull request the land's journal binds, or none.
    pub(crate) fn publish_naming(&self, snapshot: &Value, pull: Option<i64>) -> Value {
        self.publish_saying(snapshot, Some(json!(pull)))
    }

    /// [`Delivery::publish`] whose `pull` is `pull`, or with no `pull` at all, as a board older
    /// than the field asks.
    pub(crate) fn publish_saying(&self, snapshot: &Value, pull: Option<Value>) -> Value {
        let mut request = json!({
            "verb": "publish", "slug": SLUG, "by": "amber-fox-1", "head": snapshot["head"],
            "branch": "main", "title": "sim/jump: the private title",
            "body": "the private brief", "snapshot": snapshot,
        });
        if let Some(pull) = pull {
            request["pull"] = pull;
        }
        self.ask("publish", &request)
    }

    pub(crate) fn merge(&self, pull: i64) -> Value {
        self.ask(
            "merge",
            &json!({
                "verb": "merge", "slug": SLUG, "by": "amber-fox-1", "head": HEAD,
                "repository": 77, "pull": pull, "method": "squash",
                "title": "sim/jump: the private title", "body": "the private brief",
            }),
        )
    }

    /// `arm` of pull request `pull` at `head`, squashed.
    pub(crate) fn arm(&self, pull: i64, head: &str) -> Value {
        self.ask(
            "arm",
            &json!({
                "verb": "arm", "slug": SLUG, "by": "amber-fox-1", "head": head,
                "repository": 77, "pull": pull, "method": "squash",
                "title": "sim/jump: the private title", "body": "the private brief",
            }),
        )
    }

    /// `verb` (`disarm` or `close`) of pull request `pull`.
    pub(crate) fn end(&self, verb: &str, pull: i64) -> Value {
        let request = json!({"verb": verb, "slug": SLUG, "repository": 77, "pull": pull});
        self.ask(verb, &request)
    }

    pub(crate) fn result(&self, pull: i64) -> Value {
        let request = json!({"verb": "result", "slug": SLUG, "repository": 77, "pull": pull});
        self.ask("result", &request)
    }

    /// The requests made that change the repository: each one's method and path.
    pub(crate) fn writes(&self) -> Vec<String> {
        (self.rig.github.asked().into_iter())
            .filter(|(method, ..)| method != "GET")
            .map(|(method, path, _)| format!("{method} {path}"))
            .collect()
    }

    /// The token is in nothing said, kept or asked for by its address.
    pub(crate) fn assert_no_token(&self) {
        self.assert_none_of(&[TOKEN]);
    }

    /// None of `secrets` is in anything said, kept or asked for by its address.
    pub(crate) fn assert_none_of(&self, secrets: &[&str]) {
        let folder = self.rig.dir.0.join("state");
        let mut texts = vec![self.said.borrow().clone()];
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_file() {
                texts.push(std::fs::read_to_string(&path).unwrap());
            }
        }
        for (method, path, body) in self.rig.github.asked() {
            texts.push(format!("{method} {path} {}", body.unwrap_or_default()));
        }
        for text in texts {
            for secret in secrets {
                assert!(!text.contains(secret), "{text}");
            }
        }
    }
}

/// The evidence of [`SLUG`] at `head`: `tests` and `review` as given, the others waived.
pub(crate) fn snapshot(head: &str, tests: &str, revision: &str) -> Value {
    let eligible = tests == "met";
    json!({
        "version": 1, "slug": SLUG, "task": 7, "status": "done", "head": head, "branch": "main",
        "policy": "p", "catalogue": "c", "assessed": true, "eligible": eligible,
        "assessment": {"by": "calm-owl-2", "tip": head, "why": "a private reason", "stamps": []},
        "stamps": [
            {"name": "tests", "standing": tests, "detail": "a private line", "evidence": []},
            {"name": "review", "standing": "met", "detail": "by a private handle", "evidence": []},
            {"name": "pictures", "standing": "waived", "detail": "", "evidence": []},
        ],
        "blocking": [], "revision": revision,
    })
}

pub(crate) fn names(checks: &[(String, String, i64)]) -> Vec<(&str, &str)> {
    (checks.iter())
        .map(|(name, conclusion, _)| (name.as_str(), conclusion.as_str()))
        .collect()
}

#[test]
fn nothing_is_asked_of_github_unless_the_settings_serve_publish() {
    for more in [None, Some("pull_requests = \"track\""), Some("")] {
        let d = Delivery::new("deliver-off", "");
        match more {
            None => d.rig.settings.set(None),
            Some(more) => d.rig.enable(more),
        }
        for verb in [
            "publish", "arm", "merge", "disarm", "close", "result", "landed",
        ] {
            let answer = d.ask(
                verb,
                &json!({"slug": SLUG, "snapshot": snapshot(HEAD, "met", "r1")}),
            );
            assert_eq!(answer["state"], "refused", "{answer}");
            assert!(
                answer["why"].as_str().unwrap().starts_with("turned off: "),
                "{answer}"
            );
        }
        assert_eq!(d.rig.github.asked().len(), 0);
        assert!(d.remote.pushed.borrow().is_empty());
    }
}

#[test]
fn publish_pushes_the_head_opens_one_pull_request_and_writes_each_check_once() {
    let d = Delivery::new("deliver-publish", "");
    let answer = d.publish(&snapshot(HEAD, "met", "r1"));
    assert_eq!(
        answer,
        json!({
            "state": "published", "repository": {"id": 77, "name": "o/n"}, "head": HEAD,
            "pull": {"number": 100, "url": "https://github.com/o/n/pull/100"},
        })
    );
    assert_eq!(
        *d.remote.pushed.borrow(),
        [(HEAD.to_string(), SLUG.to_string())]
    );
    // Its public text is the slug: neither the title nor the brief the land handed over.
    let pulls = d.rig.github.pulls();
    assert_eq!(pulls.len(), 1);
    assert_eq!(pulls[0]["title"], SLUG);
    assert_eq!(
        pulls[0]["draft"], false,
        "ready for review, as GitHub arms no auto-merge on a draft"
    );
    assert_eq!(pulls[0]["body"], "The Symphony task `sim/jump`.");
    assert_eq!(
        (&pulls[0]["head"]["ref"], &pulls[0]["base"]["ref"]),
        (&json!(SLUG), &json!("main"))
    );
    // The stamps first and what says they all stand last, each by the App.
    let checks = d.rig.github.checks(HEAD);
    assert_eq!(
        names(&checks),
        [
            ("symphony/assessment", "success"),
            ("symphony/tests", "success"),
            ("symphony/review", "success"),
            ("symphony/pictures", "neutral"),
            ("symphony/eligible", "success"),
        ]
    );
    assert!(checks.iter().all(|(.., app)| *app == APP));
    let said = format!("{:?}", d.rig.github.asked());
    for private in ["private", "calm-owl-2", "amber-fox-1"] {
        assert!(!said.contains(private), "{private} was sent: {said}");
    }
    // Asked again, it finds each and makes nothing.
    let writes = d.writes();
    assert_eq!(writes.len(), 6, "{writes:?}");
    assert_eq!(d.publish(&snapshot(HEAD, "met", "r1")), answer);
    assert_eq!(d.writes(), writes);
    assert_eq!(d.rig.state().pulls[SLUG].number, 100);
    d.assert_no_token();
}

#[test]
fn a_curated_text_is_the_pull_requests_and_the_commits_public_text() {
    let d = Delivery::new("deliver-curated", "");
    d.rig
        .board
        .file(SLUG, "the private title", "the private brief");
    let body = d.rig.dir.0.join("body.md");
    std::fs::write(&body, "What it does, for the public.\n").unwrap();
    d.rig
        .run(&[
            "publish",
            SLUG,
            "--title",
            "Jump higher",
            "--body-file",
            body.to_str().unwrap(),
        ])
        .ok();
    d.publish(&snapshot(HEAD, "met", "r1"));
    let pull = d.rig.github.pulls().remove(0);
    assert_eq!(
        (&pull["title"], &pull["body"]),
        (
            &json!("Jump higher"),
            &json!("What it does, for the public.")
        )
    );
    assert_eq!(d.merge(100)["state"], "merged");
    let merged = d.rig.github.pulls().remove(0)["merged_as"].clone();
    assert_eq!(
        merged,
        json!({
            "sha": HEAD, "merge_method": "squash", "commit_title": "sim/jump: Jump higher",
            "commit_message": format!("Squashed from {HEAD} by pull request #100."),
        })
    );
}

#[test]
fn a_lost_answer_is_looked_for_before_anything_is_made_again() {
    // The pull request was opened and its answer lost.
    let d = Delivery::new("deliver-lost", "");
    d.rig
        .github
        .fault("POST", "/repos/o/n/pulls", Fault::LoseAnswer);
    let answer = d.publish(&snapshot(HEAD, "met", "r1"));
    assert_eq!(answer["state"], "unavailable", "{answer}");
    assert!(
        answer["why"]
            .as_str()
            .unwrap()
            .contains("the next run looks for it first"),
        "{answer}"
    );
    assert!(
        d.rig.github.checks(HEAD).is_empty(),
        "no check before its pull request is known"
    );
    // A check was written and its answer lost.
    d.rig
        .github
        .fault("POST", "/repos/o/n/check-runs", Fault::LoseAnswer);
    assert_eq!(
        d.publish(&snapshot(HEAD, "met", "r1"))["state"],
        "unavailable"
    );
    assert_eq!(
        d.rig.github.pulls().len(),
        1,
        "found by its head, not opened twice"
    );
    assert_eq!(
        names(&d.rig.github.checks(HEAD)),
        [("symphony/assessment", "success")]
    );
    assert_eq!(
        d.publish(&snapshot(HEAD, "met", "r1"))["state"],
        "published"
    );
    let checks = d.rig.github.checks(HEAD);
    assert_eq!(checks.len(), 5, "each name once: {checks:?}");
    assert_eq!(
        d.writes()
            .iter()
            .filter(|write| write.starts_with("POST /repos/o/n/pulls"))
            .count(),
        1
    );
    let journal: Vec<String> = (d.rig.journal().iter())
        .map(|entry| format!("{} {:?}", entry.kind, entry.event))
        .collect();
    assert!(journal.contains(&"pull Unknown".to_string()), "{journal:?}");
    assert!(
        journal.contains(&"check Unknown".to_string()),
        "{journal:?}"
    );
    d.assert_no_token();
}

#[test]
fn checks_github_lists_a_little_late_are_read_again_and_never_written_again() {
    // Listed from the second read after the writes, and from the last one the bound allows.
    for late in 1..=deliver::READ_BACK.len() {
        let d = Delivery::new("deliver-late", "");
        d.rig.github.list_late(late);
        let answer = d.publish(&snapshot(HEAD, "met", "r1"));
        assert_eq!(
            answer["state"], "published",
            "listed late by {late}: {answer}"
        );
        assert_eq!(*d.rested.borrow(), deliver::READ_BACK[..late]);
        assert_eq!(
            d.rig.github.listings(HEAD),
            late + 2,
            "the read before the writes, and one after each rest and before the first"
        );
        let writes = d.writes();
        assert_eq!(
            writes.len(),
            6,
            "a pull request and five checks: {writes:?}"
        );
        assert_eq!(d.rig.github.checks(HEAD).len(), 5);
    }
}

#[test]
fn a_check_not_listed_within_the_bound_stops_unavailable_and_is_not_written_again() {
    let d = Delivery::new("deliver-unlisted", "");
    let bound = deliver::READ_BACK.len() + 1;
    d.rig.github.list_late(bound);
    let answer = d.publish(&snapshot(HEAD, "met", "r1"));
    assert_eq!(
        answer,
        json!({
            "state": "unavailable",
            "why": format!(
                "the check symphony/assessment does not read back on {HEAD} as App {APP} wrote \
                 it, read again for 15 s"
            ),
        })
    );
    assert_eq!(*d.rested.borrow(), deliver::READ_BACK);
    assert_eq!(d.rig.github.listings(HEAD), 1 + bound);
    let writes = d.writes();
    assert_eq!(writes.len(), 6, "each written once: {writes:?}");
    // The next run reads them standing and writes nothing.
    assert_eq!(
        d.publish(&snapshot(HEAD, "met", "r1"))["state"],
        "published"
    );
    assert_eq!(d.writes(), writes);
    assert_eq!(d.rested.borrow().len(), deliver::READ_BACK.len());
    d.assert_no_token();
}

/// A second head of [`SLUG`], pushed to the pull request its first one opened.
const NEXT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// `verb` (`arm` or `merge`) of pull request `pull` at `head`, squashed.
fn at(d: &Delivery, verb: &str, pull: i64, head: &str) -> Value {
    d.ask(
        verb,
        &json!({
            "verb": verb, "slug": SLUG, "by": "amber-fox-1", "head": head,
            "repository": 77, "pull": pull, "method": "squash",
            "title": "sim/jump: the private title", "body": "the private brief",
        }),
    )
}

#[test]
fn a_head_pushed_to_an_open_pull_request_is_published_once_github_moved_it_there() {
    // Moved by the second read of the pull request, and by the last one the bound allows.
    for late in [1, 2, deliver::READ_BACK.len()] {
        let d = Delivery::new("deliver-head-late", "");
        let pull = d.publish(&snapshot(HEAD, "met", "r1"))["pull"]["number"].clone();
        assert!(d.rested.borrow().is_empty(), "one opened after its push");
        d.rig.github.move_late(late, None);
        let answer = d.publish(&snapshot(NEXT, "met", "r1"));
        assert_eq!(answer["state"], "published", "moved late by {late}");
        assert_eq!(answer["head"], NEXT, "moved late by {late}: {answer}");
        assert_eq!(answer["pull"]["number"], pull);
        assert_eq!(*d.rested.borrow(), deliver::READ_BACK[..late]);
        assert_eq!(d.rig.github.pulls().len(), 1);
        assert_eq!(d.rig.github.checks(NEXT).len(), 5);
    }
}

#[test]
fn a_pull_request_not_moved_within_the_bound_stops_unavailable_and_is_read_first_next() {
    let d = Delivery::new("deliver-head-unmoved", "");
    d.publish(&snapshot(HEAD, "met", "r1"));
    d.rig.github.move_late(5, None);
    let answer = d.publish(&snapshot(NEXT, "met", "r1"));
    assert_eq!(
        answer,
        json!({
            "state": "unavailable",
            "why": "pull request #100 is still at aaaaaaa, not the head bbbbbbb pushed to its \
                    branch, read again for 15 s: the next run reads it first",
        })
    );
    assert_eq!(*d.rested.borrow(), deliver::READ_BACK);
    let writes = d.writes();
    // The next run finds it moved and the checks standing: nothing is written or rested for.
    let answer = d.publish(&snapshot(NEXT, "met", "r1"));
    assert_eq!(answer["state"], "published", "{answer}");
    assert_eq!(answer["head"], NEXT);
    assert_eq!(d.writes(), writes);
    assert_eq!(d.rested.borrow().len(), deliver::READ_BACK.len());
    d.assert_no_token();
}

#[test]
fn a_head_someone_else_pushed_is_answered_as_it_is_and_never_as_the_one_pushed() {
    let other = "c".repeat(40);
    // There at the first read after the push, and after two reads at the head before.
    for late in [0, 2] {
        let d = Delivery::new("deliver-head-other", "");
        d.publish(&snapshot(HEAD, "met", "r1"));
        d.rig.github.move_late(late, Some(&other));
        let answer = d.publish(&snapshot(NEXT, "met", "r1"));
        // What the land refuses: a publication at another head than its tip.
        assert_eq!(answer["state"], "published", "{answer}");
        assert_eq!(answer["head"], json!(other), "late by {late}");
        assert_eq!(*d.rested.borrow(), deliver::READ_BACK[..late]);
        // And nothing is armed or merged at the head asked for.
        for verb in ["arm", "merge"] {
            let refused = at(&d, verb, 100, NEXT);
            assert_eq!(refused["kind"], "head", "{verb}: {refused}");
        }
        assert_eq!(d.rig.github.pulls()[0]["state"], "open");
    }
}

#[test]
fn arm_and_merge_after_a_publish_see_the_head_it_pushed() {
    for verb in ["arm", "merge"] {
        let d = Delivery::new("deliver-head-then", "");
        (d.rig.github).set_repository(&json!({"allow_auto_merge": true}));
        d.publish(&snapshot(HEAD, "met", "r1"));
        d.rig.github.move_late(2, None);
        // While GitHub has not moved it, either refuses the head pushed, as it does any other.
        d.remote.push(NEXT, SLUG).unwrap();
        let refused = at(&d, verb, 100, NEXT);
        assert_eq!(
            (&refused["state"], &refused["kind"]),
            (&json!("refused"), &json!("head")),
            "{verb}: {refused}"
        );
        // A publish answers once it is there, so what the land asks next sees it.
        assert_eq!(d.publish(&snapshot(NEXT, "met", "r1"))["head"], NEXT);
        let answer = at(&d, verb, 100, NEXT);
        let done = if verb == "arm" { "mergeable" } else { "merged" };
        assert_eq!(answer["state"], done, "{verb}: {answer}");
    }
}

/// Whether a request asked since the `from`th names pull request `number`, by its path or its
/// node.
fn asked_of(d: &Delivery, from: usize, number: i64) -> Vec<String> {
    let node = format!("PR_node{number}");
    let path = format!("/pulls/{number}");
    (d.rig.github.asked().into_iter().skip(from))
        .map(|(method, at, body)| format!("{method} {at} {}", body.unwrap_or_default()))
        .filter(|asked| asked.contains(&node) || asked.contains(&format!("{path} ")))
        .collect()
}

#[test]
fn a_pull_request_closed_unmerged_is_forgotten_once_the_land_names_it_no_more() {
    // Closed by someone at GitHub, or by the exchange's own `close`, then dispatched again at its
    // head as it was or at a new one.
    for (closed_by, next) in [("someone", HEAD), ("close", NEXT), ("someone", NEXT)] {
        let case = format!("closed by {closed_by}, next at {}", next.get(..7).unwrap());
        let d = Delivery::new("deliver-closed", "");
        let first = d.publish(&snapshot(HEAD, "met", "r1"));
        assert_eq!(first["pull"]["number"], 100, "{case}");
        match closed_by {
            "close" => assert_eq!(d.end("close", 100)["state"], "closed", "{case}"),
            _ => d.rig.github.set_pull(100, &json!({"state": "closed"})),
        }
        // Named still by the land's journal: bound as it was, refused, and nothing opened. The
        // land reads it closed next and drops its journal. A number is no word that the land
        // binds none, even one that is not the one bound here.
        for pull in [100, 99] {
            let named = d.publish_naming(&snapshot(next, "met", "r2"), Some(pull));
            assert_eq!(
                named,
                json!({
                    "state": "refused",
                    "why": "pull request #100 of o/n is closed: nothing more is published to it",
                }),
                "{case}, naming #{pull}"
            );
        }
        assert_eq!(d.rig.state().pulls[SLUG].number, 100, "{case}");
        assert_eq!(d.rig.github.pulls().len(), 1, "{case}");
        assert_eq!(
            d.result(100)["state"],
            "closed",
            "{case}: still read as the land asks"
        );
        // Its next publication names none: forgotten, journaled, and another opened and bound.
        let from = d.rig.github.asked().len();
        let answer = d.publish(&snapshot(next, "met", "r2"));
        assert_eq!(
            answer,
            json!({
                "state": "published", "repository": {"id": 77, "name": "o/n"}, "head": next,
                "pull": {"number": 101, "url": "https://github.com/o/n/pull/101"},
            }),
            "{case}"
        );
        let kept = d.rig.state().pulls[SLUG].clone();
        assert_eq!(
            (kept.number, kept.task, kept.landed),
            (101, 7, None),
            "{case}"
        );
        let unbound: Vec<_> = (d.rig.journal().into_iter())
            .filter(|entry| entry.kind == "unbound")
            .map(|entry| (entry.target, entry.result))
            .collect();
        assert_eq!(unbound, [(SLUG.to_string(), json!(100))], "{case}");
        // The one closed receives nothing: no request names it but the one read that found it
        // closed, and it stays closed at the head it was closed at.
        assert_eq!(
            asked_of(&d, from, 100),
            ["GET /repos/o/n/pulls/100 "],
            "{case}: read once, written to never"
        );
        let pulls = d.rig.github.pulls();
        assert_eq!(
            (&pulls[0]["state"], &pulls[0]["head"]["sha"]),
            (&json!("closed"), &json!(HEAD)),
            "{case}"
        );
        assert_eq!(
            (
                &pulls[1]["state"],
                &pulls[1]["head"]["sha"],
                &pulls[1]["title"]
            ),
            (&json!("open"), &json!(next), &json!(SLUG)),
            "{case}"
        );
        assert_eq!(d.rig.github.checks(next).len(), 5, "{case}");
        // Asked again as the land now asks, naming the new one: found, and nothing made twice.
        let writes = d.writes();
        assert_eq!(
            d.publish_naming(&snapshot(next, "met", "r2"), Some(101)),
            answer
        );
        assert_eq!(d.writes(), writes, "{case}");
        // What the land asks of it next reads the new one; the one closed is bound no more.
        assert_eq!(d.result(101)["state"], "open", "{case}");
        assert!(
            (d.result(100)["why"].as_str().unwrap())
                .contains("is bound to pull request #101 of o/n"),
            "{case}"
        );
        d.assert_no_token();
    }
}

#[test]
fn a_pull_request_merged_landed_or_not_shown_unmerged_stays_bound() {
    let squash = "5".repeat(40);
    for case in ["merged", "landed", "unshown"] {
        let d = Delivery::new("deliver-closed-kept", "");
        d.publish(&snapshot(HEAD, "met", "r1"));
        match case {
            "merged" => assert_eq!(d.merge(100)["state"], "merged"),
            "landed" => {
                // Landed, then read closed unmerged, as it would were its merge undone by hand.
                let request = json!({"verb": "landed", "slug": SLUG, "by": "amber-fox-1",
                    "head": HEAD, "repository": 77, "pull": 100, "squash": squash});
                assert_eq!(d.ask("landed", &request)["state"], "noted");
                d.rig.github.set_pull(100, &json!({"state": "closed"}));
            }
            _ => {
                d.rig
                    .github
                    .set_pull(100, &json!({"state": "closed", "merged": null}));
            }
        }
        let answer = d.publish(&snapshot(NEXT, "met", "r2"));
        let said = if case == "merged" { "merged" } else { "closed" };
        assert_eq!(
            answer["why"],
            format!("pull request #100 of o/n is {said}: nothing more is published to it"),
            "{case}"
        );
        assert_eq!(d.rig.github.pulls().len(), 1, "{case}: none opened");
        assert_eq!(d.rig.state().pulls[SLUG].number, 100, "{case}");
        assert!(
            !(d.rig.journal().iter()).any(|entry| entry.kind == "unbound"),
            "{case}"
        );
    }
}

/// The journal's lines since its `from`th that bind or unbind a pull request: each one's event,
/// kind and result.
fn bindings(d: &Delivery, from: usize) -> Vec<String> {
    (d.rig.journal().into_iter().skip(from))
        .filter(|entry| ["pull", "unbound"].contains(&entry.kind.as_str()))
        .map(|entry| format!("{:?} {} {}", entry.event, entry.kind, entry.result))
        .collect()
}

#[test]
fn an_open_pull_request_stays_bound_whatever_the_land_names() {
    // `null`, the one bound, another, and no `pull` at all as an older board asks.
    for pull in [Some(Value::Null), Some(json!(100)), Some(json!(101)), None] {
        let d = Delivery::new("deliver-open-kept", "");
        d.publish(&snapshot(HEAD, "met", "r1"));
        let (asked, journaled) = (d.rig.github.asked().len(), d.rig.journal().len());
        let answer = d.publish_saying(&snapshot(NEXT, "met", "r2"), pull.clone());
        assert_eq!(answer["state"], "published", "{pull:?}: {answer}");
        assert_eq!(answer["pull"]["number"], 100, "{pull:?}");
        assert_eq!(d.rig.github.pulls().len(), 1, "{pull:?}");
        assert_eq!(d.rig.state().pulls[SLUG].number, 100, "{pull:?}");
        // Kept, not forgotten and found again by its branch, which would end bound to the same
        // number: nothing is unbound or bound again, and the branch's are not listed.
        assert_eq!(bindings(&d, journaled), Vec::<String>::new(), "{pull:?}");
        let listed = (d.rig.github.asked().into_iter().skip(asked))
            .filter(|(_, path, _)| path.contains("/pulls?"))
            .count();
        assert_eq!(listed, 0, "{pull:?}");
    }
}

#[test]
fn a_pull_request_closed_unmerged_stays_bound_where_the_request_names_no_pull() {
    // A board older than `pull` asks with none: the binding it keeps names the one closed, so the
    // exchange keeps it too, refuses it as before, and reads it closed for that board to drop.
    for (closed_by, next) in [("someone", HEAD), ("close", NEXT)] {
        let case = format!("closed by {closed_by}, next at {}", next.get(..7).unwrap());
        let d = Delivery::new("deliver-closed-unnamed", "");
        let first = d.publish_saying(&snapshot(HEAD, "met", "r1"), None);
        assert_eq!(first["pull"]["number"], 100, "{case}");
        match closed_by {
            "close" => assert_eq!(d.end("close", 100)["state"], "closed", "{case}"),
            _ => d.rig.github.set_pull(100, &json!({"state": "closed"})),
        }
        let (writes, journaled) = (d.writes(), d.rig.journal().len());
        // As that board's watcher publishes an armed task whose evidence moved, twice.
        for revision in ["r2", "r3"] {
            let answer = d.publish_saying(&snapshot(next, "met", revision), None);
            assert_eq!(
                answer,
                json!({
                    "state": "refused",
                    "why": "pull request #100 of o/n is closed: nothing more is published to it",
                }),
                "{case}, {revision}"
            );
        }
        assert_eq!(d.rig.state().pulls[SLUG].number, 100, "{case}");
        assert_eq!(d.rig.github.pulls().len(), 1, "{case}: none opened");
        assert_eq!(d.writes(), writes, "{case}: nothing written to GitHub");
        assert_eq!(bindings(&d, journaled), Vec::<String>::new(), "{case}");
        // What that board asks next is answered of the one it binds, never "bound to #101".
        let result = d.result(100);
        assert_eq!(result["state"], "closed", "{case}: {result}");
        // A board that says it binds none, once it reads it closed, is the one that moves on.
        let answer = d.publish(&snapshot(next, "met", "r3"));
        assert_eq!(answer["pull"]["number"], 101, "{case}: {answer}");
        assert_eq!(
            bindings(&d, journaled),
            ["Ack unbound 100", "Intent pull null", "Ack pull 101"]
                .map(String::from)
                .to_vec(),
            "{case}"
        );
    }
}

#[test]
fn a_pull_that_is_neither_a_number_nor_null_is_refused_before_anything_is_asked() {
    for pull in [
        json!("100"),
        json!(1.5),
        json!(true),
        json!({"number": 100}),
        json!([100]),
    ] {
        let d = Delivery::new("deliver-pull-shape", "");
        let asked = d.rig.github.asked().len();
        let answer = d.publish_saying(&snapshot(HEAD, "met", "r1"), Some(pull.clone()));
        assert_eq!(
            answer,
            json!({
                "state": "refused",
                "why": "the request's pull is the number of the pull request the land binds, \
                        or null",
            }),
            "{pull}"
        );
        assert_eq!(
            d.rig.github.asked().len(),
            asked,
            "{pull}: nothing asked of GitHub"
        );
        assert!(d.remote.pushed.borrow().is_empty(), "{pull}");
    }
}

#[test]
fn a_rewritten_check_is_read_again_while_github_still_lists_what_it_said_before() {
    let d = Delivery::new("deliver-rewritten-late", "");
    d.publish(&snapshot(HEAD, "met", "r1"));
    let before = d.writes().len();
    d.rig.github.list_late(2);
    let answer = d.publish(&snapshot(HEAD, "missing", "r2"));
    assert_eq!(answer["state"], "published", "{answer}");
    assert_eq!(*d.rested.borrow(), deliver::READ_BACK[..2]);
    let rewritten = d.writes().split_off(before);
    assert_eq!(rewritten.len(), 5, "each rewritten once: {rewritten:?}");
    assert!(names(&d.rig.github.checks(HEAD)).contains(&("symphony/eligible", "failure")));
    assert!(names(&d.rig.github.checks(HEAD)).contains(&("symphony/tests", "in_progress")));
}

#[test]
fn evidence_that_changed_rewrites_its_checks_the_failing_verdict_first() {
    let d = Delivery::new("deliver-changed", "");
    d.publish(&snapshot(HEAD, "met", "r1"));
    let before = d.writes().len();
    let answer = d.publish(&snapshot(HEAD, "missing", "r2"));
    assert_eq!(
        answer["state"], "published",
        "what stands is said, whatever it is"
    );
    // Each rewritten in place, but the stamp now pending: a run of its own after the one that
    // completed, which GitHub goes by in its place.
    let rewritten = d.writes().split_off(before);
    assert_eq!(rewritten.len(), 5, "{rewritten:?}");
    assert_eq!(
        (rewritten.iter())
            .filter(|write| write.starts_with("PATCH /repos/o/n/check-runs/"))
            .count(),
        4,
        "{rewritten:?}"
    );
    assert_eq!(
        (rewritten.iter())
            .filter(|write| *write == "POST /repos/o/n/check-runs")
            .count(),
        1,
        "{rewritten:?}"
    );
    // The first rewritten is the one that said every stamp stood.
    let first = d
        .rig
        .github
        .asked()
        .into_iter()
        .find(|(method, ..)| method == "PATCH");
    let sent: Value = serde_json::from_str(&first.unwrap().2.unwrap()).unwrap();
    assert_eq!(
        (&sent["name"], &sent["conclusion"]),
        (&json!("symphony/eligible"), &json!("failure"))
    );
    // The verdict that passed at this head fails now, as withdrawn; the stamp short of it waits.
    let checks = d.rig.github.checks(HEAD);
    assert_eq!(checks.len(), 5, "each name once: {checks:?}");
    assert!(names(&checks).contains(&("symphony/tests", "in_progress")));
    assert!(names(&checks).contains(&("symphony/eligible", "failure")));
    assert_eq!(
        d.rig.github.runs(HEAD).len(),
        6,
        "the completed run stays beside it"
    );
    // Another head carries none of them.
    let other = "b".repeat(40);
    d.publish(&snapshot(&other, "met", "r3"));
    assert_eq!(d.rig.github.checks(&other).len(), 5);
    assert_eq!(d.rig.github.pulls()[0]["head"]["sha"], other.as_str());
}

#[test]
fn only_the_named_apps_checks_count_and_no_other_token_writes_them() {
    // A check of the same name by another App is not ours: ours is written beside it.
    let d = Delivery::new("deliver-app", "");
    d.rig.github.foreign_check(HEAD, "symphony/eligible", 99);
    assert_eq!(
        d.publish(&snapshot(HEAD, "met", "r1"))["state"],
        "published"
    );
    let eligible: Vec<i64> = (d.rig.github.checks(HEAD).into_iter())
        .filter(|(name, ..)| name == "symphony/eligible")
        .map(|(.., app)| app)
        .collect();
    assert_eq!(eligible, [99, APP]);
    // The token a person's: GitHub takes no check of it, and nothing says published.
    let d = Delivery::new("deliver-person", "");
    std::fs::write(&d.rig.token_file, "ghp_aPersonsOtherToken\n").unwrap();
    let answer = d.publish(&snapshot(HEAD, "met", "r1"));
    assert_eq!(answer["state"], "refused", "{answer}");
    assert!(
        answer["why"]
            .as_str()
            .unwrap()
            .starts_with("GitHub took no check symphony/assessment"),
        "{answer}"
    );
    assert_eq!(d.rig.github.checks(HEAD).len(), 0);
    // A token others can read is not read at all.
    std::fs::set_permissions(&d.rig.token_file, std::fs::Permissions::from_mode(0o644)).unwrap();
    let answer = d.publish(&snapshot(HEAD, "met", "r1"));
    assert!(
        answer["why"]
            .as_str()
            .unwrap()
            .ends_with("is readable by others; chmod 600 it"),
        "{answer}"
    );
    d.assert_no_token();
}

#[test]
fn what_does_not_lead_to_the_repository_named_is_refused_before_anything_is_pushed() {
    let mut d = Delivery::new("deliver-elsewhere", "");
    d.remote.url = "git@github.com:o/another.git".into();
    let answer = d.publish(&snapshot(HEAD, "met", "r1"));
    assert_eq!(
        answer["why"],
        "origin is not o/n on GitHub.com: nothing is pushed anywhere else"
    );
    assert!(d.rig.github.asked().is_empty() && d.remote.pushed.borrow().is_empty());
    // Evidence of another shape, task or head, and a stamp no check is named for.
    d.remote.url = "https://github.com/O/N".into();
    let mut other = snapshot(HEAD, "met", "r1");
    other["version"] = json!(2);
    assert!(
        d.publish(&other)["why"]
            .as_str()
            .unwrap()
            .contains("of shape 2")
    );
    let mut other = snapshot(HEAD, "met", "r1");
    other["slug"] = json!("sim/other");
    assert!(
        d.publish(&other)["why"]
            .as_str()
            .unwrap()
            .contains("another task or head")
    );
    let mut other = snapshot(HEAD, "met", "r1");
    other["stamps"][0]["name"] = json!("te sts");
    assert!(
        d.publish(&other)["why"]
            .as_str()
            .unwrap()
            .contains("no check is named")
    );
    assert!(d.rig.github.asked().is_empty() && d.remote.pushed.borrow().is_empty());
    // A branch there that holds another history: no pull request, no check.
    d.remote.refuses = true;
    let answer = d.publish(&snapshot(HEAD, "met", "r1"));
    assert!(
        answer["why"]
            .as_str()
            .unwrap()
            .starts_with("aaaaaaa was not pushed to the branch sim/jump of o/n: "),
        "{answer}"
    );
    assert_eq!(d.writes().len(), 0);
    // Another repository answering at the name the state is bound to.
    d.remote.refuses = false;
    assert_eq!(
        d.publish(&snapshot(HEAD, "met", "r1"))["state"],
        "published"
    );
    d.rig.github.replace_repository(78);
    let before = d.writes();
    let answer = d.publish(&snapshot(HEAD, "met", "r1"));
    assert!(
        answer["why"]
            .as_str()
            .unwrap()
            .starts_with("o/n is repository 78, and what is kept here is bound to o/n (id 77)"),
        "{answer}"
    );
    assert_eq!(d.merge(100)["state"], "refused");
    assert_eq!(d.writes(), before);
}

#[test]
fn a_merge_is_one_squash_of_the_head_asked_once() {
    let d = Delivery::new("deliver-merge", "");
    d.publish(&snapshot(HEAD, "met", "r1"));
    // Another pull request than the one bound, and another method, ask nothing.
    assert!(
        d.merge(101)["why"]
            .as_str()
            .unwrap()
            .contains("is bound to pull request #100 of o/n (id 77)")
    );
    let squash = "5".repeat(40);
    assert_eq!(
        d.result(100),
        json!({
            "state": "open", "head": HEAD, "sha": null, "base": "main", "repository": 77,
            "auto_merge": false, "mergeable_state": "clean", "foreign": [],
        })
    );
    assert_eq!(d.merge(100), json!({"state": "merged", "sha": squash}));
    let merged = d.rig.github.pulls().remove(0)["merged_as"].clone();
    assert_eq!(
        (
            &merged["sha"],
            &merged["merge_method"],
            &merged["commit_title"]
        ),
        (&json!(HEAD), &json!("squash"), &json!(SLUG))
    );
    // Opened ready: nothing is marked.
    assert!(
        !(d.writes().iter()).any(|write| write == "POST /graphql"),
        "{:?}",
        d.writes()
    );
    // Asked again it reads that it is merged and asks for no second merge.
    assert_eq!(d.merge(100), json!({"state": "merged", "sha": squash}));
    assert_eq!(
        d.writes()
            .iter()
            .filter(|write| write.starts_with("PUT "))
            .count(),
        1
    );
    assert_eq!(
        d.result(100),
        json!({
            "state": "merged", "head": HEAD, "sha": squash, "base": "main", "repository": 77,
            "auto_merge": false, "mergeable_state": "clean", "foreign": [],
        })
    );
    d.assert_no_token();
}

#[test]
fn a_merge_whose_answer_is_lost_is_unknown_and_its_result_is_read() {
    let d = Delivery::new("deliver-merge-lost", "");
    d.publish(&snapshot(HEAD, "met", "r1"));
    d.rig
        .github
        .fault("PUT", "/repos/o/n/pulls/100/merge", Fault::LoseAnswer);
    assert_eq!(d.merge(100)["state"], "unknown");
    assert_eq!(d.result(100)["state"], "merged");
    assert_eq!(d.merge(100)["state"], "merged");
    assert_eq!(
        d.writes()
            .iter()
            .filter(|write| write.starts_with("PUT "))
            .count(),
        1
    );
}

#[test]
fn a_merge_github_refuses_says_which_kind_and_merges_nothing() {
    for (planted, kind) in [
        (json!({"mergeable_state": "dirty"}), "conflict"),
        (json!({"mergeable_state": "blocked"}), "checks"),
        (
            json!({"head": {"ref": SLUG, "sha": "c".repeat(40), "repo": {"id": 77}}}),
            "head",
        ),
        (json!({"state": "closed"}), "closed"),
    ] {
        let d = Delivery::new("deliver-refused", "");
        d.publish(&snapshot(HEAD, "met", "r1"));
        d.rig.github.set_pull(100, &planted);
        let answer = d.merge(100);
        assert_eq!(
            (&answer["state"], &answer["kind"]),
            (&json!("refused"), &json!(kind)),
            "{answer}"
        );
        assert_eq!(d.rig.github.pulls()[0]["merged"], false);
        let asked = d
            .writes()
            .iter()
            .filter(|write| write.starts_with("PUT "))
            .count();
        assert_eq!(
            asked,
            usize::from(matches!(kind, "conflict" | "checks")),
            "{kind}"
        );
        assert_eq!(
            d.result(100)["state"],
            if kind == "closed" { "closed" } else { "open" }
        );
    }
    // A head that moved between the reading and the request, and a token that may not merge.
    for (status, kind) in [(409, "head"), (403, "permission")] {
        let d = Delivery::new("deliver-refused-status", "");
        d.publish(&snapshot(HEAD, "met", "r1"));
        d.rig.github.fault(
            "PUT",
            "/repos/o/n/pulls/100/merge",
            Fault::Status(status, Vec::new()),
        );
        let answer = d.merge(100);
        assert_eq!(answer["kind"], kind, "{answer}");
        assert_eq!(
            answer["why"],
            format!("GitHub merged nothing ({status}): planted")
        );
    }
}

#[test]
fn landed_keeps_the_commit_and_closes_the_mapped_issue_only_where_the_settings_say() {
    let squash = "5".repeat(40);
    for closes in [false, true] {
        let d = Delivery::new(
            "deliver-landed",
            if closes { "close_issues = true" } else { "" },
        );
        let request = json!({"verb": "landed", "slug": SLUG, "by": "amber-fox-1", "head": HEAD,
            "repository": 77, "pull": 100, "squash": squash});
        assert_eq!(
            d.ask("landed", &request)["why"],
            "sim/jump is bound to no pull request here"
        );
        d.publish(&snapshot(HEAD, "met", "r1"));
        d.merge(100);
        // The task is exchanged with an issue.
        let number = d.rig.github.open("Jump higher", "public", &[]);
        let mut state = d.rig.state();
        state.mappings.insert(
            SLUG.into(),
            Mapping {
                origin: Origin::Export,
                task_id: 7,
                generation: "generation-1".into(),
                issue_number: number,
                issue_id: 5000 + number,
                issue_node: String::new(),
                url: String::new(),
                local: String::new(),
                remote: String::new(),
                remote_state: "open".into(),
                comments_since: None,
                comments_seen: Vec::new(),
                flag: None,
            },
        );
        d.rig.store().save(&state).unwrap();
        let before = d.writes().len();
        let said = if closes {
            format!("issue #{number} is closed")
        } else {
            String::new()
        };
        for _ in 0..2 {
            assert_eq!(
                d.ask("landed", &request),
                json!({"state": "noted", "said": said})
            );
        }
        assert_eq!(
            d.writes().len() - before,
            usize::from(closes),
            "closed once, or never"
        );
        assert_eq!(
            d.rig.github.issue(number)["state"],
            if closes { "closed" } else { "open" }
        );
        assert_eq!(
            d.rig.state().pulls[SLUG].landed.as_deref(),
            Some(squash.as_str())
        );
        let status = d.rig.run(&["status"]).ok().out;
        assert!(
            status.contains("pull requests: publish (a land's delivery: `deliver`)"),
            "{status}"
        );
        assert!(
            status.contains("sim/jump is delivered by #100 https://github.com/o/n/pull/100 (landed as 555555555555)"),
            "{status}"
        );
        d.assert_no_token();
    }
}

#[test]
fn the_command_takes_one_verb_and_a_request_that_reads() {
    let d = Delivery::new("deliver-cli", "");
    let none = d.rig.dir.0.join("none.json");
    let ran = d
        .rig
        .run(&["deliver", "publish", "--request", none.to_str().unwrap()]);
    assert_eq!(ran.code, 1);
    assert!(
        ran.err.starts_with("symphony-github: ") && ran.err.contains("none.json"),
        "{}",
        ran.err
    );
    std::fs::write(&none, "[]").unwrap();
    let ran = d
        .rig
        .run(&["deliver", "publish", "--request", none.to_str().unwrap()]);
    assert!(
        ran.err.ends_with("a request is one JSON object\n"),
        "{}",
        ran.err
    );
    let ran = d.rig.run(&["deliver", "--request", none.to_str().unwrap()]);
    assert!(ran.err.contains("deliver takes one VERB"), "{}", ran.err);
    std::fs::write(&none, "{}").unwrap();
    let ran = d
        .rig
        .run(&["deliver", "rebase", "--request", none.to_str().unwrap()])
        .ok();
    assert_eq!(
        ran.out,
        "{\"state\":\"refused\",\"why\":\"no verb \\\"rebase\\\"\"}\n"
    );
    assert_eq!(d.rig.github.asked().len(), 0);
}

#[test]
fn a_draft_an_earlier_build_opened_is_made_ready_and_a_mark_that_fails_merges_nothing() {
    // The answer to the mark is lost: nothing more published, and the next run reads it first.
    let d = Delivery::new("deliver-draft", "");
    d.publish(&snapshot(HEAD, "met", "r1"));
    d.rig.github.set_pull(100, &json!({"draft": true}));
    d.rig.github.fault("POST", "/graphql", Fault::LoseAnswer);
    let answer = d.publish(&snapshot(HEAD, "met", "r1"));
    assert_eq!(answer["state"], "unavailable", "{answer}");
    assert!(
        answer["why"]
            .as_str()
            .unwrap()
            .contains("nothing was merged"),
        "{answer}"
    );
    // It was marked: the next publication finds it ready and marks nothing twice.
    assert_eq!(
        d.publish(&snapshot(HEAD, "met", "r1"))["state"],
        "published"
    );
    assert_eq!(d.rig.github.pulls()[0]["draft"], false);
    assert_eq!(
        d.writes()
            .iter()
            .filter(|write| *write == "POST /graphql")
            .count(),
        1
    );
    // A merge of one still a draft marks it first, once.
    let d = Delivery::new("deliver-draft-merge", "");
    d.publish(&snapshot(HEAD, "met", "r1"));
    d.rig.github.set_pull(100, &json!({"draft": true}));
    assert_eq!(d.merge(100)["state"], "merged");
    let ready: Vec<Value> = (d.rig.github.asked().into_iter())
        .filter(|(method, path, _)| method == "POST" && path == "/graphql")
        .map(|(.., body)| serde_json::from_str(&body.unwrap()).unwrap())
        .collect();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0]["variables"], json!({"id": "PR_node100"}));
    // A token that may not mark it: refused by kind, still a draft, nothing merged.
    let d = Delivery::new("deliver-draft-refused", "");
    d.publish(&snapshot(HEAD, "met", "r1"));
    d.rig.github.set_pull(100, &json!({"draft": true}));
    d.rig
        .github
        .fault("POST", "/graphql", Fault::Status(403, Vec::new()));
    let answer = d.merge(100);
    assert_eq!(
        (&answer["state"], &answer["kind"]),
        (&json!("refused"), &json!("permission"))
    );
    assert_eq!(
        answer["why"],
        "GitHub left pull request #100 a draft (403): planted"
    );
    let pull = d.rig.github.pulls().remove(0);
    assert_eq!(
        (&pull["draft"], &pull["merged"]),
        (&json!(true), &json!(false))
    );
    let journal: Vec<String> = (d.rig.journal().iter())
        .map(|entry| format!("{} {:?}", entry.kind, entry.event))
        .collect();
    assert!(journal.contains(&"ready Failed".to_string()), "{journal:?}");
    d.assert_no_token();
}

/// The evidence as the board's own type writes it, so the two sides cannot drift apart: a land
/// that refuses a head it has a pull request for hands over exactly this, not eligible
/// (`Landing::bound_before` in the board's `land/delivery.rs`).
fn withdrawn(head: &str) -> Value {
    use symphony_board::land::evidence::{Assessment, Snapshot, Stamp, VERSION};
    let stamp = |name: &str, standing: &str| -> Stamp {
        serde_json::from_value(json!({
            "name": name, "standing": standing, "detail": "a private line", "evidence": [],
        }))
        .unwrap()
    };
    serde_json::to_value(Snapshot {
        version: VERSION,
        slug: SLUG.into(),
        task: 7,
        status: "done".into(),
        head: head.into(),
        branch: "main".into(),
        policy: "p".into(),
        catalogue: "c".into(),
        assessment: Some(Assessment {
            by: "calm-owl-2".into(),
            tip: Some(head.into()),
            why: "a private reason".into(),
            stamps: vec!["tests".into(), "review".into()],
            import: None,
        }),
        assessed: true,
        stamps: vec![
            stamp("tests", "missing"),
            stamp("review", "met"),
            stamp("pictures", "waived"),
        ],
        blocking: vec!["tests: red at the head".into()],
        eligible: false,
        revision: "withdrawn".into(),
    })
    .unwrap()
}

#[test]
fn evidence_the_board_withdrew_fails_the_required_check_of_a_pull_request_already_ready() {
    let d = Delivery::new("deliver-withdrawn", "");
    d.publish(&snapshot(HEAD, "met", "r1"));
    // A merge GitHub refused leaves the pull request ready, its checks green.
    d.rig
        .github
        .set_pull(100, &json!({"mergeable_state": "blocked"}));
    assert_eq!(d.merge(100)["kind"], "checks");
    assert_eq!(d.rig.github.pulls()[0]["draft"], false);
    assert!(names(&d.rig.github.checks(HEAD)).contains(&("symphony/eligible", "success")));
    // The board's own snapshot of the withdrawal, at that head.
    let before = d.writes().len();
    assert_eq!(d.publish(&withdrawn(HEAD))["state"], "published");
    let checks = d.rig.github.checks(HEAD);
    assert_eq!(checks.len(), 5, "each name once: {checks:?}");
    assert!(names(&checks).contains(&("symphony/eligible", "failure")));
    assert!(names(&checks).contains(&("symphony/tests", "in_progress")));
    let first: Value = serde_json::from_str(
        &(d.rig
            .github
            .asked()
            .into_iter()
            .skip_while(|(method, ..)| method != "PUT"))
        .find(|(method, ..)| method == "PATCH")
        .unwrap()
        .2
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        first["name"], "symphony/eligible",
        "the verdict fails before any stamp moves"
    );
    assert_eq!(
        d.writes().len() - before,
        5,
        "each check is of the new revision"
    );
    // At a head the branch moved to: pushed, so the pull request is at it, where nothing stood
    // yet, so its verdict is pending there.
    let other = "b".repeat(40);
    assert_eq!(d.publish(&withdrawn(&other))["head"], other.as_str());
    assert!(names(&d.rig.github.checks(&other)).contains(&("symphony/eligible", "in_progress")));
    assert_eq!(d.rig.github.pulls()[0]["head"]["sha"], other.as_str());
    d.assert_no_token();
}
