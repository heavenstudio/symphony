//! `project add`: registered once, again, and as the running server's host rule says.

use super::*;
use crate::test_layouts::{Others, others, repository};
use crate::test_tree::Tree;

#[path = "readiness_tests.rs"]
mod readiness;

/// [`super::running`], the server given a look only a stall outlasts.
fn running(machine: &Path, older: Option<&Path>) -> Option<symphony_api::Front> {
    running_within(machine, older, test_time::backstop(LOOK))
}

/// [`super::add`], its board probe bounded only by the test's stall backstop.
fn add(
    dir: &Path,
    from: &Path,
    machine: &Path,
    front: Option<&symphony_api::Front>,
) -> Result<String, String> {
    add_within(dir, from, machine, front, test_time::backstop(LOOK))
}

fn project(tree: &Tree, id: &str, plugins: &str) -> PathBuf {
    tree.write(&format!("{id}/docs/README.md"), "# Docs\n");
    tree.write(
        &format!("{id}/symphony.toml"),
        &format!("[project]\nid = \"{id}\"\nname = \"{id} works\"\n{plugins}"),
    );
    tree.path(id)
}

fn front(plugins: &[&str], pin: u32, schema: i64) -> symphony_api::Front {
    symphony_api::Front {
        pid: 7,
        port: 1,
        root: "/work/host".into(),
        ready: true,
        app: None,
        tailnet: None,
        orphans: 0,
        plugins: plugins.iter().map(|name| name.to_string()).collect(),
        pin,
        schema,
        projects: Vec::new(),
        release: None,
    }
}

#[test]
fn a_project_is_registered_once_and_said_again_as_the_running_server_serves_it() {
    let tree = Tree::new();
    let root = project(&tree, "lantern", "");
    let machine = tree.path("machine");
    let first = super::add(&root, &tree.root, &machine, None).unwrap();
    assert!(
        first.starts_with("· lantern works registered at /lantern/ ("),
        "{first}"
    );
    assert!(
        first.ends_with("; `just symphony` starts the Symphony"),
        "{first}"
    );
    let peer = readiness::Peer::new(symphony_board::SCHEMA, 200, r#"{"tasks":[],"next":null}"#);
    let again = add(&root, &tree.root, &machine, Some(&peer.front)).unwrap();
    assert!(
        again.contains("already registered") && again.ends_with("the running Symphony serves it"),
        "{again}"
    );
    let old = add(&root, &tree.root, &machine, Some(&front(&[], 0, 0))).unwrap();
    assert!(old.contains("predates the registry"), "{old}");
    let registry = Registry::read(&machine).unwrap();
    assert_eq!(registry.projects.len(), 1);
    assert_eq!(
        registry.projects[0].manifest,
        Some(registry.projects[0].root.join("symphony.toml"))
    );
}

#[test]
fn a_renamed_project_is_registered_again_and_one_whose_folder_moved_says_where_from() {
    let tree = Tree::new();
    let first = project(&tree, "lantern", "");
    let machine = tree.path("machine");
    add(&first, &tree.root, &machine, None).unwrap();
    tree.write(
        "lantern/symphony.toml",
        "[project]\nid = \"lantern\"\nname = \"Lantern Lit\"\n",
    );
    let again = add(&first, &tree.root, &machine, None).unwrap();
    assert!(
        again.starts_with("· Lantern Lit registered again at /lantern/ ("),
        "{again}"
    );
    let left = Registry::read(&machine).unwrap().projects[0].root.clone();
    std::fs::rename(&first, tree.path("lamp")).unwrap();
    let moved = add(&tree.path("lamp"), &tree.root, &machine, None).unwrap();
    assert!(
        moved.starts_with(&format!(
            "· Lantern Lit moved from {} at /lantern/ (",
            left.display()
        )),
        "{moved}"
    );
    let registry = Registry::read(&machine).unwrap();
    assert_eq!(registry.projects.len(), 1);
    assert_eq!(registry.projects[0].root, left.with_file_name("lamp"));
}

#[test]
fn a_running_host_lacking_its_plugins_is_said_and_a_newer_schema_refused_unregistered() {
    let tree = Tree::new();
    let shelved = project(
        &tree,
        "shelved",
        "[plugins]\ncrates = [\"fixture-absent\"]\n",
    );
    let machine = tree.path("machine");
    let host = front(&[], crate::machine::PIN, symphony_board::SCHEMA);
    let said = add(&shelved, &tree.root, &machine, Some(&host)).unwrap();
    assert!(
        said.contains(
            "· the running Symphony lacks the plugin fixture-absent, which shelved names"
        ),
        "{said}"
    );
    let plain = project(&tree, "plain", "");
    let older = front(
        &["fixture-absent"],
        crate::machine::PIN,
        symphony_board::SCHEMA - 1,
    );
    let refused = add(&plain, &tree.root, &machine, Some(&older)).unwrap_err();
    assert!(refused.contains("reads board schema"), "{refused}");
    assert!(
        Registry::read(&machine).unwrap().get("plain").is_none(),
        "a refused project is not registered"
    );
    assert!(Registry::read(&machine).unwrap().get("shelved").is_some());
}

#[test]
fn a_linked_worktree_registers_no_project_of_its_own_repository_and_names_the_main_checkout() {
    let tree = Tree::new();
    let (main, linked) = repository(&tree.root);
    let machine = tree.path("machine");
    let host = front(&[], crate::machine::PIN, symphony_board::SCHEMA);
    let refusal = format!(
        "this runs in the linked worktree {}, and a linked worktree registers no project of its \
         own repository with the machine's Symphony: run `project add` in {}, the main checkout",
        linked.display(),
        main.display()
    );
    // Whichever folder of its repository it names and whatever runs, it registers none.
    for from in [linked.clone(), linked.join("docs")] {
        for dir in [linked.clone(), main.clone(), main.join("docs")] {
            for running in [None, Some(&host)] {
                assert_eq!(
                    add(&dir, &from, &machine, running),
                    Err(refusal.clone()),
                    "{} from {}",
                    dir.display(),
                    from.display()
                );
            }
        }
    }
    assert!(
        !machine.join(crate::machine::PROJECTS).exists(),
        "a linked worktree wrote the registry for its own repository"
    );
    // The main checkout registers the project its worktree names, at its main checkout.
    let added = add(&linked, &main, &machine, Some(&host)).unwrap();
    assert!(
        added.starts_with(&format!(
            "· Lantern registered at /lantern/ ({})",
            main.display()
        )),
        "{added}"
    );
    assert_eq!(Registry::read(&machine).unwrap().projects[0].root, main);
}

#[test]
fn a_linked_worktree_registers_another_repositorys_project_and_a_submodule_its_own() {
    let tree = Tree::new();
    let (_, linked) = repository(&tree.root);
    let Others {
        submodule,
        separate,
        bare,
        bare_worktree,
    } = others(&tree.root);
    let plain = project(&tree, "plain", "");
    let machine = tree.path("machine");
    // From a linked worktree, a project of another repository, or of none, registers as it would
    // from anywhere: what a scratch repository's `project add` run in a branch's checkout does.
    for (dir, said) in [
        (&submodule, "lib registered at /lib/"),
        (&separate, "sep registered at /sep/"),
        (&plain, "plain works registered at /plain/"),
    ] {
        let added = add(dir, &linked.join("docs"), &machine, None).unwrap();
        assert!(
            added.starts_with(&format!("· {said} (")),
            "{} from the worktree: {added}",
            dir.display()
        );
    }
    // A submodule and a `--separate-git-dir` checkout are main checkouts, whose `.git` is a file:
    // each registers its own project, from itself, at itself.
    for (dir, id) in [(&submodule, "lib"), (&separate, "sep")] {
        let added = add(dir, dir, &machine, None).unwrap();
        assert!(
            added.starts_with(&format!("· {id} already registered at /{id}/ (")),
            "{added}"
        );
        let registry = Registry::read(&machine).unwrap();
        assert_eq!(&registry.get(id).unwrap().root, dir);
    }
    // A bare repository's worktree registers none of its own, and names no checkout to run in.
    assert_eq!(
        add(&bare_worktree, &bare_worktree, &machine, None),
        Err(format!(
            "this runs in the linked worktree {}, and a linked worktree registers no project of \
             its own repository with the machine's Symphony: its repository's git dir {} names \
             no main checkout: run `project add` in that repository's main checkout, if it has one",
            bare_worktree.display(),
            bare.display()
        ))
    );
    assert_eq!(Registry::read(&machine).unwrap().projects.len(), 3);
}

#[test]
fn a_folder_outside_a_repository_is_its_own_main_checkout_and_a_bad_line_is_refused() {
    let tree = Tree::new();
    assert_eq!(main_checkout(&tree.root), tree.root);
    for (args, problem) in [
        (
            &["remove"][..],
            "argument command: invalid choice: \"remove\" (choose from 'add')",
        ),
        (
            &["ad"],
            "argument command: invalid choice: \"ad\" (choose from 'add'); did you mean 'add'?",
        ),
        (&[], "the following arguments are required: command"),
        (&["add", "--all"], "unrecognized arguments: --all"),
        (&["add", "here", "there"], "unrecognized arguments: there"),
    ] {
        let args: Vec<String> = args.iter().map(ToString::to_string).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(main(&args, &mut out, &mut err), 2, "{args:?}");
        assert_eq!(
            String::from_utf8(err).unwrap(),
            format!("{USAGE}\nsymphony project: error: {problem}\n"),
            "{args:?}"
        );
        assert!(out.is_empty(), "{args:?}");
    }
}

#[test]
fn the_running_server_is_read_from_its_record_and_its_front() {
    use std::io::BufRead;
    let tree = Tree::new();
    let machine = tree.path("machine");
    let older = tree.path("common/symphony/server");
    assert_eq!(
        running(&machine, Some(&older)),
        None,
        "no record, no server"
    );
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let record = format!("{{\"port\":{port},\"pid\":7,\"root\":\"/r\"}}");
    // A server from before the machine folder recorded itself in the project's install folder.
    tree.write("common/symphony/server/server.json", &record);
    let said = serde_json::to_string(&front(&["shelf"], 0, 0)).unwrap();
    let answering = std::thread::spawn(move || {
        for status in ["200 OK", "200 OK", "403 Forbidden"] {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap() > 2 {
                line.clear();
            }
            write!(
                stream,
                "HTTP/1.1 {status}\r\nConnection: close\r\n\r\n{said}"
            )
            .unwrap();
        }
    });
    assert_eq!(running(&machine, None), None, "the older record unasked");
    let old = running(&machine, Some(&older)).unwrap();
    assert_eq!(old.pin, 0, "{old:?}");
    let plain = project(&tree, "plain", "");
    let added = add(&plain, &tree.root, &machine, Some(&old)).unwrap();
    assert!(added.contains("predates the registry"), "{added}");
    // The machine folder's record is read before the older one.
    tree.write("machine/server.json", &record);
    std::fs::remove_file(older.join("server.json")).unwrap();
    assert_eq!(running(&machine, Some(&older)).unwrap().plugins, ["shelf"]);
    assert_eq!(running(&machine, None), None, "a refusal is no server");
    answering.join().unwrap();
}

#[test]
fn a_projects_older_record_is_in_its_repositorys_install_folder() {
    let tree = Tree::new();
    tree.write("checkout/.git/HEAD", "ref: refs/heads/main\n");
    tree.write("checkout/docs/README.md", "# Docs\n");
    let common = tree.path("checkout/.git").canonicalize().unwrap();
    assert_eq!(
        older_record_folder(&tree.path("checkout/docs")),
        Some(common.join(INSTALL))
    );
}

#[test]
fn what_the_running_server_writes_is_its_project_lists_word_and_nothing_where_it_says_none() {
    use std::io::BufRead;
    let tree = Tree::new();
    let machine = tree.path("machine");
    assert_eq!(writes(&machine), None, "no record, no server");
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    tree.write("machine/server.json", &format!("{{\"port\":{port}}}"));
    let lists = [
        (
            "200 OK",
            r#"{"projects": [{"id": "alpha", "writes": true}, {"id": "beta", "writes": false},
               {"id": "gamma", "writes": null}, {"id": "delta"}, {"name": "no id"}]}"#,
        ),
        ("200 OK", r#"{"projects": "not a list"}"#),
        ("200 OK", "not json"),
        ("404 Not Found", "{}"),
    ];
    let answering = std::thread::spawn(move || {
        for (status, body) in lists {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            assert!(line.starts_with("GET /api/projects "), "{line}");
            while reader.read_line(&mut line).unwrap() > 2 {
                line.clear();
            }
            write!(
                stream,
                "HTTP/1.1 {status}\r\nConnection: close\r\n\r\n{body}"
            )
            .unwrap();
        }
    });
    assert_eq!(
        writes(&machine).unwrap(),
        [
            ("alpha".to_string(), Some(true)),
            ("beta".to_string(), Some(false)),
            ("gamma".to_string(), None),
        ],
        "one it cannot say of is None; one listed with no word of writes (an older server's), \
         or with no id, is left out"
    );
    for _ in 0..3 {
        assert_eq!(
            writes(&machine),
            None,
            "an answer that is no project list is none"
        );
    }
    answering.join().unwrap();
}
