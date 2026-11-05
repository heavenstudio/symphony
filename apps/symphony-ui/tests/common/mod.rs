//! The wire fixtures crates/symphony-api pins, read for the client's tests.
#![allow(dead_code)] // each test binary uses its own share of these

use symphony_api::{Board, Delta, PROJECT_ELEMENT, ProjectView};

pub fn fixture_path(name: &str) -> String {
    format!(
        "{}/../../crates/symphony-api/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    )
}

pub fn fixture(name: &str) -> String {
    let path = fixture_path(name);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{path}: {err}"))
}

pub fn board(name: &str) -> Board {
    serde_json::from_str(&fixture(name)).unwrap_or_else(|err| panic!("{name}: {err}"))
}

pub fn delta(name: &str) -> Delta {
    serde_json::from_str(&fixture(name)).unwrap_or_else(|err| panic!("{name}: {err}"))
}

/// `shell`, a build's `index.html`, as the server serves it to a project with a plugin page: the project fixture
/// (`project.json`) inlined after `<body>`, escaped as the server escapes it.
pub fn served_shell(shell: &str) -> String {
    let project: ProjectView = serde_json::from_str(&fixture("project.json")).expect("a project");
    let element = format!(
        "<script type=\"application/json\" id=\"{PROJECT_ELEMENT}\">{}</script>",
        serde_json::to_string(&project)
            .unwrap()
            .replace("</", "<\\/")
    );
    let at = shell
        .find("<body")
        .and_then(|at| shell[at..].find('>').map(|end| at + end + 1))
        .expect("a shell with a body");
    format!("{}{element}{}", &shell[..at], &shell[at..])
}
