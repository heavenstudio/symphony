use super::*;
use crate::servers::{List, MACHINE_DIR, SETTINGS_DIR};
use crate::testing::{Tree, listing};

/// `args` run with the folders of `tree` as the places, every server answering `answer`: what it
/// asks for, and its two streams.
fn run(tree: &Tree, args: &[&str], answer: &Answer) -> (Asked, String, String) {
    let settings = tree.path("app");
    let machine = tree.path("machine");
    let var = |name: &str| match name {
        SETTINGS_DIR => Some(settings.display().to_string()),
        MACHINE_DIR => Some(machine.display().to_string()),
        _ => None,
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let args: Vec<String> = args.iter().map(|&arg| arg.to_owned()).collect();
    let asked = main_reading(&args, var, &mut out, &mut err, |_| answer.clone());
    (
        asked,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn no_words_open_the_app_on_the_places_named() {
    let tree = Tree::new();
    let (asked, out, err) = run(&tree, &[], &Answer::Older);
    assert_eq!(
        asked,
        Asked::Open(Places {
            settings: tree.path("app"),
            machine: Some(tree.path("machine")),
        })
    );
    assert_eq!((out.as_str(), err.as_str()), ("", ""));
}

#[test]
fn add_saves_a_server_that_answers_and_says_where() {
    let tree = Tree::new();
    let answer = Answer::Listed(listing(Some("studio"), vec![]));
    let (asked, out, err) = run(
        &tree,
        &["add", "http://studio.tail1234.ts.net:8765/"],
        &answer,
    );
    assert_eq!((asked, err.as_str()), (Asked::Exit(0), ""));
    assert_eq!(
        out,
        format!(
            "added http://studio.tail1234.ts.net:8765 as studio, in {}\n",
            tree.path("app/servers.json").display()
        )
    );
    assert_eq!(List::read(&tree.path("app")).unwrap().servers.len(), 1);
}

#[test]
fn add_refuses_a_server_this_macs_record_names() {
    let tree = Tree::new();
    tree.write("machine/server.json", "{\"port\":8765}");
    let answer = Answer::Listed(listing(Some("mine"), vec![]));
    let (asked, out, err) = run(&tree, &["add", "http://127.0.0.1:8765"], &answer);
    assert_eq!((asked, out.as_str()), (Asked::Exit(2), ""));
    assert!(
        err.starts_with("✖ symphony-desktop: http://127.0.0.1:8765 is this Mac's"),
        "{err}"
    );
    assert!(!tree.path("app/servers.json").exists());
}

#[test]
fn help_prints_the_usage_and_other_words_are_refused_with_it() {
    let tree = Tree::new();
    let (asked, out, _) = run(&tree, &["--help"], &Answer::Older);
    assert_eq!(asked, Asked::Exit(0));
    assert!(
        out.starts_with("usage: symphony-desktop [add URL]"),
        "{out}"
    );
    for words in [
        &["add"][..],
        &["remove", "x"],
        &["add", "a", "b"],
        &["--port", "0"],
    ] {
        let (asked, out, err) = run(&tree, words, &Answer::Older);
        assert_eq!((asked, out.as_str()), (Asked::Exit(2), ""), "{words:?}");
        assert!(
            err.starts_with("✖ symphony-desktop: usage: symphony-desktop [add URL]")
                && err.contains("it takes no words"),
            "{words:?}: {err}"
        );
    }
}

#[test]
fn add_reads_the_server_itself_and_adds_none_that_does_not_answer() {
    let tree = Tree::new();
    // A port nothing listens on: bound, then let go.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let settings = tree.path("app");
    let var = |name: &str| (name == SETTINGS_DIR).then(|| settings.display().to_string());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let args = ["add".to_owned(), format!("http://127.0.0.1:{port}")];
    let asked = main(&args, var, &mut out, &mut err);
    let err = String::from_utf8(err).unwrap();
    assert_eq!((asked, out.as_slice()), (Asked::Exit(2), &b""[..]));
    assert!(
        err.starts_with("✖ symphony-desktop: ") && err.ends_with(": nothing is added\n"),
        "{err}"
    );
    assert!(!tree.path("app/servers.json").exists());
}

#[test]
fn a_place_that_cannot_be_named_is_refused() {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let asked = main_reading(&[], |_| None, &mut out, &mut err, |_| Answer::Older);
    assert_eq!(asked, Asked::Exit(2));
    assert_eq!(
        String::from_utf8(err).unwrap(),
        "✖ symphony-desktop: neither SYMPHONY_DESKTOP_DIR nor HOME names a folder\n"
    );
}
