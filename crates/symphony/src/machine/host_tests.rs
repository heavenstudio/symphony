//! The host rule's cases: each refusal and each replacement.

use std::path::PathBuf;

use super::*;

fn build(plugins: &[&str], pin: u32, schema: i64) -> Build {
    Build {
        plugins: plugins.iter().map(|name| name.to_string()).collect(),
        pin,
        schema,
        by: "/work/host".into(),
    }
}

fn entry(id: &str, plugins: &[&str], pin: u32, schema: i64) -> Entry {
    Entry {
        id: id.into(),
        name: id.into(),
        root: PathBuf::from(format!("/work/{id}")),
        manifest: None,
        plugins: plugins.iter().map(|name| name.to_string()).collect(),
        pin,
        schema,
    }
}

fn registry(entries: &[Entry]) -> Registry {
    Registry {
        projects: entries.to_vec(),
    }
}

#[test]
fn a_host_serves_a_project_whose_plugins_it_holds_at_its_pin_or_an_older_one() {
    let running = build(&["shelf"], 2, 7);
    let ours = build(&[], 2, 7);
    for project in [
        entry("plain", &[], 2, 7),
        entry("older", &[], 1, 6),
        entry("shelved", &["shelf"], 2, 7),
    ] {
        assert_eq!(
            judge(&running, &ours, &project, &registry(&[])),
            Verdict::Serves,
            "{project:?}"
        );
    }
}

#[test]
fn a_project_with_no_plugins_never_replaces_a_running_host() {
    let running = build(&["shelf"], 1, 6);
    let ours = build(&[], 2, 7);
    let newer = entry("plain", &[], 2, 7);
    let Verdict::Refuse(why) = judge(&running, &ours, &newer, &registry(&[])) else {
        panic!("a plain project's newer pin is refused");
    };
    assert_eq!(
        why,
        "the running Symphony is at Symphony pin 1, older than plain's 2: a project with no \
         plugins never replaces the running Symphony: /work/host must bump its Symphony, then \
         run `just symphony` there"
    );
    let schema = entry("plain", &[], 1, 7);
    let Verdict::Refuse(why) = judge(&running, &ours, &schema, &registry(&[])) else {
        panic!("a plain project's newer board schema is refused");
    };
    assert!(why.starts_with("the running Symphony reads board schema 6, older than plain's 7"));
}

#[test]
fn a_build_holding_every_registered_plugin_replaces_a_host_that_lacks_one() {
    let running = build(&[], 1, 6);
    let ours = build(&["shelf"], 1, 6);
    let shelved = entry("shelf-project", &["shelf"], 1, 6);
    let plain = entry("plain", &[], 1, 6);
    let Verdict::Replace(why) = judge(&running, &ours, &shelved, &registry(&[plain])) else {
        panic!("the plugin project's build replaces the plain host");
    };
    assert_eq!(
        why,
        "the running Symphony lacks the plugin shelf, which shelf-project names; this build \
         serves every registered project and replaces it"
    );
    // A newer pin is replaced by a newer build that serves everyone too.
    let newer = build(&["shelf"], 2, 6);
    let bumped = entry("shelf-project", &["shelf"], 2, 6);
    assert!(matches!(
        judge(&running, &newer, &bumped, &registry(&[])),
        Verdict::Replace(_)
    ));
}

#[test]
fn a_build_that_cannot_serve_every_project_waits_or_refuses_naming_the_fix() {
    let running = build(&[], 1, 6);
    // `project add` from a plain build for a project with plugins: its own start replaces the host.
    let plain_build = build(&[], 1, 6);
    let shelved = entry("shelf-project", &["shelf", "map"], 1, 6);
    let Verdict::Waits(why) = judge(&running, &plain_build, &shelved, &registry(&[])) else {
        panic!("a plugin project waits for its own start");
    };
    assert_eq!(
        why,
        "the running Symphony lacks the plugins shelf, map, which shelf-project names: run \
         `just symphony` in /work/shelf-project, whose build holds them"
    );
    // A build lacking another registered project's plugin cannot be the one host.
    let ours = build(&["shelf", "map"], 1, 6);
    let other = entry("other", &["atlas"], 1, 6);
    let Verdict::Waits(why) = judge(&running, &ours, &shelved, &registry(&[other])) else {
        panic!("a build lacking a registered project's plugin does not replace");
    };
    assert!(
        why.ends_with(
            "this build cannot replace it: it lacks the plugin atlas, which other names; one host \
             must compile every registered project's plugins"
        ),
        "{why}"
    );
    // A build older than the running host never replaces it, whatever it holds.
    let newer_host = build(&[], 2, 6);
    let Verdict::Waits(why) = judge(&newer_host, &ours, &shelved, &registry(&[])) else {
        panic!("an older build does not replace a newer host");
    };
    assert!(
        why.ends_with("this build is older than the running Symphony: /work/shelf-project must bump its Symphony"),
        "{why}"
    );
    // A newer pin is refused, not registered, when this build cannot serve every project.
    let host = build(&["shelf", "map", "atlas"], 1, 6);
    let bumped = entry("shelf-project", &["shelf", "map"], 2, 6);
    let atlas = entry("other", &["atlas"], 1, 6);
    let Verdict::Refuse(why) = judge(
        &host,
        &build(&["shelf", "map"], 2, 6),
        &bumped,
        &registry(&[atlas]),
    ) else {
        panic!("a newer pin this build cannot host everyone at is refused");
    };
    assert_eq!(
        why,
        "the running Symphony is at Symphony pin 1, older than shelf-project's 2: this build \
         cannot replace it: it lacks the plugin atlas, which other names; one host must compile \
         every registered project's plugins"
    );
}

#[test]
fn the_project_itself_is_judged_by_its_new_entry_not_its_old_one() {
    // The registry still holds the project at its old pin; its new entry is the one judged.
    let running = build(&["shelf"], 1, 6);
    let ours = build(&["shelf"], 2, 6);
    let old = entry("shelf-project", &["shelf"], 1, 6);
    let new = entry("shelf-project", &["shelf"], 2, 6);
    assert!(matches!(
        judge(&running, &ours, &new, &registry(&[old])),
        Verdict::Replace(_)
    ));
}

#[test]
fn a_build_reads_its_own_plugins_pin_and_schema_and_a_server_its_front() {
    let this = Build::this(Path::new("/work/here"));
    assert_eq!(this.pin, crate::machine::PIN);
    assert_eq!(this.schema, symphony_board::SCHEMA);
    assert_eq!(this.by, "/work/here");
    let front = symphony_api::Front {
        pid: 1,
        port: 2,
        root: "/work/there".into(),
        ready: true,
        app: None,
        tailnet: None,
        orphans: 0,
        plugins: vec!["shelf".into()],
        pin: 3,
        schema: 4,
        projects: vec!["there".into()],
    };
    assert_eq!(
        Build::of(&front),
        build(&["shelf"], 3, 4).with_by("/work/there")
    );
}

impl Build {
    fn with_by(self, by: &str) -> Build {
        Build {
            by: by.into(),
            ..self
        }
    }
}
