//! The hand-off request's checks.

use super::*;
use symphony::machine::Entry;

fn entry(id: &str) -> Entry {
    Entry {
        id: id.into(),
        name: id.into(),
        root: PathBuf::from(format!("/work/{id}")),
        manifest: None,
        plugins: Vec::new(),
        pin: 1,
        schema: 1,
    }
}

#[test]
fn a_handover_request_is_taken_once_and_only_when_it_names_what_the_server_may_follow() {
    let tree = crate::testing::Tree::new();
    let machine = tree.path("machine");
    std::fs::create_dir_all(&machine).unwrap();
    let root = tree.path("project");
    let install = tree.path("install");
    std::fs::create_dir_all(&root).unwrap();
    let exe = tree.write("install/symphony-server", "");
    assert_eq!(Handover::take(&machine), None, "no request, nothing taken");
    let request = Handover {
        exe: exe.clone(),
        root: root.clone(),
        install: install.clone(),
    };
    request.ask(&machine).unwrap();
    let refused = Handover::take(&machine).unwrap().unwrap_err();
    assert!(
        refused.contains("names a project the registry does not hold"),
        "{refused}"
    );
    assert_eq!(
        Handover::take(&machine),
        None,
        "a refused request is removed"
    );
    Registry {
        projects: vec![Entry {
            root: root.clone(),
            ..entry("project")
        }],
    }
    .write(&machine)
    .unwrap();
    request.ask(&machine).unwrap();
    assert_eq!(Handover::take(&machine), Some(Ok(request.clone())));
    assert_eq!(Handover::take(&machine), None, "a request is taken once");
    for (bad, said) in [
        (
            Handover {
                exe: "symphony-server".into(),
                ..request.clone()
            },
            "names a relative path",
        ),
        (
            Handover {
                exe: install.clone(),
                ..request.clone()
            },
            "names no executable file",
        ),
        (
            Handover {
                install: exe.clone(),
                ..request.clone()
            },
            "names no install folder",
        ),
    ] {
        bad.ask(&machine).unwrap();
        let why = Handover::take(&machine).unwrap().unwrap_err();
        assert!(why.contains(said), "{why}");
    }
    std::fs::write(machine.join(HANDOVER), "not json").unwrap();
    assert!(Handover::take(&machine).unwrap().is_err());
    assert!(!machine.join(HANDOVER).exists());
}
