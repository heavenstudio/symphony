use super::*;
use std::fs;
use std::path::PathBuf;
use symphony_plugin::execution::PrincipalKind;

/// A custom definition of one phase: a task is available, and a person with `accept` closes it.
const CUSTOM: &str = r#"
[workflow]
format = 1
initial = { phase = "build", state = "available" }
task_kinds = ["task"]
phases = [{ id = "build", label = "Build", order = 0, phone_order = 0, column = true }]
states = [{ id = "available", label = "Available" }, { id = "accepted", label = "Accepted" }]
capabilities = ["accept"]
adapters = []
guards = []
positions = [
  { at = { phase = "build", state = "available" }, slot = "release", dependencies = "none", unsuccessful_dependency = "block", accepts_work = true, terminal = false, legacy = "ready", satisfies_dependencies = false, completion = "none" },
  { at = { phase = "build", state = "accepted" }, slot = "release", dependencies = "none", unsuccessful_dependency = "block", accepts_work = false, terminal = true, legacy = "done", outcome = "success", satisfies_dependencies = true, completion = "covered_close" }
]
edges = [{ id = "accept", label = "Accept", from = { phase = "build", state = "available" }, to = { phase = "build", state = "accepted" }, operation = "close_covered", authority = [{ kind = "project_capability", capability = "accept" }], inputs = [], dependencies = "none", guards = [], effects = [{ kind = "outcome", outcome = "success", satisfies_dependencies = true }, { kind = "slot", policy = "release" }], automatic = false, human_wait = true }]
"#;

const MAPPED: &str = r#"
[[workflow_mappings]]
from = { phase = "ready", state = "available" }
to = { phase = "build", state = "available" }

[[workflow_mappings]]
from = { phase = "review", state = "awaiting_review" }
to = { phase = "build", state = "available" }
"#;

fn pair(phase: &str, state: &str) -> Pair {
    Pair {
        phase: phase.into(),
        state: state.into(),
    }
}

fn mapping(from: (&str, &str), to: (&str, &str)) -> Mapping {
    Mapping {
        from: pair(from.0, from.1),
        to: pair(to.0, to.1),
    }
}

#[test]
fn every_board_status_has_its_built_in_pair_and_nothing_else_does() {
    let declared: BTreeSet<Pair> = builtin().positions.into_iter().map(|p| p.at).collect();
    for status in [
        "backlog",
        "concept",
        "design",
        "ready",
        "in_progress",
        "review",
        "done",
        "blocked",
        "cancelled",
    ] {
        let at = legacy_pair(status).unwrap();
        assert_eq!(at.phase, status);
        assert!(declared.contains(&at), "{status}");
    }
    assert_eq!(
        legacy_pair("in_progress"),
        Some(pair("in_progress", "working"))
    );
    assert_eq!(legacy_pair("blocked"), Some(pair("blocked", "waiting")));
    assert_eq!(legacy_pair("shipped"), None);
    assert_eq!(legacy_pair(""), None);
}

#[test]
fn a_file_without_a_workflow_is_none_and_its_mappings_map_onto_nothing() {
    assert_eq!(written("").unwrap(), (None, vec![]));
    assert_eq!(
        written("[project]\nid = \"demo\"\n").unwrap(),
        (None, vec![])
    );
    assert_eq!(
        written(MAPPED).unwrap_err(),
        "workflow_mappings: no [workflow] to map onto"
    );
    // The candidate reader keeps its meaning: omission is built-in v1, mappings not read.
    assert_eq!(
        candidate(MAPPED).unwrap(),
        Validated::new(builtin()).unwrap()
    );
}

#[test]
fn a_custom_workflow_reads_with_its_mappings() {
    let (definition, mappings) = written(&format!("{CUSTOM}{MAPPED}")).unwrap();
    assert_eq!(definition.unwrap(), candidate(CUSTOM).unwrap());
    assert_eq!(
        mappings,
        vec![
            mapping(("ready", "available"), ("build", "available")),
            mapping(("review", "awaiting_review"), ("build", "available")),
        ]
    );
    let (_, none) = written(CUSTOM).unwrap();
    assert!(none.is_empty());
}

#[test]
fn mappings_refuse_unknown_fields_and_malformed_pairs() {
    for broken in [
        MAPPED.replace("to = {", "reason = \"x\"\nto = {"),
        MAPPED.replace("phase = \"ready\", ", ""),
        MAPPED.replace(
            "state = \"available\" }\nto",
            "state = \"available\", why = 1 }\nto",
        ),
    ] {
        assert!(written(&format!("{CUSTOM}{broken}")).is_err(), "{broken}");
    }
}

#[test]
fn a_mapping_names_a_removed_pair_and_a_declared_destination_once() {
    let custom = candidate(CUSTOM).unwrap();
    custom
        .check_mappings(&[mapping(("ready", "available"), ("build", "accepted"))])
        .unwrap();
    custom.check_mappings(&[]).unwrap();
    let still = custom
        .check_mappings(&[mapping(("build", "available"), ("build", "accepted"))])
        .unwrap_err();
    assert!(
        still.contains("build/available is still declared"),
        "{still}"
    );
    let nowhere = custom
        .check_mappings(&[mapping(("ready", "available"), ("verify", "available"))])
        .unwrap_err();
    assert!(
        nowhere.contains("ready/available -> verify/available names a destination"),
        "{nowhere}"
    );
    let twice = custom
        .check_mappings(&[
            mapping(("ready", "available"), ("build", "available")),
            mapping(("ready", "available"), ("build", "accepted")),
        ])
        .unwrap_err();
    assert_eq!(twice, "workflow_mappings: ready/available is mapped twice");
    assert!(
        written(&format!(
            "{CUSTOM}{}",
            MAPPED.replace("\"build\"", "\"verify\"")
        ))
        .is_err()
    );
}

#[test]
fn a_grant_gives_one_capability_to_one_principal() {
    let owner = Principal::new(PrincipalKind::Human, "local".into(), "owner".into());
    let agent = Principal::new(PrincipalKind::Agent, "local".into(), "amber-heron-1".into());
    let pretender = Principal::new(PrincipalKind::Agent, "local".into(), "owner".into());
    let none = Grants::none();
    assert!(none.supported().is_empty());
    assert!(!none.holds(&owner, "accept"));
    let grants = Grants::none()
        .grant("accept", owner.clone())
        .grant("accept", owner.clone())
        .grant("recovery", agent.clone());
    assert_eq!(grants.supported(), BTreeSet::from(["accept", "recovery"]));
    assert!(grants.holds(&owner, "accept"));
    assert!(!grants.holds(&owner, "recovery"));
    assert!(!grants.holds(&agent, "accept"));
    // An agent named like the owner is still an agent.
    assert!(!grants.holds(&pretender, "accept"));
    assert_eq!(
        grants,
        Grants::none()
            .grant("accept", owner)
            .grant("recovery", agent)
    );
}

#[test]
fn a_declared_capability_nobody_is_granted_is_unsupported() {
    let custom = candidate(CUSTOM).unwrap();
    assert_eq!(
        prerequisites(&custom, &[], &[]).unwrap_err(),
        "unsupported capability: accept"
    );
    let owner = Principal::new(PrincipalKind::Human, "local".into(), "owner".into());
    let grants = Grants::none().grant("accept", owner);
    let supported: Vec<&str> = grants.supported().into_iter().collect();
    prerequisites(&custom, &supported, &[]).unwrap();
    assert_eq!(
        prerequisites(&Validated::new(builtin()).unwrap(), &supported, &[]).unwrap_err(),
        "unsupported capability: legacy_actor"
    );
}

/// A folder under the system's temporary one, a git repository on `main`, removed when dropped.
pub(crate) struct Scratch(pub(crate) PathBuf);

impl Scratch {
    pub(crate) fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!(
            "workflow-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let scratch = Scratch(dir.canonicalize().unwrap());
        scratch.git(&["init", "-q", "-b", "main"]);
        scratch
    }

    pub(crate) fn git(&self, args: &[&str]) {
        let done = crate::sys::output(
            crate::sys::command("git")
                .args(["-c", "user.email=t@t", "-c", "user.name=t", "-C"])
                .arg(&self.0)
                .args(args),
        )
        .unwrap();
        assert!(done.status.success(), "git {args:?}: {done:?}");
    }

    pub(crate) fn commit(&self, message: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "--allow-empty", "-m", message]);
    }

    pub(crate) fn head(&self) -> String {
        let out = crate::sys::output(
            crate::sys::git()
                .arg("-C")
                .arg(&self.0)
                .args(["rev-parse", "HEAD"]),
        )
        .unwrap();
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn the_committed_reader_reads_the_branch_tip_and_nothing_uncommitted() {
    let repo = Scratch::new("committed");
    repo.commit("empty");
    assert_eq!(
        committed(&repo.0, "main").unwrap(),
        Committed {
            commit: repo.head(),
            custom: None
        }
    );
    fs::write(repo.0.join(FILE), "[project]\nid = \"demo\"\n").unwrap();
    repo.commit("no workflow");
    assert_eq!(committed(&repo.0, "main").unwrap().custom, None);

    fs::write(repo.0.join(FILE), format!("{CUSTOM}{MAPPED}")).unwrap();
    repo.commit("custom");
    let tip = repo.head();
    let read = committed(&repo.0, "main").unwrap();
    assert_eq!(read.commit, tip);
    let custom = read.custom.clone().unwrap();
    assert_eq!(custom.definition, candidate(CUSTOM).unwrap());
    assert_eq!(custom.mappings.len(), 2);
    assert_eq!(custom.revision.source.commit, tip);
    custom.definition.check_revision(&custom.revision).unwrap();

    // What the working tree and the index hold changes nothing until it is committed.
    fs::write(repo.0.join(FILE), "[workflow]\nformat = 999\n").unwrap();
    assert_eq!(committed(&repo.0, "main").unwrap(), read);
    repo.git(&["add", FILE]);
    assert_eq!(committed(&repo.0, "main").unwrap(), read);
    // Another branch's commit is not the default branch's.
    repo.git(&["checkout", "-q", "-b", "topic"]);
    repo.commit("invalid on a branch");
    assert_eq!(committed(&repo.0, "main").unwrap(), read);
    assert!(
        committed(&repo.0, "topic")
            .unwrap_err()
            .starts_with("workflow committed on topic: workflow:")
    );
}

#[test]
fn the_committed_reader_refuses_a_missing_branch_and_a_file_that_is_not_regular_text() {
    let repo = Scratch::new("refusals");
    assert_eq!(
        committed(&repo.0, "main").unwrap_err(),
        "workflow committed on main: the branch names no commit"
    );
    repo.commit("empty");
    assert!(committed(&repo.0, "missing").is_err());

    std::os::unix::fs::symlink("elsewhere", repo.0.join(FILE)).unwrap();
    repo.commit("linked");
    let linked = committed(&repo.0, "main").unwrap_err();
    assert!(
        linked.contains("must be a regular file, not mode 120000 blob"),
        "{linked}"
    );

    fs::remove_file(repo.0.join(FILE)).unwrap();
    fs::create_dir(repo.0.join(FILE)).unwrap();
    fs::write(repo.0.join(FILE).join("inner"), "x").unwrap();
    repo.commit("folder");
    let folder = committed(&repo.0, "main").unwrap_err();
    assert!(folder.contains("not mode 040000 tree"), "{folder}");

    fs::remove_dir_all(repo.0.join(FILE)).unwrap();
    fs::write(repo.0.join(FILE), [0xff, 0xfe]).unwrap();
    repo.commit("binary");
    assert_eq!(
        committed(&repo.0, "main").unwrap_err(),
        "workflow committed on main: the file is not UTF-8"
    );

    fs::write(
        repo.0.join(FILE),
        format!("{CUSTOM}{}", MAPPED.replace("\"build\"", "\"verify\"")),
    )
    .unwrap();
    repo.commit("bad mapping");
    assert!(
        committed(&repo.0, "main")
            .unwrap_err()
            .contains("names a destination the workflow does not declare")
    );
}
