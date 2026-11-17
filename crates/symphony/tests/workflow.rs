//! Candidate syntax, graph invariants and legacy parity; no database or activation fixtures.

use std::path::Path;
use symphony::project::{Project, workflow::*};
use symphony_api::workflow::*;
#[path = "common/layouts.rs"]
mod layouts;
#[path = "common/tree.rs"]
mod tree;

fn source() -> Source {
    Source {
        commit: "a".repeat(40),
        project_blob: "b".repeat(40),
    }
}

fn invalid(mut change: impl FnMut(&mut Definition), expected: &str) {
    let mut d = builtin();
    change(&mut d);
    let error = Validated::new(d).unwrap_err();
    assert!(
        error.contains(expected),
        "expected {expected:?}, got {error:?}"
    );
}

fn pair(phase: &str, state: &str) -> Pair {
    Pair {
        phase: phase.into(),
        state: state.into(),
    }
}

#[test]
fn default_is_valid_and_omission_preserves_project_and_definition() {
    let default = Validated::new(builtin()).unwrap();
    assert_eq!(candidate("").unwrap(), default);
    assert_eq!(candidate("[project]\nname = 'demo'\n").unwrap(), default);
    assert_eq!(
        Project::parse(Path::new("/tmp/demo"), "").unwrap(),
        Project::plain(Path::new("/tmp/demo"))
    );
    assert_eq!(DEFAULT_VERSION, 1);
    let encoded = serde_json::to_string(default.definition()).unwrap();
    assert_eq!(
        Validated::new(serde_json::from_str(&encoded).unwrap()).unwrap(),
        default
    );
    let d = default.definition();
    assert_eq!(d.initial, pair("ready", "available"));
    let mut columns: Vec<_> = d.phases.iter().filter(|p| p.column).collect();
    columns.sort_by_key(|p| p.order);
    assert_eq!(
        columns.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
        symphony_board::PIPELINE
    );
    assert!(
        d.positions
            .iter()
            .any(|p| p.at == pair("done", "approved") && !p.terminal && p.outcome.is_none())
    );
    assert!(d.positions.iter().any(|p| p.at == pair("done", "completed")
        && p.terminal
        && p.outcome == Some(Outcome::Success)));
}

#[test]
fn default_move_routes_match_every_legacy_pair() {
    let d = builtin();
    for from in symphony_board::STATUSES {
        for to in symphony_board::STATUSES {
            let route = d.edges.iter().find(|e| e.id == format!("{from}_to_{to}"));
            assert_eq!(
                route.is_some(),
                symphony_board::can_transition(from, to),
                "{from} -> {to}"
            );
            if let Some(route) = route {
                assert_eq!(route.from.phase, from);
                assert_eq!(route.to.phase, to);
                assert_eq!(route.guards, ["legacy_board"]);
            }
        }
    }
    for op in [
        Operation::Claim,
        Operation::Release,
        Operation::Block,
        Operation::Resume,
        Operation::RequestReview,
        Operation::Approve,
        Operation::Changes,
        Operation::CompleteSubtask,
        Operation::CloseCovered,
        Operation::PrepareLand,
        Operation::ConfirmLand,
    ] {
        assert!(d.edges.iter().any(|e| e.operation == op), "{op:?}");
    }
    let work = d
        .positions
        .iter()
        .find(|p| p.at.phase == "in_progress")
        .unwrap();
    assert_eq!(
        work.recovery.as_ref().unwrap().lost_holder.target,
        Some(pair("ready", "available"))
    );
    let hold = d.edges.iter().find(|e| e.id == "hold_review").unwrap();
    assert!(hold.effects.contains(&Effect::Slot {
        policy: Slot::Retain
    }));
    assert!(hold.effects.contains(&Effect::Block { save_resume: true }));
    let movement = d
        .edges
        .iter()
        .find(|e| e.id == "ready_to_in_progress")
        .unwrap();
    assert_eq!(movement.operation, Operation::Advance);
    assert!(
        !movement
            .effects
            .iter()
            .any(|e| matches!(e, Effect::AcquireRole { .. }))
    );
    assert!(movement.effects.contains(&Effect::Slot {
        policy: Slot::Acquire
    }));
    let review = d
        .edges
        .iter()
        .find(|e| e.id == "in_progress_to_review")
        .unwrap();
    assert!(
        !review
            .effects
            .iter()
            .any(|e| matches!(e, Effect::ReleaseRole { .. }))
    );
    let block = d
        .edges
        .iter()
        .find(|e| e.id == "review_to_blocked")
        .unwrap();
    assert!(block.effects.contains(&Effect::Slot {
        policy: Slot::Release
    }));
}

#[test]
fn strict_ids_duplicates_and_pairs() {
    for id in ["", "Build", "1build", "à", "a.b", "a/b", "a b"] {
        invalid(|d| d.phases[0].id = id.into(), "invalid workflow ID");
    }
    invalid(|d| d.phases.push(d.phases[0].clone()), "duplicate phase");
    invalid(|d| d.states.push(d.states[0].clone()), "duplicate state");
    invalid(|d| d.edges.push(d.edges[0].clone()), "duplicate edge");
    invalid(
        |d| d.positions.push(d.positions[0].clone()),
        "duplicate position",
    );
    invalid(
        |d| d.positions[0].at.phase = "unknown".into(),
        "unknown phase",
    );
    invalid(
        |d| d.positions[0].at.state = "unknown".into(),
        "unknown state",
    );
    invalid(
        |d| d.initial = pair("ready", "working"),
        "invalid phase/state pair",
    );
    invalid(
        |d| d.edges[0].to = pair("ready", "working"),
        "invalid phase/state pair",
    );
    invalid(|d| d.format = FORMAT + 1, "unsupported workflow format");
    invalid(|d| d.task_kinds.clear(), "requires task kinds");
    invalid(|d| d.phases[0].label = " ".into(), "phase labels");
    invalid(
        |d| d.phases[0].order = d.phases[1].order,
        "unique display/phone order",
    );
    invalid(
        |d| d.phases[0].phone_order = d.phases[1].phone_order,
        "unique display/phone order",
    );
    invalid(|d| d.states[0].label.clear(), "state label");
}

#[test]
fn ambiguous_aliases_and_routes_are_refused() {
    invalid(
        |d| {
            let mut e = d.edges[0].clone();
            e.id = "other".into();
            d.edges.push(e);
        },
        "ambiguous legacy alias",
    );
    invalid(
        |d| {
            let mut e = d.edges[0].clone();
            e.id = "other".into();
            e.alias = None;
            d.edges[0].alias = None;
            d.edges.push(e);
        },
        "ambiguous operation route",
    );
    invalid(
        |d| d.edges[0].alias = Some("Move".into()),
        "invalid workflow ID",
    );
    invalid(
        |d| d.edges[0].label.clear(),
        "requires a label and authority",
    );
    invalid(
        |d| d.edges[0].authority.clear(),
        "requires a label and authority",
    );
}

#[test]
fn capabilities_guards_and_effects_cannot_supply_authority() {
    invalid(
        |d| {
            let a = d.edges[0].authority[0].clone();
            d.edges[0].authority.push(a);
        },
        "duplicate authority",
    );
    invalid(
        |d| {
            d.edges[0].authority = vec![Authority::ProjectCapability {
                capability: "owner".into(),
            }]
        },
        "unknown capability",
    );
    invalid(
        |d| {
            d.edges[0].authority = vec![Authority::SystemCapability {
                capability: "reaper".into(),
            }]
        },
        "unknown capability",
    );
    invalid(
        |d| d.guards[0].adapter = "missing".into(),
        "unknown guard adapter",
    );
    invalid(
        |d| d.edges[0].guards.push("missing".into()),
        "unknown guard",
    );
    invalid(
        |d| d.edges[0].effects.push(Effect::Review { approved: true }),
        "cannot impersonate effect",
    );
    invalid(
        |d| {
            d.edges
                .iter_mut()
                .find(|e| e.operation == Operation::Approve)
                .unwrap()
                .authority = vec![Authority::ProjectCapability {
                capability: "legacy_actor".into(),
            }]
        },
        "independent task reviewer",
    );
    invalid(
        |d| {
            d.edges
                .iter_mut()
                .find(|e| e.operation == Operation::Claim)
                .unwrap()
                .effects
                .retain(|e| !matches!(e, Effect::AcquireRole { .. }))
        },
        "missing operation effect",
    );
    invalid(|d| d.edges[0].effects.clear(), "explicit slot effect");
    invalid(
        |d| {
            d.edges
                .iter_mut()
                .find(|e| e.operation == Operation::PrepareLand)
                .unwrap()
                .effects = vec![Effect::Prepare {
                adapter: "missing".into(),
            }]
        },
        "unknown effect adapter",
    );
    let d = Validated::new(builtin()).unwrap();
    assert!(
        prerequisites(&d, &[], &["legacy_board"])
            .unwrap_err()
            .contains("capability")
    );
    assert!(
        prerequisites(&d, &["legacy_actor"], &[])
            .unwrap_err()
            .contains("adapter")
    );
    prerequisites(&d, &["legacy_actor"], &["legacy_board"]).unwrap();
}

#[test]
fn all_operation_effect_requirements_are_checked() {
    let mut external = builtin();
    for e in &mut external.edges {
        e.operation = match e.operation {
            Operation::PrepareLand => Operation::PrepareExternal,
            Operation::ConfirmLand => Operation::ConfirmExternal,
            other => other,
        };
    }
    Validated::new(external).unwrap();
    for op in [
        Operation::Claim,
        Operation::Release,
        Operation::Approve,
        Operation::Changes,
        Operation::Block,
        Operation::Resume,
        Operation::Cancel,
        Operation::CompleteSubtask,
        Operation::CloseCovered,
        Operation::PrepareLand,
        Operation::ConfirmLand,
    ] {
        invalid(
            |d| {
                d.edges
                    .iter_mut()
                    .find(|e| e.operation == op)
                    .unwrap()
                    .effects = vec![Effect::Slot {
                    policy: Slot::Retain,
                }]
            },
            "missing operation effect",
        );
    }
    for effect in [
        Effect::ParentIntegration,
        Effect::Finalize {
            adapter: "legacy_board".into(),
        },
        Effect::Prepare {
            adapter: "legacy_board".into(),
        },
        Effect::Outcome {
            outcome: Outcome::Success,
            satisfies_dependencies: true,
        },
    ] {
        invalid(
            |d| d.edges[0].effects.push(effect.clone()),
            "cannot impersonate effect",
        );
    }
}

#[test]
fn terminal_routes_and_conflicting_effects_cannot_fabricate_outcomes() {
    for completion in [
        Completion::Landing,
        Completion::CoveredClose,
        Completion::ParentIntegration,
    ] {
        invalid(
            |d| {
                d.positions
                    .iter_mut()
                    .find(|p| p.at == pair("done", "completed"))
                    .unwrap()
                    .completion = completion
            },
            "completion requirement",
        );
    }
    invalid(
        |d| {
            let e = d
                .edges
                .iter_mut()
                .find(|e| e.id == "confirm_land_done")
                .unwrap();
            e.effects.retain(|e| !matches!(e, Effect::Outcome { .. }));
        },
        "terminal route requires matching outcome",
    );
    invalid(
        |d| {
            d.edges[0].effects.push(Effect::Slot {
                policy: Slot::Acquire,
            })
        },
        "exactly one explicit slot",
    );
    invalid(
        |d| {
            let e = d
                .edges
                .iter_mut()
                .find(|e| e.operation == Operation::Approve)
                .unwrap();
            e.effects.push(Effect::Review { approved: true });
        },
        "conflicting repeated effect",
    );
    invalid(
        |d| d.edges[0].effects.push(Effect::ClearBlock),
        "cannot impersonate effect",
    );
    invalid(
        |d| {
            let e = d
                .edges
                .iter_mut()
                .find(|e| e.operation == Operation::Cancel)
                .unwrap();
            e.effects.iter_mut().for_each(|e| {
                if let Effect::Outcome {
                    satisfies_dependencies,
                    ..
                } = e
                {
                    *satisfies_dependencies = true;
                }
            });
        },
        "cannot impersonate effect",
    );
}

#[test]
fn staffed_positions_and_recovery_are_explicit() {
    invalid(
        |d| {
            d.positions
                .iter_mut()
                .find(|p| p.staffing.is_some())
                .unwrap()
                .recovery = None
        },
        "missing holder recovery",
    );
    invalid(
        |d| {
            d.positions[0].recovery.as_mut().unwrap().lost_holder.target =
                Some(pair("missing", "working"))
        },
        "invalid phase/state pair",
    );
    invalid(
        |d| d.positions[0].recovery.as_mut().unwrap().release.manual = false,
        "stay requires explicit manual",
    );
    invalid(
        |d| {
            d.positions
                .iter_mut()
                .find(|p| p.terminal)
                .unwrap()
                .accepts_work = true
        },
        "terminal position",
    );
    invalid(
        |d| d.positions.iter_mut().find(|p| p.terminal).unwrap().outcome = None,
        "terminal position",
    );
    invalid(
        |d| d.initial = pair("cancelled", "cancelled"),
        "initial position is terminal",
    );
    invalid(
        |d| d.edges[0].from = pair("cancelled", "cancelled"),
        "ordinary edge from terminal",
    );
    invalid(
        |d| d.edges.retain(|e| e.to != pair("done", "completed")),
        "unreachable completion",
    );
    invalid(
        |d| {
            d.edges.retain(|e| e.from.phase != "backlog");
            d.positions
                .iter_mut()
                .find(|p| p.at.phase == "backlog")
                .unwrap()
                .recovery = None;
        },
        "no outcome or manual recovery",
    );
}

#[test]
fn deliberate_rework_is_allowed_automatic_cycles_need_bounded_manual_exit() {
    let mut d = builtin();
    let index = d
        .edges
        .iter()
        .position(|e| e.id == "prepare_land_in_progress")
        .unwrap();
    d.edges[index].automatic = true;
    assert!(
        Validated::new(d.clone())
            .unwrap_err()
            .contains("finite retry bound")
    );
    d.edges[index].retry = Some(RetryBound {
        maximum: 2,
        exhausted: pair("blocked", "waiting"),
    });
    Validated::new(d.clone()).unwrap();
    d.edges[index].retry.as_mut().unwrap().maximum = 0;
    assert!(
        Validated::new(d.clone())
            .unwrap_err()
            .contains("finite positive maximum")
    );
    d.edges[index].retry.as_mut().unwrap().maximum = 1;
    d.edges[index].retry.as_mut().unwrap().exhausted = pair("cancelled", "cancelled");
    assert!(
        Validated::new(d)
            .unwrap_err()
            .contains("manual exhaustion destination")
    );
    let mut d = builtin();
    for e in &mut d.edges {
        if ["concept_to_design", "design_to_concept"].contains(&e.id.as_str()) {
            e.automatic = true;
        }
    }
    assert!(
        Validated::new(d)
            .unwrap_err()
            .contains("finite retry bound")
    );
}

#[test]
fn human_wait_needs_host_person_capability_and_no_automatic_retry() {
    let mut d = builtin();
    d.edges[0].human_wait = true;
    Validated::new(d.clone()).unwrap();
    d.edges[0].authority = vec![Authority::TaskRole {
        role: Role::Executor,
    }];
    assert!(
        Validated::new(d.clone())
            .unwrap_err()
            .contains("human wait")
    );
    d.edges[0].authority = vec![Authority::SystemCapability {
        capability: "legacy_actor".into(),
    }];
    assert!(
        Validated::new(d.clone())
            .unwrap_err()
            .contains("human wait")
    );
    d.edges[0].authority = vec![Authority::ProjectCapability {
        capability: "legacy_actor".into(),
    }];
    d.edges[0].automatic = true;
    assert!(Validated::new(d).unwrap_err().contains("human wait"));
}

#[test]
fn revision_normalizes_sets_and_fences_content_format_and_source() {
    let d = Validated::new(builtin()).unwrap();
    let revision = d.revision(source()).unwrap();
    d.check_revision(&revision).unwrap();
    let mut reordered = builtin();
    reordered.edges.reverse();
    reordered.positions.reverse();
    reordered.phases.reverse();
    reordered.states.reverse();
    assert_eq!(
        Validated::new(reordered)
            .unwrap()
            .revision(source())
            .unwrap(),
        revision
    );
    let mut changed = builtin();
    changed.phases[0].label.push('!');
    let changed = Validated::new(changed).unwrap();
    assert!(
        changed
            .check_revision(&revision)
            .unwrap_err()
            .contains("stale_workflow")
    );
    let mut changed = builtin();
    changed.phases.swap(0, 1);
    let a = changed.phases[0].order;
    changed.phases[0].order = changed.phases[1].order;
    changed.phases[1].order = a;
    assert_ne!(
        Validated::new(changed).unwrap().revision(source()).unwrap(),
        revision
    );
    let mut other = source();
    other.commit = "c".repeat(40);
    assert_ne!(d.revision(other).unwrap(), revision);
    let mut other = source();
    other.project_blob = "c".repeat(64);
    assert_ne!(d.revision(other).unwrap(), revision);
    let mut forged = revision.clone();
    forged.format += 1;
    assert!(d.check_revision(&forged).is_err());
    for bad in ["", "abc", &"A".repeat(40), &"g".repeat(64)] {
        let mut other = source();
        other.commit = bad.into();
        assert!(d.revision(other).is_err());
    }
}

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

#[test]
fn candidate_is_strict_and_runtime_activation_is_explicitly_unsupported() {
    let d = candidate(CUSTOM).unwrap();
    assert_eq!(d.definition().phases[0].id, "build");
    assert!(
        Project::parse(Path::new("/tmp/demo"), CUSTOM)
            .unwrap_err()
            .contains("unsupported_activation")
    );
    for (from, to) in [
        ("format = 1", "format = 1\nunknown = true"),
        ("close_covered", "deploy_shell"),
        ("accepts_work", "accept_work"),
        ("project_capability", "owner"),
        ("slot = \"release\"", "slot = \"unlimited\""),
    ] {
        assert!(candidate(&CUSTOM.replace(from, to)).is_err(), "{to}");
    }
    assert!(candidate("[workflow]").is_err());
    assert!(
        candidate(&CUSTOM.replace("format = 1", "format = 2"))
            .unwrap_err()
            .contains("unsupported workflow format")
    );
    assert!(
        Project::parse(
            Path::new("/tmp/demo"),
            &CUSTOM.replace("format = 1", "format = 2")
        )
        .unwrap_err()
        .contains("unsupported workflow format")
    );
}

#[test]
fn project_of_reports_custom_workflow_instead_of_silently_ignoring_it() {
    let tree = tree::Tree::new();
    tree.write("symphony.toml", CUSTOM);
    Project::of(&tree.root);
    assert!(
        Project::problem(&tree.root)
            .unwrap()
            .contains("unsupported_activation")
    );
}

#[test]
fn committed_reader_ignores_dirty_and_staged_configuration() {
    let tree = tree::Tree::new();
    layouts::git(&tree.root, &["init", "-q"]);
    tree.write("symphony.toml", CUSTOM);
    layouts::git(&tree.root, &["add", "symphony.toml"]);
    layouts::git(&tree.root, &["commit", "-qm", "workflow fixture"]);
    let (definition, revision) = committed(&tree.root, "HEAD").unwrap();
    definition.check_revision(&revision).unwrap();
    tree.write("symphony.toml", "[workflow]\nformat = 999");
    layouts::git(&tree.root, &["add", "symphony.toml"]);
    assert_eq!(
        committed(&tree.root, "HEAD").unwrap(),
        (definition, revision.clone())
    );
    layouts::git(&tree.root, &["commit", "-qm", "invalid candidate"]);
    assert!(
        committed(&tree.root, "HEAD")
            .unwrap_err()
            .contains("workflow")
    );
    assert_eq!(
        committed(&tree.root, &revision.source.commit).unwrap().1,
        revision
    );
    assert!(committed(&tree.root, "missing_ref").is_err());
    assert!(committed(&tree.root, "HEAD:symphony.toml").is_err());
}

#[test]
fn committed_reader_refuses_missing_nonregular_and_non_utf8_project_files() {
    let tree = tree::Tree::new();
    assert!(committed(&tree.root, "HEAD").is_err());
    layouts::git(&tree.root, &["init", "-q"]);
    layouts::git(
        &tree.root,
        &["commit", "--allow-empty", "-qm", "empty fixture"],
    );
    assert!(
        committed(&tree.root, "HEAD")
            .unwrap_err()
            .contains("missing")
    );
    std::os::unix::fs::symlink("elsewhere", tree.path("symphony.toml")).unwrap();
    layouts::git(&tree.root, &["add", "symphony.toml"]);
    layouts::git(&tree.root, &["commit", "-qm", "linked fixture"]);
    assert!(
        committed(&tree.root, "HEAD")
            .unwrap_err()
            .contains("regular blob")
    );
    std::fs::remove_file(tree.path("symphony.toml")).unwrap();
    std::fs::write(tree.path("symphony.toml"), [0xff]).unwrap();
    layouts::git(&tree.root, &["add", "symphony.toml"]);
    layouts::git(&tree.root, &["commit", "-qm", "binary fixture"]);
    assert!(committed(&tree.root, "HEAD").unwrap_err().contains("utf-8"));
}
