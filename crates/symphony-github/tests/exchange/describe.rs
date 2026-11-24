//! `describe`: the exchange's entry on the Settings page, one line of JSON, from the committed
//! settings (each setting with the line that writes it) and what is kept, every setting reviewed.

use crate::memory::Fault;
use crate::{Rig, settings};
use symphony_blocks::{SettingClass, SettingScope, SettingsEntry};
use symphony_github::state::{State, Store};

/// What `describe` printed, read back as the page reads it: as the kit's entry, which writes it
/// again member for member, so the adapter's own spelling of the shape is the kit's.
fn described(rig: &Rig) -> SettingsEntry {
    let ran = rig.run(&["describe"]).ok();
    assert_eq!(ran.out.lines().count(), 1, "one line: {}", ran.out);
    let entry: SettingsEntry = serde_json::from_str(&ran.out).unwrap();
    let printed: serde_json::Value = serde_json::from_str(&ran.out).unwrap();
    assert_eq!(
        serde_json::to_value(&entry).unwrap(),
        printed,
        "a member the kit's entry does not read, or reads otherwise"
    );
    entry
}

/// The entry's setting of `key`.
fn setting<'e>(entry: &'e SettingsEntry, key: &str) -> &'e symphony_blocks::Setting {
    (entry.settings.iter())
        .find(|setting| setting.key == key)
        .expect("the entry shows the setting")
}

#[test]
fn with_no_file_committed_it_is_off_and_every_setting_is_its_default() {
    let rig = Rig::off("describe-off");
    let entry = described(&rig);
    assert_eq!(
        (entry.id.as_str(), entry.name.as_str()),
        ("github", "GitHub")
    );
    let state = entry.state.clone().unwrap();
    assert_eq!((state.word.as_str(), state.on), ("Off", Some(false)));
    // Why it is off, and nothing of what off means (RULES.md `code/pages-explain-nothing`).
    assert_eq!(state.why, "Off: no symphony-github.toml. Nothing is kept.");
    assert!(entry.needs.is_empty(), "{:?}", entry.needs);
    let keys: Vec<&str> = entry.settings.iter().map(|s| s.key.as_str()).collect();
    assert_eq!(
        keys,
        [
            "enabled",
            "repository",
            "issues",
            "pull_requests",
            "releases",
            "verification",
            "token_file"
        ]
    );
    for each in &entry.settings {
        assert_eq!(each.class, SettingClass::Reviewed, "{each:?}");
        assert_eq!(each.scopes, [SettingScope::Project], "{each:?}");
        assert_eq!(each.source.scope, None, "{each:?}");
        assert_eq!(
            each.source.said,
            "default: symphony-github.toml does not write it"
        );
    }
    assert_eq!(setting(&entry, "enabled").value, "off");
    assert_eq!(
        setting(&entry, "issues").choices,
        ["off", "import", "export", "exchange"]
    );
}

#[test]
fn enabled_it_is_on_with_what_it_turns_on_each_setting_at_its_line_and_its_token_file_found() {
    let rig = Rig::new("describe-on", "issues = \"export\"\napp_id = 12");
    let entry = described(&rig);
    let state = entry.state.clone().unwrap();
    assert_eq!((state.word.as_str(), state.on), ("On", Some(true)));
    assert!(
        state.why.starts_with(
            "Enabled by symphony-github.toml for o/n: issues export, pull requests off, \
             releases off. "
        ),
        "{}",
        state.why
    );
    let enabled = setting(&entry, "enabled");
    assert_eq!(enabled.value, "on");
    assert_eq!(enabled.source.scope, Some(SettingScope::Project));
    assert_eq!(enabled.source.said, "symphony-github.toml enabled, line 1");
    assert_eq!(enabled.source.file.as_deref(), Some("symphony-github.toml"));
    assert_eq!(enabled.source.line, Some(1));
    assert_eq!(setting(&entry, "repository").value, "o/n");
    assert_eq!(setting(&entry, "repository").source.line, Some(2));
    assert_eq!(setting(&entry, "token_file").source.line, Some(3));
    assert_eq!(setting(&entry, "issues").value, "export");
    assert_eq!(setting(&entry, "issues").source.line, Some(4));
    assert_eq!(setting(&entry, "pull_requests").source.line, None);
    assert_eq!(setting(&entry, "app_id").value, "12");
    assert_eq!(setting(&entry, "app_id").source.line, Some(5));
    assert_eq!(
        entry.needs.len(),
        1,
        "the token file alone: {:?}",
        entry.needs
    );
    let need = &entry.needs[0];
    assert!(need.met, "{need:?}");
    assert_eq!(
        need.said,
        format!("Found on this machine: {}.", rig.token_file.display())
    );
    // The token is never said, only where it is kept.
    let printed = rig.run(&["describe"]).ok().out;
    assert!(!printed.contains(crate::TOKEN), "{printed}");
}

#[test]
fn a_token_file_this_machine_lacks_is_an_unmet_need() {
    let rig = Rig::off("describe-no-token");
    let gone = rig.dir.0.join("gone");
    rig.settings.set(Some(settings(&gone, "")));
    let entry = described(&rig);
    assert_eq!(entry.state.unwrap().word, "On");
    assert_eq!(
        entry.needs.len(),
        1,
        "the token file alone: {:?}",
        entry.needs
    );
    let need = &entry.needs[0];
    assert!(!need.met);
    assert!(
        need.said
            .starts_with(&format!("Not found on this machine: {};", gone.display())),
        "{}",
        need.said
    );
}

#[test]
fn a_file_that_does_not_enable_it_is_off_and_its_settings_still_shown() {
    let rig = Rig::off("describe-disabled");
    rig.settings
        .set(Some("enabled = false\nrepository = \"o/n\"\n".into()));
    let entry = described(&rig);
    let state = entry.state.clone().unwrap();
    assert_eq!(state.word, "Off");
    assert!(
        state
            .why
            .starts_with("Off: symphony-github.toml does not enable it. "),
        "{}",
        state.why
    );
    assert_eq!(setting(&entry, "enabled").source.line, Some(1));
    assert_eq!(setting(&entry, "repository").value, "o/n");
    assert!(entry.needs.is_empty(), "no token is needed while it is off");
}

#[test]
fn settings_that_do_not_read_are_off_naming_why_and_show_no_setting() {
    let rig = Rig::off("describe-refused");
    rig.settings.set(Some("enabled = true\n".into()));
    let entry = described(&rig);
    let state = entry.state.unwrap();
    assert_eq!((state.word.as_str(), state.on), ("Off", Some(false)));
    assert!(
        state.why.starts_with(
            "Off: its settings do not read: symphony-github.toml: enabled needs repository = \
             \"OWNER/NAME\"."
        ),
        "{}",
        state.why
    );
    assert!(entry.settings.is_empty(), "{:?}", entry.settings);
}

#[test]
fn what_is_kept_is_said_with_a_pause_and_a_rate_limit_still_running() {
    let rig = Rig::new("describe-kept", "");
    let store = Store::open(&rig.dir.0.join("state")).unwrap();
    store
        .save(&State {
            paused: Some("the repository was renamed".into()),
            rate_limited_until: Some(rig.now.get() + 60),
            ..State::default()
        })
        .unwrap();
    let why = described(&rig).state.unwrap().why;
    assert!(
        why.ends_with(&format!(
            "Bound to no repository yet, 0 mapped, 0 curated texts, 0 delivered by pull request. \
             Paused: the repository was renamed. Rate limited until {} (seconds since 1970).",
            rig.now.get() + 60
        )),
        "{why}"
    );
    // A limit that has run out is not said.
    rig.now.set(rig.now.get() + 61);
    let why = described(&rig).state.unwrap().why;
    assert!(!why.contains("Rate limited"), "{why}");
}

#[test]
fn describe_takes_no_argument() {
    let rig = Rig::off("describe-argument");
    let ran = rig.run(&["describe", "--json"]);
    assert_eq!(ran.code, 1);
    assert!(
        ran.err.contains("unexpected argument \"--json\""),
        "{}",
        ran.err
    );
}

#[test]
fn what_is_kept_that_does_not_read_or_cannot_be_found_is_said_in_its_place() {
    let rig = Rig::new("describe-unread", "");
    let folder = rig.dir.0.join("state");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("state.json"), "not json").unwrap();
    let state = described(&rig).state.unwrap();
    assert_eq!(state.word, "On", "what is kept does not turn it off");
    assert!(
        state.why.contains("What it keeps does not read: ") && state.why.contains("state.json"),
        "{}",
        state.why
    );
    rig.board.fault("state_folder", Fault::LoseAnswer);
    let why = described(&rig).state.unwrap().why;
    assert!(
        why.ends_with("What it keeps cannot be read: state_folder: no answer came."),
        "{why}"
    );
}
