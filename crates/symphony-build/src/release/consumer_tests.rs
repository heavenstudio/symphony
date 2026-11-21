//! A release's notes for the project that takes it: the trailers read, what git derives between two
//! releases of a scratch repository, the notes composed across releases, and the check of a branch.

use super::*;
use crate::test_tree::Tree;
use std::path::PathBuf;

const GUARD: &str = "crates/tasks/tests/consumer_api.rs";

/// A scratch repository holding the files the notes are derived from.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    /// On `main`, one commit: the board at schema 20, pin 1, a guard of two calls, a shipped
    /// recipe file and the three tools pinned.
    fn new(tree: &Tree) -> Scratch {
        let root = tree.path("repo");
        std::fs::create_dir_all(&root).unwrap();
        let scratch = Scratch { root };
        scratch.git(&["init", "-q", "-b", "main"]);
        scratch.write(SCHEMA_FILE, "pub const VERSION: i64 = 20;\n");
        scratch.write(PIN_FILE, "pub const PIN: u32 = 1;\n");
        scratch.write(
            GUARD,
            "//! The calls.\nfn calls() {\n    let _ = Store::opening_on;\n    let _ = old_one;\n}\n",
        );
        scratch.write("crates/symphony/shipped/symphony.just", "tasks:\n");
        scratch.write("rust-toolchain.toml", "[toolchain]\nchannel = \"1.98.0\"\n");
        scratch.write("tools/surreal.sh", "#!/bin/sh\nVERSION=3.3.0\n");
        scratch.write("apps/symphony-web/.node-version", "24.1.0\n");
        scratch.write(LOCK, &lock("0.1.0"));
        scratch.write(
            SETTINGS,
            "# The project file\n\n## `[board]`\n\n| Key | Default |\n| --- | --- |\n| `guide` | \
             none |\n| `installation` | none |\n",
        );
        scratch.commit("first");
        scratch
    }

    fn git(&self, args: &[&str]) -> String {
        let named = ["-c", "user.name=t", "-c", "user.email=t@example.com"];
        git(&self.root, &[&named[..], args].concat()).unwrap()
    }

    fn write(&self, file: &str, text: &str) {
        let path = self.root.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// Everything written committed as `message`, or an empty commit: the commit.
    fn commit(&self, message: &str) -> String {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "--allow-empty", "-m", message]);
        self.git(&["rev-parse", "HEAD"])
    }

    /// `HEAD` tagged as the release `name`, as a release run tags it.
    fn release(&self, name: &str) {
        self.git(&["tag", "-a", name, "-m", name]);
    }

    /// A branch `name` started here, `work` done on it, then landed on main as the land does: one
    /// merge commit, never fast-forwarded.
    fn land(&self, name: &str, work: impl FnOnce(&Scratch)) {
        self.git(&["checkout", "-q", "-b", name]);
        work(self);
        self.git(&["checkout", "-q", "main"]);
        let message = format!("Merge branch '{name}'");
        self.git(&["merge", "-q", "--no-ff", "-m", &message, name]);
    }

    fn guard_without(&self, gone: &str) {
        let text = std::fs::read_to_string(self.root.join(GUARD)).unwrap();
        let kept: Vec<&str> = text.lines().filter(|line| !line.contains(gone)).collect();
        self.write(GUARD, &(kept.join("\n") + "\n"));
    }
}

/// A lock file naming two workspace packages, one at `version`, and one fetched package.
fn lock(version: &str) -> String {
    format!(
        "version = 4\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.0\"\nsource = \
         \"registry+https://example.com\"\n\n[[package]]\nname = \"symphony-app\"\nversion = \
         \"{version}\"\ndependencies = [\n \"serde\",\n]\n\n[[package]]\nname = \
         \"symphony-args\"\nversion = \"1.0.0\"\n"
    )
}

/// The line every release after the first says first: how a project names it.
fn named(release: &str, commit: &str) -> String {
    format!(
        "- Name this release in each of the project's Symphony dependency lines, `tag = \
         \"{release}\"`, or `rev = \"{commit}\"` where the project pins a commit, and let \
         Cargo.lock follow: a build run with `--locked` refuses until the lock is updated."
    )
}

fn entry(value: &str) -> Entry {
    parse(value).unwrap_or_else(|why| panic!("{value}: {why}"))
}

fn notes(release: &str, previous: Option<&str>, entries: &[&str]) -> Notes {
    Notes {
        release: release.into(),
        previous: previous.map(str::to_string),
        entries: entries.iter().map(|value| entry(value)).collect(),
        ..Notes::default()
    }
}

#[test]
fn a_trailer_names_its_section_and_says_its_text() {
    assert_eq!(
        entry("settings: `symphony.local.toml` beside symphony.toml overrides it"),
        Entry {
            release: None,
            section: Section::Settings,
            text: "`symphony.local.toml` beside symphony.toml overrides it".into(),
            moved: None,
            after: None,
        }
    );
    for (section, word) in [
        (Section::Manifest, "manifest"),
        (Section::Board, "board"),
        (Section::Api, "api"),
        (Section::Settings, "settings"),
        (Section::Init, "init"),
        (Section::Tools, "tools"),
        (Section::Once, "once"),
    ] {
        assert_eq!(entry(&format!("  {word} :  said  ")).section, section);
    }
    // A colon in the text is the text's.
    assert_eq!(
        entry("once: run: just tasks migrate").text,
        "run: just tasks migrate"
    );
    // An arrow outside `api` is text.
    assert_eq!(entry("settings: a => b").moved, None);
    // An `api` entry may say something no item moved for.
    assert_eq!(entry("api: `order::sort_by` is new").moved, None);
}

#[test]
fn an_api_entry_names_the_item_and_what_replaces_it() {
    assert_eq!(
        entry("api: `Store::opening_on` => `Store::reading_on`; it links no migration"),
        Entry {
            release: None,
            section: Section::Api,
            text: "it links no migration".into(),
            moved: Some(Moved {
                old: "Store::opening_on".into(),
                new: "Store::reading_on".into(),
                removed: false,
            }),
            after: None,
        }
    );
    let gone = entry("api: removed Store::opening_on => Store::reading_on");
    assert_eq!(gone.text, "");
    assert_eq!(
        gone.moved,
        Some(Moved {
            old: "Store::opening_on".into(),
            new: "Store::reading_on".into(),
            removed: true,
        })
    );
}

#[test]
fn a_trailer_may_name_the_release_already_cut_it_is_said_of() {
    for value in [
        "release-2026-11-18 api: a => b",
        "release-2026-11-18: api: a => b",
    ] {
        let read = entry(value);
        assert_eq!(
            read.release.as_deref(),
            Some("release-2026-11-18"),
            "{value}"
        );
        assert_eq!(read.section, Section::Api);
    }
    assert_eq!(
        entry("release-2026-11-18.2 once: x").release.as_deref(),
        Some("release-2026-11-18.2")
    );
}

#[test]
fn a_trailer_that_reads_as_no_entry_says_why() {
    for (value, why) in [
        ("just some words", "names no section"),
        ("", "names no section"),
        (
            "apis: a => b",
            "\"apis\" is no section: one of manifest, board, api, settings, init, tools, once",
        ),
        ("board:   ", "the board entry says nothing"),
        ("api: => new", "leaves out an item"),
        ("api: removed  => new", "leaves out an item"),
        ("api: old => ; why", "leaves out an item"),
        (
            "release-next api: a => b",
            "\"release-next\" names no release",
        ),
        ("release-2026-11-18.1 api: a => b", "names no release"),
    ] {
        let refused = parse(value).unwrap_err();
        assert!(refused.contains(why), "{value}: {refused}");
    }
}

#[test]
fn each_pinned_tool_is_read_from_its_file() {
    assert_eq!(
        toolchain("# pinned\n[toolchain]\nchannel = \"1.99.0\"\ncomponents = []\n"),
        Some("1.99.0".into())
    );
    assert_eq!(toolchain("[toolchain]\nchannels = \"1.99.0\"\n"), None);
    assert_eq!(toolchain("[toolchain]\nchannel = stable\n"), None);
    assert_eq!(
        surreal("# version VERSION\nVERSION=3.3.0\nurl $VERSION\n"),
        Some("3.3.0".into())
    );
    assert_eq!(surreal("  VERSION=3\nVERSION=\n"), None);
    assert_eq!(node("24.1.0\n"), Some("24.1.0".into()));
    assert_eq!(node("\n"), None);
    assert_eq!(
        postgres("/// The oldest.\npub const OLDEST_MAJOR: u32 = 18;\n"),
        Some("18".into())
    );
    assert_eq!(postgres("const NEWEST_MAJOR: u32 = 19;\n"), None);
}

#[test]
fn the_settings_reference_lists_each_key_under_its_tables_heading() {
    let text = "# The project file\n\n## The settings check\n\n| `not` | a key |\n\n## \
                `[agents]`\n\nProse with `[agents] panes`.\n\n| Key | Default | What it sets \
                |\n| --- | --- | --- |\n| `panes` | `\"none\"` | terminals |\n| `launch` | below \
                | the table |\n\n## `[agents.launch]`\n\n| `default` | `\"codex\"` | the agent \
                |\n\n## Afterwards\n\n| `stray` | no table |\n";
    let keys: Vec<String> = settings(text).into_iter().collect();
    assert_eq!(
        keys,
        [
            "[agents.launch] default",
            "[agents] launch",
            "[agents] panes"
        ]
    );
}

#[test]
fn a_guard_loses_a_call_only_when_no_line_of_it_holds_that_call_any_more() {
    let before = "//! The calls.\nfn calls() {\n    let _: fn(&str, &str) -> Config = \
                  hosted::Config::new; // as called\n    let _ = Store::opening_on;\n    \
                  wrapped(first, second);\n}\n";
    // Moved, wrapped anew with a trailing comma, commented anew, and a call added beside the old.
    let tidied = "//! The calls, said again.\nfn calls() {\n    let _ = Store::opening_on;\n    \
                  let _ = Store::reading_on;\n    // Its constructor.\n    let _: fn(\n        \
                  &str,\n        &str,\n    ) -> Config = hosted::Config::new;\n    \
                  wrapped(\n        first,\n        second,\n    );\n}\n";
    assert_eq!(lost(before, tidied), Vec::<String>::new());
    assert_eq!(lost(before, before), Vec::<String>::new());
    // The call gone, and one whose signature changed.
    let dropped = "fn calls() {\n    let _: fn(&str) -> Config = hosted::Config::new;\n    \
                   wrapped(first, second);\n}\n";
    assert_eq!(
        lost(before, dropped),
        [
            "let _: fn(&str, &str) -> Config = hosted::Config::new; // as called",
            "let _ = Store::opening_on;"
        ]
    );
    assert_eq!(
        lost(before, "").len(),
        4,
        "a guard deleted loses every call"
    );
    assert_eq!(lost("", before), Vec::<String>::new());
}

/// The files the notes read here say their versions as the reading expects: a rename or a move
/// would leave every release's notes silent on its tools.
#[test]
fn this_repositorys_tools_guards_and_shipped_files_are_where_the_notes_read_them() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (name, file, read) in TOOLS {
        let text = std::fs::read_to_string(workspace.join(file)).unwrap();
        assert!(read(&text).is_some(), "{name}: {file}");
    }
    assert!(workspace.join(SHIPPED).join("symphony.just").is_file());
    assert!(
        workspace
            .join("crates/tasks/tests/consumer_api.rs")
            .is_file()
    );
}

#[test]
fn a_releases_section_says_what_git_derived_and_each_entry_under_its_heading() {
    let mut taken = notes(
        "release-2026-11-18",
        Some("release-2026-11-17"),
        &[
            "once: run `just symphony-cli init --root .`",
            "board: schema 21 adds the writers' table",
            "settings: `[agents] launch` is read",
            "tools: PostgreSQL 18 or later for the board's server tests",
            "api: none: the guard was only rewrapped",
            "manifest: the library's line names `package = \"symphony-app\"`",
        ],
    );
    taken.commit = "c0ffee".into();
    taken.derived = Derived {
        packages: vec![
            ("symphony-app".into(), "0.1.0".into(), "0.2.0".into()),
            ("symphony-board".into(), "0.1.0".into(), "0.2.0".into()),
        ],
        schema: (Some(20), Some(21)),
        pin: (Some(1), Some(2)),
        tools: vec![("Rust toolchain".into(), "1.98.0".into(), "1.99.0".into())],
        settings: (
            vec!["[agents.launch] default".into()],
            vec!["[board] installation".into(), "[board] old".into()],
        ),
        shipped: vec!["symphony.just".into(), "skills/work/finish.md".into()],
        guards: vec![GUARD.into()],
    };
    let said = section(&[taken], 0);
    assert_eq!(
        said[..5],
        [
            "Cargo.toml and Cargo.lock:".to_string(),
            named("release-2026-11-18", "c0ffee"),
            "- Versions that move in Cargo.lock: symphony-app 0.1.0 to 0.2.0, symphony-board \
             0.1.0 to 0.2.0."
                .to_string(),
            "- the library's line names `package = \"symphony-app\"`".to_string(),
            "Board:".to_string(),
        ]
    );
    let steps = &said[5];
    assert!(
        steps.starts_with(
            "- Board schema 20 to 21. A board is moved only by a build at schema 21: take this \
             release first, so the project's own `tasks` is at 21. Rebuild at this release every \
             program of the project that writes its board"
        ),
        "{steps}"
    );
    for step in [
        "Then `just tasks migrate` in the project's main checkout moves the board.",
        "moves nothing (exit 2): that refusal is the one list of the board's writers.",
        "a path named by a hash) is left behind with `just tasks migrate --leave-behind`",
        "a build at schema 21 is refused the board at 20;",
        "a server still on a build at 20 reads the board and its writes are refused until it \
         runs a build at 21.",
    ] {
        assert!(steps.contains(step), "{step}: {steps}");
    }
    assert!(said[6].starts_with("- Pin 1 to 2: "), "{}", said[6]);
    assert_eq!(
        said[7..],
        [
            "- schema 21 adds the writers' table",
            "Public Rust items a consumer calls:",
            "- The calls a consumer's code is held to changed in: \
             crates/tasks/tests/consumer_api.rs.",
            "symphony.toml:",
            "- Keys read anew (docs/PROJECT_FILE.md says each): `[agents.launch] default`.",
            "- Keys no longer read, which `just symphony-cli settings-check` refuses as unknown: \
             `[board] installation`, `[board] old`.",
            "- `[agents] launch` is read",
            "Installed recipes, skills, hooks and rules:",
            "- `just symphony-cli init --root .` in the project rewrites: symphony.just, \
             skills/work/finish.md.",
            "Tools:",
            "- Rust toolchain 1.98.0 to 1.99.0.",
            "- PostgreSQL 18 or later for the board's server tests",
            "Run once:",
            "- run `just symphony-cli init --root .`",
        ]
    );
}

#[test]
fn a_release_that_asks_nothing_says_so_and_an_unchanged_schema_is_not_said() {
    let mut same = notes("release-2026-11-18", Some("release-2026-11-17"), &[]);
    same.derived.schema = (Some(21), Some(21));
    same.derived.pin = (None, Some(1));
    assert_eq!(
        section(&[same], 0),
        [
            "Cargo.toml and Cargo.lock:".to_string(),
            named("release-2026-11-18", ""),
            "Nothing else changes for a project.".to_string(),
        ]
    );
    // The first release is moved to from none.
    assert_eq!(
        section(&[notes("release-2026-11-17", None, &[])], 0),
        ["Nothing a project changes to take it."]
    );
    let mut unread = notes("release-2026-11-18", None, &[]);
    unread.unread = vec!["something a project must know".into()];
    assert_eq!(
        section(&[unread], 0),
        ["Said without a section:", "- something a project must know"]
    );
}

#[test]
fn a_replacement_and_its_later_removal_read_as_move_before_that_release() {
    let history = [
        notes("release-2026-11-17", None, &[]),
        notes(
            "release-2026-11-18",
            Some("release-2026-11-17"),
            &["api: Store::opening_on => Store::reading_on; it links no migration"],
        ),
        notes("release-2026-11-19", Some("release-2026-11-18"), &[]),
        notes(
            "release-2026-11-20",
            Some("release-2026-11-19"),
            &[
                "api: removed Store::opening_on => Store::reading_on",
                "api: removed never_replaced => its_caller; inline it",
                "api: kept => newer",
            ],
        ),
    ];
    assert_eq!(
        section(&history, 1)[2..],
        [
            "Public Rust items a consumer calls:",
            "- `Store::opening_on` is replaced by `Store::reading_on`; both work in this release. \
             Move to `Store::reading_on` before release-2026-11-20, which removes \
             `Store::opening_on`. it links no migration",
        ]
    );
    assert_eq!(
        section(&history, 3)[2..],
        [
            "Public Rust items a consumer calls:",
            "- `Store::opening_on` is removed: call `Store::reading_on`. `Store::reading_on` has \
             been there since release-2026-11-18.",
            "- `never_replaced` is removed: call `its_caller`. inline it",
            "- `kept` is replaced by `newer`; both work in this release.",
        ]
    );
    let document = document(&history);
    assert!(document.starts_with("# What a project changes to take each Symphony release\n\n"));
    let heads: Vec<&str> = document.lines().filter(|l| l.starts_with("## ")).collect();
    assert_eq!(
        heads,
        [
            "## release-2026-11-17",
            "## release-2026-11-18",
            "## release-2026-11-19",
            "## release-2026-11-20"
        ]
    );
    assert!(
        document.contains("## release-2026-11-17\n\nCommit: \nThe first release.\n\nNothing a")
    );
    assert!(
        document.contains(
            "## release-2026-11-19\n\nCommit: \nFrom release-2026-11-18.\n\nCargo.toml and"
        )
    );
}

#[test]
fn what_a_branch_owes_is_an_entry_for_each_thing_it_changed_under_a_consumer() {
    let moved = Changed {
        schema: Some((20, 21)),
        lost: vec![(GUARD.into(), "let _ = old_one;".into())],
        keys: vec!["[board] installation".into()],
    };
    let owed_by = |entries: &[&str]| {
        let entries: Vec<Result<Entry, String>> = entries.iter().map(|v| parse(v)).collect();
        owed(&moved, &entries)
    };
    let all = owed_by(&[]);
    assert_eq!(all.len(), 3, "{all:?}");
    assert!(
        all[0].starts_with("the board schema moves from 20 to 21, which a project's board must")
            && all[0].contains("`Consumer-Note: board: TEXT`"),
        "{}",
        all[0]
    );
    assert!(
        all[1].starts_with(
            "crates/tasks/tests/consumer_api.rs no longer holds a call a consumer makes (`let _ \
             = old_one;`)"
        ) && all[1].contains("`Consumer-Note: api: removed OLD => NEW`"),
        "{}",
        all[1]
    );
    assert!(
        all[2].starts_with("docs/PROJECT_FILE.md no longer lists [board] installation, ")
            && all[2].contains("`Consumer-Note: settings: TEXT`"),
        "{}",
        all[2]
    );
    assert_eq!(owed_by(&["tools: another section"]).len(), 3);
    assert_eq!(owed_by(&["board: the move adds a table"]).len(), 2);
    assert_eq!(owed_by(&["api: removed old_one => new_one"]).len(), 2);
    assert_eq!(
        owed_by(&["settings: `installation` is found, never set"]).len(),
        2
    );
    assert_eq!(
        owed_by(&[
            "board: the move adds a table",
            "api: none: only rewrapped",
            "settings: `installation` is found, never set"
        ]),
        Vec::<String>::new()
    );
    // Said of a release already cut, an entry answers for that release, not for this branch.
    assert_eq!(
        owed_by(&[
            "release-2026-11-18 board: said late",
            "release-2026-11-18 api: removed old_one => new_one",
            "release-2026-11-18 settings: said late"
        ])
        .len(),
        3
    );
    // A trailer that reads as no entry is red, and answers nothing.
    let unread = owed_by(&[
        "board: said",
        "api: removed old_one => new_one",
        "settings: said",
        "apis: x",
    ]);
    assert_eq!(unread.len(), 1, "{unread:?}");
    assert!(unread[0].contains("reads as no entry"), "{}", unread[0]);
    // A branch that changed none of them owes nothing, whatever it says or leaves unsaid.
    assert_eq!(owed(&Changed::default(), &[]), Vec::<String>::new());
}

/// What [`check`] says the branch checked out in `repo` owes, against `main`.
fn owes(repo: &Scratch) -> Vec<String> {
    match check(&repo.root, "main") {
        Ok(Checked::Owed(owed)) => owed,
        other => panic!("git reads the scratch repository: {other:?}"),
    }
}

#[test]
fn a_branch_that_moves_the_schema_or_drops_a_guarded_call_is_red_until_a_commit_says_so() {
    let tree = Tree::new();
    let repo = Scratch::new(&tree);
    assert_eq!(owes(&repo), Vec::<String>::new(), "main itself");
    assert_eq!(check(&repo.root, "trunk"), Ok(Checked::NoBase));

    repo.git(&["checkout", "-q", "-b", "tools/raise"]);
    repo.write(SCHEMA_FILE, "pub const VERSION: i64 = 21;\n");
    repo.guard_without("old_one");
    // Prose that names the trailer is no entry: git reads the message's last paragraph alone.
    repo.commit(
        "Raise the schema and drop a call\n\nConsumer-Note: board: said in the body, not a \
         trailer\n\nThe end.",
    );
    let owed = owes(&repo);
    assert_eq!(owed.len(), 2, "{owed:?}");
    assert!(owed[0].contains("from 20 to 21") && owed[1].contains("`let _ = old_one;`"));

    // A trailer that reads as no entry leaves it red, naming the commit.
    let bad = repo.commit("Say it\n\nConsumer-Note: boards: moved");
    let owed = owes(&repo);
    assert_eq!(owed.len(), 3, "{owed:?}");
    assert!(owed[0].contains(&bad[..7]) && owed[0].contains("\"boards\" is no section"));

    // Said after the fact, by a commit that changes nothing else; a trailer git folds over two
    // lines is one entry, and each of a commit's trailers is read.
    repo.git(&["reset", "-q", "--hard", "HEAD~1"]);
    repo.commit(
        "Say what a consumer does\n\nConsumer-Note: board: schema 21 adds the writers' table;\n  \
         nothing beyond the migrate\nConsumer-Note: api: removed old_one => new_one",
    );
    assert_eq!(owes(&repo), Vec::<String>::new());
}

#[test]
fn a_branch_is_answered_by_its_own_commits_alone_however_often_it_merged_main() {
    let tree = Tree::new();
    let repo = Scratch::new(&tree);
    // Main already holds an entry of each section, said by a landing of its own.
    repo.land("tools/earlier", |repo| {
        repo.commit(
            "Earlier\n\nConsumer-Note: board: earlier\nConsumer-Note: api: removed a => \
             b\nConsumer-Note: settings: earlier",
        );
    });
    repo.git(&["checkout", "-q", "-b", "tools/later"]);
    repo.write(SCHEMA_FILE, "pub const VERSION: i64 = 21;\n");
    repo.write(SETTINGS, "## `[board]`\n\n| `guide` | none |\n");
    repo.commit("Raise the schema and stop reading a key");
    assert_eq!(owes(&repo).len(), 2, "what main said answers nothing here");
    // Main moves, saying more, and the branch merges it once before its review.
    repo.git(&["checkout", "-q", "main"]);
    repo.commit("On main\n\nConsumer-Note: board: main's own\nConsumer-Note: settings: main's");
    repo.git(&["checkout", "-q", "tools/later"]);
    repo.git(&[
        "merge",
        "-q",
        "--no-ff",
        "-m",
        "Merge branch 'main'",
        "main",
    ]);
    let owed = owes(&repo);
    assert_eq!(owed.len(), 2, "{owed:?}");
    assert!(
        owed[1].contains("no longer lists [board] installation"),
        "{}",
        owed[1]
    );

    // An entry said of a release that is no tag here is refused; of one that is, it is read, and
    // still answers nothing of this branch.
    let said = repo.commit("Late\n\nConsumer-Note: release-2026-11-17 board: said late");
    let owed = owes(&repo);
    assert_eq!(owed.len(), 3, "{owed:?}");
    assert!(
        owed[0].contains(&said[..7])
            && owed[0].contains("release-2026-11-17, which is no release tagged here"),
        "{}",
        owed[0]
    );
    repo.git(&["tag", "-a", "release-2026-11-17", "-m", "one", "main~1"]);
    assert_eq!(owes(&repo).len(), 2);
    repo.commit("Say it\n\nConsumer-Note: board: a table\nConsumer-Note: settings: found now");
    assert_eq!(owes(&repo), Vec::<String>::new());
}

#[test]
fn a_guard_whose_lines_only_moved_or_gained_comments_owes_nothing() {
    let tree = Tree::new();
    let repo = Scratch::new(&tree);
    repo.git(&["checkout", "-q", "-b", "tools/tidy"]);
    repo.write(
        GUARD,
        "//! The calls, said again.\nfn calls() {\n    // The old one first.\n    let _ = \
         old_one;\n    let _ = Store::opening_on;\n    let _ = Store::reading_on;\n}\n",
    );
    repo.commit("Reorder the guard and add a call");
    assert_eq!(owes(&repo), Vec::<String>::new());
}

#[test]
fn the_notes_of_two_releases_come_from_git_and_the_commits_between_them() {
    let tree = Tree::new();
    let repo = Scratch::new(&tree);
    let first = repo.git(&["rev-parse", "HEAD"]);
    repo.release("release-2026-11-17");
    repo.land("tools/reader", |repo| {
        repo.write(SCHEMA_FILE, "pub const VERSION: i64 = 21;\n");
        repo.write("crates/symphony/shipped/symphony.just", "tasks:\nland:\n");
        repo.write("rust-toolchain.toml", "[toolchain]\nchannel = \"1.99.0\"\n");
        repo.write(LOCK, &lock("0.2.0"));
        repo.write(
            SETTINGS,
            "## `[board]`\n\n| `guide` | none |\n| `postgresql` | none |\n",
        );
        repo.commit(
            "Open a board to read\n\nConsumer-Note: board: schema 21 adds the writers' \
             table\nConsumer-Note: api: Store::opening_on => Store::reading_on",
        );
    });
    let second = repo.git(&["rev-parse", "HEAD"]);
    repo.release("release-2026-11-18");
    let mut late = String::new();
    repo.land("tools/drop", |repo| {
        repo.guard_without("opening_on");
        repo.commit(
            "Drop the old opening\n\nConsumer-Note: api: removed Store::opening_on => \
             Store::reading_on",
        );
        // What the release before left unsaid, said now and named its.
        late = repo.commit(
            "Say it\n\nConsumer-Note: release-2026-11-18 settings: `symphony.local.toml` is read",
        );
    });
    let late = repo.git(&["rev-parse", "--short", &late]);
    let tip = repo.git(&["rev-parse", "HEAD"]);

    // The release being cut is not a tag yet.
    let releases = through(&repo.root, "release-2026-11-19", &tip).unwrap();
    assert_eq!(
        releases,
        [
            ("release-2026-11-17".to_string(), first.clone()),
            ("release-2026-11-18".to_string(), second.clone()),
            ("release-2026-11-19".to_string(), tip.clone()),
        ]
    );
    let history = history(&repo.root, &releases).unwrap();
    assert_eq!(history[0].derived.schema, (None, Some(20)));
    assert_eq!(history[0].entries, []);
    assert_eq!(
        history[1].derived,
        Derived {
            schema: (Some(20), Some(21)),
            pin: (Some(1), Some(1)),
            packages: vec![("symphony-app".into(), "0.1.0".into(), "0.2.0".into())],
            tools: vec![("Rust toolchain".into(), "1.98.0".into(), "1.99.0".into())],
            settings: (
                vec!["[board] postgresql".into()],
                vec!["[board] installation".into()]
            ),
            shipped: vec!["symphony.just".into()],
            guards: Vec::new(),
        }
    );
    assert_eq!(history[2].derived.guards, [GUARD]);
    assert_eq!(history[2].derived.schema, (Some(21), Some(21)));
    assert_eq!(
        history[1].entries[2].after,
        Some((late.clone(), "release-2026-11-19".to_string())),
        "the correction says where it was said"
    );
    assert_eq!(history[2].late, [history[1].entries[2].clone()]);
    assert_eq!(history[1].late, []);

    let document = document_at(&repo.root, "release-2026-11-19", &tip).unwrap();
    assert!(document.contains(&format!(
        "## release-2026-11-17\n\nCommit: {first}\nThe first release.\n\nNothing a project"
    )));
    let from = document.find("## release-2026-11-18").unwrap();
    assert_eq!(
        &document[from..],
        format!(
            "## release-2026-11-18\n\nCommit: {second}\nFrom release-2026-11-17.\n\nCargo.toml \
             and Cargo.lock:\n{}\n- Versions that move in Cargo.lock: symphony-app 0.1.0 to \
             0.2.0.\nBoard:\n- {}\n- schema 21 adds the writers' table\nPublic Rust items a \
             consumer calls:\n- `Store::opening_on` is replaced by `Store::reading_on`; both work \
             in this release. Move to `Store::reading_on` before release-2026-11-19, which \
             removes `Store::opening_on`.\nsymphony.toml:\n- Keys read anew \
             (docs/PROJECT_FILE.md says each): `[board] postgresql`.\n- Keys no longer read, \
             which `just symphony-cli settings-check` refuses as unknown: `[board] \
             installation`.\n- `symphony.local.toml` is read (added after this release was cut, \
             by the commit {late} that release-2026-11-19 holds: not in its tag's \
             message)\nInstalled recipes, skills, hooks and rules:\n- `just symphony-cli init \
             --root .` in the project rewrites: symphony.just.\nTools:\n- Rust toolchain 1.98.0 \
             to 1.99.0.\n\n## release-2026-11-19\n\nCommit: {tip}\nFrom \
             release-2026-11-18.\n\nCargo.toml and Cargo.lock:\n{}\nPublic Rust items a \
             consumer calls:\n- `Store::opening_on` is removed: call `Store::reading_on`. \
             `Store::reading_on` has been there since release-2026-11-18.\n- The calls a \
             consumer's code is held to changed in: crates/tasks/tests/consumer_api.rs.\nSaid \
             with this release of an earlier one, for a project already at or past it:\n- Of \
             release-2026-11-18, symphony.toml: `symphony.local.toml` is read\n",
            named("release-2026-11-18", &second),
            schema_steps(20, 21),
            named("release-2026-11-19", &tip),
        )
    );

    // The tag's message says the release's own section, under its line.
    let taking = taking(&repo.root, "release-2026-11-19", &tip).unwrap();
    assert_eq!(taking[0], TAKING);
    assert_eq!(taking[1], "Cargo.toml and Cargo.lock:");
    assert_eq!(taking[3], "Public Rust items a consumer calls:");
    // What its commits said of the release before is in its message too.
    assert_eq!(
        taking[6..],
        [
            "Said with this release of an earlier one, for a project already at or past it:",
            "- Of release-2026-11-18, symphony.toml: `symphony.local.toml` is read"
        ],
        "{taking:?}"
    );
    // A project at the release corrected reads the correction when it takes the next, from the
    // notes alone; one behind both reads it under each.
    let at = symphony::machine::release::notes_between;
    let next = at(&document, Some("release-2026-11-18"), None).unwrap();
    assert!(next.starts_with("## release-2026-11-19\n"), "{next}");
    assert_eq!(
        next.matches("`symphony.local.toml` is read").count(),
        1,
        "{next}"
    );
    let both = at(&document, Some("release-2026-11-17"), None).unwrap();
    assert_eq!(
        both.matches("`symphony.local.toml` is read").count(),
        2,
        "{both}"
    );

    // Once it is tagged, the same notes: the tag is not read twice.
    repo.release("release-2026-11-19");
    assert_eq!(
        document_at(&repo.root, "release-2026-11-19", &tip).unwrap(),
        document
    );
    // A release on another line than the commit's is not among its releases.
    repo.git(&["checkout", "-q", "-b", "side", "release-2026-11-17"]);
    repo.commit("aside");
    repo.release("release-2026-11-19.2");
    let on_main = super::releases(&repo.root, &tip).unwrap();
    assert_eq!(on_main.len(), 3, "{on_main:?}");
    assert_eq!(on_main[2], ("release-2026-11-19".to_string(), tip.clone()));
}

#[test]
fn the_command_prints_every_releases_notes_and_checks_a_branch() {
    let tree = Tree::new();
    let repo = Scratch::new(&tree);
    let first = repo.git(&["rev-parse", "HEAD"]);
    repo.release("release-2026-11-17");
    let run = |args: &[&str]| {
        let root = repo.root.to_string_lossy().into_owned();
        let args: Vec<String> = args.iter().map(|arg| arg.replace("ROOT", &root)).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = main(&args, &mut out, &mut err);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    };
    let (code, out, err) = run(&["ROOT"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert!(
        out.ends_with(&format!(
            "## release-2026-11-17\n\nCommit: {first}\nThe first release.\n\nNothing a project \
             changes to take it.\n"
        )),
        "{out}"
    );
    assert_eq!(run(&["--check", "ROOT"]), (0, String::new(), String::new()));

    repo.git(&["checkout", "-q", "-b", "tools/raise"]);
    repo.write(SCHEMA_FILE, "pub const VERSION: i64 = 21;\n");
    repo.commit("Raise the schema");
    let (code, out, err) = run(&["ROOT", "--check"]);
    assert_eq!((code, out.as_str()), (1, ""));
    assert!(
        err.starts_with("consumer-notes: the board schema moves from 20 to 21")
            && err
                .trim_end()
                .ends_with("(docs/RELEASES.md, \"Its notes\")"),
        "{err}"
    );
    // What the branch's commits would say in the next release's notes, and of the one before.
    repo.commit(
        "Say it\n\nConsumer-Note: board: a table\nConsumer-Note: release-2026-11-17 once: run it",
    );
    assert_eq!(run(&["--check", "ROOT"]), (0, String::new(), String::new()));
    let (code, out, _) = run(&["ROOT"]);
    assert_eq!(code, 0);
    assert!(
        out.contains("\nRun once:\n- run it (added after this release was cut, by the commit ")
            && out.contains("that unreleased holds: not in its tag's message)\n\n## unreleased\n")
            && out.ends_with(&format!(
                "From release-2026-11-17.\n\nCargo.toml and Cargo.lock:\n{}\nBoard:\n- {}\n- a \
                 table\nSaid with this release of an earlier one, for a project already at or \
                 past it:\n- Of release-2026-11-17, Run once: run it\n",
                named("unreleased", &repo.git(&["rev-parse", "HEAD"])),
                schema_steps(20, 21)
            )),
        "{out}"
    );
    for (args, why) in [
        (vec!["--chekc"], "unknown --chekc"),
        (vec!["ROOT", "more"], "unexpected more"),
    ] {
        let (code, _, err) = run(&args);
        assert_eq!(code, 2);
        assert!(err.starts_with(USAGE) && err.contains(why), "{err}");
    }
    let (code, _, err) = run(&[&tree.path("nowhere").to_string_lossy()]);
    assert_eq!(code, 2);
    assert!(err.starts_with("symphony-build consumer-notes: "), "{err}");
}

#[test]
fn a_landed_trailer_that_reads_as_no_entry_is_said_as_written_and_a_checkout_with_no_default_branch_is_not_checked()
 {
    let tree = Tree::new();
    let repo = Scratch::new(&tree);
    repo.release("release-2026-11-17");
    repo.land("tools/unread", |repo| {
        repo.commit("Say it badly\n\nConsumer-Note: a project runs the migrate\nConsumer-Note: once: run it");
    });
    let tip = repo.git(&["rev-parse", "HEAD"]);
    let history = history(
        &repo.root,
        &through(&repo.root, "release-2026-11-18", &tip).unwrap(),
    )
    .unwrap();
    assert_eq!(history[1].unread, ["a project runs the migrate"]);
    assert_eq!(
        section(&history, 1)[2..],
        [
            "Run once:",
            "- run it",
            "Said without a section:",
            "- a project runs the migrate"
        ]
    );

    // A checkout whose default branch is not there: said, and green.
    repo.git(&["checkout", "-q", "-b", "only"]);
    repo.git(&["branch", "-q", "-D", "main"]);
    let args = [
        "--check".to_string(),
        repo.root.to_string_lossy().into_owned(),
    ];
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(main(&args, &mut out, &mut err), 0);
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "consumer-notes: this checkout has no main to compare with: nothing checked\n"
    );
    assert!(err.is_empty());
}

#[test]
fn the_lock_names_each_workspace_package_with_its_version_and_no_source() {
    let found: Vec<(String, String)> = packages(&lock("0.2.0")).into_iter().collect();
    assert_eq!(
        found,
        [
            ("symphony-app".to_string(), "0.2.0".to_string()),
            ("symphony-args".to_string(), "1.0.0".to_string()),
        ]
    );
    assert!(packages("version = 4\n").is_empty());
    // This repository's lock reads so: a change of its shape would silence every release's notes.
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let own = packages(&std::fs::read_to_string(workspace.join(LOCK)).unwrap());
    assert!(own.contains_key("symphony-app") && own.contains_key("symphony-board"));
    assert!(!own.contains_key("serde"));
}

/// The branch `slug` started at `from` with one commit of `message` writing a file of its own,
/// then squashed onto main as a pull request delivers it, its receipt kept: the receipt.
fn deliver(
    repo: &Scratch,
    slug: &str,
    from: &str,
    message: &str,
) -> symphony_board::land::delivery::Receipt {
    use crate::release::receipts::tests::{keep, squash};
    repo.git(&["checkout", "-q", "-b", slug, from]);
    repo.write(&format!("{}.txt", slug.replace('/', "-")), slug);
    let head = repo.commit(message);
    repo.git(&["checkout", "-q", "main"]);
    let receipt = squash(&repo.root, slug, &head);
    keep(&repo.root, &receipt);
    receipt
}

#[test]
fn what_a_delivered_landing_said_is_read_from_the_head_its_receipt_kept() {
    let tree = Tree::new();
    let repo = Scratch::new(&tree);
    repo.release("release-2026-11-17");
    let receipt = deliver(
        &repo,
        "tools/reader",
        "main",
        "Open a board to read\n\nConsumer-Note: board: schema 21 adds the writers' table",
    );
    // The squash itself says nothing: what the host wrote is its title alone.
    let tip = repo.git(&["rev-parse", "HEAD"]);
    assert_eq!(tip, receipt.squash);
    assert_eq!(entries(&repo.root, "release-2026-11-17..HEAD").unwrap(), []);
    let said = |repo: &Scratch| {
        let history = history(
            &repo.root,
            &through(&repo.root, "release-2026-11-18", &tip).unwrap(),
        )
        .unwrap();
        history[1].entries.clone()
    };
    assert_eq!(
        said(&repo),
        [entry("board: schema 21 adds the writers' table")]
    );
    let taking = taking(&repo.root, "release-2026-11-18", &tip).unwrap();
    assert!(
        (taking.iter()).any(|line| line.contains("schema 21 adds the writers' table")),
        "{taking:?}"
    );
    // By the receipt alone, borne out: one git does not bear out, or none, reads nothing.
    let path = symphony_board::land::delivery::Receipt::path(
        &crate::release::receipts::lands(&repo.root).unwrap(),
        "tools/reader",
    );
    let wrong = symphony_board::land::delivery::Receipt {
        tree: receipt.parent.clone(),
        ..receipt.clone()
    };
    std::fs::write(&path, serde_json::to_string(&wrong).unwrap()).unwrap();
    assert_eq!(said(&repo), []);
    std::fs::remove_file(&path).unwrap();
    assert_eq!(said(&repo), []);
}

#[test]
fn a_release_on_a_delivered_head_stays_in_the_notes_and_is_the_one_before_the_next() {
    let tree = Tree::new();
    let repo = Scratch::new(&tree);
    let first = repo.git(&["rev-parse", "HEAD"]);
    repo.release("release-2026-11-17");
    // Main moves on; a release candidate held at the first commit is fixed on a branch of its
    // own, delivered by a squash, and released on that branch's head.
    repo.land("tools/other", |repo| {
        repo.commit("Other work\n\nConsumer-Note: settings: `other` is read");
    });
    let fix = deliver(
        &repo,
        "tests/fix",
        &first,
        "Fix it\n\nConsumer-Note: board: the fix moves a row",
    );
    repo.git(&[
        "tag",
        "-a",
        "release-2026-11-18",
        "-m",
        "release-2026-11-18",
        &fix.head,
    ]);
    repo.land("tools/later", |repo| {
        repo.commit("Later work\n\nConsumer-Note: settings: `later` is read");
    });
    let tip = repo.git(&["rev-parse", "HEAD"]);
    let names = |releases: Vec<(String, String)>| -> Vec<String> {
        releases.into_iter().map(|(name, _)| name).collect()
    };
    assert_eq!(
        names(releases(&repo.root, &tip).unwrap()),
        ["release-2026-11-17", "release-2026-11-18"],
        "held by the squash that delivered its head"
    );
    assert_eq!(
        names(releases(&repo.root, &fix.parent).unwrap()),
        ["release-2026-11-17"],
        "not before its squash"
    );
    let history = history(
        &repo.root,
        &through(&repo.root, "release-2026-11-19", &tip).unwrap(),
    )
    .unwrap();
    assert_eq!(history.len(), 3);
    assert_eq!(history[1].commit, fix.head);
    assert_eq!(history[1].entries, [entry("board: the fix moves a row")]);
    assert_eq!(
        history[2].previous.as_deref(),
        Some("release-2026-11-18"),
        "the next tag composes from it"
    );
    // What main held that the candidate's release did not, and what landed since; the fix said
    // its own in the release on its head, and its squash does not say it again.
    assert_eq!(
        history[2].entries,
        [
            entry("settings: `other` is read"),
            entry("settings: `later` is read")
        ]
    );
    let document = document(&history);
    assert!(document.contains("release-2026-11-18"), "{document}");
    // With its receipt gone it is a tag main does not hold, as before receipts.
    std::fs::remove_file(symphony_board::land::delivery::Receipt::path(
        &crate::release::receipts::lands(&repo.root).unwrap(),
        "tests/fix",
    ))
    .unwrap();
    assert_eq!(
        names(releases(&repo.root, &tip).unwrap()),
        ["release-2026-11-17"]
    );
}

#[test]
fn a_head_started_at_another_delivered_head_says_that_ones_entries_once() {
    let tree = Tree::new();
    let repo = Scratch::new(&tree);
    let first = repo.git(&["rev-parse", "HEAD"]);
    repo.release("release-2026-11-17");
    // A release candidate held at the first commit advances twice, each fix started where the
    // one before left it and delivered by a squash, then is dropped: no release is tagged on
    // either head, so neither is the commit the next release's entries are read beyond.
    let one = deliver(
        &repo,
        "tests/one",
        &first,
        "Fix one\n\nConsumer-Note: board: the first fix moves a row",
    );
    let two = deliver(
        &repo,
        "tests/two",
        &one.head,
        "Fix two\n\nConsumer-Note: settings: the second fix reads `two`",
    );
    let after_two = repo.git(&["rev-parse", "HEAD"]);
    assert_eq!(after_two, two.squash);
    let both = [
        entry("board: the first fix moves a row"),
        entry("settings: the second fix reads `two`"),
    ];
    let history_at = |name: &str, tip: &str| {
        history(&repo.root, &through(&repo.root, name, tip).unwrap()).unwrap()
    };
    assert_eq!(
        history_at("release-2026-11-18", &after_two)[1].entries,
        both
    );
    // Released between the two squashes instead, on main: the second's release says its own
    // entry alone, as the first's is in the release before it.
    repo.git(&[
        "tag",
        "-a",
        "release-2026-11-18",
        "-m",
        "release-2026-11-18",
        &one.squash,
    ]);
    let history = history_at("release-2026-11-19", &after_two);
    assert_eq!(history[1].entries, both[..1]);
    assert_eq!(history[2].entries, both[1..]);
}

/// One landing [`deliver_all`] delivers: its slug, the earlier one of those delivered with it
/// whose head its branch starts at (main's tip where none), what its one commit says, and the
/// release tagged on main once its squash is there, if any.
struct Planned {
    slug: String,
    from: Option<usize>,
    message: String,
    release: Option<String>,
}

/// Each of `planned` delivered as [`deliver`] delivers one, in a single `git fast-import` rather
/// than a dozen commands for each: its branch's one commit writing a file of its own, squashed
/// onto main in one commit of one parent, its head retained, then its receipt kept. The receipts,
/// in order.
fn deliver_all(
    repo: &Scratch,
    planned: &[Planned],
) -> Vec<symphony_board::land::delivery::Receipt> {
    use std::collections::HashMap;
    use symphony_board::land::delivery::{RETAINED, Receipt};
    let first = repo.git(&["rev-parse", "main"]);
    let data = |text: &str| format!("data {}\n{text}\n", text.len());
    let mut stream = String::new();
    let mut main = first.clone();
    for (at, plan) in planned.iter().enumerate() {
        let (head, squash) = (2 * at + 1, 2 * at + 2);
        let from =
            (plan.from).map_or_else(|| main.clone(), |earlier| format!(":{}", 2 * earlier + 1));
        let who = format!("t <t@example.com> {} +0000", 1_700_000_000 + at);
        let file = format!(
            "M 100644 inline {}.txt\n{}",
            plan.slug.replace('/', "-"),
            data(&plan.slug)
        );
        let commit = |branch: &str, mark: usize, message: &str, from: &str| {
            format!(
                "commit refs/heads/{branch}\nmark :{mark}\nauthor {who}\ncommitter {who}\n{}from \
                 {from}\n{file}",
                data(message)
            )
        };
        stream += &commit(&plan.slug, head, &plan.message, &from);
        stream += &commit("main", squash, &format!("{}: its title", plan.slug), &main);
        stream += &format!("reset {RETAINED}{}\nfrom :{head}\n", plan.slug);
        main = format!(":{squash}");
        if let Some(release) = &plan.release {
            stream += &format!(
                "tag {release}\nfrom {main}\ntagger {who}\n{}",
                data(release)
            );
        }
    }
    let file = repo.root.with_file_name("delivered.marks");
    let export = format!("--export-marks={}", file.display());
    crate::release::git::git_fed(&repo.root, &["fast-import", "--quiet", &export], &stream)
        .unwrap();
    let marks: HashMap<String, String> = (std::fs::read_to_string(&file).unwrap().lines())
        .filter_map(|line| line.split_once(' '))
        .map(|(mark, commit)| (mark.to_string(), commit.to_string()))
        .collect();
    let trees: HashMap<String, String> = (repo.git(&["log", "--format=%H %T", "main"]).lines())
        .filter_map(|line| line.split_once(' '))
        .map(|(commit, tree)| (commit.to_string(), tree.to_string()))
        .collect();
    let lands = crate::release::receipts::lands(&repo.root).unwrap();
    let mut parent = first;
    (planned.iter().enumerate())
        .map(|(at, plan)| {
            let squash = marks[&format!(":{}", 2 * at + 2)].clone();
            let receipt = Receipt {
                slug: plan.slug.clone(),
                task: 7,
                operation: "task".into(),
                repository: 11,
                name: "owner/repository".into(),
                pull: 5,
                head: marks[&format!(":{}", 2 * at + 1)].clone(),
                base_observed: parent.clone(),
                parent: std::mem::replace(&mut parent, squash.clone()),
                tree: trees[&squash].clone(),
                squash,
                revision: "r1".into(),
                by: "lander".into(),
                import: None,
            };
            let path = Receipt::path(&lands, &receipt.slug);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, serde_json::to_string_pretty(&receipt).unwrap()).unwrap();
            receipt
        })
        .collect()
}

/// Fifty landings delivered by pull requests between six releases, each saying an entry of its
/// own. Every fifth head was started at the head before it, and so was the first after each
/// release, so a head holds another's commits. Git bears out two receipts no more, the second the
/// one a later head was started at. The notes say each entry once, in the release its squash came
/// in, but for the unborne receipt no later head holds. What they cost in git grows with the
/// receipts, never with their pairs: the receipts are proved once for the whole history, and
/// each delivered squash is read by one `git log` of its own.
#[test]
fn fifty_delivered_landings_are_said_once_each_by_git_linear_in_their_receipts() {
    use symphony_board::land::delivery::Receipt;
    let tree = Tree::new();
    let repo = Scratch::new(&tree);
    repo.release("release-2026-11-01");
    let planned: Vec<Planned> = (0..50_usize)
        .map(|n| Planned {
            slug: format!("tools/r{n:02}"),
            from: (n > 0 && (n % 5 == 4 || n % 10 == 0)).then(|| n - 1),
            message: format!("Work {n}\n\nConsumer-Note: settings: `r{n:02}` is read"),
            release: (n % 10 == 9).then(|| format!("release-2026-11-{:02}", n / 10 + 2)),
        })
        .collect();
    let receipts = deliver_all(&repo, &planned);
    let lands = crate::release::receipts::lands(&repo.root).unwrap();
    for unborne in [&receipts[31], &receipts[43]] {
        let wrong = Receipt {
            tree: unborne.parent.clone(),
            ..unborne.clone()
        };
        let path = Receipt::path(&lands, &wrong.slug);
        std::fs::write(path, serde_json::to_string(&wrong).unwrap()).unwrap();
    }
    let last = receipts.last().cloned();
    let tip = repo.git(&["rev-parse", "HEAD"]);
    let all = releases(&repo.root, &tip).unwrap();
    assert_eq!(all.len(), 6);
    let counted = || {
        let from = crate::release::git::ran();
        let history = history(&repo.root, &all);
        (history, crate::release::git::ran() - from)
    };

    let (read, with) = counted();
    let said: Vec<Vec<Entry>> = (read.unwrap().into_iter())
        .map(|notes| notes.entries)
        .collect();
    let expected: Vec<Vec<Entry>> = (0..6)
        .map(|release| {
            let landed = (release > 0).then(|| (release - 1) * 10..release * 10);
            (landed.into_iter().flatten())
                .filter(|n| *n != 31)
                .map(|n| entry(&format!("settings: `r{n:02}` is read")))
                .collect()
        })
        .collect();
    assert_eq!(said, expected);

    // A delivered squash whose head is gone is refused, as it was.
    let gone = Receipt {
        head: "f".repeat(40),
        ..last.clone().unwrap()
    };
    let path = Receipt::path(&lands, &gone.slug);
    std::fs::write(&path, serde_json::to_string(&gone).unwrap()).unwrap();
    let refused = history(&repo.root, &all).unwrap_err();
    assert!(
        refused.starts_with("git log --reverse") && refused.contains(&gone.head),
        "{refused}"
    );
    std::fs::write(&path, serde_json::to_string(&last.unwrap()).unwrap()).unwrap();

    // The same history read with no receipt: what the receipts cost is the difference. Each of
    // the five ranges lists its commits once, each of the 48 squashes git bears out is read
    // once, and two reads prove every receipt.
    std::fs::remove_dir_all(lands.join(symphony_board::land::delivery::RECEIPTS)).unwrap();
    let (_, without) = counted();
    assert_eq!(with - without, 5 + 48 + 2);
}
