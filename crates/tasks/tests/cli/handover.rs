//! One orchestrator a task from the command line: the refusal, the hand-over, and the listing of
//! tasks that still hold two.

use crate::Case;
use serde_json::json;

#[test]
fn a_taken_task_refuses_a_second_orchestrator_and_hands_over_in_one_command() {
    let c = Case::new();
    let (a, b) = (c.register("orchestrator"), c.register("orchestrator"));
    c.ok(&a, &["create", "scratch/one", "One"]);
    c.ok(&a, &["role", "scratch/one", "orchestrator"]);
    let ran = c.run_as(&b, &["role", "scratch/one", "orchestrator"], "");
    assert_eq!(ran.code, 1);
    assert!(
        ran.err.contains(&format!(
            "scratch/one is orchestrated by {a} (live); {a} hands it over with: {} handover \
             scratch/one {b}",
            c.board()
        )),
        "{}",
        ran.err
    );
    assert_eq!(
        c.ok(&a, &["handover", "scratch/one", &b]),
        format!("handed scratch/one to {b} (orchestrator)\n")
    );
    assert_eq!(
        c.store()
            .get("scratch/one")
            .unwrap()
            .holders("orchestrator"),
        [b.as_str()]
    );
    let again = c.run_as(&a, &["handover", "scratch/one", &b], "");
    assert_eq!(again.code, 1);
    assert!(
        again
            .err
            .contains(&format!("{a} does not orchestrate scratch/one")),
        "{}",
        again.err
    );
}

#[test]
fn the_doubled_listing_names_each_task_two_orchestrators_hold() {
    let c = Case::new();
    let (a, b) = (c.register("orchestrator"), c.register("orchestrator"));
    for slug in ["scratch/double", "scratch/single"] {
        c.ok(&a, &["create", slug, "Work"]);
        c.ok(&a, &["role", slug, "orchestrator"]);
    }
    // Seeded as the board held them before the rule: a second row written beside the first.
    c.store()
        .client()
        .query(
            "CREATE type::record('role', ['scratch/double', $h, 'orchestrator'])
                CONTENT { slug: 'scratch/double', handle: $h, role: 'orchestrator', since: 0 } RETURN NONE;",
            json!({"h": b}),
        )
        .unwrap();
    let out = c.ok(&a, &["list", "--doubled"]);
    assert_eq!(out.lines().count(), 1, "{out}");
    assert!(out.contains("scratch/double"), "{out}");
    assert!(out.contains(&a) && out.contains(&b), "{out}");
    let listing = c.ok(&a, &["list"]);
    assert!(listing.contains("scratch/single"), "{listing}");
    c.ok(&b, &["handover", "scratch/double", &a]);
    assert_eq!(
        c.ok(&a, &["list", "--doubled"]),
        "no task holds two orchestrators\n"
    );
}
