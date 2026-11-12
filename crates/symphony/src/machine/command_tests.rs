//! `project add`: registered once, again, and as the running server's host rule says.

use super::*;
use crate::test_tree::Tree;

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
    }
}

#[test]
fn a_project_is_registered_once_and_said_again_as_the_running_server_serves_it() {
    let tree = Tree::new();
    let root = project(&tree, "lantern", "");
    let machine = tree.path("machine");
    let first = add(&root, &machine, None).unwrap();
    assert!(
        first.starts_with("· lantern works registered at /lantern/ ("),
        "{first}"
    );
    assert!(
        first.ends_with("; `just symphony` starts the Symphony"),
        "{first}"
    );
    let host = front(&[], crate::machine::PIN, symphony_board::SCHEMA);
    let again = add(&root, &machine, Some(&host)).unwrap();
    assert!(
        again.contains("already registered") && again.ends_with("the running Symphony serves it"),
        "{again}"
    );
    let old = add(&root, &machine, Some(&front(&[], 0, 0))).unwrap();
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
    add(&first, &machine, None).unwrap();
    tree.write(
        "lantern/symphony.toml",
        "[project]\nid = \"lantern\"\nname = \"Lantern Lit\"\n",
    );
    let again = add(&first, &machine, None).unwrap();
    assert!(
        again.starts_with("· Lantern Lit registered again at /lantern/ ("),
        "{again}"
    );
    let left = Registry::read(&machine).unwrap().projects[0].root.clone();
    std::fs::rename(&first, tree.path("lamp")).unwrap();
    let moved = add(&tree.path("lamp"), &machine, None).unwrap();
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
    let said = add(&shelved, &machine, Some(&host)).unwrap();
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
    let refused = add(&plain, &machine, Some(&older)).unwrap_err();
    assert!(refused.contains("reads board schema"), "{refused}");
    assert!(
        Registry::read(&machine).unwrap().get("plain").is_none(),
        "a refused project is not registered"
    );
    assert!(Registry::read(&machine).unwrap().get("shelved").is_some());
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
    let older = tree.path("common/sf-symphony");
    assert_eq!(
        running(&machine, Some(&older)),
        None,
        "no record, no server"
    );
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let record = format!("{{\"port\":{port},\"pid\":7,\"root\":\"/r\"}}");
    // A server from before the machine folder recorded itself in the project's install folder.
    tree.write("common/sf-symphony/server.json", &record);
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
    let added = add(&plain, &machine, Some(&old)).unwrap();
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
