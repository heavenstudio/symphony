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
    served_shell_with(shell, |_| {})
}

/// [`served_shell`] with the project fixture's view changed by `change` first: who reads the page,
/// say, as the server names them.
pub fn served_shell_with(shell: &str, change: impl FnOnce(&mut ProjectView)) -> String {
    let mut project: ProjectView =
        serde_json::from_str(&fixture("project.json")).expect("a project");
    change(&mut project);
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

/// The modules `module`, the published script at `path` (`/assets/NAME.js`), imports by name, as
/// published paths, sorted: each `import` or `from` of a module beside it, as Vite writes them.
pub fn imported(path: &str, module: &str) -> Vec<String> {
    let folder = &path[..=path.rfind('/').expect("a published path")];
    let mut names: Vec<String> = ["from\"./", "import\"./"]
        .iter()
        .flat_map(|keyword| {
            module.match_indices(keyword).map(|(at, _)| {
                let rest = &module[at + keyword.len()..];
                let name = &rest[..rest.find('"').expect("a closed name")];
                format!("{folder}{name}")
            })
        })
        .collect();
    names.sort();
    names.dedup();
    names
}
