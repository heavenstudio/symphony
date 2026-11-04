use symphony_args::{Flag, command_at, parse};

const FLAGS: &[Flag] = &[
    Flag {
        name: "--root",
        value: true,
    },
    Flag {
        name: "--raw",
        value: false,
    },
];

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn values_keep_repetitions_and_last_value_while_options_allow_unique_prefixes() {
    let parsed = parse(
        &args(&["first", "--roo=a", "--root", "b", "--raw", "last"]),
        FLAGS,
    )
    .unwrap();
    assert_eq!(parsed.values("--root").collect::<Vec<_>>(), ["a", "b"]);
    assert_eq!(parsed.value("--root"), Some("b"));
    assert_eq!(parsed.value("--missing"), None);
    assert!(parsed.flag("--raw"));
    assert!(!parsed.flag("--missing"));
    assert_eq!(parsed.positionals, ["first", "last"]);
    assert!(!parsed.help);
}

#[test]
fn end_of_options_help_negative_numbers_and_empty_values_keep_their_meaning() {
    let parsed = parse(
        &args(&["-h", "--help", "--root=", "-2", "-a b", "--", "--raw", "-h"]),
        FLAGS,
    )
    .unwrap();
    assert!(parsed.help);
    assert_eq!(parsed.value("--root"), Some(""));
    assert_eq!(parsed.positionals, ["-2", "-a b", "--raw", "-h"]);
    assert!(!parsed.flag("--raw"));
    let parsed = parse(&args(&["--root", "-2"]), FLAGS).unwrap();
    assert_eq!(parsed.value("--root"), Some("-2"));
}

#[test]
fn malformed_options_keep_their_refusals() {
    for (input, expected) in [
        (vec!["--root"], "argument --root: expected one argument"),
        (
            vec!["--root", "--raw"],
            "argument --root: expected one argument",
        ),
        (
            vec!["--r"],
            "ambiguous option: --r could match --root, --raw",
        ),
        (
            vec!["--raw=yes"],
            "argument --raw: ignored explicit argument \"yes\"",
        ),
        (
            vec!["-z", "--missing=2"],
            "unrecognized arguments: -z --missing=2",
        ),
        (
            vec!["--help", "--root"],
            "argument --root: expected one argument",
        ),
    ] {
        assert_eq!(
            parse(&args(&input), FLAGS).err().as_deref(),
            Some(expected),
            "{input:?}"
        );
    }
}

#[test]
fn the_command_follows_options_and_their_values_without_consuming_positionals() {
    for (input, expected) in [
        (vec![], 0),
        (vec!["status"], 0),
        (vec!["--root", "folder", "status"], 2),
        (vec!["--roo=folder", "status"], 1),
        (vec!["--roo", "folder", "status"], 2),
        (vec!["--raw", "status"], 1),
        (vec!["--unknown", "status"], 1),
        (vec!["--root", "folder"], 2),
        (vec!["--root"], 1),
        (vec!["-2", "status"], 0),
    ] {
        assert_eq!(command_at(&args(&input), FLAGS), expected, "{input:?}");
    }
}
