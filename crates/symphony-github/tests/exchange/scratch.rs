//! The exchange on a real board: a scratch repository's own, reached as the program reaches it,
//! by the board's JSON calls (`tasks api`, run here in this process), with its settings read
//! from what that repository's `main` commits and its requests made by a real `curl` to the
//! fake repository on a loopback port.

use crate::github::Fake;
use crate::http::Server;
use crate::{Dir, Ran, TOKEN, run};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use symphony_board::cli::Env;
use symphony_github::board::{Api, Board as _, Caller};
use symphony_github::config::{Committed, Source as _, checkouts};
use symphony_github::credential::Secret;
use symphony_github::plan::import_slug;
use symphony_github::state::Flag;
use symphony_github::transport::{Curl, Transport};

const NOW: &str = "1800000000";

/// The pinned `surreal`: `SYMPHONY_SURREAL`, else the one `tools/surreal.sh` fetches.
fn surreal() -> PathBuf {
    if let Some(named) = symphony_env::SURREAL.os() {
        return named.into();
    }
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/surreal.sh");
    let found = Command::new("bash").arg(script).output().unwrap();
    assert!(
        found.status.success(),
        "tools/surreal.sh found no SurrealDB server: {}",
        String::from_utf8_lossy(&found.stderr)
    );
    PathBuf::from(String::from_utf8(found.stdout).unwrap().trim())
}

fn git(dir: &Path, args: &[&str]) {
    let done = Command::new("git")
        .args(["-c", "user.email=t@t", "-c", "user.name=t", "-C"])
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(done.status.success(), "git {args:?}: {done:?}");
}

/// The board's calls made in this process, as `tasks api` answers them.
struct InProcess(Env);

impl InProcess {
    fn tasks(&self, args: &[&str], input: &str) -> (i32, String, String) {
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code =
            symphony_board::cli::main(&args, &self.0, &mut input.as_bytes(), &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }
}

impl Caller for InProcess {
    fn call(&self, method: &str, args: Value) -> Result<Value, String> {
        let (code, out, err) = self.tasks(
            &["api"],
            &json!({"method": method, "args": args}).to_string(),
        );
        if code != 0 {
            return Err(err);
        }
        serde_json::from_str(&out).map_err(|error| format!("{error}: {out:?}"))
    }
}

/// A repository of its own with a board, a fake GitHub on a loopback port, and a token file.
struct Scratch {
    dir: Dir,
    root: PathBuf,
    board: Api<InProcess>,
    github: Arc<Fake>,
    server: Server,
    handle: String,
}

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = Dir::new(name);
        let root = dir.0.join("project");
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        std::fs::write(root.join("symphony.toml"), "[project]\nid = \"alpha\"\n").unwrap();
        git(&root, &["add", "symphony.toml"]);
        git(&root, &["commit", "-q", "-m", "base"]);
        let machine = dir.0.join("machine");
        std::fs::create_dir_all(&machine).unwrap();
        let vars: BTreeMap<String, String> = [
            ("SYMPHONY_SURREAL", surreal().display().to_string()),
            ("SYMPHONY_MACHINE_DIR", machine.display().to_string()),
            ("TASKS_NOW", NOW.to_string()),
            ("TASKS_SWEEP", "off".to_string()),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_string(), value))
        .collect();
        let board = Api(InProcess(Env {
            vars,
            cwd: root.clone(),
        }));
        let registered = board.0.call("register", json!({"kind": "worker"})).unwrap();
        let handle = registered["ok"]["handle"]
            .as_str()
            .expect("a session")
            .to_string();
        let github = Arc::new(Fake::new(77, "o", "n"));
        let server = Server::start(Arc::clone(&github));
        Scratch {
            dir,
            root,
            board,
            github,
            server,
            handle,
        }
    }

    /// `main` commits these settings.
    fn commit_settings(&self, more: &str) {
        let text = crate::settings(&self.dir.token_file(), more);
        std::fs::write(self.root.join("symphony-github.toml"), text).unwrap();
        git(&self.root, &["add", "symphony-github.toml"]);
        git(&self.root, &["commit", "-q", "-m", "settings"]);
    }

    fn run(&self, args: &[&str]) -> Ran {
        let base = self.server.base.clone();
        let connect = move |token: Secret| -> Result<Box<dyn Transport>, String> {
            Ok(Box::new(Curl::new(&base, token)?))
        };
        let folders = checkouts(&self.root).unwrap();
        let source = Committed {
            main: folders[0].clone(),
        };
        run(args, &self.board, &source, &connect, 1_000, folders)
    }

    fn sync(&self) -> String {
        let preview = self.run(&["preview"]).ok();
        let Some(digest) = preview.digest() else {
            return preview.out;
        };
        let applied = self
            .run(&["reconcile", "--preview", &digest, "--as", &self.handle])
            .ok();
        format!("{}{}", preview.out, applied.out)
    }

    fn api(&self, method: &str, args: Value) -> Value {
        self.board.0.call(method, args).unwrap()
    }

    fn state_folder(&self) -> PathBuf {
        self.root.join(".git/symphony/integrations/github")
    }

    /// Everything the adapter keeps, as text.
    fn kept(&self) -> String {
        let mut all = String::new();
        for entry in std::fs::read_dir(self.state_folder()).unwrap() {
            all.push_str(&std::fs::read_to_string(entry.unwrap().path()).unwrap_or_default());
        }
        all
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let home =
            symphony_board::server::Home::in_common(&self.root.join(".git"), Some(&surreal()));
        let _ = home.stop_board();
    }
}

#[test]
fn only_what_main_commits_turns_it_on() {
    let _starts = crate::starting();
    let dir = Dir::new("committed");
    let root = dir.0.join("project");
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["commit", "-q", "--allow-empty", "-m", "base"]);
    let source = Committed { main: root.clone() };
    let enabled = crate::settings(&dir.token_file(), "issues = \"export\"");

    // No file: turned off, read where the default branch would commit it.
    let absent = source.load().unwrap();
    assert!(!absent.config.enabled);
    assert_eq!(
        (absent.text, absent.place.as_str()),
        (None, "symphony-github.toml on main")
    );

    // A file in the working tree, and one a branch commits, enable nothing.
    std::fs::write(root.join("symphony-github.toml"), &enabled).unwrap();
    assert!(
        !source.load().unwrap().config.enabled,
        "an uncommitted file"
    );
    git(&root, &["checkout", "-q", "-b", "feature"]);
    git(&root, &["add", "symphony-github.toml"]);
    git(&root, &["commit", "-q", "-m", "a branch's settings"]);
    assert!(!source.load().unwrap().config.enabled, "a branch's file");

    // A linked worktree's file enables nothing either, and its main checkout is found from it.
    git(&root, &["checkout", "-q", "main"]);
    let linked = dir.0.join("linked");
    git(
        &root,
        &["worktree", "add", "-q", linked.to_str().unwrap(), "feature"],
    );
    let folders = checkouts(&linked).unwrap();
    assert_eq!(folders[0], root, "the main checkout first");
    assert!(
        folders.contains(&linked) && folders.contains(&root.join(".git")),
        "{folders:?}"
    );
    assert!(
        !Committed {
            main: folders[0].clone()
        }
        .load()
        .unwrap()
        .config
        .enabled
    );

    // Landed on main: enabled, at a revision that follows the file's text.
    git(&root, &["merge", "-q", "feature"]);
    let first = source.load().unwrap();
    assert!(first.config.enabled);
    assert_eq!(first.text.as_deref(), Some(enabled.as_str()));
    assert_eq!(first.place, "symphony-github.toml on main");
    std::fs::write(
        root.join("symphony-github.toml"),
        format!("{enabled}# again\n"),
    )
    .unwrap();
    git(&root, &["commit", "-q", "-am", "edited"]);
    assert_ne!(source.load().unwrap().revision, first.revision);

    // Committed and unreadable: a refusal, which every caller reads as turned off.
    std::fs::write(root.join("symphony-github.toml"), "enabled = tru").unwrap();
    git(&root, &["commit", "-q", "-am", "broken"]);
    assert!(source.load().unwrap_err().contains("symphony-github.toml"));

    // Another default branch is the one read once the main checkout commits it, as the land
    // reads it: an edit of the working file names no branch.
    std::fs::write(
        root.join("symphony.toml"),
        "[project]\ndefault_branch = \"trunk\"\n",
    )
    .unwrap();
    let still = source.load().unwrap_err();
    assert!(still.contains("symphony-github.toml"), "main's: {still}");
    git(&root, &["add", "symphony.toml"]);
    git(&root, &["commit", "-q", "-m", "trunk"]);
    assert!(source.load().unwrap_err().contains("no branch trunk"));
    git(&root, &["branch", "trunk", "feature"]);
    assert!(source.load().unwrap().config.enabled);

    // A project file HEAD commits that does not read names no branch: the refusal names it, by
    // the TOML message alone.
    std::fs::write(root.join("symphony.toml"), "[project\n").unwrap();
    git(&root, &["commit", "-q", "-am", "broken project"]);
    let unread = source.load().unwrap_err();
    let message = unread.strip_prefix("symphony.toml: ").unwrap_or_default();
    assert!(
        !message.is_empty() && !message.contains("symphony"),
        "{unread}"
    );
}

#[test]
fn a_token_file_inside_the_repository_is_refused_before_any_request() {
    let _starts = crate::starting();
    let scratch = Scratch::new("token-inside");
    let inside = scratch.root.join("token");
    std::fs::write(&inside, TOKEN).unwrap();
    std::fs::write(
        scratch.root.join("symphony-github.toml"),
        format!(
            "enabled = true\nrepository = \"o/n\"\ntoken_file = \"{}\"\n",
            inside.display()
        ),
    )
    .unwrap();
    git(&scratch.root, &["add", "symphony-github.toml"]);
    git(&scratch.root, &["commit", "-q", "-m", "settings"]);
    let refused = scratch.run(&["preview"]);
    assert_eq!(refused.code, 1);
    assert!(
        refused.err.contains("inside the repository"),
        "{}",
        refused.err
    );
    assert!(!refused.err.contains(TOKEN));
    assert!(
        scratch.server.heads.lock().unwrap().is_empty(),
        "no request was made"
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one board's life in order, each step on what the one before left"
)]
fn issues_are_exchanged_with_a_real_board_through_its_api_and_curl() {
    let _starts = crate::starting();
    let scratch = Scratch::new("board");
    let mut said = String::new();

    // With no settings committed the board is as it was and nothing is kept or asked.
    let off = scratch.run(&["status"]).ok();
    assert!(off.out.contains("turned off"), "{}", off.out);
    assert!(!scratch.state_folder().exists());
    assert!(scratch.server.heads.lock().unwrap().is_empty());

    scratch.commit_settings(
        "issues = \"exchange\"\nimport_labels = [\"symphony\"]\nimport_comments = true",
    );
    // The board has no recovery binding yet: an import cannot be mapped, so the run ends.
    let number = scratch
        .github
        .open("Crash on start", "It crashes.", &["symphony"]);
    let slug = import_slug(77, number);
    let preview = scratch.run(&["preview"]).ok();
    let unbound = scratch.run(&[
        "reconcile",
        "--preview",
        &preview.digest().unwrap(),
        "--as",
        &scratch.handle,
    ]);
    assert_eq!(unbound.code, 1);
    assert!(unbound.err.contains("recovery"), "{}", unbound.err);
    said.push_str(&unbound.err);
    assert_eq!(scratch.board.0.tasks(&["recovery", "rebind"], "").0, 0);

    // Bound: the proposal the cut run made is taken up, not made twice.
    said.push_str(&scratch.sync());
    let tasks = scratch.board.tasks().unwrap();
    assert_eq!(tasks.len(), 1, "{tasks:?}");
    let task = scratch.api("get", json!({"slug": slug}));
    assert_eq!(task["ok"]["status"], "backlog", "{task}");
    assert_eq!(task["ok"]["title"], "Crash on start");
    assert_eq!(
        task["ok"]["roles"]["executor"],
        Value::Null,
        "nobody was started on it"
    );
    assert!(
        scratch.state_folder().join("state.json").is_file(),
        "kept in the git common dir"
    );

    // An edit there reaches the task by its token; a comment becomes a note once.
    scratch
        .github
        .edit(number, "Crash on start-up", "It crashes at once.");
    scratch.github.comment(number, "octocat", "Same here.");
    said.push_str(&scratch.sync());
    said.push_str(&scratch.sync());
    let task = scratch.api("get", json!({"slug": slug}));
    assert_eq!(task["ok"]["title"], "Crash on start-up");
    let thread = scratch.api("thread", json!({"slug": slug}));
    let notes = |thread: &Value, has: &str| {
        thread["ok"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|message| message["text"].as_str().unwrap_or_default().contains(has))
            .count()
    };
    assert_eq!(
        notes(&thread, "The GitHub user octocat commented"),
        1,
        "{thread}"
    );

    // The task is rewritten here between a preview and its reconcile: the board refuses the
    // token in the write's own transaction, and the run ends with the brief as it was written.
    scratch
        .github
        .edit(number, "Crash on start-up", "Remote again.");
    let preview = scratch.run(&["preview"]).ok();
    let bound = scratch.api("bound", json!({"slug": slug}));
    let edited = scratch.api(
        "edit_bound",
        json!({"slug": slug, "bound": bound["ok"]["bound"], "by": scratch.handle, "body": "Refined here."}),
    );
    assert!(edited["ok"].is_object(), "{edited}");
    let refused = scratch.run(&[
        "reconcile",
        "--preview",
        &preview.digest().unwrap(),
        "--as",
        &scratch.handle,
    ]);
    assert_eq!(refused.code, 1);
    assert!(
        refused.err.contains("the board refused the edit"),
        "{}",
        refused.err
    );
    said.push_str(&refused.err);
    assert_eq!(
        scratch.api("get", json!({"slug": slug}))["ok"]["body"],
        "Refined here."
    );

    // Read again it is a conflict, said once on the thread, and neither side is overwritten.
    said.push_str(&scratch.sync());
    said.push_str(&scratch.sync());
    assert_eq!(
        scratch.api("get", json!({"slug": slug}))["ok"]["body"],
        "Refined here."
    );
    assert_eq!(scratch.github.issue(number)["body"], "Remote again.");
    assert_eq!(
        notes(
            &scratch.api("thread", json!({"slug": slug})),
            "Neither was overwritten"
        ),
        1
    );

    // A task of the board is published from its curated text, over a lost answer.
    let filed = scratch.api(
        "create",
        json!({"slug": "tools/a", "title": "Internal", "by": scratch.handle, "body": "Private brief."}),
    );
    assert!(filed["ok"].is_object(), "{filed}");
    let public = scratch.dir.0.join("public.md");
    std::fs::write(&public, "Public \"quoted\" body \\ with a backslash.").unwrap();
    scratch
        .run(&[
            "publish",
            "tools/a",
            "--title",
            "Public",
            "--body-file",
            public.to_str().unwrap(),
        ])
        .ok();
    let preview = scratch.run(&["preview"]).ok();
    scratch.github.fault(
        "POST",
        "/repos/o/n/issues",
        crate::github::Fault::LoseAnswer,
    );
    let digest = preview.digest().unwrap();
    let cut = scratch.run(&["reconcile", "--preview", &digest, "--as", &scratch.handle]);
    assert_eq!(cut.code, 1, "{}", cut.out);
    assert!(cut.err.contains("result unknown"), "{}", cut.err);
    said.push_str(&cut.err);
    said.push_str(
        &scratch
            .run(&["reconcile", "--preview", &digest, "--as", &scratch.handle])
            .ok()
            .out,
    );
    let issues = scratch.github.issues();
    assert_eq!(issues.len(), 2, "published once");
    assert!(
        issues[1]["body"]
            .as_str()
            .unwrap()
            .starts_with("Public \"quoted\" body \\ with a backslash."),
        "the body arrived as written: {}",
        issues[1]["body"]
    );
    assert_eq!(
        notes(
            &scratch.api("thread", json!({"slug": "tools/a"})),
            "Published as GitHub issue #2"
        ),
        1
    );

    // The board is rebound, as after a restore: every mapping kept before it is suspended.
    assert_eq!(scratch.board.0.tasks(&["recovery", "rebind"], "").0, 0);
    scratch.github.edit(2, "Public", "Edited there.");
    said.push_str(&scratch.sync());
    let state: symphony_github::state::State = serde_json::from_str(
        &std::fs::read_to_string(scratch.state_folder().join("state.json")).unwrap(),
    )
    .unwrap();
    assert!(
        matches!(state.mappings["tools/a"].flag, Some(Flag::Suspended(_))),
        "{state:?}"
    );

    // The token went by the Authorization header alone, and nothing said or kept holds it.
    let heads = scratch.server.heads.lock().unwrap().clone();
    assert!(heads.len() > 10);
    for head in &heads {
        assert!(
            head.contains(&format!("Authorization: Bearer {TOKEN}")),
            "{head}"
        );
        assert!(!head.lines().next().unwrap().contains(TOKEN), "{head}");
        assert!(head.contains("User-Agent: symphony-github"), "{head}");
    }
    said.push_str(&scratch.run(&["status"]).ok().out);
    assert!(!said.contains(TOKEN), "said: {said}");
    assert!(!scratch.kept().contains(TOKEN), "kept");
    assert!(
        scratch.kept().contains("\"event\":\"ack\""),
        "a journal is kept"
    );
}
