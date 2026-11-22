use super::*;
use crate::store_fixture::pair;
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
fn a_file_without_a_workflow_is_none_and_its_mappings_map_onto_built_in_v1() {
    use crate::workflow::activate::tests::BACK;
    assert_eq!(written("").unwrap(), (None, vec![]));
    assert_eq!(
        written("[project]\nid = \"demo\"\n").unwrap(),
        (None, vec![])
    );
    // Mappings with no [workflow] say where a return to the board's writers sends each task:
    // from a pair built-in v1 does not declare to one it does.
    let (none, back) = written(BACK).unwrap();
    assert_eq!((none, back.len()), (None, 5));
    assert_eq!(
        (back[0].from.clone(), back[0].to.clone()),
        (pair("build", "available"), pair("ready", "available"))
    );
    assert_eq!(
        written(MAPPED).unwrap_err(),
        "workflow_mappings: ready/available is still declared; an ID keeps its meaning, so \
         only a removed pair is mapped"
    );
    let nowhere = "[[workflow_mappings]]\nfrom = { phase = \"build\", state = \"working\" }\nto = { phase = \"deploy\", state = \"queued\" }\n";
    assert_eq!(
        written(nowhere).unwrap_err(),
        "workflow_mappings: build/working -> deploy/queued names a destination the workflow \
         does not declare"
    );
    let twice = format!(
        "{BACK}[[workflow_mappings]]\nfrom = {{ phase = \"build\", state = \"working\" }}\nto = {{ phase = \"ready\", state = \"available\" }}\n"
    );
    assert_eq!(
        written(&twice).unwrap_err(),
        "workflow_mappings: build/working is mapped twice"
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
fn a_pair_written_as_an_array_is_refused_as_no_table() {
    let positional = MAPPED.replace(
        "from = { phase = \"ready\", state = \"available\" }",
        "from = [\"ready\", \"available\"]",
    );
    assert_ne!(positional, MAPPED);
    assert_eq!(
        written(&format!("{CUSTOM}{positional}")).unwrap_err(),
        "workflow_mappings: invalid type: sequence, expected struct Pair"
    );
    let listed = "workflow = [\"one\"]\n";
    let refused = candidate(listed).unwrap_err();
    assert!(
        refused.starts_with("workflow: invalid type: sequence, expected struct "),
        "{refused}"
    );
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
            custom: None,
            returning: Vec::new(),
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
    assert!(
        read.returning.is_empty(),
        "a custom definition's mappings are its own"
    );

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

/// The file is found among the entries of the commit's top tree, whatever sorts beside it, an
/// executable one read as a regular file, on either of git's object formats.
#[test]
fn the_committed_reader_finds_the_file_among_its_neighbours_on_either_object_format() {
    for format in ["sha1", "sha256"] {
        let repo = Scratch::new(&format!("neighbours-{format}"));
        fs::remove_dir_all(repo.0.join(".git")).unwrap();
        repo.git(&["init", "-q", "-b", "main", "--object-format", format]);
        // Entries named like it before and after it, and its own name lower in the tree.
        for (path, text) in [
            ("a", "a"),
            ("symphony.tom", "x"),
            ("symphony.toml.d/symphony.toml", "[workflow\n"),
            ("sub/symphony.toml", "[workflow\n"),
            ("z", "z"),
        ] {
            let path = repo.0.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        repo.commit("no file at the top");
        assert_eq!(committed(&repo.0, "main").unwrap().custom, None, "{format}");

        fs::write(repo.0.join(FILE), format!("{CUSTOM}{MAPPED}")).unwrap();
        repo.commit("custom");
        let read = committed(&repo.0, "main").unwrap();
        let custom = read.custom.unwrap();
        assert_eq!(custom.definition, candidate(CUSTOM).unwrap(), "{format}");
        let blob = crate::sys::output(
            crate::sys::git()
                .arg("-C")
                .arg(&repo.0)
                .args(["rev-parse", &format!("HEAD:{FILE}")]),
        )
        .unwrap();
        let blob = String::from_utf8(blob.stdout).unwrap();
        assert_eq!(custom.revision.source.project_blob, blob.trim());
        assert_eq!(read.commit, repo.head());

        // Executable, it is a regular file still.
        repo.git(&["update-index", "--chmod=+x", FILE]);
        repo.git(&["commit", "-q", "-m", "executable"]);
        let executable = committed(&repo.0, "main").unwrap().custom.unwrap();
        assert_eq!(executable.definition, custom.definition, "{format}");
    }
}

/// A submodule's commit where the file stands is no regular file either.
#[test]
fn the_committed_reader_refuses_a_submodule_at_the_files_name() {
    let repo = Scratch::new("gitlink");
    repo.commit("empty");
    let commit = repo.head();
    repo.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{commit},{FILE}"),
    ]);
    repo.git(&["commit", "-q", "-m", "a submodule"]);
    let gitlink = committed(&repo.0, "main").unwrap_err();
    assert_eq!(
        gitlink,
        "workflow committed on main: symphony.toml must be a regular file, not mode 160000 commit"
    );
}

#[test]
fn the_file_is_read_through_json_and_refuses_what_a_reading_from_toml_refuses() {
    let custom = candidate(CUSTOM).unwrap();
    let (written_one, mappings) = written(&format!("{CUSTOM}{MAPPED}")).unwrap();
    assert_eq!(written_one.unwrap(), custom);
    assert_eq!(mappings.len(), 2);
    // A syntax error is named at its line, as TOML names it.
    let syntax = written("[workflow\n").unwrap_err();
    assert!(
        syntax.starts_with("workflow: TOML parse error at line 1"),
        "{syntax}"
    );
    // A datetime is a table to the reader, so no field takes it, a string field neither.
    let dated = CUSTOM.replace("label = \"Build\"", "label = 1979-05-27T07:32:00Z");
    let refused = candidate(&dated).unwrap_err();
    assert!(
        refused.contains("invalid type: map, expected a string"),
        "{refused}"
    );
    // A float where an order is wanted, and one that is no number at all.
    for order in ["0.5", "nan", "inf"] {
        let float = CUSTOM.replace("order = 0,", &format!("order = {order},"));
        assert!(candidate(&float).is_err(), "{order}");
    }
    // A table where one is wanted, and a value that is not one.
    assert!(
        candidate("workflow = \"v1\"\n")
            .unwrap_err()
            .starts_with("workflow: ")
    );
    let mapped_wrong = written(&format!("workflow_mappings = 1\n{CUSTOM}")).unwrap_err();
    assert!(
        mapped_wrong.starts_with("workflow_mappings: invalid type"),
        "{mapped_wrong}"
    );
    // Unknown fields, at any depth, still refuse.
    for unknown in [
        CUSTOM.replace("format = 1", "format = 1\nextra = true"),
        CUSTOM.replace("column = true }", "column = true, wide = 2 }"),
        CUSTOM.replace("kind = \"slot\"", "kind = \"slot\", why = \"x\""),
    ] {
        let refused = candidate(&unknown).unwrap_err();
        assert!(refused.contains("unknown field"), "{refused}");
    }
    // Other tables of the file are not the workflow's to read.
    let other = format!("[project]\nname = \"x\"\nwhen = 2026-01-01\n{CUSTOM}");
    assert_eq!(candidate(&other).unwrap(), custom);
}

#[test]
fn ordered_sorts_by_key_and_keeps_the_order_of_equal_keys() {
    let mut items = vec![("b", 1), ("a", 2), ("b", 3), ("", 4), ("a", 5)];
    ordered(&mut items, |(key, _)| key.to_string());
    assert_eq!(items, [("", 4), ("a", 2), ("a", 5), ("b", 1), ("b", 3)]);
    let mut none: Vec<u8> = Vec::new();
    ordered(&mut none, |_| String::new());
    assert!(none.is_empty());
}

#[test]
fn a_pair_key_orders_as_the_pair_does() {
    let at = |phase: &str, state: &str| Pair {
        phase: phase.into(),
        state: state.into(),
    };
    // A phase that begins another sorts first, whatever the states; '-' and '/' sort below letters.
    let pairs = [
        at("a", "z"),
        at("a-b", "a"),
        at("a/b", "a"),
        at("ab", "a"),
        at("a", "a"),
        at("b", ""),
        at("", "b"),
        at("a", "a-b"),
        at("a", "ab"),
    ];
    for one in &pairs {
        for other in &pairs {
            assert_eq!(
                pair_key(one).cmp(&pair_key(other)),
                one.cmp(other),
                "{one:?} {other:?}"
            );
        }
    }
    let mut positions = pairs.to_vec();
    ordered(&mut positions, pair_key);
    let mut sorted = pairs.to_vec();
    sorted.sort();
    assert_eq!(positions, sorted);
}

#[test]
fn a_mapping_map_keeps_each_source_once_with_its_last_destination() {
    let at = |phase: &str| Pair {
        phase: phase.into(),
        state: "s".into(),
    };
    let one = |from: &str, to: &str| Mapping {
        from: at(from),
        to: at(to),
    };
    let mappings = [one("b", "x"), one("a", "y"), one("b", "z")];
    let map = mapped(&mappings);
    assert_eq!(map.len(), 2);
    assert_eq!(map[&at("b")], &at("z"));
    assert_eq!(map[&at("a")], &at("y"));
    assert!(mapped(&[]).is_empty());
}

#[test]
fn a_digest_is_taken_of_the_same_bytes_as_its_value_written_straight() {
    use symphony_api::workflow::{InputValue, OperationRequest, PositionSnapshot};
    let source = Source {
        commit: "1".repeat(40),
        project_blob: "2".repeat(40),
    };
    for definition in [builtin(), candidate(CUSTOM).unwrap().definition().clone()] {
        let value = (FORMAT, &definition, &source);
        assert_eq!(
            canonical(&value).unwrap(),
            serde_json::to_vec(&value).unwrap()
        );
    }
    let revision = Validated::new(builtin()).unwrap().revision(source).unwrap();
    let request = OperationRequest {
        project: "sim \"quoted\" \u{e9}\u{1f600}\n".into(),
        task: "sim/jump".into(),
        edge: "claim".into(),
        expected: PositionSnapshot {
            workflow: revision,
            activation_generation: u64::MAX,
            task_revision: 0,
            at: Pair {
                phase: "build".into(),
                state: "available".into(),
            },
        },
        input_revisions: [("b".into(), "2".into()), ("a".into(), "1".into())].into(),
        idempotency_key: "key\u{0}\u{7f}".into(),
        inputs: [
            ("text".into(), InputValue::Text("\\ </script>".into())),
            ("flag".into(), InputValue::Flag(true)),
            (
                "pair".into(),
                InputValue::Pair(Pair {
                    phase: "p".into(),
                    state: "s".into(),
                }),
            ),
        ]
        .into(),
    };
    assert_eq!(
        canonical(&request).unwrap(),
        serde_json::to_vec(&request).unwrap()
    );
}

#[test]
fn an_authority_key_orders_as_the_authority_does() {
    use symphony_api::workflow::{Authority, Role};
    let capability = |kind: fn(String) -> Authority, name: &str| kind(name.into());
    let project = |capability| Authority::ProjectCapability { capability };
    let system = |capability| Authority::SystemCapability { capability };
    let all = [
        capability(system, "a"),
        Authority::TaskRole {
            role: Role::Orchestrator,
        },
        capability(project, "b"),
        capability(project, ""),
        Authority::TaskRole {
            role: Role::Executor,
        },
        capability(system, ""),
        Authority::TaskRole {
            role: Role::Reviewer,
        },
        capability(project, "a-b"),
        capability(project, "a"),
    ];
    for one in &all {
        for other in &all {
            let keys = authority_key(one).cmp(&authority_key(other));
            assert_eq!(keys, one.cmp(other), "{one:?} {other:?}");
        }
    }
    let mut keyed = all.to_vec();
    ordered(&mut keyed, authority_key);
    let mut sorted = all.to_vec();
    sorted.sort();
    assert_eq!(keyed, sorted);
}

#[test]
fn a_built_in_position_built_alone_is_the_one_built_in_v1_declares() {
    let definition = builtin();
    assert_eq!(definition.positions.len(), 13);
    for position in &definition.positions {
        assert_eq!(builtin::position_at(&position.at).as_ref(), Some(position));
    }
    let pair = |phase: &str, state: &str| Pair {
        phase: phase.into(),
        state: state.into(),
    };
    for undeclared in [
        pair("ready", "held"),
        pair("done", "held"),
        pair("done", "available"),
        pair("review", "available"),
        pair("shipped", "available"),
        pair("", ""),
    ] {
        assert_eq!(builtin::position_at(&undeclared), None, "{undeclared:?}");
    }
}
