//! `tasks land` delivered by a hosted pull request (`[land] delivery`, docs/TASKS.md "A land
//! delivered by a pull request"), in scratch repositories on a board of their own, the host a
//! bare repository this file merges in as a host would: what it asks of its transport and in what
//! order, what refuses before anything is asked, what a lost answer, a wrong commit and a main
//! checkout that cannot follow leave, and that a project with no such key lands as it did.

use crate::{Repo, git, write};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use symphony_board::cli::Env;
use symphony_board::cli::machine_work::recorded::{self, FOLDER_VAR, Phase, Record, Step};
use symphony_board::land::delivery::{Journal, Receipt, Stage, Transport};
use symphony_board::land::{Stop, evidence, land, land_by};
use symphony_board::stamps::{Catalogue, Needs};

mod candidate;
mod dispatched;
mod generated;
mod import;

const SLUG: &str = "sim/jump";
/// The project file of a project that delivers by pull request.
const DELIVERS: &str = "[land]\ncheck = false\ndelivery = [\"host\", \"deliver\"]\n";

fn head(dir: &Path, rev: &str) -> String {
    git(dir, &["rev-parse", rev])
}

fn project_file(r: &Repo, text: &str) {
    write(&r.root, "symphony.toml", text);
    git(&r.root, &["add", "symphony.toml"]);
    git(&r.root, &["commit", "-qm", "the project file"]);
}

/// A bare repository beside the checkout, its `origin`, holding main as it is now.
fn remote(r: &Repo) -> PathBuf {
    let bare = r.dir.join("origin.git");
    let at = bare.to_string_lossy().into_owned();
    git(&r.dir, &["init", "-q", "--bare", "-b", "main", &at]);
    git(&r.root, &["remote", "add", "origin", &at]);
    git(&r.root, &["push", "-q", "origin", "main"]);
    bare
}

/// A repository whose default branch commits `[land] delivery`, and its host.
fn delivering() -> (Repo, PathBuf) {
    let r = Repo::new();
    project_file(&r, DELIVERS);
    let bare = remote(&r);
    (r, bare)
}

/// Who holds a task here.
struct People {
    orch: String,
    exe: String,
    assessor: String,
}

/// `slug` filed, orchestrated, its branch writing `file`, claimed and in review: the people, the
/// worktree and the head.
fn in_review(r: &Repo, slug: &str, file: &str) -> (People, PathBuf, String) {
    in_review_from(r, slug, file, "main")
}

/// [`in_review`], its branch started at `base`.
fn in_review_from(r: &Repo, slug: &str, file: &str, base: &str) -> (People, PathBuf, String) {
    let wt = r.add(&slug.replace('/', "-"), Some(slug), base);
    write(&wt, file, "extends Node\n");
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-qm", &format!("work on {slug}")]);
    let store = &r.board.store;
    let people = People {
        orch: r.board.session("orchestrator"),
        exe: r.board.session("worker"),
        assessor: r.board.session("reviewer"),
    };
    r.task(slug);
    store.take_role(slug, &people.orch, "orchestrator").unwrap();
    store.claim(slug, &people.exe).unwrap();
    store.move_to(slug, "review", &people.exe).unwrap();
    let tip = head(&wt, "HEAD");
    (people, wt, tip)
}

fn assess(r: &Repo, slug: &str, by: &str, stamps: &[&str], tip: &str) {
    let needs = Needs {
        stamps: if stamps.is_empty() {
            vec!["none".into()]
        } else {
            stamps.iter().map(|s| (*s).to_string()).collect()
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

fn work(r: &Repo) -> PathBuf {
    r.dir.join("work")
}

static RUNS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

/// A test run of `slug` at `commit` the recorder kept, finished with `exit`.
fn ran(r: &Repo, slug: &str, commit: &str, label: &str, exit: i32) {
    let n = RUNS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let started = 1000.0 + f64::from(n);
    let record = Record {
        version: 1,
        operation_id: format!("{n:032x}"),
        parent_id: None,
        phase: Phase::Test,
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
        run: None,
        reading: None,
    };
    recorded::keep(&work(r), &record).unwrap();
}

/// `slug` ready to deliver: assessed at its head as needing `tests` and `review`, its tests green
/// there and a fresh reviewer's approve naming it.
fn ready(r: &Repo, slug: &str) -> (People, PathBuf, String) {
    let (people, wt, tip) = in_review(r, slug, "sim/jump.gd");
    assess(r, slug, &people.assessor, &["tests", "review"], &tip);
    ran(r, slug, &tip, "cargo test -p sim", 0);
    approve(r, slug, &format!("{tip}: reads well"));
    (people, wt, tip)
}

/// A fresh reviewer's approve of `slug` saying `text`.
fn approve(r: &Repo, slug: &str, text: &str) {
    let reviewer = r.board.session("reviewer");
    let store = &r.board.store;
    store.take_role(slug, &reviewer, "reviewer").unwrap();
    store.verdict(slug, &reviewer, "approve", text).unwrap();
}

/// How the host answers a merge.
#[derive(Clone, Copy, PartialEq)]
enum Merges {
    /// As asked: a squash for `squash`, a merge commit for `merge`.
    AsAsked,
    /// One commit on its default branch, the merge of it and the head, whatever was asked.
    Squash,
    /// A merge commit of two parents.
    ByCommit,
    /// A merge commit of two parents, the head first and the base second.
    Reversed,
    /// One commit whose tree is the default branch's own: the head's change lost.
    WrongTree,
    /// Refused, for this kind of reason.
    Refuses(&'static str),
    /// Closed without a merge.
    Closes,
}

/// How the host answers an arming of its own merge.
#[derive(Clone, Copy, PartialEq)]
enum Arms {
    /// As GitHub does: `mergeable` where the pull request could merge now (its eligible published
    /// passing, no red check of another's), else armed.
    AsGitHub,
    /// Armed whatever its checks say: a repository whose other required checks are pending.
    Always,
    /// Refused, for this kind of reason.
    Refuses(&'static str),
}

/// What the host's own merge, armed, merges by: the title and the method it was armed with.
type Armed = Option<(String, String)>;

/// A verb, and what runs when it is asked.
type Hook<'r> = (&'static str, Box<dyn FnOnce() + 'r>);

/// A host: the bare repository, one pull request, and what it was asked.
struct Host<'r> {
    r: &'r Repo,
    bare: PathBuf,
    merges: Merges,
    /// Each verb asked, with its request.
    asked: RefCell<Vec<(String, Value)>>,
    /// The pull request: its head, and the commit it was merged as or that it was closed.
    pull: RefCell<(Option<String>, Option<String>, bool)>,
    /// Verbs that give no answer, each once, the act made first.
    lost: RefCell<Vec<&'static str>>,
    /// Run when a verb is asked, before it acts.
    before: RefCell<Vec<Hook<'r>>>,
    /// The head `publish` says the pull request is at, in place of the one pushed.
    says_head: Option<String>,
    /// Fields laid over a verb's answer, whatever the host did.
    says: RefCell<Vec<(&'static str, Value)>>,
    arms: Arms,
    /// Its own merge, armed: merged by itself at the next read once the eligible published at
    /// the pull request's head passes and no check of another's is red, as GitHub merges seconds
    /// after the check it waits on turns green.
    armed: RefCell<Armed>,
    /// Whether the evidence last published says it stands.
    eligible: RefCell<bool>,
    /// The checks of another's that are red at its head, `{"name", "conclusion"}`.
    foreign: RefCell<Vec<Value>>,
    /// What GitHub says of whether it merges: `blocked`, or `dirty` in conflict with main.
    mergeable_state: RefCell<&'static str>,
    /// A check of another's the rules require is still pending at its head: nothing merges.
    waiting: RefCell<bool>,
}

impl<'r> Host<'r> {
    fn new(r: &'r Repo, bare: &Path) -> Host<'r> {
        Host {
            r,
            bare: bare.into(),
            merges: Merges::AsAsked,
            asked: RefCell::default(),
            pull: RefCell::default(),
            lost: RefCell::default(),
            before: RefCell::default(),
            says_head: None,
            says: RefCell::default(),
            arms: Arms::AsGitHub,
            armed: RefCell::default(),
            eligible: RefCell::default(),
            foreign: RefCell::default(),
            mergeable_state: RefCell::new("blocked"),
            waiting: RefCell::default(),
        }
    }

    fn arming(mut self, arms: Arms) -> Self {
        self.arms = arms;
        self
    }

    /// Whether its own merge is armed now.
    fn is_armed(&self) -> bool {
        self.armed.borrow().is_some()
    }

    /// From now on a check of another's is red at its head (a CI the repository adds).
    fn red_check(&self, name: &str) {
        (self.foreign.borrow_mut()).push(json!({"name": name, "conclusion": "failure"}));
    }

    /// From now on a check of another's the rules require stays pending: it merges nothing.
    fn waits(&self) {
        *self.waiting.borrow_mut() = true;
    }

    /// From now on GitHub reports the pull request in conflict with main.
    fn conflicts(&self) {
        *self.mergeable_state.borrow_mut() = "dirty";
    }

    /// Someone closes the pull request without a merge.
    fn close_by_hand(&self) {
        self.pull.borrow_mut().2 = true;
        *self.armed.borrow_mut() = None;
    }

    /// Its own merge made, where it is armed and nothing holds it.
    fn merge_on_green(&self) {
        let (head, merged, closed) = self.pull.borrow().clone();
        let held = !*self.eligible.borrow()
            || *self.waiting.borrow()
            || !self.foreign.borrow().is_empty()
            || *self.mergeable_state.borrow() == "dirty";
        let armed = self.armed.borrow().clone();
        if let (Some(head), None, false, false, Some((title, method))) =
            (head, merged, closed, held, armed)
        {
            let made = self.merge(&head, &title, &method);
            self.pull.borrow_mut().1 = Some(made);
        }
    }

    fn merging(mut self, merges: Merges) -> Self {
        self.merges = merges;
        self
    }

    fn losing(self, verbs: &[&'static str]) -> Self {
        self.lost.borrow_mut().extend(verbs);
        self
    }

    fn before(self, verb: &'static str, act: impl FnOnce() + 'r) -> Self {
        self.before.borrow_mut().push((verb, Box::new(act)));
        self
    }

    /// From now on `verb`'s answer says `fields` over what the host did.
    fn saying(self, verb: &'static str, fields: Value) -> Self {
        self.says.borrow_mut().push((verb, fields));
        self
    }

    fn verbs(&self) -> Vec<String> {
        (self.asked.borrow().iter())
            .map(|(verb, _)| verb.clone())
            .collect()
    }

    fn request(&self, verb: &str) -> Value {
        (self.asked.borrow().iter().rev())
            .find(|(asked, _)| asked == verb)
            .map(|(_, request)| request.clone())
            .unwrap()
    }

    /// The host's own merge of `head` into its default branch by `method`, as `self.merges` has
    /// it.
    fn merge(&self, head: &str, title: &str, method: &str) -> String {
        let bare = &self.bare;
        let base = head_of(bare, "main");
        let tree = match self.merges {
            Merges::WrongTree => head_of(bare, "main^{tree}"),
            _ => git(bare, &["merge-tree", "--write-tree", &base, head]),
        };
        let mut args = vec!["commit-tree", &tree, "-p", &base, "-m", title];
        let asked = self.merges == Merges::AsAsked && method == "merge";
        if self.merges == Merges::ByCommit || asked {
            args.extend(["-p", head]);
        }
        if self.merges == Merges::Reversed {
            args = vec!["commit-tree", &tree, "-p", head, "-p", &base, "-m", title];
        }
        let made = git(bare, &args);
        git(bare, &["update-ref", "refs/heads/main", &made]);
        made
    }
}

fn head_of(dir: &Path, rev: &str) -> String {
    git(dir, &["rev-parse", rev])
}

impl Transport for Host<'_> {
    fn ask(&self, verb: &str, request: &Value) -> Result<Value, String> {
        (self.asked.borrow_mut()).push((verb.to_string(), request.clone()));
        let hook = {
            let mut before = self.before.borrow_mut();
            (before.iter().position(|(at, _)| *at == verb)).map(|at| before.remove(at).1)
        };
        if let Some(hook) = hook {
            hook();
        }
        let lost = {
            let mut lost = self.lost.borrow_mut();
            (lost.iter().position(|at| *at == verb)).map(|at| lost.remove(at))
        };
        let answer = match verb {
            "publish" => {
                let (slug, head) = (
                    request["slug"].as_str().unwrap(),
                    request["head"].as_str().unwrap(),
                );
                // As a host does: nothing more is published to a pull request merged or closed.
                let (_, merged, closed) = self.pull.borrow().clone();
                if merged.is_some() || closed {
                    let why = "pull request #3 is merged or closed: nothing more is published";
                    return Ok(json!({"state": "refused", "why": why}));
                }
                let pushed = format!("{head}:refs/heads/{slug}");
                git(&self.r.root, &["push", "-q", "origin", &pushed]);
                self.pull.borrow_mut().0 = Some(head.to_string());
                *self.eligible.borrow_mut() = request["snapshot"]["eligible"] == json!(true);
                let mut published = json!({
                    "state": "published",
                    "repository": {"id": 7, "name": "owner/name"},
                    "pull": {"number": 3, "url": "https://example.test/owner/name/pull/3"},
                    "head": self.says_head.clone().unwrap_or_else(|| head.to_string()),
                });
                // As the program does: an import's upstream, pinned at the host, by its number.
                if request["operation"] == "import" {
                    published["upstream"] = json!({"id": 9, "name": "up/stream"});
                }
                published
            }
            "merge" => {
                let head = request["head"].as_str().unwrap();
                match self.merges {
                    Merges::Refuses(kind) => {
                        json!({"state": "refused", "kind": kind, "why": format!("no: {kind}")})
                    }
                    Merges::Closes => {
                        self.pull.borrow_mut().2 = true;
                        json!({"state": "refused", "kind": "other", "why": "it is closed"})
                    }
                    _ => {
                        let (title, method) = (&request["title"], &request["method"]);
                        let made =
                            self.merge(head, title.as_str().unwrap(), method.as_str().unwrap());
                        self.pull.borrow_mut().1 = Some(made.clone());
                        json!({"state": "merged", "sha": made})
                    }
                }
            }
            "arm" => {
                let (_, merged, closed) = self.pull.borrow().clone();
                let could = *self.eligible.borrow() && self.foreign.borrow().is_empty();
                match (merged, closed, self.arms) {
                    (Some(_), ..) => json!({"state": "merged"}),
                    (None, true, _) => json!({"state": "closed"}),
                    (None, false, Arms::Refuses(kind)) => {
                        json!({"state": "refused", "kind": kind, "why": format!("no: {kind}")})
                    }
                    // Armed already, as the program answers it, asking nothing again.
                    (None, false, _) if self.is_armed() => json!({"state": "armed"}),
                    (None, false, Arms::AsGitHub) if could => json!({"state": "mergeable"}),
                    (None, false, _) => {
                        let (title, method) = (&request["title"], &request["method"]);
                        *self.armed.borrow_mut() = Some((
                            title.as_str().unwrap().to_string(),
                            method.as_str().unwrap().to_string(),
                        ));
                        json!({"state": "armed"})
                    }
                }
            }
            "disarm" | "close" => {
                let merged = self.pull.borrow().1.is_some();
                if merged {
                    json!({"state": "merged"})
                } else {
                    *self.armed.borrow_mut() = None;
                    if verb == "close" {
                        self.pull.borrow_mut().2 = true;
                        json!({"state": "closed"})
                    } else {
                        json!({"state": "disarmed"})
                    }
                }
            }
            "result" => {
                self.merge_on_green();
                let (head, merged, closed) = self.pull.borrow().clone();
                let state = match (&merged, closed) {
                    (Some(_), _) => "merged",
                    (None, true) => "closed",
                    (None, false) => "open",
                };
                let open = state == "open";
                json!({
                    "state": state, "head": head, "sha": merged, "base": "main", "repository": 7,
                    "auto_merge": self.is_armed() && open,
                    "mergeable_state": *self.mergeable_state.borrow(),
                    "foreign": if open { self.foreign.borrow().clone() } else { Vec::new() },
                })
            }
            "landed" => json!({"state": "noted", "said": "its issue is closed"}),
            other => panic!("no verb {other}"),
        };
        let mut answer = answer;
        for (_, fields) in self.says.borrow().iter().filter(|(at, _)| *at == verb) {
            for (field, value) in fields.as_object().unwrap() {
                answer[field] = value.clone();
            }
        }
        match lost {
            Some(verb) => Err(format!("{verb}: no answer")),
            None => Ok(answer),
        }
    }
}

fn refusal(stop: Stop) -> String {
    match stop {
        Stop::Refused(line) => line,
        Stop::Board(error) => panic!("the board failed: {error:?}"),
    }
}

/// The land of `slug` as `actor` with `host` answering: what it printed, and how it ended.
fn landed(r: &Repo, slug: &str, actor: &str, host: &Host) -> (String, Result<String, String>) {
    landed_with(r, slug, actor, host, &[])
}

fn landed_with(
    r: &Repo,
    slug: &str,
    actor: &str,
    host: &Host,
    vars: &[(&str, &str)],
) -> (String, Result<String, String>) {
    let mut out = Vec::new();
    let result = land_by(
        &r.board.store,
        &landing(r, vars),
        slug,
        actor,
        &mut out,
        host,
    );
    (String::from_utf8(out).unwrap(), result.map_err(refusal))
}

/// The environment a land runs in here: the checkout's, with the recorder's folder and `vars`.
fn landing(r: &Repo, vars: &[(&str, &str)]) -> Env {
    let mut env = r.env(&r.root);
    let folder = work(r).to_string_lossy().into_owned();
    env.vars.insert(FOLDER_VAR.into(), folder);
    for (name, value) in vars {
        env.vars.insert((*name).into(), (*value).into());
    }
    env
}

fn lands(r: &Repo) -> PathBuf {
    r.root.join(".git/symphony/tasks/land")
}

/// The landed lines on `slug`'s thread.
fn landings(r: &Repo, slug: &str) -> Vec<String> {
    (r.board.said(slug).into_iter())
        .filter(|line| line.starts_with("landed "))
        .collect()
}

/// Nothing of `slug` landed: the host's default branch and the checkout's where they were, the
/// task open, no receipt.
fn assert_nothing_landed(r: &Repo, bare: &Path, before: &str) {
    assert_eq!(head(bare, "main"), before, "the host's main stays");
    assert_eq!(head(&r.root, "main"), before, "main stays here");
    assert!(landings(r, SLUG).is_empty(), "{:?}", r.board.said(SLUG));
    assert!(Receipt::read(&lands(r), SLUG).is_none());
}

// -- with no key -----------------------------------------------------------------------------------

#[test]
fn with_no_delivery_key_the_land_merges_here_and_asks_no_transport() {
    let r = Repo::new();
    project_file(&r, "[land]\ncheck = false\n");
    let bare = remote(&r);
    let (people, _, tip) = ready(&r, SLUG);
    let host = Host::new(&r, &bare);
    let (said, result) = landed(&r, SLUG, &people.orch, &host);
    let line = result.unwrap();
    assert!(host.verbs().is_empty(), "{:?}", host.verbs());
    assert_eq!(
        head(&r.root, "main^2"),
        tip,
        "one merge commit, the tip its second parent"
    );
    assert_eq!(
        head(&bare, "main"),
        head(&r.root, "main"),
        "pushed by the land"
    );
    assert!(line.ends_with("; pushed to origin"), "{line}");
    assert!(said.contains("merging "), "{said}");
    assert!(!Journal::path(&lands(&r), SLUG).exists());
    assert!(!lands(&r).join("receipts").exists());
    // The same task, the same land, by the entry every land takes: the same line but its merge.
    let s = Repo::new();
    project_file(&s, "[land]\ncheck = false\n");
    remote(&s);
    let (people, _, _) = ready(&s, SLUG);
    let mut env = s.env(&s.root);
    (env.vars).insert(FOLDER_VAR.into(), work(&s).to_string_lossy().into_owned());
    let mut out = Vec::new();
    let plain = land(&s.board.store, &env, SLUG, &people.orch, None, &mut out)
        .map_err(refusal)
        .unwrap();
    let shape = |line: &str| {
        let (start, rest) = line.split_once(" at ").unwrap();
        format!("{start}{}", &rest[12..])
    };
    assert_eq!(shape(&plain), shape(&line));
}

#[test]
fn a_project_file_naming_an_empty_delivery_command_is_refused() {
    let r = Repo::new();
    project_file(&r, "[land]\ndelivery = [\"\"]\n");
    let bare = remote(&r);
    let (people, ..) = ready(&r, SLUG);
    let (_, result) = landed(&r, SLUG, &people.orch, &Host::new(&r, &bare));
    assert_eq!(
        result.unwrap_err(),
        "symphony.toml: [land] delivery names no command: leave it out to land here"
    );
}

// -- what is delivered -----------------------------------------------------------------------------

#[test]
fn a_ready_task_is_squashed_by_its_pull_request_and_the_checkout_follows() {
    let (r, bare) = delivering();
    let (people, _, tip) = ready(&r, SLUG);
    let base = head(&bare, "main");
    // The evidence the api answers, read before the land.
    let mut env = r.env(&r.root);
    (env.vars).insert(FOLDER_VAR.into(), work(&r).to_string_lossy().into_owned());
    let read = evidence::of(&r.board.store, &env, SLUG, Some(&tip)).unwrap();
    assert!(read.eligible, "{:?}", read.blocking);
    let host = Host::new(&r, &bare);
    let (said, result) = landed(&r, SLUG, &people.orch, &host);
    let line = result.unwrap();
    assert_eq!(
        host.verbs(),
        ["publish", "arm", "merge", "result", "landed"]
    );
    // What was published is the evidence the api answers: one reading.
    let published: evidence::Snapshot =
        serde_json::from_value(host.request("publish")["snapshot"].clone()).unwrap();
    assert_eq!(published, read);
    let merge = host.request("merge");
    assert_eq!(merge["head"], tip.as_str());
    assert_eq!(merge["method"], "squash");
    assert_eq!(
        (merge["repository"].as_i64(), merge["pull"].as_i64()),
        (Some(7), Some(3))
    );
    assert_eq!(merge["title"], format!("{SLUG}: A task"));
    // One commit on the base, made by the host; the checkout followed it and merged nothing.
    let squash = head(&bare, "main");
    assert_eq!(head(&r.root, "main"), squash);
    assert_eq!(
        git(&r.root, &["rev-list", "--parents", "-n1", "main"]),
        format!("{squash} {base}")
    );
    assert!(!said.contains("merging "), "{said}");
    assert_eq!(
        line,
        format!(
            "landed {SLUG} on main at {}; squashed from {tip} by pull request #3 of owner/name",
            &squash[..12]
        )
    );
    assert_eq!(r.board.store.get(SLUG).unwrap().status, "done");
    assert_eq!(landings(&r, SLUG).len(), 1);
    // The receipt keeps the head and the commit made of it, and the head stays reachable.
    let receipt = Receipt::read(&lands(&r), SLUG).unwrap();
    assert_eq!(
        (receipt.head.as_str(), receipt.squash.as_str()),
        (tip.as_str(), squash.as_str())
    );
    assert_eq!(
        (receipt.parent.as_str(), receipt.base_observed.as_str()),
        (base.as_str(), base.as_str())
    );
    assert_eq!(receipt.revision, read.revision);
    assert_eq!(receipt.tree, head(&r.root, "main^{tree}"));
    assert_eq!(
        head(&r.root, &format!("refs/symphony/delivered/{SLUG}")),
        tip
    );
    assert!(
        !Journal::path(&lands(&r), SLUG).exists(),
        "nothing is left to finish"
    );
    assert!(
        said.contains("land sim/jump: its issue is closed"),
        "{said}"
    );
    // Asked again, it delivers nothing twice.
    let again = Host::new(&r, &bare);
    let (_, result) = landed(&r, SLUG, &people.orch, &again);
    assert!(again.verbs().is_empty());
    let line = result.unwrap_err();
    assert!(
        line.starts_with(&format!(
            "{SLUG} was closed after its approval: landed {SLUG} on main"
        )),
        "{line}"
    );
    assert_eq!(head(&bare, "main"), squash, "delivered once");
    // Nor does its head go back to review: its receipt says it landed, as no history does.
    let reopened = symphony_board::land::reopenable(&r.board.store, Some(&r.env(&r.root)), SLUG);
    assert_eq!(
        reopened.unwrap_err().to_string(),
        format!(
            "{SLUG}'s tip {} is on main already, squashed as {} by pull request #3: it does not \
             go back to review, and what it still needs goes in a new task",
            &tip[..7],
            &squash[..7]
        )
    );
}

#[test]
fn a_task_assessed_as_needing_no_stamp_is_delivered_from_review_with_each_waiver_said() {
    let (r, bare) = delivering();
    let (people, _, tip) = in_review(&r, SLUG, "docs/design.md");
    assess(&r, SLUG, &people.assessor, &[], &tip);
    let host = Host::new(&r, &bare);
    let (_, result) = landed(&r, SLUG, &people.orch, &host);
    let line = result.unwrap();
    assert!(
        line.contains(&format!(
            "; its needs (none) stood at {tip}; squashed from {tip}"
        )),
        "{line}"
    );
    let snapshot = host.request("publish")["snapshot"].clone();
    assert_eq!(snapshot["assessed"], true);
    for stamp in snapshot["stamps"].as_array().unwrap() {
        assert_eq!(stamp["standing"], "waived", "{stamp}");
        assert_eq!(
            stamp["detail"],
            format!(
                "Waived by the assessment of {}: as the guidelines say",
                people.assessor
            )
        );
    }
}

#[test]
fn the_base_moving_alone_keeps_the_heads_evidence() {
    let (r, bare) = delivering();
    let (people, _, tip) = ready(&r, SLUG);
    // The host's main gains a commit the checkout has not fetched.
    let other = r.dir.join("elsewhere");
    git(
        &r.dir,
        &[
            "clone",
            "-q",
            &bare.to_string_lossy(),
            &other.to_string_lossy(),
        ],
    );
    for (key, value) in [("user.email", "o@example.com"), ("user.name", "o")] {
        git(&other, &["config", key, value]);
    }
    write(&other, "sim/other.gd", "extends Node\n");
    git(&other, &["add", "-A"]);
    git(&other, &["commit", "-qm", "pushed elsewhere"]);
    git(&other, &["push", "-q", "origin", "main"]);
    let moved = head(&other, "HEAD");
    let host = Host::new(&r, &bare);
    let (_, result) = landed(&r, SLUG, &people.orch, &host);
    result.unwrap();
    let receipt = Receipt::read(&lands(&r), SLUG).unwrap();
    assert_eq!(
        receipt.parent, moved,
        "squashed on the base as it was when merged"
    );
    assert_eq!(receipt.head, tip);
    assert_eq!(head(&r.root, "main"), head(&bare, "main"));
    assert!(r.root.join("sim/other.gd").exists() && r.root.join("sim/jump.gd").exists());
}

// -- what refuses before anything is asked ---------------------------------------------------------

#[test]
fn evidence_that_does_not_name_the_head_refuses_before_anything_is_published() {
    // No assessment: the catalogue's default, an approve, lands a task here, never by a pull
    // request.
    let (r, bare) = delivering();
    let before = head(&bare, "main");
    let (people, _, tip) = in_review(&r, SLUG, "sim/jump.gd");
    approve(&r, SLUG, &format!("{tip}: reads well"));
    let host = Host::new(&r, &bare);
    let (_, result) = landed(&r, SLUG, &people.orch, &host);
    let line = result.unwrap_err();
    assert!(
        line.starts_with(&format!(
            "{SLUG} is not eligible for delivery by pull request at"
        )),
        "{line}"
    );
    assert!(
        line.contains("no assessment names what sim/jump needs"),
        "{line}"
    );
    assert!(host.verbs().is_empty());
    assert_nothing_landed(&r, &bare, &before);

    // An assessment of another commit waives nothing at the head.
    let (r, bare) = delivering();
    let (people, wt, old) = in_review(&r, SLUG, "docs/design.md");
    assess(&r, SLUG, &people.assessor, &[], &old);
    write(&wt, "docs/design.md", "# more\n");
    git(&wt, &["commit", "-qam", "more"]);
    let tip = head(&wt, "HEAD");
    approve(&r, SLUG, &format!("{tip}: reads well"));
    let host = Host::new(&r, &bare);
    let (_, result) = landed(&r, SLUG, &people.orch, &host);
    let line = result.unwrap_err();
    assert!(
        line.contains(&format!("read {}, not the head {}", &old[..7], &tip[..7])),
        "{line}"
    );
    assert!(host.verbs().is_empty());

    // An approve that names no commit covers the branch by time here, and no pull request.
    let (r, bare) = delivering();
    let (people, _, tip) = in_review(&r, SLUG, "sim/jump.gd");
    assess(&r, SLUG, &people.assessor, &["review"], &tip);
    approve(&r, SLUG, "reads well");
    let host = Host::new(&r, &bare);
    let (_, result) = landed(&r, SLUG, &people.orch, &host);
    let line = result.unwrap_err();
    assert!(line.contains("does not name the head"), "{line}");
    assert!(host.verbs().is_empty());
}

#[test]
fn a_needed_stamp_that_is_red_refuses_as_it_does_here() {
    let (r, bare) = delivering();
    let before = head(&bare, "main");
    let (people, _, tip) = in_review(&r, SLUG, "sim/jump.gd");
    assess(&r, SLUG, &people.assessor, &["tests"], &tip);
    ran(&r, SLUG, &tip, "cargo test -p sim", 1);
    let host = Host::new(&r, &bare);
    let (_, result) = landed(&r, SLUG, &people.orch, &host);
    let line = result.unwrap_err();
    assert!(
        line.starts_with(&format!("{SLUG} lacks at its tip")),
        "{line}"
    );
    assert!(host.verbs().is_empty());
    assert_nothing_landed(&r, &bare, &before);
    // The api says the same of it: one reading.
    let mut env = r.env(&r.root);
    (env.vars).insert(FOLDER_VAR.into(), work(&r).to_string_lossy().into_owned());
    let read = evidence::of(&r.board.store, &env, SLUG, None).unwrap();
    assert!(!read.eligible);
    assert_eq!(read.blocking.len(), 1, "{:?}", read.blocking);
    assert!(
        read.blocking[0].starts_with("tests: red at"),
        "{:?}",
        read.blocking
    );
    let other = "0".repeat(40);
    assert_eq!(
        evidence::of(&r.board.store, &env, SLUG, Some(&other)).unwrap_err(),
        format!("{SLUG}'s branch is at {tip}, not {other}: evidence is of the head alone")
    );
}

#[test]
fn the_evidence_says_a_stamp_a_gate_decision_waived_at_the_head_and_blocks_on_a_raise_not_made() {
    use symphony_board::gates::{Decision, Kind};
    let (r, _) = delivering();
    write(&r.root, "tools/size-budget.toml", "symphony = 100\n");
    git(&r.root, &["add", "-A"]);
    git(&r.root, &["commit", "-qm", "the budget"]);
    let (people, wt, _) = in_review(&r, SLUG, "ui/rows.gd");
    write(&wt, "tools/size-budget.toml", "symphony = 130\n");
    git(&wt, &["commit", "-qam", "past the line"]);
    let tip = head(&wt, "HEAD");
    assess(&r, SLUG, &people.assessor, &["tests", "pictures"], &tip);
    ran(&r, SLUG, &tip, "cargo test -p sim", 0);
    let decide = |decision: Decision| {
        (r.board.store)
            .decide_gate(SLUG, &Catalogue::symphony(), &decision)
            .unwrap();
    };
    let waiver = Decision {
        gate: "pictures".into(),
        kind: Kind::Waive,
        from: None,
        to: None,
        tip: Some(tip.clone()),
        by: people.orch.clone(),
        why: "nothing a page shows".into(),
        at: 0.0,
    };
    decide(waiver.clone());
    decide(Decision {
        gate: "tools/size-budget.toml:symphony".into(),
        kind: Kind::Raise,
        from: Some(100),
        to: Some(120),
        tip: None,
        why: "over after the cut".into(),
        ..waiver
    });
    let mut env = r.env(&r.root);
    (env.vars).insert(FOLDER_VAR.into(), work(&r).to_string_lossy().into_owned());
    let read = evidence::of(&r.board.store, &env, SLUG, None).unwrap();
    let pictures = read
        .stamps
        .iter()
        .find(|stamp| stamp.name == "pictures")
        .unwrap();
    assert_eq!(pictures.standing, evidence::Standing::Waived);
    assert_eq!(
        pictures.detail,
        format!(
            "Waived at {} by the gate decision of {}: nothing a page shows",
            &tip[..7],
            people.orch
        )
    );
    assert_eq!(
        read.blocking,
        [format!(
            "tools/size-budget.toml:symphony raised 100 -> 120 by {}, but the tip {} sets it to \
             130, which no decision gave: a raise lands at the number decided",
            people.orch,
            &tip[..7]
        )]
    );
    assert!(!read.eligible);
}

#[test]
fn a_head_changing_generated_paths_or_merging_another_history_is_refused() {
    let r = Repo::new();
    project_file(
        &r,
        "[land]\ncheck = false\ndelivery = [\"host\"]\ngenerators = [{ name = \"the client\", \
         outputs = \"dist/\", command = [\"true\"] }]\n",
    );
    let bare = remote(&r);
    let before = head(&bare, "main");
    let (people, wt, _) = in_review(&r, SLUG, "sim/jump.gd");
    write(&wt, "dist/app.js", "built\n");
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-qm", "its own build"]);
    let tip = head(&wt, "HEAD");
    assess(&r, SLUG, &people.assessor, &[], &tip);
    let host = Host::new(&r, &bare);
    let (_, result) = landed(&r, SLUG, &people.orch, &host);
    assert_eq!(
        result.unwrap_err(),
        format!(
            "{SLUG} changes 1 generated path (dist/app.js), which a pull request would deliver as \
             the branch built it"
        )
    );
    assert!(host.verbs().is_empty());
    assert_nothing_landed(&r, &bare, &before);

    // A merge of a commit the host's main does not hold: an import, which a squash would flatten.
    let (r, bare) = delivering();
    let (people, wt, _) = in_review(&r, SLUG, "sim/jump.gd");
    let upstream = r.add("upstream", Some("upstream"), "main");
    write(&upstream, "sim/up.gd", "extends Node\n");
    git(&upstream, &["add", "-A"]);
    git(&upstream, &["commit", "-qm", "upstream's own"]);
    let foreign = head(&upstream, "HEAD");
    git(&wt, &["merge", "-q", "--no-ff", "-m", "import", &foreign]);
    let tip = head(&wt, "HEAD");
    assess(&r, SLUG, &people.assessor, &[], &tip);
    let host = Host::new(&r, &bare);
    let (_, result) = landed(&r, SLUG, &people.orch, &host);
    let line = result.unwrap_err();
    assert!(
        line.starts_with(&format!(
            "{SLUG}'s head {} merges {}",
            &tip[..7],
            &foreign[..7]
        )),
        "{line}"
    );
    assert!(host.verbs().is_empty());
    let (told, to) = {
        let post = r.board.store.thread(SLUG).unwrap().pop().unwrap();
        (post.text, post.recipient)
    };
    assert_eq!(to.as_deref(), Some("@orchestrator"));
    assert!(
        told.contains(
            "An import of another history is delivered only from the upstream symphony.toml \
             names ([land.upstream]), assessed as one: nothing landed"
        ),
        "{told}"
    );
}

#[test]
fn a_project_file_the_remote_default_does_not_commit_refuses() {
    let (r, bare) = delivering();
    let (people, ..) = ready(&r, SLUG);
    // Committed here and pushed nowhere: the remote's is another.
    project_file(&r, &format!("{DELIVERS}push = false\n"));
    let host = Host::new(&r, &bare);
    let (_, result) = landed(&r, SLUG, &people.orch, &host);
    let line = result.unwrap_err();
    assert!(
        line.starts_with("symphony.toml here is not the one origin/main commits at"),
        "{line}"
    );
    assert!(host.verbs().is_empty());
}

// -- the pull request and its head -----------------------------------------------------------------

#[test]
fn a_pull_request_at_another_head_or_bound_to_another_number_merges_nothing() {
    let (r, bare) = delivering();
    let before = head(&bare, "main");
    let (people, ..) = ready(&r, SLUG);
    let mut host = Host::new(&r, &bare);
    host.says_head = Some("1".repeat(40));
    let (_, result) = landed(&r, SLUG, &people.orch, &host);
    let line = result.unwrap_err();
    assert!(
        line.starts_with("pull request #3 is at 1111111, not the head"),
        "{line}"
    );
    assert_eq!(host.verbs(), ["publish"]);
    assert_nothing_landed(&r, &bare, &before);
    // The journal a run left binds the task to one pull request: another is refused.
    let mut journal = Journal::read(&lands(&r), SLUG).unwrap();
    assert_eq!(journal.stage, Stage::Prepared);
    journal.binding = Some(symphony_board::land::delivery::Binding {
        repository: 7,
        name: "owner/name".into(),
        pull: 9,
        url: String::new(),
    });
    journal.write(&lands(&r)).unwrap();
    let host = Host::new(&r, &bare);
    let (_, result) = landed(&r, SLUG, &people.orch, &host);
    let line = result.unwrap_err();
    assert!(
        line.contains("was bound to pull request #9 of repository 7 and is now answered with #3"),
        "{line}"
    );
    // The pull request the journal names is read first, then the one the command answers with.
    assert_eq!(host.verbs(), ["result", "publish"]);
}

#[test]
fn a_merge_the_host_refuses_leaves_the_task_open_and_names_who_acts() {
    for (kind, to) in [
        ("conflict", "@executor"),
        ("head", "@executor"),
        ("checks", "@orchestrator"),
    ] {
        let (r, bare) = delivering();
        let before = head(&bare, "main");
        let (people, _, tip) = ready(&r, SLUG);
        let host = Host::new(&r, &bare).merging(Merges::Refuses(kind));
        let (_, result) = landed(&r, SLUG, &people.orch, &host);
        assert_eq!(
            result.unwrap_err(),
            format!(
                "owner/name refused to squash {SLUG} at {} by pull request #3: no: {kind}",
                &tip[..7]
            )
        );
        assert_eq!(host.verbs(), ["publish", "arm", "merge", "result"]);
        assert_nothing_landed(&r, &bare, &before);
        let post = r.board.store.thread(SLUG).unwrap().pop().unwrap();
        assert_eq!(post.recipient.as_deref(), Some(to));
        // Asked again only by a run that validates again.
        assert_eq!(
            Journal::read(&lands(&r), SLUG).unwrap().stage,
            Stage::Eligible
        );
    }
}

// -- a lost answer ---------------------------------------------------------------------------------

#[test]
fn a_merge_whose_answer_was_lost_is_adopted_from_the_pull_request_and_asked_once() {
    let (r, bare) = delivering();
    let (people, _, tip) = ready(&r, SLUG);
    // The merge is made and neither its answer nor the first reading of it arrives.
    let host = Host::new(&r, &bare).losing(&["merge", "result"]);
    let (_, result) = landed(&r, SLUG, &people.orch, &host);
    let line = result.unwrap_err();
    assert!(
        line.contains("was asked for and its result is not known"),
        "{line}"
    );
    assert!(line.contains("nothing is merged here"), "{line}");
    assert_eq!(
        Journal::read(&lands(&r), SLUG).unwrap().stage,
        Stage::MergeRequested
    );
    assert_eq!(
        head(&r.root, "main"),
        git(&r.root, &["rev-parse", "origin/main"]),
        "nothing moved here"
    );
    assert!(landings(&r, SLUG).is_empty());
    // The next run reads the pull request before anything else and asks for no second merge.
    let (said, result) = landed(&r, SLUG, &people.orch, &host);
    let line = result.unwrap();
    assert_eq!(
        host.verbs(),
        ["publish", "arm", "merge", "result", "result", "landed"]
    );
    assert!(
        said.starts_with(&format!(
            "land {SLUG}: a delivery is under way (merge requested at"
        )),
        "{said}"
    );
    let squash = head(&bare, "main");
    assert!(
        line.starts_with(&format!("landed {SLUG} on main at {}", &squash[..12])),
        "{line}"
    );
    assert_eq!(head(&r.root, "main"), squash);
    assert_eq!(landings(&r, SLUG).len(), 1);
    assert_eq!(Receipt::read(&lands(&r), SLUG).unwrap().head, tip);
}

#[test]
fn once_a_merge_was_asked_for_no_run_merges_here_whatever_the_settings_say() {
    let (r, bare) = delivering();
    let (people, _, tip) = ready(&r, SLUG);
    let host = Host::new(&r, &bare).losing(&["merge", "result"]);
    landed(&r, SLUG, &people.orch, &host).1.unwrap_err();
    // The key is taken out of the project file, here and on the host.
    project_file(&r, "[land]\ncheck = false\n");
    let local = head(&r.root, "main");
    let mut out = Vec::new();
    let result = land(
        &r.board.store,
        &r.env(&r.root),
        SLUG,
        &people.orch,
        None,
        &mut out,
    );
    let line = refusal(result.unwrap_err());
    assert!(
        line.starts_with(&format!(
            "{SLUG} has a delivery by pull request under way (merge requested at {}, pull request \
             #3 of owner/name), and [land] delivery is no longer set on main: nothing is merged \
             here over it",
            &tip[..7]
        )),
        "{line}"
    );
    assert_eq!(head(&r.root, "main"), local, "nothing merged here");
    assert!(landings(&r, SLUG).is_empty());
    assert!(
        Journal::path(&lands(&r), SLUG).exists(),
        "the journal is kept"
    );
}

#[test]
fn a_journal_that_asked_for_no_merge_does_not_hold_a_land_made_here() {
    let (r, bare) = delivering();
    let (people, _, tip) = ready(&r, SLUG);
    let mut host = Host::new(&r, &bare);
    host.says_head = Some("1".repeat(40));
    landed(&r, SLUG, &people.orch, &host).1.unwrap_err();
    assert!(Journal::path(&lands(&r), SLUG).exists());
    project_file(&r, "[land]\ncheck = false\npush = false\n");
    let mut env = r.env(&r.root);
    (env.vars).insert(FOLDER_VAR.into(), work(&r).to_string_lossy().into_owned());
    let mut out = Vec::new();
    land(&r.board.store, &env, SLUG, &people.orch, None, &mut out)
        .map_err(refusal)
        .unwrap();
    assert_eq!(head(&r.root, "main^2"), tip);
    assert!(!Journal::path(&lands(&r), SLUG).exists());
}

// -- a commit that is not what was read ------------------------------------------------------------

#[test]
fn a_commit_that_is_not_the_squash_of_the_head_is_held_and_closes_nothing() {
    for (merges, said) in [
        (Merges::WrongTree, "is not what"),
        (
            Merges::ByCommit,
            "has 2 parents: the head was merged by a merge commit, not squashed",
        ),
        (Merges::Closes, "pull request #3 was closed without a merge"),
    ] {
        let (r, bare) = delivering();
        let local = head(&r.root, "main");
        let (people, ..) = ready(&r, SLUG);
        let host = Host::new(&r, &bare).merging(merges);
        let (_, result) = landed(&r, SLUG, &people.orch, &host);
        let line = result.unwrap_err();
        assert!(
            line.starts_with(&format!(
                "the delivery of {SLUG} is held for reconciliation: "
            )),
            "{line}"
        );
        assert!(line.contains(said), "{line}");
        assert!(
            line.ends_with("Nothing is closed and nothing is merged here"),
            "{line}"
        );
        assert_eq!(
            head(&r.root, "main"),
            local,
            "the checkout follows nothing unproved"
        );
        assert!(landings(&r, SLUG).is_empty());
        assert_eq!(
            r.board.store.get(SLUG).unwrap().status,
            "done",
            "its approve stands, unlanded"
        );
        assert!(
            Receipt::read(&lands(&r), SLUG).is_none(),
            "no receipt of what was not proved"
        );
        let posts = |r: &Repo| {
            (r.board.said(SLUG).iter())
                .filter(|line| line.contains("held for reconciliation"))
                .count()
        };
        assert_eq!(posts(&r), 1);
        // A run again reads the pull request, finds it as it was, and says it once.
        let (_, again) = landed(&r, SLUG, &people.orch, &host);
        assert_eq!(again.unwrap_err(), line);
        assert_eq!(posts(&r), 1);
        assert_eq!(
            host.verbs().iter().filter(|verb| *verb == "merge").count(),
            1
        );
    }
}

// -- a remote that cannot be read, and an answer git does not bear out ----------------------------

#[test]
fn with_no_remote_to_deliver_to_nothing_is_asked() {
    let r = Repo::new();
    project_file(&r, DELIVERS);
    let (people, ..) = ready(&r, SLUG);
    let bare = r.dir.join("nowhere.git");
    let host = Host::new(&r, &bare);
    let (_, result) = landed(&r, SLUG, &people.orch, &host);
    assert_eq!(
        result.unwrap_err(),
        "[land] delivery is set and there is no origin remote: a delivery by pull request has a \
         remote to deliver to"
    );
    assert!(host.verbs().is_empty());
    assert!(landings(&r, SLUG).is_empty());
}

#[test]
fn a_remote_that_cannot_be_fetched_after_the_merge_proves_nothing_until_it_can() {
    let (r, bare) = delivering();
    let (people, _, tip) = ready(&r, SLUG);
    let local = head(&r.root, "main");
    let away = r.dir.join("away.git");
    // The host merges, and the remote is gone before the commit can be fetched.
    let host = Host::new(&r, &bare).before("result", || fs::rename(&bare, &away).unwrap());
    let (_, result) = landed(&r, SLUG, &people.orch, &host);
    let line = result.unwrap_err();
    assert!(
        line.starts_with(&format!(
            "the merge of {SLUG} by pull request #3 was asked for and its result is not known ("
        )),
        "{line}"
    );
    assert!(line.contains("nothing is merged here"), "{line}");
    assert_eq!(head(&r.root, "main"), local, "nothing moved here");
    assert!(landings(&r, SLUG).is_empty() && Receipt::read(&lands(&r), SLUG).is_none());
    assert_eq!(
        Journal::read(&lands(&r), SLUG).unwrap().stage,
        Stage::MergeRequested
    );
    // Still gone: the same, and no second merge is asked for.
    landed(&r, SLUG, &people.orch, &host).1.unwrap_err();
    assert_eq!(
        host.verbs().iter().filter(|verb| *verb == "merge").count(),
        1
    );
    // Back: the commit is fetched, proved and followed.
    fs::rename(&away, &bare).unwrap();
    let (_, result) = landed(&r, SLUG, &people.orch, &host);
    result.unwrap();
    assert_eq!(head(&r.root, "main"), head(&bare, "main"));
    assert_eq!(Receipt::read(&lands(&r), SLUG).unwrap().head, tip);
    assert_eq!(
        host.verbs().iter().filter(|verb| *verb == "merge").count(),
        1
    );
    assert_eq!(landings(&r, SLUG).len(), 1);
}

#[test]
fn an_answer_git_does_not_bear_out_closes_nothing() {
    let unknown = "9".repeat(40);
    for (says, said, held) in [
        // A commit the remote does not give: not proved yet, and not held against anyone.
        (
            json!({"sha": unknown}),
            "does not give the commit 9999999",
            false,
        ),
        (json!({"sha": null}), "merged, with no commit named", false),
        (json!({"state": "stale"}), "its result is not known", false),
        // A commit that is there and is not on the default branch: the head itself.
        (json!({"sha": "HEAD"}), "is not on origin/main", true),
        (
            json!({"head": "1".repeat(40)}),
            "was merged at 1111111, not the head",
            true,
        ),
        (
            json!({"repository": 8}),
            "answers for repository 8 and base \"main\", not the ones bound",
            true,
        ),
        (
            json!({"base": "other"}),
            "answers for repository 7 and base \"other\"",
            true,
        ),
    ] {
        let (r, bare) = delivering();
        let local = head(&r.root, "main");
        let (people, _, tip) = ready(&r, SLUG);
        let says = if says["sha"] == "HEAD" {
            json!({"sha": tip})
        } else {
            says
        };
        let host = Host::new(&r, &bare).saying("result", says);
        let (_, result) = landed(&r, SLUG, &people.orch, &host);
        let line = result.unwrap_err();
        assert!(line.contains(said), "{line}");
        assert_eq!(
            line.starts_with("the delivery of sim/jump is held for reconciliation: "),
            held,
            "{line}"
        );
        assert_eq!(
            head(&r.root, "main"),
            local,
            "the checkout follows nothing unproved"
        );
        assert!(landings(&r, SLUG).is_empty(), "nothing is closed");
        assert!(Receipt::read(&lands(&r), SLUG).is_none());
        assert_eq!(
            host.verbs().iter().filter(|verb| *verb == "landed").count(),
            0
        );
    }
}

#[test]
fn a_merge_given_no_usable_answer_whose_pull_request_is_still_open_is_asked_again_by_the_next_land()
{
    let (r, bare) = delivering();
    let before = head(&bare, "main");
    let (people, ..) = ready(&r, SLUG);
    // The program gives no usable answer to the merge, and reads the pull request as open.
    let host = Host::new(&r, &bare)
        .saying(
            "merge",
            json!({"state": "unavailable", "why": "the host is away"}),
        )
        .saying("result", json!({"state": "open", "sha": null}));
    let (_, result) = landed(&r, SLUG, &people.orch, &host);
    assert_eq!(
        result.unwrap_err(),
        format!("pull request #3 of {SLUG} is still open: nothing merged")
    );
    assert_eq!(head(&r.root, "main"), before);
    assert_eq!(
        Journal::read(&lands(&r), SLUG).unwrap().stage,
        Stage::Eligible
    );
    let post = r.board.store.thread(SLUG).unwrap().pop().unwrap();
    assert_eq!(post.recipient.as_deref(), Some("@orchestrator"));
}

// -- a pull request bound by a run before ----------------------------------------------------------

/// `slug` published and its merge refused by the host's checks: bound to its pull request, which
/// is open and says the evidence stands.
fn bound<'r>(r: &'r Repo, bare: &Path, people: &People) -> Host<'r> {
    let host = Host::new(r, bare).merging(Merges::Refuses("checks"));
    landed(r, SLUG, &people.orch, &host).1.unwrap_err();
    assert_eq!(host.verbs(), ["publish", "arm", "merge", "result"]);
    assert_eq!(host.request("publish")["snapshot"]["eligible"], true);
    host
}

#[test]
fn evidence_withdrawn_at_a_bound_head_is_published_as_blocking_before_the_land_refuses() {
    // A red run at the head, and a new head nobody assessed. A task done by its approve takes
    // no later verdict or assessment on the board, so these are what withdraws its evidence.
    for withdrawn in ["red", "moved"] {
        let (r, bare) = delivering();
        let before = head(&bare, "main");
        let (people, wt, tip) = ready(&r, SLUG);
        let host = bound(&r, &bare, &people);
        let mut now = tip.clone();
        match withdrawn {
            "red" => ran(&r, SLUG, &tip, "cargo test -p sim", 1),
            _ => {
                write(&wt, "sim/jump.gd", "extends Node # more\n");
                git(&wt, &["commit", "-qam", "more"]);
                now = head(&wt, "HEAD");
            }
        }
        let (said, result) = landed(&r, SLUG, &people.orch, &host);
        result.unwrap_err();
        // The pull request is read, then told: the head as it is and a snapshot that blocks.
        assert_eq!(host.verbs()[4..], ["result", "publish"], "{withdrawn}");
        let published = host.request("publish");
        assert_eq!(published["head"], now.as_str(), "{withdrawn}");
        assert_eq!(published["snapshot"]["eligible"], false, "{withdrawn}");
        assert_eq!(published["snapshot"]["head"], now.as_str());
        assert!(
            !published["snapshot"]["blocking"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(
            said.contains(&format!(
                "its evidence does not stand at {}, and pull request #3 of owner/name says so now",
                &now[..7]
            )),
            "{withdrawn}: {said}"
        );
        assert_eq!(
            host.verbs().iter().filter(|verb| *verb == "merge").count(),
            1
        );
        assert_nothing_landed(&r, &bare, &before);
        // Asked again as it is: told again, which the command answers by finding what stands.
        landed(&r, SLUG, &people.orch, &host).1.unwrap_err();
        assert_eq!(host.verbs()[6..], ["result", "publish"], "{withdrawn}");
    }
}

#[test]
fn evidence_that_stands_at_a_bound_head_is_published_once_by_the_delivery_itself() {
    let (r, bare) = delivering();
    let (people, ..) = ready(&r, SLUG);
    let mut host = bound(&r, &bare, &people);
    host.merges = Merges::Squash;
    let (_, result) = landed(&r, SLUG, &people.orch, &host);
    result.unwrap();
    assert_eq!(
        host.verbs()[4..],
        ["result", "publish", "arm", "merge", "result", "landed"]
    );
    assert_eq!(host.request("publish")["snapshot"]["eligible"], true);
}

#[test]
fn a_bound_pull_request_merged_since_it_was_read_as_open_is_adopted_or_held_never_published_to() {
    // The host finished a merge whose answer was lost and read as open, or a person merged it.
    let (r, bare) = delivering();
    let (people, _, tip) = ready(&r, SLUG);
    let host = bound(&r, &bare, &people);
    let squash = host.merge(&tip, "by hand", "squash");
    host.pull.borrow_mut().1 = Some(squash.clone());
    let (said, result) = landed(&r, SLUG, &people.orch, &host);
    let line = result.unwrap();
    assert!(
        said.contains("was merged since it was read as open: going on from it"),
        "{said}"
    );
    assert!(
        line.starts_with(&format!("landed {SLUG} on main at {}", &squash[..12])),
        "{line}"
    );
    assert_eq!(
        host.verbs()[4..],
        ["result", "result", "landed"],
        "no publication, no merge"
    );
    assert_eq!(head(&r.root, "main"), squash);
    assert_eq!(Receipt::read(&lands(&r), SLUG).unwrap().head, tip);
    assert_eq!(landings(&r, SLUG).len(), 1);

    // Merged by hand as a merge commit: held by name, on every run the same, said once.
    let (r, bare) = delivering();
    let local = head(&r.root, "main");
    let (people, _, tip) = ready(&r, SLUG);
    let mut host = bound(&r, &bare, &people);
    host.merges = Merges::ByCommit;
    let made = host.merge(&tip, "by hand", "squash");
    host.pull.borrow_mut().1 = Some(made);
    for _ in 0..2 {
        let (_, result) = landed(&r, SLUG, &people.orch, &host);
        let line = result.unwrap_err();
        assert!(
            line.starts_with("the delivery of sim/jump is held for reconciliation: "),
            "{line}"
        );
        assert!(line.contains("has 2 parents"), "{line}");
    }
    assert_eq!(
        host.verbs()
            .iter()
            .filter(|verb| *verb == "publish")
            .count(),
        1
    );
    assert_eq!(head(&r.root, "main"), local);
    assert!(landings(&r, SLUG).is_empty() && Receipt::read(&lands(&r), SLUG).is_none());
    let held = (r.board.said(SLUG).iter())
        .filter(|line| line.contains("held for reconciliation"))
        .count();
    assert_eq!(held, 1);

    // Merged by hand after its evidence was withdrawn, after the branch moved on, or closed:
    // nothing is adopted, and each is held by its name.
    for (case, said) in [
        ("red", "the evidence of sim/jump does not stand there"),
        ("moved", "'s branch is at"),
        ("closed", "pull request #3 was closed without a merge"),
    ] {
        let (r, bare) = delivering();
        let local = head(&r.root, "main");
        let (people, wt, tip) = ready(&r, SLUG);
        let host = bound(&r, &bare, &people);
        if case == "closed" {
            host.pull.borrow_mut().2 = true;
        } else {
            let squash = host.merge(&tip, "by hand", "squash");
            host.pull.borrow_mut().1 = Some(squash);
        }
        match case {
            "red" => ran(&r, SLUG, &tip, "cargo test -p sim", 1),
            "moved" => {
                write(&wt, "sim/jump.gd", "extends Node # more\n");
                git(&wt, &["commit", "-qam", "more"]);
            }
            _ => {}
        }
        let (_, result) = landed(&r, SLUG, &people.orch, &host);
        let line = result.unwrap_err();
        assert!(
            line.starts_with("the delivery of sim/jump is held for reconciliation: "),
            "{case}: {line}"
        );
        assert!(line.contains(said), "{case}: {line}");
        assert_eq!(
            host.verbs()[4..],
            ["result"],
            "{case}: nothing published, nothing merged"
        );
        assert_eq!(head(&r.root, "main"), local, "{case}");
        assert!(landings(&r, SLUG).is_empty() && Receipt::read(&lands(&r), SLUG).is_none());
    }
}

// -- the project's merge check ---------------------------------------------------------------------

#[test]
fn a_project_whose_land_checks_its_merge_delivers_nothing_past_that_check() {
    // `[land] check` left as it is, on, with a check to run: the default land runs it on the
    // merge, and no squash made by a host is run through it.
    let r = Repo::new();
    project_file(
        &r,
        "[land]\ndelivery = [\"host\", \"deliver\"]\n\n[verdicts]\ncheck = [\"sh\", \"-c\", \"true\"]\n",
    );
    let bare = remote(&r);
    let before = head(&bare, "main");
    let (people, ..) = ready(&r, SLUG);
    let host = Host::new(&r, &bare);
    let (_, result) = landed(&r, SLUG, &people.orch, &host);
    assert_eq!(
        result.unwrap_err(),
        "[land] delivery is set and so is the land's merge check (`sh -c true`), which a squash \
         made by the host is never run on: nothing is delivered past a check the project asks for"
    );
    assert!(host.verbs().is_empty());
    assert_nothing_landed(&r, &bare, &before);
    let post = r.board.store.thread(SLUG).unwrap().pop().unwrap();
    assert_eq!(post.recipient.as_deref(), Some("@orchestrator"));
    // With the check on and none to run, nothing is lost and the task is delivered.
    let r = Repo::new();
    project_file(&r, "[land]\ndelivery = [\"host\", \"deliver\"]\n");
    let bare = remote(&r);
    let (people, ..) = ready(&r, SLUG);
    landed(&r, SLUG, &people.orch, &Host::new(&r, &bare))
        .1
        .unwrap();
}

// -- a checkout that cannot follow -----------------------------------------------------------------

#[test]
fn a_main_checkout_that_cannot_follow_is_landed_remotely_and_closed_once_it_can() {
    for diverged in [false, true] {
        let (r, bare) = delivering();
        project_file(
            &r,
            &format!(
                "{DELIVERS}\n[board]\non_land = [\"sh\", \"-c\", \"echo $1 $2 >> '{}'\", \"hook\"]\n",
                r.dir.join("landings").display()
            ),
        );
        git(&r.root, &["push", "-q", "origin", "main"]);
        let (people, ..) = ready(&r, SLUG);
        let dirty = || write(&r.root, "sim/world.gd", "extends RefCounted # edited\n");
        let host = if diverged {
            // A commit made here while the host merges: main here holds what the remote lacks.
            Host::new(&r, &bare).before("result", || {
                write(&r.root, "sim/local.gd", "extends Node\n");
                git(&r.root, &["add", "-A"]);
                git(&r.root, &["commit", "-qm", "made here"]);
            })
        } else {
            Host::new(&r, &bare).before("result", dirty)
        };
        let vars = [("TASKS_ON_LAND", "on"), ("TASKS_ON_LAND_SAYS", "60")];
        let (_, result) = landed_with(&r, SLUG, &people.orch, &host, &vars);
        let local = head(&r.root, "main");
        let squash = head(&bare, "main");
        let line = result.unwrap_err();
        assert!(
            line.starts_with(&format!(
                "{SLUG} landed remotely as {} by pull request #3 of owner/name; reconciliation \
                 pending: ",
                &squash[..7]
            )),
            "{line}"
        );
        assert!(
            line.contains("Nothing here was reset, merged or pushed"),
            "{line}"
        );
        assert_ne!(local, squash);
        assert!(
            landings(&r, SLUG).is_empty(),
            "not closed until the checkout follows"
        );
        assert_eq!(
            Journal::read(&lands(&r), SLUG).unwrap().stage,
            Stage::RemoteMerged
        );
        assert!(
            Receipt::read(&lands(&r), SLUG).is_some(),
            "the merge is proved and kept"
        );
        // Tried again as it is: the same, said on the thread once.
        landed_with(&r, SLUG, &people.orch, &host, &vars)
            .1
            .unwrap_err();
        let said = (r.board.said(SLUG).iter())
            .filter(|line| line.contains("reconciliation pending"))
            .count();
        assert_eq!(said, 1);
        assert_eq!(head(&r.root, "main"), local, "never reset or merged over");
        // Put right by a person: the edit dropped, or the commit made here taken off main.
        if diverged {
            git(&r.root, &["reset", "-q", "--hard", "origin/main~1"]);
        } else {
            git(&r.root, &["checkout", "-q", "--", "sim/world.gd"]);
        }
        let (_, result) = landed_with(&r, SLUG, &people.orch, &host, &vars);
        let line = result.unwrap();
        assert!(
            line.starts_with(&format!("landed {SLUG} on main at {}", &squash[..12])),
            "{line}"
        );
        assert_eq!(head(&r.root, "main"), squash);
        assert_eq!(landings(&r, SLUG).len(), 1);
        assert_eq!(
            host.verbs().iter().filter(|verb| *verb == "merge").count(),
            1
        );
        // What follows a land was started once, with the commit the host made.
        let counted = fs::read_to_string(r.dir.join("landings")).unwrap_or_default();
        assert_eq!(counted, format!("{SLUG} {squash}\n"));
    }
}

#[test]
fn a_run_cut_short_after_the_board_closed_the_task_closes_and_dispatches_nothing_twice() {
    let (r, bare) = delivering();
    let (people, ..) = ready(&r, SLUG);
    let host = Host::new(&r, &bare);
    landed(&r, SLUG, &people.orch, &host).1.unwrap();
    let squash = head(&bare, "main");
    let journal = |stage| Journal {
        slug: SLUG.into(),
        operation: "task".into(),
        stage,
        head: Receipt::read(&lands(&r), SLUG).unwrap().head,
        base_observed: String::new(),
        revision: String::new(),
        title: String::new(),
        body: String::new(),
        by: people.orch.clone(),
        closes: vec!["done".into()],
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
    };
    // Killed after the board's write and before the journal said closed: the line is found.
    journal(Stage::Reconciled).write(&lands(&r)).unwrap();
    let (_, result) = landed(&r, SLUG, &people.orch, &host);
    result.unwrap();
    assert_eq!(landings(&r, SLUG).len(), 1, "closed once");
    assert_eq!(
        host.verbs().iter().filter(|verb| *verb == "landed").count(),
        2
    );
    // Killed while it started what follows a land: nothing is started again.
    journal(Stage::Dispatching).write(&lands(&r)).unwrap();
    let (said, result) = landed(&r, SLUG, &people.orch, &host);
    result.unwrap();
    assert!(
        said.contains("was started by a run before this one, and is not started twice"),
        "{said}"
    );
    // The command is told again, as it is safe to: a run cut before it was asked told nobody.
    assert_eq!(
        host.verbs().iter().filter(|verb| *verb == "landed").count(),
        3
    );
    assert!(!Journal::path(&lands(&r), SLUG).exists());
}

// -- evidence that changes while it is published ---------------------------------------------------

#[test]
fn evidence_that_changes_while_it_is_published_is_published_again_or_refused_never_merged_stale() {
    // Another green run at the head: the same standing on other evidence, published again.
    let (r, bare) = delivering();
    let (people, _, tip) = ready(&r, SLUG);
    let host =
        Host::new(&r, &bare).before("publish", || ran(&r, SLUG, &tip, "cargo test -p sim", 0));
    let (said, result) = landed(&r, SLUG, &people.orch, &host);
    result.unwrap();
    assert_eq!(
        host.verbs(),
        ["publish", "publish", "arm", "merge", "result", "landed"]
    );
    assert!(
        said.contains("its evidence changed while it was published; publishing it again"),
        "{said}"
    );
    let (first, second) = {
        let asked = host.asked.borrow();
        (
            asked[0].1["snapshot"]["revision"].clone(),
            asked[1].1["snapshot"]["revision"].clone(),
        )
    };
    assert_ne!(first, second);
    assert_eq!(
        Receipt::read(&lands(&r), SLUG).unwrap().revision,
        second.as_str().unwrap()
    );

    // A red run at the head while it was published: no merge is asked for.
    let (r, bare) = delivering();
    let before = head(&bare, "main");
    let (people, _, tip) = ready(&r, SLUG);
    let host =
        Host::new(&r, &bare).before("publish", || ran(&r, SLUG, &tip, "cargo test -p sim", 1));
    let (_, result) = landed(&r, SLUG, &people.orch, &host);
    let line = result.unwrap_err();
    assert!(
        line.contains("is not eligible for delivery by pull request"),
        "{line}"
    );
    assert!(line.contains("tests: red at"), "{line}");
    assert_eq!(host.verbs(), ["publish"]);
    assert_nothing_landed(&r, &bare, &before);
}

/// The limit the design states: a red arriving after the last validation, while the host works on
/// the request, does not recall it. The receipt names the revision that was validated, and the
/// evidence read after says what changed.
#[test]
fn a_withdrawal_after_the_last_validation_does_not_recall_a_request_already_made() {
    let (r, bare) = delivering();
    let (people, _, tip) = ready(&r, SLUG);
    let host = Host::new(&r, &bare).before("merge", || ran(&r, SLUG, &tip, "cargo test -p sim", 1));
    let (_, result) = landed(&r, SLUG, &people.orch, &host);
    result.unwrap();
    let receipt = Receipt::read(&lands(&r), SLUG).unwrap();
    let validated = host.request("publish")["snapshot"]["revision"].clone();
    assert_eq!(receipt.revision, validated.as_str().unwrap());
    let mut env = r.env(&r.root);
    (env.vars).insert(FOLDER_VAR.into(), work(&r).to_string_lossy().into_owned());
    let now = evidence::of(&r.board.store, &env, SLUG, None).unwrap();
    assert_ne!(
        now.revision, receipt.revision,
        "what stands now is another revision"
    );
    assert!(
        now.blocking
            .iter()
            .any(|line| line.starts_with("tests: red at")),
        "{:?}",
        now.blocking
    );
}

// -- the api ---------------------------------------------------------------------------------------

/// `tasks api`'s answer to `call` in `r`'s main checkout.
fn api(r: &Repo, call: Value) -> Value {
    let mut env = r.env(&r.root);
    (env.vars).insert(FOLDER_VAR.into(), work(r).to_string_lossy().into_owned());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let input = call.to_string();
    let args = ["api".to_string()];
    let code = symphony_board::cli::main(&args, &env, &mut input.as_bytes(), &mut out, &mut err);
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
    serde_json::from_slice(&out).unwrap()
}

#[test]
fn the_api_answers_the_evidence_the_land_publishes_and_writes_nothing() {
    let (r, bare) = delivering();
    let (people, _, tip) = ready(&r, SLUG);
    let thread = r.board.said(SLUG);
    let call = |args: Value| api(&r, json!({"method": "evidence", "args": args}));
    let answered = call(json!({"slug": SLUG, "head": tip}));
    let read: evidence::Snapshot = serde_json::from_value(answered["ok"].clone()).unwrap();
    assert_eq!(read.version, evidence::VERSION);
    assert_eq!(
        (read.slug.as_str(), read.head.as_str(), read.branch.as_str()),
        (SLUG, tip.as_str(), "main")
    );
    assert!(
        read.eligible && read.assessed && read.blocking.is_empty(),
        "{answered}"
    );
    let standing: Vec<(&str, &str)> = (answered["ok"]["stamps"].as_array().unwrap().iter())
        .map(|stamp| {
            (
                stamp["name"].as_str().unwrap(),
                stamp["standing"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        standing,
        [
            ("tests", "met"),
            ("review", "met"),
            ("pictures", "waived"),
            ("owner", "waived")
        ]
    );
    for stamp in answered["ok"]["stamps"].as_array().unwrap() {
        let met = stamp["standing"] == "met";
        assert_eq!(
            !stamp["evidence"].as_array().unwrap().is_empty(),
            met,
            "{stamp}"
        );
    }
    // Asked with no head it reads the branch's, and twice it says the same.
    assert_eq!(call(json!({"slug": SLUG})), answered);
    assert_eq!(r.board.said(SLUG), thread, "a reading posts nothing");
    // Another head than the branch's, and no slug, are refused by name.
    let other = "0".repeat(40);
    assert_eq!(
        call(json!({"slug": SLUG, "head": other}))["refused"],
        format!("{SLUG}'s branch is at {tip}, not {other}: evidence is of the head alone")
    );
    assert!(call(json!({}))["error"].as_str().unwrap().contains("slug"));
    // What the land publishes is that answer.
    let host = Host::new(&r, &bare);
    landed(&r, SLUG, &people.orch, &host).1.unwrap();
    assert_eq!(host.request("publish")["snapshot"], answered["ok"]);
}

// -- the command ----------------------------------------------------------------------------------

#[test]
fn the_delivery_command_is_run_with_its_verb_and_request_and_its_refusal_merges_nothing() {
    let r = Repo::new();
    let script = r.dir.join("deliver.sh");
    let seen = r.dir.join("seen");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\necho \"$1 $2\" >> '{seen}'\ncp \"$3\" '{seen}.request'\nprintf \
             '{{\"state\":\"unavailable\",\"why\":\"the host is away\"}}'\n",
            seen = seen.display()
        ),
    )
    .unwrap();
    project_file(
        &r,
        &format!(
            "[land]\ncheck = false\ndelivery = [\"sh\", \"{}\"]\n",
            script.display()
        ),
    );
    let bare = remote(&r);
    let before = head(&bare, "main");
    let (people, _, tip) = ready(&r, SLUG);
    let mut env = r.env(&r.root);
    (env.vars).insert(FOLDER_VAR.into(), work(&r).to_string_lossy().into_owned());
    let mut out = Vec::new();
    let result = land(&r.board.store, &env, SLUG, &people.orch, None, &mut out);
    assert_eq!(
        refusal(result.unwrap_err()),
        format!("{SLUG} was not published (unavailable): the host is away; nothing merged")
    );
    assert_eq!(fs::read_to_string(&seen).unwrap(), "publish --request\n");
    let request: Value =
        serde_json::from_str(&fs::read_to_string(r.dir.join("seen.request")).unwrap()).unwrap();
    assert_eq!(request["verb"], "publish");
    assert_eq!(request["head"], tip.as_str());
    assert_eq!(request["snapshot"]["eligible"], true);
    assert_nothing_landed(&r, &bare, &before);
    // A command that fails, or says what is no object, is no answer.
    fs::write(&script, "#!/bin/sh\necho 'cannot reach it' >&2\nexit 3\n").unwrap();
    let mut out = Vec::new();
    let result = land(&r.board.store, &env, SLUG, &people.orch, None, &mut out);
    let line = refusal(result.unwrap_err());
    assert!(line.starts_with("nothing merged: sh "), "{line}");
    assert!(line.ends_with(" publish: cannot reach it"), "{line}");
}

// -- a slug is delivered once ------------------------------------------------------------------------

const RETAINED_SLUG: &str = "refs/symphony/delivered/sim/jump";

/// `SLUG` delivered before, as a task this board no longer holds (a board made again, the slug
/// filed again): its squash on the default branch, its receipt and its head kept in the
/// repository, which outlive the board. A landed task itself moves no further. The receipt.
fn delivered_earlier(r: &Repo) -> Receipt {
    let parent = head(&r.root, "main");
    let earlier = r.add("earlier", None, "main");
    write(&earlier, "sim/earlier.gd", "extends Node\n");
    git(&earlier, &["add", "-A"]);
    git(&earlier, &["commit", "-qm", "work on the earlier sim/jump"]);
    let first = head(&earlier, "HEAD");
    git(&r.root, &["merge", "-q", "--squash", &first]);
    git(&r.root, &["commit", "-qm", "sim/jump: A task"]);
    git(&r.root, &["push", "-q", "origin", "main"]);
    let receipt = Receipt {
        slug: SLUG.into(),
        task: 9000,
        operation: "task".into(),
        repository: 7,
        name: "owner/name".into(),
        pull: 2,
        head: first.clone(),
        base_observed: parent.clone(),
        parent,
        squash: head(&r.root, "main"),
        tree: head(&r.root, "main^{tree}"),
        revision: "r0".into(),
        by: "an-earlier-lander".into(),
        import: None,
    };
    let path = Receipt::path(&lands(r), SLUG);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();
    git(&r.root, &["update-ref", RETAINED_SLUG, &first]);
    receipt
}

#[test]
fn a_slug_delivered_once_is_refused_a_second_delivery_before_anything_is_published() {
    let (r, bare) = delivering();
    let receipt = delivered_earlier(&r);
    let first = receipt.head.clone();
    let path = Receipt::path(&lands(&r), SLUG);
    let was = fs::read(&path).unwrap();

    let (people, _, second) = ready(&r, SLUG);
    assert_ne!(r.board.store.get(SLUG).unwrap().id, receipt.task);
    // A host that would open a pull request for it, as nothing in the verbs forbids.
    let host = Host::new(&r, &bare);
    let (_, result) = landed(&r, SLUG, &people.orch, &host);
    assert_eq!(
        result.unwrap_err(),
        format!(
            "{SLUG} was delivered already as {} from {} by pull request #2 of owner/name, and \
             its branch is now at {}: a slug is delivered once, its receipt and the head kept \
             under {RETAINED_SLUG} never replaced",
            &receipt.squash[..7],
            &first[..7],
            &second[..7]
        )
    );
    assert!(host.verbs().is_empty(), "{:?}", host.verbs());
    assert_eq!(fs::read(&path).unwrap(), was, "the receipt as it was");
    assert_eq!(head(&r.root, RETAINED_SLUG), first, "the head kept");
    assert_eq!(head(&bare, "main"), receipt.squash, "nothing merged");
    assert!(landings(&r, SLUG).is_empty());
    let said = r.board.said(SLUG).join("\n");
    assert!(
        said.contains("What it still needs goes in a new task"),
        "{said}"
    );
    // Said to the orchestrator each time it is tried, and still nothing asked.
    let (_, again) = landed(&r, SLUG, &people.orch, &host);
    assert!(again.unwrap_err().contains("a slug is delivered once"));
    assert!(host.verbs().is_empty());
}

#[test]
fn a_slug_filed_again_at_the_very_head_delivered_before_is_refused_as_delivered() {
    let (r, bare) = delivering();
    let receipt = delivered_earlier(&r);
    let first = receipt.head.clone();
    let path = Receipt::path(&lands(&r), SLUG);
    let was = fs::read(&path).unwrap();
    // Filed again with its branch at the head the receipt kept, assessed, green and approved
    // there: everything a delivery reads stands, and the host would merge it a second time.
    git(&r.root, &["branch", SLUG, &first]);
    let store = &r.board.store;
    let people = People {
        orch: r.board.session("orchestrator"),
        exe: r.board.session("worker"),
        assessor: r.board.session("reviewer"),
    };
    r.task(SLUG);
    store.take_role(SLUG, &people.orch, "orchestrator").unwrap();
    store.claim(SLUG, &people.exe).unwrap();
    store.move_to(SLUG, "review", &people.exe).unwrap();
    assess(&r, SLUG, &people.assessor, &["tests", "review"], &first);
    ran(&r, SLUG, &first, "cargo test -p sim", 0);
    approve(&r, SLUG, &format!("{first}: reads well"));
    let refused = format!(
        "{SLUG} was delivered already as {} from {} by pull request #2 of owner/name, and its \
         branch is at that head again, as a task that never landed it: a slug is delivered once, \
         its receipt and the head kept under {RETAINED_SLUG} never replaced",
        &receipt.squash[..7],
        &first[..7]
    );
    let host = Host::new(&r, &bare);
    assert_eq!(
        landed(&r, SLUG, &people.orch, &host).1.unwrap_err(),
        refused
    );
    assert!(host.verbs().is_empty(), "{:?}", host.verbs());
    assert_eq!(fs::read(&path).unwrap(), was, "the receipt as it was");
    assert_eq!(head(&r.root, RETAINED_SLUG), first);
    assert_eq!(head(&bare, "main"), receipt.squash, "nothing merged");
    assert!(landings(&r, SLUG).is_empty());
    // Another delivery's journal is no word of this one: of another pull request, or of
    // another head, it is refused the same.
    let journal = |head: &str, pull| Journal {
        slug: SLUG.into(),
        operation: "task".into(),
        stage: Stage::Eligible,
        head: head.into(),
        base_observed: receipt.parent.clone(),
        revision: "r0".into(),
        title: String::new(),
        body: String::new(),
        by: people.orch.clone(),
        closes: vec!["done".into()],
        stood: String::new(),
        binding: Some(symphony_board::land::delivery::Binding {
            repository: 7,
            name: "owner/name".into(),
            pull,
            url: String::new(),
        }),
        squash: None,
        held: None,
        import: None,
        green: None,
        armed: false,
    };
    for other in [journal(&first, 3), journal(&receipt.parent, 2)] {
        other.write(&lands(&r)).unwrap();
        let host = Host::new(&r, &bare);
        assert_eq!(
            landed(&r, SLUG, &people.orch, &host).1.unwrap_err(),
            refused
        );
        assert!(host.verbs().is_empty(), "{:?}", host.verbs());
    }
    // This delivery's own journal, of that head and bound to the receipt's pull request (a run
    // cut between the receipt and the journal's next stage), is not refused: the land goes on
    // to read its pull request.
    journal(&first, 2).write(&lands(&r)).unwrap();
    let host = Host::new(&r, &bare);
    let (said, _) = landed(&r, SLUG, &people.orch, &host);
    assert!(!said.contains("a slug is delivered once"), "{said}");
    assert_eq!(host.verbs().first().map(String::as_str), Some("result"));
    assert_eq!(fs::read(&path).unwrap(), was, "the receipt as it was");
    assert_eq!(head(&r.root, RETAINED_SLUG), first);
}

#[test]
fn a_file_that_does_not_read_as_the_slugs_receipt_refuses_its_delivery_by_name() {
    let (r, bare) = delivering();
    let before = head(&bare, "main");
    let (people, ..) = ready(&r, SLUG);
    let path = Receipt::path(&lands(&r), SLUG);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let other = serde_json::json!({
        "slug": "sim/other", "task": 1, "operation": "task", "repository": 7, "name": "o/n",
        "pull": 3, "head": "a", "base_observed": "b", "parent": "b", "squash": "c", "tree": "d",
        "revision": "r", "by": "x",
    });
    for (text, why) in [
        ("{\"slug\": ".to_string(), "EOF while parsing"),
        (other.to_string(), "it names sim/other"),
    ] {
        fs::write(&path, &text).unwrap();
        let host = Host::new(&r, &bare);
        let (_, result) = landed(&r, SLUG, &people.orch, &host);
        let line = result.unwrap_err();
        assert!(
            line.starts_with(&format!(
                "the receipt kept for {SLUG}, {}, does not read as its own (",
                path.display()
            )) && line.contains(why)
                && line.ends_with(
                    "what a delivery proved is never written over, and a person judges the file"
                ),
            "{line}"
        );
        assert!(host.verbs().is_empty(), "{:?}", host.verbs());
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
    }
    // Nor one that cannot be read at all, which is not the same as none kept.
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    let host = Host::new(&r, &bare);
    let line = landed(&r, SLUG, &people.orch, &host).1.unwrap_err();
    assert!(line.contains("does not read as its own ("), "{line}");
    assert!(host.verbs().is_empty(), "{:?}", host.verbs());
    assert_eq!(head(&bare, "main"), before);
    assert!(landings(&r, SLUG).is_empty());
    // With the file judged and gone, it is delivered as any task.
    fs::remove_dir(&path).unwrap();
    landed(&r, SLUG, &people.orch, &Host::new(&r, &bare))
        .1
        .unwrap();
    assert_eq!(landings(&r, SLUG).len(), 1);
}

#[test]
fn a_merge_found_beside_another_deliverys_receipt_or_retained_head_is_held_and_replaces_neither() {
    // Each appears after the land's own look and before the host's answer is proved: a receipt
    // of another delivery of the slug, a file that reads as none, a retained head elsewhere.
    for case in ["receipt", "unread", "retained"] {
        let (r, bare) = delivering();
        let local = head(&r.root, "main");
        let (people, _, tip) = ready(&r, SLUG);
        let path = Receipt::path(&lands(&r), SLUG);
        let other = Receipt {
            slug: SLUG.into(),
            task: 99,
            operation: "task".into(),
            repository: 7,
            name: "owner/name".into(),
            pull: 2,
            head: local.clone(),
            base_observed: local.clone(),
            parent: local.clone(),
            squash: local.clone(),
            tree: head(&r.root, "main^{tree}"),
            revision: "r0".into(),
            by: "someone".into(),
            import: None,
        };
        let text = match case {
            "receipt" => serde_json::to_string_pretty(&other).unwrap(),
            _ => "not a receipt".to_string(),
        };
        let (root, at, write_text) = (r.root.clone(), path.clone(), text.clone());
        let elsewhere = local.clone();
        let host = Host::new(&r, &bare).before("merge", move || {
            if case == "retained" {
                git(&root, &["update-ref", RETAINED_SLUG, &elsewhere]);
            } else {
                fs::create_dir_all(at.parent().unwrap()).unwrap();
                fs::write(&at, write_text).unwrap();
            }
        });
        let (_, result) = landed(&r, SLUG, &people.orch, &host);
        let line = result.unwrap_err();
        let squash = head(&bare, "main");
        let said = match case {
            "receipt" => format!(
                "{SLUG} was delivered already as {} from {}, and origin/main holds {} of {} as \
                 well: its receipt and the head kept under {RETAINED_SLUG} are not replaced",
                &local[..7],
                &local[..7],
                &squash[..7],
                &tip[..7]
            ),
            "unread" => "does not read as its own".to_string(),
            _ => format!(
                "{RETAINED_SLUG} keeps {}, not the head {} that {} is of: it is not moved",
                &local[..7],
                &tip[..7],
                &squash[..7]
            ),
        };
        assert!(
            line.starts_with(&format!(
                "the delivery of {SLUG} is held for reconciliation: "
            )) && line.contains(&said),
            "{case}: {line}"
        );
        assert_ne!(squash, local, "the host merged it");
        assert_eq!(head(&r.root, "main"), local, "the checkout follows nothing");
        assert!(landings(&r, SLUG).is_empty());
        if case == "retained" {
            assert_eq!(head(&r.root, RETAINED_SLUG), local, "not moved");
            assert!(!path.exists(), "no receipt beside a head it does not name");
        } else {
            assert_eq!(fs::read_to_string(&path).unwrap(), text, "as it was");
            assert!(
                !git(&r.root, &["for-each-ref", RETAINED_SLUG]).contains(&tip),
                "no head retained beside another delivery's receipt"
            );
        }
        // A run again finds it held as it was, asks for no second merge and says it once.
        let (_, again) = landed(&r, SLUG, &people.orch, &host);
        assert_eq!(again.unwrap_err(), line);
        assert_eq!(
            host.verbs().iter().filter(|verb| *verb == "merge").count(),
            1
        );
        let posts = (r.board.said(SLUG).iter())
            .filter(|line| line.contains("held for reconciliation"))
            .count();
        assert_eq!(posts, 1);
    }
}

#[test]
fn a_delivery_proved_again_after_a_run_cut_short_keeps_the_receipt_it_has_and_lands_once() {
    let (r, bare) = delivering();
    let (people, _, tip) = ready(&r, SLUG);
    let host = Host::new(&r, &bare);
    // Landed, then a journal as a run cut short after the receipt was kept and before the
    // journal said the merge was proved leaves it: the next run proves the merge again.
    landed(&r, SLUG, &people.orch, &host).1.unwrap();
    let kept = fs::read(Receipt::path(&lands(&r), SLUG)).unwrap();
    let receipt = Receipt::read(&lands(&r), SLUG).unwrap();
    let journal = Journal {
        slug: SLUG.into(),
        operation: "task".into(),
        stage: Stage::MergeRequested,
        head: tip.clone(),
        base_observed: receipt.base_observed.clone(),
        revision: "another".into(),
        title: String::new(),
        body: String::new(),
        by: "another-lander".into(),
        closes: vec!["done".into()],
        stood: String::new(),
        binding: Some(symphony_board::land::delivery::Binding {
            repository: 7,
            name: "owner/name".into(),
            pull: 3,
            url: String::new(),
        }),
        squash: None,
        held: None,
        import: None,
        green: None,
        armed: false,
    };
    journal.write(&lands(&r)).unwrap();
    let (said, result) = landed(&r, SLUG, &people.orch, &host);
    result.unwrap();
    assert!(said.contains("a delivery is under way"), "{said}");
    assert_eq!(
        fs::read(Receipt::path(&lands(&r), SLUG)).unwrap(),
        kept,
        "the receipt as the first proof wrote it, never by the second run's"
    );
    assert_eq!(head(&r.root, RETAINED_SLUG), tip);
    assert_eq!(landings(&r, SLUG).len(), 1, "closed once");
    assert_eq!(
        host.verbs().iter().filter(|verb| *verb == "merge").count(),
        1
    );
    assert!(!Journal::path(&lands(&r), SLUG).exists());
}
