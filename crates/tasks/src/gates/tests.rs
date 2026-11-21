//! A gate decision as the board keeps and says it, the limit a raise names read from its file, and
//! which waivers lift a stamp at a tip.

use super::*;
use serde_json::json;

const TIP: &str = "1a2b3c4d5e6f7a8b9c0d1a2b3c4d5e6f7a8b9c0d";
const LATER: &str = "9f8e7d6c5b4a39281706f5e4d3c2b1a098765432";

fn raise(gate: &str, from: u64, to: u64, by: &str) -> Decision {
    Decision {
        gate: gate.into(),
        kind: Kind::Raise,
        from: Some(from),
        to: Some(to),
        tip: None,
        by: by.into(),
        why: "measured after the cut".into(),
        at: 0.0,
    }
}

fn waive(gate: &str, tip: &str, by: &str) -> Decision {
    Decision {
        gate: gate.into(),
        kind: Kind::Waive,
        from: None,
        to: None,
        tip: Some(tip.into()),
        by: by.into(),
        why: "nothing a page shows".into(),
        at: 0.0,
    }
}

fn limit(gate: &str) -> Limit {
    Limit::parse(gate).unwrap()
}

#[test]
fn a_decision_says_its_gate_its_values_or_its_tip_who_and_why() {
    let raised = raise(
        "tools/size-budget.toml:symphony",
        12_390_000,
        12_400_000,
        "orch",
    );
    assert_eq!(
        raised.line(),
        "gate tools/size-budget.toml:symphony raised 12,390,000 -> 12,400,000: measured after the \
         cut; by orch"
    );
    let waived = waive("pictures", TIP, "orch");
    assert_eq!(waived.said(), "pictures waived at 1a2b3c4");
    assert_eq!(
        waived.line(),
        "gate pictures waived at 1a2b3c4: nothing a page shows; by orch"
    );
}

#[test]
fn the_board_field_reads_every_decision_and_the_newest_on_a_gate_stands() {
    let first = raise("a.toml:x", 1, 2, "orch");
    let other = waive("pictures", TIP, "orch");
    let second = raise("a.toml:x", 1, 3, "orch");
    let all = vec![first.clone(), other.clone(), second.clone()];
    let mut fields = BTreeMap::new();
    assert!(of(&fields).is_empty(), "no board fields");
    fields.insert(
        crate::red::FIELDS.to_string(),
        BTreeMap::from([(GATES.to_string(), serde_json::to_value(&all).unwrap())]),
    );
    assert_eq!(of(&fields), all);
    assert_eq!(standing(&all), vec![other, second]);
    // A raise keeps no tip and a waiver no numbers, on the wire as in the field.
    assert_eq!(
        serde_json::to_value(&first).unwrap(),
        json!({"gate": "a.toml:x", "kind": "raise", "from": 1, "to": 2, "by": "orch",
               "why": "measured after the cut", "at": 0.0})
    );
    // A field that does not read is none, never a refusal of the task.
    fields.insert(
        crate::red::FIELDS.to_string(),
        BTreeMap::from([(GATES.to_string(), json!("not a list"))]),
    );
    assert!(of(&fields).is_empty());
    // Nor does a decision written as its fields in order, which no writer writes: the list is
    // none, where a reading of a struct from an array would take it for `first`.
    let positional = json!([[
        "a.toml:x",
        "raise",
        1,
        2,
        null,
        "orch",
        "measured after the cut",
        0.0
    ]]);
    assert_eq!(
        serde_json::from_value::<Vec<Decision>>(positional.clone()).unwrap(),
        vec![first]
    );
    fields.insert(
        crate::red::FIELDS.to_string(),
        BTreeMap::from([(GATES.to_string(), positional)]),
    );
    assert!(of(&fields).is_empty());
}

#[test]
fn a_limit_is_named_as_path_and_name() {
    assert_eq!(
        limit("tools/size-budget.toml:symphony-server"),
        Limit {
            path: "tools/size-budget.toml".into(),
            name: "symphony-server".into()
        }
    );
    assert_eq!(
        limit("/a/b.rs:TASKS_FIRST_VISIT_BYTES").gate(),
        "a/b.rs:TASKS_FIRST_VISIT_BYTES"
    );
    for (gate, why) in [
        ("pictures", "has no :"),
        (":symphony", "names no file inside the project"),
        ("tools/:symphony", "names no file inside the project"),
        ("../x.toml:symphony", "names no file inside the project"),
        ("a//b.toml:symphony", "names no file inside the project"),
        ("a.toml:", "names no key or constant"),
        ("a.toml:two words", "names no key or constant"),
        ("a.toml:x.y", "names no key or constant"),
    ] {
        let refused = Limit::parse(gate).unwrap_err();
        assert!(refused.ends_with(why), "{gate}: {refused}");
        assert!(
            refused.starts_with("a raise names its limit as PATH:NAME"),
            "{refused}"
        );
    }
}

#[test]
fn a_limit_reads_the_one_line_that_sets_it_in_toml_or_rust() {
    let budget = "# symphony's line, and symphony = 1 in a comment\n\
                  client = 2_360_000\n\
                  [binaries]\n\
                  symphony-server = 17_430_000\n\
                  symphony = 12_390_000\n\
                  symphony_old = 3\n";
    assert_eq!(limit("b.toml:symphony").read(budget), Ok(12_390_000));
    assert_eq!(limit("b.toml:symphony-server").read(budget), Ok(17_430_000));
    assert_eq!(limit("b.toml:client").read(budget), Ok(2_360_000));
    let rust = "/// TASKS_FIRST_VISIT_BYTES: the board's cap\n\
                const TASKS_FIRST_VISIT_BYTES: f64 = 125_000.0;\n\
                pub const TASK_FIRST_VISIT_BYTES: f64 = 145_000.0;\n\
                    let RETURN = 20,000;\n\
                // const SHOWN: u64 = 5;\n\
                if SHOWN == 3 {}\n";
    assert_eq!(
        limit("m.rs:TASKS_FIRST_VISIT_BYTES").read(rust),
        Ok(125_000)
    );
    assert_eq!(limit("m.rs:TASK_FIRST_VISIT_BYTES").read(rust), Ok(145_000));
    assert_eq!(limit("m.rs:RETURN").read(rust), Ok(20_000));
    assert_eq!(
        limit("m.rs:SHOWN").read(rust),
        Err("no line of m.rs sets SHOWN".into()),
        "a comment and a comparison set nothing"
    );
    assert_eq!(
        limit("m.rs:x").read("x == 3\nx >= 4\n"),
        Err("no line of m.rs sets x".into()),
        "a comparison sets nothing"
    );
    assert_eq!(
        limit("m.rs:x").read("x = 1\n[t]\nx = 2\n"),
        Err("m.rs sets x on 2 lines (1, 3), so no one number is its limit".into())
    );
    for text in ["x = 1.5\n", "x = \"10\"\n", "x = _1\n", "x = 3e4\n"] {
        assert_eq!(
            limit("m.rs:x").read(text),
            Err("m.rs sets x on line 1 to no whole number".into()),
            "{text:?}"
        );
    }
}

#[test]
fn a_number_is_whole_grouped_as_written_and_said_by_threes() {
    for (text, number) in [
        ("150000", 150_000),
        ("150_000", 150_000),
        ("150,000", 150_000),
        ("125_000.0", 125_000),
        ("125000.00", 125_000),
        (" 7 ", 7),
        ("0", 0),
    ] {
        assert_eq!(parse(text), Some(number), "{text:?}");
    }
    for text in [
        "",
        "1.5",
        "1.",
        "_1",
        "1_",
        ",1",
        "1e3",
        "-1",
        "x",
        "99999999999999999999999",
    ] {
        assert_eq!(parse(text), None, "{text:?}");
    }
    for (n, said) in [
        (0, "0"),
        (999, "999"),
        (1_000, "1,000"),
        (12_390_000, "12,390,000"),
        (100_000, "100,000"),
    ] {
        assert_eq!(grouped(n), said);
    }
}

#[test]
fn a_waiver_lifts_its_stamp_at_its_own_tip_alone_and_only_where_the_catalogue_allows() {
    let own = Catalogue::symphony();
    let needed: Vec<String> = ["tests", "review", "pictures"].map(String::from).to_vec();
    let decisions = vec![
        waive("pictures", TIP, "orch"),
        raise("a.toml:x", 1, 2, "orch"),
        // Recorded where no command would take it: the land still lifts nothing for it.
        waive("tests", TIP, "orch"),
        waive("owner", TIP, "orch"),
    ];
    let (here, elsewhere) = waivers(&decisions, &own, &needed, Some(TIP));
    assert_eq!(here, vec![&decisions[0]]);
    assert!(elsewhere.is_empty());
    // At a later tip it lifts nothing, and is named as given for another.
    let (here, elsewhere) = waivers(&decisions, &own, &needed, Some(LATER));
    assert!(here.is_empty());
    assert_eq!(elsewhere, vec![&decisions[0]]);
    // With no tip, nothing is lifted.
    let (here, _) = waivers(&decisions, &own, &needed, None);
    assert!(here.is_empty());
    // A stamp not needed has nothing to lift.
    let (here, elsewhere) = waivers(&decisions, &own, &needed[..2], Some(TIP));
    assert!(here.is_empty() && elsewhere.is_empty());
    // A catalogue that does not mark pictures waivable lifts none.
    let strict =
        Catalogue::parse("[stamps.review]\n[stamps.pictures]\nwaivable = false\n").unwrap();
    let (here, elsewhere) = waivers(&decisions, &strict, &needed, Some(TIP));
    assert!(here.is_empty() && elsewhere.is_empty());
}
