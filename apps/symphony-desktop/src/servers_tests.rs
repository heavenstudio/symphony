use std::cell::RefCell;

use super::*;
use crate::testing::{Tree, listing, project};

fn saved(address: &str, installation: &str) -> Saved {
    let server = Server::parse(address).unwrap();
    Saved {
        name: server.default_name(),
        server,
        installation: Some(installation.into()),
    }
}

#[test]
fn the_list_is_written_whole_and_read_back_as_it_was() {
    let tree = Tree::new();
    let folder = tree.path("Symphony Desktop");
    assert_eq!(
        List::read(&folder).unwrap(),
        List::default(),
        "no file yet is an empty list"
    );
    let list = List {
        servers: vec![
            saved("http://studio.tail1234.ts.net:8765", "b"),
            Saved {
                installation: None,
                ..saved("http://100.64.0.9:8765", "c")
            },
        ],
    };
    list.write(&folder).unwrap();
    assert_eq!(List::read(&folder).unwrap(), list);
    assert_eq!(
        tree.names("Symphony Desktop"),
        [LIST],
        "nothing is left beside it"
    );
    let text = tree.read("Symphony Desktop/servers.json");
    assert!(
        text.contains(r#""server": "http://studio.tail1234.ts.net:8765""#)
            && text.contains(r#""name": "studio""#)
            && text.ends_with("}\n"),
        "{text}"
    );
    // Written again, it replaces the old one whole.
    let shorter = List {
        servers: list.servers[..1].to_vec(),
    };
    shorter.write(&folder).unwrap();
    assert_eq!(List::read(&folder).unwrap(), shorter);
}

#[test]
fn a_list_that_does_not_read_is_kept_and_never_written_over() {
    let tree = Tree::new();
    for (text, why) in [
        (
            "{ half a list",
            "does not read as the app's list of servers",
        ),
        (
            r#"{"servers": [{"server": "http://example.com", "name": "x", "installation": null}]}"#,
            "neither this Mac nor on the tailnet",
        ),
        (
            r#"{"servers": 3}"#,
            "does not read as the app's list of servers",
        ),
    ] {
        tree.write("settings/servers.json", text);
        let folder = tree.path("settings");
        let refused = List::read(&folder).unwrap_err();
        assert!(
            refused.contains(why) && refused.contains("it is kept as it is"),
            "{refused}"
        );
        let written = List::default().write(&folder).unwrap_err();
        assert_eq!(written, refused);
        assert_eq!(
            tree.read("settings/servers.json"),
            text,
            "the file is untouched"
        );
        assert_eq!(tree.names("settings"), [LIST]);
    }
}

#[test]
fn a_folder_that_cannot_hold_the_list_says_so() {
    let tree = Tree::new();
    tree.write("taken", "a file where the folder would be");
    let refused = List::default().write(&tree.path("taken")).unwrap_err();
    assert!(refused.contains("cannot be"), "{refused}");
}

#[test]
fn this_macs_server_is_the_port_its_record_names_and_none_without_one() {
    let tree = Tree::new();
    let machine = tree.path("machine");
    assert_eq!(this_mac(&machine), None, "no folder");
    tree.write(
        "machine/server.json",
        "{\"port\":8765,\"pid\":1,\"root\":\"/x\"}",
    );
    assert_eq!(
        this_mac(&machine).map(|server| server.to_string()),
        Some("http://127.0.0.1:8765".into())
    );
    for broken in ["{", "{\"port\":\"x\"}", "{\"pid\":1}", "{\"port\":70000}"] {
        tree.write("machine/server.json", broken);
        assert_eq!(this_mac(&machine), None, "{broken}");
    }
}

#[test]
fn the_places_are_the_named_folders_else_the_platforms_under_home() {
    let vars = |pairs: &'static [(&'static str, &'static str)]| {
        move |name: &str| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        }
    };
    assert_eq!(
        Places::from(vars(&[("HOME", "/Users/someone")])).unwrap(),
        Places {
            settings: "/Users/someone/Library/Application Support/Symphony Desktop".into(),
            machine: Some("/Users/someone/Library/Application Support/Symphony".into()),
        }
    );
    assert_eq!(
        Places::from(vars(&[
            ("HOME", "/Users/someone"),
            (SETTINGS_DIR, "/tmp/app"),
            (MACHINE_DIR, "/tmp/machine"),
        ]))
        .unwrap(),
        Places {
            settings: "/tmp/app".into(),
            machine: Some("/tmp/machine".into()),
        }
    );
    assert_eq!(
        Places::from(vars(&[(SETTINGS_DIR, "/tmp/app"), ("HOME", "")])).unwrap(),
        Places {
            settings: "/tmp/app".into(),
            machine: None
        },
        "an empty value is unset"
    );
    assert_eq!(
        Places::from(vars(&[])).unwrap_err(),
        "neither SYMPHONY_DESKTOP_DIR nor HOME names a folder"
    );
    assert_eq!(
        Places::from(vars(&[("HOME", "/h"), (MACHINE_DIR, "relative")])).unwrap_err(),
        "SYMPHONY_MACHINE_DIR names relative, not an absolute folder"
    );
}

/// A reader answering `answers` by address, and every address it was asked.
struct Reader {
    answers: Vec<(&'static str, Answer)>,
    asked: RefCell<Vec<String>>,
}

impl Reader {
    fn new(answers: Vec<(&'static str, Answer)>) -> Reader {
        Reader {
            answers,
            asked: RefCell::new(Vec::new()),
        }
    }

    fn read(&self, server: &Server) -> Answer {
        self.asked.borrow_mut().push(server.to_string());
        self.answers
            .iter()
            .find(|(address, _)| *address == server.to_string())
            .map_or_else(
                || Answer::Lost(format!("{server} does not answer")),
                |(_, answer)| answer.clone(),
            )
    }
}

fn listed(installation: &str) -> Answer {
    Answer::Listed(listing(Some(installation), vec![project("symphony", 1)]))
}

#[test]
fn a_server_that_answers_as_a_symphony_is_added_by_its_origin_and_named() {
    let tree = Tree::new();
    let settings = tree.path("app");
    let here = Server::parse("http://127.0.0.1:8765").unwrap();
    let reader = Reader::new(vec![
        ("http://127.0.0.1:8765", listed("mine")),
        ("http://studio.tail1234.ts.net:8765", listed("studio")),
    ]);
    let added = add(
        &settings,
        "http://studio.tail1234.ts.net:8765/symphony/tasks",
        Some(&here),
        |server| reader.read(server),
    )
    .unwrap();
    assert_eq!(added, saved("http://studio.tail1234.ts.net:8765", "studio"));
    assert_eq!(List::read(&settings).unwrap().servers, [added]);
}

#[test]
fn a_server_is_refused_before_any_read_when_its_address_is_or_it_is_listed_already() {
    let tree = Tree::new();
    let settings = tree.path("app");
    List {
        servers: vec![saved("http://studio.tail1234.ts.net:8765", "studio")],
    }
    .write(&settings)
    .unwrap();
    let here = Server::parse("http://127.0.0.1:8765").unwrap();
    let reader = Reader::new(vec![]);
    for (pasted, why) in [
        (
            "http://example.com:8765",
            "neither this Mac nor on the tailnet",
        ),
        ("https://studio.tail1234.ts.net", "not https://"),
        (
            "http://studio.tail1234.ts.net:8765/tasks",
            "is listed already, as studio",
        ),
        (
            "http://127.0.0.1:8765/symphony/",
            "is this Mac's: its server is found by itself",
        ),
    ] {
        let refused =
            add(&settings, pasted, Some(&here), |server| reader.read(server)).unwrap_err();
        assert!(refused.contains(why), "{pasted}: {refused}");
    }
    assert_eq!(reader.asked.borrow().len(), 0, "no address was read");
    assert_eq!(List::read(&settings).unwrap().servers.len(), 1);
}

#[test]
fn a_server_that_answers_no_list_or_an_installation_listed_already_is_refused() {
    let tree = Tree::new();
    let settings = tree.path("app");
    List {
        servers: vec![saved("http://studio.tail1234.ts.net:8765", "studio")],
    }
    .write(&settings)
    .unwrap();
    let here = Server::parse("http://127.0.0.1:8765").unwrap();
    let reader = Reader::new(vec![
        ("http://127.0.0.1:8765", listed("mine")),
        ("http://100.101.102.103:8765", listed("studio")),
        ("http://[fd7a:115c:a1e0::5]:8765", listed("mine")),
        ("http://old.tail1234.ts.net:8765", Answer::Older),
    ]);
    for (pasted, why) in [
        (
            "http://100.101.102.103:8765",
            "is studio, listed already at http://studio.tail1234.ts.net:8765: one server is listed once",
        ),
        (
            "http://[fd7a:115c:a1e0::5]:8765",
            "is this Mac's own server, reached by another name",
        ),
        (
            "http://old.tail1234.ts.net:8765",
            "needs a newer Symphony to be added",
        ),
        (
            "http://gone.tail1234.ts.net:8765",
            "does not answer: nothing is added",
        ),
    ] {
        let refused =
            add(&settings, pasted, Some(&here), |server| reader.read(server)).unwrap_err();
        assert!(
            refused.ends_with(why) || refused.contains(why),
            "{pasted}: {refused}"
        );
    }
    assert_eq!(
        List::read(&settings).unwrap().servers.len(),
        1,
        "nothing was added"
    );
    // With this Mac's server not answering, another name of it cannot be told: it is added.
    let alone = Reader::new(vec![("http://[fd7a:115c:a1e0::5]:8765", listed("mine"))]);
    add(
        &settings,
        "http://[fd7a:115c:a1e0::5]:8765",
        Some(&here),
        |server| alone.read(server),
    )
    .unwrap();
    // A server whose installation is unknown is added, as nothing tells it from another.
    let unknown = Reader::new(vec![(
        "http://100.64.0.2:8765",
        Answer::Listed(listing(None, vec![])),
    )]);
    let added = add(&settings, "http://100.64.0.2:8765", None, |server| {
        unknown.read(server)
    })
    .unwrap();
    assert_eq!(
        (added.name.as_str(), added.installation),
        ("100.64.0.2", None)
    );
    assert_eq!(List::read(&settings).unwrap().servers.len(), 3);
}

#[test]
fn nothing_is_added_while_the_list_file_does_not_read() {
    let tree = Tree::new();
    tree.write("app/servers.json", "{ half");
    let reader = Reader::new(vec![("http://100.64.0.2:8765", listed("x"))]);
    let refused = add(
        &tree.path("app"),
        "http://100.64.0.2:8765",
        None,
        |server| reader.read(server),
    )
    .unwrap_err();
    assert!(
        refused.contains("does not read as the app's list"),
        "{refused}"
    );
    assert_eq!(tree.read("app/servers.json"), "{ half");
    assert_eq!(reader.asked.borrow().len(), 0);
}
