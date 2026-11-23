//! The exchange run whole, as an operator runs it: `symphony-github COMMAND` against a GitHub
//! repository held in memory (`github.rs`). `flows.rs` and `recovery.rs` run it on a board held
//! in memory (`memory.rs`), where each failure is planted exactly; `scratch.rs` runs it on a
//! real scratch board through `tasks api`, its requests made by a real `curl` to a loopback
//! server (`http.rs`). `deliver.rs` asks it the verbs of a land delivered by a pull request,
//! `armed.rs` those of one GitHub merges by its auto-merge, `protection.rs` what those verbs
//! refuse of a repository not protected for one, and
//! `rulesets.rs` the rulesets a person verifies for them. `release.rs` asks it for a GitHub
//! Release of a tag, as the release run does, and `verify.rs` for its commit's verification on
//! GitHub's machines.

#![expect(
    clippy::unwrap_used,
    reason = "the fixtures beside the tests assert as they do: an unwrap is the failure"
)]

mod armed;
mod deliver;
mod flows;
mod github;
mod http;
mod import;
mod memory;
mod minting;
mod protection;
mod recovery;
mod release;
mod rulesets;
mod scratch;
mod verify;

use github::Fake;
use memory::MemBoard;
use std::cell::Cell;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use symphony_github::board::Board;
use symphony_github::config::{self, Loaded, Source};
use symphony_github::credential::Secret;
use symphony_github::reconcile::Context;
use symphony_github::state::{Entry, State, Store};
use symphony_github::transport::{Failure, Request, Response, Transport};

/// The token every fixture's file holds: nothing a test reads back may contain it.
pub(crate) const TOKEN: &str = "ghp_fixtureTOKEN0123456789abcdefSECRET";

/// A folder of its own under the temp folder, removed when dropped.
pub(crate) struct Dir(pub(crate) PathBuf);

impl Dir {
    pub(crate) fn new(name: &str) -> Dir {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "symphony-github-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Dir(path.canonicalize().unwrap())
    }

    /// A token file in it, readable by its owner alone.
    pub(crate) fn token_file(&self) -> PathBuf {
        let folder = self.0.join("secret");
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("token");
        std::fs::write(&file, format!("{TOKEN}\n")).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        file
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The settings a test commits, as text; none is no file.
pub(crate) fn settings(token_file: &Path, more: &str) -> String {
    format!(
        "enabled = true\nrepository = \"o/n\"\ntoken_file = \"{}\"\n{more}\n",
        token_file.display()
    )
}

/// Settings held in memory, read again on every load, which a test counts and changes.
#[derive(Default)]
pub(crate) struct Settings {
    text: Mutex<Option<String>>,
    loads: AtomicUsize,
    /// The text the settings take once this many more loads were answered.
    then: Mutex<Option<(usize, Option<String>)>>,
}

impl Settings {
    pub(crate) fn set(&self, text: Option<String>) {
        *self.text.lock().unwrap() = text;
    }

    pub(crate) fn after(&self, loads: usize, text: Option<String>) {
        *self.then.lock().unwrap() = Some((self.loads.load(Ordering::SeqCst) + loads, text));
    }
}

impl Source for Settings {
    fn load(&self) -> Result<Loaded, String> {
        let loads = self.loads.fetch_add(1, Ordering::SeqCst);
        let mut then = self.then.lock().unwrap();
        if then.as_ref().is_some_and(|(at, _)| loads >= *at) {
            let (_, text) = then.take().unwrap();
            self.set(text);
        }
        match self.text.lock().unwrap().as_deref() {
            Some(text) => config::load(text),
            None => Ok(Loaded::absent()),
        }
    }
}

/// The fake repository as the transport a run is given.
pub(crate) struct Shared(pub(crate) Arc<Fake>);

impl Transport for Shared {
    fn send(&self, request: &Request) -> Result<Response, Failure> {
        self.0.send(request)
    }

    fn fetch(&self, url: &str, to: &Path) -> Result<u16, Failure> {
        self.0.fetch(url, to)
    }
}

/// What one command said.
#[derive(Debug)]
pub(crate) struct Ran {
    pub(crate) code: i32,
    pub(crate) out: String,
    pub(crate) err: String,
}

impl Ran {
    /// The digest `preview` printed, when it kept one.
    pub(crate) fn digest(&self) -> Option<String> {
        let after = self.out.split("--preview ").nth(1)?;
        after.split_whitespace().next().map(str::to_string)
    }

    /// The same, which must have succeeded.
    pub(crate) fn ok(self) -> Ran {
        assert_eq!(self.code, 0, "{}\n{}", self.out, self.err);
        self
    }
}

/// `symphony-github ARGS` with this board, these settings and this transport.
pub(crate) fn run(
    args: &[&str],
    board: &dyn Board,
    source: &dyn Source,
    connect: &dyn Fn(Secret) -> Result<Box<dyn Transport>, String>,
    now: i64,
    repository: Vec<PathBuf>,
) -> Ran {
    run_gh(args, board, source, connect, now, repository, "gh")
}

/// The same, with `gh` the gh CLI it runs.
pub(crate) fn run_gh(
    args: &[&str],
    board: &dyn Board,
    source: &dyn Source,
    connect: &dyn Fn(Secret) -> Result<Box<dyn Transport>, String>,
    now: i64,
    repository: Vec<PathBuf>,
    gh: &str,
) -> Ran {
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
    let ctx = Context {
        source,
        board,
        connect,
        now,
        repository,
        openssl: "openssl",
        gh,
        rest: &|_| {},
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = symphony_github::cli::run(&args, &ctx, &mut out, &mut err);
    Ran {
        code,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

/// `slug` published from a curated draft: `title`, and `body` from a file.
pub(crate) fn draft(rig: &Rig, slug: &str, title: &str, body: &str) {
    let file = rig.dir.0.join("public.md");
    std::fs::write(&file, body).unwrap();
    rig.run(&[
        "publish",
        slug,
        "--title",
        title,
        "--body-file",
        file.to_str().unwrap(),
    ])
    .ok();
}

/// The exchange on the board in memory.
pub(crate) struct Rig {
    pub(crate) dir: Dir,
    pub(crate) github: Arc<Fake>,
    pub(crate) board: MemBoard,
    pub(crate) settings: Settings,
    pub(crate) now: Cell<i64>,
    pub(crate) token_file: PathBuf,
}

impl Rig {
    /// Turned on, with `more` lines of settings.
    pub(crate) fn new(name: &str, more: &str) -> Rig {
        let rig = Rig::off(name);
        rig.enable(more);
        rig
    }

    /// With no settings file committed.
    pub(crate) fn off(name: &str) -> Rig {
        let dir = Dir::new(name);
        let token_file = dir.token_file();
        Rig {
            board: MemBoard::new(dir.0.join("state")),
            github: Arc::new(Fake::new(77, "o", "n")),
            settings: Settings::default(),
            now: Cell::new(1_000),
            token_file,
            dir,
        }
    }

    pub(crate) fn enable(&self, more: &str) {
        self.settings.set(Some(settings(&self.token_file, more)));
    }

    pub(crate) fn run(&self, args: &[&str]) -> Ran {
        let github = Arc::clone(&self.github);
        let connect = move |_token: Secret| -> Result<Box<dyn Transport>, String> {
            Ok(Box::new(Shared(Arc::clone(&github))))
        };
        run(
            args,
            &self.board,
            &self.settings,
            &connect,
            self.now.get(),
            vec![self.dir.0.join("repository")],
        )
    }

    pub(crate) fn preview(&self) -> Ran {
        self.run(&["preview"]).ok()
    }

    pub(crate) fn reconcile(&self, digest: &str) -> Ran {
        self.run(&["reconcile", "--preview", digest, "--as", "amber-fox-1"])
    }

    /// A preview and its reconcile, which must both succeed: what the two said.
    pub(crate) fn sync(&self) -> String {
        let preview = self.preview();
        let Some(digest) = preview.digest() else {
            return preview.out;
        };
        let applied = self.reconcile(&digest).ok();
        format!("{}{}", preview.out, applied.out)
    }

    pub(crate) fn store(&self) -> Store {
        Store::existing(&self.dir.0.join("state")).expect("a state folder")
    }

    pub(crate) fn state(&self) -> State {
        self.store().state().unwrap().unwrap_or_default()
    }

    pub(crate) fn journal(&self) -> Vec<Entry> {
        self.store().entries().unwrap()
    }
}
