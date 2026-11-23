//! The Settings page's entries a project's plugins describe (`symphony::settings::page`, the
//! describers seam): each describer of each plugin the project selects, linked or from its
//! process alike, after the Symphony's own in the integrations, its class judged so that a
//! protected key it calls a preference is shown reviewed; a plugin the project does not select
//! adds none, and a describer that cannot say is shown in its entry's place.

use symphony::settings::{Machine, page};
use symphony_api::{SettingClass, SettingScope, SettingsEntry, SettingsPage};

use super::Tree;
use super::compiled::naming;
use super::every::{SHADE, described};

/// The test plugin's binary.
const PLUGIN: &str = env!("CARGO_BIN_EXE_symphony-test-plugin");

/// The integrations section's entries, by id.
fn integrations(page: &SettingsPage) -> Vec<&SettingsEntry> {
    let section = (page.sections.iter())
        .find(|section| section.id == "integrations")
        .expect("an integrations section");
    section.entries.iter().collect()
}

fn ids(page: &SettingsPage) -> Vec<&str> {
    integrations(page).iter().map(|e| e.id.as_str()).collect()
}

fn entry<'p>(page: &'p SettingsPage, id: &str) -> &'p SettingsEntry {
    (integrations(page).into_iter())
        .find(|entry| entry.id == id)
        .unwrap_or_else(|| panic!("no entry {id} in {:?}", ids(page)))
}

/// The page of `tree` on a machine with nothing on its search path.
fn read(tree: &Tree) -> SettingsPage {
    page(&tree.root, &Machine::path(None))
}

/// What the page shows of the plugin `every`'s two describers, and that it is as the describer
/// answered but for its id and the class of the key it may not call a preference.
fn shown_as_described(page: &SettingsPage, tree: &Tree) {
    assert_eq!(
        ids(page),
        [
            "claude",
            "codex",
            "herdr",
            "github",
            "plugins",
            "plugin-every-widgets",
            "plugin-every-unread"
        ],
        "the Symphony's own first, then each describer in the plugin's order"
    );
    let widgets = entry(page, "plugin-every-widgets");
    let answered = described(&tree.root);
    assert_eq!(widgets.name, answered.name);
    assert_eq!(widgets.state, answered.state);
    assert_eq!(widgets.needs, answered.needs);
    assert_eq!(
        widgets.state.as_ref().unwrap().why,
        format!("Its shade is bright, read in {}.", tree.root.display())
    );
    let [shade, delivery] = widgets.settings.as_slice() else {
        panic!("two settings: {:?}", widgets.settings);
    };
    assert_eq!(*shade, answered.settings[0], "its own key, kept as it says");
    assert_eq!(shade.class, SettingClass::Preference);
    assert_eq!(shade.scopes, [SettingScope::Project, SettingScope::Machine]);
    // The land's delivery decides what lands: reviewed, whatever the plugin calls it, and so
    // written in the project's scope alone.
    assert_eq!(delivery.key, "[land] delivery");
    assert_eq!(delivery.class, SettingClass::Reviewed);
    assert_eq!(delivery.scopes, [SettingScope::Project]);
    assert_eq!(
        (&delivery.value, &delivery.source),
        (&answered.settings[1].value, &answered.settings[1].source)
    );
    let unread = entry(page, "plugin-every-unread");
    let state = unread.state.as_ref().unwrap();
    assert_eq!((state.word.as_str(), state.on), ("Unknown", None));
    assert!(
        state
            .why
            .starts_with("Unknown: the plugin every could not say: "),
        "{state:?}"
    );
    assert!(state.why.contains("every.toml does not read"), "{state:?}");
    assert!(unread.settings.is_empty());
}

#[test]
fn a_linked_plugins_describers_add_its_entries_after_the_symphonys_own() {
    let tree = naming(&["every"]);
    tree.write(SHADE, "bright\n");
    shown_as_described(&read(&tree), &tree);
}

#[test]
fn a_plugin_answering_from_its_process_adds_the_same_entries() {
    let tree = Tree::new();
    tree.write(
        symphony::project::FILE,
        &format!("[plugins]\ncrates = [\"every\"]\n\n[plugins.process]\nevery = [{PLUGIN:?}]\n"),
    );
    tree.write(SHADE, "bright\n");
    let page = read(&tree);
    shown_as_described(&page, &tree);
    // The error of the one that cannot say is its process's answer, said as the linked one's.
    let linked = naming(&["every"]);
    linked.write(SHADE, "bright\n");
    let linked = read(&linked);
    assert_eq!(
        entry(&page, "plugin-every-unread").state,
        entry(&linked, "plugin-every-unread").state
    );
}

#[test]
fn a_plugin_the_project_does_not_select_adds_no_entry() {
    // The process holds `every`, compiled in; the project names others.
    let tree = naming(&["first", "bare"]);
    let page = read(&tree);
    assert_eq!(
        ids(&page),
        ["claude", "codex", "herdr", "github", "plugins"]
    );
    let none = Tree::new();
    assert_eq!(
        ids(&read(&none)),
        ["claude", "codex", "herdr", "github", "plugins"]
    );
}
