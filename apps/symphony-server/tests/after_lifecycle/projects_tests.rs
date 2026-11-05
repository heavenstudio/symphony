//! Two projects on the one server, started in either order: the fixture's main checkout, a plain
//! project with a theme of its own in its file, from the plain build; and a project naming the
//! shelf plugin and its theme package, from the build holding it (`symphony-shelf`).

use super::fixture::{APP, PATIENCE, Process};
use super::*;
use std::path::PathBuf;

/// The build holding the shelf fixture plugin.
const SHELF: &str = env!("CARGO_BIN_EXE_symphony-shelf");
/// The client each install folder holds: a head for the theme, a body for the project.
const SHELL: &str = "<!doctype html><html><head><title>Symphony</title>\
                     <link rel=\"icon\" href=\"data:,\"></head><body>projects client</body></html>\n";
const ALPHA: &str = "[project]\nname = \"Alpha\"\nid = \"alpha\"\n\
                     [theme.tokens]\nprimary = \"#7a2e8f\"\n[theme.layouts]\nhome = \"focus\"\n";
const SHELF_PROJECT: &str = "[project]\nname = \"Shelf\"\nid = \"pantry\"\n\
                             [plugins]\ncrates = [\"fixture-shelf\"]\n\
                             [theme]\npackage = \"fixture-shelf\"\n";

/// The fixture's main checkout as the project `alpha`, and a second repository as `pantry`, each
/// install folder holding [`SHELL`]: the shelf project's root.
fn projects(repo: &Repo) -> PathBuf {
    write(&repo.main.join("symphony.toml"), ALPHA);
    write(&repo.install.join("client/public/index.html"), SHELL);
    let shelf = repo.external("shelf project");
    write(&shelf.join("symphony.toml"), SHELF_PROJECT);
    write(
        &shelf.join(".git/sf-symphony/client/public/index.html"),
        SHELL,
    );
    shelf
}

/// A start of the one server from `dir` by `program`, in the fixture's environment.
fn start_with(repo: &Repo, program: &str, dir: &Path) -> Process {
    let mut command = Command::new(program);
    command
        .current_dir(dir)
        .env_clear()
        .envs(repo.environment())
        .stdin(Stdio::null());
    repo.run(command)
}

/// The status and headers of a browser's navigation to `path`.
fn navigate(port: u16, path: &str, cookie: Option<&str>) -> (u16, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(PATIENCE)).unwrap();
    let cookie = cookie.map_or_else(String::new, |cookie| format!("Cookie: {cookie}\r\n"));
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nSec-Fetch-Mode: navigate\r\n\
         Accept: text/html\r\n{cookie}Connection: close\r\n\r\n"
    )
    .unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    let (head, _) = reply.split_once("\r\n\r\n").unwrap();
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, head.to_ascii_lowercase())
}

/// Both projects served by the one server holding the shelf, each its own page and theme, and a
/// bookmark of today's paths sent to the starter's project.
fn both_served(repo: &Repo, starter: &str) {
    let port = repo.port();
    let front = front(port).expect("native status");
    assert!(front.plugins.iter().any(|name| name == "fixture-shelf"));
    let mut projects = front.projects.clone();
    projects.sort();
    assert_eq!(projects, ["alpha", "pantry"]);
    let registry = fs::read_to_string(repo.home.join("projects.json")).unwrap();
    assert!(registry.contains("\"alpha\"") && registry.contains("\"pantry\""));
    let (status, alpha) = get(port, "/alpha/").unwrap();
    assert_eq!(status, 200, "{alpha}");
    assert!(alpha.contains("<title>Alpha</title>"), "{alpha}");
    assert!(alpha.contains(":root{--primary:#7a2e8f;}"), "{alpha}");
    assert!(alpha.contains("data-layout-home=\"focus\""), "{alpha}");
    let (status, shelf) = get(port, "/pantry/").unwrap();
    assert_eq!(status, 200, "{shelf}");
    assert!(shelf.contains("<title>The Shelf</title>"), "{shelf}");
    assert!(shelf.contains(":root{--primary:#2a6f4e;}"), "{shelf}");
    assert!(shelf.contains("data-layout-header=\"compact\""), "{shelf}");
    for (id, token) in [("alpha", "#7a2e8f"), ("pantry", "#2a6f4e")] {
        let (status, css) = get(port, &format!("/{id}/theme.css")).unwrap();
        assert_eq!(
            (status, css),
            (200, format!(":root{{--primary:{token};}}\n"))
        );
    }
    // The shelf's own page answers under its project alone.
    assert_eq!(get(port, "/pantry/x/shelf/none.item").unwrap().0, 404);
    let (status, head) = navigate(port, "/docs/A.md", None);
    assert_eq!(status, 307, "{head}");
    assert!(
        head.contains(&format!("location: /{starter}/docs/a.md")),
        "{head}"
    );
    let (status, head) = navigate(port, "/", Some("symphony-project=alpha"));
    assert_eq!(status, 307, "{head}");
    assert!(head.contains("location: /alpha/"), "{head}");
    let (status, list) = get(port, "/").unwrap();
    assert_eq!(status, 200);
    assert!(
        list.contains("href=\"/alpha/\"") && list.contains("href=\"/pantry/\""),
        "{list}"
    );
}

#[test]
fn a_plain_host_waits_for_the_plugins_project_whose_build_then_takes_over_by_hand_off() {
    let repo = Repo::new("projects plain first");
    let shelf = projects(&repo);
    repo.start(&repo.main, false);
    let plain = repo.check_server();
    assert!(plain.plugins.is_empty());
    // The plain build cannot serve the shelf: it registers the project and says whose build can.
    let mut waits = start_with(&repo, APP, &shelf);
    let said = waits.wait();
    assert!(!said.success());
    let output = waits.output();
    assert!(
        output.contains("lacks the plugin fixture-shelf"),
        "{output}"
    );
    assert!(output.contains(&shelf.display().to_string()), "{output}");
    assert_eq!(get(repo.port(), "/pantry/").unwrap().0, 503);
    // The shelf's build serves every registered project, so it replaces the plain host.
    let mut takes = start_with(&repo, SHELF, &shelf);
    takes.success();
    let output = takes.output();
    assert!(output.contains("replaces it"), "{output}");
    assert!(output.contains("/pantry/"), "{output}");
    let host = front(repo.port()).expect("native status");
    assert_ne!(host.pid, plain.pid);
    assert_eq!(host.root, shelf.to_str().unwrap());
    repo.adopt();
    until("the replaced plain host to end", || {
        (!symphony_server::sys::pid_alive(plain.pid)).then_some(())
    });
    both_served(&repo, "pantry");
}

#[test]
fn a_host_holding_the_plugin_serves_a_plain_project_started_after_it() {
    let repo = Repo::new("projects shelf first");
    let shelf = projects(&repo);
    let mut first = start_with(&repo, SHELF, &shelf);
    first.success();
    repo.adopt();
    let host = front(repo.port()).expect("native status");
    // The plain build serves no plugin the host lacks: it registers and is served as it is.
    let mut second = start_with(&repo, APP, &repo.main);
    second.success();
    assert!(second.output().contains("/alpha/"), "{}", second.output());
    assert_eq!(front(repo.port()).unwrap().pid, host.pid);
    both_served(&repo, "pantry");
}
