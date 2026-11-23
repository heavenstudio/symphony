//! The Settings page's write (`SETTING_WRITE_PATH`, `symphony::save`) on a private repository and
//! a private board: a preference kept in the board's store is in force at once, the store named
//! as its source over the committed value; made the project's, it is one commit on main naming
//! the key, the old value and the new, the file's comments and order kept and the entry removed;
//! refused with nothing written where the settings check is red or the key is not a preference;
//! and where the project delivers by pull request it waits on a branch of its own, its entry in
//! force, until its delivery is proved.
use super::*;
use symphony::commit::pending::{self, State};
use symphony::settings::stored::Board;
use symphony_api::{SettingScope, SettingsPage};

const WRITE: &str = symphony_api::SETTING_WRITE_PATH;

/// The fixture's project file with preferences written, a comment among them.
fn with_preferences() -> String {
    format!("{VERDICTS}\n[agents]\npanes = \"none\"\n\n[limits]\n# held to\nlow_volume = 2\n")
}

/// The fixture's repository with `text` as its committed project file.
fn committing(f: &Fixture, text: &str) -> PathBuf {
    let root = f.repo();
    write(&root.join("symphony.toml"), text);
    git(&root, &["commit", "-qam", "the project's preferences"]);
    root
}

fn written(saves: &ClosingSaves, key: &str, action: &str, value: Option<Value>) -> Value {
    let mut payload = json!({"key": key, "action": action});
    if let Some(value) = value {
        payload["value"] = value;
    }
    saves.post("", WRITE, payload)
}

/// A write refused: its status and why.
fn refused(saves: &ClosingSaves, payload: Value) -> (u16, String) {
    let failure = saves.0.post("", WRITE, &payload).unwrap_err();
    (failure.status, failure.error)
}

/// The page as the server reads it, with the board.
fn page(root: &Path) -> SettingsPage {
    let machine = symphony::settings::Machine::of_process();
    symphony::settings::page_with(root, &machine, &Board::of(root))
}

/// The setting the page names `key`: its value, its source's scope, where it is said and what it
/// stands over.
fn shown(root: &Path, key: &str) -> (String, Option<SettingScope>, String, Option<String>) {
    let page = page(root);
    let setting = (page.sections.iter().flat_map(|section| &section.entries))
        .flat_map(|entry| &entry.settings)
        .find(|setting| setting.key == key)
        .unwrap_or_else(|| panic!("no setting {key}"))
        .clone();
    let source = setting.source;
    (setting.value, source.scope, source.said, source.over)
}

/// What the store's entry for `key` holds: none once removed, or never written.
fn entry(f: &Fixture, key: &str) -> Option<Value> {
    (f.store().stored_entries().unwrap().into_iter())
        .find(|entry| entry.key == key)
        .and_then(|entry| entry.value)
}

/// The line `text` writes `needle` on, from 1.
fn line_of(text: &str, needle: &str) -> usize {
    text.lines().position(|line| line == needle).unwrap() + 1
}

pub(super) fn each_preference_holds_at_once_with_the_store_as_its_source(f: &Fixture) {
    let text = with_preferences();
    let root = committing(f, &text);
    let tip = git(&root, &["rev-parse", "main"]);
    let saves = ClosingSaves::start(&root);
    let said = written(&saves, "limits.low_volume", "store", Some(json!(4)));
    assert_eq!(
        said["said"],
        "[limits] low_volume is 4 in the board's store, in force at once unless \
         symphony.local.toml writes it"
    );
    assert_eq!(said["waiting"], false);
    for (key, value) in [
        ("agents.panes", json!("hc")),
        ("agents.models.reviewer", json!("opus")),
        ("agents.launch.default", json!("codex")),
    ] {
        written(&saves, key, "store", Some(value));
    }
    // The same value again writes nothing more.
    let again = written(&saves, "limits.low_volume", "store", Some(json!(4)));
    assert!(
        again["said"]
            .as_str()
            .unwrap()
            .ends_with("already: nothing written"),
        "{again}"
    );
    let low = line_of(&text, "low_volume = 2");
    assert_eq!(
        shown(&root, "[limits] low_volume"),
        (
            "4".into(),
            Some(SettingScope::Board),
            "the board's store [limits] low_volume".into(),
            Some(format!("2 (symphony.toml [limits] low_volume, line {low})")),
        )
    );
    let panes = line_of(&text, "panes = \"none\"");
    assert_eq!(
        shown(&root, "[agents] panes"),
        (
            "hc".into(),
            Some(SettingScope::Board),
            "the board's store [agents] panes".into(),
            Some(format!(
                "\"none\" (symphony.toml [agents] panes, line {panes})"
            )),
        )
    );
    for (key, value) in [
        ("[agents.models] reviewer", "opus"),
        ("[agents] launch default", "codex"),
    ] {
        let (held, scope, said, _) = shown(&root, key);
        assert_eq!(
            (held.as_str(), scope),
            (value, Some(SettingScope::Board)),
            "{key}"
        );
        assert_eq!(said, format!("the board's store {key}"));
    }
    // The page lists what it writes, each with the store's entry and its record.
    let listed = page(&root).stored;
    let low_volume = listed
        .iter()
        .find(|item| item.key == "limits.low_volume")
        .unwrap();
    assert_eq!(low_volume.setting, "[limits] low_volume");
    assert_eq!(
        (low_volume.value.as_deref(), low_volume.held),
        (Some("4"), true)
    );
    assert_eq!(low_volume.changes.len(), 1);
    assert_eq!(low_volume.changes[0].by, "owner");
    assert!(listed.iter().all(|item| item.waiting.is_none()));
    // What the board holds its work to says the same.
    let limits = symphony_board::sys::output(
        symphony_board::sys::Command::new(std::env::var("SYMPHONY_AFTER_COMMIT_TASKS").unwrap())
            .arg("limits")
            .current_dir(&root),
    )
    .unwrap();
    let printed = String::from_utf8_lossy(&limits.stdout);
    let row = printed
        .lines()
        .find(|line| line.starts_with("low_volume"))
        .unwrap_or_else(|| panic!("{printed}"));
    assert!(
        row.contains(" 4 ") && row.contains("the board's store"),
        "{row}"
    );

    // A key that is no preference is refused, whatever the request names, and nothing written.
    for (payload, said) in [
        (
            json!({"key":"land.check","action":"store","value":false}),
            "holds a command or a program",
        ),
        (
            json!({"key":"verdicts.check","action":"store","value":"rm"}),
            "holds a command or a program",
        ),
        (
            json!({"key":"project.name","action":"store","value":"x"}),
            "is not a setting the board's store keeps",
        ),
        (
            json!({"key":"limits.low_volume","action":"store","value":-1}),
            "low_volume",
        ),
        (
            json!({"key":"limits.low_volume","action":"store"}),
            "gives the value",
        ),
        (json!({"key":"","action":"store","value":1}), ""),
        (
            json!({"key":"limits.low_volume","action":"burn"}),
            "not a setting's write",
        ),
    ] {
        let (status, why) = refused(&saves, payload.clone());
        assert_eq!(status, 400, "{payload}: {why}");
        assert!(why.contains(said), "{payload}: {why}");
    }
    for key in ["land.check", "verdicts.check", "project.name"] {
        assert_eq!(entry(f, key), None, "{key}");
    }
    let (status, why) = refused(&saves, json!({"key":"land.check","action":"commit"}));
    assert_eq!(status, 409, "{why}");
    // A worktree's page writes nothing.
    let worktree = saves
        .0
        .post(
            "wt",
            WRITE,
            &json!({"key":"limits.low_volume","action":"remove"}),
        )
        .unwrap_err();
    assert_eq!(worktree.status, 404, "{}", worktree.error);
    assert_eq!(entry(f, "limits.low_volume"), Some(json!(4)));

    // Removed, the file's value is in force again, and the record keeps the change.
    written(&saves, "limits.low_volume", "remove", None);
    assert_eq!(entry(f, "limits.low_volume"), None);
    let (value, scope, ..) = shown(&root, "[limits] low_volume");
    assert_eq!((value.as_str(), scope), ("2", Some(SettingScope::Project)));
    let listed = page(&root).stored;
    let low_volume = listed
        .iter()
        .find(|item| item.key == "limits.low_volume")
        .unwrap();
    assert_eq!((low_volume.value.clone(), low_volume.held), (None, false));
    assert_eq!(low_volume.changes.len(), 2);
    // Nothing of the checkout was written: no commit, no local file.
    assert_eq!(git(&root, &["rev-parse", "main"]), tip);
    assert_eq!(git(&root, &["status", "--porcelain"]), "");
    assert!(!root.join("symphony.local.toml").exists());
    drop(saves);
    f.clean_workers();
}

pub(super) fn a_stored_value_made_the_projects_is_one_commit_and_its_entry_goes(f: &Fixture) {
    let text = with_preferences();
    let root = committing(f, &text);
    let tip = git(&root, &["rev-parse", "main"]);
    // The checkout holds work of its own, in the project file and beside it.
    write(
        &root.join("symphony.toml"),
        &format!("# a local note\n{text}"),
    );
    write(&root.join("other"), "changed here\n");
    let saves = ClosingSaves::start(&root);
    written(&saves, "limits.low_volume", "store", Some(json!(4)));
    let said = written(&saves, "limits.low_volume", "commit", None);
    let commit = said["commit"].as_str().unwrap().to_string();
    assert_eq!(git(&root, &["rev-parse", "main"]), commit);
    assert!(
        said["said"]
            .as_str()
            .unwrap()
            .starts_with(&format!("committed to main as {}", &commit[..12])),
        "{said}"
    );
    // One commit on main, the project file alone, its message naming the key, old and new; the
    // file's comments and order kept, its one value changed.
    assert_eq!(
        git(&root, &["rev-list", "--count", &format!("{tip}..main")]),
        "1"
    );
    assert_eq!(
        git(&root, &["log", "-1", "--format=%B", "main"]),
        "settings: [limits] low_volume from 2 to 4"
    );
    assert_eq!(
        git(&root, &["diff", "--name-only", &tip, "main"]),
        "symphony.toml"
    );
    let committed = text.replace("low_volume = 2", "low_volume = 4");
    assert_eq!(f.main("symphony.toml"), committed);
    // The checkout took the commit with its own edits kept, and committed none of them.
    assert_eq!(
        std::fs::read_to_string(root.join("symphony.toml")).unwrap(),
        format!("# a local note\n{committed}")
    );
    assert_eq!(
        std::fs::read_to_string(root.join("other")).unwrap(),
        "changed here\n"
    );
    assert_eq!(
        git(&root, &["status", "--porcelain"]),
        " M other\n M symphony.toml"
    );
    // The entry is gone, and the file's value is in force.
    assert_eq!(entry(f, "limits.low_volume"), None);
    let (value, scope, ..) = shown(&root, "[limits] low_volume");
    assert_eq!((value.as_str(), scope), ("4", Some(SettingScope::Project)));
    // Nothing is left to commit.
    let (status, why) = refused(&saves, json!({"key":"limits.low_volume","action":"commit"}));
    assert_eq!(status, 409);
    assert_eq!(
        why,
        "[limits] low_volume has no entry in the board's store: nothing to make the project's"
    );
    // A value main writes already is made the project's by removing its entry alone.
    written(&saves, "limits.low_volume", "store", Some(json!(4)));
    let same = written(&saves, "limits.low_volume", "commit", None);
    assert_eq!(
        same["said"],
        "symphony.toml on main writes [limits] low_volume = 4 already: the board's entry is \
         removed, and the file's value is in force"
    );
    assert_eq!(git(&root, &["rev-parse", "main"]), commit);
    assert_eq!(entry(f, "limits.low_volume"), None);
    // A key the file does not write yet goes under its table, the rest kept.
    written(
        &saves,
        "agents.models.reviewer",
        "store",
        Some(json!("opus")),
    );
    written(&saves, "agents.models.reviewer", "commit", None);
    assert_eq!(
        git(&root, &["log", "-1", "--format=%s", "main"]),
        "settings: [agents.models] reviewer from none written to \"opus\""
    );
    assert_eq!(
        f.main("symphony.toml"),
        committed.replace("[agents]\n", "[agents]\nmodels.reviewer = \"opus\"\n")
    );
    assert_eq!(entry(f, "agents.models.reviewer"), None);
    // A checkout whose own edit meets the line committed keeps its file as it is, and says so;
    // main takes the setting alone.
    let mine = std::fs::read_to_string(root.join("symphony.toml"))
        .unwrap()
        .replace("panes = \"none\"", "panes = \"none\" # mine");
    write(&root.join("symphony.toml"), &mine);
    written(&saves, "agents.panes", "store", Some(json!("hc")));
    let met = written(&saves, "agents.panes", "commit", None);
    assert!(
        met["said"]
            .as_str()
            .unwrap()
            .contains("has uncommitted edits the verdict does not merge into"),
        "{met}"
    );
    let on_main = f.main("symphony.toml");
    assert!(on_main.contains("panes = \"hc\"\n"), "{on_main}");
    assert!(
        !on_main.contains("mine") && !on_main.contains("a local note"),
        "{on_main}"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("symphony.toml")).unwrap(),
        mine
    );
    assert_eq!(entry(f, "agents.panes"), None);
    drop(saves);
    f.clean_workers();
}

pub(super) fn a_red_settings_check_commits_nothing_and_keeps_the_entry(f: &Fixture) {
    // The project file main holds names one entrypoint twice, which the settings check refuses.
    let text = format!(
        "{}\n[[entrypoints]]\nurl = \"/docs/\"\nfolder = \"docs\"\n\n[[entrypoints]]\nurl = \"/docs/\"\nfolder = \"other-docs\"\n",
        with_preferences()
    );
    let root = committing(f, &text);
    std::fs::create_dir_all(root.join("other-docs")).unwrap();
    let tip = git(&root, &["rev-parse", "main"]);
    let saves = ClosingSaves::start(&root);
    written(&saves, "limits.low_volume", "store", Some(json!(4)));
    let (status, why) = refused(&saves, json!({"key":"limits.low_volume","action":"commit"}));
    assert_eq!(status, 422, "{why}");
    let low = line_of(&text, "low_volume = 2");
    assert!(
        why.starts_with(&format!(
            "not committed: the settings check refuses symphony.toml as it would be, with \
             [limits] low_volume = 4 on line {low}: "
        )),
        "{why}"
    );
    assert!(why.contains("is named twice"), "{why}");
    assert!(
        why.ends_with("nothing was written, and the stored value holds"),
        "{why}"
    );
    // Nothing committed, nothing of the checkout written, and the entry still in force.
    assert_eq!(git(&root, &["rev-parse", "main"]), tip);
    assert_eq!(
        git(&root, &["status", "--porcelain", "--untracked-files=no"]),
        ""
    );
    assert_eq!(entry(f, "limits.low_volume"), Some(json!(4)));
    drop(saves);
    f.clean_workers();
}

pub(super) fn under_delivery_a_commit_waits_on_its_branch_until_its_delivery_is_proved(
    f: &Fixture,
) {
    let root = super::delivery::delivering(f);
    let tip = git(&root, &["rev-parse", "main"]);
    let saves = ClosingSaves::start(&root);
    written(&saves, "limits.low_volume", "store", Some(json!(4)));
    let said = written(&saves, "limits.low_volume", "commit", None);
    assert_eq!(said["waiting"], true, "{said}");
    assert!(said.get("commit").is_none(), "{said}");
    assert!(
        said["said"]
            .as_str()
            .unwrap()
            .starts_with("[limits] low_volume = 4 is kept for delivery by pull request"),
        "{said}"
    );
    drop(saves);
    // Main never moved, and the checkout was never written.
    assert_eq!(git(&root, &["rev-parse", "main"]), tip);
    assert_eq!(git(&root, &["status", "--porcelain"]), "");
    // A branch of its own holds the one commit, which changes the project file alone.
    let waiting = pending::saves(&root);
    assert_eq!(waiting.len(), 1, "{waiting:?}");
    let branch = waiting[0].branch.clone();
    assert_eq!(waiting[0].state, State::Waiting);
    assert_eq!(waiting[0].file, "symphony.toml");
    assert_eq!(waiting[0].keys, ["limits.low_volume"]);
    assert_eq!(git(&root, &["rev-parse", &format!("{branch}^")]), tip);
    assert_eq!(
        git(&root, &["diff", "--name-only", "main", &branch]),
        "symphony.toml"
    );
    assert_eq!(
        git(&root, &["log", "-1", "--format=%s", &branch]),
        "settings: [limits] low_volume from none written to 4"
    );
    let delivered = git(&root, &["show", &format!("{branch}:symphony.toml")]);
    assert!(
        delivered.starts_with("limits.low_volume = 4\n[land]"),
        "{delivered}"
    );
    // Filed as the owner's task, worded for the change.
    let store = f.store();
    let task = store.get(&branch).unwrap();
    assert_eq!(
        task.title,
        "Deliver the owner's settings change: [limits] low_volume from none written to 4"
    );
    assert_eq!(task.status, "review");
    // The entry stays in force meanwhile, and the page says it waits; a second commit is refused.
    assert_eq!(entry(f, "limits.low_volume"), Some(json!(4)));
    let (value, scope, ..) = shown(&root, "[limits] low_volume");
    assert_eq!((value.as_str(), scope), ("4", Some(SettingScope::Board)));
    let listed = page(&root).stored;
    let low_volume = listed
        .iter()
        .find(|item| item.key == "limits.low_volume")
        .unwrap();
    assert_eq!(
        low_volume.waiting.as_deref(),
        Some(format!("{branch}: waiting for its pull request").as_str())
    );
    let saves = ClosingSaves::start(&root);
    let (status, why) = refused(&saves, json!({"key":"limits.low_volume","action":"commit"}));
    assert_eq!(status, 409);
    assert!(why.contains("waits for its delivery already"), "{why}");
    drop(saves);
    assert_eq!(pending::saves(&root).len(), 1);

    // Delivered on the remote, and followed here: proved, and the entry goes.
    super::delivery::landed(&root, &branch, &branch);
    git(&root, &["merge", "-q", "--ff-only", "origin/main"]);
    super::delivery::pass(&root);
    assert!(pending::saves(&root).is_empty());
    assert_eq!(entry(f, "limits.low_volume"), None);
    let (value, scope, ..) = shown(&root, "[limits] low_volume");
    assert_eq!((value.as_str(), scope), ("4", Some(SettingScope::Project)));
    f.clean_workers();
}
