//! A Settings page describer's seam: declared by its id, asked on the wire as
//! `describers.describe` since 0.5 with the project's root, and answering its entry, its error, or
//! a refusal naming its panic or an id the plugin lacks. A plugin written before the seam declares
//! none and is asked for none.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::protocol::tests::entry;
use crate::protocol::{Ask, Declaration, Describe, Request};
use crate::testing::Host;
use crate::vocabulary::SettingsEntry;
use crate::{Describer, Plugin};

/// The root the lamp describer was last asked of.
static ASKED: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Describes its entry, keeping the root it was asked of.
struct Lamp;
/// Cannot say, naming why.
struct Unread;
/// Panics when asked.
struct Broken;

impl Describer for Lamp {
    fn id(&self) -> &str {
        "lamp"
    }
    fn describe(&self, root: &Path) -> Result<SettingsEntry, String> {
        *ASKED.lock().unwrap() = Some(root.to_path_buf());
        Ok(entry())
    }
}

impl Describer for Unread {
    fn id(&self) -> &str {
        "shelf"
    }
    fn describe(&self, _: &Path) -> Result<SettingsEntry, String> {
        Err("shelf.toml does not read".into())
    }
}

impl Describer for Broken {
    fn id(&self) -> &str {
        "broken"
    }
    fn describe(&self, _: &Path) -> Result<SettingsEntry, String> {
        panic!("the lamp's file is gone")
    }
}

struct Describing;

impl Plugin for Describing {
    fn name(&self) -> &str {
        "describing"
    }
    fn describers(&self) -> Vec<&dyn Describer> {
        vec![&Lamp, &Unread, &Broken]
    }
}

/// A plugin as one written before the seam: its name alone, so every seam is the default.
struct Before;

impl Plugin for Before {
    fn name(&self) -> &str {
        "before"
    }
}

fn describe(id: &str) -> Describe {
    Describe::new(id.into(), PathBuf::from("/project"))
}

#[test]
fn a_plugin_declares_each_describer_by_its_id_in_its_order_and_one_written_before_declares_none() {
    assert_eq!(
        Declaration::of(&Describing).describers,
        ["lamp", "shelf", "broken"]
    );
    assert!(Before.describers().is_empty());
    assert!(Declaration::of(&Before).describers.is_empty());
}

#[test]
fn a_declaration_of_0_4_names_no_describers_and_reads_as_none() {
    let mut written = serde_json::to_value(Declaration::of(&Describing)).unwrap();
    written.as_object_mut().unwrap().remove("describers");
    let read: Declaration = serde_json::from_value(written).unwrap();
    assert!(read.describers.is_empty());
}

#[test]
fn describe_is_0_5s_call_and_goes_on_the_wire_as_its_seam_and_call() {
    assert_eq!((Describe::TAG, Describe::SINCE), ("describers.describe", 5));
    let request: Request = describe("lamp").into();
    let written = serde_json::to_value(&request).unwrap();
    let fields = &written["describers.describe"];
    assert_eq!(fields["describer"], "lamp");
    assert_eq!(fields["root"], "/project");
    let read: Request = serde_json::from_value(written).unwrap();
    assert_eq!(read, request);
}

#[test]
fn the_describer_of_the_id_named_is_asked_for_the_root_and_answers_its_entry() {
    let answered = Host::new(&Describing).ask(describe("lamp"));
    assert_eq!(answered, Ok(Ok(entry())));
    assert_eq!(*ASKED.lock().unwrap(), Some(PathBuf::from("/project")));
}

#[test]
fn an_entry_goes_on_the_wire_in_the_pages_own_words() {
    let written = serde_json::to_value(entry()).unwrap();
    let setting = &written["settings"][0];
    assert_eq!(setting["kind"], "choice");
    assert_eq!(setting["class"], "preference");
    assert_eq!(setting["scopes"], serde_json::json!(["project", "machine"]));
    assert_eq!(setting["source"]["scope"], "project");
    // What an entry leaves out is not written, and reads back as none.
    assert!(written.get("source").is_none());
    let bare = serde_json::json!({"id": "a", "name": "A", "about": "B"});
    let read: SettingsEntry = serde_json::from_value(bare).unwrap();
    assert_eq!(
        (read.state, read.needs, read.settings),
        (None, vec![], vec![])
    );
}

#[test]
fn a_describer_that_cannot_say_answers_why_as_its_error() {
    assert_eq!(
        Host::new(&Describing).ask(describe("shelf")),
        Ok(Err("shelf.toml does not read".into()))
    );
}

#[test]
fn a_panic_or_an_id_the_plugin_lacks_is_refused_naming_it() {
    let host = Host::new(&Describing);
    let panicked = host.ask(describe("broken")).unwrap_err();
    assert!(
        panicked.contains("describing panicked: the lamp's file is gone"),
        "{panicked}"
    );
    let missing = host.ask(describe("candle")).unwrap_err();
    assert!(
        missing.contains("the plugin has no describer \"candle\""),
        "{missing}"
    );
    let none = Host::new(&Before).ask(describe("lamp")).unwrap_err();
    assert!(
        none.contains("the plugin has no describer \"lamp\""),
        "{none}"
    );
}
